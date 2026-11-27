//! The XUser interface (provider {01ACD177…}): one offline local profile, always signed in, adult,
//! with every privilege. Port of `runtime/xgameruntime/user_interface.cpp`; the XUID and the
//! gamertag are the C++ ones (`ANYXBOXX_GAMERTAG`, default "AnyXboxX"), since the tests check them.
//!
//! The gamer picture is generated locally. Xbox Live token requests are refused offline.

use std::ffi::{c_char, c_void};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use victory_hle::modules::xasync::AsyncBlock;

use crate::gamer_picture;
use crate::object::{
    E_GAMEUSER_DEFERRAL_NOT_AVAILABLE, E_GAMEUSER_USER_NOT_FOUND, E_INVALIDARG,
    E_NOT_SUFFICIENT_BUFFER, E_NOTIMPL, E_OUTOFMEMORY, HResult, S_OK,
};
use crate::user_async::{begin_completed, result_user};

/// Fixed XUID of the profile, outside the ranges of Xbox Live (as in the C++).
pub const XUID: u64 = 0x0009_0000_00A1_1B0B;
/// Local id of the profile, as `XUserGetLocalId` returns it.
const LOCAL_ID: u64 = 1;
const MAX_GAMERTAG: usize = 15;
const DEFAULT_GAMERTAG: &str = "AnyXboxX";
/// Magic of a handle we made: a valid handle points at it.
const HANDLE_MAGIC: u32 = 0x3155_4158; // « AXU1 »
/// XUserGamertagComponent::ModernSuffix: the only component without text here.
const GAMERTAG_MODERN_SUFFIX: u32 = 2;

/// What a user handle points at (the handle is this struct, allocated and freed by us).
#[repr(C)]
struct UserHandle {
    magic: u32,
}

/// The oracle reads the configured name in the Windows process ANSI code page.
fn gamertag() -> &'static [u8] {
    static GAMERTAG: OnceLock<Vec<u8>> = OnceLock::new();
    GAMERTAG.get_or_init(|| {
        let mut bytes = [0u8; MAX_GAMERTAG + 1];
        // SAFETY: the name is NUL-terminated and the buffer has the supplied capacity.
        let len = unsafe {
            windows_sys::Win32::System::Environment::GetEnvironmentVariableA(
                c"ANYXBOXX_GAMERTAG".as_ptr().cast(),
                bytes.as_mut_ptr(),
                bytes.len() as u32,
            )
        } as usize;
        match len {
            1..=MAX_GAMERTAG => bytes[..len].to_vec(),
            _ => DEFAULT_GAMERTAG.as_bytes().to_vec(),
        }
    })
}

/// A new handle, or null when memory is exhausted.
pub fn new_handle() -> *mut c_void {
    Box::into_raw(Box::new(UserHandle {
        magic: HANDLE_MAGIC,
    }))
    .cast()
}

/// Whether `user` is a handle this module made and has not closed.
///
/// # Safety
/// `user` is null or points at memory that was a `UserHandle` (the title only gets ours).
unsafe fn valid(user: *const c_void) -> bool {
    // SAFETY: per this function's contract, the pointer is readable when it is not null.
    !user.is_null() && unsafe { (*user.cast::<UserHandle>()).magic } == HANDLE_MAGIC
}

/// The XUID of a user handle this module made (C++ `LocalUserId`), or `None` for any other pointer.
///
/// # Safety
/// `user` is null or a pointer the title received from this module (see [`valid`]).
pub unsafe fn local_user_xuid(user: *const c_void) -> Option<u64> {
    // SAFETY: forwarded to the contract above.
    unsafe { valid(user) }.then_some(XUID)
}

/// Frees a handle made by [`new_handle`].
pub fn close_handle(user: *mut c_void) {
    if user.is_null() {
        return;
    }
    // SAFETY: a handle we made, freed here once; the title does not use it after closing.
    unsafe { drop(Box::from_raw(user.cast::<UserHandle>())) };
}

// --- Handles ----------------------------------------------------------------------

/// XUserDuplicateHandle (slot 3).
unsafe extern "system" fn duplicate_handle(
    _this: *mut c_void,
    user: *mut c_void,
    out: *mut *mut c_void,
) -> HResult {
    // SAFETY: the handle and output come from the title, per the COM contract.
    if !unsafe { valid(user) } || out.is_null() {
        return E_INVALIDARG;
    }
    let copy = new_handle();
    // SAFETY: `out` is non-null and writable.
    unsafe { *out = copy };
    if copy.is_null() { E_OUTOFMEMORY } else { S_OK }
}

