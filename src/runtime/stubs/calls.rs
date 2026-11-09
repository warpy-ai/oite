//! Calls: functions, methods, intrinsics and spread; the value interface
//! the shared built-in methods use (`NativeHost`).

use super::*;

// =========================================================================
// Calls
// =========================================================================

/// Call a function value: `f(...args)` with the given `this`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_call(func: u64, this: u64, argv: *const u64, argc: usize) -> u64 {
    match object(OtValue::from_bits(func)) {
        Some((ptr, ObjectKind::Function)) => {
            let func = unsafe { ptr.as_ref::<NativeFunction>() };
            let f: NativeFn = unsafe { std::mem::transmute(func.code) };
            f(this, argv, argc, func.env)
        }
        _ => throw_error(
            "TypeError",
            &format!("{} is not a function", inspect(OtValue::from_bits(func))),
        ),
    }
}

/// `recv.name(...args)`: looks up the method and calls it with `this = recv`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_call_method(
    recv: u64,
    key: *const u8,
    key_len: usize,
    argv: *const u64,
    argc: usize,
) -> u64 {
    let r = OtValue::from_bits(recv);
    let key = unsafe { name(key, key_len) };

    // Built-in array methods.
    if let Some((ptr, ObjectKind::Array)) = object(r) {
        let arr = array(ptr);
        let a = unsafe { args(argv, argc) };
        match key {
            "push" => {
                let mut len = arr.len as usize;
                for &v in a {
                    len = array_push(arr, v);
                }
                return number(len as f64);
            }
            "pop" => {
                if arr.len == 0 {
                    return undefined();
                }
                arr.len -= 1;
                return unsafe { *arr.elements.add(arr.len as usize) };
            }
            _ => {}
        }
        // Every other array method is shared with the VM. It works on a
        // copy of the elements, which is written back afterwards.
        let mut elems: Vec<u64> = (0..arr.len as usize)
            .map(|i| unsafe { *arr.elements.add(i) })
            .collect();
        if let Some(result) = builtins::array_method(&NativeHost, &mut elems, key, a) {
            array_reserve(arr, elems.len());
            for (i, v) in elems.iter().enumerate() {
                unsafe { *arr.elements.add(i) = *v };
            }
            arr.len = elems.len() as u32;
            return outcome(result, recv);
        }
    }

    // Promise methods.
    if let Some(id) = promise_id(recv)
        && let Some(result) = promise_method(id, key, unsafe { args(argv, argc) })
    {
        return result;
    }

    // `f.call(thisArg, ...args)`, `f.apply(thisArg, array)`
    if let Some((_, ObjectKind::Function)) = object(r)
        && (key == "call" || key == "apply")
    {
        let a = unsafe { args(argv, argc) };
        let this = a.first().copied().unwrap_or_else(undefined);
        if key == "call" {
            let rest = if a.is_empty() { a } else { &a[1..] };
            return ot_call(recv, this, rest.as_ptr(), rest.len());
        }
        let list: Vec<u64> = match a.get(1).and_then(|v| object(OtValue::from_bits(*v))) {
            Some((ptr, ObjectKind::Array)) => {
                let arr = array(ptr);
                (0..arr.len as usize)
                    .map(|i| unsafe { *arr.elements.add(i) })
                    .collect()
            }
            _ => Vec::new(),
        };
        return ot_call(recv, this, list.as_ptr(), list.len());
    }

    // Number methods, shared with the VM.
    if r.is_number() {
        let a = unsafe { args(argv, argc) };
        if let Some(result) = builtins::number_method(&NativeHost, r.as_number_unchecked(), key, a)
        {
            return outcome(result, recv);
        }
    }

    // String methods, shared with the VM.
    if let Some(s) = as_str(r) {
        let a = unsafe { args(argv, argc) };
        // Indexing: an ASCII string's flag saves scanning it.
        if let Some(result) = builtins::string_index(&NativeHost, s, is_ascii(r), key, a) {
            return outcome(result, recv);
        }
        if let Some(result) = builtins::string_method(&NativeHost, s, key, a) {
            return outcome(result, recv);
        }
    }

    let method = get_property(r, key);
    if method == EXCEPTION_BITS {
        return method;
    }
    if let Some((_, ObjectKind::Function)) = object(OtValue::from_bits(method)) {
        return ot_call(method, recv, argv, argc);
    }
    throw_error(
        "TypeError",
        &format!("{}.{} is not a function", inspect(r), key),
    )
}

/// `__intrinsic_<name>(args)`: intrinsic `id`, as on every backend
/// (`runtime::intrinsics`).
#[unsafe(no_mangle)]
pub extern "C" fn ot_intrinsic(id: usize, argv: *const u64, argc: usize) -> u64 {
    let a = unsafe { args(argv, argc) };
    outcome(super::super::intrinsics::call(&NativeHost, id as u32, a), 0)
}

