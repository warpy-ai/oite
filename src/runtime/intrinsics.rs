//! Intrinsics: the operations core provides to the rolls.
//!
//! A roll declares one and calls it like a function:
//!
//! ```text
//! declare function __intrinsic_sqrt(x: number): number;
//! export function sqrt(x: number): number { return __intrinsic_sqrt(x); }
//! ```
//!
//! The compiler turns the call into one `Intrinsic` instruction, and every
//! backend runs the function below with its own value representation (see
//! `builtins::Host`), so an intrinsic gives the same result on the VM, the
//! JIT and native binaries. An intrinsic is a pure computation: it never asks
//! the operating system (see `docs/docs/platform-architecture.md`).
//!
//! The list is the contract between core and the rolls, versioned with the
//! release: adding an intrinsic is a minor change; changing or removing one is
//! a breaking change. Arguments a call leaves out are `undefined`.

use super::builtins::{self, Error, Host, Outcome};

/// The intrinsics, by name without the `__intrinsic_` prefix. An
/// intrinsic's id is its index here.
pub const INTRINSICS: &[&str] = &[
    // Math
    "floor",
    "ceil",
    "round",
    "sqrt",
    "pow",
    "exp",
    "log",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "fround",
    "imul",
    "random",
    // Numbers
    "parse_int",
    "parse_float",
    "to_fixed",
    "to_precision",
    "to_exponential",
    // Strings
    "string_length",
    "char_at",
    "char_code_at",
    "from_char_code",
    // Values
    "is_array",
    "object_keys",
    // Target
    "platform",
];

/// The prefix that marks a call as an intrinsic.
pub const PREFIX: &str = "__intrinsic_";

/// The id of `__intrinsic_<name>`, given `<name>`.
pub fn id(name: &str) -> Option<u32> {
    INTRINSICS.iter().position(|n| *n == name).map(|i| i as u32)
}

/// Run intrinsic `id` on `args`.
pub fn call<H: Host>(h: &H, id: u32, args: &[H::Value]) -> Result<Outcome<H::Value>, Error> {
    let name = INTRINSICS.get(id as usize).copied().unwrap_or("");
    let undefined = h.undefined();
    let arg = |i: usize| args.get(i).unwrap_or(&undefined);
    let num = |i: usize| h.number(arg(i)).unwrap_or(f64::NAN);
    let text = |i: usize| -> String {
        let v = arg(i);
        match h.string(v) {
            Some(s) => s.to_string(),
            None => h.to_string(v),
        }
    };
    let is_undefined = |i: usize| h.strict_equal(arg(i), &undefined);
    let number = |n: f64| Ok(Outcome::Value(h.make_number(n)));
    let string = |s: String| Ok(Outcome::Value(h.make_string(s)));
    // A method of the receiver `args[0]` with the rest of the arguments.
    let string_method = |method: &str| {
        let s = text(0);
        builtins::string_method(h, &s, method, args.get(1..).unwrap_or(&[]))
            .expect("a string method")
    };

    match name {
        "floor" | "ceil" | "round" | "sqrt" | "pow" | "exp" | "log" | "sin" | "cos" | "tan"
        | "asin" | "acos" | "atan" | "atan2" | "random" => {
            number(builtins::math_call(name, &[num(0), num(1)]))
        }
        "fround" => number(num(0) as f32 as f64),
        "imul" => number(to_int32(num(0)).wrapping_mul(to_int32(num(1))) as f64),
        "parse_int" => number(parse_int(&text(0), num(1))),
        "parse_float" => number(parse_float(&text(0))),
        "to_fixed" => builtins::number_method(h, num(0), "toFixed", args.get(1..).unwrap_or(&[]))
            .expect("a number method"),
        "to_precision" => {
            let x = num(0);
            if is_undefined(1) || !x.is_finite() {
                return string(builtins::number_to_string(x));
            }
            let p = integer(num(1));
            if !(1.0..=100.0).contains(&p) {
                return Err(Error::range(
                    "toPrecision() argument must be between 1 and 100",
                ));
            }
            string(to_precision(x, p as usize))
        }
        "to_exponential" => {
            let x = num(0);
            let f = integer(num(1));
            if !x.is_finite() {
                return string(builtins::number_to_string(x));
            }
            if is_undefined(1) {
                return string(to_exponential(x, None));
            }
            if !(0.0..=100.0).contains(&f) {
                return Err(Error::range(
                    "toExponential() argument must be between 0 and 100",
                ));
            }
            string(to_exponential(x, Some(f as usize)))
        }
        "string_length" => number(text(0).chars().count() as f64),
        "char_at" => string_method("charAt"),
        "char_code_at" => string_method("charCodeAt"),
        "from_char_code" => {
            let unit = to_int32(num(0)) as u32 & 0xFFFF;
            string(char::from_u32(unit).unwrap_or('\u{FFFD}').to_string())
        }
        "is_array" => Ok(Outcome::Value(h.make_bool(h.is_array(arg(0))))),
        "object_keys" => Ok(Outcome::NewArray(
            h.own_keys(arg(0))
                .into_iter()
                .map(|k| h.make_string(k))
                .collect(),
        )),
        "platform" => string(std::env::consts::OS.to_string()),
        _ => Err(Error {
            kind: "ReferenceError",
            message: format!("unknown intrinsic #{id}"),
        }),
    }
}

