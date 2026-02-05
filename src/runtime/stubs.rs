//! Runtime stubs callable from native-compiled code
//!
//! These extern "C" functions provide the interface between JIT/AOT compiled
//! code and the Rust runtime. They handle:
//! - Object allocation
//! - Property access
//! - Dynamic dispatch (for `any` typed operations)
//! - String operations
//!
//! The calling convention is:
//! - All values passed as u64 (NaN-boxed TsclValue)
//! - Return values are also u64
//! - Pointers to arrays use *const u64

use super::abi::TsclValue;
use super::heap::{
    NativeArray, NativeObject, NativeString, ObjectHeader, ObjectKind, PropertyMap, heap,
};

// =========================================================================
// Allocation Stubs
// =========================================================================

/// Allocate a new empty object.
///
/// Returns a TsclValue containing the object pointer, or undefined on failure.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_alloc_object() -> u64 {
    match heap().alloc_object() {
        Some(ptr) => TsclValue::pointer(ptr).to_bits(),
        None => TsclValue::undefined().to_bits(),
    }
}

/// Allocate a new array with the given capacity.
///
/// Returns a TsclValue containing the array pointer, or undefined on failure.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_alloc_array(capacity: usize) -> u64 {
    match heap().alloc_array(capacity) {
        Some(ptr) => TsclValue::pointer(ptr).to_bits(),
        None => TsclValue::undefined().to_bits(),
    }
}

/// Allocate a new string from UTF-8 bytes.
///
/// Returns a TsclValue containing the string pointer, or undefined on failure.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_alloc_string(data: *const u8, len: usize) -> u64 {
    if data.is_null() {
        return TsclValue::undefined().to_bits();
    }

    let s = unsafe {
        let slice = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return TsclValue::undefined().to_bits(),
        }
    };

    match heap().alloc_string(s) {
        Some(ptr) => TsclValue::pointer(ptr).to_bits(),
        None => TsclValue::undefined().to_bits(),
    }
}

// =========================================================================
// Property Access Stubs
// =========================================================================

/// Get a property from an object.
///
/// # Parameters
/// - `obj`: TsclValue containing an object pointer
/// - `key`: Pointer to UTF-8 key string
/// - `key_len`: Length of key string
///
/// # Returns
/// The property value, or undefined if not found.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_get_prop(obj: u64, key: *const u8, key_len: usize) -> u64 {
    let val = TsclValue::from_bits(obj);

    let ptr = match val.as_pointer() {
        Some(p) => p,
        None => return TsclValue::undefined().to_bits(),
    };

    let key_str = unsafe {
        let slice = std::slice::from_raw_parts(key, key_len);
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return TsclValue::undefined().to_bits(),
        }
    };

    unsafe {
        let header = ptr.as_ref::<ObjectHeader>();

        match header.kind {
            ObjectKind::Object => {
                let obj = ptr.as_ref::<NativeObject>();
                if obj.properties.is_null() {
                    return TsclValue::undefined().to_bits();
                }
                // Vec-based lookup
                match (*obj.properties).iter().find(|(k, _)| k == key_str) {
                    Some((_, bits)) => *bits,
                    None => TsclValue::undefined().to_bits(),
                }
            }
            ObjectKind::Array => {
                let arr = ptr.as_ref::<NativeArray>();
                // Handle "length" property
                if key_str == "length" {
                    return TsclValue::number(arr.len as f64).to_bits();
                }
                // Try to parse as index
                if let Ok(idx) = key_str.parse::<usize>()
                    && idx < arr.len as usize
                {
                    return *arr.elements.add(idx);
                }
                TsclValue::undefined().to_bits()
            }
            ObjectKind::String => {
                let s = ptr.as_ref::<NativeString>();
                // Handle "length" property
                if key_str == "length" {
                    return TsclValue::number(s.len as f64).to_bits();
                }
                // Try to parse as index (for charAt)
                if let Ok(idx) = key_str.parse::<usize>() {
                    let str_data = s.as_str();
                    if let Some(ch) = str_data.chars().nth(idx) {
                        // Allocate a single-char string
                        let mut buf = [0u8; 4];
                        let ch_str = ch.encode_utf8(&mut buf);
                        match heap().alloc_string(ch_str) {
                            Some(ptr) => return TsclValue::pointer(ptr).to_bits(),
                            None => return TsclValue::undefined().to_bits(),
                        }
                    }
                }
                TsclValue::undefined().to_bits()
            }
            _ => TsclValue::undefined().to_bits(),
        }
    }
}

