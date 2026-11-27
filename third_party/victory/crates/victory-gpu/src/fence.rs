//! Fences: a 64-bit value and the events waiting for it. Nothing runs on a
//! GPU yet, so only signals (the title's, later our queues') move one.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::object::{self, E_NOTIMPL, FENCE, HRESULT, Iid, Object, S_OK, Table, This, guid};
use crate::resource::PAGEABLE_IIDS;

pub(crate) const FENCE_IIDS: [Iid; 5] = [
    guid(
        0x0a753dcf,
        0xc4d8,
        0x4b91,
        [0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76],
    ),
    // ID3D12Fence1.
    guid(
        0x433685fe,
        0xe22b,
        0x4ca0,
        [0xa8, 0xdb, 0xb5, 0xb4, 0xf4, 0xdd, 0x0e, 0x4a],
    ),
    PAGEABLE_IIDS[0],
    PAGEABLE_IIDS[1],
    PAGEABLE_IIDS[2],
];

#[repr(C)]
struct Fence {
    object: Object,
    /// Titles also read it straight from memory, at the fence's GPU
    /// address.
    value: AtomicU64,
    flags: u32,
    /// Events to set once the value reaches theirs (handles as integers).
    waiters: Mutex<Vec<(u64, usize)>>,
}

fn fence<'a>(this: This) -> &'a Fence {
    // SAFETY: called from fence methods only.
    unsafe { &*this.cast::<Fence>() }
}

/// Keeps a fence alive until the queue finishes using it.
#[cfg(windows)]
#[derive(PartialEq, Eq)]
pub(crate) struct FenceRef(usize);

#[cfg(windows)]
impl FenceRef {
    pub(crate) fn hold(this: This) -> Self {
        object::add_ref(this);
        Self(this as usize)
    }

    pub(crate) fn as_this(&self) -> This {
        self.0 as This
    }
}

#[cfg(windows)]
impl Clone for FenceRef {
    fn clone(&self) -> Self {
        Self::hold(self.as_this())
    }
}

#[cfg(windows)]
impl Drop for FenceRef {
    fn drop(&mut self) {
        release(self.as_this());
    }
}

pub(crate) extern "system" fn create(
    _: This,
    initial: u64,
    flags: u32,
    iid: *const Iid,
    out: *mut This,
) -> HRESULT {
    tracing::debug!(initial, flags, "fence");
    let fence = Box::new(Fence {
        object: Object::new(&FUNCTIONS),
        value: AtomicU64::new(initial),
        flags,
        waiters: Mutex::new(Vec::new()),
    });
    object::hand_out(FENCE, fence, iid, &FENCE_IIDS, out)
}

fn set_event(event: usize) {
    #[cfg(windows)]
    // SAFETY: the title passed an event handle.
    unsafe {
        windows_sys::Win32::System::Threading::SetEvent(event as *mut c_void)
    };
    #[cfg(not(windows))]
    let _ = event;
}

extern "system" fn query(this: This, iid: *const Iid, out: *mut This) -> HRESULT {
    object::answer(
        FENCE,
        iid,
        object::is(iid, &FENCE_IIDS).then_some(this),
        out,
    )
}
extern "system" fn release(this: This) -> u32 {
    object::release_boxed::<Fence>(this)
}
pub(crate) extern "system" fn completed(this: This) -> u64 {
    fence(this).value.load(Ordering::Acquire)
}
/// The title's `GetCompletedValue`.
extern "system" fn completed_called(this: This) -> u64 {
    let value = completed(this);
    #[cfg(all(windows, feature = "diagnostics"))]
    crate::timeline::polled(this as usize, value);
    value
}
/// Maximum time a blocking null-event fence wait will wait before giving up
/// instead of hanging the title thread forever.
#[cfg(windows)]
const NULL_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[cfg(windows)]
fn wait_null_event(
    fence: &Fence,
    value: u64,
    mut waiters: std::sync::MutexGuard<'_, Vec<(u64, usize)>>,
    timeout: std::time::Duration,
) -> HRESULT {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    // SAFETY: a private, initially unsignaled event.
    let handle = match unsafe { CreateEventW(None, false, false, None) } {
        Ok(handle) => handle,
        Err(error) => return error.code().0,
    };
    waiters.push((value, handle.0 as usize));
    drop(waiters);
    // A queued signal may follow work that has not been submitted or settled yet.
    crate::queue::before_sync();
    crate::host::settle_for_fence();
    let deadline = std::time::Instant::now() + timeout;
    let result = loop {
        // A direct write to the exposed fence word or settle_for_fence() may already have reached value.
        if fence.value.load(Ordering::Acquire) >= value {
            break S_OK;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            tracing::warn!(value, "blocking fence wait timed out");
            break E_NOTIMPL;
        }
        let ms = (deadline - now).as_millis().min(10) as u32;
        // SAFETY: this call owns the event until it unregisters the waiter.
        match unsafe { WaitForSingleObject(handle, ms.max(1)) } {
            WAIT_OBJECT_0 => break S_OK,
            WAIT_TIMEOUT => {
                if fence.value.load(Ordering::Acquire) >= value {
                    break S_OK;
                }
                crate::queue::release_held(false);
            }
            _ => break windows::core::Error::from_thread().code().0,
        }
    };
    let mut waiters = fence.waiters.lock().unwrap_or_else(|e| e.into_inner());
    waiters.retain(|&(_, pending)| pending != handle.0 as usize);
    // SAFETY: no signal can still use this event after unregistering it.
    let _ = unsafe { CloseHandle(handle) };
    result
}

