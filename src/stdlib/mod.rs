//! Standard Library - Native functions for the WarpyScript VM

use crate::vm::VM;
use crate::vm::value::{HeapData, HeapObject, JsValue, Promise};

/// console.log implementation
pub fn native_log(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let output: Vec<String> = args.iter().map(|arg| format!("{:?}", arg)).collect();
    println!("LOG: {}", output.join(" "));
    JsValue::Undefined
}

/// setTimeout implementation
pub fn native_set_timeout(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if !args.is_empty() {
        let callback = args[0].clone();
        let delay_ms = args
            .get(1)
            .and_then(|v| match v {
                JsValue::Number(n) => Some(*n as u64),
                _ => None,
            })
            .unwrap_or(0);

        vm.schedule_timer(callback, delay_ms);
    }
    JsValue::Undefined
}

pub fn native_read_file(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.get(0) {
        match std::fs::read_to_string(path) {
            Ok(content) => JsValue::String(content),
            Err(e) => JsValue::String(format!("Error reading file: {}", e)),
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_write_file(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.get(0) {
        let content = args
            .get(1)
            .and_then(|v| match v {
                JsValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        match std::fs::write(path, content) {
            Ok(_) => JsValue::Undefined,
            Err(e) => JsValue::String(format!("Error writing file: {}", e)),
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_require(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(module_name)) = args.get(0) {
        let module = _vm
            .modules
            .get(module_name)
            .cloned()
            .unwrap_or(JsValue::Undefined);
        return module;
    }
    JsValue::Undefined
}

// ============================================================================
// ByteStream Native Functions for Binary Bytecode Generation
// ============================================================================

/// Create a new ByteStream buffer on the heap
pub fn native_create_byte_stream(vm: &mut VM, _args: Vec<JsValue>) -> JsValue {
    let ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::ByteStream(Vec::new()),
    });
    JsValue::Object(ptr)
}

/// Write a single byte (u8) to a ByteStream
pub fn native_byte_stream_write_u8(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(byte))) = (args.get(0), args.get(1)) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            bytes.push(*byte as u8);
            return JsValue::Undefined;
        }
    }
    JsValue::Undefined
}

/// Write a variable-length integer (varint) to a ByteStream
/// Uses LEB128-style encoding: 7 bits per byte, high bit indicates continuation
pub fn native_byte_stream_write_varint(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.get(0), args.get(1)) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let mut n = *value as u64;
            loop {
                let mut byte = (n & 0x7F) as u8;
                n >>= 7;
                if n != 0 {
                    byte |= 0x80; // Set continuation bit
                }
                bytes.push(byte);
                if n == 0 {
                    break;
                }
            }
            return JsValue::Undefined;
        }
    }
    JsValue::Undefined
}

/// Write a length-prefixed UTF-8 string to a ByteStream
pub fn native_byte_stream_write_string(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::String(s))) = (args.get(0), args.get(1)) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let str_bytes = s.as_bytes();
            let len = str_bytes.len();

            // Write length as varint
            let mut n = len as u64;
            loop {
                let mut byte = (n & 0x7F) as u8;
                n >>= 7;
                if n != 0 {
                    byte |= 0x80;
                }
                bytes.push(byte);
                if n == 0 {
                    break;
                }
            }

            // Write UTF-8 bytes
            bytes.extend_from_slice(str_bytes);
            return JsValue::Undefined;
        }
    }
    JsValue::Undefined
}

/// Get the current length of a ByteStream
pub fn native_byte_stream_length(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Object(ptr)) = args.get(0) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get(*ptr)
        {
            return JsValue::Number(bytes.len() as f64);
        }
    }
    JsValue::Number(0.0)
}

/// Convert ByteStream to a JsValue array of numbers (for debugging/inspection)
pub fn native_byte_stream_to_array(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Object(ptr)) = args.get(0) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get(*ptr)
        {
            let elements: Vec<JsValue> = bytes.iter().map(|b| JsValue::Number(*b as f64)).collect();
            let arr_ptr = vm.heap.len();
            vm.heap.push(HeapObject {
                data: HeapData::Array(elements),
            });
            return JsValue::Object(arr_ptr);
        }
    }
    JsValue::Undefined
}