/// Set a property on an object.
///
/// # Parameters
/// - `obj`: TsclValue containing an object pointer
/// - `key`: Pointer to UTF-8 key string
/// - `key_len`: Length of key string
/// - `value`: TsclValue to set
#[unsafe(no_mangle)]
pub extern "C" fn tscl_set_prop(obj: u64, key: *const u8, key_len: usize, value: u64) {
    let val = TsclValue::from_bits(obj);

    let ptr = match val.as_pointer() {
        Some(p) => p,
        None => return,
    };

    let key_str = unsafe {
        let slice = std::slice::from_raw_parts(key, key_len);
        match std::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return,
        }
    };

    unsafe {
        let header = ptr.as_ref::<ObjectHeader>();

        match header.kind {
            ObjectKind::Object => {
                let obj = ptr.as_mut::<NativeObject>();
                if obj.properties.is_null() {
                    obj.properties = Box::into_raw(Box::new(PropertyMap::new()));
                }
                // Vec-based insert: find existing or append
                let props = &mut *obj.properties;
                if let Some(entry) = props.iter_mut().find(|(k, _)| k == key_str) {
                    entry.1 = value;
                } else {
                    props.push((key_str.to_string(), value));
                }
            }
            ObjectKind::Array => {
                let arr = ptr.as_mut::<NativeArray>();
                // Try to parse as index
                if let Ok(idx) = key_str.parse::<usize>()
                    && idx < arr.capacity as usize
                {
                    *arr.elements.add(idx) = value;
                    if idx >= arr.len as usize {
                        arr.len = (idx + 1) as u32;
                    }
                }
            }
            _ => {}
        }
    }
}

// =========================================================================
// Array Access Stubs
// =========================================================================

/// Get an element from an array by index.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_get_element(arr: u64, index: usize) -> u64 {
    let val = TsclValue::from_bits(arr);

    let ptr = match val.as_pointer() {
        Some(p) => p,
        None => return TsclValue::undefined().to_bits(),
    };

    unsafe {
        let header = ptr.as_ref::<ObjectHeader>();
        if header.kind != ObjectKind::Array {
            return TsclValue::undefined().to_bits();
        }

        let arr = ptr.as_ref::<NativeArray>();
        if index < arr.len as usize {
            *arr.elements.add(index)
        } else {
            TsclValue::undefined().to_bits()
        }
    }
}

/// Set an element in an array by index.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_set_element(arr: u64, index: usize, value: u64) {
    let val = TsclValue::from_bits(arr);

    let ptr = match val.as_pointer() {
        Some(p) => p,
        None => return,
    };

    unsafe {
        let header = ptr.as_ref::<ObjectHeader>();
        if header.kind != ObjectKind::Array {
            return;
        }

        let arr = ptr.as_mut::<NativeArray>();
        if index < arr.capacity as usize {
            *arr.elements.add(index) = value;
            if index >= arr.len as usize {
                arr.len = (index + 1) as u32;
            }
        }
    }
}

// =========================================================================
// Dynamic Dispatch Stubs (for 'any' typed operations)
// =========================================================================

/// Dynamic addition (handles number + number, string + string, etc.)
#[unsafe(no_mangle)]
pub extern "C" fn tscl_add_any(a: u64, b: u64) -> u64 {
    let va = TsclValue::from_bits(a);
    let vb = TsclValue::from_bits(b);

    // Number + Number
    if va.is_number() && vb.is_number() {
        return TsclValue::number(va.as_number_unchecked() + vb.as_number_unchecked()).to_bits();
    }

    // String + anything (concatenation)
    if let Some(ptr_a) = va.as_pointer() {
        unsafe {
            let header_a = ptr_a.as_ref::<ObjectHeader>();
            if header_a.kind == ObjectKind::String {
                let str_a = ptr_a.as_ref::<NativeString>().as_str();

                // Convert b to string
                let str_b = value_to_string(vb);

                // Concatenate
                let result = format!("{}{}", str_a, str_b);
                return match heap().alloc_string(&result) {
                    Some(ptr) => TsclValue::pointer(ptr).to_bits(),
                    None => TsclValue::undefined().to_bits(),
                };
            }
        }
    }

    // Fallback to NaN
    TsclValue::number(f64::NAN).to_bits()
}

