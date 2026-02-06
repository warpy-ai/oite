//! Minimal standard library for Oite core
//!
//! Contains only essential primitives needed by the language:
//! - console.log / console.error (debugging)
//! - ByteStream (binary serialization for bootstrap compiler)
//!
//! Full standard library functionality (fs, path, json, math, date, etc.)
//! will be provided by Rolls packages in the future.

use crate::vm::VM;
use crate::vm::value::{HeapData, HeapObject, JsValue};

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

pub fn native_exists_sync(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        JsValue::Boolean(std::path::Path::new(path).exists())
    } else {
        JsValue::Boolean(false)
    }
}

pub fn native_mkdir_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        // Check for { recursive: true } option
        let recursive = if let Some(JsValue::Object(ptr)) = args.get(1) {
            if let Some(HeapObject {
                data: HeapData::Object(props),
            }) = vm.heap.get(*ptr)
            {
                matches!(props.get("recursive"), Some(JsValue::Boolean(true)))
            } else {
                false
            }
        } else {
            false
        };

        let result = if recursive {
            std::fs::create_dir_all(path)
        } else {
            std::fs::create_dir(path)
        };

        match result {
            Ok(()) => JsValue::Undefined,
            Err(e) => {
                eprintln!("Error creating directory '{}': {}", path, e);
                JsValue::Undefined
            }
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_readdir_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut files: Vec<JsValue> = Vec::new();
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        files.push(JsValue::String(name.to_string()));
                    }
                }
                let arr_ptr = vm.heap.len();
                vm.heap.push(HeapObject {
                    data: HeapData::Array(files),
                });
                JsValue::Object(arr_ptr)
            }
            Err(e) => {
                eprintln!("Error reading directory '{}': {}", path, e);
                // Return empty array on error
                let arr_ptr = vm.heap.len();
                vm.heap.push(HeapObject {
                    data: HeapData::Array(Vec::new()),
                });
                JsValue::Object(arr_ptr)
            }
        }
    } else {
        // Return empty array if no path provided
        let arr_ptr = vm.heap.len();
        vm.heap.push(HeapObject {
            data: HeapData::Array(Vec::new()),
        });
        JsValue::Object(arr_ptr)
    }
}

pub fn native_stat_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        match std::fs::metadata(path) {
            Ok(metadata) => {
                let mut stat_props = std::collections::HashMap::new();

                let (is_dir_fn, is_file_fn) = if metadata.is_dir() {
                    (
                        vm.register_native(|_vm: &mut VM, _args: Vec<JsValue>| {
                            JsValue::Boolean(true)
                        }),
                        vm.register_native(|_vm: &mut VM, _args: Vec<JsValue>| {
                            JsValue::Boolean(false)
                        }),
                    )
                } else {
                    (
                        vm.register_native(|_vm: &mut VM, _args: Vec<JsValue>| {
                            JsValue::Boolean(false)
                        }),
                        vm.register_native(|_vm: &mut VM, _args: Vec<JsValue>| {
                            JsValue::Boolean(true)
                        }),
                    )
                };

                stat_props.insert(
                    "isDirectory".to_string(),
                    JsValue::NativeFunction(is_dir_fn),
                );
                stat_props.insert("isFile".to_string(), JsValue::NativeFunction(is_file_fn));
                stat_props.insert("size".to_string(), JsValue::Number(metadata.len() as f64));

                let stat_ptr = vm.heap.len();
                vm.heap.push(HeapObject {
                    data: HeapData::Object(stat_props),
                });
                JsValue::Object(stat_ptr)
            }
            Err(_) => JsValue::Null,
        }
    } else {
        JsValue::Null
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
        && let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
    {
        let value_u8 = *value as u8;
        bytes.push(value_u8);
        return JsValue::Undefined;
    }
    JsValue::Undefined
}

pub fn native_byte_stream_write_u32(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.first(), args.get(1))
        && let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
    {
        let value_u32 = *value as u32;
        bytes.extend_from_slice(&value_u32.to_le_bytes());
        return JsValue::Undefined;
    }
    JsValue::Undefined
}

pub fn native_byte_stream_write_varint(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.first(), args.get(1))
        && let Some(HeapObject {
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
    JsValue::Undefined
}

pub fn native_byte_stream_write_f64(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(value))) = (args.first(), args.get(1))
        && let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
    {
        let float_bytes = value.to_le_bytes();
        bytes.extend_from_slice(&float_bytes);
        return JsValue::Undefined;
    }
    JsValue::Undefined
}

