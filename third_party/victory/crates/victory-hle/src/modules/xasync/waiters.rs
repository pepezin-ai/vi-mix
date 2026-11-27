//! A Win32 thread-pool wait is one-shot. Dispatch the callback on the selected
//! queue port, then rearm only if the registration still belongs to this queue.
use super::{
    Callback, E_ABORT, E_INVALIDARG, HRESULT, NEXT_TOKEN, Port, Queue, S_OK, Task, This, lock,
    queue,
};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Weak};
use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::{
    CloseThreadpoolWait, CreateThreadpoolWait, DisassociateCurrentThreadFromCallback,
    PTP_CALLBACK_INSTANCE, PTP_WAIT, SetThreadpoolWait, WaitForThreadpoolWaitCallbacks,
};

const WAITING: u8 = 0;
const QUEUED: u8 = 1;
const RUNNING: u8 = 2;
const TERMINATED: u8 = 3;
const MAX_PER_PORT: usize = 60; // libHttpClient Task's PORT_WAIT_MAX
const E_OUTOFMEMORY: HRESULT = 0x8007_000e_u32 as i32;
const E_POINTER: HRESULT = 0x8000_4003_u32 as i32;

pub(super) struct Waiter {
    queue: Weak<Queue>,
    port: u32,
    token: u64,
    callback: Callback,
    context: usize,
    phase: AtomicU8,
}

impl Waiter {
    pub(super) fn belongs_to(&self, queue: &Arc<Queue>) -> bool {
        self.queue.as_ptr() == Arc::as_ptr(queue)
    }

    pub(super) fn dispatch(&self, canceled: bool) {
        if self
            .phase
            .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return; // termination already claimed this callback
        }
        (self.callback)(self.context as *mut c_void, canceled);
        if let Some(queue) = self.queue.upgrade() {
            let registrations = lock(&queue.waiters);
            if let Some(reg) = registrations.get(&self.token) {
                self.phase.store(WAITING, Ordering::Release);
                reg.arm();
            }
        }
    }
}

pub(super) struct Registration {
    wait: PTP_WAIT,
    event: usize,
    state: Arc<Waiter>,
    port: Arc<Port>,
}

impl Registration {
    fn arm(&self) {
        // SAFETY: while registered the title owns this handle and the wait is open.
        unsafe { SetThreadpoolWait(self.wait, self.event as HANDLE, null()) };
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        // SAFETY: removed from the queue before drop, so no dispatcher can rearm it.
        // Wait for callbacks to own their context before releasing our last reference.
        unsafe {
            SetThreadpoolWait(self.wait, null_mut(), null());
            WaitForThreadpoolWaitCallbacks(self.wait, 1);
            CloseThreadpoolWait(self.wait);
        }
        self.port.waiter_count.fetch_sub(1, Ordering::AcqRel);
    }
}

unsafe extern "system" fn signaled(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _: PTP_WAIT,
    _: u32,
) {
    let ptr = context.cast::<Waiter>();
    // SAFETY: Registration::drop waits until this callback has cloned the Arc.
    let state = unsafe {
        Arc::increment_strong_count(ptr);
        let state = Arc::from_raw(ptr);
        // An immediate-port callback can unregister itself or release the last queue handle.
        DisassociateCurrentThreadFromCallback(instance);
        state
    };
    if let Some(queue) = state.queue.upgrade() {
        let registrations = lock(&queue.waiters);
        if !registrations.contains_key(&state.token) {
            return;
        }
        state.phase.store(QUEUED, Ordering::Release);
        drop(registrations);
        queue.deliver(state.port, Task::Wait(state));
    }
}

pub(super) extern "system" fn register(
    _: This,
    handle: *const Queue,
    port: u32,
    event: HANDLE,
    context: *mut c_void,
    callback: Option<Callback>,
    token: *mut u64,
) -> HRESULT {
    let Some(queue) = queue(handle) else {
        return E_INVALIDARG;
    };
    let Some(callback) = callback else {
        return E_POINTER;
    };
    if token.is_null() {
        return E_POINTER;
    }
    let Some(target) = queue.ports.get(port as usize) else {
        return E_INVALIDARG;
    };
    if event.is_null() {
        return E_INVALIDARG;
    }
    let mut registrations = lock(&queue.waiters);
    if queue.terminated.load(Ordering::Acquire) {
        return E_ABORT;
    }
    if target
        .waiter_count
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < MAX_PER_PORT).then_some(n + 1)
        })
        .is_err()
    {
        return E_OUTOFMEMORY;
    }
    let id = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let state = Arc::new(Waiter {
        queue: Arc::downgrade(&queue),
        port,
        token: id,
        callback,
        context: context as usize,
        phase: AtomicU8::new(WAITING),
    });
    // SAFETY: initially disarmed. The registration owns the context until Drop drains the wait.
    let wait = unsafe {
        CreateThreadpoolWait(
            Some(signaled),
            Arc::as_ptr(&state).cast_mut().cast(),
            null(),
        )
    };
    if wait == 0 {
        let error = unsafe { GetLastError() };
        target.waiter_count.fetch_sub(1, Ordering::AcqRel);
        return (0x8007_0000 | (error & 0xffff)) as i32;
    }
    let registration = Registration {
        wait,
        event: event as usize,
        state,
        port: target.clone(),
    };
    // SAFETY: token points to a writable GDK registration token.
    unsafe { token.write(id) };
    registration.arm();
    registrations.insert(id, registration);
    S_OK
}

pub(super) extern "system" fn unregister(_: This, handle: *const Queue, token: u64) {
    if let Some(queue) = queue(handle) {
        let registration = lock(&queue.waiters).remove(&token);
        drop(registration);
    }
}

pub(super) fn terminate(queue: &Queue) {
    let registrations = std::mem::take(&mut *lock(&queue.waiters));
    for reg in registrations.into_values() {
        let state = Arc::clone(&reg.state);
        drop(reg);
        if state.phase.swap(TERMINATED, Ordering::AcqRel) != RUNNING {
            (state.callback)(state.context as *mut c_void, true);
        }
    }
}
