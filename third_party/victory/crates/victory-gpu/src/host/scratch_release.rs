//! The pressure sweep hands off only the pool it removed under the spare
//! lock. Recorded and in-flight lists still own all of their scratch.

use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

use super::{ALIVE, Chunk, Spare};

type Worker<T> = OnceLock<Option<SyncSender<T>>>;

pub(super) fn release(spare: Spare<Chunk>) {
    static ON: OnceLock<bool> = OnceLock::new();
    static WORKER: Worker<Spare<Chunk>> = OnceLock::new();
    let beside =
        *ON.get_or_init(|| std::env::var_os("VICTORY_LET_GO_BESIDE").is_none_or(|v| v != "0"));
    release_with(spare, beside, &WORKER, |spare| {
        let count = spare.kept.len() as u64;
        let bytes = spare.bytes;
        drop(spare);
        ALIVE[1].fetch_sub(count, Ordering::Relaxed);
        ALIVE[2].fetch_sub(bytes, Ordering::Relaxed);
    });
}

fn release_with<T: Send + 'static>(batch: T, beside: bool, worker: &Worker<T>, free: fn(T)) {
    if !beside {
        return free(batch);
    }
    let worker = worker.get_or_init(|| {
        // At most one sweep waiting behind the one being freed. Under
        // pressure a slow driver must not build up a backlog of buffers.
        let (send, batches) = sync_channel(1);
        match std::thread::Builder::new().name("victory-let-go".into()).spawn(move || {
            batches.into_iter().for_each(free);
        }) {
            Ok(_) => Some(send),
            Err(error) => {
                tracing::warn!(%error, "releasing spare scratch on the caller: could not start victory-let-go");
                None
            }
        }
    });
    match worker {
        Some(send) => match send.try_send(batch) {
            Ok(()) => {}
            Err(TrySendError::Full(batch) | TrySendError::Disconnected(batch)) => free(batch),
        },
        None => free(batch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{Sender, channel};
    use std::thread::{ThreadId, current};
    use std::time::Duration;

    struct Probe(Sender<(u8, ThreadId)>, u8);

    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.send((self.1, current().id())).unwrap();
        }
    }

    #[test]
    fn only_idle_chunks_are_released_beside_the_caller() {
        let (send, dropped) = channel();
        let mut spare = Spare::new();
        for id in 0..3 {
            assert!(spare.give(Probe(send.clone(), id), 4, 12).is_none());
        }
        let in_use = spare.take(4).unwrap();
        let worker = Worker::new();
        release_with(spare, true, &worker, drop);
        let (first, thread) = dropped.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(first, 0);
        assert_ne!(thread, current().id());
        assert_eq!(
            dropped.recv_timeout(Duration::from_secs(5)).unwrap(),
            (1, thread)
        );
        assert!(dropped.try_recv().is_err());
        drop(in_use);
        assert_eq!(dropped.try_recv().unwrap(), (2, current().id()));
    }

    #[test]
    fn disabled_busy_and_disconnected_workers_release_on_the_caller() {
        let (send, dropped) = channel();
        let worker = Worker::new();
        release_with(Probe(send.clone(), 0), false, &worker, drop);
        assert!(worker.get().is_none());
        let (queue, receiver) = sync_channel(1);
        worker.set(Some(queue)).unwrap();
        release_with(Probe(send.clone(), 1), true, &worker, drop);
        release_with(Probe(send.clone(), 2), true, &worker, drop);
        assert_eq!(dropped.try_recv().unwrap(), (0, current().id()));
        assert_eq!(dropped.try_recv().unwrap(), (2, current().id()));
        drop(receiver);
        release_with(Probe(send, 3), true, &worker, drop);
        assert_eq!(dropped.try_recv().unwrap(), (1, current().id()));
        assert_eq!(dropped.try_recv().unwrap(), (3, current().id()));
    }
}
