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
    setup_math_date(vm);
    setup_timers(vm);
    setup_array(vm);
    setup_process(vm);
    setup_fetch(vm);
    setup_object(vm);
    setup_promise(vm);
}

/// `Promise` global: `Promise.resolve(v)`, `Promise.reject(r)`, and
/// `new Promise(executor)` (dispatched on `__type__` in `Construct`).
///
/// Async functions wrap their result with `Promise.resolve`, so without this
/// global every async function resolved to `undefined`.
fn setup_promise(vm: &mut VM) {
    use crate::vm::value::Promise;

    fn native_promise_resolve(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        // A promise is returned as it is.
        if let Some(JsValue::Promise(p)) = args.first() {
            return JsValue::Promise(p.clone());
        }
        let p = Promise::new();
        vm.resolve_promise(&p, args.into_iter().next().unwrap_or(JsValue::Undefined));
        JsValue::Promise(p)
    }

    fn native_promise_reject(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        let reason = args.into_iter().next().unwrap_or(JsValue::Undefined);
        let p = Promise::new();
        vm.settle_promise(&p, reason, false);
        JsValue::Promise(p)
    }

    fn array_items(vm: &VM, v: Option<&JsValue>) -> Vec<JsValue> {
        match v {
            Some(JsValue::Object(ptr)) => match vm.heap.get(*ptr) {
                Some(HeapObject {
                    data: HeapData::Array(items),
                }) => items.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn native_promise_all(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        let items = array_items(vm, args.first());
        JsValue::Promise(vm.promise_all(items))
    }

    fn native_promise_race(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        let result = Promise::new();
        for v in array_items(vm, args.first()) {
            let p = vm.to_promise(v);
            vm.add_reaction(&p, crate::vm::value::Reaction::Adopt(result.clone()));
        }
        JsValue::Promise(result)
    }

    let resolve_idx = vm.register_native(native_promise_resolve);
    let reject_idx = vm.register_native(native_promise_reject);
    let all_idx = vm.register_native(native_promise_all);
    let race_idx = vm.register_native(native_promise_race);
    let mut props = std::collections::HashMap::new();
    props.insert("__type__".to_string(), JsValue::String("Promise".into()));
    props.insert("resolve".to_string(), JsValue::NativeFunction(resolve_idx));
    props.insert("reject".to_string(), JsValue::NativeFunction(reject_idx));
    props.insert("all".to_string(), JsValue::NativeFunction(all_idx));
    props.insert("race".to_string(), JsValue::NativeFunction(race_idx));
    let ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(props),
    });
    vm.call_stack[0]
        .locals
        .insert("Promise".into(), JsValue::Object(ptr));
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

    let globals = &mut vm.call_stack[0].locals;
    globals.insert("require".into(), JsValue::NativeFunction(require_idx));
    globals.insert("undefined".into(), JsValue::Undefined);
    globals.insert("NaN".into(), JsValue::Number(f64::NAN));
    globals.insert("Infinity".into(), JsValue::Number(f64::INFINITY));
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

/// `Math` and `Date.now`, computed by the code native programs use
/// (`runtime::builtins`).
fn setup_math_date(vm: &mut VM) {
    use crate::runtime::builtins::{MATH_CONSTANTS, MATH_FUNCTIONS, date_now, math_call};
    use crate::vm::value::NativeFn;

    fn numbers(vm: &VM, args: &[JsValue]) -> Vec<f64> {
        args.iter()
            .map(|a| match a {
                JsValue::Object(_) => {
                    crate::vm::errors::to_number(&JsValue::String(vm.to_js_string(a)))
                }
                other => crate::vm::errors::to_number(other),
            })
            .collect()
    }
    macro_rules! math_fns {
        ($($name:literal),* $(,)?) => {
            [$(($name, (|vm: &mut VM, args: Vec<JsValue>| {
                JsValue::Number(math_call($name, &numbers(vm, &args)))
            }) as NativeFn)),*]
        };
    }
    let fns = math_fns![
        "abs", "ceil", "floor", "round", "trunc", "sign", "sqrt", "cbrt", "pow", "exp", "log",
        "log2", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "hypot", "min",
        "max", "random",
    ];
    debug_assert_eq!(fns.len(), MATH_FUNCTIONS.len());

    let mut math = std::collections::HashMap::new();
    for (name, f) in fns {
        let idx = vm.register_native(f);
        math.insert(name.to_string(), JsValue::NativeFunction(idx));
    }
    for (name, value) in MATH_CONSTANTS {
        math.insert(name.to_string(), JsValue::Number(*value));
    }
    let math_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(math),
    });

    let now_idx = vm.register_native(|_vm, _args| JsValue::Number(date_now()));
    let mut date = std::collections::HashMap::new();
    date.insert("now".to_string(), JsValue::NativeFunction(now_idx));
    let date_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(date),
    });

    let globals = &mut vm.call_stack[0].locals;
    globals.insert("Math".into(), JsValue::Object(math_ptr));
    globals.insert("Date".into(), JsValue::Object(date_ptr));
}

/// `setTimeout(callback, ms)` and `clearTimeout(id)`.
/// `Array`: `isArray` here, the methods that take callbacks in
/// `std/core/array.ot`, on `Array.prototype`.
fn setup_array(vm: &mut VM) {
    fn is_array(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        JsValue::Boolean(matches!(
            args.first(),
            Some(JsValue::Object(ptr)) if matches!(
                vm.heap.get(*ptr).map(|o| &o.data),
                Some(HeapData::Array(_))
            )
        ))
    }
    let is_array_idx = vm.register_native(is_array);
    let proto = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(std::collections::HashMap::new()),
    });
    let mut props = std::collections::HashMap::new();
    props.insert("isArray".to_string(), JsValue::NativeFunction(is_array_idx));
    props.insert("prototype".to_string(), JsValue::Object(proto));
    let class = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(props),
    });
    vm.call_stack[0]
        .locals
        .insert("Array".into(), JsValue::Object(class));
}

fn setup_timers(vm: &mut VM) {
    fn set_timeout(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        let callback = args.first().cloned().unwrap_or(JsValue::Undefined);
        let delay = match args.get(1) {
            Some(JsValue::Number(n)) if *n > 0.0 => *n as u64,
            _ => 0,
        };
        JsValue::Number(vm.schedule_timer(callback, delay) as f64)
    }
    fn clear_timeout(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        if let Some(JsValue::Number(id)) = args.first() {
            vm.cancel_timer(*id as usize);
        }
        JsValue::Undefined
    }
    let set_idx = vm.register_native(set_timeout);
    let clear_idx = vm.register_native(clear_timeout);
    let globals = &mut vm.call_stack[0].locals;
    globals.insert("setTimeout".into(), JsValue::NativeFunction(set_idx));
    globals.insert("clearTimeout".into(), JsValue::NativeFunction(clear_idx));
}
