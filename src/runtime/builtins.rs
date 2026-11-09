//! Built-in string and array methods, shared by the VM and native code.
//!
//! Each method is implemented once, generically over the value
//! representation: the VM plugs in `JsValue`, the native runtime plugs in
//! NaN-boxed `u64` values (see `Host`). This keeps `"abc".slice(-2)` or
//! `[1, 2, 3].splice(1, 1)` identical on every backend.
//!
//! String model: method indices count Unicode scalar values (`char`s). The
//! `length` property still counts UTF-8 bytes; settling one model for both
//! is #133. For ASCII text, which is all the tests use today, the two agree.

/// What a built-in method needs from a value representation.
pub trait Host {
    type Value: Clone;

    fn number(&self, v: &Self::Value) -> Option<f64>;
    fn string<'a>(&'a self, v: &'a Self::Value) -> Option<&'a str>;
    /// `String(v)`.
    fn to_string(&self, v: &Self::Value) -> String;
    /// `a === b`.
    fn strict_equal(&self, a: &Self::Value, b: &Self::Value) -> bool;
    /// The elements, if `v` is an array.
    fn array_elements(&self, v: &Self::Value) -> Option<Vec<Self::Value>>;
    /// Whether `v` is an array.
    fn is_array(&self, v: &Self::Value) -> bool;
    /// `Object.keys(v)`: an object's own property names, an array's indices,
    /// nothing for any other value.
    fn own_keys(&self, v: &Self::Value) -> Vec<String>;

    fn make_number(&self, n: f64) -> Self::Value;
    fn make_string(&self, s: String) -> Self::Value;
    fn make_bool(&self, b: bool) -> Self::Value;
    fn undefined(&self) -> Self::Value;
}

/// The result of a built-in method call.
pub enum Outcome<V> {
    Value(V),
    /// A new array with these elements.
    NewArray(Vec<V>),
    /// The receiver itself (`reverse`, `fill`).
    This,
}

/// A built-in method's error: the error class to throw and its message.
pub struct Error {
    pub kind: &'static str,
    pub message: String,
}

