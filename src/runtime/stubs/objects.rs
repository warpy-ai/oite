//! Objects and arrays: allocation, property lookup along the prototype
//! chain, accessors, elements.

use super::*;

/// The object at `ptr`, which must be an `ObjectKind::Object`.
pub(super) fn obj(ptr: HeapPtr) -> &'static mut NativeObject {
    unsafe { ptr.as_mut::<NativeObject>() }
}

pub(super) fn array(ptr: HeapPtr) -> &'static mut NativeArray {
    unsafe { ptr.as_mut::<NativeArray>() }
}

/// Make room for at least `needed` elements.
pub(super) fn array_reserve(arr: &mut NativeArray, needed: usize) {
    use std::alloc::{Layout, realloc};
    if needed <= arr.capacity as usize {
        return;
    }
    let new_cap = needed.max(arr.capacity as usize * 2).max(4);
    let old = Layout::array::<u64>(arr.capacity as usize).unwrap();
    let new_size = Layout::array::<u64>(new_cap).unwrap().size();
    let p = unsafe { realloc(arr.elements as *mut u8, old, new_size) } as *mut u64;
    if p.is_null() {
        fatal_error("RangeError: out of memory growing an array");
    }
    arr.elements = p;
    arr.capacity = new_cap as u32;
}

pub(super) fn array_set(arr: &mut NativeArray, idx: usize, value: u64) {
    array_reserve(arr, idx + 1);
    for i in arr.len as usize..idx {
        unsafe { *arr.elements.add(i) = undefined() };
    }
    unsafe { *arr.elements.add(idx) = value };
    if idx >= arr.len as usize {
        arr.len = (idx + 1) as u32;
    }
}

pub(super) fn array_push(arr: &mut NativeArray, value: u64) -> usize {
    let len = arr.len as usize;
    array_set(arr, len, value);
    len + 1
}

/// The property `atom` on the object at `ptr` or its prototype chain: its
/// value (for an accessor, the pair) and flags.
pub(super) fn find(ptr: HeapPtr, atom: Atom) -> Option<(u64, u8)> {
    let mut cur = ptr;
    for _ in 0..1000 {
        let o = obj(cur);
        if let Some(found) = object::get_own(o, atom) {
            return Some(found);
        }
        match object(OtValue::from_bits(o.proto)) {
            Some((p, ObjectKind::Object)) => cur = p,
            _ => return None,
        }
    }
    None
}

/// The value of the data property `key` on the object at `ptr` or its
/// prototype chain. `getter:x` and `setter:x` give the accessor functions
/// of `x`.
pub(super) fn lookup(ptr: HeapPtr, key: &str) -> Option<u64> {
    lookup_atom(ptr, atom::atom(key))
}

pub(super) fn lookup_atom(ptr: HeapPtr, key: Atom) -> Option<u64> {
    let part = |target: Atom, i: usize| {
        let (v, flags) = find(ptr, target)?;
        let f = (flags & ACCESSOR != 0).then(|| object::accessor(v)[i])?;
        (f != undefined()).then_some(f)
    };
    match atom::info(key).role {
        Role::Getter(target) => part(target, 0),
        Role::Setter(target) => part(target, 1),
        Role::Plain if key == atom::PROTO => {
            let p = obj(ptr).proto;
            (p != undefined()).then_some(p)
        }
        Role::Plain => {
            let (v, flags) = find(ptr, key)?;
            (flags & ACCESSOR == 0).then_some(v)
        }
    }
}

/// The value of the own data property `key`.
pub(super) fn own_value(ptr: HeapPtr, key: Atom) -> Option<u64> {
    let (v, flags) = object::get_own(obj(ptr), key)?;
    (flags & ACCESSOR == 0).then_some(v)
}

/// The own data properties that `console.log` shows: (name, value).
pub(super) fn visible_data(ptr: HeapPtr) -> Vec<(&'static str, u64)> {
    object::own(obj(ptr))
        .into_iter()
        .filter(|(_, flags, _)| flags & ACCESSOR == 0)
        .map(|(a, _, v)| (atom::info(a), v))
        .filter(|(info, _)| !info.hidden)
        .map(|(info, v)| (info.name, v))
        .collect()
}

/// Property lookup, following the prototype chain.
pub(super) fn get_property(v: OtValue, key: &str) -> u64 {
    get_property_atom(v, atom::atom(key))
}

