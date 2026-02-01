//! Minimal standard library setup for Script VM
//!
//! Sets up only essential globals needed for language operation:
//! - console (log, error)
//! - ByteStream (binary serialization)
//! - String.fromCharCode
//! - require (module loading)
//! - fs (minimal file I/O for bootstrap compiler)

use crate::vm::VM;
use crate::vm::value::{HeapData, HeapObject, JsValue};

pub fn setup_stdlib(vm: &mut VM) {
    setup_console(vm);
    setup_bytestream(vm);
    setup_string(vm);
    setup_fs(vm);
    setup_json(vm);
    setup_globals(vm);
    setup_map_set(vm);
}

fn setup_console(vm: &mut VM) {
    let log_idx = vm.register_native(crate::stdlib::native_log);
    let error_idx = vm.register_native(crate::stdlib::native_error);
    let console_ptr = vm.heap.len();
    let mut console_props = std::collections::HashMap::new();
    console_props.insert("log".to_string(), JsValue::NativeFunction(log_idx));
    console_props.insert("error".to_string(), JsValue::NativeFunction(error_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(console_props),
    });
    vm.call_stack[0]
        .locals
        .insert("console".into(), JsValue::Object(console_ptr));
}

fn setup_bytestream(vm: &mut VM) {
    use crate::stdlib::{
        native_byte_stream_length, native_byte_stream_patch_u32, native_byte_stream_to_array,
        native_byte_stream_write_f64, native_byte_stream_write_string, native_byte_stream_write_u8,
        native_byte_stream_write_u32, native_byte_stream_write_varint, native_create_byte_stream,
    };

    let create_byte_stream_idx = vm.register_native(native_create_byte_stream);
    let write_u8_idx = vm.register_native(native_byte_stream_write_u8);
    let write_varint_idx = vm.register_native(native_byte_stream_write_varint);
    let write_u32_idx = vm.register_native(native_byte_stream_write_u32);
    let write_f64_idx = vm.register_native(native_byte_stream_write_f64);
    let write_string_idx = vm.register_native(native_byte_stream_write_string);
    let patch_u32_idx = vm.register_native(native_byte_stream_patch_u32);
    let stream_length_idx = vm.register_native(native_byte_stream_length);
    let to_array_idx = vm.register_native(native_byte_stream_to_array);

    let byte_stream_ptr = vm.heap.len();
    let mut byte_stream_props = std::collections::HashMap::new();
    byte_stream_props.insert(
        "create".to_string(),
        JsValue::NativeFunction(create_byte_stream_idx),
    );
    byte_stream_props.insert("writeU8".to_string(), JsValue::NativeFunction(write_u8_idx));
    byte_stream_props.insert(
        "writeVarint".to_string(),
        JsValue::NativeFunction(write_varint_idx),
    );
    byte_stream_props.insert(
        "writeU32".to_string(),
        JsValue::NativeFunction(write_u32_idx),
    );
    byte_stream_props.insert(
        "writeF64".to_string(),
        JsValue::NativeFunction(write_f64_idx),
    );
    byte_stream_props.insert(
        "writeString".to_string(),
        JsValue::NativeFunction(write_string_idx),
    );
    byte_stream_props.insert(
        "patchU32".to_string(),
        JsValue::NativeFunction(patch_u32_idx),
    );
    byte_stream_props.insert(
        "length".to_string(),
        JsValue::NativeFunction(stream_length_idx),
    );
    byte_stream_props.insert("toArray".to_string(), JsValue::NativeFunction(to_array_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(byte_stream_props),
    });

    vm.call_stack[0]
        .locals
        .insert("ByteStream".into(), JsValue::Object(byte_stream_ptr));
}

fn setup_string(vm: &mut VM) {
    use crate::stdlib::{native_string_constructor, native_string_from_char_code};

    // Register the String constructor as a callable function
    let string_constructor_idx = vm.register_native(native_string_constructor);
    let string_from_char_code_idx = vm.register_native(native_string_from_char_code);

    // Create String as an object with methods
    let string_ptr = vm.heap.len();
    let mut string_props = std::collections::HashMap::new();
    string_props.insert(
        "fromCharCode".to_string(),
        JsValue::NativeFunction(string_from_char_code_idx),
    );
    // Store the constructor function for when String is called
    string_props.insert(
        "__call__".to_string(),
        JsValue::NativeFunction(string_constructor_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(string_props),
    });

    // Store the String object in globals
    vm.call_stack[0]
        .locals
        .insert("String".into(), JsValue::Object(string_ptr));

    // Also register a direct String function for calling String(value)
    vm.call_stack[0].locals.insert(
        "__String__".into(),
        JsValue::NativeFunction(string_constructor_idx),
    );
}