extern "system" fn set_event_on_completion(this: This, value: u64, event: *mut c_void) -> HRESULT {
    let fence = fence(this);
    let mut waiters = fence.waiters.lock().unwrap_or_else(|e| e.into_inner());
    if fence.value.load(Ordering::Acquire) >= value {
        if !event.is_null() {
            set_event(event as usize);
        }
        return S_OK;
    }
    #[cfg(all(windows, feature = "diagnostics"))]
    crate::timeline::told("an event asked for", this as usize, value);
    if event.is_null() {
        #[cfg(windows)]
        return wait_null_event(fence, value, waiters, NULL_EVENT_TIMEOUT);
        #[cfg(not(windows))]
        return E_NOTIMPL;
    }
    waiters.push((value, event as usize));
    S_OK
}
/// Moves the fence to `value` and sets the events that waited for it.
pub(crate) fn set(this: This, value: u64) {
    let fence = fence(this);
    #[cfg(all(windows, feature = "diagnostics"))]
    crate::timeline::told("a fence set", this as usize, value);
    let mut waiters = fence.waiters.lock().unwrap_or_else(|e| e.into_inner());
    fence.value.store(value, Ordering::Release);
    waiters.retain(|&(wanted, event)| {
        let reached = wanted <= value;
        if reached {
            set_event(event);
        }
        !reached
    });
    drop(waiters);
    #[cfg(windows)]
    // SAFETY: wake any guest thread polling/blocking via WaitOnAddress on the fence's GPU VA.
    unsafe {
        windows_sys::Win32::System::Threading::WakeByAddressAll((&raw const fence.value).cast());
    }
}
pub(crate) extern "system" fn signal(this: This, value: u64) -> HRESULT {
    let _called = crate::timeline::span("a fence set");
    set(this, value);
    // A queue may have waited for it: what it held runs now.
    #[cfg(windows)]
    crate::queue::release_held(false);
    S_OK
}

extern "system" fn creation_flags(this: This) -> u32 {
    fence(this).flags
}
/// Where the value lives (GPU addresses are CPU addresses).
extern "system" fn gpu_address(this: This) -> u64 {
    (&raw const fence(this).value) as u64
}