/// Dynamic subtraction.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_sub_any(a: u64, b: u64) -> u64 {
    let va = TsclValue::from_bits(a);
    let vb = TsclValue::from_bits(b);

    if va.is_number() && vb.is_number() {
        TsclValue::number(va.as_number_unchecked() - vb.as_number_unchecked()).to_bits()
    } else {
        TsclValue::number(f64::NAN).to_bits()
    }
}

/// Dynamic multiplication.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_mul_any(a: u64, b: u64) -> u64 {
    let va = TsclValue::from_bits(a);
    let vb = TsclValue::from_bits(b);

    if va.is_number() && vb.is_number() {
        TsclValue::number(va.as_number_unchecked() * vb.as_number_unchecked()).to_bits()
    } else {
        TsclValue::number(f64::NAN).to_bits()
    }
}

/// Dynamic division.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_div_any(a: u64, b: u64) -> u64 {
    let va = TsclValue::from_bits(a);
    let vb = TsclValue::from_bits(b);

    if va.is_number() && vb.is_number() {
        TsclValue::number(va.as_number_unchecked() / vb.as_number_unchecked()).to_bits()
    } else {
        TsclValue::number(f64::NAN).to_bits()
    }
}

/// Dynamic modulo.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_mod_any(a: u64, b: u64) -> u64 {
    let va = TsclValue::from_bits(a);
    let vb = TsclValue::from_bits(b);

    if va.is_number() && vb.is_number() {
        TsclValue::number(va.as_number_unchecked() % vb.as_number_unchecked()).to_bits()
    } else {
        TsclValue::number(f64::NAN).to_bits()
    }
}

/// Exponentiation (a ** b).
#[unsafe(no_mangle)]
pub extern "C" fn tscl_pow(a: u64, b: u64) -> u64 {
    let va = TsclValue::from_bits(a);
    let vb = TsclValue::from_bits(b);

    if va.is_number() && vb.is_number() {
        TsclValue::number(va.as_number_unchecked().powf(vb.as_number_unchecked())).to_bits()
    } else {
        TsclValue::number(f64::NAN).to_bits()
    }
}

/// Get the bytes of an f64 value.
///
/// Returns 8 bytes in little-endian order via output array.
/// # Parameters
/// - `value`: The f64 value as bits
/// - `output`: Pointer to array of 8 bytes to write to
#[unsafe(no_mangle)]
pub extern "C" fn tscl_f64_to_bytes(value: u64, output: *mut u8) {
    // f64 bytes in little-endian order
    unsafe {
        *output = (value & 0xFF) as u8;
        *output.add(1) = ((value >> 8) & 0xFF) as u8;
        *output.add(2) = ((value >> 16) & 0xFF) as u8;
        *output.add(3) = ((value >> 24) & 0xFF) as u8;
        *output.add(4) = ((value >> 32) & 0xFF) as u8;
        *output.add(5) = ((value >> 40) & 0xFF) as u8;
        *output.add(6) = ((value >> 48) & 0xFF) as u8;
        *output.add(7) = ((value >> 56) & 0xFF) as u8;
    }
}

/// Dynamic strict equality (===).
#[unsafe(no_mangle)]
pub extern "C" fn tscl_eq_strict(a: u64, b: u64) -> u64 {
    TsclValue::from_bits(a)
        .strict_eq(TsclValue::from_bits(b))
        .to_bits()
}

/// Dynamic less-than comparison.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_lt(a: u64, b: u64) -> u64 {
    TsclValue::from_bits(a)
        .lt(TsclValue::from_bits(b))
        .to_bits()
}

/// Dynamic greater-than comparison.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_gt(a: u64, b: u64) -> u64 {
    TsclValue::from_bits(a)
        .gt(TsclValue::from_bits(b))
        .to_bits()
}

/// Dynamic less-than-or-equal comparison.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_lte(a: u64, b: u64) -> u64 {
    TsclValue::from_bits(a)
        .lte(TsclValue::from_bits(b))
        .to_bits()
}

/// Dynamic greater-than-or-equal comparison.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_gte(a: u64, b: u64) -> u64 {
    TsclValue::from_bits(a)
        .gte(TsclValue::from_bits(b))
        .to_bits()
}

