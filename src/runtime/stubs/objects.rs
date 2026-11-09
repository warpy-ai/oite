//! Objects and arrays: allocation, property lookup along the prototype
//! chain, accessors, elements.

use super::*;

pub(super) fn props(ptr: HeapPtr) -> &'static mut PropertyMap {
    let obj = unsafe { ptr.as_mut::<NativeObject>() };
    if obj.properties.is_null() {
        obj.properties = Box::into_raw(Box::new(PropertyMap::new()));
    }
    unsafe { &mut *obj.properties }
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

thread_local! {
    /// Whether a getter or setter was ever defined: until then property
    /// access skips looking for them.
    pub(super) static HAS_ACCESSORS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `key` on the object at `ptr` or its prototype chain, without accessors.
pub(super) fn lookup(ptr: HeapPtr, key: &str) -> Option<u64> {
    let mut cur = ptr;
    for _ in 0..1000 {
        let map = props(cur);
        if let Some((_, bits)) = map.iter().find(|(k, _)| k == key) {
            return Some(*bits);
        }
        let (_, proto) = map.iter().find(|(k, _)| k == "__proto__")?;
        match object(OtValue::from_bits(*proto)) {
            Some((p, ObjectKind::Object)) => cur = p,
            _ => return None,
        }
    }
    None
}

/// Property lookup, following `__proto__`.
pub(super) fn get_property(v: OtValue, key: &str) -> u64 {
    let Some((ptr, kind)) = object(v) else {
        if v.is_null() || v.is_undefined() {
            return throw_error(
                "TypeError",
                &format!(
                    "Cannot read properties of {} (reading '{}')",
                    value_to_string(v),
                    key
                ),
            );
        }
        return undefined();
    };
    match kind {
        ObjectKind::Object => {
            if matches!(key, "then" | "catch" | "finally") && promise_id(v.to_bits()).is_some() {
                // Promise methods run through `ot_call_method`; a function
                // so that `typeof p.then` is "function".
                return builtin(ot_builtin_promise_static);
            }
            if HAS_ACCESSORS.with(|a| a.get())
                && let Some(getter) = lookup(ptr, &format!("getter:{key}"))
                && matches!(
                    object(OtValue::from_bits(getter)),
                    Some((_, ObjectKind::Function))
                )
            {
                return ot_call(getter, v.to_bits(), std::ptr::null(), 0);
            }
            let mut cur = ptr;
            for _ in 0..1000 {
                let map = props(cur);
                if let Some((_, bits)) = map.iter().find(|(k, _)| k == key) {
                    return *bits;
                }
                match map.iter().find(|(k, _)| k == "__proto__") {
                    Some((_, proto)) => match OtValue::from_bits(*proto).as_pointer() {
                        Some(p) => cur = p,
                        None => break,
                    },
                    None => break,
                }
            }
            undefined()
        }
        ObjectKind::Array => {
            let arr = array(ptr);
            if key == "length" {
                return number(arr.len as f64);
            }
            match key.parse::<usize>() {
                Ok(i) if i < arr.len as usize => unsafe { *arr.elements.add(i) },
                _ => undefined(),
            }
        }
        ObjectKind::String => {
            let s = unsafe { ptr.as_ref::<NativeString>().as_str() };
            if key == "length" {
                // Byte length, matching the VM until the string model is
                // decided (#133).
                return number(s.len() as f64);
            }
            match key.parse::<usize>() {
                Ok(i) if i < s.len() => new_string(&s[i..i + 1]),
                _ => undefined(),
            }
        }
        _ => undefined(),
    }
}

/// `v.key = value`; `undefined`, or `EXCEPTION_BITS` if it throws.
pub(super) fn set_property(v: OtValue, key: &str, value: u64) -> u64 {
    match object(v) {
        Some((ptr, ObjectKind::Object)) => {
            if key.starts_with("getter:") || key.starts_with("setter:") {
                HAS_ACCESSORS.with(|a| a.set(true));
            } else if HAS_ACCESSORS.with(|a| a.get())
                && let Some(setter) = lookup(ptr, &format!("setter:{key}"))
                && matches!(
                    object(OtValue::from_bits(setter)),
                    Some((_, ObjectKind::Function))
                )
            {
                let r = ot_call(setter, v.to_bits(), &value, 1);
                return if r == EXCEPTION_BITS { r } else { undefined() };
            }
            let map = props(ptr);
            if let Some(entry) = map.iter_mut().find(|(k, _)| k == key) {
                entry.1 = value;
            } else {
                map.push((key.to_string(), value));
            }
        }
        Some((ptr, ObjectKind::Array)) => {
            let arr = array(ptr);
            if key == "length" {
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
            } else if let Ok(i) = key.parse::<usize>() {
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
                        key
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
    get_property(OtValue::from_bits(obj), unsafe { name(key, key_len) })
}

/// `obj.key = value`
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_prop(obj: u64, key: *const u8, key_len: usize, value: u64) -> u64 {
    set_property(
        OtValue::from_bits(obj),
        unsafe { name(key, key_len) },
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
        let key = unsafe { name(key, key_len) };
        props(ptr).retain(|(k, _)| k != key);
    }
    boolean(true)
}

/// `arr.length` (or `.length` of anything else).
#[unsafe(no_mangle)]
pub extern "C" fn ot_array_len(arr: u64) -> u64 {
    get_property(OtValue::from_bits(arr), "length")
}

/// `arr.push(value)`; returns the new length.
#[unsafe(no_mangle)]
pub extern "C" fn ot_array_push(arr: u64, value: u64) -> u64 {
    match object(OtValue::from_bits(arr)) {
        Some((ptr, ObjectKind::Array)) => number(array_push(array(ptr), value) as f64),
        _ => throw_error("TypeError", "push called on a non-array"),
    }
}
