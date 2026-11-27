//! The Xbox Series X identity the title sees (C++ `system_interface.cpp`): XSystem, the analytics
//! block, the Title ID and launch, the runtime features and the display's timeout deferral.
//! Method tables are in `gdk_api_table.inc`; signatures are those of the public GDK headers.

use std::ffi::{c_char, c_void};

use crate::game_config;
use crate::object::{E_FAIL, E_INVALIDARG, E_NOT_SUFFICIENT_BUFFER, E_NOTIMPL, HResult, S_OK};

/// XSystemConsoleIdBytes: a fixed console identifier of 39 bytes with its NUL.
const CONSOLE_ID: &[u8] = b"00000000.00000000.00000000.00000000.00\0";
const SANDBOX_ID: &[u8] = b"RETAIL\0";
const APP_DEVICE_ID: &[u8] = b"QW55WGJveFgtZGV2aWNlLTAwMDAwMDAwMDAwMDAx\0";
/// Windows.Xbox on Xbox Series X, build 25398 (the version the title reads).
const XBOX_OS: XVersion = XVersion {
    major: 10,
    minor: 0,
    build: 25398,
    revision: 0,
};

/// XVersion of XSystem.h.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XVersion {
    pub major: u16,
    pub minor: u16,
    pub build: u16,
    pub revision: u16,
}

/// XSystemAnalyticsInfo: 0x90 bytes, as the GDK's xgameruntime.lib lays it out.
#[repr(C)]
pub struct AnalyticsInfo {
    os_version: XVersion,
    hosting_os_version: XVersion,
    family: [c_char; 64],
    form: [c_char; 64],
}

/// Copies `text` (NUL included) into the title's buffer, reporting the size it needs in `used`.
///
/// # Safety
/// `buffer` is null or has `size` writable bytes; `used` is null or writable.
unsafe fn copy_text(text: &[u8], size: usize, buffer: *mut c_char, used: *mut usize) -> HResult {
    if !used.is_null() {
        // SAFETY: non-null and writable, per the contract.
        unsafe { *used = text.len() };
    }
    if buffer.is_null() || size < text.len() {
        return E_NOT_SUFFICIENT_BUFFER;
    }
    // SAFETY: the buffer has `size >= text.len()` writable bytes, per the check above.
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buffer.cast::<u8>(), text.len()) };
    S_OK
}

/// XSystemGetConsoleId (SystemInterface slot 3).
unsafe extern "system" fn get_console_id(
    _this: *mut c_void,
    size: usize,
    buffer: *mut c_char,
    used: *mut usize,
) -> HResult {
    // SAFETY: forwarded contract.
    unsafe { copy_text(CONSOLE_ID, size, buffer, used) }
}

/// XSystemGetXboxLiveSandboxId (slot 4).
unsafe extern "system" fn get_sandbox_id(
    _this: *mut c_void,
    size: usize,
    buffer: *mut c_char,
    used: *mut usize,
) -> HResult {
    // SAFETY: forwarded contract.
    unsafe { copy_text(SANDBOX_ID, size, buffer, used) }
}

/// XSystemGetAppSpecificDeviceId (slot 5).
unsafe extern "system" fn get_app_device_id(
    _this: *mut c_void,
    size: usize,
    buffer: *mut c_char,
    used: *mut usize,
) -> HResult {
    // SAFETY: forwarded contract.
    unsafe { copy_text(APP_DEVICE_ID, size, buffer, used) }
}

/// XSystemHandleTrack (slot 6): handles are not tracked on PC.
unsafe extern "system" fn handle_track(
    _this: *mut c_void,
    _a: *mut c_void,
    _b: *mut c_void,
) -> HResult {
    S_OK
}

/// XSystemIsHandleValid (slot 7): always valid.
unsafe extern "system" fn is_handle_valid(_this: *mut c_void, _handle: *mut c_void) -> bool {
    true
}

/// XSystemAllowFullDownloadBandwidth (slot 8): nothing to change.
unsafe extern "system" fn allow_full_bandwidth(_this: *mut c_void, _allow: bool) {}

/// The analytics block (slot 3 of the analytics object), returned through the pointer it is given.
unsafe extern "system" fn get_analytics_info(
    _this: *mut c_void,
    out: *mut AnalyticsInfo,
) -> *mut AnalyticsInfo {
    if out.is_null() {
        return out;
    }
    let mut family = [0 as c_char; 64];
    let mut form = [0 as c_char; 64];
    for (slot, byte) in family.iter_mut().zip(b"Windows.Xbox") {
        *slot = *byte as c_char;
    }
    for (slot, byte) in form.iter_mut().zip(b"Xbox Series X") {
        *slot = *byte as c_char;
    }
    // SAFETY: non-null and writable, per the contract.
    unsafe {
        out.write(AnalyticsInfo {
            os_version: XBOX_OS,
            hosting_os_version: XBOX_OS,
            family,
            form,
        })
    };
    out
}

