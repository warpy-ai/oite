use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use swc_common::{BytePos, FileName, input::StringInput, source_map::SourceMap};
use swc_ecma_parser::{Parser, Syntax, TsSyntax, lexer::Lexer};

use crate::module::diagnostics::{DependencyInfo, ModuleError, ModuleResult};
use crate::module::resolver::{ImportAssertions, ModuleResolver, ResolvedModule};

#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub path: Arc<PathBuf>,
    pub source: String,
    pub ast: swc_ecma_ast::Module,
    pub imports: Vec<ModuleImport>,
    pub exports: Vec<ModuleExport>,
    pub assertions: Option<ImportAssertions>,
    pub content_hash: String,
}

#[derive(Debug, Clone)]
pub struct ModuleImport {
    pub specifier: String,
    pub local_name: Option<String>,
    pub imported_name: Option<String>,
    pub is_namespace: bool,
    pub is_default: bool,
    pub is_side_effect: bool,
    pub assertions: Option<ImportAssertions>,
}

#[derive(Debug, Clone)]
pub struct ModuleExport {
    pub name: String,
    pub is_default: bool,
    pub local_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub path: Arc<PathBuf>,
    pub source: String,
    pub parsed: ParsedModule,
    pub namespace: ModuleNamespace,
    pub dependencies: Vec<(ModuleImport, Arc<LoadedModule>)>,
}

#[derive(Debug, Clone)]
pub struct ModuleNamespace {
    pub exports: HashMap<String, ModuleValue>,
    pub default_export: Option<ModuleValue>,
    pub path: Arc<PathBuf>,
}

#[derive(Debug)]
pub struct ModuleCache {
    entries: HashMap<PathBuf, CachedModule>,
    content_hashes: HashMap<PathBuf, String>,
    modification_times: HashMap<PathBuf, SystemTime>,
}

#[derive(Debug, Clone)]
struct CachedModule {
    module: Arc<LoadedModule>,
    hash: String,
    load_time: SystemTime,
}

impl ModuleCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            content_hashes: HashMap::new(),
            modification_times: HashMap::new(),
        }
    }

    pub fn get(&self, path: &PathBuf) -> Option<Arc<LoadedModule>> {
        if let Some(cached_hash) = self.content_hashes.get(path) {
            let current_hash = self.compute_hash(path);
            if cached_hash == &current_hash {
                if let Some(cached) = self.entries.get(path) {
                    return Some(cached.module.clone());
                }
            }
        }
        None
    }

    fn compute_hash(&self, path: &PathBuf) -> String {
        match fs::read(path) {
            Ok(content) => {
                let mut hasher = Sha256::new();
                hasher.update(&content);
                let result = hasher.finalize();
                hex::encode(result)
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
                }
                true
            }
            Err(_) => true,
        }
    }

    pub fn insert(&mut self, module: Arc<LoadedModule>) {
        let path = (*module.path).clone();
        let hash = self.compute_hash(&path);

        self.content_hashes.insert(path.clone(), hash.clone());
        self.entries.insert(
            path.clone(),
            CachedModule {
                module: module.clone(),
                hash,
                load_time: SystemTime::now(),
            },
        );
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
}

pub struct ModuleLoader {
    resolver: ModuleResolver,
    cache: Arc<Mutex<ModuleCache>>,
    in_progress: Arc<Mutex<HashSet<PathBuf>>>,
}