/// Logical NOT.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_not(a: u64) -> u64 {
    TsclValue::boolean(TsclValue::from_bits(a).is_falsy()).to_bits()
}

/// new.target: returns the constructor that was called with new
/// This is a bit tricky because we don't have access to the call frame from here
/// For now, return undefined
#[unsafe(no_mangle)]
pub extern "C" fn tscl_new_target() -> u64 {
    TsclValue::undefined().to_bits()
}

/// InstanceOf operator: checks if obj's prototype chain contains constructor.prototype
#[unsafe(no_mangle)]
pub extern "C" fn tscl_instanceof(obj: u64, constructor: u64) -> u64 {
    let obj_val = TsclValue::from_bits(obj);
    let ctor_val = TsclValue::from_bits(constructor);

    // Get constructor.prototype
    let target_proto_ptr = if let Some(pointer) = ctor_val.as_pointer() {
        unsafe {
            let header = pointer.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::Object {
                let props = pointer.as_ref::<PropertyMap>();
                // Find "prototype" in the property list
                for (name, value) in props.iter() {
                    if name == "prototype" {
                        // Check if value is an object pointer
                        let prop_val = TsclValue::from_bits(*value);
                        if let Some(ptr) = prop_val.as_pointer() {
                            return ptr.as_usize() as u64;
                        }
                        break;
                    }
                }
            }
        }
        0
    } else {
        0
    };

    if target_proto_ptr == 0 {
        return TsclValue::boolean(false).to_bits();
    }

    // Walk the object's prototype chain
    let mut current_ptr = obj_val.as_pointer();

    const MAX_PROTO_DEPTH: usize = 100;
    let mut depth = 0;

    while let Some(ptr) = current_ptr {
        if depth > MAX_PROTO_DEPTH {
            break;
        }
        depth += 1;

        if ptr.as_usize() as u64 == target_proto_ptr {
            return TsclValue::boolean(true).to_bits();
        }

        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::Object {
                let props = ptr.as_ref::<PropertyMap>();
                // Find "__proto__" in the property list
                for (name, value) in props.iter() {
                    if name == "__proto__" {
                        let prop_val = TsclValue::from_bits(*value);
                        if let Some(proto_ptr) = prop_val.as_pointer() {
                            current_ptr = Some(proto_ptr);
                        }
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    TsclValue::boolean(false).to_bits()
}

/// Unary negation.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_neg(a: u64) -> u64 {
    let va = TsclValue::from_bits(a);
    if va.is_number() {
        TsclValue::number(-va.as_number_unchecked()).to_bits()
    } else {
        TsclValue::number(f64::NAN).to_bits()
    }
}

// =========================================================================
// Type Conversion Stubs
// =========================================================================

/// Convert a TsclValue to a boolean.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_to_boolean(a: u64) -> u64 {
    TsclValue::boolean(!TsclValue::from_bits(a).is_falsy()).to_bits()
}

/// Convert a TsclValue to a number.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_to_number(a: u64) -> u64 {
    let va = TsclValue::from_bits(a);

    if va.is_number() {
        return a;
    }

    if va.is_boolean() {
        return TsclValue::number(if va.as_boolean_unchecked() { 1.0 } else { 0.0 }).to_bits();
    }

    if va.is_null() {
        return TsclValue::number(0.0).to_bits();
    }

    if va.is_undefined() {
        return TsclValue::number(f64::NAN).to_bits();
    }

    // String to number - attempt parse
    if let Some(ptr) = va.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let s = ptr.as_ref::<NativeString>().as_str();
                if let Ok(n) = s.trim().parse::<f64>() {
                    return TsclValue::number(n).to_bits();
                }
            }
        }
    }

    TsclValue::number(f64::NAN).to_bits()
}

// =========================================================================
// Function Call Stubs
// =========================================================================

/// Call a function with arguments.
///
/// # Parameters
/// - `func`: TsclValue containing a function pointer
/// - `argc`: Number of arguments
/// - `argv`: Pointer to array of TsclValue arguments
///
/// # Returns
/// The return value of the function, or undefined on error.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_call(_func: u64, _argc: usize, _argv: *const u64) -> u64 {
    // This is a placeholder - actual implementation requires:
    // 1. Looking up the function by address
    // 2. Setting up a new call frame
    // 3. Executing the function code
    //
    // For JIT-compiled functions, this will be a direct call.
    // For interpreted functions, this falls back to the VM.

    TsclValue::undefined().to_bits()
}

