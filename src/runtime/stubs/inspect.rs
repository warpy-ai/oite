//! How values are printed: `console.log` formatting.

use super::*;
use builtins::InspectOptions;

// =========================================================================
// Console
// =========================================================================

/// Human-readable rendering of a value, in the style of Node's
/// `util.inspect`. Must stay identical to `stdlib::inspect` in the VM, so
/// `console.log` prints the same on every backend: `[ 1, 2, 3 ]`,
/// `{ x: 1, y: 's' }`, `[Circular]`. Object keys are in insertion
/// order, and keys starting with `__` are internal and hidden.
pub fn inspect(v: OtValue) -> String {
    inspect_with(v, InspectOptions::DEFAULT)
}

pub(super) fn inspect_with(v: OtValue, opts: InspectOptions) -> String {
    let mut seen = Vec::new();
    inspect_inner(v, 0, opts, &mut seen)
}

pub(super) fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

pub(super) fn quote(s: &str) -> String {
    format!(
        "'{}'",
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
    )
}

/// `key: value, ...`, as `console.log` shows an object's properties.
fn show_entries(
    entries: Vec<(&str, u64)>,
    depth: usize,
    opts: InspectOptions,
    seen: &mut Vec<usize>,
) -> String {
    let parts: Vec<String> = entries
        .into_iter()
        .map(|(k, bits)| {
            let key = if is_identifier(k) {
                k.to_string()
            } else {
                quote(k)
            };
            let val = inspect_inner(OtValue::from_bits(bits), depth + 1, opts, seen);
            format!("{}: {}", key, val)
        })
        .collect();
    parts.join(", ")
}

/// A function or a class, `summary`, then its own properties: `[Function:
/// f] { x: 1 }`.
fn with_entries(
    summary: String,
    entries: Vec<(&str, u64)>,
    depth: usize,
    opts: InspectOptions,
    seen: &mut Vec<usize>,
) -> String {
    if entries.is_empty() || depth > opts.depth {
        return summary;
    }
    format!("{summary} {{ {} }}", show_entries(entries, depth, opts, seen))
}

pub(super) fn inspect_inner(
    v: OtValue,
    depth: usize,
    opts: InspectOptions,
    seen: &mut Vec<usize>,
) -> String {
    let Some((ptr, kind)) = object(v) else {
        return match v.as_number() {
            Some(n) => builtins::format_number(n),
            None => value_to_string(v),
        };
    };
    match kind {
        ObjectKind::String => quote(unsafe { ptr.as_ref::<NativeString>().as_str() }),
        ObjectKind::Function => {
            let name = function_name(ptr);
            let summary = if name.is_empty() {
                "[Function (anonymous)]".to_string()
            } else {
                format!("[Function: {name}]")
            };
            with_entries(summary, function_entries(ptr), depth, opts, seen)
        }
        ObjectKind::ByteStream => "ByteStream".to_string(),
        ObjectKind::Array | ObjectKind::Object => {
            let addr = ptr.as_usize();
            if seen.contains(&addr) {
                return "[Circular]".to_string();
            }
            let too_deep = depth > opts.depth;
            seen.push(addr);
            let out = if kind == ObjectKind::Array {
                let arr = array(ptr);
                if arr.len == 0 && !opts.show_hidden {
                    "[]".to_string()
                } else if too_deep {
                    "[Array]".to_string()
                } else {
                    let mut parts: Vec<String> = (0..arr.len as usize)
                        .map(|i| {
                            let e = OtValue::from_bits(unsafe { *arr.elements.add(i) });
                            inspect_inner(e, depth + 1, opts, seen)
                        })
                        .collect();
                    if opts.show_hidden {
                        parts.push(format!("[length]: {}", arr.len));
                    }
                    format!("[ {} ]", parts.join(", "))
                }
            } else if let Some(id) = promise_id(v.to_bits()) {
                promise_summary(id, depth, opts, seen)
            } else if is_class(ptr) {
                let mut statics = visible_data(ptr);
                statics.retain(|(k, _)| !builtins::is_function_key(k));
                with_entries(class_summary(ptr), statics, depth, opts, seen)
            } else if is_error(ptr) {
                // `TypeError: message`, then any other own properties.
                let summary = error_summary(ptr);
                let mut keys = visible_data(ptr);
                keys.retain(|(k, _)| *k != "message");
                if keys.is_empty() || too_deep {
                    summary
                } else {
                    let parts: Vec<String> = keys
                        .into_iter()
                        .map(|(k, bits)| {
                            let val =
                                inspect_inner(OtValue::from_bits(bits), depth + 1, opts, seen);
                            format!("{}: {}", k, val)
                        })
                        .collect();
                    format!("{} {{ {} }}", summary, parts.join(", "))
                }
            } else {
                let keys = visible_data(ptr);
                if keys.is_empty() {
                    "{}".to_string()
                } else if too_deep {
                    "[Object]".to_string()
                } else {
                    format!("{{ {} }}", show_entries(keys, depth, opts, seen))
                }
            };
            seen.pop();
            out
        }
    }
}

/// The line `console.log(...args)` prints (`builtins::format_log`).
pub(super) fn format_log_args(a: &[u64]) -> String {
    builtins::format_log(&NativeHost, a, |&v, opts| {
        inspect_with(OtValue::from_bits(v), opts)
    })
}
