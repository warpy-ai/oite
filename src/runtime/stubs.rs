//! Runtime stubs callable from native-compiled code
//!
//! These `extern "C"` functions are the runtime of Oite's native backends.
//! The Cranelift JIT calls them in-process; LLVM AOT binaries link them from
//! the static library that `build.rs` compiles from this same file. Keeping
//! the semantics here, instead of in each backend, is what makes the two
//! backends agree.
//!
//! The calling convention is:
//! - All values are passed and returned as `u64` (NaN-boxed `OtValue`).
//! - Every function value, compiled or built in, has the signature
//!   `extern "C" fn(this: u64, argv: *const u64, argc: usize) -> u64`
//!   (see `heap::NativeFn`). Missing arguments read as `undefined`.
//! - Names (properties, globals) are passed as a UTF-8 pointer and length.

use super::abi::OtValue;
use super::builtins;
use super::heap::{
    HeapPtr, NativeArray, NativeFn, NativeFunction, NativeObject, NativeString, ObjectHeader,
    ObjectKind, PropertyMap, heap,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;

// =========================================================================
// Helpers
// =========================================================================

fn undefined() -> u64 {
    OtValue::undefined().to_bits()
}

fn boolean(b: bool) -> u64 {
    OtValue::boolean(b).to_bits()
}

fn number(n: f64) -> u64 {
    OtValue::number(n).to_bits()
}

/// Report an uncaught runtime error and exit.
///
/// Native code has no exception support yet (#120), so this cannot be caught.
/// Output written so far is flushed first so it is not lost.
fn runtime_error(msg: &str) -> ! {
    let _ = std::io::stdout().flush();
    eprintln!("Uncaught {}", msg);
    std::process::exit(1);
}

/// The heap object behind a value, with its kind.
fn object(v: OtValue) -> Option<(HeapPtr, ObjectKind)> {
    let ptr = v.as_pointer()?;
    let kind = unsafe { ptr.as_ref::<ObjectHeader>().kind };
    Some((ptr, kind))
}

/// The string behind a value, if it is a string.
fn as_str(v: OtValue) -> Option<&'static str> {
    match object(v)? {
        (ptr, ObjectKind::String) => Some(unsafe { ptr.as_ref::<NativeString>().as_str() }),
        _ => None,
    }
}

fn new_string(s: &str) -> u64 {
    match heap().alloc_string(s) {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

/// A name passed as pointer + length.
unsafe fn name<'a>(ptr: *const u8, len: usize) -> &'a str {
    unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) }
}

/// The arguments passed to a native function.
unsafe fn args<'a>(argv: *const u64, argc: usize) -> &'a [u64] {
    if argv.is_null() || argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    }
}

/// JavaScript truthiness.
fn truthy(v: OtValue) -> bool {
    if v.is_falsy() {
        return false;
    }
    !matches!(as_str(v), Some(""))
}

/// JavaScript `ToNumber`, for primitives.
fn to_number(v: OtValue) -> f64 {
    if v.is_number() {
        return v.as_number_unchecked();
    }
    if v.is_boolean() {
        return if v.as_boolean_unchecked() { 1.0 } else { 0.0 };
    }
    if v.is_null() {
        return 0.0;
    }
    if let Some(s) = as_str(v) {
        let t = s.trim();
        if t.is_empty() {
            return 0.0;
        }
        return t.parse::<f64>().unwrap_or(f64::NAN);
    }
    f64::NAN
}

/// JavaScript `ToInt32`.
fn to_int32(v: OtValue) -> i32 {
    let n = to_number(v);
    if !n.is_finite() {
        return 0;
    }
    (n.trunc() as i64 as u64 as u32) as i32
}

/// Number to string, as the language prints numbers.
pub fn number_to_string(n: f64) -> String {
    builtins::number_to_string(n)
}

