//! File system functions (used by the self-hosted compiler).

use super::*;

// =========================================================================
// File system (used by the self-hosted compiler)
// =========================================================================

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_exists(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                return OtValue::boolean(std::path::Path::new(path_str).exists()).to_bits();
            }
        }
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_read_file(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(content) = std::fs::read_to_string(path_str)
                    && let Some(ptr) = heap().alloc_string(&content)
                {
                    return OtValue::pointer(ptr).to_bits();
                }
            }
        }
    }
    OtValue::undefined().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_write_file(path: u64, content: u64) -> u64 {
    let path_val = OtValue::from_bits(path);
    let content_val = OtValue::from_bits(content);

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
        return OtValue::boolean(std::fs::write(&p, &c).is_ok()).to_bits();
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_readdir(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
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
                                .map(|p| OtValue::pointer(p).to_bits())
                                .unwrap_or(OtValue::undefined().to_bits());
                            *arr.elements.add(i) = str_val;
                        }
                        arr.len = names.len() as u32;
                        return OtValue::pointer(arr_ptr).to_bits();
                    }
                }
                if let Some(arr_ptr) = heap().alloc_array(0) {
                    return OtValue::pointer(arr_ptr).to_bits();
                }
            }
        }
    }
    OtValue::undefined().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_stat(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(metadata) = std::fs::metadata(path_str)
                    && let Some(obj_ptr) = heap().alloc_object()
                {
                    let stat = OtValue::pointer(obj_ptr);
                    set_property(stat, "__isDir", boolean(metadata.is_dir()));
                    set_property(stat, "size", number(metadata.len() as f64));
                    return stat.to_bits();
                }
            }
        }
    }
    OtValue::null().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_stat_is_directory(stat: u64) -> u64 {
    let val = OtValue::from_bits(stat);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::Object
                && let Some(is_dir) = own_value(ptr, atom::atom("__isDir"))
            {
                return is_dir;
            }
        }
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_mkdir(path: u64, recursive: u64) -> u64 {
    let val = OtValue::from_bits(path);
    let is_recursive = !OtValue::from_bits(recursive).is_falsy();

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
                return OtValue::boolean(result.is_ok()).to_bits();
            }
        }
    }
    OtValue::boolean(false).to_bits()
}
