use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct CachedModule {
    pub path: PathBuf,
    pub source: String,
    pub hash: String,
    pub load_time: SystemTime,
    pub namespace_object: usize,
}

pub struct ModuleCache {
    pub entries: HashMap<PathBuf, CachedModule>,
    content_hashes: HashMap<PathBuf, String>,
    modification_times: HashMap<PathBuf, SystemTime>,
}

impl ModuleCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            content_hashes: HashMap::new(),
            modification_times: HashMap::new(),
        }
    }

    pub fn get(&self, path: &PathBuf) -> Option<&CachedModule> {
        if let Some(cached) = self.entries.get(path) {
            let current_hash = ModuleCache::compute_hash(path);
            if cached.hash == current_hash {
                return Some(cached);
            }
        }
        None
    }

    pub fn get_valid(&self, path: &PathBuf) -> Option<&CachedModule> {
        if let Some(cached) = self.entries.get(path) {
            if let Ok(metadata) = fs::metadata(path) {
                if let Ok(modified) = metadata.modified() {
                    if let Some(cached_time) = self.modification_times.get(path) {
                        if modified <= *cached_time {
                            return Some(cached);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn get_or_compute(&mut self, path: &PathBuf) -> Option<CachedModule> {
        if let Some(cached) = self.get_valid(path) {
            return Some(cached.clone());
        }

        let hash = ModuleCache::compute_hash(path);
        if !hash.is_empty() {
            if let Ok(metadata) = fs::metadata(path) {
                if let Ok(modified) = metadata.modified() {
                    self.content_hashes.insert(path.clone(), hash.clone());
                    self.modification_times.insert(path.clone(), modified);
                    return None;
                }
            }
        }
        None
    }

    pub fn compute_hash(path: &PathBuf) -> String {
        match fs::read(path) {
            Ok(content) => {
                let mut hasher = Sha256::new();
                hasher.update(&content);
                hex::encode(hasher.finalize())
            }
            Err(_) => String::new(),
        }
    }

    pub fn should_reload(&self, path: &PathBuf) -> bool {
        match fs::metadata(path) {
            Ok(metadata) => {
                if let Ok(modified) = metadata.modified() {
                    if let Some(cached_time) = self.modification_times.get(path) {
                        return modified > *cached_time;
                    }
                    return true;
                }
                true
            }
            Err(_) => true,
        }
    }

    pub fn insert(&mut self, module: CachedModule) {
        let path = module.path.clone();
        let hash = module.hash.clone();

        self.entries.insert(path.clone(), module);
        self.content_hashes.insert(path.clone(), hash);
        if let Ok(metadata) = fs::metadata(&path) {
            if let Ok(modified) = metadata.modified() {
                self.modification_times.insert(path, modified);
            }
        }
    }

    pub fn invalidate(&mut self, path: &PathBuf) {
        self.entries.remove(path);
        self.content_hashes.remove(path);
        self.modification_times.remove(path);
    }

    pub fn invalidate_all(&mut self) {
        self.entries.clear();
        self.content_hashes.clear();
        self.modification_times.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn cache_size_bytes(&self) -> usize {
        self.entries
            .values()
            .map(|cached| cached.path.as_os_str().len() + cached.source.len() + cached.hash.len())
            .sum()
    }

    pub fn get_cache_info(&self, path: &PathBuf) -> Option<(SystemTime, String, usize)> {
        self.entries.get(path).map(|cached| {
            (
                cached.load_time.clone(),
                cached.hash.clone(),
                cached.namespace_object,
            )
        })
    }

    pub fn check_hot_reload(&mut self, path: &PathBuf) -> bool {
        if self.should_reload(path) {
            self.invalidate(path);
            true
        } else {
            false
        }
    }

    pub fn entries(&self) -> &HashMap<PathBuf, CachedModule> {
        &self.entries
    }

    pub fn has_content_hash(&self, path: &PathBuf) -> bool {
        self.content_hashes.contains_key(path)
    }
}