// =========================================================================
// Console/IO Stubs
// =========================================================================

/// Create a closure object that pairs a function address with an environment.
///
/// # Parameters
/// - `func_addr`: The function's bytecode address (as a number)
/// - `env`: Environment object containing captured variables
///
/// # Returns
/// A closure object (pointer to heap-allocated closure data).
#[unsafe(no_mangle)]
pub extern "C" fn tscl_make_closure(func_addr: u64, env: u64) -> u64 {
    // For now, just return the function address as a simple closure
    // A full implementation would:
    // 1. Allocate a closure object on the heap
    // 2. Store func_addr and env in the closure
    // 3. Return a pointer to the closure

    // Simplified: pack func_addr in the low bits, treat as pointer
    // This works because we're using NaN-boxing and func_addr fits
    let _ = env; // Environment not used in simplified version
    TsclValue::number(func_addr as f64).to_bits()
}

/// Print a value to the console.
#[unsafe(no_mangle)]
pub extern "C" fn tscl_console_log(value: u64) {
    let va = TsclValue::from_bits(value);
    let s = value_to_string(va);
    println!("{}", s);
}

// =========================================================================
// Helper Functions
// =========================================================================

/// Convert a TsclValue to a string representation.
fn value_to_string(val: TsclValue) -> String {
    if val.is_number() {
        let n = val.as_number_unchecked();
        if n.is_nan() {
            return "NaN".to_string();
        }
        if n.is_infinite() {
            return if n.is_sign_positive() {
                "Infinity"
            } else {
                "-Infinity"
            }
            .to_string();
        }
        return format!("{}", n);
    }

    if val.is_boolean() {
        return if val.as_boolean_unchecked() {
            "true"
        } else {
            "false"
        }
        .to_string();
    }

    if val.is_null() {
        return "null".to_string();
    }

    if val.is_undefined() {
        return "undefined".to_string();
    }

    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            match header.kind {
                ObjectKind::String => {
                    return ptr.as_ref::<NativeString>().as_str().to_string();
                }
                ObjectKind::Array => {
                    let arr = ptr.as_ref::<NativeArray>();
                    let mut parts = Vec::new();
                    for i in 0..arr.len as usize {
                        let elem = TsclValue::from_bits(*arr.elements.add(i));
                        parts.push(value_to_string(elem));
                    }
                    return format!("[{}]", parts.join(","));
                }
                ObjectKind::Object => {
                    return "[object Object]".to_string();
                }
                ObjectKind::Function => {
                    return "[function]".to_string();
                }
                ObjectKind::ByteStream => {
                    return "[ByteStream]".to_string();
                }
            }
        }
    }

    "undefined".to_string()
}