/// `ToIntegerOrInfinity`: NaN is 0, otherwise truncated.
fn integer(n: f64) -> f64 {
    if n.is_nan() { 0.0 } else { n.trunc() }
}

/// JavaScript `ToInt32`: the integer the bitwise operators work on.
fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
}

/// JavaScript's white space and line terminators, which `parseInt` and
/// `parseFloat` skip.
fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

/// `parseInt(text, radix)`.
fn parse_int(text: &str, radix: f64) -> f64 {
    let s = text.trim_start_matches(is_js_space);
    let (negative, s) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut radix = to_int32(radix);
    let mut strip_prefix = true;
    if radix != 0 {
        if !(2..=36).contains(&radix) {
            return f64::NAN;
        }
        if radix != 16 {
            strip_prefix = false;
        }
    } else {
        radix = 10;
    }
    let mut s = s;
    if strip_prefix && (s.starts_with("0x") || s.starts_with("0X")) {
        s = &s[2..];
        radix = 16;
    }
    let mut value = 0.0;
    let mut any = false;
    for c in s.chars() {
        match c.to_digit(radix as u32) {
            Some(d) => {
                value = value * radix as f64 + d as f64;
                any = true;
            }
            None => break,
        }
    }
    if !any {
        return f64::NAN;
    }
    if negative { -value } else { value }
}

/// `parseFloat(text)`: the longest prefix that is a decimal number.
fn parse_float(text: &str) -> f64 {
    let s = text.trim_start_matches(is_js_space);
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    if s[i..].starts_with("Infinity") {
        return if b.first() == Some(&b'-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    let mut any = digits(&mut i);
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        if digits(&mut j) || any {
            any = true;
            i = j;
        }
    }
    if !any {
        return f64::NAN;
    }
    // An exponent counts only with at least one digit.
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if digits(&mut j) {
            i = j;
        }
    }
    s[..i].parse::<f64>().unwrap_or(f64::NAN)
}