/// XUserCloseHandle (slot 4).
unsafe extern "system" fn close_user_handle(_this: *mut c_void, user: *mut c_void) {
    // SAFETY: as above.
    if unsafe { valid(user) } {
        close_handle(user);
    }
}

/// XUserCompare (slot 5): one user, so two valid handles name the same person.
unsafe extern "system" fn compare(
    _this: *mut c_void,
    first: *mut c_void,
    second: *mut c_void,
) -> i32 {
    // SAFETY: as above.
    if unsafe { valid(first) && valid(second) } {
        0
    } else if first < second {
        -1
    } else if first == second {
        0
    } else {
        1
    }
}

// --- Profiles ------------------------------------------------------------------------

/// XUserGetMaxUsers (slot 6).
unsafe extern "system" fn get_max_users(_this: *mut c_void, max: *mut u32) -> HResult {
    if max.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: non-null and writable.
    unsafe { *max = 1 };
    S_OK
}

/// XUserAddAsync (slot 7): completes at once with the profile.
unsafe extern "system" fn add_async(
    _this: *mut c_void,
    options: u64,
    block: *mut AsyncBlock,
) -> HResult {
    trace_add(options);
    // SAFETY: the block is the title's, per the XAsync contract.
    unsafe { begin_completed(block, ADD_IDENTITY.as_ptr().cast(), S_OK, true) }
}

/// XUserAddResult (slot 8).
unsafe extern "system" fn add_result(
    _this: *mut c_void,
    block: *mut AsyncBlock,
    out: *mut *mut c_void,
) -> HResult {
    // SAFETY: as above.
    unsafe { result_user(block, ADD_IDENTITY.as_ptr().cast(), out) }
}

/// XUserGetLocalId (slot 9).
unsafe extern "system" fn get_local_id(
    _this: *mut c_void,
    user: *mut c_void,
    out: *mut u64,
) -> HResult {
    // SAFETY: as above.
    if !unsafe { valid(user) } || out.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: non-null and writable.
    unsafe { *out = LOCAL_ID };
    S_OK
}

/// XUserFindUserByLocalId (slot 10).
unsafe extern "system" fn find_by_local_id(
    _this: *mut c_void,
    local_id: u64,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_INVALIDARG;
    }
    if local_id != LOCAL_ID {
        return E_GAMEUSER_USER_NOT_FOUND;
    }
    let user = new_handle();
    // SAFETY: non-null and writable.
    unsafe { *out = user };
    if user.is_null() { E_OUTOFMEMORY } else { S_OK }
}

/// XUserGetId (slot 11).
unsafe extern "system" fn get_id(_this: *mut c_void, user: *mut c_void, out: *mut u64) -> HResult {
    // SAFETY: as above.
    if !unsafe { valid(user) } || out.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: non-null and writable.
    unsafe { *out = XUID };
    S_OK
}

/// XUserFindUserById (slot 12).
unsafe extern "system" fn find_by_id(
    _this: *mut c_void,
    id: u64,
    out: *mut *mut c_void,
) -> HResult {
    if out.is_null() {
        return E_INVALIDARG;
    }
    if id != XUID {
        return E_GAMEUSER_USER_NOT_FOUND;
    }
    let user = new_handle();
    // SAFETY: non-null and writable.
    unsafe { *out = user };
    if user.is_null() { E_OUTOFMEMORY } else { S_OK }
}

/// XUserGetIsGuest (slot 13).
unsafe extern "system" fn get_is_guest(
    _this: *mut c_void,
    user: *mut c_void,
    out: *mut bool,
) -> HResult {
    // SAFETY: as above.
    if !unsafe { valid(user) } || out.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: non-null and writable.
    unsafe { *out = false };
    S_OK
}

/// XUserGetState (slot 14): always signed in (XUserState::SignedIn = 0).
unsafe extern "system" fn get_state(
    _this: *mut c_void,
    user: *mut c_void,
    out: *mut u32,
) -> HResult {
    // SAFETY: as above.
    if !unsafe { valid(user) } || out.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: non-null and writable.
    unsafe { *out = 0 };
    S_OK
}

/// XUserGetAgeGroup (slot 19): adult (XUserAgeGroup::Adult = 3).
unsafe extern "system" fn get_age_group(
    _this: *mut c_void,
    user: *mut c_void,
    out: *mut u32,
) -> HResult {
    // SAFETY: as above.
    if !unsafe { valid(user) } || out.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: non-null and writable.
    unsafe { *out = 3 };
    S_OK
}

