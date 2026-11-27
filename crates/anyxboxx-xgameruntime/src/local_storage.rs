//! XPersistentLocalStorage (C++ `local_storage_interface.cpp`): one folder on the PC per game.
//!
//! Provider {F4FAF4D4…}, interface {41A4E10C…}, methods 3 to 8 of `gdk_api_table.inc`. The declared
//! size is the `<SizeMB>` of `MicrosoftGame.config`; without one, 512 MB. Asking for space is always
//! granted: there is no window to show.

use std::ffi::{c_char, c_void};
use std::path::Path;

use victory_hle::modules::xasync::AsyncBlock;

use crate::object::{E_INVALIDARG, E_NOTIMPL, HResult, S_OK};
use crate::save_async::{self, Completion};
use crate::save_store;

/// Used when the game config declares no local storage size.
pub const DEFAULT_BYTES: u64 = 512 * 1024 * 1024;
/// HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER): the title's buffer is too small.
const E_NOT_SUFFICIENT_BUFFER: HResult = 0x8007_007A_u32 as i32;

static PROMPT_IDENTITY: [u8; 1] = [7];

/// XPersistentLocalStorageSpaceInfo: free, total free, used and total bytes (read by the title).
#[repr(C)]
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceInfo {
    pub available_free_bytes: u64,
    pub total_free_bytes: u64,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

/// The space figures for a declared size (0 means none declared) and the bytes in use.
pub fn space_info(declared: u64, used: u64) -> SpaceInfo {
    let total = if declared != 0 {
        declared
    } else {
        DEFAULT_BYTES
    };
    let free = total.saturating_sub(used);
    SpaceInfo {
        available_free_bytes: free,
        total_free_bytes: free,
        used_bytes: used,
        total_bytes: total,
    }
}

/// The folder of the game's local storage with its trailing backslash, or `None`.
fn folder_path() -> Option<String> {
    let (root, title) = save_store::title_root()?;
    let folder = save_store::local_storage_folder(&root, title)?;
    Some(path_text(&folder))
}

fn path_text(folder: &Path) -> String {
    format!("{}\\", folder.display())
}

/// Bytes of a path with its NUL.
fn with_nul(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    bytes
}

/// XPersistentLocalStorageGetPathSize (slot 3).
unsafe extern "system" fn get_path_size(_this: *mut c_void, size: *mut usize) -> HResult {
    if size.is_null() {
        return E_INVALIDARG;
    }
    let Some(folder) = folder_path() else {
        return crate::save_store::E_PATH_NOT_FOUND;
    };
    // SAFETY: `size` is writable, per the title's contract.
    unsafe { *size = folder.len() + 1 };
    S_OK
}

/// XPersistentLocalStorageGetPath (slot 4).
unsafe extern "system" fn get_path(
    _this: *mut c_void,
    size: usize,
    path: *mut c_char,
    used: *mut usize,
) -> HResult {
    let Some(folder) = folder_path() else {
        return crate::save_store::E_PATH_NOT_FOUND;
    };
    let bytes = with_nul(&folder);
    if !used.is_null() {
        // SAFETY: `used` is writable, per the title's contract.
        unsafe { *used = bytes.len() };
    }
    if path.is_null() || size < bytes.len() {
        return E_NOT_SUFFICIENT_BUFFER;
    }
    // SAFETY: `path` holds `size >= bytes.len()` bytes, per the check above.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), path.cast::<u8>(), bytes.len()) };
    S_OK
}

/// XPersistentLocalStorageGetSpaceInfo (slot 5).
unsafe extern "system" fn get_space_info(_this: *mut c_void, info: *mut SpaceInfo) -> HResult {
    if info.is_null() {
        return E_INVALIDARG;
    }
    let used = match save_store::title_root()
        .and_then(|(root, title)| save_store::local_storage_folder(&root, title))
    {
        Some(folder) => save_store::used_bytes(&folder),
        None => 0,
    };
    let declared = crate::game_config::local_storage_bytes();
    // SAFETY: `info` is writable, per the title's contract.
    unsafe { *info = space_info(declared, used) };
    S_OK
}

/// XPersistentLocalStoragePromptUserForSpaceAsync (slot 6): granted at once.
unsafe extern "system" fn prompt_for_space_async(
    _this: *mut c_void,
    _bytes: u64,
    block: *mut AsyncBlock,
) -> HResult {
    save_async::begin(
        block,
        PROMPT_IDENTITY.as_ptr().cast(),
        Completion::status(S_OK),
    )
}

/// XPersistentLocalStoragePromptUserForSpaceResult (slot 7).
unsafe extern "system" fn prompt_for_space_result(
    _this: *mut c_void,
    block: *mut AsyncBlock,
) -> HResult {
    save_async::status(block)
}

/// XPersistentLocalStorageMountForPackage (slot 8): another package's storage is not supported yet.
unsafe extern "system" fn mount_for_package(
    _this: *mut c_void,
    _package: *const c_char,
    mount: *mut *mut c_void,
) -> HResult {
    if !mount.is_null() {
        // SAFETY: `mount` is writable, per the title's contract.
        unsafe { *mount = std::ptr::null_mut() };
    }
    crate::log::write(format_args!(
        "[runtime] XPersistentLocalStorageMountForPackage -> E_NOTIMPL"
    ));
    E_NOTIMPL
}

/// The method table of the local storage object, slots 3 to 8.
pub fn methods() -> Vec<(usize, *const c_void)> {
    vec![
        (3, get_path_size as *const c_void),
        (4, get_path as *const c_void),
        (5, get_space_info as *const c_void),
        (6, prompt_for_space_async as *const c_void),
        (7, prompt_for_space_result as *const c_void),
        (8, mount_for_package as *const c_void),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_declared_size_is_the_total_and_free_space_is_what_is_left() {
        let info = space_info(64 * 1024 * 1024, 10 * 1024 * 1024);
        assert_eq!(info.total_bytes, 64 * 1024 * 1024);
        assert_eq!(info.used_bytes, 10 * 1024 * 1024);
        assert_eq!(info.available_free_bytes, 54 * 1024 * 1024);
        assert_eq!(info.total_free_bytes, info.available_free_bytes);
    }

    #[test]
    fn an_undeclared_size_is_the_default_and_full_storage_has_no_free_space() {
        assert_eq!(space_info(0, 0).total_bytes, DEFAULT_BYTES);
        assert_eq!(space_info(100, 250).available_free_bytes, 0);
    }

    #[test]
    fn the_path_ends_with_a_backslash_and_its_nul_is_counted() {
        let folder = Path::new("C:\\data\\ABCD0001\\local");
        assert_eq!(path_text(folder), "C:\\data\\ABCD0001\\local\\");
        assert_eq!(with_nul("ab"), vec![b'a', b'b', 0]);
    }

    #[test]
    fn the_table_covers_slots_3_to_8() {
        let slots: Vec<usize> = methods().iter().map(|(slot, _)| *slot).collect();
        assert_eq!(slots, vec![3, 4, 5, 6, 7, 8]);
    }
}