impl Error {
    pub fn range(message: impl Into<String>) -> Self {
        Error {
            kind: "RangeError",
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

/// `s.charAt(i)`, `s.charCodeAt(i)` and `s.at(i)`, without copying `s`.
/// `ascii` says every byte of `s` is ASCII, so character `i` is byte `i`;
/// otherwise the characters are counted. `None` for any other method.
pub fn string_index<H: Host>(
    h: &H,
    s: &str,
    ascii: bool,
    name: &str,
    args: &[H::Value],
) -> Option<Result<Outcome<H::Value>, Error>> {
    if !matches!(name, "charAt" | "charCodeAt" | "at") {
        return None;
    }
    let len = if ascii { s.len() } else { s.chars().count() };
    let i = args
        .first()
        .map_or(0.0, |v| integer(h.number(v).unwrap_or(f64::NAN)));
    let i = if name == "at" && i < 0.0 {
        i + len as f64
    } else {
        i
    };
    let c = if i >= 0.0 && i < len as f64 {
        let i = i as usize;
        if ascii {
            Some(s.as_bytes()[i] as char)
        } else {
            s.chars().nth(i)
        }
    } else {
        None
    };
    Some(Ok(Outcome::Value(match (name, c) {
        ("charCodeAt", Some(c)) => h.make_number(c as u32 as f64),
        ("charCodeAt", None) => h.make_number(f64::NAN),
        ("charAt", c) => h.make_string(c.map(String::from).unwrap_or_default()),
        (_, Some(c)) => h.make_string(c.to_string()),
        _ => h.undefined(),
    })))
}

/// `Some(result)` if `name` is a string method, `None` otherwise.
pub fn string_method<H: Host>(
    h: &H,
    s: &str,
    name: &str,
    args: &[H::Value],
) -> Option<Result<Outcome<H::Value>, Error>> {
    if let Some(result) = string_index(h, s, s.is_ascii(), name, args) {
        return Some(result);
    }
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let arg = |i: usize| args.get(i);
    let arg_str = |i: usize| -> String {
        match arg(i) {
            Some(v) => h.to_string(v),
            None => "undefined".to_string(),
        }
    };
    let arg_num = |i: usize| arg(i).map(|v| integer(h.number(v).unwrap_or(f64::NAN)));
    let substr = |a: usize, b: usize| -> String {
        if a >= b {
            String::new()
        } else {
            chars[a..b].iter().collect()
        }
    };
    let string = |x: String| Ok(Outcome::Value(h.make_string(x)));
    let number = |x: f64| Ok(Outcome::Value(h.make_number(x)));
    let boolean = |x: bool| Ok(Outcome::Value(h.make_bool(x)));

    Some(match name {
        "charAt" => {
            let i = arg_num(0).unwrap_or(0.0);
            if i >= 0.0 && (i as usize) < len {
                string(chars[i as usize].to_string())
            } else {
                string(String::new())
            }
        }
        "charCodeAt" => {
            let i = arg_num(0).unwrap_or(0.0);
            if i >= 0.0 && (i as usize) < len {
                number(chars[i as usize] as u32 as f64)
            } else {
                number(f64::NAN)
            }
        }
        "at" => {
            let i = arg_num(0).unwrap_or(0.0);
            let i = if i < 0.0 { i + len as f64 } else { i };
            if i >= 0.0 && (i as usize) < len {
                string(chars[i as usize].to_string())
            } else {
                Ok(Outcome::Value(h.undefined()))
            }
        }
        "slice" => {
            let start = relative(arg_num(0), len, 0);
            let end = relative(arg_num(1), len, len);
            string(substr(start, end))
        }
        "substring" => {
            let a = clamp(arg_num(0).unwrap_or(0.0), len);
            let b = arg_num(1).map_or(len, |n| clamp(n, len));
            string(substr(a.min(b), a.max(b)))
        }
        "indexOf" => {
            let needle: Vec<char> = arg_str(0).chars().collect();
            let from = clamp(arg_num(1).unwrap_or(0.0), len);
            number(find(&chars, &needle, from).map_or(-1.0, |i| i as f64))
        }
        "lastIndexOf" => {
            let needle: Vec<char> = arg_str(0).chars().collect();
            let from = match arg_num(1) {
                Some(n) if !n.is_nan() => clamp(n, len),
                _ => len,
            };
            number(rfind(&chars, &needle, from).map_or(-1.0, |i| i as f64))
        }
        "includes" => {
            let needle: Vec<char> = arg_str(0).chars().collect();
            let from = clamp(arg_num(1).unwrap_or(0.0), len);
            boolean(find(&chars, &needle, from).is_some())
        }
        "startsWith" => {
            let needle: Vec<char> = arg_str(0).chars().collect();
            let pos = clamp(arg_num(1).unwrap_or(0.0), len);
            boolean(chars[pos..].starts_with(&needle))
        }
        "endsWith" => {
            let needle: Vec<char> = arg_str(0).chars().collect();
            let end = arg_num(1).map_or(len, |n| clamp(n, len));
            boolean(chars[..end].ends_with(&needle))
        }
        "split" => {
            let limit = arg_num(1).map_or(usize::MAX, |n| n.max(0.0) as usize);
            let parts: Vec<String> = match arg(0) {
                None => vec![s.to_string()],
                Some(sep) if h.string(sep).is_none() && h.number(sep).is_none() => {
                    vec![s.to_string()]
                }
                Some(sep) => {
                    let sep = h.to_string(sep);
                    if sep.is_empty() {
                        chars.iter().map(|c| c.to_string()).collect()
                    } else {
                        s.split(sep.as_str()).map(str::to_string).collect()
                    }
                }
            };
            Ok(Outcome::NewArray(
                parts
                    .into_iter()
                    .take(limit)
                    .map(|p| h.make_string(p))
                    .collect(),
            ))
        }
        "trim" => string(s.trim().to_string()),
        "trimStart" | "trimLeft" => string(s.trim_start().to_string()),
        "trimEnd" | "trimRight" => string(s.trim_end().to_string()),
        "toLowerCase" => string(s.to_lowercase()),
        "toUpperCase" => string(s.to_uppercase()),
        "replace" => {
            // String patterns only; regular expressions are not supported.
            let search = arg_str(0);
            let replacement = arg_str(1);
            string(s.replacen(search.as_str(), replacement.as_str(), 1))
        }
        "replaceAll" => {
            let search = arg_str(0);
            let replacement = arg_str(1);
            string(s.replace(search.as_str(), replacement.as_str()))
        }
        "repeat" => {
            let n = arg_num(0).unwrap_or(0.0);
            if n < 0.0 || n.is_infinite() {
                return Some(Err(Error::range(format!("Invalid count value: {}", n))));
            }
            string(s.repeat(n as usize))
        }
        "concat" => {
            let mut out = s.to_string();
            for v in args {
                out.push_str(&h.to_string(v));
            }
            string(out)
        }
        "padStart" | "padEnd" => {
            let target = arg_num(0).unwrap_or(0.0).max(0.0) as usize;
            let pad: Vec<char> = match arg(1) {
                Some(v) => h.to_string(v).chars().collect(),
                None => vec![' '],
            };
            if target <= len || pad.is_empty() {
                string(s.to_string())
            } else {
                let fill: String = pad.iter().cycle().take(target - len).collect();
                if name == "padStart" {
                    string(fill + s)
                } else {
                    string(format!("{}{}", s, fill))
                }
            }
        }
        "toString" | "valueOf" => string(s.to_string()),
        _ => return None,
    })
}

/// `Some(result)` if `name` is an array method, `None` otherwise. `arr` is
/// the receiver's elements; mutating methods change it in place.
pub fn array_method<H: Host>(
    h: &H,
    arr: &mut Vec<H::Value>,
    name: &str,
    args: &[H::Value],
) -> Option<Result<Outcome<H::Value>, Error>> {
    let len = arr.len();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_else(|| h.undefined());
    let arg_num = |i: usize| {
        args.get(i)
            .map(|v| integer(h.number(v).unwrap_or(f64::NAN)))
    };
    let number = |x: f64| Ok(Outcome::Value(h.make_number(x)));

    Some(match name {
        "push" => {
            arr.extend(args.iter().cloned());
            number(arr.len() as f64)
        }
        "pop" => Ok(Outcome::Value(arr.pop().unwrap_or_else(|| h.undefined()))),
        "shift" => Ok(Outcome::Value(if arr.is_empty() {
            h.undefined()
        } else {
            arr.remove(0)
        })),
        "unshift" => {
            for (i, v) in args.iter().enumerate() {
                arr.insert(i, v.clone());
            }
            number(arr.len() as f64)
        }
        "at" => {
            let i = arg_num(0).unwrap_or(0.0);
            let i = if i < 0.0 { i + len as f64 } else { i };
            Ok(Outcome::Value(if i >= 0.0 && (i as usize) < len {
                arr[i as usize].clone()
            } else {
                h.undefined()
            }))
        }
        "join" => {
            let sep = match args.first() {
                Some(v) if h.string(v).is_some() || h.number(v).is_some() => h.to_string(v),
                _ => ",".to_string(),
            };
            let parts: Vec<String> = arr.iter().map(|v| join_part(h, v)).collect();
            Ok(Outcome::Value(h.make_string(parts.join(&sep))))
        }
        "toString" => {
            let parts: Vec<String> = arr.iter().map(|v| join_part(h, v)).collect();
            Ok(Outcome::Value(h.make_string(parts.join(","))))
        }
        "indexOf" => {
            let needle = arg(0);
            let from = relative(arg_num(1), len, 0);
            let found = (from..len).find(|&i| h.strict_equal(&arr[i], &needle));
            number(found.map_or(-1.0, |i| i as f64))
        }
        "lastIndexOf" => {
            let needle = arg(0);
            let from = match arg_num(1) {
                Some(n) if n < 0.0 => n + len as f64,
                Some(n) => n,
                None => len as f64 - 1.0,
            };
            if from < 0.0 || len == 0 {
                return Some(number(-1.0));
            }
            let from = (from as usize).min(len - 1);
            let found = (0..=from).rev().find(|&i| h.strict_equal(&arr[i], &needle));
            number(found.map_or(-1.0, |i| i as f64))
        }
        "includes" => {
            let needle = arg(0);
            let from = relative(arg_num(1), len, 0);
            // SameValueZero: like ===, except NaN includes NaN.
            let is_nan = |v: &H::Value| h.number(v).is_some_and(f64::is_nan);
            let found = (from..len)
                .any(|i| h.strict_equal(&arr[i], &needle) || (is_nan(&arr[i]) && is_nan(&needle)));
            Ok(Outcome::Value(h.make_bool(found)))
        }
        "slice" => {
            let start = relative(arg_num(0), len, 0);
            let end = relative(arg_num(1), len, len);
            Ok(Outcome::NewArray(if start < end {
                arr[start..end].to_vec()
            } else {
                Vec::new()
            }))
        }
        "splice" => {
            let start = relative(arg_num(0), len, 0);
            let delete = match args.len() {
                0 => 0,
                1 => len - start,
                _ => clamp(arg_num(1).unwrap_or(0.0), len - start),
            };
            let removed: Vec<H::Value> = arr
                .splice(start..start + delete, args.iter().skip(2).cloned())
                .collect();
            Ok(Outcome::NewArray(removed))
        }
        "concat" => {
            let mut out = arr.clone();
            for v in args {
                match h.array_elements(v) {
                    Some(elems) => out.extend(elems),
                    None => out.push(v.clone()),
                }
            }
            Ok(Outcome::NewArray(out))
        }
        "reverse" => {
            arr.reverse();
            Ok(Outcome::This)
        }
        "fill" => {
            let value = arg(0);
            let start = relative(arg_num(1), len, 0);
            let end = relative(arg_num(2), len, len);
            for slot in arr.iter_mut().take(end).skip(start) {
                *slot = value.clone();
            }
            Ok(Outcome::This)
        }
        _ => return None,
    })
}

/// An array element as `join` prints it: `null` and `undefined` are empty.
fn join_part<H: Host>(h: &H, v: &H::Value) -> String {
    let s = h.to_string(v);
    if (s == "null" || s == "undefined") && h.string(v).is_none() {
        String::new()
    } else {
        s
    }
}

/// `ToIntegerOrInfinity`: NaN is 0, otherwise truncated.
fn integer(n: f64) -> f64 {
    if n.is_nan() { 0.0 } else { n.trunc() }
}

/// Clamp an integer index to `0..=len`.
fn clamp(n: f64, len: usize) -> usize {
    if n <= 0.0 {
        0
    } else if n >= len as f64 {
        len
    } else {
        n as usize
    }
}

/// A `slice`-style index: negative counts from the end. `None` is `default`.
fn relative(n: Option<f64>, len: usize, default: usize) -> usize {
    match n {
        None => default,
        Some(n) if n < 0.0 => clamp(n + len as f64, len),
        Some(n) => clamp(n, len),
    }
}

fn find(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(hay.len()));
    }
    (from..=hay.len().saturating_sub(needle.len()))
        .find(|&i| i + needle.len() <= hay.len() && hay[i..i + needle.len()] == *needle)
}

fn rfind(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    let last = from.min(hay.len() - needle.len());
    (0..=last)
        .rev()
        .find(|&i| hay[i..i + needle.len()] == *needle)
}

/// The function members of `Math`. Native code identifies one by its index.
pub const MATH_FUNCTIONS: &[&str] = &[
    "abs", "ceil", "floor", "round", "trunc", "sign", "sqrt", "cbrt", "pow", "exp", "log", "log2",
    "log10", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "hypot", "min", "max", "random",
];

/// The constant members of `Math`.
pub const MATH_CONSTANTS: &[(&str, f64)] = &[
    ("PI", std::f64::consts::PI),
    ("E", std::f64::consts::E),
    ("LN2", std::f64::consts::LN_2),
    ("LN10", std::f64::consts::LN_10),
    ("LOG2E", std::f64::consts::LOG2_E),
    ("LOG10E", std::f64::consts::LOG10_E),
    ("SQRT2", std::f64::consts::SQRT_2),
    ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
];

/// `Math.<name>(...args)`, with the arguments already converted to numbers.
pub fn math_call(name: &str, args: &[f64]) -> f64 {
    let a = args.first().copied().unwrap_or(f64::NAN);
    let b = args.get(1).copied().unwrap_or(f64::NAN);
    match name {
        "abs" => a.abs(),
        "ceil" => a.ceil(),
        "floor" => a.floor(),
        // Halves round up, towards +Infinity (`Math.round(-2.5)` is -2).
        "round" => {
            let f = a.floor();
            if a - f >= 0.5 { f + 1.0 } else { f }
        }
        "trunc" => a.trunc(),
        "sign" => {
            if a.is_nan() || a == 0.0 {
                a
            } else {
                a.signum()
            }
        }
        "sqrt" => a.sqrt(),
        "cbrt" => a.cbrt(),
        "pow" => a.powf(b),
        "exp" => a.exp(),
        "log" => a.ln(),
        "log2" => a.log2(),
        "log10" => a.log10(),
        "sin" => a.sin(),
        "cos" => a.cos(),
        "tan" => a.tan(),
        "asin" => a.asin(),
        "acos" => a.acos(),
        "atan" => a.atan(),
        "atan2" => a.atan2(b),
        "hypot" => args.iter().map(|x| x * x).sum::<f64>().sqrt(),
        "min" => args.iter().fold(f64::INFINITY, |m, &x| {
            if m.is_nan() || x.is_nan() {
                f64::NAN
            } else {
                m.min(x)
            }
        }),
        "max" => args.iter().fold(f64::NEG_INFINITY, |m, &x| {
            if m.is_nan() || x.is_nan() {
                f64::NAN
            } else {
                m.max(x)
            }
        }),
        "random" => random(),
        _ => f64::NAN,
    }
}

/// A number in [0, 1), from a xorshift generator seeded by the clock.
fn random() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
            | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// `Date.now()`: milliseconds since the Unix epoch.
pub fn date_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

/// `Some(result)` if `name` is a number method, `None` otherwise.
pub fn number_method<H: Host>(
    h: &H,
    n: f64,
    name: &str,
    args: &[H::Value],
) -> Option<Result<Outcome<H::Value>, Error>> {
    let arg_num = |i: usize| args.get(i).and_then(|v| h.number(v));
    Some(Ok(Outcome::Value(match name {
        "toString" => {
            let radix = match args.first() {
                None => 10.0,
                Some(v) => integer(h.number(v).unwrap_or(f64::NAN)),
            };
            if !(2.0..=36.0).contains(&radix) {
                return Some(Err(Error::range(
                    "toString() radix must be between 2 and 36",
                )));
            }
            if radix == 10.0 || !n.is_finite() {
                h.make_string(h.to_string(&h.make_number(n)))
            } else {
                h.make_string(to_radix(n, radix as u32))
            }
        }
        "toFixed" => {
            let digits = integer(arg_num(0).unwrap_or(0.0));
            if !(0.0..=100.0).contains(&digits) {
                return Some(Err(Error::range(
                    "toFixed() digits argument must be between 0 and 100",
                )));
            }
            if !n.is_finite() || n.abs() >= 1e21 {
                h.make_string(h.to_string(&h.make_number(n)))
            } else {
                h.make_string(to_fixed(n, digits as usize))
            }
        }
        "valueOf" => h.make_number(n),
        _ => return None,
    })))
}

/// `n.toFixed(digits)`: exact ties round away from zero, as JavaScript
/// picks the larger candidate (Rust's formatting rounds them to even).
fn to_fixed(n: f64, digits: usize) -> String {
    // Rust prints the correctly rounded exact value; only an exact tie (the
    // digits after the cut are 5 then zeros) needs rounding up instead.
    let exact = format!("{:.*}", digits + 1100, n.abs());
    let point = exact.find('.').unwrap_or(exact.len());
    let rest = &exact[(point + 1 + digits).min(exact.len())..];
    let tie = rest.starts_with('5') && rest[1..].bytes().all(|b| b == b'0');
    let s = if tie {
        format!("{:.*}", digits, n.abs() + 0.5 / 10f64.powi(digits as i32))
    } else {
        format!("{:.*}", digits, n.abs())
    };
    if n < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') {
        format!("-{s}")
    } else {
        s
    }
}