/// `|x|` (finite, not zero) rounded to `p` significant digits: the digits
/// and the exponent of the first one (`d.ddd × 10^e`). A tie rounds to the
/// larger value, as JavaScript does.
fn round_significant(x: f64, p: usize) -> (Vec<u8>, i32) {
    // Rust prints the exact decimal value of a double given enough digits.
    let exact = format!("{:.*e}", 1100, x.abs());
    let (mantissa, exp) = exact.split_once('e').unwrap_or((&exact, "0"));
    let all: Vec<u8> = mantissa.bytes().filter(|c| *c != b'.').collect();
    let mut e: i32 = exp.parse().unwrap_or(0);
    let mut digits = all[..p].to_vec();
    if all.get(p).is_some_and(|d| *d >= b'5') {
        let mut i = p;
        loop {
            if i == 0 {
                // 9.99 rounded up to 10.0.
                digits.insert(0, b'1');
                digits.truncate(p);
                e += 1;
                break;
            }
            i -= 1;
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    (digits, e)
}

fn exponent(e: i32) -> String {
    format!("e{}{}", if e < 0 { '-' } else { '+' }, e.abs())
}

/// `x.toPrecision(p)`, for a finite `x` and `p` in 1..=100.
fn to_precision(x: f64, p: usize) -> String {
    let (digits, e) = if x == 0.0 {
        (vec![b'0'; p], 0)
    } else {
        round_significant(x, p)
    };
    let d = String::from_utf8(digits).unwrap_or_default();
    let body = if e < -6 || e >= p as i32 {
        let rest = if p > 1 {
            format!(".{}", &d[1..])
        } else {
            String::new()
        };
        format!("{}{rest}{}", &d[..1], exponent(e))
    } else if e == p as i32 - 1 {
        d
    } else if e >= 0 {
        let point = e as usize + 1;
        format!("{}.{}", &d[..point], &d[point..])
    } else {
        format!("0.{}{d}", "0".repeat((-(e + 1)) as usize))
    };
    if x < 0.0 { format!("-{body}") } else { body }
}

/// `x.toExponential(f)`, for a finite `x`; `None` gives as many digits as it
/// takes to tell `x` apart from every other double.
fn to_exponential(x: f64, fraction: Option<usize>) -> String {
    let (d, e) = match fraction {
        _ if x == 0.0 => ("0".repeat(fraction.unwrap_or(0) + 1), 0),
        Some(f) => {
            let (digits, e) = round_significant(x, f + 1);
            (String::from_utf8(digits).unwrap_or_default(), e)
        }
        None => {
            let shortest = format!("{:e}", x.abs());
            let (mantissa, exp) = shortest.split_once('e').unwrap_or((&shortest, "0"));
            (mantissa.replace('.', ""), exp.parse().unwrap_or(0))
        }
    };
    let rest = if d.len() > 1 {
        format!(".{}", &d[1..])
    } else {
        String::new()
    };
    let body = format!("{}{rest}{}", &d[..1], exponent(e));
    if x < 0.0 { format!("-{body}") } else { body }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precision_and_exponential_match_javascript() {
        assert_eq!(to_precision(123.456, 4), "123.5");
        assert_eq!(to_precision(0.000123, 2), "0.00012");
        assert_eq!(to_precision(123456.0, 2), "1.2e+5");
        assert_eq!(to_precision(2.5, 1), "3");
        assert_eq!(to_precision(-1.25, 2), "-1.3");
        assert_eq!(to_precision(99.99, 3), "100");
        assert_eq!(to_precision(0.0, 3), "0.00");
        assert_eq!(to_precision(1e-7, 1), "1e-7");
        assert_eq!(to_exponential(123456.0, Some(2)), "1.23e+5");
        assert_eq!(to_exponential(0.00015, None), "1.5e-4");
        assert_eq!(to_exponential(-5.0, Some(0)), "-5e+0");
        assert_eq!(to_exponential(0.0, Some(2)), "0.00e+0");
        assert_eq!(to_exponential(9.99, Some(1)), "1.0e+1");
    }

    #[test]
    fn parsing_matches_javascript() {
        assert_eq!(parse_int("  42px", f64::NAN), 42.0);
        assert_eq!(parse_int("-0x1F", f64::NAN), -31.0);
        assert_eq!(parse_int("ff", 16.0), 255.0);
        assert_eq!(parse_int("0x10", 10.0), 0.0);
        assert!(parse_int("z", 10.0).is_nan());
        assert!(parse_int("1", 37.0).is_nan());
        assert_eq!(parse_float(" 3.14abc"), 3.14);
        assert_eq!(parse_float("1e3x"), 1000.0);
        assert_eq!(parse_float("1e"), 1.0);
        assert_eq!(parse_float(".5"), 0.5);
        assert_eq!(parse_float("-Infinityx"), f64::NEG_INFINITY);
        assert!(parse_float("abc").is_nan());
    }

    #[test]
    fn int32() {
        assert_eq!(to_int32(4_294_967_297.0), 1);
        assert_eq!(to_int32(-1.5), -1);
        assert_eq!(to_int32(2_147_483_648.0), i32::MIN);
    }
}