impl ModuleLoader {
    pub fn new() -> Self {
        Self {
            resolver: ModuleResolver::new(),
            cache: Arc::new(Mutex::new(ModuleCache::new())),
            in_progress: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn with_base_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        Self {
            resolver: self.resolver.with_base_path(path),
            ..self
        }
    }

    pub fn resolver(&self) -> &ModuleResolver {
        &self.resolver
    }

    pub async fn load(&mut self, entry_path: &Path) -> ModuleResult<Arc<LoadedModule>> {
        let canonical = fs::canonicalize(entry_path)
            .map_err(|e| ModuleError::io_error(entry_path.to_path_buf(), e.to_string()))?;

        let cache = self.cache.lock().unwrap();
        if let Some(cached) = cache.get(&canonical) {
            return Ok(cached);
        }
        drop(cache);

        {
            let in_progress = self.in_progress.lock().unwrap();
            if in_progress.contains(&canonical) {
                return Err(ModuleError::cycle_detected(self.format_cycle(&canonical)));
            }
        }

        {
            let mut in_progress = self.in_progress.lock().unwrap();
            in_progress.insert(canonical.clone());
        }

        let source = fs::read_to_string(&canonical)
            .map_err(|e| ModuleError::io_error(canonical.clone(), e.to_string()))?;

        let parsed = self.parse_module(&canonical, &source)?;
        let imports = self.extract_imports(&parsed.ast);

        let mut dependencies = Vec::new();
        for import in &imports {
            let resolved = self.resolver.resolve(&import.specifier, &canonical)?;
            let loaded = self.load(&resolved.path).await?;
            dependencies.push((import.clone(), loaded));
        }

        let namespace = self.build_namespace(&parsed.exports, &dependencies);

        let loaded = Arc::new(LoadedModule {
            path: Arc::new(canonical.clone()),
            source,
            parsed: parsed.clone(),
            namespace,
            dependencies,
        });

        {
            let mut in_progress = self.in_progress.lock().unwrap();
            in_progress.remove(&canonical);
        }

        let mut cache = self.cache.lock().unwrap();
        cache.insert(loaded.clone());
        Ok(loaded)
    }

    fn parse_module(&self, path: &PathBuf, source: &str) -> ModuleResult<ParsedModule> {
        let source_map = SourceMap::new();
        let fm = source_map.new_source_file(
            FileName::Custom(path.to_string_lossy().to_string()).into(),
            source.to_string(),
        );

        let lexer = Lexer::new(
            Syntax::Typescript(TsSyntax {
                decorators: true,
                tsx: false,
                dts: false,
                no_early_errors: false,
                disallow_ambiguous_jsx_like: true,
            }),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );

        let mut parser = Parser::new_from(lexer);

        match parser.parse_module() {
            Ok(ast) => {
                let imports = self.extract_imports(&ast);
                let exports = self.extract_exports(&ast);
                let content_hash = self.compute_source_hash(source);

                Ok(ParsedModule {
                    path: Arc::new(path.clone()),
                    source: source.to_string(),
                    ast,
                    imports,
                    exports,
                    assertions: None,
                    content_hash,
                })
            }
            Err(e) => {
                let location = format!("{:?}", e.span());
                Err(ModuleError::parse_error(
                    format!("Parse error: {}", e.kind().msg()),
                    path.clone(),
                    0,
                    0,
                ))
            }
        }
    }

    fn extract_imports(&self, ast: &swc_ecma_ast::Module) -> Vec<ModuleImport> {
        let mut imports = Vec::new();

        for item in &ast.body {
            if let swc_ecma_ast::ModuleItem::ModuleDecl(decl) = item {
                if let swc_ecma_ast::ModuleDecl::Import(import) = decl {
                    let specifier = import.src.value.to_string();
                    let assertions = import
                        .with
                        .as_ref()
                        .map(|with| self.resolver.parse_import_assertions(Some(with)))
                        .flatten();

                    for spec in &import.specifiers {
                        match spec {
                            swc_ecma_ast::ImportSpecifier::Named(named) => {
                                let local = named.local.sym.to_string();
                                let imported = named
                                    .imported
                                    .as_ref()
                                    .map(|i| i.sym.to_string())
                                    .unwrap_or_else(|| local.clone());

                                imports.push(ModuleImport {
                                    specifier: specifier.clone(),
                                    local_name: Some(local),
                                    imported_name: Some(imported),
                                    is_namespace: false,
                                    is_default: false,
                                    is_side_effect: false,
                                    assertions: assertions.clone(),
                                });
                            }
                            swc_ecma_ast::ImportSpecifier::Default(default) => {
                                imports.push(ModuleImport {
                                    specifier: specifier.clone(),
                                    local_name: Some(default.local.sym.to_string()),
                                    imported_name: None,
                                    is_namespace: false,
                                    is_default: true,
                                    is_side_effect: false,
                                    assertions: assertions.clone(),
                                });
                            }
                            swc_ecma_ast::ImportSpecifier::Namespace(ns) => {
                                imports.push(ModuleImport {
                                    specifier: specifier.clone(),
                                    local_name: Some(ns.local.sym.to_string()),
                                    imported_name: None,
                                    is_namespace: true,
                                    is_default: false,
                                    is_side_effect: false,
                                    assertions: assertions.clone(),
                                });
                            }
                        }
                    }

                    if import.specifiers.is_empty() {
                        imports.push(ModuleImport {
                            specifier,
                            local_name: None,
                            imported_name: None,
                            is_namespace: false,
                            is_default: false,
                            is_side_effect: true,
                            assertions,
                        });
                    }
                }
            }
        }

        imports
    }

    fn extract_exports(&self, ast: &swc_ecma_ast::Module) -> Vec<ModuleExport> {
        let mut exports = Vec::new();

        for item in &ast.body {
            if let swc_ecma_ast::ModuleItem::ModuleDecl(decl) = item {
                match decl {
                    swc_ecma_ast::ModuleDecl::ExportNamed(named) => {
                        if let Some(src) = &named.src {
                            for spec in &named.specifiers {
                                let export_name = match spec {
                                    swc_ecma_ast::ExportSpecifier::Named(named) => {
                                        named.orig.sym.to_string()
                                    }
                                    swc_ecma_ast::ExportSpecifier::Default(_) => {
                                        "default".to_string()
                                    }
                                    swc_ecma_ast::ExportSpecifier::Namespace(ns) => {
                                        ns.sym.to_string()
                                    }
                                };
                                exports.push(ModuleExport {
                                    name: export_name,
                                    is_default: false,
                                    local_name: None,
                                });
                            }
                        } else {
                            for spec in &named.specifiers {
                                match spec {
                                    swc_ecma_ast::ExportSpecifier::Named(named) => {
                                        let exported = named
                                            .exported
                                            .as_ref()
                                            .map(|e| &e.sym)
                                            .unwrap_or(&named.orig.sym);
                                        exports.push(ModuleExport {
                                            name: exported.to_string(),
                                            is_default: false,
                                            local_name: Some(named.orig.sym.to_string()),
                                        });
                                    }
                                    swc_ecma_ast::ExportSpecifier::Default(_) => {
                                        exports.push(ModuleExport {
                                            name: "default".to_string(),
                                            is_default: true,
                                            local_name: None,
                                        });
                                    }
                                    swc_ecma_ast::ExportSpecifier::Namespace(ns) => {
                                        exports.push(ModuleExport {
                                            name: ns.sym.to_string(),
                                            is_default: false,
                                            local_name: None,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    swc_ecma_ast::ModuleDecl::ExportAll(_all) => {
                        exports.push(ModuleExport {
                            name: "*".to_string(),
                            is_default: false,
                            local_name: None,
                        });
                    }
                    swc_ecma_ast::ModuleDecl::ExportDefaultDecl(_default) => {
                        exports.push(ModuleExport {
                            name: "default".to_string(),
                            is_default: true,
                            local_name: None,
                        });
                    }
                    _ => {}
                }
            } else if let swc_ecma_ast::ModuleItem::Stmt(stmt) = item {
                if let swc_ecma_ast::Stmt::Decl(decl) = stmt {
                    if let swc_ecma_ast::Decl::Var(var) = decl {
                        for declarator in &var.decls {
                            if let swc_ecma_ast::Pat::Ident(ident) = &declarator.name {
                                exports.push(ModuleExport {
                                    name: ident.id.sym.to_string(),
                                    is_default: false,
                                    local_name: Some(ident.id.sym.to_string()),
                                });
                            }
                        }
                    } else if let swc_ecma_ast::Decl::Fn(fn_decl) = decl {
                        exports.push(ModuleExport {
                            name: fn_decl.ident.sym.to_string(),
                            is_default: false,
                            local_name: Some(fn_decl.ident.sym.to_string()),
                        });
                    } else if let swc_ecma_ast::Decl::Class(class) = decl {
                        exports.push(ModuleExport {
                            name: class.ident.sym.to_string(),
                            is_default: false,
                            local_name: Some(class.ident.sym.to_string()),
                        });
                    }
                }
            }
        }

        exports
    }

    fn build_namespace(
        &self,
        exports: &[ModuleExport],
        dependencies: &[(ModuleImport, Arc<LoadedModule>)],
    ) -> ModuleNamespace {
        let mut namespace_exports: HashMap<String, ModuleValue> = HashMap::new();
        let mut default_export = None;

        for export in exports {
            if export.is_default {
                default_export = Some(ModuleValue::Undefined);
            } else if let Some(local_name) = &export.local_name {
                namespace_exports.insert(export.name.clone(), ModuleValue::Undefined);
            }
        }

        ModuleNamespace {
            exports: namespace_exports,
            default_export,
            path: Arc::new(PathBuf::new()),
        }
    }

    fn compute_source_hash(&self, source: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn format_cycle(&self, path: &PathBuf) -> Vec<String> {
        let in_progress = self.in_progress.lock().unwrap();
        let cycle: Vec<String> = in_progress
            .iter()
            .filter(|p| p == &path)
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        cycle
    }
}

#[derive(Debug, Clone)]
pub enum ModuleValue {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Object(HashMap<String, ModuleValue>),
    Function(String),
    Module(Arc<PathBuf>),
}

impl Default for ModuleValue {
    fn default() -> Self {
        ModuleValue::Undefined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_module() {
        let mut loader = ModuleLoader::new();
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path();

        std::fs::write(
            temp_path.join("math.ot"),
            r#"
export function add(a: number, b: number): number {
    return a + b;
}

export const PI = 3.14159;

export default function multiply(a: number, b: number): number {
    return a * b;
}
"#,
        )
        .unwrap();

        std::fs::write(
            temp_path.join("main.ot"),
            r#"
import { add, PI } from './math';
import multiply from './math';

const result = add(PI as number, 10);
export const output = result;
"#,
        )
        .unwrap();

        let result = loader.load(&temp_path.join("main.ot")).await;
        assert!(result.is_ok(), "Failed to load module: {:?}", result.err());

        let module = result.unwrap();
        assert!(!module.dependencies.is_empty());
    }
}
