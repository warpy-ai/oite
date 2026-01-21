use crate::vm::value::{HeapData, HeapObject, JsValue};
use crate::vm::VM;

pub fn setup_stdlib(vm: &mut VM) {
    setup_console(vm);
    setup_fs(vm);
    setup_json(vm);
    setup_math(vm);
    setup_date(vm);
    setup_bytestream(vm);
    setup_string(vm);
    setup_promise(vm);
    setup_path(vm);
    setup_globals(vm);
}

fn setup_console(vm: &mut VM) {
    let log_idx = vm.register_native(crate::stdlib::native_log);
    let console_ptr = vm.heap.len();
    let mut console_props = std::collections::HashMap::new();
    console_props.insert("log".to_string(), JsValue::NativeFunction(log_idx));
    vm.heap.push(HeapObject {
        data: HeapData::Object(console_props),
    });
    vm.call_stack[0]
        .locals
        .insert("console".into(), JsValue::Object(console_ptr));
}

fn setup_fs(vm: &mut VM) {
    use crate::stdlib::fs::{
        native_fs_append_file_sync, native_fs_copy_file_sync, native_fs_exists_async,
        native_fs_exists_sync, native_fs_mkdir_sync, native_fs_read_dir_async,
        native_fs_read_file_async, native_fs_read_file_sync, native_fs_readdir_sync,
        native_fs_rename, native_fs_rmdir, native_fs_stat_sync, native_fs_unlink,
        native_fs_write_file_async, native_fs_write_file_sync,
    };

    let fs_exists_sync_idx = vm.register_native(native_fs_exists_sync);
    let fs_mkdir_sync_idx = vm.register_native(native_fs_mkdir_sync);
    let fs_readdir_sync_idx = vm.register_native(native_fs_readdir_sync);
    let fs_unlink_idx = vm.register_native(native_fs_unlink);
    let fs_rmdir_idx = vm.register_native(native_fs_rmdir);
    let fs_stat_sync_idx = vm.register_native(native_fs_stat_sync);
    let fs_append_file_sync_idx = vm.register_native(native_fs_append_file_sync);
    let fs_copy_file_sync_idx = vm.register_native(native_fs_copy_file_sync);
    let fs_rename_idx = vm.register_native(native_fs_rename);
    let fs_read_file_sync_idx = vm.register_native(native_fs_read_file_sync);
    let fs_write_file_sync_idx = vm.register_native(native_fs_write_file_sync);
    let fs_exists_async_idx = vm.register_native(native_fs_exists_async);
    let fs_read_file_async_idx = vm.register_native(native_fs_read_file_async);
    let fs_write_file_async_idx = vm.register_native(native_fs_write_file_async);
    let fs_read_dir_async_idx = vm.register_native(native_fs_read_dir_async);

    let fs_ptr = vm.heap.len();
    let mut fs_props = std::collections::HashMap::new();
    fs_props.insert(
        "readFileSync".to_string(),
        JsValue::NativeFunction(fs_read_file_sync_idx),
    );
    fs_props.insert(
        "writeFileSync".to_string(),
        JsValue::NativeFunction(fs_write_file_sync_idx),
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
    fs_props.insert("unlink".to_string(), JsValue::NativeFunction(fs_unlink_idx));
    fs_props.insert("rmdir".to_string(), JsValue::NativeFunction(fs_rmdir_idx));
    fs_props.insert(
        "statSync".to_string(),
        JsValue::NativeFunction(fs_stat_sync_idx),
    );
    fs_props.insert(
        "appendFileSync".to_string(),
        JsValue::NativeFunction(fs_append_file_sync_idx),
    );
    fs_props.insert(
        "copyFileSync".to_string(),
        JsValue::NativeFunction(fs_copy_file_sync_idx),
    );
    fs_props.insert("rename".to_string(), JsValue::NativeFunction(fs_rename_idx));
    fs_props.insert(
        "exists".to_string(),
        JsValue::NativeFunction(fs_exists_async_idx),
    );
    fs_props.insert(
        "readFile".to_string(),
        JsValue::NativeFunction(fs_read_file_async_idx),
    );
    fs_props.insert(
        "writeFile".to_string(),
        JsValue::NativeFunction(fs_write_file_async_idx),
    );
    fs_props.insert(
        "readDir".to_string(),
        JsValue::NativeFunction(fs_read_dir_async_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(fs_props),
    });

    vm.modules.insert("fs".to_string(), JsValue::Object(fs_ptr));
}

fn setup_json(vm: &mut VM) {
    use crate::stdlib::json::{native_json_parse, native_json_stringify};

    let json_parse_idx = vm.register_native(native_json_parse);
    let json_stringify_idx = vm.register_native(native_json_stringify);
    let json_ptr = vm.heap.len();
    let mut json_props = std::collections::HashMap::new();
    json_props.insert("parse".to_string(), JsValue::NativeFunction(json_parse_idx));
    json_props.insert(
        "stringify".to_string(),
        JsValue::NativeFunction(json_stringify_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(json_props),
    });
    vm.call_stack[0]
        .locals
        .insert("JSON".into(), JsValue::Object(json_ptr));
}

fn setup_math(vm: &mut VM) {
    use crate::stdlib::math::{
        native_math_abs, native_math_acos, native_math_acosh, native_math_asin, native_math_asinh,
        native_math_atan, native_math_atan2, native_math_atanh, native_math_cbrt, native_math_ceil,
        native_math_clz32, native_math_cos, native_math_cosh, native_math_exp, native_math_expm1,
        native_math_floor, native_math_fround, native_math_hypot, native_math_imul,
        native_math_log, native_math_log10, native_math_log1p, native_math_log2, native_math_max,
        native_math_min, native_math_pow, native_math_random, native_math_round, native_math_sign,
        native_math_sin, native_math_sinh, native_math_sqrt, native_math_tan, native_math_tanh,
        native_math_trunc,
    };

    let math_abs_idx = vm.register_native(native_math_abs);
    let math_floor_idx = vm.register_native(native_math_floor);
    let math_ceil_idx = vm.register_native(native_math_ceil);
    let math_round_idx = vm.register_native(native_math_round);
    let math_trunc_idx = vm.register_native(native_math_trunc);
    let math_max_idx = vm.register_native(native_math_max);
    let math_min_idx = vm.register_native(native_math_min);
    let math_pow_idx = vm.register_native(native_math_pow);
    let math_sqrt_idx = vm.register_native(native_math_sqrt);
    let math_cbrt_idx = vm.register_native(native_math_cbrt);
    let math_random_idx = vm.register_native(native_math_random);
    let math_sin_idx = vm.register_native(native_math_sin);
    let math_cos_idx = vm.register_native(native_math_cos);
    let math_tan_idx = vm.register_native(native_math_tan);
    let math_asin_idx = vm.register_native(native_math_asin);
    let math_acos_idx = vm.register_native(native_math_acos);
    let math_atan_idx = vm.register_native(native_math_atan);
    let math_atan2_idx = vm.register_native(native_math_atan2);
    let math_exp_idx = vm.register_native(native_math_exp);
    let math_expm1_idx = vm.register_native(native_math_expm1);
    let math_log_idx = vm.register_native(native_math_log);
    let math_log10_idx = vm.register_native(native_math_log10);
    let math_log1p_idx = vm.register_native(native_math_log1p);
    let math_log2_idx = vm.register_native(native_math_log2);
    let math_sign_idx = vm.register_native(native_math_sign);
    let math_hypot_idx = vm.register_native(native_math_hypot);
    let math_imul_idx = vm.register_native(native_math_imul);
    let math_fround_idx = vm.register_native(native_math_fround);
    let math_clz32_idx = vm.register_native(native_math_clz32);
    let math_sinh_idx = vm.register_native(native_math_sinh);
    let math_cosh_idx = vm.register_native(native_math_cosh);
    let math_tanh_idx = vm.register_native(native_math_tanh);
    let math_asinh_idx = vm.register_native(native_math_asinh);
    let math_acosh_idx = vm.register_native(native_math_acosh);
    let math_atanh_idx = vm.register_native(native_math_atanh);

    let math_ptr = vm.heap.len();
    let mut math_props = std::collections::HashMap::new();
    math_props.insert("abs".to_string(), JsValue::NativeFunction(math_abs_idx));
    math_props.insert("floor".to_string(), JsValue::NativeFunction(math_floor_idx));
    math_props.insert("ceil".to_string(), JsValue::NativeFunction(math_ceil_idx));
    math_props.insert("round".to_string(), JsValue::NativeFunction(math_round_idx));
    math_props.insert("trunc".to_string(), JsValue::NativeFunction(math_trunc_idx));
    math_props.insert("max".to_string(), JsValue::NativeFunction(math_max_idx));
    math_props.insert("min".to_string(), JsValue::NativeFunction(math_min_idx));
    math_props.insert("pow".to_string(), JsValue::NativeFunction(math_pow_idx));
    math_props.insert("sqrt".to_string(), JsValue::NativeFunction(math_sqrt_idx));
    math_props.insert("cbrt".to_string(), JsValue::NativeFunction(math_cbrt_idx));
    math_props.insert(
        "random".to_string(),
        JsValue::NativeFunction(math_random_idx),
    );
    math_props.insert("sin".to_string(), JsValue::NativeFunction(math_sin_idx));
    math_props.insert("cos".to_string(), JsValue::NativeFunction(math_cos_idx));
    math_props.insert("tan".to_string(), JsValue::NativeFunction(math_tan_idx));
    math_props.insert("asin".to_string(), JsValue::NativeFunction(math_asin_idx));
    math_props.insert("acos".to_string(), JsValue::NativeFunction(math_acos_idx));
    math_props.insert("atan".to_string(), JsValue::NativeFunction(math_atan_idx));
    math_props.insert("atan2".to_string(), JsValue::NativeFunction(math_atan2_idx));
    math_props.insert("exp".to_string(), JsValue::NativeFunction(math_exp_idx));
    math_props.insert("expm1".to_string(), JsValue::NativeFunction(math_expm1_idx));
    math_props.insert("log".to_string(), JsValue::NativeFunction(math_log_idx));
    math_props.insert("log10".to_string(), JsValue::NativeFunction(math_log10_idx));
    math_props.insert("log1p".to_string(), JsValue::NativeFunction(math_log1p_idx));
    math_props.insert("log2".to_string(), JsValue::NativeFunction(math_log2_idx));
    math_props.insert("sign".to_string(), JsValue::NativeFunction(math_sign_idx));
    math_props.insert("hypot".to_string(), JsValue::NativeFunction(math_hypot_idx));
    math_props.insert("imul".to_string(), JsValue::NativeFunction(math_imul_idx));
    math_props.insert(
        "fround".to_string(),
        JsValue::NativeFunction(math_fround_idx),
    );
    math_props.insert("clz32".to_string(), JsValue::NativeFunction(math_clz32_idx));
    math_props.insert("sinh".to_string(), JsValue::NativeFunction(math_sinh_idx));
    math_props.insert("cosh".to_string(), JsValue::NativeFunction(math_cosh_idx));
    math_props.insert("tanh".to_string(), JsValue::NativeFunction(math_tanh_idx));
    math_props.insert("asinh".to_string(), JsValue::NativeFunction(math_asinh_idx));
    math_props.insert("acosh".to_string(), JsValue::NativeFunction(math_acosh_idx));
    math_props.insert("atanh".to_string(), JsValue::NativeFunction(math_atanh_idx));
    math_props.insert("PI".to_string(), JsValue::Number(std::f64::consts::PI));
    math_props.insert("E".to_string(), JsValue::Number(std::f64::consts::E));
    math_props.insert("LN2".to_string(), JsValue::Number(std::f64::consts::LN_2));
    math_props.insert("LN10".to_string(), JsValue::Number(std::f64::consts::LN_10));
    math_props.insert(
        "LOG2E".to_string(),
        JsValue::Number(std::f64::consts::LOG2_E),
    );
    math_props.insert(
        "LOG10E".to_string(),
        JsValue::Number(std::f64::consts::LOG10_E),
    );
    math_props.insert(
        "SQRT1_2".to_string(),
        JsValue::Number(std::f64::consts::FRAC_1_SQRT_2),
    );
    math_props.insert(
        "SQRT2".to_string(),
        JsValue::Number(std::f64::consts::SQRT_2),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(math_props),
    });
    vm.call_stack[0]
        .locals
        .insert("Math".into(), JsValue::Object(math_ptr));
}

