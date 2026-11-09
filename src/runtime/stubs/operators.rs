//! Operators: arithmetic, bitwise, equality, comparison, `typeof`.

use super::*;

// =========================================================================
// Operators
// =========================================================================

/// `a + b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_add_any(a: u64, b: u64) -> u64 {
    let mut va = OtValue::from_bits(a);
    let mut vb = OtValue::from_bits(b);
    if va.is_number() && vb.is_number() {
        return number(va.as_number_unchecked() + vb.as_number_unchecked());
    }
    // An object with its own `valueOf` / `toString` converts through it.
    for v in [&mut va, &mut vb] {
        match to_primitive(*v, false) {
            Ok(Some(p)) => *v = p,
            Ok(None) => {}
            Err(e) => return e,
        }
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

/// `a op b` for `-`, `*`, `/`, `%` and `**`: both operands as numbers
/// (`ToNumber`, through `valueOf` for an object, which may throw).
pub(super) fn numeric(a: u64, b: u64, f: impl Fn(f64, f64) -> f64) -> u64 {
    let x = match number_operand(a) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let y = match number_operand(b) {
        Ok(y) => y,
        Err(e) => return e,
    };
    number(f(x, y))
}

/// `ToNumber(v)`; `Err` is the exception a `valueOf` threw.
pub(super) fn number_operand(v: u64) -> Result<f64, u64> {
    let v = OtValue::from_bits(v);
    if v.is_number() {
        return Ok(v.as_number_unchecked());
    }
    let p = to_primitive(v, false)?.unwrap_or(v);
    Ok(if object(p).is_some() && as_str(p).is_none() {
        to_number(OtValue::from_bits(new_string(&value_to_string(p))))
    } else {
        to_number(p)
    })
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

/// A bitwise operator: both operands as 32-bit integers (`ToNumber`,
/// through `valueOf` for an object, which may throw).
pub(super) fn int_op(a: u64, b: u64, f: impl Fn(i32, i32) -> f64) -> u64 {
    let x = match number_operand(a) {
        Ok(x) => int32(x),
        Err(e) => return e,
    };
    let y = match number_operand(b) {
        Ok(y) => int32(y),
        Err(e) => return e,
    };
    number(f(x, y))
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_and(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| (x & y) as f64)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_or(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| (x | y) as f64)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_xor(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| (x ^ y) as f64)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_shl(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x.wrapping_shl((y & 31) as u32) as f64)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_shr(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x.wrapping_shr((y & 31) as u32) as f64)
}

/// `a >>> b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_shr_u(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| (x as u32).wrapping_shr((y & 31) as u32) as f64)
}

/// Strict equality as a raw `bool`.
pub(super) fn strict_equal(va: OtValue, vb: OtValue) -> bool {
    if va.is_number() && vb.is_number() {
        return va.as_number_unchecked() == vb.as_number_unchecked();
    }
    if let (Some(x), Some(y)) = (as_str(va), as_str(vb)) {
        return x == y;
    }
    va.to_bits() == vb.to_bits()
}

/// Loose equality (`==`) as a raw `bool`.
pub(super) fn loose_equal(va: OtValue, vb: OtValue) -> bool {
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

/// `a < b` and the others: strings compare as strings, anything else as
/// numbers, after `ToPrimitive` (whose `valueOf` may throw).
pub(super) fn compare(a: u64, b: u64, f: impl Fn(std::cmp::Ordering) -> bool) -> u64 {
    let va = OtValue::from_bits(a);
    let vb = OtValue::from_bits(b);
    if let (Some(x), Some(y)) = (va.as_number(), vb.as_number()) {
        return boolean(x.partial_cmp(&y).is_some_and(f));
    }
    let va = match to_primitive(va, false) {
        Ok(p) => p.unwrap_or(va),
        Err(e) => return e,
    };
    let vb = match to_primitive(vb, false) {
        Ok(p) => p.unwrap_or(vb),
        Err(e) => return e,
    };
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
            Some((ptr, ObjectKind::Object)) if is_class(ptr) => "function",
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

/// Flush buffered output. Called when a native program exits.
#[unsafe(no_mangle)]
pub extern "C" fn ot_runtime_exit() {
    let _ = std::io::stdout().flush();
}
