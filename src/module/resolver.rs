use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::module::diagnostics::ModuleErrorKind;
use crate::module::diagnostics::{ModuleError, ModuleResult};

#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub path: Arc<PathBuf>,
    pub original_specifier: String,
    pub is_entry: bool,
    pub assertions: Option<ImportAssertions>,
}

impl ResolvedModule {
    pub fn new(
        path: PathBuf,
        original_specifier: String,
        is_entry: bool,
        assertions: Option<ImportAssertions>,
    ) -> Self {
        Self {
            path: Arc::new(path),
            original_specifier,
            is_entry,
            assertions,
        }
    }

    pub fn as_entry(source: &str) -> Self {
        let path = Arc::new(fs::canonicalize(source).unwrap_or_else(|_| PathBuf::from(source)));
        Self {
            path,
            original_specifier: source.to_string(),
            is_entry: true,
            assertions: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportAssertions {
    TypeOnly,
    JSON,
    Custom(Vec<(String, String)>),
}

impl ImportAssertions {
    pub fn is_type_only(&self) -> bool {
        matches!(self, ImportAssertions::TypeOnly)
    }

    pub fn is_json(&self) -> bool {
        matches!(self, ImportAssertions::JSON)
    }
}

pub struct ModuleResolver {
    extensions: [&'static str; 3],
    base_paths: Vec<PathBuf>,
}

impl Default for ModuleResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleResolver {
    pub fn new() -> Self {
        Self {
            extensions: [".ot", ".ts", ".js"],
            base_paths: Vec::new(),
        }
    }

    pub fn with_base_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.base_paths.push(path.into());
        self
    }

    pub fn resolve(&self, specifier: &str, importer: &Path) -> ModuleResult<ResolvedModule> {
        if specifier.is_empty() {
            return Err(ModuleError::unsupported_specifier(specifier.to_string()));
        }

        let first_char = specifier.chars().next().unwrap();

        match first_char {
            '.' => self.resolve_relative(specifier, importer),
            _ => Err(ModuleError::unsupported_specifier(specifier.to_string())),
        }
    }

    fn resolve_relative(&self, specifier: &str, importer: &Path) -> ModuleResult<ResolvedModule> {
        let importer_dir = if importer.is_file() {
            importer.parent().unwrap_or(Path::new("."))
        } else {
            importer
        };

        let mut path = importer_dir.to_path_buf();
        let mut tried_paths = Vec::new();

        for component in specifier.split('/') {
            match component {
                "." => {}
                ".." => {
                    if !path.as_os_str().is_empty() {
                        path.pop();
                    }
                }
                "" if specifier.starts_with("./") => {}
                "" if specifier.starts_with("../") => {}
                _ => path.push(component),
            };
        }

        if path.as_os_str().is_empty() || specifier.ends_with('/') {
            for ext in self.extensions {
                let index_path = path.join("index").with_extension(&ext[1..]);
                tried_paths.push(index_path.display().to_string());
                if index_path.exists() {
                    let canonical = fs::canonicalize(&index_path)?;
                    return Ok(ResolvedModule::new(
                        canonical,
                        specifier.to_string(),
                        false,
                        None,
                    ));
                }
            }
            return Err(ModuleError::not_found(specifier.to_string(), tried_paths));
        }

        for ext in self.extensions {
            let with_ext = path.with_extension(&ext[1..]);
            tried_paths.push(with_ext.display().to_string());
            if with_ext.exists() {
                let canonical = fs::canonicalize(&with_ext)?;
                return Ok(ResolvedModule::new(
                    canonical,
                    specifier.to_string(),
                    false,
                    None,
                ));
            }
        }

        if path.exists() && path.is_dir() {
            for ext in self.extensions {
                let index_path = path.join("index").with_extension(&ext[1..]);
                tried_paths.push(index_path.display().to_string());
                if index_path.exists() {
                    let canonical = fs::canonicalize(&index_path)?;
                    return Ok(ResolvedModule::new(
                        canonical,
                        specifier.to_string(),
                        false,
                        None,
                    ));
                }
            }
        }

        Err(ModuleError::not_found(specifier.to_string(), tried_paths))
    }

    pub fn parse_import_assertions(
        &self,
        with: Option<&swc_ecma_ast::ObjectLit>,
    ) -> Option<ImportAssertions> {
        if with.is_none() {
            return None;
        }

        let with = with.unwrap();

        for prop in &with.props {
            if let swc_ecma_ast::PropOrSpread::Prop(prop) = prop {
                if let swc_ecma_ast::Prop::KeyValue(kv) = prop.as_ref() {
                    if let swc_ecma_ast::PropName::Str(key) = &kv.key {
                        if key.value == "type" {
                            if let swc_ecma_ast::Expr::Lit(lit) = kv.value.as_ref() {
                                if let swc_ecma_ast::Lit::Str(str_lit) = lit {
                                    match str_lit.value.as_str() {
                                        "json" => return Some(ImportAssertions::JSON),
                                        "typescript" | "ts" => {
                                            return Some(ImportAssertions::TypeOnly);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let custom_assertions: Vec<(String, String)> = with
            .props
            .iter()
            .filter_map(|prop| {
                if let swc_ecma_ast::PropOrSpread::Prop(prop) = prop {
                    if let swc_ecma_ast::Prop::KeyValue(kv) = prop.as_ref() {
                        let key = match &kv.key {
                            swc_ecma_ast::PropName::Str(s) => {
                                s.value.to_string_lossy().into_owned()
                            }
                            swc_ecma_ast::PropName::Ident(i) => i.sym.to_string(),
                            _ => return None,
                        };
                        let value = match kv.value.as_ref() {
                            swc_ecma_ast::Expr::Lit(lit) => match lit {
                                swc_ecma_ast::Lit::Str(s) => s.value.to_string_lossy().into_owned(),
                                swc_ecma_ast::Lit::Bool(b) => b.value.to_string(),
                                swc_ecma_ast::Lit::Num(n) => n.value.to_string(),
                                _ => return None,
                            },
                            _ => return None,
                        };
                        Some((key, value))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if custom_assertions.is_empty() {
            None
        } else {
            Some(ImportAssertions::Custom(custom_assertions))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_relative_file() {
        let resolver = ModuleResolver::new();
        let importer = PathBuf::from("/project/src/main.ot");

        let result = resolver.resolve("./utils", &importer);
        assert!(result.is_ok());

        let resolved = result.unwrap();
        assert_eq!(resolved.original_specifier, "./utils");
    }

    #[test]
    fn test_resolve_parent_directory() {
        let resolver = ModuleResolver::new();
        let importer = PathBuf::from("/project/src/utils/helper.ot");

        let result = resolver.resolve("../lib/math", &importer);
        assert!(result.is_ok());

        let resolved = result.unwrap();
        assert_eq!(resolved.original_specifier, "../lib/math");
    }

    #[test]
    fn test_resolve_with_extension() {
        let resolver = ModuleResolver::new();
        let importer = PathBuf::from("/project/src/main.ot");

        let result = resolver.resolve("./foo.js", &importer);
        assert!(result.is_ok());
    }

    #[test]
    fn test_unsupported_specifier() {
        let resolver = ModuleResolver::new();
        let importer = PathBuf::from("/project/src/main.ot");

        let result = resolver.resolve("react", &importer);
        assert!(result.is_err());

        if let Err(e) = result {
            match &e.kind {
                ModuleErrorKind::UnsupportedSpec(_) => {}
                _ => panic!("Expected UnsupportedSpec error"),
            }
        }
    }
}
