//! Minimal standard library for Script core
//!
//! Contains only essential primitives needed by the language:
//! - console.log / console.error (debugging)
//! - ByteStream (binary serialization for bootstrap compiler)
//!
//! Full standard library functionality (fs, path, json, math, date, etc.)
//! will be provided by Rolls packages in the future.

use crate::vm::value::{HeapData, HeapObject, JsValue};
use crate::vm::VM;

// ============================================================================
// Console Functions
// ============================================================================

pub fn native_log(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
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

pub fn native_error(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    for arg in args {
        match arg {
            JsValue::String(s) => eprint!("{}", s),
            JsValue::Number(n) => eprint!("{}", n),
            JsValue::Boolean(b) => eprint!("{}", b),
            JsValue::Null => eprint!("null"),
            JsValue::Undefined => eprint!("undefined"),
            JsValue::Object(ptr) => eprint!("Object({})", ptr),
            JsValue::Function { address, env: _ } => eprint!("Function({})", address),
            JsValue::NativeFunction(idx) => eprint!("NativeFunction({})", idx),
            JsValue::Promise(_) => eprint!("Promise"),
            JsValue::Accessor(_, _) => eprint!("Accessor"),
        }
    }
    eprintln!();
    JsValue::Undefined
}

// ============================================================================
// Module System (minimal)
// ============================================================================

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

// ============================================================================
// File I/O (minimal - needed for bootstrap compiler output)
// ============================================================================

pub fn native_read_file(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
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

pub fn native_write_file(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
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

// ============================================================================
// ByteStream Functions (needed for bootstrap compiler bytecode emission)
// ============================================================================

pub fn native_create_byte_stream(vm: &mut VM, _args: Vec<JsValue>) -> JsValue {
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

pub fn native_byte_stream_write_string(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::String(s))) = (args.first(), args.get(1)) {
        if let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
        {
            bytes.extend_from_slice(s.as_bytes());
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

// ============================================================================
// String Utilities (minimal - needed for bootstrap compiler)
// ============================================================================

pub fn native_string_from_char_code(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
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
