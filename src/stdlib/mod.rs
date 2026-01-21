use crate::vm::value::{HeapData, HeapObject, JsValue, Promise};
use crate::vm::Frame;
use crate::vm::VM;

pub mod array;
pub mod date;
pub mod fs;
pub mod json;
pub mod math;
pub mod string;

pub fn native_log(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    for arg in args {
        match arg {
            JsValue::String(s) => print!("{}", s),
            JsValue::Number(n) => print!("{}", n),
            JsValue::Boolean(b) => print!("{}", b),
            JsValue::Null => print!("null"),
            JsValue::Undefined => print!("undefined"),
            JsValue::Object(ptr) => print!("Object({})", ptr),
            JsValue::Function { address, env: _ } => print!("Function({})", address),
            JsValue::NativeFunction(idx) => print!("NativeFunction({})", idx),
            JsValue::Promise(_) => print!("Promise"),
            JsValue::Accessor(_, _) => print!("Accessor"),
        }
    }
    println!();
    JsValue::Undefined
}

pub fn native_require(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(module_name)) = args.first() {
        if let Some(module) = vm.modules.get(module_name) {
            return module.clone();
        } else {
            eprintln!("Module '{}' not found", module_name);
        }
    }
    JsValue::Undefined
}

pub fn native_set_timeout(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Number(ms)), Some(callback)) = (args.first(), args.get(1)) {
        let ms = *ms as u64;
        let callback = callback.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        });
    }
    JsValue::Undefined
}

pub fn native_read_file(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(filename)) = args.first() {
        match std::fs::read_to_string(filename) {
            Ok(contents) => JsValue::String(contents),
            Err(e) => {
                eprintln!("Error reading file '{}': {}", filename, e);
                JsValue::Undefined
            }
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_write_file(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::String(filename)), Some(JsValue::String(contents))) =
        (args.first(), args.get(1))
    {
        match std::fs::write(filename, contents) {
            Ok(()) => JsValue::Boolean(true),
            Err(e) => {
                eprintln!("Error writing file '{}': {}", filename, e);
                JsValue::Boolean(false)
            }
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_write_binary_file(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::String(filename)), Some(JsValue::Object(ptr))) =
        (args.first(), args.get(1))
    {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get(*ptr)
        {
            match std::fs::write(filename, bytes) {
                Ok(()) => JsValue::Boolean(true),
                Err(e) => {
                    eprintln!("Error writing file '{}': {}", filename, e);
                    JsValue::Boolean(false)
                }
            }
        } else {
            JsValue::Undefined
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_create_byte_stream(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::ByteStream(Vec::new()),
    });
    JsValue::Object(ptr)
}

pub fn native_byte_stream_write_u8(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.first(), args.get(1))
    {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let value_u8 = *value as u8;
            bytes.push(value_u8);
            return JsValue::Undefined;
        }
    }
    JsValue::Undefined
}

pub fn native_byte_stream_write_u32(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.first(), args.get(1))
    {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let value_u32 = *value as u32;
            bytes.extend_from_slice(&value_u32.to_le_bytes());
            return JsValue::Undefined;
        }
    }
    JsValue::Undefined
}

pub fn native_byte_stream_write_varint(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.first(), args.get(1))
    {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let mut value = *value as u64;
            loop {
                let mut byte = (value & 0x7F) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }
                bytes.push(byte);
                if value == 0 {
                    break;
                }
            }
            return JsValue::Undefined;
        }
    }
    JsValue::Undefined
}

pub fn native_byte_stream_write_f64(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.first(), args.get(1))
    {
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

pub fn native_byte_stream_length(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Object(ptr)) = args.first() {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get(*ptr)
        {
            return JsValue::Number(bytes.len() as f64);
        }
    }
    JsValue::Number(0.0)
}

pub fn native_byte_stream_patch_u32(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (
        Some(JsValue::Object(ptr)),
        Some(JsValue::Number(offset)),
        Some(JsValue::Number(value)),
    ) = (args.first(), args.get(1), args.get(2))
    {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            let offset_usize = *offset as usize;
            let value_u32 = *value as u32;
            let bytes_slice = value_u32.to_le_bytes();
            if offset_usize + 4 <= bytes.len() {
                for (i, b) in bytes_slice.iter().enumerate() {
                    bytes[offset_usize + i] = *b;
                }
                return JsValue::Undefined;
            }
        }
    }
    JsValue::Undefined
}

pub fn native_byte_stream_to_array(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Object(ptr)) = args.first() {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get(*ptr)
        {
            let array: Vec<JsValue> = bytes.iter().map(|b| JsValue::Number(*b as f64)).collect();
            let arr_ptr = vm.heap.len();
            vm.heap.push(HeapObject {
                data: HeapData::Array(array),
            });
            return JsValue::Object(arr_ptr);
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

pub fn native_promise_constructor(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    // new Promise((resolve, reject) => { ... })
    // For synchronous Promise, we just create and return a pending promise
    // The executor is expected to be called synchronously before this returns
    eprintln!(
        "DEBUG: native_promise_constructor called with {} args",
        args.len()
    );

    let promise = Promise::new();
    eprintln!("DEBUG: created pending promise");

    JsValue::Promise(promise)
}

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
