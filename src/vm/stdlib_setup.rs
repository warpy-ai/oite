use crate::stdlib::{
    native_byte_stream_length, native_byte_stream_patch_u32, native_byte_stream_to_array,
    native_byte_stream_write_f64, native_byte_stream_write_u32, native_byte_stream_write_u8,
    native_byte_stream_write_varint, native_create_byte_stream, native_log, native_promise_all,
    native_promise_catch, native_promise_constructor, native_promise_reject,
    native_promise_resolve, native_promise_then, native_read_file, native_require,
    native_set_timeout, native_string_from_char_code, native_write_binary_file, native_write_file,
};
use crate::vm::value::{HeapData, HeapObject, JsValue, NativeFn};
use crate::vm::VM;
use std::collections::HashMap;

pub fn setup_stdlib(vm: &mut VM) {
    let log_idx = vm.register_native(native_log);
    let timeout_idx = vm.register_native(native_set_timeout);
    let read_idx = vm.register_native(native_read_file);
    let write_idx = vm.register_native(native_write_file);
    let require_idx = vm.register_native(native_require);

    let create_byte_stream_idx = vm.register_native(native_create_byte_stream);
    let write_u8_idx = vm.register_native(native_byte_stream_write_u8);
    let write_varint_idx = vm.register_native(native_byte_stream_write_varint);
    let write_u32_idx = vm.register_native(native_byte_stream_write_u32);
    let write_f64_idx = vm.register_native(native_byte_stream_write_f64);
    let patch_u32_idx = vm.register_native(native_byte_stream_patch_u32);
    let stream_length_idx = vm.register_native(native_byte_stream_length);
    let to_array_idx = vm.register_native(native_byte_stream_to_array);
    let write_binary_file_idx = vm.register_native(native_write_binary_file);

    let string_from_char_code_idx = vm.register_native(native_string_from_char_code);

    let console_ptr = vm.heap.len();
    let mut console_props = HashMap::new();
    console_props.insert("log".to_string(), JsValue::NativeFunction(log_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(console_props),
    });

    let fs_ptr = vm.heap.len();
    let mut fs_props = HashMap::new();
    fs_props.insert(
        "readFileSync".to_string(),
        JsValue::NativeFunction(read_idx),
    );
    fs_props.insert(
        "writeFileSync".to_string(),
        JsValue::NativeFunction(write_idx),
    );
    fs_props.insert(
        "writeBinaryFile".to_string(),
        JsValue::NativeFunction(write_binary_file_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(fs_props),
    });

    let byte_stream_ptr = vm.heap.len();
    let mut byte_stream_props = HashMap::new();
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

    let globals = vm.call_stack.first_mut().expect("Missing global frame");
    globals
        .locals
        .insert("console".into(), JsValue::Object(console_ptr));
    globals
        .locals
        .insert("setTimeout".into(), JsValue::NativeFunction(timeout_idx));
    globals
        .locals
        .insert("require".into(), JsValue::NativeFunction(require_idx));
    globals
        .locals
        .insert("ByteStream".into(), JsValue::Object(byte_stream_ptr));

    vm.modules.insert("fs".to_string(), JsValue::Object(fs_ptr));

    let string_ptr = vm.heap.len();
    let mut string_props = HashMap::new();
    string_props.insert(
        "fromCharCode".to_string(),
        JsValue::NativeFunction(string_from_char_code_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(string_props),
    });
    vm.call_stack[0]
        .locals
        .insert("String".into(), JsValue::Object(string_ptr));

    let promise_constructor_idx = vm.register_native(native_promise_constructor);
    let promise_resolve_idx = vm.register_native(native_promise_resolve);
    let promise_reject_idx = vm.register_native(native_promise_reject);
    let promise_then_idx = vm.register_native(native_promise_then);
    let promise_catch_idx = vm.register_native(native_promise_catch);
    let promise_all_idx = vm.register_native(native_promise_all);

    let promise_ptr = vm.heap.len();
    let mut promise_props = HashMap::new();

    promise_props.insert(
        "constructor".to_string(),
        JsValue::NativeFunction(promise_constructor_idx),
    );
    promise_props.insert(
        "resolve".to_string(),
        JsValue::NativeFunction(promise_resolve_idx),
    );
    promise_props.insert(
        "reject".to_string(),
        JsValue::NativeFunction(promise_reject_idx),
    );
    promise_props.insert(
        "then".to_string(),
        JsValue::NativeFunction(promise_then_idx),
    );
    promise_props.insert(
        "catch".to_string(),
        JsValue::NativeFunction(promise_catch_idx),
    );
    promise_props.insert("all".to_string(), JsValue::NativeFunction(promise_all_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(promise_props),
    });
    vm.call_stack[0]
        .locals
        .insert("Promise".into(), JsValue::Object(promise_ptr));
}
