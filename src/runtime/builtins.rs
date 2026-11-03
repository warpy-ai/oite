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

/// A built-in method's error, as a message (`TypeError: ...`).
pub type Error = String;

/// `Some(result)` if `name` is a string method, `None` otherwise.
pub fn string_method<H: Host>(
    h: &H,
    s: &str,
    name: &str,
    args: &[H::Value],
) -> Option<Result<Outcome<H::Value>, Error>> {
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
                return Some(Err(format!("RangeError: Invalid count value: {}", n)));
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
