use crate::vm::value::{HeapData, HeapObject, JsValue};
use crate::vm::VM;

pub fn call_string_method(
    vm: &mut VM,
    s: &str,
    method_name: &str,
    arg_count: usize,
) -> Option<JsValue> {
    match method_name {
        "trim" => Some(JsValue::String(s.trim().to_string())),
        "includes" => {
            let search_value = vm.stack.pop().unwrap_or(JsValue::Undefined);
            let search_str = match search_value {
                JsValue::String(ss) => ss,
                JsValue::Number(n) => n.to_string(),
                JsValue::Boolean(b) => b.to_string(),
                JsValue::Null => "null".to_string(),
                JsValue::Undefined => "undefined".to_string(),
                _ => "".to_string(),
            };
            Some(JsValue::Boolean(s.contains(&search_str)))
        }
        "charCodeAt" => {
            let idx_val = vm.stack.pop().unwrap_or(JsValue::Number(0.0));
            if let JsValue::Number(idx) = idx_val {
                let code = s.chars().nth(idx as usize).map(|c| c as u32).unwrap_or(0);
                Some(JsValue::Number(code as f64))
            } else {
                Some(JsValue::Number(0.0))
            }
        }
        "charAt" => {
            let idx_val = vm.stack.pop().unwrap_or(JsValue::Number(0.0));
            if let JsValue::Number(idx) = idx_val {
                let ch = s
                    .chars()
                    .nth(idx as usize)
                    .map(|c| c.to_string())
                    .unwrap_or("".to_string());
                Some(JsValue::String(ch))
            } else {
                Some(JsValue::String(String::new()))
            }
        }
        "slice" => {
            let end = if arg_count > 1 {
                vm.stack
                    .pop()
                    .and_then(|v| match v {
                        JsValue::Number(n) => {
                            let char_count = s.chars().count();
                            if n < 0.0 {
                                Some((char_count as f64 + n) as usize)
                            } else {
                                Some(n as usize)
                            }
                        }
                        _ => None,
                    })
                    .unwrap_or(s.chars().count())
            } else {
                s.chars().count()
            };
            let start = vm
                .stack
                .pop()
                .and_then(|v| match v {
                    JsValue::Number(n) => {
                        let char_count = s.chars().count();
                        if n < 0.0 {
                            Some((char_count as f64 + n) as usize)
                        } else {
                            Some(n as usize)
                        }
                    }
                    _ => None,
                })
                .unwrap_or(0);

            let char_count = s.chars().count();
            let start = start.min(char_count);
            let end = end.min(char_count).max(start);

            let result: String = s
                .chars()
                .enumerate()
                .filter_map(|(i, ch)| {
                    if i >= start && i < end {
                        Some(ch)
                    } else {
                        None
                    }
                })
                .collect();

            Some(JsValue::String(result))
        }
        "toUpperCase" => Some(JsValue::String(s.to_uppercase())),
        "toLowerCase" => Some(JsValue::String(s.to_lowercase())),
        "indexOf" => {
            let search_value = vm.stack.pop().unwrap_or(JsValue::Undefined);
            let from_index = if arg_count > 1 {
                vm.stack
                    .pop()
                    .and_then(|v| match v {
                        JsValue::Number(n) => Some(n as usize),
                        _ => None,
                    })
                    .unwrap_or(0)
            } else {
                0
            };
            let search_str = match search_value {
                JsValue::String(ss) => ss,
                JsValue::Number(n) => n.to_string(),
                JsValue::Boolean(b) => b.to_string(),
                JsValue::Null => "null".to_string(),
                JsValue::Undefined => "undefined".to_string(),
                _ => "".to_string(),
            };
            let result = if from_index <= s.len() {
                if let Some(pos) = s[from_index..].find(&search_str) {
                    (from_index + pos) as f64
                } else {
                    -1.0
                }
            } else {
                -1.0
            };
            Some(JsValue::Number(result))
        }
        "lastIndexOf" => {
            let search_value = vm.stack.pop().unwrap_or(JsValue::Undefined);
            let from_index = if arg_count > 1 {
                vm.stack
                    .pop()
                    .and_then(|v| match v {
                        JsValue::Number(n) => Some(n as usize),
                        _ => None,
                    })
                    .unwrap_or(s.len())
            } else {
                s.len()
            };
            let search_str = match search_value {
                JsValue::String(ss) => ss,
                JsValue::Number(n) => n.to_string(),
                JsValue::Boolean(b) => b.to_string(),
                JsValue::Null => "null".to_string(),
                JsValue::Undefined => "undefined".to_string(),
                _ => "".to_string(),
            };
            let search_len = search_str.len();
            let end_pos = from_index.min(s.len()).saturating_sub(search_len);
            let mut found_pos = -1i64;
            for i in 0..=end_pos {
                if &s[i..i + search_len] == search_str {
                    found_pos = i as i64;
                }
            }
            Some(JsValue::Number(found_pos as f64))
        }
        "substring" => {
            let end = if arg_count > 1 {
                vm.stack
                    .pop()
                    .and_then(|v| match v {
                        JsValue::Number(n) => Some(n as usize),
                        _ => None,
                    })
                    .unwrap_or(s.len())
            } else {
                s.len()
            };
            let start = vm
                .stack
                .pop()
                .and_then(|v| match v {
                    JsValue::Number(n) => Some(n as usize),
                    _ => None,
                })
                .unwrap_or(0);

            let char_count = s.chars().count();
            let start = start.min(char_count);
            let end = end.min(char_count).max(start);

            let result: String = s
                .chars()
                .enumerate()
                .filter_map(|(i, ch)| {
                    if i >= start && i < end {
                        Some(ch)
                    } else {
                        None
                    }
                })
                .collect();

            Some(JsValue::String(result))
        }
        "replace" => {
            let new_value = vm.stack.pop().unwrap_or(JsValue::Undefined);
            let search_value = vm.stack.pop().unwrap_or(JsValue::Undefined);
            let search_str = match search_value {
                JsValue::String(ss) => ss,
                _ => "".to_string(),
            };
            let replace_str = match new_value {
                JsValue::String(ss) => ss,
                _ => "".to_string(),
            };
            let result = if let Some(pos) = s.find(&search_str) {
                let mut new_s = s[..pos].to_string();
                new_s.push_str(&replace_str);
                new_s.push_str(&s[pos + search_str.len()..]);
                new_s
            } else {
                s.to_string()
            };
            Some(JsValue::String(result))
        }
        "startsWith" => {
            let search_value = vm.stack.pop().unwrap_or(JsValue::Undefined);
            let from_index = if arg_count > 1 {
                vm.stack
                    .pop()
                    .and_then(|v| match v {
                        JsValue::Number(n) => Some(n as usize),
                        _ => None,
                    })
                    .unwrap_or(0)
            } else {
                0
            };
            let search_str = match search_value {
                JsValue::String(ss) => ss,
                JsValue::Number(n) => n.to_string(),
                JsValue::Boolean(b) => b.to_string(),
                JsValue::Null => "null".to_string(),
                JsValue::Undefined => "undefined".to_string(),
                _ => "".to_string(),
            };
            let starts_with = if from_index <= s.len() && s.len() - from_index >= search_str.len() {
                &s[from_index..from_index + search_str.len()] == search_str
            } else {
                false
            };
            Some(JsValue::Boolean(starts_with))
        }
        "endsWith" => {
            let search_value = vm.stack.pop().unwrap_or(JsValue::Undefined);
            let end_position = if arg_count > 1 {
                vm.stack
                    .pop()
                    .and_then(|v| match v {
                        JsValue::Number(n) => {
                            if n < 0.0 {
                                None
                            } else {
                                Some(n as usize)
                            }
                        }
                        _ => None,
                    })
                    .unwrap_or(s.len())
            } else {
                s.len()
            };
            let search_str = match search_value {
                JsValue::String(ss) => ss,
                JsValue::Number(n) => n.to_string(),
                JsValue::Boolean(b) => b.to_string(),
                JsValue::Null => "null".to_string(),
                JsValue::Undefined => "undefined".to_string(),
                _ => "".to_string(),
            };
            let end_pos = end_position.min(s.len());
            let ends_with = s.len() >= search_str.len()
                && end_pos >= search_str.len()
                && s[end_pos - search_str.len()..end_pos] == search_str;
            Some(JsValue::Boolean(ends_with))
        }
        "trimStart" | "trimLeft" => Some(JsValue::String(s.trim_start().to_string())),
        "trimEnd" | "trimRight" => Some(JsValue::String(s.trim_end().to_string())),
        "repeat" => {
            let count = vm.stack.pop().unwrap_or(JsValue::Number(0.0));
            if let JsValue::Number(n) = count {
                let count_usize = n as usize;
                if n < 0.0 || !n.is_finite() {
                    Some(JsValue::String(String::new()))
                } else {
                    Some(JsValue::String(s.repeat(count_usize)))
                }
            } else {
                Some(JsValue::String(String::new()))
            }
        }
        "concat" => {
            let mut result = s.to_string();
            for _ in 0..arg_count {
                let arg = vm.stack.pop().unwrap_or(JsValue::Undefined);
                match arg {
                    JsValue::String(ss) => result.push_str(&ss),
                    JsValue::Number(n) => result.push_str(&n.to_string()),
                    JsValue::Boolean(b) => result.push_str(&b.to_string()),
                    JsValue::Null => result.push_str("null"),
                    JsValue::Undefined => result.push_str("undefined"),
                    _ => {}
                }
            }
            Some(JsValue::String(result))
        }
        "split" => {
            let separator = vm.stack.pop().unwrap_or(JsValue::Undefined);
            let limit = if arg_count > 1 {
                vm.stack.pop().and_then(|v| match v {
                    JsValue::Number(n) => Some(n as usize),
                    _ => None,
                })
            } else {
                None
            };
            match separator {
                JsValue::String(sep) => {
                    let parts: Vec<String> = if sep.is_empty() {
                        s.chars().map(|c| c.to_string()).collect()
                    } else {
                        s.split(&sep).map(|s| s.to_string()).collect()
                    };
                    let parts_to_use = limit.unwrap_or(parts.len());
                    let result_parts: Vec<JsValue> = parts[..parts_to_use]
                        .iter()
                        .map(|p| JsValue::String(p.clone()))
                        .collect();
                    let result_ptr = vm.heap.len();
                    vm.heap.push(HeapObject {
                        data: HeapData::Array(result_parts),
                    });
                    Some(JsValue::Object(result_ptr))
                }
                _ => {
                    let result_parts: Vec<JsValue> = vec![JsValue::String(s.to_string())];
                    let result_ptr = vm.heap.len();
                    vm.heap.push(HeapObject {
                        data: HeapData::Array(result_parts),
                    });
                    Some(JsValue::Object(result_ptr))
                }
            }
        }
        _ => None,
    }
}