/// Write ByteStream contents to a binary file
pub fn native_write_binary_file(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::String(path)), Some(JsValue::Object(ptr))) = (args.get(0), args.get(1)) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get(*ptr)
        {
            match std::fs::write(path, bytes) {
                Ok(_) => return JsValue::Boolean(true),
                Err(e) => return JsValue::String(format!("Error writing file: {}", e)),
            }
        }
    }
    JsValue::Boolean(false)
}

/// Patch a 32-bit value at a specific offset in a ByteStream (for backpatching jumps)
pub fn native_byte_stream_patch_u32(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (
        Some(JsValue::Object(ptr)),
        Some(JsValue::Number(offset)),
        Some(JsValue::Number(value)),
    ) = (args.get(0), args.get(1), args.get(2))
    {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let off = *offset as usize;
            let val = *value as u32;
            if off + 4 <= bytes.len() {
                bytes[off] = (val & 0xFF) as u8;
                bytes[off + 1] = ((val >> 8) & 0xFF) as u8;
                bytes[off + 2] = ((val >> 16) & 0xFF) as u8;
                bytes[off + 3] = ((val >> 24) & 0xFF) as u8;
                return JsValue::Boolean(true);
            }
        }
    }
    JsValue::Boolean(false)
}

/// Write a 32-bit unsigned integer (little-endian) to a ByteStream
pub fn native_byte_stream_write_u32(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.get(0), args.get(1)) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let val = *value as u32;
            bytes.push((val & 0xFF) as u8);
            bytes.push(((val >> 8) & 0xFF) as u8);
            bytes.push(((val >> 16) & 0xFF) as u8);
            bytes.push(((val >> 24) & 0xFF) as u8);
            return JsValue::Undefined;
        }
    }
    JsValue::Undefined
}

/// Write an IEEE 754 double (f64) to a ByteStream (8 bytes, little-endian)
pub fn native_byte_stream_write_f64(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.get(0), args.get(1)) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let float_bytes = value.to_le_bytes();
            bytes.extend_from_slice(&float_bytes);
            return JsValue::Undefined;
        }
    }
    JsValue::Undefined
}

pub fn native_string_from_char_code(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let mut result = String::new();

    for arg in args {
        if let JsValue::Number(code) = arg {
            let code_u32 = code as u32;
            if let Some(ch) = char::from_u32(code_u32) {
                result.push(ch);
            }
        }
    }

    JsValue::String(result)
}

// ============================================================================
// Promise Native Functions
// ============================================================================

pub fn native_promise_resolve(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let value = args.get(0).cloned().unwrap_or(JsValue::Undefined);
    let promise = Promise::with_value(value);
    JsValue::Promise(promise)
}

pub fn native_promise_reject(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let reason = args.get(0).cloned().unwrap_or(JsValue::Undefined);
    let promise = Promise::new();
    promise.set_value(reason, false);
    JsValue::Promise(promise)
}

pub fn native_promise_then(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Promise(promise)), Some(on_fulfilled)) = (args.get(0), args.get(1)) {
        let result_promise = promise.then(Some(on_fulfilled.clone()));
        return JsValue::Promise(result_promise);
    }
    JsValue::Undefined
}

pub fn native_promise_catch(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Promise(promise)), Some(on_rejected)) = (args.get(0), args.get(1)) {
        let result_promise = promise.catch(Some(on_rejected.clone()));
        return JsValue::Promise(result_promise);
    }
    JsValue::Undefined
}

pub fn native_promise_all(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Object(ptr)) = args.get(0) {
        if let Some(HeapObject {
            data: HeapData::Array(items),
            ..
        }) = vm.heap.get(*ptr)
        {
            let mut all_pending = true;
            let mut results = Vec::new();

            for item in items {
                if let JsValue::Promise(p) = item {
                    match p.get_state() {
                        crate::vm::value::PromiseState::Fulfilled => {
                            results.push(p.get_value().unwrap_or(JsValue::Undefined));
                        }
                        crate::vm::value::PromiseState::Rejected => {
                            return JsValue::Promise(Promise::with_value(JsValue::Undefined));
                        }
                        crate::vm::value::PromiseState::Pending => {
                            all_pending = false;
                        }
                    }
                } else {
                    results.push(item.clone());
                }
            }

            if all_pending {
                let result_array_ptr = vm.heap.len();
                vm.heap.push(HeapObject {
                    data: HeapData::Array(results),
                });
                return JsValue::Object(result_array_ptr);
            }
        }
    }
    JsValue::Undefined
}