/// `n.toString(radix)` for a radix other than 10.
fn to_radix(n: f64, radix: u32) -> String {
    let digit = |d: u32| std::char::from_digit(d, radix).unwrap_or('0');
    let neg = n < 0.0;
    let n = n.abs();
    let mut int = n.trunc();
    let mut frac = n - int;
    let mut int_digits = Vec::new();
    if int == 0.0 {
        int_digits.push('0');
    }
    while int >= 1.0 {
        int_digits.push(digit((int % radix as f64) as u32));
        int = (int / radix as f64).trunc();
    }
    int_digits.reverse();
    let mut out: String = int_digits.into_iter().collect();
    if frac > 0.0 {
        out.push('.');
        // Enough digits to identify a double.
        for _ in 0..52 {
            if frac == 0.0 {
                break;
            }
            frac *= radix as f64;
            let d = frac.trunc();
            out.push(digit(d as u32));
            frac -= d;
        }
    }
    if neg {
        out.insert(0, '-');
    }
    out
}

/// JavaScript `Number.prototype.toString()`: the shortest digits that
/// round-trip, in exponent form from 1e21 up and below 1e-6.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    // `{:e}` gives the shortest round-trip digits: "1.2345e-7".
    let sci = format!("{:e}", n.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    // The value is 0.<digits> * 10^point.
    let point = exp.parse::<i32>().unwrap_or(0) + 1;
    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let e = point - 1;
        let sign = if e < 0 { '-' } else { '+' };
        if k == 1 {
            format!("{digits}e{sign}{}", e.abs())
        } else {
            format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e.abs())
        }
    };
    if n < 0.0 { format!("-{body}") } else { body }
}

/// Property names that `console.log` and `Object.keys` leave out: internal
/// ones (`__proto__`, `%parent`), private fields (`#x`) and accessors
/// (`getter:x`, `setter:x`).
/// The name `Object.keys` shows for a stored property key: an accessor
/// (`getter:x`, `setter:x`) is the property `x`; internal keys are none.
pub fn own_key_name(key: &str) -> Option<&str> {
    let name = key
        .strip_prefix("getter:")
        .or_else(|| key.strip_prefix("setter:"))
        .unwrap_or(key);
    (!is_hidden_key(name)).then_some(name)
}

pub fn is_hidden_key(key: &str) -> bool {
    key.starts_with("__")
        || key.starts_with('%')
        || key.starts_with('#')
        || key.starts_with("getter:")
        || key.starts_with("setter:")
}