/// `[..., ...source]`: append `source`'s elements (an array's, or a
/// string's characters) to the array `target`; `target`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_array_spread(target: u64, source: u64) -> u64 {
    use builtins::Host;
    let Some((tptr, ObjectKind::Array)) = object(OtValue::from_bits(target)) else {
        return target;
    };
    let src = OtValue::from_bits(source);
    let items: Vec<u64> = if let Some(s) = as_str(src) {
        s.chars().map(|c| new_string(&c.to_string())).collect()
    } else if let Some(elems) = NativeHost.array_elements(&source) {
        elems
    } else {
        return throw_error("TypeError", &format!("{} is not iterable", inspect(src)));
    };
    for v in items {
        array_push(array(tptr), v);
    }
    target
}

/// `{..., ...source}`: copy `source`'s own properties onto the object
/// `target` (a getter's value, not the getter); `target`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_object_spread(target: u64, source: u64) -> u64 {
    use builtins::Host;
    let src = OtValue::from_bits(source);
    let entries: Vec<(String, u64)> = if let Some(s) = as_str(src) {
        s.chars()
            .enumerate()
            .map(|(i, c)| (i.to_string(), new_string(&c.to_string())))
            .collect()
    } else if let Some(elems) = NativeHost.array_elements(&source) {
        elems
            .into_iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect()
    } else {
        let mut entries = Vec::new();
        for k in NativeHost.own_keys(&source) {
            let v = get_property(src, &k);
            if v == EXCEPTION_BITS {
                return v;
            }
            entries.push((k, v));
        }
        entries
    };
    for (k, v) in entries {
        if set_property(OtValue::from_bits(target), &k, v) == EXCEPTION_BITS {
            return EXCEPTION_BITS;
        }
    }
    target
}

/// The native value representation, for the shared built-in methods.
pub(super) struct NativeHost;

impl builtins::Host for NativeHost {
    type Value = u64;

    fn number(&self, v: &u64) -> Option<f64> {
        OtValue::from_bits(*v).as_number()
    }

    fn string<'a>(&'a self, v: &'a u64) -> Option<&'a str> {
        as_str(OtValue::from_bits(*v))
    }

    fn to_string(&self, v: &u64) -> String {
        value_to_string(OtValue::from_bits(*v))
    }

    fn strict_equal(&self, a: &u64, b: &u64) -> bool {
        strict_equal(OtValue::from_bits(*a), OtValue::from_bits(*b))
    }

    fn array_elements(&self, v: &u64) -> Option<Vec<u64>> {
        match object(OtValue::from_bits(*v)) {
            Some((ptr, ObjectKind::Array)) => {
                let arr = array(ptr);
                Some(
                    (0..arr.len as usize)
                        .map(|i| unsafe { *arr.elements.add(i) })
                        .collect(),
                )
            }
            _ => None,
        }
    }

    fn is_array(&self, v: &u64) -> bool {
        matches!(object(OtValue::from_bits(*v)), Some((_, ObjectKind::Array)))
    }

    fn own_keys(&self, v: &u64) -> Vec<String> {
        match object(OtValue::from_bits(*v)) {
            Some((ptr, ObjectKind::Object)) => {
                let mut names: Vec<String> = Vec::new();
                for (k, _) in props(ptr).iter() {
                    if let Some(name) = builtins::own_key_name(k)
                        && !names.iter().any(|n| n == name)
                    {
                        names.push(name.to_string());
                    }
                }
                names
            }
            Some((ptr, ObjectKind::Array)) => (0..array(ptr).len).map(|i| i.to_string()).collect(),
            _ => Vec::new(),
        }
    }

    fn make_number(&self, n: f64) -> u64 {
        number(n)
    }

    fn make_string(&self, s: String) -> u64 {
        new_string(&s)
    }

    fn make_bool(&self, b: bool) -> u64 {
        boolean(b)
    }

    fn undefined(&self) -> u64 {
        undefined()
    }
}

/// Turn a built-in method's result into a value.
pub(super) fn outcome(result: Result<builtins::Outcome<u64>, builtins::Error>, recv: u64) -> u64 {
    match result {
        Ok(builtins::Outcome::Value(v)) => v,
        Ok(builtins::Outcome::This) => recv,
        Ok(builtins::Outcome::NewArray(elems)) => {
            let out = ot_alloc_array(elems.len());
            if let Some((ptr, _)) = object(OtValue::from_bits(out)) {
                let arr = array(ptr);
                for v in elems {
                    array_push(arr, v);
                }
            }
            out
        }
        Err(e) => throw_error(e.kind, &e.message),
    }
}