/// XUserCheckPrivilege (slot 20): every privilege is granted, with no deny reason.
unsafe extern "system" fn check_privilege(
    _this: *mut c_void,
    user: *mut c_void,
    _options: u32,
    privilege: u32,
    allowed: *mut bool,
    reason: *mut u32,
) -> HResult {
    // SAFETY: as above.
    if !unsafe { valid(user) } || allowed.is_null() {
        return E_INVALIDARG;
    }
    crate::log::trace!("[runtime] XUserCheckPrivilege({privilege}) -> accordé");
    // SAFETY: non-null and writable.
    unsafe { *allowed = true };
    if !reason.is_null() {
        // SAFETY: non-null and writable.
        unsafe { *reason = 0 };
    }
    S_OK
}

/// XUserIsSignOutPresent (slot 47).
unsafe extern "system" fn is_sign_out_present(_this: *mut c_void) -> bool {
    false
}

/// XUserIsStoreUser (slot 42).
unsafe extern "system" fn is_store_user(_this: *mut c_void, _user: *mut c_void) -> bool {
    false
}

/// XUserGetSignOutDeferral (slot 35): not available offline.
unsafe extern "system" fn get_sign_out_deferral(
    _this: *mut c_void,
    out: *mut *mut c_void,
) -> HResult {
    if !out.is_null() {
        // SAFETY: non-null and writable.
        unsafe { *out = std::ptr::null_mut() };
    }
    E_GAMEUSER_DEFERRAL_NOT_AVAILABLE
}

/// XUserCloseSignOutDeferralHandle (slot 36).
unsafe extern "system" fn close_sign_out_deferral(_this: *mut c_void, _handle: *mut c_void) {}

/// XUserRegisterForChangeEvent (slot 33): the subscription is accepted; the profile never changes.
unsafe extern "system" fn register_for_change(
    _this: *mut c_void,
    _queue: *const c_void,
    _context: *mut c_void,
    _callback: *mut c_void,
    token: *mut u64,
) -> HResult {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    if token.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: non-null and writable.
    unsafe { *token = NEXT.fetch_add(1, Ordering::Relaxed) };
    S_OK
}

/// XUserUnregisterForChangeEvent (slot 34).
unsafe extern "system" fn unregister_for_change(
    _this: *mut c_void,
    _token: u64,
    _wait: bool,
) -> bool {
    true
}

/// XUserPlatformRemoteConnectSetEventHandlers (slot 43): nothing to show offline.
unsafe extern "system" fn remote_connect_handlers(
    _this: *mut c_void,
    _queue: *const c_void,
    _context: *mut c_void,
) -> HResult {
    S_OK
}

/// XUserPlatformRemoteConnectCancelPrompt (slot 44).
unsafe extern "system" fn remote_connect_cancel(
    _this: *mut c_void,
    _handle: *mut c_void,
) -> HResult {
    S_OK
}

/// XUserPlatformSpopPromptSetEventHandlers (slot 45).
unsafe extern "system" fn spop_handlers(
    _this: *mut c_void,
    _queue: *const c_void,
    _a: *mut c_void,
    _b: *mut c_void,
) -> HResult {
    S_OK
}

/// XUserPlatformSpopPromptComplete (slot 46).
unsafe extern "system" fn spop_complete(
    _this: *mut c_void,
    _handle: *mut c_void,
    _result: u32,
) -> HResult {
    S_OK
}

// --- Asynchronous calls that complete at once -------------------------------------

/// Completes an empty asynchronous call at once with S_OK.
///
/// # Safety
/// `block` is the title's XAsyncBlock, per the XAsync contract.
unsafe fn complete_empty(block: *mut AsyncBlock) -> HResult {
    // SAFETY: forwarded contract.
    unsafe { begin_completed(block, EMPTY_IDENTITY.as_ptr().cast(), S_OK, false) }
}

/// XUserResolvePrivilegeWithUiAsync (slot 21): (user, options, privilege, async).
unsafe extern "system" fn resolve_privilege_async(
    _this: *mut c_void,
    _user: *mut c_void,
    _options: u32,
    _privilege: u32,
    block: *mut AsyncBlock,
) -> HResult {
    // SAFETY: as above.
    unsafe { complete_empty(block) }
}