pub fn native_byte_stream_write_string(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::String(s))) = (args.first(), args.get(1))
        && let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get_mut(*ptr)
    {
        // Write length as varint first (matching decoder's read_string)
        let len = s.len() as u64;
        let mut value = len;
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
        // Then write string bytes
        bytes.extend_from_slice(s.as_bytes());
        return JsValue::Undefined;
    }
    JsValue::Undefined
}

pub fn native_byte_stream_length(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Object(ptr)) = args.first()
        && let Some(HeapObject {
            data: HeapData::ByteStream(bytes),
        }) = vm.heap.get(*ptr)
    {
        return JsValue::Number(bytes.len() as f64);
    }
    JsValue::Number(0.0)
}

pub fn native_byte_stream_patch_u32(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::Object(ptr)), Some(JsValue::Number(offset)), Some(JsValue::Number(value))) =
        (args.first(), args.get(1), args.get(2))
        && let Some(HeapObject {
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
    JsValue::Undefined
}

pub fn native_byte_stream_to_array(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Object(ptr)) = args.first()
        && let Some(HeapObject {
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
    JsValue::Undefined
}

// ============================================================================
// String Utilities (minimal - needed for bootstrap compiler)
// ============================================================================

/// String constructor - converts any value to a string
pub fn native_string_constructor(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if args.is_empty() {
        return JsValue::String(String::new());
    }
    let value = &args[0];
    let result = match value {
        JsValue::String(s) => s.clone(),
        JsValue::Number(n) => n.to_string(),
        JsValue::Boolean(b) => b.to_string(),
        JsValue::Null => "null".to_string(),
        JsValue::Undefined => "undefined".to_string(),
        JsValue::Object(ptr) => {
            if let Some(HeapObject { data }) = vm.heap.get(*ptr) {
                match data {
                    HeapData::Array(arr) => {
                        let parts: Vec<String> = arr
                            .iter()
                            .map(|v| match v {
                                JsValue::String(s) => s.clone(),
                                JsValue::Number(n) => n.to_string(),
                                JsValue::Boolean(b) => b.to_string(),
                                JsValue::Null => "null".to_string(),
                                JsValue::Undefined => "".to_string(),
                                _ => String::new(),
                            })
                            .collect();
                        parts.join(",")
                    }
                    HeapData::Object(_) => "[object Object]".to_string(),
                    HeapData::ByteStream(_) => "[object ByteStream]".to_string(),
                    HeapData::Map(_) => "[object Map]".to_string(),
                    HeapData::Set(_) => "[object Set]".to_string(),
                }
            } else {
                "[object Object]".to_string()
            }
        }
        JsValue::Function { .. } => "function".to_string(),
        JsValue::NativeFunction(_) => "function".to_string(),
        JsValue::Promise(_) => "[object Promise]".to_string(),
        JsValue::Accessor(_, _) => "".to_string(),
    };
    JsValue::String(result)
}

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

// ============================================================================
// JSON Functions (minimal - needed for compiler AST output)
// ============================================================================

fn json_stringify_value(vm: &VM, value: &JsValue, indent: usize, pretty: bool) -> String {
    let indent_str = if pretty {
        "  ".repeat(indent)
    } else {
        String::new()
    };
    let next_indent = if pretty {
        "  ".repeat(indent + 1)
    } else {
        String::new()
    };
    let newline = if pretty { "\n" } else { "" };
    let space = if pretty { " " } else { "" };

    match value {
        JsValue::Null => "null".to_string(),
        JsValue::Undefined => "null".to_string(), // undefined becomes null in JSON
        JsValue::Boolean(b) => b.to_string(),
        JsValue::Number(n) => {
            if n.is_nan() {
                "null".to_string()
            } else if n.is_infinite() {
                "null".to_string()
            } else {
                n.to_string()
            }
        }
        JsValue::String(s) => {
            // Escape special characters
            let escaped: String = s
                .chars()
                .map(|c| match c {
                    '"' => "\\\"".to_string(),
                    '\\' => "\\\\".to_string(),
                    '\n' => "\\n".to_string(),
                    '\r' => "\\r".to_string(),
                    '\t' => "\\t".to_string(),
                    c if c.is_control() => format!("\\u{:04x}", c as u32),
                    c => c.to_string(),
                })
                .collect();
            format!("\"{}\"", escaped)
        }
        JsValue::Object(ptr) => {
            if let Some(HeapObject { data }) = vm.heap.get(*ptr) {
                match data {
                    HeapData::Array(arr) => {
                        if arr.is_empty() {
                            "[]".to_string()
                        } else {
                            let items: Vec<String> = arr
                                .iter()
                                .map(|v| {
                                    format!(
                                        "{}{}",
                                        next_indent,
                                        json_stringify_value(vm, v, indent + 1, pretty)
                                    )
                                })
                                .collect();
                            format!(
                                "[{}{}{}{}]",
                                newline,
                                items.join(&format!(",{}", newline)),
                                newline,
                                indent_str
                            )
                        }
                    }
                    HeapData::Object(props) => {
                        if props.is_empty() {
                            "{}".to_string()
                        } else {
                            let mut items: Vec<String> = props
                                .iter()
                                .map(|(k, v)| {
                                    format!(
                                        "{}\"{}\":{}{}",
                                        next_indent,
                                        k,
                                        space,
                                        json_stringify_value(vm, v, indent + 1, pretty)
                                    )
                                })
                                .collect();
                            items.sort(); // Sort for consistent output
                            format!(
                                "{{{}{}{}{}}}",
                                newline,
                                items.join(&format!(",{}", newline)),
                                newline,
                                indent_str
                            )
                        }
                    }
                    _ => "null".to_string(),
                }
            } else {
                "null".to_string()
            }
        }
        JsValue::Function { .. } => "null".to_string(), // Functions become null in JSON
        JsValue::NativeFunction(_) => "null".to_string(),
        JsValue::Accessor(_, _) => "null".to_string(),
        JsValue::Promise(_) => "null".to_string(),
    }
}

pub fn native_json_stringify(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(value) = args.first() {
        // Check for indent parameter (second arg is replacer, third is indent)
        let indent = args.get(2).and_then(|v| match v {
            JsValue::Number(n) if *n > 0.0 => Some(*n as usize),
            _ => None,
        });
        let pretty = indent.is_some();
        JsValue::String(json_stringify_value(vm, value, 0, pretty))
    } else {
        JsValue::Undefined
    }
}

pub fn native_json_parse(_vm: &mut VM, _args: Vec<JsValue>) -> JsValue {
    // JSON.parse is complex to implement; return undefined for now
    // The modular compiler doesn't need parse, just stringify
    JsValue::Undefined
}

// ============================================================================
// Process Functions (for @rolls/process and unroll)
// ============================================================================

/// Get environment variable
pub fn native_getenv(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(name)) = args.first() {
        match std::env::var(name) {
            Ok(value) => JsValue::String(value),
            Err(_) => JsValue::Undefined,
        }
    } else {
        JsValue::Undefined
    }
}

/// Set environment variable
pub fn native_setenv(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::String(name)), Some(JsValue::String(value))) = (args.first(), args.get(1))
    {
        // SAFETY: Setting environment variables is inherently unsafe in multi-threaded contexts,
        // but we control when this is called and it's a common operation in CLI tools.
        unsafe {
            std::env::set_var(name, value);
        }
        JsValue::Undefined
    } else {
        JsValue::Undefined
    }
}

