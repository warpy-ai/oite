use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file.display(), self.line, self.column)
    }
}

#[derive(Debug, Clone)]
pub struct DependencyInfo {
    pub path: PathBuf,
    pub specifier: String,
}

impl DependencyInfo {
    pub fn new(path: PathBuf, specifier: String) -> Self {
        Self { path, specifier }
    }
}

#[derive(Debug)]
pub enum ModuleErrorKind {
    NotFound {
        specifier: String,
        tried_paths: Vec<String>,
    },
    CycleDetected {
        cycle: Vec<String>,
    },
    ParseError {
        message: String,
        line: usize,
        column: usize,
    },
    ExportError {
        export_name: String,
        module_path: String,
        available_exports: Vec<String>,
    },
    UnsupportedSpec(String),
    UnsupportedAssertion {
        assertion_type: String,
        specifier: String,
    },
    IOError {
        path: PathBuf,
        message: String,
    },
}

#[derive(Debug)]
pub struct ModuleError {
    pub kind: ModuleErrorKind,
    pub source_location: Option<SourceLocation>,
    pub dependency_chain: Vec<DependencyInfo>,
    pub suggestion: Option<String>,
}

impl ModuleError {
    pub fn not_found(specifier: String, tried_paths: Vec<String>) -> Self {
        Self {
            kind: ModuleErrorKind::NotFound { specifier, tried_paths },
            source_location: None,
            dependency_chain: Vec::new(),
            suggestion: Some("Check the file path and ensure the file exists with a supported extension (.ot, .ts, .js)".to_string()),
        }
    }

    pub fn cycle_detected(cycle: Vec<String>) -> Self {
        Self {
            kind: ModuleErrorKind::CycleDetected { cycle },
            source_location: None,
            dependency_chain: Vec::new(),
            suggestion: Some(
                "Review your import statements to break the circular dependency".to_string(),
            ),
        }
    }

    pub fn parse_error(message: String, file: PathBuf, line: usize, column: usize) -> Self {
        Self {
            kind: ModuleErrorKind::ParseError {
                message,
                line,
                column,
            },
            source_location: Some(SourceLocation { file, line, column }),
            dependency_chain: Vec::new(),
            suggestion: None,
        }
    }

    pub fn export_not_found(
        export_name: String,
        module_path: String,
        available: Vec<String>,
    ) -> Self {
        Self {
            kind: ModuleErrorKind::ExportError {
                export_name,
                module_path,
                available_exports: available,
            },
            source_location: None,
            dependency_chain: Vec::new(),
            suggestion: Some(format!("Available exports: {}", available.join(", "))),
        }
    }

    pub fn unsupported_specifier(spec: String) -> Self {
        Self {
            kind: ModuleErrorKind::UnsupportedSpec(spec),
            source_location: None,
            dependency_chain: Vec::new(),
            suggestion: Some("Use relative paths (./, ../) for local imports".to_string()),
        }
    }

    pub fn unsupported_assertion(assertion_type: String, specifier: String) -> Self {
        Self {
            kind: ModuleErrorKind::UnsupportedAssertion {
                assertion_type,
                specifier,
            },
            source_location: None,
            dependency_chain: Vec::new(),
            suggestion: Some(
                "Import assertions are parsed but not enforced. The assertion will be ignored."
                    .to_string(),
            ),
        }
    }

    pub fn io_error(path: PathBuf, message: String) -> Self {
        Self {
            kind: ModuleErrorKind::IOError { path, message },
            source_location: None,
            dependency_chain: Vec::new(),
            suggestion: None,
        }
    }

    pub fn with_dependency_chain(mut self, chain: Vec<DependencyInfo>) -> Self {
        self.dependency_chain = chain;
        self
    }

    pub fn with_source_location(mut self, loc: SourceLocation) -> Self {
        self.source_location = Some(loc);
        self
    }
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.kind {
            ModuleErrorKind::NotFound {
                specifier,
                tried_paths,
            } => {
                writeln!(f, "Module '{}' not found", specifier)?;
                if !tried_paths.is_empty() {
                    writeln!(f, "\nAttempted paths:")?;
                    for path in tried_paths {
                        writeln!(f, "  - {}", path)?;
                    }
                }
            }
            ModuleErrorKind::CycleDetected { cycle } => {
                writeln!(f, "Circular dependency detected:")?;
                writeln!(f, "{}", cycle.join(" -> "))?;
            }
            ModuleErrorKind::ParseError {
                message,
                line,
                column,
            } => {
                if let Some(ref loc) = self.source_location {
                    writeln!(
                        f,
                        "Parse error at {}:{}:{}",
                        loc.file.display(),
                        line,
                        column
                    )?;
                } else {
                    writeln!(f, "Parse error at line {}, column {}", line, column)?;
                }
                writeln!(f, "{}", message)?;
            }
            ModuleErrorKind::ExportError {
                export_name,
                module_path,
                available_exports,
            } => {
                writeln!(
                    f,
                    "Export '{}' not found in module '{}'",
                    export_name, module_path
                )?;
                writeln!(f, "Available exports: {}", available_exports.join(", "))?;
            }
            ModuleErrorKind::UnsupportedSpec(spec) => {
                writeln!(f, "Unsupported module specifier: '{}'", spec)?;
            }
            ModuleErrorKind::UnsupportedAssertion {
                assertion_type,
                specifier,
            } => {
                writeln!(
                    f,
                    "Warning: Import assertion '{}' for '{}' is not fully supported",
                    assertion_type, specifier
                )?;
            }
            ModuleErrorKind::IOError { path, message } => {
                writeln!(f, "IO error reading '{}': {}", path.display(), message)?;
            }
        }

        if !self.dependency_chain.is_empty() {
            writeln!(f, "\nDependency chain:")?;
            for (i, dep) in self.dependency_chain.iter().enumerate() {
                writeln!(
                    f,
                    "  {}. {} (imported as '{}')",
                    i + 1,
                    dep.path.display(),
                    dep.specifier
                )?;
            }
        }

        if let Some(ref suggestion) = self.suggestion {
            writeln!(f, "\nSuggestion: {}", suggestion)?;
        }

        Ok(())
    }
}

impl std::error::Error for ModuleError {}

pub type ModuleResult<T> = Result<T, ModuleError>;
