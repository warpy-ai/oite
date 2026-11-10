//! Property access through an inline cache (`object::ic`): what compiled
//! code calls when its own check of the cache misses.

use super::*;
use std::mem::offset_of;

/// The site's property name as an atom (interned on first use).
fn cache_atom(c: &mut PropCache) -> Atom {
    if c.resolved == 0 {
        c.atom = unsafe { atom::static_atom(c.name_ptr, c.name_len) };
        c.resolved = 1;
    }
    c.atom
}

/// The object behind `v`, if it has a shape (not a dictionary).
fn shaped(v: u64) -> Option<&'static mut NativeObject> {
    match object(OtValue::from_bits(v)) {
        Some((ptr, ObjectKind::Object)) => {
            let o = obj(ptr);
            (!o.shape.is_null()).then_some(o)
        }
        _ => None,
    }
}

/// The value the second cache entry finds for `o` on its prototype chain.
fn chain_hit(c: &PropCache, o: &NativeObject) -> Option<u64> {
    (o.shape as u64 == c.proto_shape && o.proto == c.proto && c.epoch == ic::epoch())
        .then(|| unsafe { *ic::slot_at(c.holder as *mut NativeObject, c.holder_offset) })
}

/// Remember where a read of `key` on `o` found a data property: on `o`
/// itself (first entry) or on its prototype chain (second entry).
fn remember_read(c: &mut PropCache, o: &NativeObject, key: Atom) {
    if !atom::is_plain(key) || key == atom::PROTO {
        return;
    }
    let shape = unsafe { &*o.shape };
    match shape.find(key) {
        Some((i, flags)) if flags & ACCESSOR == 0 => {
            c.shape = o.shape as u64;
            c.offset = ic::slot_offset(i);
        }
        Some(_) => {}
        None => {
            let mut holder = o.proto;
            for _ in 0..1000 {
                let Some(h) = shaped(holder) else { return };
                match unsafe { &*h.shape }.find(key) {
                    Some((i, flags)) if flags & ACCESSOR == 0 => {
                        c.proto_shape = o.shape as u64;
                        c.proto = o.proto;
                        c.holder = h as *mut NativeObject as u64;
                        c.holder_offset = ic::slot_offset(i);
                        c.epoch = ic::epoch();
                        return;
                    }
                    Some(_) => return,
                    None => holder = h.proto,
                }
            }
        }
    }
}

/// `obj.key`, for a site whose inline check missed.
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_prop_ic(value: u64, cache: *mut PropCache) -> u64 {
    let c = unsafe { &mut *cache };
    let key = cache_atom(c);
    let Some(o) = shaped(value) else {
        return get_property_atom(OtValue::from_bits(value), key);
    };
    if let Some(v) = chain_hit(c, o) {
        return v;
    }
    let result = get_property_atom(OtValue::from_bits(value), key);
    // A promise's `then` is not a property: nothing to remember.
    if result != EXCEPTION_BITS && !o.shape.is_null() {
        remember_read(c, o, key);
    }
    result
}

/// `obj.key = value`, for a site whose inline check missed.
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_prop_ic(target: u64, cache: *mut PropCache, value: u64) -> u64 {
    let c = unsafe { &mut *cache };
    let key = cache_atom(c);
    let Some(o) = shaped(target) else {
        return set_property_atom(OtValue::from_bits(target), key, value);
    };
    // Second entry: adding the property to an object of a known shape.
    if o.shape as u64 == c.proto_shape
        && o.proto == c.proto
        && c.epoch == ic::epoch()
        && o.header.flags & ObjectHeader::PROTOTYPE == 0
    {
        let inline = offset_of!(NativeObject, inline) as i32;
        let slot = if c.holder_offset >= 0 {
            ((c.holder_offset - inline) / 8) as usize
        } else {
            crate_inline_slots() + (!c.holder_offset / 8) as usize
        };
        object::reserve(o, slot);
        unsafe { *ic::slot_at(o, c.holder_offset) = value };
        o.shape = c.holder as *const _;
        return undefined();
    }
    let before = o.shape;
    let result = set_property_atom(OtValue::from_bits(target), key, value);
    if result == EXCEPTION_BITS || o.shape.is_null() || !atom::is_plain(key) || key == atom::PROTO {
        return result;
    }
    let after = unsafe { &*o.shape };
    if let Some((i, flags)) = after.find(key)
        && flags & ACCESSOR == 0
    {
        if before == o.shape {
            // The property was there: later writes store in place.
            c.shape = o.shape as u64;
            c.offset = ic::slot_offset(i);
        } else if !before.is_null()
            && unsafe { &*before }.with(key, 0) == o.shape
            && o.header.flags & ObjectHeader::PROTOTYPE == 0
        {
            c.proto_shape = before as u64;
            c.proto = o.proto;
            c.holder = o.shape as u64;
            c.holder_offset = ic::slot_offset(i);
            c.epoch = ic::epoch();
        }
    }
    result
}

fn crate_inline_slots() -> usize {
    super::super::heap::INLINE_SLOTS
}

/// `recv.key(...args)` through the site's cache.
#[unsafe(no_mangle)]
pub extern "C" fn ot_call_method_ic(
    recv: u64,
    cache: *mut PropCache,
    argv: *const u64,
    argc: usize,
) -> u64 {
    let c = unsafe { &mut *cache };
    // Strings, arrays, numbers and dictionaries: nothing to cache.
    let Some(o) = shaped(recv) else {
        return ot_call_method(recv, c.name_ptr, c.name_len, argv, argc);
    };
    let method = if o.shape as u64 == c.shape {
        Some(unsafe { *ic::slot_at(o, c.offset) })
    } else {
        chain_hit(c, o)
    };
    if let Some(m) = method
        && let Some((_, ObjectKind::Function)) = object(OtValue::from_bits(m))
    {
        return ot_call(m, recv, argv, argc);
    }
    let result = ot_call_method(recv, c.name_ptr, c.name_len, argv, argc);
    // After the call: the cache describes the receiver as it is now.
    if result != EXCEPTION_BITS
        && let Some(o) = shaped(recv)
        && promise_id(recv).is_none()
    {
        let key = cache_atom(c);
        remember_read(c, o, key);
    }
    result
}