/// XGameGetXboxTitleId (game slot 3): the Title ID of the config, or E_FAIL.
unsafe extern "system" fn get_title_id(_this: *mut c_void, out: *mut u32) -> HResult {
    if out.is_null() {
        return E_INVALIDARG;
    }
    let id = game_config::title_id();
    // SAFETY: non-null and writable.
    unsafe { *out = id };
    if id != 0 { S_OK } else { E_FAIL }
}

/// XLaunchNewGame (game slot 4): changing game is not supported yet.
unsafe extern "system" fn launch_new_game(
    _this: *mut c_void,
    exe: *const c_char,
    _a: *const c_char,
    _b: *mut c_void,
) {
    crate::log::write(format_args!(
        "[runtime] XLaunchNewGame : changement de jeu pas encore pris en charge ({})",
        if exe.is_null() {
            ""
        } else {
            "exécutable demandé"
        }
    ));
}

/// XLaunchRestartOnCrash (game slot 5): accepted.
unsafe extern "system" fn restart_on_crash(
    _this: *mut c_void,
    _a: *const c_char,
    _b: u32,
) -> HResult {
    S_OK
}

/// XGameRuntimeIsFeatureAvailable (feature slot 3): every feature is there.
unsafe extern "system" fn feature_available(_this: *mut c_void, _feature: u32) -> bool {
    true
}

/// XLaunchUri (display slot 3): opening links on the PC's behalf is refused.
unsafe extern "system" fn launch_uri(
    _this: *mut c_void,
    _a: *mut c_void,
    _uri: *const c_char,
) -> HResult {
    crate::log::write(format_args!("[runtime] XLaunchUri -> E_NOTIMPL"));
    E_NOTIMPL
}

/// XDisplayAcquireTimeoutDeferral (display slot 4): a token with nothing behind it.
unsafe extern "system" fn acquire_timeout_deferral(
    _this: *mut c_void,
    handle: *mut *mut c_void,
) -> HResult {
    if handle.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: non-null and writable.
    unsafe { *handle = std::ptr::dangling_mut::<c_void>() };
    S_OK
}

/// XDisplayCloseTimeoutDeferralHandle (display slot 5).
unsafe extern "system" fn close_timeout_deferral(_this: *mut c_void, _handle: *mut c_void) {}

/// The method table of one of this module's objects, by its C++ name.
pub fn methods(object: &str) -> Vec<(usize, *const c_void)> {
    let table: Vec<(usize, *const c_void)> = match object {
        "SystemInterface" => vec![
            (3, get_console_id as *const c_void),
            (4, get_sandbox_id as *const c_void),
            (5, get_app_device_id as *const c_void),
            (6, handle_track as *const c_void),
            (7, is_handle_valid as *const c_void),
            (8, allow_full_bandwidth as *const c_void),
        ],
        "AnalyticsInterface" => vec![(3, get_analytics_info as *const c_void)],
        "GameInterface" => vec![
            (3, get_title_id as *const c_void),
            (4, launch_new_game as *const c_void),
            (5, restart_on_crash as *const c_void),
        ],
        "FeatureInterface" => vec![(3, feature_available as *const c_void)],
        "DisplayInterface" => vec![
            (3, launch_uri as *const c_void),
            (4, acquire_timeout_deferral as *const c_void),
            (5, close_timeout_deferral as *const c_void),
        ],
        _ => Vec::new(),
    };
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_console_id_fits_its_documented_size() {
        assert_eq!(CONSOLE_ID.len(), 39);
    }

    #[test]
    fn analytics_names_a_series_x() {
        let mut info = std::mem::MaybeUninit::<AnalyticsInfo>::uninit();
        // SAFETY: the pointer is to writable storage of the right size.
        let back = unsafe { get_analytics_info(std::ptr::null_mut(), info.as_mut_ptr()) };
        assert_eq!(back, info.as_mut_ptr());
        // SAFETY: the function initialised the block.
        let info = unsafe { info.assume_init() };
        assert_eq!(
            &info.family[..12]
                .iter()
                .map(|c| *c as u8)
                .collect::<Vec<_>>(),
            b"Windows.Xbox"
        );
        assert_eq!(info.os_version.major, 10);
    }

    #[test]
    fn a_short_buffer_is_refused_with_the_size_needed() {
        let mut buffer = [0 as c_char; 4];
        let mut used = 0usize;
        // SAFETY: valid buffer and output.
        let hr = unsafe { copy_text(CONSOLE_ID, buffer.len(), buffer.as_mut_ptr(), &mut used) };
        assert_eq!((hr, used), (E_NOT_SUFFICIENT_BUFFER, 39));
    }
}