fn setup_date(vm: &mut VM) {
    use crate::stdlib::date::{
        native_date_constructor, native_date_get_date, native_date_get_day,
        native_date_get_full_year, native_date_get_hours, native_date_get_milliseconds,
        native_date_get_minutes, native_date_get_month, native_date_get_seconds,
        native_date_get_time, native_date_get_timezone_offset, native_date_now, native_date_parse,
        native_date_set_date, native_date_set_full_year, native_date_set_hours,
        native_date_set_milliseconds, native_date_set_minutes, native_date_set_month,
        native_date_set_seconds, native_date_set_time, native_date_to_iso_string,
        native_date_to_json, native_date_to_string, native_date_to_utc_string, native_date_utc,
        native_date_value_of,
    };

    let date_constructor_idx = vm.register_native(native_date_constructor);
    let date_now_idx = vm.register_native(native_date_now);
    let date_parse_idx = vm.register_native(native_date_parse);
    let date_utc_idx = vm.register_native(native_date_utc);
    let date_get_time_idx = vm.register_native(native_date_get_time);
    let date_get_full_year_idx = vm.register_native(native_date_get_full_year);
    let date_get_month_idx = vm.register_native(native_date_get_month);
    let date_get_date_idx = vm.register_native(native_date_get_date);
    let date_get_day_idx = vm.register_native(native_date_get_day);
    let date_get_hours_idx = vm.register_native(native_date_get_hours);
    let date_get_minutes_idx = vm.register_native(native_date_get_minutes);
    let date_get_seconds_idx = vm.register_native(native_date_get_seconds);
    let date_get_milliseconds_idx = vm.register_native(native_date_get_milliseconds);
    let date_get_timezone_offset_idx = vm.register_native(native_date_get_timezone_offset);
    let date_set_time_idx = vm.register_native(native_date_set_time);
    let date_set_full_year_idx = vm.register_native(native_date_set_full_year);
    let date_set_month_idx = vm.register_native(native_date_set_month);
    let date_set_date_idx = vm.register_native(native_date_set_date);
    let date_set_hours_idx = vm.register_native(native_date_set_hours);
    let date_set_minutes_idx = vm.register_native(native_date_set_minutes);
    let date_set_seconds_idx = vm.register_native(native_date_set_seconds);
    let date_set_milliseconds_idx = vm.register_native(native_date_set_milliseconds);
    let date_to_iso_string_idx = vm.register_native(native_date_to_iso_string);
    let date_to_string_idx = vm.register_native(native_date_to_string);
    let date_to_utc_string_idx = vm.register_native(native_date_to_utc_string);
    let date_value_of_idx = vm.register_native(native_date_value_of);
    let date_to_json_idx = vm.register_native(native_date_to_json);

    let date_proto_ptr = vm.heap.len();
    let mut date_proto_props = std::collections::HashMap::new();
    date_proto_props.insert(
        "constructor".to_string(),
        JsValue::NativeFunction(date_constructor_idx),
    );
    date_proto_props.insert(
        "getTime".to_string(),
        JsValue::NativeFunction(date_get_time_idx),
    );
    date_proto_props.insert(
        "getFullYear".to_string(),
        JsValue::NativeFunction(date_get_full_year_idx),
    );
    date_proto_props.insert(
        "getMonth".to_string(),
        JsValue::NativeFunction(date_get_month_idx),
    );
    date_proto_props.insert(
        "getDate".to_string(),
        JsValue::NativeFunction(date_get_date_idx),
    );
    date_proto_props.insert(
        "getDay".to_string(),
        JsValue::NativeFunction(date_get_day_idx),
    );
    date_proto_props.insert(
        "getHours".to_string(),
        JsValue::NativeFunction(date_get_hours_idx),
    );
    date_proto_props.insert(
        "getMinutes".to_string(),
        JsValue::NativeFunction(date_get_minutes_idx),
    );
    date_proto_props.insert(
        "getSeconds".to_string(),
        JsValue::NativeFunction(date_get_seconds_idx),
    );
    date_proto_props.insert(
        "getMilliseconds".to_string(),
        JsValue::NativeFunction(date_get_milliseconds_idx),
    );
    date_proto_props.insert(
        "getTimezoneOffset".to_string(),
        JsValue::NativeFunction(date_get_timezone_offset_idx),
    );
    date_proto_props.insert(
        "setTime".to_string(),
        JsValue::NativeFunction(date_set_time_idx),
    );
    date_proto_props.insert(
        "setFullYear".to_string(),
        JsValue::NativeFunction(date_set_full_year_idx),
    );
    date_proto_props.insert(
        "setMonth".to_string(),
        JsValue::NativeFunction(date_set_month_idx),
    );
    date_proto_props.insert(
        "setDate".to_string(),
        JsValue::NativeFunction(date_set_date_idx),
    );
    date_proto_props.insert(
        "setHours".to_string(),
        JsValue::NativeFunction(date_set_hours_idx),
    );
    date_proto_props.insert(
        "setMinutes".to_string(),
        JsValue::NativeFunction(date_set_minutes_idx),
    );
    date_proto_props.insert(
        "setSeconds".to_string(),
        JsValue::NativeFunction(date_set_seconds_idx),
    );
    date_proto_props.insert(
        "setMilliseconds".to_string(),
        JsValue::NativeFunction(date_set_milliseconds_idx),
    );
    date_proto_props.insert(
        "toISOString".to_string(),
        JsValue::NativeFunction(date_to_iso_string_idx),
    );
    date_proto_props.insert(
        "toString".to_string(),
        JsValue::NativeFunction(date_to_string_idx),
    );
    date_proto_props.insert(
        "toUTCString".to_string(),
        JsValue::NativeFunction(date_to_utc_string_idx),
    );
    date_proto_props.insert(
        "valueOf".to_string(),
        JsValue::NativeFunction(date_value_of_idx),
    );
    date_proto_props.insert(
        "toJSON".to_string(),
        JsValue::NativeFunction(date_to_json_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(date_proto_props),
    });

    let date_ptr = vm.heap.len();
    let mut date_props = std::collections::HashMap::new();
    date_props.insert("now".to_string(), JsValue::NativeFunction(date_now_idx));
    date_props.insert("parse".to_string(), JsValue::NativeFunction(date_parse_idx));
    date_props.insert("UTC".to_string(), JsValue::NativeFunction(date_utc_idx));
    date_props.insert(
        "constructor".to_string(),
        JsValue::NativeFunction(date_constructor_idx),
    );
    date_props.insert("prototype".to_string(), JsValue::Object(date_proto_ptr));
    vm.heap.push(HeapObject {
        data: HeapData::Object(date_props),
    });
    vm.call_stack[0]
        .locals
        .insert("Date".into(), JsValue::Object(date_ptr));
}