/// `String(v)`: the conversion used by `+` with a string operand.
fn value_to_string(v: OtValue) -> String {
    if v.is_number() {
        return number_to_string(v.as_number_unchecked());
    }
    if v.is_boolean() {
        return v.as_boolean_unchecked().to_string();
    }
    if v.is_null() {
        return "null".to_string();
    }
    if v.is_undefined() {
        return "undefined".to_string();
    }
    match object(v) {
        Some((ptr, ObjectKind::String)) => unsafe { ptr.as_ref::<NativeString>().as_str() }.into(),
        Some((ptr, ObjectKind::Array)) => {
            let arr = unsafe { ptr.as_ref::<NativeArray>() };
            (0..arr.len as usize)
                .map(|i| {
                    let e = OtValue::from_bits(unsafe { *arr.elements.add(i) });
                    if e.is_null() || e.is_undefined() {
                        String::new()
                    } else {
                        value_to_string(e)
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        }
        Some((_, ObjectKind::Function)) => "function".to_string(),
        _ => "[object Object]".to_string(),
    }
}

fn props(ptr: HeapPtr) -> &'static mut PropertyMap {
    let obj = unsafe { ptr.as_mut::<NativeObject>() };
    if obj.properties.is_null() {
        obj.properties = Box::into_raw(Box::new(PropertyMap::new()));
    }
    unsafe { &mut *obj.properties }
}

fn array(ptr: HeapPtr) -> &'static mut NativeArray {
    unsafe { ptr.as_mut::<NativeArray>() }
}

/// Make room for at least `needed` elements.
fn array_reserve(arr: &mut NativeArray, needed: usize) {
    use std::alloc::{Layout, realloc};
    if needed <= arr.capacity as usize {
        return;
    }
    let new_cap = needed.max(arr.capacity as usize * 2).max(4);
    let old = Layout::array::<u64>(arr.capacity as usize).unwrap();
    let new_size = Layout::array::<u64>(new_cap).unwrap().size();
    let p = unsafe { realloc(arr.elements as *mut u8, old, new_size) } as *mut u64;
    if p.is_null() {
        runtime_error("RangeError: out of memory growing an array");
    }
    arr.elements = p;
    arr.capacity = new_cap as u32;
}

fn array_set(arr: &mut NativeArray, idx: usize, value: u64) {
    array_reserve(arr, idx + 1);
    for i in arr.len as usize..idx {
        unsafe { *arr.elements.add(i) = undefined() };
    }
    unsafe { *arr.elements.add(idx) = value };
    if idx >= arr.len as usize {
        arr.len = (idx + 1) as u32;
    }
}

fn array_push(arr: &mut NativeArray, value: u64) -> usize {
    let len = arr.len as usize;
    array_set(arr, len, value);
    len + 1
}

/// Property lookup, following `__proto__`.
fn get_property(v: OtValue, key: &str) -> u64 {
    let Some((ptr, kind)) = object(v) else {
        if v.is_null() || v.is_undefined() {
            runtime_error(&format!(
                "TypeError: Cannot read properties of {} (reading '{}')",
                value_to_string(v),
                key
            ));
        }
        return undefined();
    };
    match kind {
        ObjectKind::Object => {
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

fn set_property(v: OtValue, key: &str, value: u64) {
    match object(v) {
        Some((ptr, ObjectKind::Object)) => {
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
                runtime_error(&format!(
                    "TypeError: Cannot set properties of {} (setting '{}')",
                    value_to_string(v),
                    key
                ));
            }
        }
    }
}

/// An index or key value as a property name.
fn key_string(key: OtValue) -> String {
    value_to_string(key)
}

/// A number key usable as an array index.
fn index_of(key: OtValue) -> Option<usize> {
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
pub extern "C" fn ot_set_prop(obj: u64, key: *const u8, key_len: usize, value: u64) {
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
pub extern "C" fn ot_set_index(obj: u64, key: u64, value: u64) {
    let o = OtValue::from_bits(obj);
    let k = OtValue::from_bits(key);
    if let (Some((ptr, ObjectKind::Array)), Some(i)) = (object(o), index_of(k)) {
        array_set(array(ptr), i, value);
        return;
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
        _ => runtime_error("TypeError: push called on a non-array"),
    }
}

// =========================================================================
// Globals
// =========================================================================

thread_local! {
    static GLOBALS: RefCell<HashMap<String, u64>> = RefCell::new(initial_globals());
}

fn builtin(code: NativeFn) -> u64 {
    ot_make_function(code as *const u8)
}

fn initial_globals() -> HashMap<String, u64> {
    let mut g = HashMap::new();
    let console = ot_alloc_object();
    let c = OtValue::from_bits(console);
    set_property(c, "log", builtin(ot_builtin_console_log));
    set_property(c, "error", builtin(ot_builtin_console_error));
    g.insert("console".to_string(), console);
    let object = ot_alloc_object();
    set_property(
        OtValue::from_bits(object),
        "keys",
        builtin(ot_builtin_object_keys),
    );
    g.insert("Object".to_string(), object);
    // `Math`: one built-in serves every function member; its environment
    // is the member's index in `MATH_FUNCTIONS`.
    let math = ot_alloc_object();
    let m = OtValue::from_bits(math);
    for (i, name) in builtins::MATH_FUNCTIONS.iter().enumerate() {
        let f = ot_make_closure(ot_builtin_math as *const u8, number(i as f64));
        set_property(m, name, f);
    }
    for (name, value) in builtins::MATH_CONSTANTS {
        set_property(m, name, number(*value));
    }
    g.insert("Math".to_string(), math);
    let date = ot_alloc_object();
    set_property(
        OtValue::from_bits(date),
        "now",
        builtin(ot_builtin_date_now),
    );
    g.insert("Date".to_string(), date);
    g.insert("undefined".to_string(), undefined());
    g.insert("NaN".to_string(), number(f64::NAN));
    g.insert("Infinity".to_string(), number(f64::INFINITY));
    g
}

/// Read a global variable; a `ReferenceError` if it was never set.
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_global(key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    match GLOBALS.with(|g| g.borrow().get(key).copied()) {
        Some(v) => v,
        None => runtime_error(&format!("ReferenceError: {key} is not defined")),
    }
}

/// `typeof name` of a global: "undefined" if it was never set.
#[unsafe(no_mangle)]
pub extern "C" fn ot_typeof_global(key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    match GLOBALS.with(|g| g.borrow().get(key).copied()) {
        Some(v) => ot_typeof(v),
        None => new_string("undefined"),
    }
}

/// Write a global variable.
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_global(key: *const u8, key_len: usize, value: u64) {
    let key = unsafe { name(key, key_len) };
    GLOBALS.with(|g| {
        g.borrow_mut().insert(key.to_string(), value);
    });
}

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
        _ => runtime_error(&format!(
            "TypeError: {} is not a function",
            inspect(OtValue::from_bits(func))
        )),
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
        if let Some(result) = builtins::string_method(&NativeHost, s, key, a) {
            return outcome(result, recv);
        }
    }

    let method = get_property(r, key);
    if let Some((_, ObjectKind::Function)) = object(OtValue::from_bits(method)) {
        return ot_call(method, recv, argv, argc);
    }
    runtime_error(&format!(
        "TypeError: {}.{} is not a function",
        inspect(r),
        key
    ))
}

/// The native value representation, for the shared built-in methods.
struct NativeHost;

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
fn outcome(result: Result<builtins::Outcome<u64>, builtins::Error>, recv: u64) -> u64 {
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
        Err(e) => runtime_error(&e.to_string()),
    }
}

// =========================================================================
// Operators
// =========================================================================

/// `a + b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_add_any(a: u64, b: u64) -> u64 {
    let va = OtValue::from_bits(a);
    let vb = OtValue::from_bits(b);
    if va.is_number() && vb.is_number() {
        return number(va.as_number_unchecked() + vb.as_number_unchecked());
    }
    let a_str =
        as_str(va).is_some() || matches!(object(va), Some((_, k)) if k != ObjectKind::String);
    let b_str =
        as_str(vb).is_some() || matches!(object(vb), Some((_, k)) if k != ObjectKind::String);
    if a_str || b_str {
        let mut s = value_to_string(va);
        s.push_str(&value_to_string(vb));
        return new_string(&s);
    }
    number(to_number(va) + to_number(vb))
}

fn numeric(a: u64, b: u64, f: impl Fn(f64, f64) -> f64) -> u64 {
    number(f(
        to_number(OtValue::from_bits(a)),
        to_number(OtValue::from_bits(b)),
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_sub_any(a: u64, b: u64) -> u64 {
    numeric(a, b, |x, y| x - y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_mul_any(a: u64, b: u64) -> u64 {
    numeric(a, b, |x, y| x * y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_div_any(a: u64, b: u64) -> u64 {
    numeric(a, b, |x, y| x / y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_mod_any(a: u64, b: u64) -> u64 {
    numeric(a, b, |x, y| x % y)
}

/// `a ** b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_pow(a: u64, b: u64) -> u64 {
    numeric(a, b, f64::powf)
}

/// `-a`
#[unsafe(no_mangle)]
pub extern "C" fn ot_neg(a: u64) -> u64 {
    number(-to_number(OtValue::from_bits(a)))
}

fn int_op(a: u64, b: u64, f: impl Fn(i32, i32) -> i32) -> u64 {
    number(f(
        to_int32(OtValue::from_bits(a)),
        to_int32(OtValue::from_bits(b)),
    ) as f64)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_and(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x & y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_or(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x | y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_xor(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x ^ y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_shl(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x.wrapping_shl((y & 31) as u32))
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_shr(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x.wrapping_shr((y & 31) as u32))
}

/// `a >>> b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_shr_u(a: u64, b: u64) -> u64 {
    let x = to_int32(OtValue::from_bits(a)) as u32;
    let y = (to_int32(OtValue::from_bits(b)) & 31) as u32;
    number(x.wrapping_shr(y) as f64)
}

/// Strict equality as a raw `bool`.
fn strict_equal(va: OtValue, vb: OtValue) -> bool {
    if va.is_number() && vb.is_number() {
        return va.as_number_unchecked() == vb.as_number_unchecked();
    }
    if let (Some(x), Some(y)) = (as_str(va), as_str(vb)) {
        return x == y;
    }
    va.to_bits() == vb.to_bits()
}

/// Loose equality (`==`) as a raw `bool`.
fn loose_equal(va: OtValue, vb: OtValue) -> bool {
    let nullish = |v: OtValue| v.is_null() || v.is_undefined();
    if nullish(va) || nullish(vb) {
        return nullish(va) && nullish(vb);
    }
    let primitive = |v: OtValue| v.is_number() || v.is_boolean() || as_str(v).is_some();
    if primitive(va) && primitive(vb) {
        if let (Some(x), Some(y)) = (as_str(va), as_str(vb)) {
            return x == y;
        }
        return to_number(va) == to_number(vb);
    }
    strict_equal(va, vb)
}

/// `a === b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_eq_strict(a: u64, b: u64) -> u64 {
    boolean(strict_equal(OtValue::from_bits(a), OtValue::from_bits(b)))
}

/// `a == b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_eq_loose(a: u64, b: u64) -> u64 {
    boolean(loose_equal(OtValue::from_bits(a), OtValue::from_bits(b)))
}

fn compare(a: u64, b: u64, f: impl Fn(std::cmp::Ordering) -> bool) -> u64 {
    let va = OtValue::from_bits(a);
    let vb = OtValue::from_bits(b);
    if let (Some(x), Some(y)) = (as_str(va), as_str(vb)) {
        return boolean(f(x.cmp(y)));
    }
    match to_number(va).partial_cmp(&to_number(vb)) {
        Some(ord) => boolean(f(ord)),
        None => boolean(false), // NaN compares false
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_lt(a: u64, b: u64) -> u64 {
    compare(a, b, |o| o.is_lt())
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_gt(a: u64, b: u64) -> u64 {
    compare(a, b, |o| o.is_gt())
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_lte(a: u64, b: u64) -> u64 {
    compare(a, b, |o| o.is_le())
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_gte(a: u64, b: u64) -> u64 {
    compare(a, b, |o| o.is_ge())
}

/// `!a`
#[unsafe(no_mangle)]
pub extern "C" fn ot_not(a: u64) -> u64 {
    boolean(!truthy(OtValue::from_bits(a)))
}

/// Truthiness as a raw `0` / `1`, for branches.
#[unsafe(no_mangle)]
pub extern "C" fn ot_truthy(a: u64) -> u64 {
    truthy(OtValue::from_bits(a)) as u64
}

/// `typeof a`
#[unsafe(no_mangle)]
pub extern "C" fn ot_typeof(a: u64) -> u64 {
    let v = OtValue::from_bits(a);
    let t = if v.is_number() {
        "number"
    } else if v.is_boolean() {
        "boolean"
    } else if v.is_undefined() {
        "undefined"
    } else {
        match object(v) {
            Some((_, ObjectKind::String)) => "string",
            Some((_, ObjectKind::Function)) => "function",
            _ => "object",
        }
    };
    new_string(t)
}

/// `Boolean(a)`
#[unsafe(no_mangle)]
pub extern "C" fn ot_to_boolean(a: u64) -> u64 {
    boolean(truthy(OtValue::from_bits(a)))
}

/// `Number(a)`
#[unsafe(no_mangle)]
pub extern "C" fn ot_to_number(a: u64) -> u64 {
    number(to_number(OtValue::from_bits(a)))
}

/// `new.target` (not tracked natively yet).
#[unsafe(no_mangle)]
pub extern "C" fn ot_new_target() -> u64 {
    undefined()
}

/// `obj instanceof ctor`: is `ctor.prototype` on `obj`'s `__proto__` chain?
#[unsafe(no_mangle)]
pub extern "C" fn ot_instanceof(obj: u64, ctor: u64) -> u64 {
    let proto = get_property(OtValue::from_bits(ctor), "prototype");
    let Some(target) = OtValue::from_bits(proto).as_pointer() else {
        return boolean(false);
    };
    let mut cur = OtValue::from_bits(obj);
    for _ in 0..1000 {
        let Some((ptr, ObjectKind::Object)) = object(cur) else {
            break;
        };
        let next = props(ptr)
            .iter()
            .find(|(k, _)| k == "__proto__")
            .map(|(_, v)| OtValue::from_bits(*v));
        match next {
            Some(p) if p.as_pointer() == Some(target) => return boolean(true),
            Some(p) => cur = p,
            None => break,
        }
    }
    boolean(false)
}

/// Flush buffered output. Called when a native program exits.
#[unsafe(no_mangle)]
pub extern "C" fn ot_runtime_exit() {
    let _ = std::io::stdout().flush();
}

// =========================================================================
// Console
// =========================================================================

/// Nesting depth beyond which objects print as `[Object]` / `[Array]`.
const INSPECT_DEPTH: usize = 2;

/// Human-readable rendering of a value, in the style of Node's
/// `util.inspect`. Must stay identical to `stdlib::inspect` in the VM, so
/// `console.log` prints the same on every backend: `[ 1, 2, 3 ]`,
/// `{ x: 1, y: 's' }`, `[Circular]`. Object keys are sorted, and keys
/// starting with `__` are internal and hidden.
pub fn inspect(v: OtValue) -> String {
    let mut seen = Vec::new();
    inspect_inner(v, 0, &mut seen)
}

fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn quote(s: &str) -> String {
    format!(
        "'{}'",
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
    )
}

fn inspect_inner(v: OtValue, depth: usize, seen: &mut Vec<usize>) -> String {
    let Some((ptr, kind)) = object(v) else {
        return value_to_string(v);
    };
    match kind {
        ObjectKind::String => quote(unsafe { ptr.as_ref::<NativeString>().as_str() }),
        ObjectKind::Function => "[Function]".to_string(),
        ObjectKind::ByteStream => "ByteStream".to_string(),
        ObjectKind::Array | ObjectKind::Object => {
            let addr = ptr.as_usize();
            if seen.contains(&addr) {
                return "[Circular]".to_string();
            }
            let too_deep = depth > INSPECT_DEPTH;
            seen.push(addr);
            let out = if kind == ObjectKind::Array {
                let arr = array(ptr);
                if arr.len == 0 {
                    "[]".to_string()
                } else if too_deep {
                    "[Array]".to_string()
                } else {
                    let parts: Vec<String> = (0..arr.len as usize)
                        .map(|i| {
                            let e = OtValue::from_bits(unsafe { *arr.elements.add(i) });
                            inspect_inner(e, depth + 1, seen)
                        })
                        .collect();
                    format!("[ {} ]", parts.join(", "))
                }
            } else {
                let map = props(ptr);
                let mut keys: Vec<&(String, u64)> =
                    map.iter().filter(|(k, _)| !k.starts_with("__")).collect();
                keys.sort_by(|a, b| a.0.cmp(&b.0));
                if keys.is_empty() {
                    "{}".to_string()
                } else if too_deep {
                    "[Object]".to_string()
                } else {
                    let parts: Vec<String> = keys
                        .into_iter()
                        .map(|(k, bits)| {
                            let key = if is_identifier(k) {
                                k.clone()
                            } else {
                                quote(k)
                            };
                            let val = inspect_inner(OtValue::from_bits(*bits), depth + 1, seen);
                            format!("{}: {}", key, val)
                        })
                        .collect();
                    format!("{{ {} }}", parts.join(", "))
                }
            };
            seen.pop();
            out
        }
    }
}

/// `console.log` arguments joined by a space; top-level strings print raw.
fn format_log_args(a: &[u64]) -> String {
    a.iter()
        .map(|&bits| {
            let v = OtValue::from_bits(bits);
            match as_str(v) {
                Some(s) => s.to_string(),
                None => inspect(v),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `console.log(...)`
pub extern "C" fn ot_builtin_console_log(
    _this: u64,
    argv: *const u64,
    argc: usize,
    _env: u64,
) -> u64 {
    let line = format_log_args(unsafe { args(argv, argc) });
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", line);
    undefined()
}

/// `console.error(...)`
pub extern "C" fn ot_builtin_console_error(
    _this: u64,
    argv: *const u64,
    argc: usize,
    _env: u64,
) -> u64 {
    let line = format_log_args(unsafe { args(argv, argc) });
    let _ = std::io::stdout().flush();
    eprintln!("{}", line);
    undefined()
}

/// A `Math` function; `env` is its index in `MATH_FUNCTIONS`.
pub extern "C" fn ot_builtin_math(_this: u64, argv: *const u64, argc: usize, env: u64) -> u64 {
    let a = unsafe { args(argv, argc) };
    let nums: Vec<f64> = a
        .iter()
        .map(|v| {
            let v = OtValue::from_bits(*v);
            if object(v).is_some() && as_str(v).is_none() {
                to_number(OtValue::from_bits(new_string(&value_to_string(v))))
            } else {
                to_number(v)
            }
        })
        .collect();
    let id = OtValue::from_bits(env).as_number_unchecked() as usize;
    let name = builtins::MATH_FUNCTIONS.get(id).copied().unwrap_or("");
    number(builtins::math_call(name, &nums))
}

/// `Date.now()`
pub extern "C" fn ot_builtin_date_now(
    _this: u64,
    _argv: *const u64,
    _argc: usize,
    _env: u64,
) -> u64 {
    number(builtins::date_now())
}

/// `Object.keys(obj)`: own property names, in insertion order. (`for...in`
/// compiles to this.)
pub extern "C" fn ot_builtin_object_keys(
    _this: u64,
    argv: *const u64,
    argc: usize,
    _env: u64,
) -> u64 {
    let a = unsafe { args(argv, argc) };
    let out = ot_alloc_array(0);
    let Some((out_ptr, _)) = object(OtValue::from_bits(out)) else {
        return out;
    };
    let target = a.first().map(|v| OtValue::from_bits(*v));
    match target.and_then(object) {
        Some((ptr, ObjectKind::Object)) => {
            let names: Vec<String> = props(ptr)
                .iter()
                .filter(|(k, _)| !k.starts_with("__") && !k.starts_with('%'))
                .map(|(k, _)| k.clone())
                .collect();
            for k in names {
                array_push(array(out_ptr), new_string(&k));
            }
        }
        Some((ptr, ObjectKind::Array)) => {
            let len = array(ptr).len;
            for i in 0..len {
                array_push(array(out_ptr), new_string(&i.to_string()));
            }
        }
        _ => {}
    }
    out
}

/// Print one value, as `console.log(value)` does.
#[unsafe(no_mangle)]
pub extern "C" fn ot_console_log(value: u64) {
    ot_builtin_console_log(undefined(), &value, 1, undefined());
}

// =========================================================================
// File system (used by the self-hosted compiler)
// =========================================================================

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_exists(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                return OtValue::boolean(std::path::Path::new(path_str).exists()).to_bits();
            }
        }
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_read_file(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(content) = std::fs::read_to_string(path_str)
                    && let Some(ptr) = heap().alloc_string(&content)
                {
                    return OtValue::pointer(ptr).to_bits();
                }
            }
        }
    }
    OtValue::undefined().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_write_file(path: u64, content: u64) -> u64 {
    let path_val = OtValue::from_bits(path);
    let content_val = OtValue::from_bits(content);

    let (path_str, content_str) = unsafe {
        let p = path_val.as_pointer().and_then(|ptr| {
            if ptr.as_ref::<ObjectHeader>().kind == ObjectKind::String {
                Some(ptr.as_ref::<NativeString>().as_str().to_string())
            } else {
                None
            }
        });
        let c = content_val.as_pointer().and_then(|ptr| {
            if ptr.as_ref::<ObjectHeader>().kind == ObjectKind::String {
                Some(ptr.as_ref::<NativeString>().as_str().to_string())
            } else {
                None
            }
        });
        (p, c)
    };

    if let (Some(p), Some(c)) = (path_str, content_str) {
        return OtValue::boolean(std::fs::write(&p, &c).is_ok()).to_bits();
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_readdir(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(entries) = std::fs::read_dir(path_str) {
                    let names: Vec<String> = entries
                        .flatten()
                        .filter_map(|e| e.file_name().to_str().map(String::from))
                        .collect();
                    if let Some(arr_ptr) = heap().alloc_array(names.len()) {
                        let arr = arr_ptr.as_mut::<NativeArray>();
                        for (i, name) in names.iter().enumerate() {
                            let str_val = heap()
                                .alloc_string(name)
                                .map(|p| OtValue::pointer(p).to_bits())
                                .unwrap_or(OtValue::undefined().to_bits());
                            *arr.elements.add(i) = str_val;
                        }
                        arr.len = names.len() as u32;
                        return OtValue::pointer(arr_ptr).to_bits();
                    }
                }
                if let Some(arr_ptr) = heap().alloc_array(0) {
                    return OtValue::pointer(arr_ptr).to_bits();
                }
            }
        }
    }
    OtValue::undefined().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_stat(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(metadata) = std::fs::metadata(path_str)
                    && let Some(obj_ptr) = heap().alloc_object()
                {
                    let obj = obj_ptr.as_mut::<NativeObject>();
                    if obj.properties.is_null() {
                        obj.properties = Box::into_raw(Box::new(PropertyMap::new()));
                    }
                    let props = &mut *obj.properties;
                    props.push((
                        "__isDir".to_string(),
                        OtValue::boolean(metadata.is_dir()).to_bits(),
                    ));
                    props.push((
                        "size".to_string(),
                        OtValue::number(metadata.len() as f64).to_bits(),
                    ));
                    return OtValue::pointer(obj_ptr).to_bits();
                }
            }
        }
    }
    OtValue::null().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_stat_is_directory(stat: u64) -> u64 {
    let val = OtValue::from_bits(stat);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::Object {
                let obj = ptr.as_ref::<NativeObject>();
                if !obj.properties.is_null() {
                    for (key, value) in (*obj.properties).iter() {
                        if key == "__isDir" {
                            return *value;
                        }
                    }
                }
            }
        }
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_mkdir(path: u64, recursive: u64) -> u64 {
    let val = OtValue::from_bits(path);
    let is_recursive = !OtValue::from_bits(recursive).is_falsy();

    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                let result = if is_recursive {
                    std::fs::create_dir_all(path_str)
                } else {
                    std::fs::create_dir(path_str)
                };
                return OtValue::boolean(result.is_ok()).to_bits();
            }
        }
    }
    OtValue::boolean(false).to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_log() {
        ot_console_log(OtValue::number(42.0).to_bits());
        ot_console_log(OtValue::boolean(true).to_bits());
        ot_console_log(OtValue::null().to_bits());
    }

    #[test]
    fn test_arithmetic_stubs() {
        let a = OtValue::number(10.0).to_bits();
        let b = OtValue::number(3.0).to_bits();

        assert_eq!(OtValue::from_bits(ot_add_any(a, b)).as_number(), Some(13.0));
        assert_eq!(OtValue::from_bits(ot_sub_any(a, b)).as_number(), Some(7.0));
        assert_eq!(OtValue::from_bits(ot_mul_any(a, b)).as_number(), Some(30.0));

        let div = OtValue::from_bits(ot_div_any(a, b)).as_number().unwrap();
        assert!((div - 3.333333).abs() < 0.001);
    }

    #[test]
    fn test_object_property() {
        let obj_bits = ot_alloc_object();
        assert!(!OtValue::from_bits(obj_bits).is_undefined());

        let key = "foo";
        let value = OtValue::number(42.0).to_bits();

        ot_set_prop(obj_bits, key.as_ptr(), key.len(), value);
        let retrieved = ot_get_prop(obj_bits, key.as_ptr(), key.len());

        assert_eq!(OtValue::from_bits(retrieved).as_number(), Some(42.0));
    }
}