/// IUnknown 3, object 4, device child 1, fence 3, fence 1 (2, a console
/// one among them).
static FUNCTIONS: Table<24> = {
    let mut f = object::d3d12_object(object::missing_table::<FENCE, 24>());
    f[0] = query as *const ();
    f[2] = release as *const ();
    f[7] = object::parent_device as *const (); // 0x38
    f[8] = completed_called as *const (); // 0x40
    f[9] = set_event_on_completion as *const (); // 0x48
    f[10] = signal as *const (); // 0x50
    f[11] = creation_flags as *const (); // 0x58
    f[12] = gpu_address as *const (); // 0x60
    Table(f)
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::slot;

    #[cfg(windows)]
    #[test]
    fn queued_references_keep_the_fence_alive_after_release() {
        let mut f = std::ptr::null();
        assert_eq!(create(std::ptr::null(), 1, 0, &FENCE_IIDS[0], &mut f), S_OK);
        let queued = FenceRef::hold(f);
        let waiting = queued.clone();
        assert_eq!(release(f), 2);
        drop(waiting);
        set(queued.as_this(), 5);
        assert_eq!(completed(queued.as_this()), 5);
    }

    #[cfg(windows)]
    #[test]
    fn signals_set_waiting_events() {
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

        let mut f = std::ptr::null();
        assert_eq!(create(std::ptr::null(), 1, 0, &FENCE_IIDS[0], &mut f), S_OK);
        assert_eq!(slot::<extern "system" fn(This) -> u64>(f, 0x40)(f), 1);
        // SAFETY: plain auto-reset event.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        assert!(!event.is_null());
        let wait = slot::<extern "system" fn(This, u64, *mut c_void) -> HRESULT>(f, 0x48);
        let signal = slot::<extern "system" fn(This, u64) -> HRESULT>(f, 0x50);
        assert_eq!(wait(f, 5, event), S_OK);
        // SAFETY: a valid event.
        assert_ne!(unsafe { WaitForSingleObject(event, 0) }, WAIT_OBJECT_0);
        assert_eq!(signal(f, 5), S_OK);
        // SAFETY: as above.
        assert_eq!(unsafe { WaitForSingleObject(event, 0) }, WAIT_OBJECT_0);
        assert_eq!(wait(f, 5, event), S_OK, "already reached");
        // SAFETY: as above.
        assert_eq!(unsafe { WaitForSingleObject(event, 0) }, WAIT_OBJECT_0);
        assert_eq!(wait(f, 5, std::ptr::null_mut()), S_OK);
        // The title reads the value at the fence's address.
        let at = slot::<extern "system" fn(This) -> u64>(f, 0x60)(f);
        // SAFETY: the fence's value.
        assert_eq!(unsafe { *(at as *const u64) }, 5);
        // SAFETY: ours.
        unsafe { CloseHandle(event) };
        assert_eq!(slot::<extern "system" fn(This) -> u32>(f, 0x10)(f), 0);
    }

    #[test]
    fn fence1_interface_is_created_and_queried() {
        const FENCE1_IID: Iid = guid(
            0x433685fe,
            0xe22b,
            0x4ca0,
            [0xa8, 0xdb, 0xb5, 0xb4, 0xf4, 0xdd, 0x0e, 0x4a],
        );
        type Query = extern "system" fn(This, *const Iid, *mut This) -> HRESULT;
        type Flags = extern "system" fn(This) -> u32;
        type Release = extern "system" fn(This) -> u32;

        // Direct creation as ID3D12Fence1, and GetCreationFlags at slot 0x58.
        let mut f1 = std::ptr::null();
        assert_eq!(create(std::ptr::null(), 3, 0x5, &FENCE1_IID, &mut f1), S_OK);
        assert!(!f1.is_null());
        assert_eq!(slot::<Flags>(f1, 0x58)(f1), 0x5);
        assert_eq!(slot::<Release>(f1, 0x10)(f1), 0);

        // QueryInterface from ID3D12Fence to ID3D12Fence1 returns the same object.
        let mut f = std::ptr::null();
        assert_eq!(
            create(std::ptr::null(), 0, 0x2, &FENCE_IIDS[0], &mut f),
            S_OK
        );
        let mut queried = std::ptr::null();
        assert_eq!(slot::<Query>(f, 0x00)(f, &FENCE1_IID, &mut queried), S_OK);
        assert_eq!(queried, f);
        assert_eq!(slot::<Flags>(queried, 0x58)(queried), 0x2);
        assert_eq!(slot::<Release>(queried, 0x10)(queried), 1);
        assert_eq!(slot::<Release>(f, 0x10)(f), 0);
    }

    #[cfg(windows)]
    #[test]
    fn null_event_times_out_and_unregisters_waiter() {
        let mut f = std::ptr::null();
        assert_eq!(create(std::ptr::null(), 1, 0, &FENCE_IIDS[0], &mut f), S_OK);
        let fence_obj = fence(f);
        let waiters = fence_obj.waiters.lock().unwrap();
        let hr = wait_null_event(
            fence_obj,
            9,
            waiters,
            std::time::Duration::from_millis(30),
        );
        assert_eq!(hr, E_NOTIMPL, "unreached blocking wait must time out");
        assert!(
            fence_obj.waiters.lock().unwrap().is_empty(),
            "timed-out waiter must be unregistered"
        );
        assert_eq!(slot::<extern "system" fn(This) -> u32>(f, 0x10)(f), 0);
    }

    #[cfg(windows)]
    #[test]
    fn null_event_waits_for_a_queued_gpu_copy() {
        use std::ptr::null;
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        use windows::Win32::Graphics::Direct3D12::{
            D3D12_RANGE, ID3D12CommandAllocator, ID3D12CommandQueue, ID3D12GraphicsCommandList,
        };
        use windows::core::Interface;

        let Some(host) = crate::host::host() else {
            eprintln!("skipped queued fence test: no D3D12 device");
            return;
        };
        const MARKER: u32 = 0x1357_9bdf;
        const TARGET: u64 = 1;
        let bytes = size_of::<u32>() as u64;
        let (upload, data) = host.upload_buffer(bytes).unwrap();
        let output = host.read_back_buffer(bytes).unwrap();
        // SAFETY: the upload mapping holds one word.
        unsafe { data.cast::<u32>().write(MARKER) };

        fn iid<T: Interface>() -> Iid {
            let i = T::IID;
            guid(i.data1, i.data2, i.data3, i.data4)
        }
        let (mut allocator, mut list, mut queue, mut gate, mut done) =
            (null(), null(), null(), null(), null());
        assert_eq!(
            crate::list::create_allocator(
                null(),
                0,
                &iid::<ID3D12CommandAllocator>(),
                &mut allocator
            ),
            S_OK
        );
        assert_eq!(
            crate::list::create_list(
                null(),
                0,
                0,
                allocator,
                null(),
                &iid::<ID3D12GraphicsCommandList>(),
                &mut list
            ),
            S_OK
        );
        let queue_desc = [0u32; 4];
        assert_eq!(
            crate::queue::create(
                null(),
                queue_desc.as_ptr().cast(),
                &iid::<ID3D12CommandQueue>(),
                &mut queue
            ),
            S_OK
        );
        assert_eq!(create(null(), 0, 0, &FENCE_IIDS[0], &mut gate), S_OK);
        assert_eq!(create(null(), 0, 0, &FENCE_IIDS[0], &mut done), S_OK);

        let host_list = crate::list::host_list(list).unwrap();
        // SAFETY: an open list and live upload/readback buffers of the given size.
        unsafe {
            host_list
                .list
                .CopyBufferRegion(&output, 0, &upload, 0, bytes)
        };
        assert_eq!(
            slot::<extern "system" fn(This) -> HRESULT>(list, 0x48)(list),
            S_OK
        );
        let signal_offset = if crate::device::newer() { 0x80 } else { 0x70 };
        type QueueFence = extern "system" fn(This, This, u64) -> HRESULT;
        assert_eq!(
            slot::<QueueFence>(queue, signal_offset + 8)(queue, gate, TARGET),
            S_OK
        );
        slot::<extern "system" fn(This, u32, *const This)>(queue, 0x50)(queue, 1, &list);
        assert_eq!(
            slot::<QueueFence>(queue, signal_offset)(queue, done, TARGET),
            S_OK
        );
        assert_eq!(completed(done), 0, "the gate must hold the queue");

        let (sent, result) = mpsc::channel();
        let (registered, early, outcome) =
            std::thread::scope(|scope| {
                let address = done as usize;
                scope.spawn(move || {
                    let done = address as This;
                    let hr = slot::<extern "system" fn(This, u64, *mut c_void) -> HRESULT>(
                        done, 0x48,
                    )(done, TARGET, std::ptr::null_mut());
                    sent.send((hr, completed(done))).unwrap();
                });
                // Wait for registration or an early return before opening the gate.
                let deadline = Instant::now() + Duration::from_secs(5);
                let (registered, early) = loop {
                    if let Ok(outcome) = result.try_recv() {
                        break (false, Some(outcome));
                    }
                    if fence(done)
                        .waiters
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|w| w.0 == TARGET)
                    {
                        break (true, result.try_recv().ok());
                    }
                    if Instant::now() >= deadline {
                        break (false, None);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                };
                signal(gate, TARGET);
                let outcome = early.or_else(|| result.recv_timeout(Duration::from_secs(5)).ok());
                if outcome.is_none() {
                    // Release a pending test thread before reporting a timeout.
                    set(done, TARGET);
                }
                (registered, early, outcome)
            });
        crate::host::settle_for_fence();
        let mut mapped = std::ptr::null_mut();
        // SAFETY: the queue's list finished, and the readback holds one word.
        let gpu = unsafe {
            output.Map(0, None, Some(&mut mapped)).unwrap();
            let word = mapped.cast::<u32>().read();
            output.Unmap(0, Some(&D3D12_RANGE::default()));
            word
        };
        for object in [queue, list, allocator, gate, done] {
            slot::<extern "system" fn(This) -> u32>(object, 0x10)(object);
        }
        let (hr, completed) =
            outcome.expect("null-event waiter did not finish after the queue signal");
        println!(
            "late={}, registered={registered}, returned_before_signal={}, HRESULT={:#010x}, fence={completed}, GPU={gpu:#010x}",
            crate::host::late(),
            early.is_some(),
            hr as u32,
        );
        assert!(
            registered && early.is_none(),
            "null-event wait returned before the signal: HRESULT {:#010x}",
            hr as u32
        );
        assert_eq!((hr, completed, gpu), (S_OK, TARGET, MARKER));
    }
}