/// XUserResolveIssueWithUiAsync (slot 29) and its UTF-16 twin (31): (user, text, async).
unsafe extern "system" fn resolve_issue_async(
    _this: *mut c_void,
    _user: *mut c_void,
    _text: *const c_void,
    block: *mut AsyncBlock,
) -> HResult {
    // SAFETY: as above.
    unsafe { complete_empty(block) }
}

/// XUserSignOutAsync (slot 48): (user, async).
unsafe extern "system" fn sign_out_async(
    _this: *mut c_void,
    _user: *mut c_void,
    block: *mut AsyncBlock,
) -> HResult {
    // SAFETY: as above.
    unsafe { complete_empty(block) }
}

/// Result of an empty asynchronous call (slots 22, 30, 32 and 49): its status.
unsafe extern "system" fn empty_result(_this: *mut c_void, block: *mut AsyncBlock) -> HResult {
    // SAFETY: as above; XAsync reads the status of the block.
    victory_hle::modules::xasync::XAsyncGetStatus(block, false)
}

/// XUserAddByIdWithUiAsync (slot 37): the profile for its XUID; otherwise not found.
unsafe extern "system" fn add_by_id_async(
    _this: *mut c_void,
    id: u64,
    block: *mut AsyncBlock,
) -> HResult {
    let found = id == XUID;
    let result = if found {
        S_OK
    } else {
        E_GAMEUSER_USER_NOT_FOUND
    };
    // SAFETY: as above.
    unsafe { begin_completed(block, ADD_BY_ID_IDENTITY.as_ptr().cast(), result, found) }
}

/// XUserAddByIdWithUiResult (slot 38).
unsafe extern "system" fn add_by_id_result(
    _this: *mut c_void,
    block: *mut AsyncBlock,
    out: *mut *mut c_void,
) -> HResult {
    // SAFETY: as above.
    unsafe { result_user(block, ADD_BY_ID_IDENTITY.as_ptr().cast(), out) }
}

/// Identity of the gamer picture calls: XAsync compares their addresses.
static PICTURE_IDENTITY: [u8; 1] = [0];

/// XUserGetGamerPictureAsync (slot 16): the PNG of the size code, completed at once.
unsafe extern "system" fn gamer_picture_async(
    _this: *mut c_void,
    user: *mut c_void,
    size_code: u32,
    block: *mut AsyncBlock,
) -> HResult {
    // SAFETY: the handle is the title's, checked by `valid`.
    if !unsafe { valid(user) } || block.is_null() {
        return E_INVALIDARG;
    }
    let Some(side) = gamer_picture::side(size_code) else {
        return E_INVALIDARG;
    };
    victory_hle::modules::xasync::finished_with(
        block,
        PICTURE_IDENTITY.as_ptr().cast(),
        Ok(gamer_picture::png(side)),
    )
}

/// XUserGetGamerPictureResultSize (slot 17).
unsafe extern "system" fn gamer_picture_size(
    _this: *mut c_void,
    block: *mut AsyncBlock,
    size: *mut usize,
) -> HResult {
    if size.is_null() {
        return E_INVALIDARG;
    }
    victory_hle::modules::xasync::XAsyncGetResultSize(block, size)
}

/// XUserGetGamerPictureResult (slot 18): copies the PNG into the title's buffer.
unsafe extern "system" fn gamer_picture_result(
    _this: *mut c_void,
    block: *mut AsyncBlock,
    size: usize,
    buffer: *mut c_void,
    used: *mut usize,
) -> HResult {
    victory_hle::modules::xasync::XAsyncGetResult(
        block,
        PICTURE_IDENTITY.as_ptr().cast(),
        size,
        buffer,
        used,
    )
}

/// Not reachable offline: Xbox Live tokens (slots 23 to 28).
unsafe extern "system" fn not_available(_this: *mut c_void) -> HResult {
    crate::log::write(format_args!("[runtime] XUser Xbox Live token -> E_NOTIMPL"));
    E_NOTIMPL
}

// Identities of the asynchronous calls: XAsync compares their addresses.
static ADD_IDENTITY: [u8; 1] = [0];
static ADD_BY_ID_IDENTITY: [u8; 1] = [0];
static EMPTY_IDENTITY: [u8; 1] = [0];

fn trace_add(options: u64) {
    crate::log::trace!(
        "[runtime] XUserAddAsync (options 0x{options:x}) -> profil local « {} »",
        String::from_utf8_lossy(gamertag())
    );
}

