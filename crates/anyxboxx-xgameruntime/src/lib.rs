//! xgameruntime.dll in Rust: the GDK runtime as the title sees it. Port of `runtime/xgameruntime`
//! (C++), with the same exports (`xgameruntime.def`) and the same behaviour, checked by running
//! the pytest files that test the C++ DLL against this one (`tests/test_rust_dlls.py`).
//!
//! The title does not link the runtime directly: `xgameruntime.lib` loads this DLL, calls the
//! `InitializeApiImpl*` functions, then asks each interface through `QueryApiImpl`.
//!
//! The two GDK tables are shared with the C++ oracle. VIctory supplies the task queue.
//! The remaining objects, registry, DXGI hooks and HDR normalization are implemented here.

#![allow(non_snake_case)]

mod dxgi_factory;
mod entry;
mod events;
mod frame_counter;
mod game_config;
mod gamer_picture;
mod guid;
mod handle;
mod hdr;
mod hdr_display;
mod local_storage;
mod log;
mod networking;
mod object;
mod package;
mod package_enumerate;
mod package_monitor;
mod package_mount;
mod registry;
mod save_async;
mod save_blobs;
mod save_interface;
mod save_store;
mod served;
mod store;
mod system;
mod system_interface;
mod tables;
mod user;
mod user_async;

use std::ffi::{c_char, c_void};

use guid::Guid;
use object::HResult;

/// Title-side initialisers: forwarded to Windows' runtime when present, otherwise `S_OK`.
///
/// # Safety
/// The title passes its own arguments; they are forwarded unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn InitializeApiImpl(
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
    e: *mut c_void,
    f: *mut c_void,
    g: *mut c_void,
    h: *mut c_void,
) -> HResult {
    entry::initialize(c"InitializeApiImpl", [a, b, c, d, e, f, g, h])
}

/// # Safety
/// As [`InitializeApiImpl`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn InitializeApiImplEx(
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
    e: *mut c_void,
    f: *mut c_void,
    g: *mut c_void,
    h: *mut c_void,
) -> HResult {
    entry::initialize(c"InitializeApiImplEx", [a, b, c, d, e, f, g, h])
}

/// # Safety
/// As [`InitializeApiImpl`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn InitializeApiImplEx2(
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
    e: *mut c_void,
    f: *mut c_void,
    g: *mut c_void,
    h: *mut c_void,
) -> HResult {
    entry::initialize(c"InitializeApiImplEx2", [a, b, c, d, e, f, g, h])
}

/// # Safety
/// As [`InitializeApiImpl`].
#[unsafe(no_mangle)]
pub unsafe extern "system" fn UninitializeApiImpl(
    a: *mut c_void,
    b: *mut c_void,
    c: *mut c_void,
    d: *mut c_void,
    e: *mut c_void,
    f: *mut c_void,
    g: *mut c_void,
    h: *mut c_void,
) -> HResult {
    entry::initialize(c"UninitializeApiImpl", [a, b, c, d, e, f, g, h])
}

/// Returns the object that serves `(provider, iid)`; `E_NOINTERFACE` (output null) otherwise.
///
/// # Safety
/// `provider` and `iid` are null or point at GUIDs; `out` is null or points at one pointer slot.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn QueryApiImpl(
    provider: *const Guid,
    iid: *const Guid,
    out: *mut *mut c_void,
    reserved: *mut c_void,
) -> HResult {
    // SAFETY: forwarded to the caller's contract above.
    unsafe { entry::query(provider, iid, out, reserved) }
}

/// # Safety
/// `message` is null or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn XErrorReport(hr: HResult, message: *const c_char) {
    // SAFETY: forwarded to the caller's contract above.
    unsafe { entry::error_report(hr, message) }
}

#[unsafe(no_mangle)]
pub extern "system" fn XGameRuntimeInitialize() -> HResult {
    object::S_OK
}

#[unsafe(no_mangle)]
pub extern "system" fn XGameRuntimeUninitialize() {}

#[unsafe(no_mangle)]
pub extern "system" fn XSystemGetDeviceType() -> i32 {
    entry::device_type()
}