/// Get current working directory
pub fn native_cwd(_vm: &mut VM, _args: Vec<JsValue>) -> JsValue {
    match std::env::current_dir() {
        Ok(path) => JsValue::String(path.to_string_lossy().to_string()),
        Err(_) => JsValue::Undefined,
    }
}

/// Change current working directory
pub fn native_chdir(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        match std::env::set_current_dir(path) {
            Ok(()) => JsValue::Boolean(true),
            Err(_) => JsValue::Boolean(false),
        }
    } else {
        JsValue::Boolean(false)
    }
}

/// Exit the process with a status code
pub fn native_exit(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let code = if let Some(JsValue::Number(n)) = args.first() {
        *n as i32
    } else {
        0
    };
    std::process::exit(code);
}

/// Execute a command and return the result
/// Usage: process.exec(command, args?)
/// Returns: { exitCode: number, stdout: string, stderr: string }
pub fn native_exec(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    use std::process::Command;

    let command = match args.first() {
        Some(JsValue::String(cmd)) => cmd.clone(),
        _ => return create_exec_error(vm, "exec requires a command string"),
    };

    // Parse arguments array if provided
    let mut cmd_args: Vec<String> = Vec::new();
    if let Some(JsValue::Object(args_ptr)) = args.get(1)
        && let Some(HeapObject {
            data: HeapData::Array(arr),
        }) = vm.heap.get(*args_ptr)
    {
        for item in arr {
            if let JsValue::String(s) = item {
                cmd_args.push(s.clone());
            }
        }
    }

    // Execute the command
    let output = Command::new(&command).args(&cmd_args).output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout).to_string();
            let stderr = String::from_utf8_lossy(&result.stderr).to_string();
            let exit_code = result.status.code().unwrap_or(-1);

            // Create result object
            let mut response = std::collections::HashMap::new();
            response.insert("exitCode".to_string(), JsValue::Number(exit_code as f64));
            response.insert("stdout".to_string(), JsValue::String(stdout));
            response.insert("stderr".to_string(), JsValue::String(stderr));

            let response_ptr = vm.heap.len();
            vm.heap.push(HeapObject {
                data: HeapData::Object(response),
            });
            JsValue::Object(response_ptr)
        }
        Err(e) => create_exec_error(vm, &format!("exec failed: {}", e)),
    }
}

