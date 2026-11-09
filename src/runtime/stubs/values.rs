//! Values: making and reading NaN-boxed values, strings and conversions.

use super::*;

// =========================================================================
// Helpers
// =========================================================================

pub(super) fn undefined() -> u64 {
    OtValue::undefined().to_bits()
}

pub(super) fn boolean(b: bool) -> u64 {
    OtValue::boolean(b).to_bits()
}

pub(super) fn number(n: f64) -> u64 {
    OtValue::number(n).to_bits()
}

/// An unrecoverable condition (out of memory): report it and exit.
pub(super) fn fatal_error(msg: &str) -> ! {
    let _ = std::io::stdout().flush();
    eprintln!("Uncaught {}", msg);
    std::process::exit(1);
}

/// The heap object behind a value, with its kind.
pub(super) fn object(v: OtValue) -> Option<(HeapPtr, ObjectKind)> {
    let ptr = v.as_pointer()?;
    let kind = unsafe { ptr.as_ref::<ObjectHeader>().kind };
    Some((ptr, kind))
}

/// The string behind a value, if it is a string.
pub(super) fn as_str(v: OtValue) -> Option<&'static str> {
    match object(v)? {
        (ptr, ObjectKind::String) => Some(unsafe { ptr.as_ref::<NativeString>().as_str() }),
        _ => None,
    }
}

/// Whether `v` is a string known to be all ASCII (`ObjectHeader::ASCII`).
pub(super) fn is_ascii(v: OtValue) -> bool {
    match object(v) {
        Some((ptr, ObjectKind::String)) => {
            unsafe { ptr.as_ref::<NativeString>() }.header.flags & ObjectHeader::ASCII != 0
        }
        _ => false,
    }
}

pub(super) fn new_string(s: &str) -> u64 {
    match heap().alloc_string(s) {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

/// A name passed as pointer + length.
pub(super) unsafe fn name<'a>(ptr: *const u8, len: usize) -> &'a str {
    unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) }
}

/// The arguments passed to a native function.
pub(super) unsafe fn args<'a>(argv: *const u64, argc: usize) -> &'a [u64] {
    if argv.is_null() || argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    }
}

/// JavaScript truthiness.
pub(super) fn truthy(v: OtValue) -> bool {
    if v.is_falsy() {
        return false;
    }
    !matches!(as_str(v), Some(""))
}

/// JavaScript `ToNumber`, for primitives.
pub(super) fn to_number(v: OtValue) -> f64 {
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
pub(super) fn to_int32(v: OtValue) -> i32 {
    int32(to_number(v))
}

/// JavaScript `ToInt32` of a number: the integer part, modulo 2^32.
pub(super) fn int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
}

/// Number to string, as the language prints numbers.
pub fn number_to_string(n: f64) -> String {
    builtins::number_to_string(n)
}

/// `String(v)`: the conversion used by `+` with a string operand.
pub(super) fn value_to_string(v: OtValue) -> String {
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
        Some((ptr, ObjectKind::Object)) if is_error(ptr) => error_summary(ptr),
        _ => "[object Object]".to_string(),
    }
}
