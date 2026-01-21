use crate::vm::value::JsValue;

pub fn call_array_method(
    stack: &mut Vec<JsValue>,
    arr: &mut Vec<JsValue>,
    method_name: &str,
    arg_count: usize,
) -> Option<JsValue> {
    match method_name {
        "push" => {
            let mut args = Vec::with_capacity(arg_count);
            for _ in 0..arg_count {
                args.push(stack.pop().expect("Missing argument"));
            }
            args.reverse();
            for arg in args {
                arr.push(arg);
            }
            Some(JsValue::Number(arr.len() as f64))
        }
        "pop" => Some(arr.pop().unwrap_or(JsValue::Undefined)),
        "shift" => {
            let result = if arr.is_empty() {
                JsValue::Undefined
            } else {
                arr.remove(0)
            };
            Some(result)
        }
        "unshift" => {
            let mut args = Vec::with_capacity(arg_count);
            for _ in 0..arg_count {
                args.push(stack.pop().expect("Missing argument"));
            }
            args.reverse();
            for arg in args.into_iter().rev() {
                arr.insert(0, arg);
            }
            Some(JsValue::Number(arr.len() as f64))
        }
        "length" => Some(JsValue::Number(arr.len() as f64)),
        "indexOf" => {
            let from_index = if arg_count > 1 {
                stack
                    .pop()
                    .and_then(|v| match v {
                        JsValue::Number(n) => Some(n as usize),
                        _ => None,
                    })
                    .unwrap_or(0)
            } else {
                0
            };
            let search_value = stack.pop().expect("Missing argument for indexOf");

            let result = arr
                .iter()
                .enumerate()
                .skip(from_index)
                .find(|(_, val)| **val == search_value)
                .map(|(i, _)| i as f64)
                .unwrap_or(-1.0);

            Some(JsValue::Number(result))
        }
        "includes" => {
            let search_value = stack.pop().expect("Missing argument for includes");
            let found = arr.contains(&search_value);
            Some(JsValue::Boolean(found))
        }
        "join" => {
            let separator = if arg_count > 0 {
                stack
                    .pop()
                    .map(|v| match v {
                        JsValue::String(s) => s,
                        JsValue::Undefined => ",".to_string(),
                        _ => ",".to_string(),
                    })
                    .unwrap_or_else(|| ",".to_string())
            } else {
                ",".to_string()
            };

            let result = arr
                .iter()
                .map(|v| match v {
                    JsValue::String(s) => s.clone(),
                    JsValue::Number(n) => n.to_string(),
                    JsValue::Boolean(b) => b.to_string(),
                    JsValue::Null => "null".to_string(),
                    JsValue::Undefined => "undefined".to_string(),
                    _ => "".to_string(),
                })
                .collect::<Vec<String>>()
                .join(&separator);

            Some(JsValue::String(result))
        }
        _ => None,
    }
}
