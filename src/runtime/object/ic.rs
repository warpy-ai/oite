//! Inline caches: what a property access site remembers about the last
//! object it saw (docs/docs/object-model.md, section 3).
//!
//! Compiled code has one `PropCache` per `o.x`, `o.x = v` and `o.m()` site,
//! in the binary's data. For an own data property, compiled code itself
//! compares the object's shape with `shape` and loads or stores the slot.
//! Everything else goes to the runtime (`stubs::ic`), which checks the other
//! entry (a property found on the prototype chain, or a property being
//! added) before doing the full lookup.

use super::super::heap::{INLINE_SLOTS, NativeObject};
use std::cell::Cell;

/// A shape pointer no object has: an empty cache entry.
pub const NO_SHAPE: u64 = 1;

/// The cache of one access site. Compiled code reads `shape` and `offset`
/// at the offsets below.
#[repr(C)]
pub struct PropCache {
    /// Own data property: objects with this shape have it at `offset`.
    pub shape: u64,
    /// See `slot_offset`.
    pub offset: i32,
    /// The property name's atom, once `resolved`.
    pub atom: u32,
    /// The property name, a constant in the compiled code.
    pub name_ptr: *const u8,
    pub name_len: usize,
    /// Second entry. A read or call found on the prototype chain: objects
    /// with this shape and the prototype `proto` find the property on
    /// `holder`, at `holder_offset`, while `epoch` is current. A write that
    /// adds the property: objects with this shape and prototype become
    /// shape `holder`, with the new slot at `holder_offset`.
    pub proto_shape: u64,
    pub proto: u64,
    pub holder: u64,
    pub holder_offset: i32,
    pub epoch: u32,
    pub resolved: u32,
    pub _pad: u32,
}

pub const SHAPE_OFFSET: usize = std::mem::offset_of!(PropCache, shape);
pub const OFFSET_OFFSET: usize = std::mem::offset_of!(PropCache, offset);

impl PropCache {
    pub const fn new(name_ptr: *const u8, name_len: usize) -> Self {
        PropCache {
            shape: NO_SHAPE,
            offset: 0,
            atom: 0,
            name_ptr,
            name_len,
            proto_shape: NO_SHAPE,
            proto: 0,
            holder: 0,
            holder_offset: 0,
            epoch: 0,
            resolved: 0,
            _pad: 0,
        }
    }
}

/// Where slot `i` is, as compiled code wants it: `>= 0` is a byte offset
/// from the object; `< 0` is `!offset` bytes into the overflow array.
pub fn slot_offset(i: usize) -> i32 {
    if i < INLINE_SLOTS {
        (std::mem::offset_of!(NativeObject, inline) + 8 * i) as i32
    } else {
        !((8 * (i - INLINE_SLOTS)) as i32)
    }
}

/// The slot at `offset` (see `slot_offset`) of `obj`.
///
/// # Safety
/// `offset` must come from `slot_offset` for a slot `obj` has room for.
pub unsafe fn slot_at(obj: *mut NativeObject, offset: i32) -> *mut u64 {
    unsafe {
        if offset >= 0 {
            (obj as *mut u8).add(offset as usize) as *mut u64
        } else {
            ((*obj).overflow as *mut u8).add(!offset as usize) as *mut u64
        }
    }
}

thread_local! {
    static EPOCH: Cell<u32> = const { Cell::new(0) };
}

/// The prototype epoch: it changes whenever an object that is used as a
/// prototype gets, loses or redefines a property, or gets another
/// prototype. Cached lookups through a prototype chain are valid only in
/// the epoch they were made in.
pub fn epoch() -> u32 {
    EPOCH.with(|e| e.get())
}

pub fn bump_epoch() {
    EPOCH.with(|e| e.set(e.get().wrapping_add(1)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_offsets() {
        assert_eq!(std::mem::size_of::<PropCache>(), 72);
        assert_eq!(SHAPE_OFFSET, 0);
        assert_eq!(OFFSET_OFFSET, 8);
        assert_eq!(std::mem::offset_of!(PropCache, name_ptr), 16);
        assert_eq!(std::mem::offset_of!(PropCache, proto_shape), 32);
        assert_eq!(std::mem::offset_of!(PropCache, holder_offset), 56);
        assert_eq!(slot_offset(0), 40);
        assert_eq!(
            slot_offset(INLINE_SLOTS - 1),
            40 + 8 * (INLINE_SLOTS as i32 - 1)
        );
        assert_eq!(!slot_offset(INLINE_SLOTS), 0);
        assert_eq!(!slot_offset(INLINE_SLOTS + 2), 16);
    }
}
