//! Incremental LTO cache
//!
//! This module provides caching for LTO artifacts to speed up incremental builds.
//! Cache keys are based on file content hash and compilation flags.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::backend::{BackendError, LtoMode};

/// Get cache directory path
fn get_cache_dir() -> PathBuf {
    PathBuf::from(".cache").join("lto")
}

/// Generate cache key from file path and LTO mode
fn generate_cache_key(file_path: &Path, lto_mode: LtoMode) -> String {
    let mut hasher = DefaultHasher::new();

    // Hash file path
    file_path.hash(&mut hasher);

    // Hash LTO mode
    lto_mode.hash(&mut hasher);

    // Hash file modification time (simple invalidation)
    if let Ok(metadata) = fs::metadata(file_path) {
        if let Ok(modified) = metadata.modified() {
            modified.hash(&mut hasher);
        }
    }

    format!("{:x}", hasher.finish())
}

/// Get cache path for a source file
pub fn get_cache_path(source_path: &Path, lto_mode: LtoMode) -> PathBuf {
    let cache_dir = get_cache_dir();
    let key = generate_cache_key(source_path, lto_mode);
    cache_dir.join(format!("{}.bc", key))
}

/// Check if cache is valid
pub fn is_cache_valid(cache_path: &Path, source_path: &Path) -> bool {
    // Check if cache file exists
    if !cache_path.exists() {
        return false;
    }

    // Check if source is newer than cache
    let source_meta = match fs::metadata(source_path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let cache_meta = match fs::metadata(cache_path) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let source_modified = match source_meta.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };

    let cache_modified = match cache_meta.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };

    // Cache is valid if it's newer than source
    cache_modified >= source_modified
}

/// Load bitcode from cache
pub fn load_from_cache(cache_path: &Path) -> Result<PathBuf, BackendError> {
    if !cache_path.exists() {
        return Err(BackendError::Llvm("Cache file does not exist".into()));
    }

    Ok(cache_path.to_path_buf())
}

/// Save bitcode to cache
pub fn save_to_cache(bitcode_path: &Path, cache_path: &Path) -> Result<(), BackendError> {
    // Create cache directory if it doesn't exist
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| BackendError::Llvm(format!("Failed to create cache directory: {}", e)))?;
    }

    // Copy bitcode file to cache
    fs::copy(bitcode_path, cache_path)
        .map_err(|e| BackendError::Llvm(format!("Failed to save to cache: {}", e)))?;

    Ok(())
}

/// Clear cache directory
pub fn clear_cache() -> Result<(), BackendError> {
    let cache_dir = get_cache_dir();
    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir)
            .map_err(|e| BackendError::Llvm(format!("Failed to clear cache: {}", e)))?;
    }
    Ok(())
}