// --- Gamertag interface {CEF4FAC0…} --------------------------------------------------

/// XUserGetGamertag (gamertag slot 3): the text of a component into `buffer`, with its size.
unsafe extern "system" fn get_gamertag(
    _this: *mut c_void,
    user: *mut c_void,
    component: u32,
    size: usize,
    buffer: *mut c_char,
    used: *mut usize,
) -> HResult {
    // SAFETY: as above.
    if !unsafe { valid(user) } {
        return E_INVALIDARG;
    }
    let text = if component == GAMERTAG_MODERN_SUFFIX {
        &[]
    } else {
        gamertag()
    };
    let needed = text.len() + 1;
    if !used.is_null() {
        // SAFETY: non-null and writable.
        unsafe { *used = needed };
    }
    if buffer.is_null() || size < needed {
        return E_NOT_SUFFICIENT_BUFFER;
    }
    // SAFETY: the buffer holds `size >= needed` bytes, per the check above.
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr(), buffer.cast::<u8>(), text.len());
        buffer.add(text.len()).write(0);
    }
    S_OK
}

/// The method table of the XUser object, by slot, for the slots this module writes.
pub fn methods() -> Vec<(usize, *const c_void)> {
    let table = [
        (3, duplicate_handle as *const c_void),
        (4, close_user_handle as *const c_void),
        (5, compare as *const c_void),
        (6, get_max_users as *const c_void),
        (7, add_async as *const c_void),
        (8, add_result as *const c_void),
        (9, get_local_id as *const c_void),
        (10, find_by_local_id as *const c_void),
        (11, get_id as *const c_void),
        (12, find_by_id as *const c_void),
        (13, get_is_guest as *const c_void),
        (14, get_state as *const c_void),
        (16, gamer_picture_async as *const c_void),
        (17, gamer_picture_size as *const c_void),
        (18, gamer_picture_result as *const c_void),
        (19, get_age_group as *const c_void),
        (20, check_privilege as *const c_void),
        (21, resolve_privilege_async as *const c_void),
        (22, empty_result as *const c_void),
        (23, not_available as *const c_void),
        (24, not_available as *const c_void),
        (25, not_available as *const c_void),
        (26, not_available as *const c_void),
        (27, not_available as *const c_void),
        (28, not_available as *const c_void),
        (29, resolve_issue_async as *const c_void),
        (30, empty_result as *const c_void),
        (31, resolve_issue_async as *const c_void),
        (32, empty_result as *const c_void),
        (33, register_for_change as *const c_void),
        (34, unregister_for_change as *const c_void),
        (35, get_sign_out_deferral as *const c_void),
        (36, close_sign_out_deferral as *const c_void),
        (37, add_by_id_async as *const c_void),
        (38, add_by_id_result as *const c_void),
        (42, is_store_user as *const c_void),
        (43, remote_connect_handlers as *const c_void),
        (44, remote_connect_cancel as *const c_void),
        (45, spop_handlers as *const c_void),
        (46, spop_complete as *const c_void),
        (47, is_sign_out_present as *const c_void),
        (48, sign_out_async as *const c_void),
        (49, empty_result as *const c_void),
    ];
    table.to_vec()
}

/// The method table of the gamertag object.
pub fn gamertag_methods() -> Vec<(usize, *const c_void)> {
    vec![(3, get_gamertag as *const c_void)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handle_is_valid_until_closed() {
        let user = new_handle();
        // SAFETY: a handle we just made.
        assert!(unsafe { valid(user) });
        close_handle(user);
        assert!(!unsafe { valid(std::ptr::null()) });
    }

    #[test]
    fn the_gamertag_is_the_default_unless_configured() {
        assert_eq!(DEFAULT_GAMERTAG, "AnyXboxX");
        assert!(gamertag().len() <= MAX_GAMERTAG);
    }

    #[test]
    fn gamertag_text_needs_its_nul_and_the_title_is_told_the_size() {
        let user = new_handle();
        let mut buffer = [0 as c_char; 4];
        let mut used = 0usize;
        // SAFETY: valid handle and buffers; the buffer is too small for "AnyXboxX\0".
        let hr = unsafe {
            get_gamertag(
                std::ptr::null_mut(),
                user,
                0,
                buffer.len(),
                buffer.as_mut_ptr(),
                &mut used,
            )
        };
        assert_eq!((hr, used), (E_NOT_SUFFICIENT_BUFFER, gamertag().len() + 1));
        close_handle(user);
    }
}