fn create_exec_error(vm: &mut VM, message: &str) -> JsValue {
    let mut response = std::collections::HashMap::new();
    response.insert("exitCode".to_string(), JsValue::Number(-1.0));
    response.insert("stdout".to_string(), JsValue::String("".to_string()));
    response.insert("stderr".to_string(), JsValue::String(message.to_string()));

    let response_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(response),
    });
    JsValue::Object(response_ptr)
}

// ============================================================================
// HTTP Fetch (for @rolls/http and unroll)
// ============================================================================

/// Synchronous HTTP fetch - returns response object
/// Usage: __ffi_fetch(url, options?)
/// options: { method?: string, headers?: object, body?: string }
pub fn native_fetch(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let url = match args.first() {
        Some(JsValue::String(u)) => u.clone(),
        _ => return create_fetch_error(vm, "fetch requires a URL string"),
    };

    // Default options
    let mut method = "GET".to_string();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut body: Option<String> = None;

    // Parse options if provided
    if let Some(JsValue::Object(opts_ptr)) = args.get(1)
        && let Some(HeapObject {
            data: HeapData::Object(opts),
        }) = vm.heap.get(*opts_ptr)
    {
        if let Some(JsValue::String(m)) = opts.get("method") {
            method = m.to_uppercase();
        }
        if let Some(JsValue::String(b)) = opts.get("body") {
            body = Some(b.clone());
        }
        if let Some(JsValue::Object(hdrs_ptr)) = opts.get("headers")
            && let Some(HeapObject {
                data: HeapData::Object(hdrs),
            }) = vm.heap.get(*hdrs_ptr)
        {
            for (k, v) in hdrs {
                if let JsValue::String(val) = v {
                    headers.push((k.clone(), val.clone()));
                }
            }
        }
    }

    // Build request
    let mut request = match method.as_str() {
        "GET" => ureq::get(&url),
        "POST" => ureq::post(&url),
        "PUT" => ureq::put(&url),
        "DELETE" => ureq::delete(&url),
        "PATCH" => ureq::patch(&url),
        "HEAD" => ureq::head(&url),
        _ => return create_fetch_error(vm, &format!("Unsupported HTTP method: {}", method)),
    };

    // Add headers
    for (key, value) in &headers {
        request = request.set(key, value);
    }

    // Send request
    let response = if let Some(body_str) = body {
        request.send_string(&body_str)
    } else {
        request.call()
    };

    match response {
        Ok(resp) => {
            let status = resp.status();
            let status_text = resp.status_text().to_string();

            // Collect response headers
            let mut resp_headers = std::collections::HashMap::new();
            for name in resp.headers_names() {
                if let Some(value) = resp.header(&name) {
                    resp_headers.insert(name.to_lowercase(), JsValue::String(value.to_string()));
                }
            }

            // Read body
            let body_text = resp.into_string().unwrap_or_default();

            // Create response object
            create_fetch_response(vm, status, &status_text, resp_headers, &body_text, false)
        }
        Err(ureq::Error::Status(code, resp)) => {
            let status_text = resp.status_text().to_string();
            let mut resp_headers = std::collections::HashMap::new();
            for name in resp.headers_names() {
                if let Some(value) = resp.header(&name) {
                    resp_headers.insert(name.to_lowercase(), JsValue::String(value.to_string()));
                }
            }
            let body_text = resp.into_string().unwrap_or_default();
            create_fetch_response(vm, code, &status_text, resp_headers, &body_text, false)
        }
        Err(e) => create_fetch_error(vm, &format!("Fetch error: {}", e)),
    }
}