pub(super) fn get_property_atom(v: OtValue, key: Atom) -> u64 {
    let Some((ptr, kind)) = object(v) else {
        if v.is_null() || v.is_undefined() {
            return throw_error(
                "TypeError",
                &format!(
                    "Cannot read properties of {} (reading '{}')",
                    value_to_string(v),
                    atom::name(key)
                ),
            );
        }
        return undefined();
    };
    match kind {
        ObjectKind::Object => {
            if matches!(key, atom::THEN | atom::CATCH | atom::FINALLY)
                && promise_id(v.to_bits()).is_some()
            {
                // Promise methods run through `ot_call_method`; a function
                // so that `typeof p.then` is "function".
                return builtin(ot_builtin_promise_static);
            }
            if key == atom::PROTO || !atom::is_plain(key) {
                return lookup_atom(ptr, key).unwrap_or_else(undefined);
            }
            match find(ptr, key) {
                Some((pair, flags)) if flags & ACCESSOR != 0 => {
                    let getter = object::accessor(pair)[0];
                    if is_function_value(getter) {
                        ot_call(getter, v.to_bits(), std::ptr::null(), 0)
                    } else {
                        undefined()
                    }
                }
                Some((value, _)) => value,
                None => undefined(),
            }
        }
        ObjectKind::Array => {
            let arr = array(ptr);
            if key == atom::LENGTH {
                return number(arr.len as f64);
            }
            match atom::name(key).parse::<usize>() {
                Ok(i) if i < arr.len as usize => unsafe { *arr.elements.add(i) },
                _ => undefined(),
            }
        }
        ObjectKind::String => {
            let s = unsafe { ptr.as_ref::<NativeString>().as_str() };
            if key == atom::LENGTH {
                // Byte length, matching the VM until the string model is
                // decided (#133).
                return number(s.len() as f64);
            }
            match atom::name(key).parse::<usize>() {
                Ok(i) if i < s.len() => new_string(&s[i..i + 1]),
                _ => undefined(),
            }
        }
        _ => undefined(),
    }
}

fn is_function_value(v: u64) -> bool {
    matches!(
        object(OtValue::from_bits(v)),
        Some((_, ObjectKind::Function))
    )
}

/// `v.key = value`; `undefined`, or `EXCEPTION_BITS` if it throws.
pub(super) fn set_property(v: OtValue, key: &str, value: u64) -> u64 {
    set_property_atom(v, atom::atom(key), value)
}

pub(super) fn set_property_atom(v: OtValue, key: Atom, value: u64) -> u64 {
    match object(v) {
        Some((ptr, ObjectKind::Object)) => {
            let o = obj(ptr);
            // The common case: an own data property, or a new one on an
            // object whose prototype chain has no such accessor.
            if atom::is_plain(key) && key != atom::PROTO {
                match object::get_own(o, key) {
                    Some((_, flags)) if flags & ACCESSOR == 0 => {
                        object::set_own(o, key, 0, value);
                        return undefined();
                    }
                    _ => {}
                }
            }
            match atom::info(key).role {
                Role::Getter(target) => object::define_accessor(o, target, Some(value), None),
                Role::Setter(target) => object::define_accessor(o, target, None, Some(value)),
                Role::Plain if key == atom::PROTO => o.proto = value,
                Role::Plain => match find(ptr, key) {
                    // An accessor, here or on the prototype chain: its
                    // setter, or nothing without one.
                    Some((pair, flags)) if flags & ACCESSOR != 0 => {
                        let setter = object::accessor(pair)[1];
                        if is_function_value(setter) {
                            let r = ot_call(setter, v.to_bits(), &value, 1);
                            return if r == EXCEPTION_BITS { r } else { undefined() };
                        }
                    }
                    _ => object::set_own(o, key, 0, value),
                },
            }
        }
        Some((ptr, ObjectKind::Array)) => {
            let arr = array(ptr);
            if key == atom::LENGTH {
                let n = to_number(OtValue::from_bits(value));
                if n >= 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64 {
                    let n = n as usize;
                    if n < arr.len as usize {
                        arr.len = n as u32;
                    } else {
                        for i in arr.len as usize..n {
                            array_set(arr, i, undefined());
                        }
                    }
                }
            } else if let Ok(i) = atom::name(key).parse::<usize>() {
                array_set(arr, i, value);
            }
        }
        Some(_) => {}
        None => {
            if v.is_null() || v.is_undefined() {
                return throw_error(
                    "TypeError",
                    &format!(
                        "Cannot set properties of {} (setting '{}')",
                        value_to_string(v),
                        atom::name(key)
                    ),
                );
            }
        }
    }
    undefined()
}

