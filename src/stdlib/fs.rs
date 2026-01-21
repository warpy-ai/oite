use crate::vm::value::{HeapData, HeapObject, JsValue, Promise};
use crate::vm::VM;

// ============================================================================
// File System Synchronous Methods
// ============================================================================

pub fn native_fs_exists_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        JsValue::Boolean(std::path::Path::new(path).exists())
    } else {
        JsValue::Boolean(false)
    }
}

pub fn native_fs_mkdir_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        match std::fs::create_dir_all(path) {
            Ok(()) => JsValue::Boolean(true),
            Err(e) => {
                eprintln!("Error creating directory '{}': {}", path, e);
                JsValue::Boolean(false)
            }
        }
    } else {
        JsValue::Boolean(false)
    }
}

pub fn native_fs_readdir_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut names: Vec<JsValue> = Vec::new();
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        names.push(JsValue::String(name.to_string()));
                    }
                }
                let arr_ptr = vm.heap.len();
                vm.heap.push(HeapObject {
                    data: HeapData::Array(names),
                });
                JsValue::Object(arr_ptr)
            }
            Err(e) => {
                eprintln!("Error reading directory '{}': {}", path, e);
                JsValue::Undefined
            }
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_fs_unlink(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        match std::fs::remove_file(path) {
            Ok(()) => JsValue::Boolean(true),
            Err(e) => {
                eprintln!("Error deleting file '{}': {}", path, e);
                JsValue::Boolean(false)
            }
        }
    } else {
        JsValue::Boolean(false)
    }
}

pub fn native_fs_rmdir(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        match std::fs::remove_dir(path) {
            Ok(()) => JsValue::Boolean(true),
            Err(e) => {
                eprintln!("Error deleting directory '{}': {}", path, e);
                JsValue::Boolean(false)
            }
        }
    } else {
        JsValue::Boolean(false)
    }
}

pub fn native_fs_stat_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(path)) = args.first() {
        match std::fs::metadata(path) {
            Ok(metadata) => {
                let stats_ptr = vm.heap.len();
                let mut stats = std::collections::HashMap::new();
                stats.insert("isFile".to_string(), JsValue::Boolean(metadata.is_file()));
                stats.insert(
                    "isDirectory".to_string(),
                    JsValue::Boolean(metadata.is_dir()),
                );
                stats.insert("size".to_string(), JsValue::Number(metadata.len() as f64));
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        stats.insert("mtime".to_string(), JsValue::Number(duration.as_secs_f64()));
                    }
                }
                vm.heap.push(HeapObject {
                    data: HeapData::Object(stats),
                });
                JsValue::Object(stats_ptr)
            }
            Err(e) => {
                eprintln!("Error stat '{}': {}", path, e);
                JsValue::Undefined
            }
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_fs_append_file_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let (path, content) = if let (Some(JsValue::String(path)), Some(JsValue::String(content))) =
        (args.first(), args.get(1))
    {
        (path.clone(), content.clone())
    } else if let (Some(JsValue::String(path)), Some(JsValue::Number(n))) =
        (args.first(), args.get(1))
    {
        (path.clone(), n.to_string())
    } else {
        return JsValue::Undefined;
    };

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut file) => match std::io::Write::write_all(&mut file, content.as_bytes()) {
            Ok(()) => JsValue::Boolean(true),
            Err(e) => {
                eprintln!("Error appending to file '{}': {}", path, e);
                JsValue::Boolean(false)
            }
        },
        Err(e) => {
            eprintln!("Error opening file '{}': {}", path, e);
            JsValue::Boolean(false)
        }
    }
}

pub fn native_fs_copy_file_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::String(src)), Some(JsValue::String(dst))) = (args.first(), args.get(1)) {
        match std::fs::copy(src, dst) {
            Ok(_) => JsValue::Boolean(true),
            Err(e) => {
                eprintln!("Error copying file '{}' -> '{}': {}", src, dst, e);
                JsValue::Boolean(false)
            }
        }
    } else {
        JsValue::Boolean(false)
    }
}

pub fn native_fs_rename(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let (Some(JsValue::String(old_path)), Some(JsValue::String(new_path))) =
        (args.first(), args.get(1))
    {
        match std::fs::rename(old_path, new_path) {
            Ok(()) => JsValue::Boolean(true),
            Err(e) => {
                eprintln!("Error renaming '{}' -> '{}': {}", old_path, new_path, e);
                JsValue::Boolean(false)
            }
        }
    } else {
        JsValue::Boolean(false)
    }
}

