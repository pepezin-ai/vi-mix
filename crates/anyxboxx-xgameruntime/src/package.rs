//! XPackage identity and installed-package queries. Unported slots retain the logged E_NOTIMPL stub.
use crate::object::{E_INVALIDARG, E_NOT_SUFFICIENT_BUFFER, HResult, S_OK};
use std::ffi::{c_char, c_void};

unsafe extern "system" fn identifier(
    _this: *mut c_void,
    size: usize,
    buffer: *mut c_char,
) -> HResult {
    let name = crate::game_config::package_name();
    let bytes = if name.is_empty() {
        b"AnyXboxX.Game".as_slice()
    } else {
        name.as_bytes()
    };
    let bytes = &bytes[..bytes.len().min(32)];
    if buffer.is_null() || size <= bytes.len() {
        return E_NOT_SUFFICIENT_BUFFER;
    }
    // SAFETY: caller supplies a buffer of at least size bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast(), bytes.len());
        buffer.add(bytes.len()).write(0);
    }
    S_OK
}

unsafe extern "system" fn packaged(_this: *mut c_void) -> bool {
    true
}

unsafe extern "system" fn user_locale(
    _this: *mut c_void,
    size: usize,
    output: *mut c_char,
) -> HResult {
    const LOCALE_NAME_MAX_LENGTH: usize = 85;
    let mut wide = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: wide has the documented capacity for a locale name.
    let length = unsafe {
        windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
            wide.as_mut_ptr(),
            wide.len() as i32,
        )
    };
    if length == 0 {
        // SAFETY: GetLastError reads this thread's last error.
        let error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        return if error == 0 {
            0
        } else {
            (0x8007_0000 | (error & 0xffff)) as i32
        };
    }
    let locale = String::from_utf16_lossy(&wide[..length as usize - 1]);
    let bytes = locale.as_bytes();
    if output.is_null() || size <= bytes.len() {
        return E_NOT_SUFFICIENT_BUFFER;
    }
    // SAFETY: the caller supplies size writable bytes and we checked their length.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast(), bytes.len());
        output.add(bytes.len()).write(0);
    }
    S_OK
}
unsafe extern "system" fn package_kind(
    _this: *mut c_void,
    _id: *const c_char,
    kind: *mut u32,
) -> HResult {
    if kind.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: the output is non-null and writable under the caller's contract.
    unsafe {
        *kind = 0;
    }
    S_OK
}
unsafe extern "system" fn chunk_availability(
    _this: *mut c_void,
    _id: *const c_char,
    _count: u32,
    _selectors: *mut c_void,
    availability: *mut u32,
) -> HResult {
    if availability.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: the output is non-null and writable under the caller's contract.
    unsafe {
        *availability = 0;
    }
    S_OK
}

unsafe extern "system" fn enumerate_chunks(
    _this: *mut c_void,
    _id: *const c_char,
    _count: u32,
    _context: *mut c_void,
    _callback: *mut c_void,
) -> HResult {
    S_OK
}

unsafe extern "system" fn change_install_order(
    _this: *mut c_void,
    _id: *const c_char,
    _count: u32,
    _selectors: *mut c_void,
) -> HResult {
    S_OK
}

unsafe extern "system" fn uninstall_chunks(
    _this: *mut c_void,
    _id: *const c_char,
    _count: u32,
    _selectors: *mut c_void,
) -> HResult {
    crate::log::write(format_args!(
        "[runtime] XPackageUninstallChunks -> E_NOTIMPL"
    ));
    crate::object::E_NOTIMPL
}

unsafe extern "system" fn uninstall_uwp(_this: *mut c_void, _name: *const c_char) -> HResult {
    crate::log::write(format_args!(
        "[runtime] XPackageUninstallUWPInstance -> E_NOTIMPL"
    ));
    crate::object::E_NOTIMPL
}

unsafe extern "system" fn uninstall_package(_this: *mut c_void, _id: *const c_char) -> bool {
    crate::log::write(format_args!(
        "[runtime] XPackageUninstallPackage: not implemented"
    ));
    false
}

unsafe extern "system" fn enumerate_features(
    _this: *mut c_void,
    _id: *const c_char,
    _context: *mut c_void,
    _callback: *mut c_void,
) -> HResult {
    S_OK
}

unsafe extern "system" fn estimate_download(
    _this: *mut c_void,
    _id: *const c_char,
    _count: u32,
    _selectors: *mut c_void,
    size: *mut u64,
    confirm: *mut bool,
) -> HResult {
    // SAFETY: non-null outputs are writable according to the caller's contract.
    unsafe {
        if !size.is_null() {
            *size = 0;
        }
        if !confirm.is_null() {
            *confirm = false;
        }
    }
    S_OK
}
#[repr(C)]
struct WriteStats {
    interval: u64,
    budget: u64,
    elapsed: u64,
    bytes_written: u64,
}
unsafe extern "system" fn write_stats(_this: *mut c_void, stats: *mut WriteStats) -> HResult {
    if stats.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: the output is non-null and writable under the caller's contract.
    unsafe {
        *stats = WriteStats {
            interval: 0,
            budget: u64::MAX,
            elapsed: 0,
            bytes_written: 0,
        };
    }
    S_OK
}
pub fn methods() -> Vec<(usize, *const c_void)> {
    vec![
        (3, identifier as *const c_void),
        (4, packaged as *const c_void),
        (5, crate::package_monitor::create as *const c_void),
        (6, crate::package_monitor::close as *const c_void),
        (7, crate::package_monitor::progress as *const c_void),
        (8, crate::package_monitor::update as *const c_void),
        (9, crate::package_monitor::register as *const c_void),
        (
            10,
            crate::package_monitor::unregister_progress as *const c_void,
        ),
        (11, user_locale as *const c_void),
        (12, chunk_availability as *const c_void),
        (13, enumerate_chunks as *const c_void),
        (14, change_install_order as *const c_void),
        (15, crate::package_monitor::install as *const c_void),
        (16, crate::package_monitor::install_async as *const c_void),
        (17, crate::package_monitor::install_result as *const c_void),
        (18, estimate_download as *const c_void),
        (19, uninstall_chunks as *const c_void),
        (
            22,
            crate::package_monitor::unregister_package as *const c_void,
        ),
        (24, crate::package_mount::path_size as *const c_void),
        (25, crate::package_mount::path as *const c_void),
        (26, crate::package_mount::close as *const c_void),
        (30, write_stats as *const c_void),
        (32, uninstall_uwp as *const c_void),
        (33, enumerate_features as *const c_void),
        (34, uninstall_package as *const c_void),
        (37, crate::package_mount::mount_async as *const c_void),
        (38, crate::package_mount::mount_result as *const c_void),
        (39, crate::package_enumerate::enumerate as *const c_void),
        (40, crate::package_monitor::register as *const c_void),
        (41, package_kind as *const c_void),
    ]
}
