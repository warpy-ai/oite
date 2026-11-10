//! An object's own properties: slots laid out by its shape (the first
//! `header.inline_slots` in the object, the rest in an overflow array), or
//! a dictionary.

use super::super::abi::OtValue;
use super::super::heap::NativeObject;
use super::atom::Atom;
use super::dictionary::Dict;
use super::shape::{ACCESSOR, MAX_PROPS, Shape};

fn shape(obj: &NativeObject) -> Option<&'static Shape> {
    unsafe { obj.shape.as_ref() }
}

/// The dictionary of an object in dictionary mode (`shape` is null).
fn dict(obj: &NativeObject) -> &'static mut Dict {
    unsafe { &mut *(obj.overflow as *mut Dict) }
}

/// Slot `i` of `obj`, which must have room for it.
pub fn slot(obj: &NativeObject, i: usize) -> *mut u64 {
    let inline = obj.header.inline_slots as usize;
    if i < inline {
        obj.inline.as_ptr().wrapping_add(i) as *mut u64
    } else {
        unsafe { obj.overflow.add(i - inline) }
    }
}

/// Make room for slot `i`.
pub fn reserve(obj: &mut NativeObject, i: usize) {
    let inline = obj.header.inline_slots as usize;
    if i < inline {
        return;
    }
    let needed = i + 1 - inline;
    let cap = obj.overflow_cap as usize;
    if needed <= cap {
        return;
    }
    let new_cap = needed.max(cap * 2).max(4);
    let mut slots: Vec<u64> = Vec::with_capacity(new_cap);
    if cap > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(obj.overflow, slots.as_mut_ptr(), cap);
            drop(Vec::from_raw_parts(obj.overflow, 0, cap));
        }
    }
    obj.overflow = slots.as_mut_ptr();
    obj.overflow_cap = slots.capacity() as u32;
    std::mem::forget(slots);
}

/// The value (or accessor pair, see `accessor`) and flags of an own property.
pub fn get_own(obj: &NativeObject, atom: Atom) -> Option<(u64, u8)> {
    match shape(obj) {
        Some(s) => {
            let (i, flags) = s.find(atom)?;
            Some((unsafe { *slot(obj, i) }, flags))
        }
        None => dict(obj).get(atom),
    }
}

/// Add an own property, or replace its value and flags. Returns whether
/// the object's layout changed (anything but a new value in place).
pub fn set_own(obj: &mut NativeObject, atom: Atom, flags: u8, value: u64) -> bool {
    let Some(s) = shape(obj) else {
        let d = dict(obj);
        let same = matches!(d.get(atom), Some((_, f)) if f == flags);
        d.set(atom, flags, value);
        return !same;
    };
    match s.find(atom) {
        Some((i, f)) if f == flags => {
            unsafe { *slot(obj, i) = value };
            return false;
        }
        None if s.props.len() < MAX_PROPS => {
            let i = s.props.len();
            reserve(obj, i);
            unsafe { *slot(obj, i) = value };
            obj.shape = s.with(atom, flags);
        }
        // A property changing kind, or too many properties.
        _ => {
            to_dictionary(obj);
            dict(obj).set(atom, flags, value);
        }
    }
    true
}

/// Remove an own property. The object becomes a dictionary.
pub fn remove_own(obj: &mut NativeObject, atom: Atom) -> bool {
    if get_own(obj, atom).is_none() {
        return false;
    }
    to_dictionary(obj);
    dict(obj).remove(atom)
}

/// The own properties in insertion order: (name, flags, value).
pub fn own(obj: &NativeObject) -> Vec<(Atom, u8, u64)> {
    match shape(obj) {
        Some(s) => s
            .props
            .iter()
            .enumerate()
            .map(|(i, p)| (p.atom, p.flags, unsafe { *slot(obj, i) }))
            .collect(),
        None => dict(obj).iter().collect(),
    }
}

fn to_dictionary(obj: &mut NativeObject) {
    if obj.shape.is_null() {
        return;
    }
    let mut d = Dict::default();
    for (atom, flags, value) in own(obj) {
        d.set(atom, flags, value);
    }
    if obj.overflow_cap > 0 {
        unsafe {
            drop(Vec::from_raw_parts(
                obj.overflow,
                0,
                obj.overflow_cap as usize,
            ))
        };
    }
    obj.overflow = Box::into_raw(Box::new(d)) as *mut u64;
    obj.overflow_cap = 0;
    obj.shape = std::ptr::null();
}

/// The (getter, setter) pair an accessor property's slot points to; each
/// is a function or `undefined`.
pub fn accessor(slot_value: u64) -> &'static mut [u64; 2] {
    unsafe { &mut *(slot_value as usize as *mut [u64; 2]) }
}

/// Set the getter or the setter of the accessor property `atom`, making
/// the property an accessor if it is not one.
pub fn define_accessor(
    obj: &mut NativeObject,
    atom: Atom,
    getter: Option<u64>,
    setter: Option<u64>,
) {
    let pair = match get_own(obj, atom) {
        Some((v, flags)) if flags & ACCESSOR != 0 => accessor(v),
        _ => {
            let undefined = OtValue::undefined().to_bits();
            let pair = Box::leak(Box::new([undefined, undefined]));
            set_own(obj, atom, ACCESSOR, pair as *mut [u64; 2] as usize as u64);
            pair
        }
    };
    if let Some(g) = getter {
        pair[0] = g;
    }
    if let Some(s) = setter {
        pair[1] = s;
    }
}
