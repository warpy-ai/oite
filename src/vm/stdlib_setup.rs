//! Minimal standard library setup for Oite VM
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
    setup_process(vm);
    setup_fetch(vm);
    setup_object(vm);
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
        native_exists_sync, native_mkdir_sync, native_read_file, native_readdir_sync,
        native_stat_sync, native_write_binary_file, native_write_file,
    };

    let fs_read_file_idx = vm.register_native(native_read_file);
    let fs_write_file_idx = vm.register_native(native_write_file);
    let fs_write_binary_file_idx = vm.register_native(native_write_binary_file);
    let fs_exists_sync_idx = vm.register_native(native_exists_sync);
    let fs_mkdir_sync_idx = vm.register_native(native_mkdir_sync);
    let fs_readdir_sync_idx = vm.register_native(native_readdir_sync);
    let fs_stat_sync_idx = vm.register_native(native_stat_sync);

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
    fs_props.insert(
        "readdirSync".to_string(),
        JsValue::NativeFunction(fs_readdir_sync_idx),
    );
    fs_props.insert(
        "statSync".to_string(),
        JsValue::NativeFunction(fs_stat_sync_idx),
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

    // Also set process.argv to the same array
    if let Some(JsValue::Object(process_ptr)) = vm.call_stack[0].locals.get("process").cloned()
        && let Some(HeapObject {
            data: HeapData::Object(props),
        }) = vm.heap.get_mut(process_ptr)
    {
        props.insert("argv".to_string(), JsValue::Object(array_ptr));
    }
}

fn setup_process(vm: &mut VM) {
    use crate::stdlib::{
        native_chdir, native_cwd, native_exec, native_exit, native_getenv, native_setenv,
        native_stdin_read_bytes, native_stdin_read_line, native_stdout_write,
    };

    // Register native functions
    let getenv_idx = vm.register_native(native_getenv);
    let setenv_idx = vm.register_native(native_setenv);
    let cwd_idx = vm.register_native(native_cwd);
    let chdir_idx = vm.register_native(native_chdir);
    let exit_idx = vm.register_native(native_exit);
    let exec_idx = vm.register_native(native_exec);
    let stdin_read_line_idx = vm.register_native(native_stdin_read_line);
    let stdin_read_bytes_idx = vm.register_native(native_stdin_read_bytes);
    let stdout_write_idx = vm.register_native(native_stdout_write);

    // Create process.env object with get/set methods
    let env_ptr = vm.heap.len();
    let mut env_props = std::collections::HashMap::new();
    env_props.insert("get".to_string(), JsValue::NativeFunction(getenv_idx));
    env_props.insert("set".to_string(), JsValue::NativeFunction(setenv_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(env_props),
    });

    // Create process.stdin object
    let stdin_ptr = vm.heap.len();
    let mut stdin_props = std::collections::HashMap::new();
    stdin_props.insert(
        "readLine".to_string(),
        JsValue::NativeFunction(stdin_read_line_idx),
    );
    stdin_props.insert(
        "readBytes".to_string(),
        JsValue::NativeFunction(stdin_read_bytes_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(stdin_props),
    });

    // Create process.stdout object
    let stdout_ptr = vm.heap.len();
    let mut stdout_props = std::collections::HashMap::new();
    stdout_props.insert(
        "write".to_string(),
        JsValue::NativeFunction(stdout_write_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(stdout_props),
    });

    // Create empty argv array (will be populated by set_script_args)
    let argv_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Array(Vec::new()),
    });

    // Create process object
    let process_ptr = vm.heap.len();
    let mut process_props = std::collections::HashMap::new();
    process_props.insert("env".to_string(), JsValue::Object(env_ptr));
    process_props.insert("stdin".to_string(), JsValue::Object(stdin_ptr));
    process_props.insert("stdout".to_string(), JsValue::Object(stdout_ptr));
    process_props.insert("argv".to_string(), JsValue::Object(argv_ptr));
    process_props.insert("cwd".to_string(), JsValue::NativeFunction(cwd_idx));
    process_props.insert("chdir".to_string(), JsValue::NativeFunction(chdir_idx));
    process_props.insert("exit".to_string(), JsValue::NativeFunction(exit_idx));
    process_props.insert("exec".to_string(), JsValue::NativeFunction(exec_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(process_props),
    });

    // Add process to global scope
    vm.call_stack[0]
        .locals
        .insert("process".into(), JsValue::Object(process_ptr));

    // Also expose getenv directly as __ffi_getenv for @rolls/process
    vm.call_stack[0]
        .locals
        .insert("__ffi_getenv".into(), JsValue::NativeFunction(getenv_idx));
}

fn setup_fetch(vm: &mut VM) {
    use crate::stdlib::native_fetch;

    let fetch_idx = vm.register_native(native_fetch);

    // Add fetch as global function
    vm.call_stack[0]
        .locals
        .insert("fetch".into(), JsValue::NativeFunction(fetch_idx));

    // Also expose as __ffi_fetch for @rolls/http
    vm.call_stack[0]
        .locals
        .insert("__ffi_fetch".into(), JsValue::NativeFunction(fetch_idx));
}

fn setup_object(vm: &mut VM) {
    use crate::stdlib::native_object_keys;

    let keys_idx = vm.register_native(native_object_keys);

    // Create Object global with keys method
    let object_ptr = vm.heap.len();
    let mut object_props = std::collections::HashMap::new();
    object_props.insert("keys".to_string(), JsValue::NativeFunction(keys_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(object_props),
    });

    vm.call_stack[0]
        .locals
        .insert("Object".into(), JsValue::Object(object_ptr));
}
