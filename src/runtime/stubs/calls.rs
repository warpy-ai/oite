//! Calls: functions, methods, intrinsics and spread; the value interface
//! the shared built-in methods use (`NativeHost`).

use super::*;
use builtins::{Method, Receiver};

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
    let key_atom = unsafe { atom::static_atom(key, key_len) };
    let key = unsafe { name(key, key_len) };
    let a = unsafe { args(argv, argc) };

    if let Some(result) = run_builtin(recv, key, a) {
        return result;
    }

    // The array methods added to `Array.prototype`.
    if let Some((_, ObjectKind::Array)) = object(r)
        && let Some(method) = array_proto_method(key)
    {
        return ot_call(method, recv, argv, argc);
    }

    // `f.call(thisArg, ...args)`, `f.apply(thisArg, array)`
    if let Some((_, ObjectKind::Function)) = object(r)
        && (key == "call" || key == "apply")
    {
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

    let method = get_property_atom(r, key_atom);
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

/// `recv.key(...a)` when `key` is a built-in method of `recv`, the same
/// code the VM runs (`builtins`); `None` when it is not one.
fn run_builtin(recv: u64, key: &str, a: &[u64]) -> Option<u64> {
    let r = OtValue::from_bits(recv);
    if let Some((ptr, ObjectKind::Array)) = object(r) {
        let arr = array(ptr);
        match key {
            "push" => {
                let mut len = arr.len as usize;
                for &v in a {
                    len = array_push(arr, v);
                }
                return Some(number(len as f64));
            }
            "pop" => {
                if arr.len == 0 {
                    return Some(undefined());
                }
                arr.len -= 1;
                return Some(unsafe { *arr.elements.add(arr.len as usize) });
            }
            _ => {}
        }
        // The others work on a copy of the elements, written back after.
        let mut elems: Vec<u64> = (0..arr.len as usize)
            .map(|i| unsafe { *arr.elements.add(i) })
            .collect();
        let result = builtins::array_method(&NativeHost, &mut elems, key, a)?;
        array_reserve(arr, elems.len());
        for (i, v) in elems.iter().enumerate() {
            unsafe { *arr.elements.add(i) = *v };
        }
        arr.len = elems.len() as u32;
        return Some(outcome(result, recv));
    }
    if let Some(id) = promise_id(recv) {
        return promise_method(id, key, a);
    }
    match receiver_of(recv) {
        Some(Receiver::Map | Receiver::Set) => return collection_method(recv, key, a),
        Some(Receiver::MapIterator | Receiver::SetIterator) => {
            return (key == "next").then(|| iterator_next(recv));
        }
        _ => {}
    }
    if r.is_number() {
        let result = builtins::number_method(&NativeHost, r.as_number_unchecked(), key, a)?;
        return Some(outcome(result, recv));
    }
    let s = as_str(r)?;
    // Indexing: an ASCII string's flag saves scanning it.
    let result = builtins::string_index(&NativeHost, s, is_ascii(r), key, a)
        .or_else(|| builtins::string_method(&NativeHost, s, key, a))?;
    Some(outcome(result, recv))
}

/// The kind of value `v` is, if it has built-in methods.
pub(super) fn receiver_of(v: u64) -> Option<Receiver> {
    let r = OtValue::from_bits(v);
    if r.is_number() {
        return Some(Receiver::Number);
    }
    match object(r)? {
        (_, ObjectKind::String) => Some(Receiver::String),
        (_, ObjectKind::Array) => Some(Receiver::Array),
        (_, ObjectKind::Object) if promise_id(v).is_some() => Some(Receiver::Promise),
        (_, ObjectKind::Map) => Some(Receiver::Map),
        (_, ObjectKind::Set) => Some(Receiver::Set),
        (_, ObjectKind::Iterator) => iterator_receiver(v),
        _ => None,
    }
}

thread_local! {
    /// The function value of each built-in method, made when first read,
    /// so that `s.trim === t.trim`.
    static METHOD_VALUES: RefCell<HashMap<Method, u64>> = RefCell::new(HashMap::new());
}

/// The built-in method `name` of `receiver` as a function value, if it
/// has one.
pub(super) fn builtin_method(receiver: Receiver, name: &str) -> Option<u64> {
    let m = Method::find(receiver, name)?;
    Some(METHOD_VALUES.with(|values| {
        *values.borrow_mut().entry(m).or_insert_with(|| {
            ot_make_closure(ot_builtin_method as *const u8, number(m.to_bits() as f64))
        })
    }))
}

/// Whether the function at `ptr` is a built-in method (`builtin_method`),
/// which is not a constructor.
pub(super) fn is_builtin_method(ptr: HeapPtr) -> bool {
    unsafe { ptr.as_ref::<NativeFunction>() }.code == ot_builtin_method as *const u8
}

/// A built-in method called as a function value (`const trim = s.trim;
/// trim.call(t)`); `env` is the method (`Method::to_bits`). A `this` of
/// another kind is a TypeError, except that a string method converts it to
/// a string, as JavaScript does.
pub extern "C" fn ot_builtin_method(this: u64, argv: *const u64, argc: usize, env: u64) -> u64 {
    let bits = OtValue::from_bits(env).as_number_unchecked() as u32;
    let m = Method::from_bits(bits).expect("a built-in method's environment");
    let v = OtValue::from_bits(this);
    let nullish = v.is_null() || v.is_undefined();
    let mut this = this;
    if receiver_of(this) != Some(m.receiver()) {
        if m.receiver() != Receiver::String || nullish {
            let shown = builtins::receiver_shown(&NativeHost, &this);
            let e = m.receiver_error(nullish, &shown);
            return throw_error(e.kind, &e.message);
        }
        this = ot_to_str(this);
        if this == EXCEPTION_BITS {
            return this;
        }
    }
    let a = unsafe { args(argv, argc) };
    run_builtin(this, m.name(), a).expect("a listed method is implemented")
}

/// `__intrinsic_<name>(args)`: intrinsic `id`, as on every backend
/// (`runtime::intrinsics`).
#[unsafe(no_mangle)]
pub extern "C" fn ot_intrinsic(id: usize, argv: *const u64, argc: usize) -> u64 {
    let a = unsafe { args(argv, argc) };
    outcome(super::super::intrinsics::call(&NativeHost, id as u32, a), 0)
}

/// A type annotation checked when the program runs, where compiled code
/// could not tell on sight: `value` if its kind is in the set `kinds`
/// (`builtins::Kind::bit`), else a TypeError.
#[unsafe(no_mangle)]
pub extern "C" fn ot_check_type(value: u64, kinds: usize) -> u64 {
    let id = super::super::intrinsics::id("check_type").expect("the check_type intrinsic");
    let args = [value, number(kinds as f64)];
    outcome(super::super::intrinsics::call(&NativeHost, id, &args), 0)
}

/// `[..., ...source]`: append what `source` iterates (`iterate`) to the
/// array `target`; `target`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_array_spread(target: u64, source: u64) -> u64 {
    let Some((tptr, ObjectKind::Array)) = object(OtValue::from_bits(target)) else {
        return target;
    };
    let Some(items) = iterate(source) else {
        return not_iterable(source);
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

    fn kind(&self, v: &u64) -> builtins::Kind {
        use builtins::Kind;
        let v = OtValue::from_bits(*v);
        if v.is_number() {
            Kind::Number
        } else if v.is_boolean() {
            Kind::Boolean
        } else if v.is_undefined() {
            Kind::Undefined
        } else if v.is_null() {
            Kind::Null
        } else {
            match object(v) {
                Some((_, ObjectKind::String)) => Kind::String,
                Some((_, ObjectKind::Array)) => Kind::Array,
                Some((_, ObjectKind::Function)) => Kind::Function,
                Some((ptr, ObjectKind::Object)) if is_class(ptr) => Kind::Function,
                _ => Kind::Object,
            }
        }
    }

    fn own_keys(&self, v: &u64) -> Vec<String> {
        match object(OtValue::from_bits(*v)) {
            Some((ptr, ObjectKind::Object)) => object::own(obj(ptr))
                .into_iter()
                .map(|(a, _, _)| atom::info(a))
                .filter(|info| !info.hidden)
                .map(|info| info.name.to_string())
                .collect(),
            Some((ptr, ObjectKind::Array)) => (0..array(ptr).len).map(|i| i.to_string()).collect(),
            _ => Vec::new(),
        }
    }

    fn identity(&self, v: &u64) -> u64 {
        *v
    }

    fn receiver(&self, v: &u64) -> Option<Receiver> {
        receiver_of(*v)
    }

    fn own_entries(&self, v: &u64) -> Vec<(String, u64)> {
        match object(OtValue::from_bits(*v)) {
            Some((ptr, ObjectKind::Object)) => visible_data(ptr)
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
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
