//! Inline caches: what a property access site remembers about the last
//! object it saw (docs/docs/object-model.md, section 3).
//!
//! Compiled code has one `PropCache` per `o.x`, `o.x = v` and `o.m()` site,
//! in the binary's data. For an own data property, compiled code itself
//! compares the object's shape with `shape` and loads or stores the slot.
//! A write that adds the property to an object of a known shape is also
//! done in compiled code, from the second entry. Everything else goes to
//! the runtime (`stubs::prop_ic`), which checks the second entry (a
//! property found on the prototype chain, or a property being added)
//! before doing the full lookup.

use super::super::heap::NativeObject;
use std::sync::atomic::{AtomicU32, Ordering};

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
/// The second entry, as compiled code reads it for a write that adds the
/// property: the shape before, the prototype, the shape after, the new
/// slot and the epoch.
pub const ADD_SHAPE_OFFSET: usize = std::mem::offset_of!(PropCache, proto_shape);
pub const ADD_PROTO_OFFSET: usize = std::mem::offset_of!(PropCache, proto);
pub const ADD_NEXT_OFFSET: usize = std::mem::offset_of!(PropCache, holder);
pub const ADD_SLOT_OFFSET: usize = std::mem::offset_of!(PropCache, holder_offset);
pub const ADD_EPOCH_OFFSET: usize = std::mem::offset_of!(PropCache, epoch);
/// The second entry, as compiled code reads it for a method found on the
/// prototype chain: the receiver's shape and prototype, the object that
/// has the method, its slot there and the epoch.
pub const CHAIN_SHAPE_OFFSET: usize = std::mem::offset_of!(PropCache, proto_shape);
pub const CHAIN_PROTO_OFFSET: usize = std::mem::offset_of!(PropCache, proto);
pub const CHAIN_HOLDER_OFFSET: usize = std::mem::offset_of!(PropCache, holder);
pub const CHAIN_SLOT_OFFSET: usize = std::mem::offset_of!(PropCache, holder_offset);
pub const CHAIN_EPOCH_OFFSET: usize = std::mem::offset_of!(PropCache, epoch);

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

/// The cache of one object literal site: the shape of the finished literal.
/// Compiled code allocates the object in that shape (`ot_alloc_literal`)
/// and stores property `i` in slot `i` itself.
#[repr(C)]
pub struct LiteralCache {
    /// The shape with all `count` properties; 0 until the site first runs.
    pub shape: u64,
    /// Slots to allocate in the object itself (at least `count`).
    pub slots: u32,
    pub count: u32,
    /// The property names (pointer, length), in order.
    pub names: *const (*const u8, usize),
}

/// The cache of one `new` site: where a class of the cached shape keeps
/// its class marker, constructor and prototype.
#[repr(C)]
pub struct NewCache {
    pub shape: u64,
    pub class_offset: i32,
    pub ctor_offset: i32,
    pub proto_offset: i32,
    pub _pad: u32,
}

impl NewCache {
    pub const fn new() -> Self {
        NewCache {
            shape: NO_SHAPE,
            class_offset: 0,
            ctor_offset: 0,
            proto_offset: 0,
            _pad: 0,
        }
    }
}

impl Default for NewCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Where slot `i` is in an object with `inline_slots` slots in itself, as
/// compiled code wants it: `>= 0` is a byte offset from the object; `< 0`
/// is `!offset` bytes into the overflow array.
pub fn slot_offset(i: usize, inline_slots: usize) -> i32 {
    if i < inline_slots {
        (std::mem::offset_of!(NativeObject, inline) + 8 * i) as i32
    } else {
        !((8 * (i - inline_slots)) as i32)
    }
}

/// The slot index `offset` stands for (the inverse of `slot_offset`).
pub fn slot_index(offset: i32, inline_slots: usize) -> usize {
    if offset >= 0 {
        (offset as usize - std::mem::offset_of!(NativeObject, inline)) / 8
    } else {
        inline_slots + (!offset as usize) / 8
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

/// The prototype epoch: it changes whenever an object that is used as a
/// prototype gets, loses or redefines a property, or gets another
/// prototype. Cached lookups through a prototype chain are valid only in
/// the epoch they were made in.
///
/// Compiled code reads it by name. It is one counter for the process: a
/// change on any thread only makes other threads' caches miss once.
#[unsafe(no_mangle)]
pub static OT_EPOCH: AtomicU32 = AtomicU32::new(0);

pub fn epoch() -> u32 {
    OT_EPOCH.load(Ordering::Relaxed)
}

pub fn bump_epoch() {
    OT_EPOCH.fetch_add(1, Ordering::Relaxed);
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
        assert_eq!(std::mem::size_of::<LiteralCache>(), 24);
        assert_eq!(std::mem::offset_of!(LiteralCache, names), 16);
        assert_eq!(std::mem::size_of::<NewCache>(), 24);
        assert_eq!(slot_offset(0, 4), 40);
        assert_eq!(slot_offset(3, 4), 64);
        assert_eq!(!slot_offset(4, 4), 0);
        assert_eq!(!slot_offset(6, 4), 16);
        assert_eq!(!slot_offset(0, 0), 0);
        for (i, inline) in [(0, 4), (3, 4), (4, 4), (9, 2), (0, 0)] {
            assert_eq!(slot_index(slot_offset(i, inline), inline), i);
        }
    }
}