fn setup_fs(vm: &mut VM) {
    use crate::stdlib::{
        native_exists_sync, native_mkdir_sync, native_read_file, native_write_binary_file,
        native_write_file,
    };

    let fs_read_file_idx = vm.register_native(native_read_file);
    let fs_write_file_idx = vm.register_native(native_write_file);
    let fs_write_binary_file_idx = vm.register_native(native_write_binary_file);
    let fs_exists_sync_idx = vm.register_native(native_exists_sync);
    let fs_mkdir_sync_idx = vm.register_native(native_mkdir_sync);

    let fs_ptr = vm.heap.len();
    let mut fs_props = std::collections::HashMap::new();
    fs_props.insert(
        "readFileSync".to_string(),
        JsValue::NativeFunction(fs_read_file_idx),
    );
    fs_props.insert(
        "writeFileSync".to_string(),
        JsValue::NativeFunction(fs_write_file_idx),
    );
    fs_props.insert(
        "writeBinaryFile".to_string(),
        JsValue::NativeFunction(fs_write_binary_file_idx),
    );
    fs_props.insert(
        "existsSync".to_string(),
        JsValue::NativeFunction(fs_exists_sync_idx),
    );
    fs_props.insert(
        "mkdirSync".to_string(),
        JsValue::NativeFunction(fs_mkdir_sync_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(fs_props),
    });

    // Also add fs to global scope for direct access (fs.existsSync, fs.readFileSync, etc.)
    vm.call_stack[0]
        .locals
        .insert("fs".into(), JsValue::Object(fs_ptr));

    vm.modules.insert("fs".to_string(), JsValue::Object(fs_ptr));
}

fn setup_json(vm: &mut VM) {
    use crate::stdlib::{native_json_parse, native_json_stringify};

    let stringify_idx = vm.register_native(native_json_stringify);
    let parse_idx = vm.register_native(native_json_parse);

    let json_ptr = vm.heap.len();
    let mut json_props = std::collections::HashMap::new();
    json_props.insert(
        "stringify".to_string(),
        JsValue::NativeFunction(stringify_idx),
    );
    json_props.insert("parse".to_string(), JsValue::NativeFunction(parse_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(json_props),
    });

    vm.call_stack[0]
        .locals
        .insert("JSON".into(), JsValue::Object(json_ptr));
}

fn setup_globals(vm: &mut VM) {
    let require_idx = vm.register_native(crate::stdlib::native_require);

    vm.call_stack[0]
        .locals
        .insert("require".into(), JsValue::NativeFunction(require_idx));
}

fn setup_map_set(vm: &mut VM) {
    // Create Map constructor object
    let map_ptr = vm.heap.len();
    let mut map_props = std::collections::HashMap::new();
    // Mark this as a Map constructor for detection in Construct opcode
    map_props.insert("__type__".to_string(), JsValue::String("Map".to_string()));
    vm.heap.push(HeapObject {
        data: HeapData::Object(map_props),
    });
    vm.call_stack[0]
        .locals
        .insert("Map".into(), JsValue::Object(map_ptr));

    // Create Set constructor object
    let set_ptr = vm.heap.len();
    let mut set_props = std::collections::HashMap::new();
    // Mark this as a Set constructor for detection in Construct opcode
    set_props.insert("__type__".to_string(), JsValue::String("Set".to_string()));
    vm.heap.push(HeapObject {
        data: HeapData::Object(set_props),
    });
    vm.call_stack[0]
        .locals
        .insert("Set".into(), JsValue::Object(set_ptr));
}

/// Set script arguments as __args__ global variable.
/// Arguments are provided as strings and converted to a JS array.
pub fn set_script_args(vm: &mut VM, args: Vec<String>) {
    // Convert args to JsValue strings
    let js_args: Vec<JsValue> = args.into_iter().map(JsValue::String).collect();

    // Create array on heap (arrays are stored as Object pointing to HeapData::Array)
    let array_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Array(js_args),
    });

    // Set __args__ global (arrays use JsValue::Object pointing to array heap data)
    vm.call_stack[0]
        .locals
        .insert("__args__".into(), JsValue::Object(array_ptr));
}