#[unsafe(no_mangle)]
pub extern "C" fn tscl_fs_exists(path: u64) -> u64 {
    let val = TsclValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                return TsclValue::boolean(std::path::Path::new(path_str).exists()).to_bits();
            }
        }
    }
    TsclValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn tscl_fs_read_file(path: u64) -> u64 {
    let val = TsclValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(content) = std::fs::read_to_string(path_str)
                    && let Some(ptr) = heap().alloc_string(&content)
                {
                    return TsclValue::pointer(ptr).to_bits();
                }
            }
        }
    }
    TsclValue::undefined().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn tscl_fs_write_file(path: u64, content: u64) -> u64 {
    let path_val = TsclValue::from_bits(path);
    let content_val = TsclValue::from_bits(content);

    let (path_str, content_str) = unsafe {
        let p = path_val.as_pointer().and_then(|ptr| {
            if ptr.as_ref::<ObjectHeader>().kind == ObjectKind::String {
                Some(ptr.as_ref::<NativeString>().as_str().to_string())
            } else {
                None
            }
        });
        let c = content_val.as_pointer().and_then(|ptr| {
            if ptr.as_ref::<ObjectHeader>().kind == ObjectKind::String {
                Some(ptr.as_ref::<NativeString>().as_str().to_string())
            } else {
                None
            }
        });
        (p, c)
    };

    if let (Some(p), Some(c)) = (path_str, content_str) {
        return TsclValue::boolean(std::fs::write(&p, &c).is_ok()).to_bits();
    }
    TsclValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn tscl_fs_readdir(path: u64) -> u64 {
    let val = TsclValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(entries) = std::fs::read_dir(path_str) {
                    let names: Vec<String> = entries
                        .flatten()
                        .filter_map(|e| e.file_name().to_str().map(String::from))
                        .collect();
                    if let Some(arr_ptr) = heap().alloc_array(names.len()) {
                        let arr = arr_ptr.as_mut::<NativeArray>();
                        for (i, name) in names.iter().enumerate() {
                            let str_val = heap()
                                .alloc_string(name)
                                .map(|p| TsclValue::pointer(p).to_bits())
                                .unwrap_or(TsclValue::undefined().to_bits());
                            *arr.elements.add(i) = str_val;
                        }
                        arr.len = names.len() as u32;
                        return TsclValue::pointer(arr_ptr).to_bits();
                    }
                }
                if let Some(arr_ptr) = heap().alloc_array(0) {
                    return TsclValue::pointer(arr_ptr).to_bits();
                }
            }
        }
    }
    TsclValue::undefined().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn tscl_fs_stat(path: u64) -> u64 {
    let val = TsclValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(metadata) = std::fs::metadata(path_str)
                    && let Some(obj_ptr) = heap().alloc_object()
                {
                    let obj = obj_ptr.as_mut::<NativeObject>();
                    if obj.properties.is_null() {
                        obj.properties = Box::into_raw(Box::new(PropertyMap::new()));
                    }
                    let props = &mut *obj.properties;
                    props.push((
                        "__isDir".to_string(),
                        TsclValue::boolean(metadata.is_dir()).to_bits(),
                    ));
                    props.push((
                        "size".to_string(),
                        TsclValue::number(metadata.len() as f64).to_bits(),
                    ));
                    return TsclValue::pointer(obj_ptr).to_bits();
                }
            }
        }
    }
    TsclValue::null().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn tscl_stat_is_directory(stat: u64) -> u64 {
    let val = TsclValue::from_bits(stat);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::Object {
                let obj = ptr.as_ref::<NativeObject>();
                if !obj.properties.is_null() {
                    for (key, value) in (*obj.properties).iter() {
                        if key == "__isDir" {
                            return *value;
                        }
                    }
                }
            }
        }
    }
    TsclValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn tscl_fs_mkdir(path: u64, recursive: u64) -> u64 {
    let val = TsclValue::from_bits(path);
    let is_recursive = !TsclValue::from_bits(recursive).is_falsy();

    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                let result = if is_recursive {
                    std::fs::create_dir_all(path_str)
                } else {
                    std::fs::create_dir(path_str)
                };
                return TsclValue::boolean(result.is_ok()).to_bits();
            }
        }
    }
    TsclValue::boolean(false).to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_log() {
        tscl_console_log(TsclValue::number(42.0).to_bits());
        tscl_console_log(TsclValue::boolean(true).to_bits());
        tscl_console_log(TsclValue::null().to_bits());
    }

    #[test]
    fn test_arithmetic_stubs() {
        let a = TsclValue::number(10.0).to_bits();
        let b = TsclValue::number(3.0).to_bits();

        assert_eq!(
            TsclValue::from_bits(tscl_add_any(a, b)).as_number(),
            Some(13.0)
        );
        assert_eq!(
            TsclValue::from_bits(tscl_sub_any(a, b)).as_number(),
            Some(7.0)
        );
        assert_eq!(
            TsclValue::from_bits(tscl_mul_any(a, b)).as_number(),
            Some(30.0)
        );

        let div = TsclValue::from_bits(tscl_div_any(a, b))
            .as_number()
            .unwrap();
        assert!((div - 3.333333).abs() < 0.001);
    }

    #[test]
    fn test_object_property() {
        let obj_bits = tscl_alloc_object();
        assert!(!TsclValue::from_bits(obj_bits).is_undefined());

        let key = "foo";
        let value = TsclValue::number(42.0).to_bits();

        tscl_set_prop(obj_bits, key.as_ptr(), key.len(), value);
        let retrieved = tscl_get_prop(obj_bits, key.as_ptr(), key.len());

        assert_eq!(TsclValue::from_bits(retrieved).as_number(), Some(42.0));
    }
}