/// An index or key value as a property name.
pub(super) fn key_string(key: OtValue) -> String {
    value_to_string(key)
}

/// A number key usable as an array index.
pub(super) fn index_of(key: OtValue) -> Option<usize> {
    let n = key.as_number()?;
    if n >= 0.0 && n.fract() == 0.0 && n < u32::MAX as f64 {
        Some(n as usize)
    } else {
        None
    }
}

// =========================================================================
// Allocation
// =========================================================================

/// Allocate a new empty object.
#[unsafe(no_mangle)]
pub extern "C" fn ot_alloc_object() -> u64 {
    match heap().alloc_object() {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

/// Allocate a new empty array with room for `capacity` elements.
#[unsafe(no_mangle)]
pub extern "C" fn ot_alloc_array(capacity: usize) -> u64 {
    match heap().alloc_array(capacity) {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

/// Allocate a new string from UTF-8 bytes.
#[unsafe(no_mangle)]
pub extern "C" fn ot_alloc_string(data: *const u8, len: usize) -> u64 {
    if data.is_null() {
        return new_string("");
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match std::str::from_utf8(bytes) {
        Ok(s) => new_string(s),
        Err(_) => undefined(),
    }
}

/// Wrap compiled code (with the `NativeFn` signature) as a function value.
#[unsafe(no_mangle)]
pub extern "C" fn ot_make_function(code: *const u8) -> u64 {
    ot_make_closure(code, undefined())
}

/// A closure: compiled code plus the environment object it captured.
#[unsafe(no_mangle)]
pub extern "C" fn ot_make_closure(code: *const u8, env: u64) -> u64 {
    match heap().alloc_function(code, env) {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

// =========================================================================
// Properties and elements
// =========================================================================

/// `obj.key`
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_prop(obj: u64, key: *const u8, key_len: usize) -> u64 {
    get_property_atom(OtValue::from_bits(obj), unsafe {
        atom::static_atom(key, key_len)
    })
}

/// `obj.key = value`
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_prop(obj: u64, key: *const u8, key_len: usize, value: u64) -> u64 {
    set_property_atom(
        OtValue::from_bits(obj),
        unsafe { atom::static_atom(key, key_len) },
        value,
    )
}

/// `obj[key]`
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_index(obj: u64, key: u64) -> u64 {
    let o = OtValue::from_bits(obj);
    let k = OtValue::from_bits(key);
    if let (Some((ptr, ObjectKind::Array)), Some(i)) = (object(o), index_of(k)) {
        let arr = array(ptr);
        return if i < arr.len as usize {
            unsafe { *arr.elements.add(i) }
        } else {
            undefined()
        };
    }
    get_property(o, &key_string(k))
}

/// `obj[key] = value`
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_index(obj: u64, key: u64, value: u64) -> u64 {
    let o = OtValue::from_bits(obj);
    let k = OtValue::from_bits(key);
    if let (Some((ptr, ObjectKind::Array)), Some(i)) = (object(o), index_of(k)) {
        array_set(array(ptr), i, value);
        return undefined();
    }
    set_property(o, &key_string(k), value)
}

/// `delete obj.key`
#[unsafe(no_mangle)]
pub extern "C" fn ot_delete_prop(obj: u64, key: *const u8, key_len: usize) -> u64 {
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(obj)) {
        let key = unsafe { atom::static_atom(key, key_len) };
        object::remove_own(self::obj(ptr), key);
    }
    boolean(true)
}

/// `arr.length` (or `.length` of anything else).
#[unsafe(no_mangle)]
pub extern "C" fn ot_array_len(arr: u64) -> u64 {
    get_property_atom(OtValue::from_bits(arr), atom::LENGTH)
}

/// `arr.push(value)`; returns the new length.
#[unsafe(no_mangle)]
pub extern "C" fn ot_array_push(arr: u64, value: u64) -> u64 {
    match object(OtValue::from_bits(arr)) {
        Some((ptr, ObjectKind::Array)) => number(array_push(array(ptr), value) as f64),
        _ => throw_error("TypeError", "push called on a non-array"),
    }
}
