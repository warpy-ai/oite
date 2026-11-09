//! How values are printed: `console.log` formatting.

use super::*;

// =========================================================================
// Console
// =========================================================================

/// Nesting depth beyond which objects print as `[Object]` / `[Array]`.
pub(super) const INSPECT_DEPTH: usize = 2;

/// Human-readable rendering of a value, in the style of Node's
/// `util.inspect`. Must stay identical to `stdlib::inspect` in the VM, so
/// `console.log` prints the same on every backend: `[ 1, 2, 3 ]`,
/// `{ x: 1, y: 's' }`, `[Circular]`. Object keys are sorted, and keys
/// starting with `__` are internal and hidden.
pub fn inspect(v: OtValue) -> String {
    let mut seen = Vec::new();
    inspect_inner(v, 0, &mut seen)
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

pub(super) fn inspect_inner(v: OtValue, depth: usize, seen: &mut Vec<usize>) -> String {
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
            } else if let Some(id) = promise_id(v.to_bits()) {
                promise_summary(id, depth, seen)
            } else if is_class(ptr) {
                class_summary(ptr)
            } else if is_error(ptr) {
                // `TypeError: message`, then any other own properties.
                let summary = error_summary(ptr);
                let map = props(ptr);
                let mut keys: Vec<&(String, u64)> = map
                    .iter()
                    .filter(|(k, _)| !is_hidden_key(k) && k != "message")
                    .collect();
                keys.sort_by(|a, b| a.0.cmp(&b.0));
                if keys.is_empty() || too_deep {
                    summary
                } else {
                    let parts: Vec<String> = keys
                        .into_iter()
                        .map(|(k, bits)| {
                            let val = inspect_inner(OtValue::from_bits(*bits), depth + 1, seen);
                            format!("{}: {}", k, val)
                        })
                        .collect();
                    format!("{} {{ {} }}", summary, parts.join(", "))
                }
            } else {
                let map = props(ptr);
                let mut keys: Vec<&(String, u64)> =
                    map.iter().filter(|(k, _)| !is_hidden_key(k)).collect();
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
pub(super) fn format_log_args(a: &[u64]) -> String {
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