pub fn native_fs_read_file_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(filename)) = args.first() {
        match std::fs::read(filename) {
            Ok(bytes) => {
                let ptr = vm.heap.len();
                vm.heap.push(HeapObject {
                    data: HeapData::ByteStream(bytes),
                });
                JsValue::Object(ptr)
            }
            Err(e) => {
                eprintln!("Error reading file '{}': {}", filename, e);
                JsValue::Undefined
            }
        }
    } else {
        JsValue::Undefined
    }
}

pub fn native_fs_write_file_sync(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let (filename, content) =
        if let (Some(JsValue::String(filename)), Some(content)) = (args.first(), args.get(1)) {
            let bytes = match content {
                JsValue::String(s) => s.as_bytes().to_vec(),
                JsValue::Object(ptr) => {
                    if let Some(HeapObject {
                        data: HeapData::ByteStream(bytes),
                    }) = vm.heap.get(*ptr)
                    {
                        bytes.clone()
                    } else {
                        return JsValue::Undefined;
                    }
                }
                _ => return JsValue::Undefined,
            };
            (filename.clone(), bytes)
        } else {
            return JsValue::Undefined;
        };

    match std::fs::write(&filename, content) {
        Ok(()) => JsValue::Boolean(true),
        Err(e) => {
            eprintln!("Error writing file '{}': {}", filename, e);
            JsValue::Boolean(false)
        }
    }
}

// ============================================================================
// Async File System Methods (Return Promises)
// ============================================================================

pub fn native_fs_exists_async(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let path = if let Some(JsValue::String(path)) = args.first() {
        path.clone()
    } else {
        return JsValue::Undefined;
    };

    let exists = std::path::Path::new(&path).exists();
    JsValue::Promise(Promise::with_value(JsValue::Boolean(exists)))
}

pub fn native_fs_read_file_async(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let path = if let Some(JsValue::String(path)) = args.first() {
        path.clone()
    } else {
        return JsValue::Undefined;
    };

    let promise = Promise::new();
    let promise_clone = promise.clone();

    std::thread::spawn(move || match std::fs::read(&path) {
        Ok(bytes) => {
            promise_clone.set_value(
                JsValue::String(String::from_utf8_lossy(&bytes).into_owned()),
                true,
            );
        }
        Err(e) => {
            promise_clone.set_value(JsValue::String(format!("Error reading file: {}", e)), false);
        }
    });

    JsValue::Promise(promise)
}

pub fn native_fs_write_file_async(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let (path, content) =
        if let (Some(JsValue::String(path)), Some(content)) = (args.first(), args.get(1)) {
            let bytes = match content {
                JsValue::String(s) => s.as_bytes().to_vec(),
                JsValue::Object(ptr) => {
                    if let Some(HeapObject {
                        data: HeapData::ByteStream(bytes),
                    }) = vm.heap.get(*ptr)
                    {
                        bytes.clone()
                    } else {
                        return JsValue::Undefined;
                    }
                }
                _ => return JsValue::Undefined,
            };
            (path.clone(), bytes)
        } else {
            return JsValue::Undefined;
        };

    let promise = Promise::new();
    let promise_clone = promise.clone();
    let path_for_thread = path.clone();
    let content_for_thread = content;

    std::thread::spawn(
        move || match std::fs::write(&path_for_thread, content_for_thread) {
            Ok(()) => {
                promise_clone.set_value(JsValue::Boolean(true), true);
            }
            Err(e) => {
                promise_clone
                    .set_value(JsValue::String(format!("Error writing file: {}", e)), false);
            }
        },
    );

    JsValue::Promise(promise)
}

pub fn native_fs_read_dir_async(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let path = if let Some(JsValue::String(path)) = args.first() {
        path.clone()
    } else {
        return JsValue::Undefined;
    };

    let promise = Promise::new();
    let promise_clone = promise.clone();

    std::thread::spawn(move || match std::fs::read_dir(&path) {
        Ok(entries) => {
            let mut names: Vec<JsValue> = Vec::new();
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    names.push(JsValue::String(name.to_string()));
                }
            }
            let array: Vec<JsValue> = names;
            promise_clone.set_value(JsValue::String(format!("[{}]", array.len())), true);
        }
        Err(e) => {
            promise_clone.set_value(
                JsValue::String(format!("Error reading directory: {}", e)),
                false,
            );
        }
    });

    JsValue::Promise(promise)
}
