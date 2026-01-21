use crate::vm::value::{HeapData, HeapObject, JsValue};
use crate::vm::VM;

pub fn native_json_parse(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(json_str)) = args.first() {
        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(value) => json_to_jsvalue(vm, value),
            Err(e) => {
                eprintln!("JSON parse error: {}", e);
                JsValue::Undefined
            }
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_json_stringify(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(value) = args.first() {
        let json_str = jsvalue_to_json(value, vm);
        JsValue::String(json_str)
    } else {
        JsValue::Undefined
    }
}

fn json_to_jsvalue(vm: &mut VM, value: serde_json::Value) -> JsValue {
    match value {
        serde_json::Value::Null => JsValue::Null,
        serde_json::Value::Bool(b) => JsValue::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                JsValue::Number(f)
            } else {
                JsValue::Number(0.0)
            }
        }
        serde_json::Value::String(s) => JsValue::String(s),
        serde_json::Value::Array(arr) => {
            let js_arr: Vec<JsValue> = arr.into_iter().map(|v| json_to_jsvalue(vm, v)).collect();
            let arr_ptr = vm.heap.len();
            vm.heap.push(HeapObject {
                data: HeapData::Array(js_arr),
            });
            JsValue::Object(arr_ptr)
        }
        serde_json::Value::Object(obj) => {
            let mut js_obj = std::collections::HashMap::new();
            for (k, v) in obj {
                js_obj.insert(k, json_to_jsvalue(vm, v));
            }
            let obj_ptr = vm.heap.len();
            vm.heap.push(HeapObject {
                data: HeapData::Object(js_obj),
            });
            JsValue::Object(obj_ptr)
        }
    }
}

fn jsvalue_to_json(value: &JsValue, vm: &VM) -> String {
    match value {
        JsValue::Null => "null".to_string(),
        JsValue::Undefined => "null".to_string(),
        JsValue::Boolean(b) => b.to_string(),
        JsValue::Number(n) => n.to_string(),
        JsValue::String(s) => {
            let mut escaped = String::new();
            for c in s.chars() {
                match c {
                    '"' => escaped.push_str("\\\""),
                    '\\' => escaped.push_str("\\\\"),
                    '\n' => escaped.push_str("\\n"),
                    '\r' => escaped.push_str("\\r"),
                    '\t' => escaped.push_str("\\t"),
                    '\u{0008}' => escaped.push_str("\\b"),
                    '\u{000C}' => escaped.push_str("\\f"),
                    _ => escaped.push(c),
                }
            }
            format!("\"{}\"", escaped)
        }
        JsValue::Object(ptr) => {
            if let Some(HeapObject { data }) = vm.heap.get(*ptr) {
                match data {
                    HeapData::Array(arr) => {
                        let items: Vec<String> =
                            arr.iter().map(|v| jsvalue_to_json(v, vm)).collect();
                        format!("[{}]", items.join(","))
                    }
                    HeapData::Object(obj) => {
                        let pairs: Vec<String> = obj
                            .iter()
                            .map(|(k, v)| format!("\"{}\":{}", k, jsvalue_to_json(v, vm)))
                            .collect();
                        format!("{{{}}}", pairs.join(","))
                    }
                    _ => "null".to_string(),
                }
            } else {
                "null".to_string()
            }
        }
        JsValue::Function { .. } => "null".to_string(),
        JsValue::NativeFunction(_) => "null".to_string(),
        JsValue::Promise(_) => "null".to_string(),
        JsValue::Accessor(_, _) => "null".to_string(),
    }
}
