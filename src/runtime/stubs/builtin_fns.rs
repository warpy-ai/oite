//! Built-in functions: `console`, `Math`, `String`, `Date.now`,
//! `Object.keys`, and conversion to a primitive.

use super::*;

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
    let id = OtValue::from_bits(env).as_number_unchecked() as usize;
    ot_math_call(id, argv, argc)
}

/// `Math.<f>(args)`, `f` an index in `MATH_FUNCTIONS`: what native code
/// calls when an argument is not a number (`valueOf` may throw).
#[unsafe(no_mangle)]
pub extern "C" fn ot_math_call(id: usize, argv: *const u64, argc: usize) -> u64 {
    let a = unsafe { args(argv, argc) };
    let mut nums = Vec::with_capacity(a.len());
    for v in a {
        match number_operand(*v) {
            Ok(n) => nums.push(n),
            Err(e) => return e,
        }
    }
    let name = builtins::MATH_FUNCTIONS.get(id).copied().unwrap_or("");
    number(builtins::math_call(name, &nums))
}

/// `String(value)`
pub extern "C" fn ot_builtin_string(_this: u64, argv: *const u64, argc: usize, _env: u64) -> u64 {
    let Some(v) = unsafe { args(argv, argc) }.first() else {
        return new_string("");
    };
    let v = match to_primitive(OtValue::from_bits(*v), true) {
        Ok(Some(p)) => p,
        Ok(None) => OtValue::from_bits(*v),
        Err(e) => return e,
    };
    new_string(&value_to_string(v))
}

/// `String(v)`, as in a template literal.
#[unsafe(no_mangle)]
pub extern "C" fn ot_to_str(v: u64) -> u64 {
    let value = OtValue::from_bits(v);
    if as_str(value).is_some() {
        return v;
    }
    let value = match to_primitive(value, true) {
        Ok(Some(p)) => p,
        Ok(None) => value,
        Err(e) => return e,
    };
    new_string(&value_to_string(value))
}

/// JavaScript `ToPrimitive` for an object with a compiled `valueOf` or
/// `toString` (`prefer_string`: `toString` first). `Ok(None)`: no such
/// method; `Err`: it threw (`EXCEPTION_BITS`).
pub(super) fn to_primitive(v: OtValue, prefer_string: bool) -> Result<Option<OtValue>, u64> {
    let Some((ptr, ObjectKind::Object)) = object(v) else {
        return Ok(None);
    };
    let order = if prefer_string {
        ["toString", "valueOf"]
    } else {
        ["valueOf", "toString"]
    };
    for name in order {
        let Some(method) = lookup(ptr, name) else {
            continue;
        };
        if !matches!(
            object(OtValue::from_bits(method)),
            Some((_, ObjectKind::Function))
        ) {
            continue;
        }
        let result = ot_call(method, v.to_bits(), std::ptr::null(), 0);
        if result == EXCEPTION_BITS {
            return Err(result);
        }
        let r = OtValue::from_bits(result);
        if !matches!(object(r), Some((_, kind)) if kind != ObjectKind::String) {
            return Ok(Some(r));
        }
    }
    Ok(None)
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
    use builtins::Host;
    let a = unsafe { args(argv, argc) };
    let keys = NativeHost.own_keys(&a.first().copied().unwrap_or_else(undefined));
    outcome(
        Ok(builtins::Outcome::NewArray(
            keys.into_iter().map(|k| new_string(&k)).collect(),
        )),
        0,
    )
}

/// Print one value, as `console.log(value)` does.
#[unsafe(no_mangle)]
pub extern "C" fn ot_console_log(value: u64) {
    ot_builtin_console_log(undefined(), &value, 1, undefined());
}
