//! Static runtime objects: a title calls their methods through a vtable, slot by slot.
//!
//! Layout as the C++ `StaticInterface`: the object's first word points at its method table.
//! Slots 0 to 2 are IUnknown; the GDK functions follow at the slot given by `gdk_api_table.inc`.
//! Objects are never freed, so AddRef and Release do nothing.

use std::ffi::c_void;

use crate::log;

pub type HResult = i32;
pub const S_OK: HResult = 0;
pub const E_NOTIMPL: HResult = 0x8000_4001_u32 as i32;
pub const E_NOINTERFACE: HResult = 0x8000_4002_u32 as i32;
pub const E_POINTER: HResult = 0x8000_4003_u32 as i32;
pub const E_INVALIDARG: HResult = 0x8007_0057_u32 as i32;
pub const E_OUTOFMEMORY: HResult = 0x8007_000E_u32 as i32;
/// HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER): the title's buffer is too small.
pub const E_NOT_SUFFICIENT_BUFFER: HResult = 0x8007_007A_u32 as i32;
/// The documented "no such user" error (XUser).
pub const E_GAMEUSER_USER_NOT_FOUND: HResult = 0x8924_5104_u32 as i32;
/// XSystem / XGame failure with no more detail (E_FAIL).
pub const E_FAIL: HResult = 0x8000_4005_u32 as i32;
/// XUser sign-out deferral not available offline.
pub const E_GAMEUSER_DEFERRAL_NOT_AVAILABLE: HResult = 0x8924_5103_u32 as i32;

/// Slots every object has (IUnknown).
pub const IUNKNOWN_SLOTS: usize = 3;

/// The first word is the method table; the table is leaked with the object.
#[repr(C)]
pub struct Object {
    methods: *const *const c_void,
}

// SAFETY: the method table is written once, before the object is shared, and never after.
unsafe impl Sync for Object {}

impl Object {
    /// The method table, as title code reads it through the object's first word.
    #[cfg(test)]
    fn methods(&self) -> *const *const c_void {
        self.methods
    }
}

/// Builds a static object with `slots` method slots (at least IUnknown's three).
///
/// `query_interface` answers IUnknown's slot 0; every other slot starts as `E_NOTIMPL`, then the
/// `(slot, method)` pairs of `methods` replace theirs. The table is complete before it is leaked,
/// so it is never written afterwards.
pub fn build(
    slots: usize,
    query_interface: *const c_void,
    methods: &[(usize, *const c_void)],
) -> &'static Object {
    let mut table = vec![not_implemented as *const c_void; slots.max(IUNKNOWN_SLOTS)];
    table[0] = query_interface;
    table[1] = add_ref as *const c_void;
    table[2] = release as *const c_void;
    for &(slot, method) in methods {
        table[slot] = method;
    }
    let table: &'static [*const c_void] = Box::leak(table.into_boxed_slice());
    Box::leak(Box::new(Object {
        methods: table.as_ptr(),
    }))
}

/// AddRef of a static object: always 1, nothing to count.
unsafe extern "system" fn add_ref(_this: *mut c_void) -> u32 {
    1
}

/// Release of a static object: always 1, the object lives for the whole process.
unsafe extern "system" fn release(_this: *mut c_void) -> u32 {
    1
}

/// A method we know the slot of but have not written: logged, then `E_NOTIMPL`.
pub unsafe extern "system" fn not_implemented(_this: *mut c_void) -> HResult {
    log::write(format_args!(
        "[runtime] méthode d'interface non écrite appelée -> E_NOTIMPL"
    ));
    E_NOTIMPL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unwritten_slot_is_not_implemented_and_iunknown_is_kept() {
        let qi = std::ptr::dangling::<c_void>();
        let object = build(8, qi, &[]);
        // SAFETY: the table has 8 slots, all readable.
        let slot = |i: usize| unsafe { *object.methods().add(i) };
        assert_eq!(slot(0), qi);
        assert_eq!(slot(1), add_ref as *const c_void);
        assert_eq!(slot(7), not_implemented as *const c_void);
        // SAFETY: as above; the method is called with the object as `this`.
        let call = unsafe {
            std::mem::transmute::<*const c_void, unsafe extern "system" fn(*mut c_void) -> HResult>(
                slot(7),
            )
        };
        // SAFETY: not_implemented ignores its argument.
        assert_eq!(unsafe { call(std::ptr::null_mut()) }, E_NOTIMPL);
    }
}