fn setup_bytestream(vm: &mut VM) {
    use crate::stdlib::{
        native_byte_stream_length, native_byte_stream_patch_u32, native_byte_stream_to_array,
        native_byte_stream_write_f64, native_byte_stream_write_string,
        native_byte_stream_write_u32, native_byte_stream_write_u8, native_byte_stream_write_varint,
        native_create_byte_stream,
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
    use crate::stdlib::native_string_from_char_code;

    let string_from_char_code_idx = vm.register_native(native_string_from_char_code);
    let string_ptr = vm.heap.len();
    let mut string_props = std::collections::HashMap::new();
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
}

fn setup_promise(vm: &mut VM) {
    use crate::stdlib::{
        native_promise_all, native_promise_catch, native_promise_constructor,
        native_promise_reject, native_promise_resolve, native_promise_then,
    };

    let promise_constructor_idx = vm.register_native(native_promise_constructor);
    let promise_resolve_idx = vm.register_native(native_promise_resolve);
    let promise_reject_idx = vm.register_native(native_promise_reject);
    let promise_then_idx = vm.register_native(native_promise_then);
    let promise_catch_idx = vm.register_native(native_promise_catch);
    let promise_all_idx = vm.register_native(native_promise_all);

    let promise_ptr = vm.heap.len();
    let mut promise_props = std::collections::HashMap::new();
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

fn setup_path(vm: &mut VM) {
    use crate::stdlib::path::{
        native_path_basename, native_path_dirname, native_path_extname, native_path_format,
        native_path_is_absolute, native_path_join, native_path_parse, native_path_relative,
        native_path_resolve, native_path_to_namespaced_path,
    };

    let path_join_idx = vm.register_native(native_path_join);
    let path_resolve_idx = vm.register_native(native_path_resolve);
    let path_dirname_idx = vm.register_native(native_path_dirname);
    let path_basename_idx = vm.register_native(native_path_basename);
    let path_extname_idx = vm.register_native(native_path_extname);
    let path_parse_idx = vm.register_native(native_path_parse);
    let path_format_idx = vm.register_native(native_path_format);
    let path_is_absolute_idx = vm.register_native(native_path_is_absolute);
    let path_relative_idx = vm.register_native(native_path_relative);
    let path_to_namespaced_path_idx = vm.register_native(native_path_to_namespaced_path);

    let path_ptr = vm.heap.len();
    let mut path_props = std::collections::HashMap::new();
    path_props.insert("join".to_string(), JsValue::NativeFunction(path_join_idx));
    path_props.insert(
        "resolve".to_string(),
        JsValue::NativeFunction(path_resolve_idx),
    );
    path_props.insert(
        "dirname".to_string(),
        JsValue::NativeFunction(path_dirname_idx),
    );
    path_props.insert(
        "basename".to_string(),
        JsValue::NativeFunction(path_basename_idx),
    );
    path_props.insert(
        "extname".to_string(),
        JsValue::NativeFunction(path_extname_idx),
    );
    path_props.insert("parse".to_string(), JsValue::NativeFunction(path_parse_idx));
    path_props.insert(
        "format".to_string(),
        JsValue::NativeFunction(path_format_idx),
    );
    path_props.insert(
        "isAbsolute".to_string(),
        JsValue::NativeFunction(path_is_absolute_idx),
    );
    path_props.insert(
        "relative".to_string(),
        JsValue::NativeFunction(path_relative_idx),
    );
    path_props.insert(
        "toNamespacedPath".to_string(),
        JsValue::NativeFunction(path_to_namespaced_path_idx),
    );
    vm.heap.push(HeapObject {
        data: HeapData::Object(path_props),
    });
    vm.call_stack[0]
        .locals
        .insert("path".into(), JsValue::Object(path_ptr));
}

fn setup_globals(vm: &mut VM) {
    let timeout_idx = vm.register_native(crate::stdlib::native_set_timeout);
    let require_idx = vm.register_native(crate::stdlib::native_require);

    vm.call_stack[0]
        .locals
        .insert("setTimeout".into(), JsValue::NativeFunction(timeout_idx));
    vm.call_stack[0]
        .locals
        .insert("require".into(), JsValue::NativeFunction(require_idx));
}