fn create_fetch_response(
    vm: &mut VM,
    status: u16,
    status_text: &str,
    headers: std::collections::HashMap<String, JsValue>,
    body: &str,
    _redirected: bool,
) -> JsValue {
    // Create headers object
    let headers_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(headers),
    });

    // Create response object
    let mut response = std::collections::HashMap::new();
    response.insert("status".to_string(), JsValue::Number(status as f64));
    response.insert(
        "statusText".to_string(),
        JsValue::String(status_text.to_string()),
    );
    response.insert(
        "ok".to_string(),
        JsValue::Boolean((200..300).contains(&status)),
    );
    response.insert("headers".to_string(), JsValue::Object(headers_ptr));
    response.insert("body".to_string(), JsValue::String(body.to_string()));
    response.insert("error".to_string(), JsValue::Undefined);

    let response_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(response),
    });
    JsValue::Object(response_ptr)
}

fn create_fetch_error(vm: &mut VM, message: &str) -> JsValue {
    let mut response = std::collections::HashMap::new();
    response.insert("status".to_string(), JsValue::Number(0.0));
    response.insert("statusText".to_string(), JsValue::String("".to_string()));
    response.insert("ok".to_string(), JsValue::Boolean(false));
    response.insert("body".to_string(), JsValue::String("".to_string()));
    response.insert("error".to_string(), JsValue::String(message.to_string()));

    let response_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Object(response),
    });
    JsValue::Object(response_ptr)
}

// ============================================================================
// Stdin/Stdout Functions (for LSP server and interactive tools)
// ============================================================================

/// Read a line from stdin, stripping trailing \r\n
/// Returns null on EOF
pub fn native_stdin_read_line(_vm: &mut VM, _args: Vec<JsValue>) -> JsValue {
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => JsValue::Null, // EOF
        Ok(_) => {
            let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
            JsValue::String(trimmed.to_string())
        }
        Err(_) => JsValue::Null,
    }
}

/// Read exactly n bytes from stdin, returns as string
/// Returns null on EOF or error
pub fn native_stdin_read_bytes(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let n = match args.first() {
        Some(JsValue::Number(n)) => *n as usize,
        _ => return JsValue::Null,
    };
    let mut buf = vec![0u8; n];
    use std::io::Read;
    match std::io::stdin().read_exact(&mut buf) {
        Ok(()) => JsValue::String(String::from_utf8_lossy(&buf).to_string()),
        Err(_) => JsValue::Null,
    }
}

/// Write string to stdout WITHOUT trailing newline
pub fn native_stdout_write(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    use std::io::Write;
    if let Some(val) = args.first() {
        let s = match val {
            JsValue::String(s) => s.clone(),
            JsValue::Number(n) => n.to_string(),
            JsValue::Boolean(b) => b.to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Undefined => "undefined".to_string(),
            _ => String::new(),
        };
        let _ = std::io::stdout().write_all(s.as_bytes());
        let _ = std::io::stdout().flush();
    }
    JsValue::Undefined
}

// ============================================================================
// Object Functions
// ============================================================================

/// Object.keys(obj) - Returns an array of the object's own enumerable property names
pub fn native_object_keys(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::Object(ptr)) = args.first()
        && let Some(HeapObject { data }) = vm.heap.get(*ptr)
    {
        let keys: Vec<JsValue> = match data {
            HeapData::Object(props) => props.keys().map(|k| JsValue::String(k.clone())).collect(),
            HeapData::Array(arr) => (0..arr.len())
                .map(|i| JsValue::String(i.to_string()))
                .collect(),
            _ => Vec::new(),
        };
        let arr_ptr = vm.heap.len();
        vm.heap.push(HeapObject {
            data: HeapData::Array(keys),
        });
        return JsValue::Object(arr_ptr);
    }
    // Return empty array for non-objects
    let arr_ptr = vm.heap.len();
    vm.heap.push(HeapObject {
        data: HeapData::Array(Vec::new()),
    });
    JsValue::Object(arr_ptr)
}
