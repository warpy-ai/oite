//! Modules, linked at compile time.
//!
//! A program that imports other modules is compiled as one program, the
//! same on every backend: each module once, in dependency order, the entry
//! module last. A module's top-level bindings are named with its prefix
//! (`@rolls/array::map`, `./lib.ot::double`), so modules cannot collide; the
//! entry module keeps its names. An import is not a copy: the importing
//! module refers to the exporter's binding itself, so it sees later
//! assignments to it (a live binding).
//!
//! Where a module comes from (see `docs/docs/platform-architecture.md`):
//!
//! - `./x`, `../x`: relative to the importing file; `.ot`, `.ts`, `.js` may
//!   be left out, and a directory means its `mod.ot` or `index.ot`.
//! - `@rolls/<name>`: the roll's `src/mod.ot`, from the first of:
//!   `--roll @rolls/<name>=DIR`, a sibling of the importing roll (rolls in
//!   one checkout find each other), each directory of `OITE_ROLLS_PATH`, and
//!   `$OITE_HOME/rolls` (the rolls bundled with oitec).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use swc_common::{Span, Spanned};
use swc_ecma_ast::*;
use swc_ecma_parser::{Syntax, TsSyntax};

use super::diagnostic::{CompileError, DebugInfo, Diagnostic};
use super::{Compiler, Parsed, parse, position};
use crate::vm::opcodes::OpCode;

/// How a module of a linked program names things (see `Codegen::link`).
#[derive(Debug, Clone, Default)]
pub struct ModuleLink {
    /// Prepended to the module's top-level names; empty for the entry
    /// module.
    pub prefix: String,
    /// Imported bindings: local name -> the global name of the binding.
    pub imports: HashMap<String, String>,
    /// `import * as ns`: local name -> (export name, global name), sorted.
    pub namespaces: HashMap<String, Vec<(String, String)>>,
}

/// Where `@rolls/*` modules are found.
#[derive(Debug, Clone, Default)]
pub struct ModuleOptions {
    /// `--roll @rolls/<name>=DIR`: the roll's directory.
    pub rolls: HashMap<String, PathBuf>,
    /// Directories holding rolls as `<name>/src/mod.ot`, searched in order.
    pub search: Vec<PathBuf>,
}

impl ModuleOptions {
    /// `OITE_ROLLS_PATH` (directories, `:`-separated), then
    /// `$OITE_HOME/rolls` (default `~/.oite/rolls`).
    pub fn from_env() -> Self {
        let mut search = Vec::new();
        if let Ok(paths) = std::env::var("OITE_ROLLS_PATH") {
            search.extend(std::env::split_paths(&paths).filter(|p| !p.as_os_str().is_empty()));
        }
        let home = std::env::var_os("OITE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".oite")));
        if let Some(home) = home {
            search.push(home.join("rolls"));
        }
        Self {
            rolls: HashMap::new(),
            search,
        }
    }

    /// Add `--roll @rolls/<name>=DIR`.
    pub fn add_roll(&mut self, spec: &str) -> Result<(), String> {
        let (name, dir) = spec
            .split_once('=')
            .ok_or_else(|| format!("--roll expects @rolls/<name>=DIR, got '{spec}'"))?;
        if !name.starts_with("@rolls/") {
            return Err(format!("--roll expects @rolls/<name>=DIR, got '{spec}'"));
        }
        self.rolls.insert(name.to_string(), PathBuf::from(dir));
        Ok(())
    }
}

/// One module of a linked program, compiled.
pub struct CompiledModule {
    /// The file, as stack traces and diagnostics show it.
    pub file: String,
    pub source: String,
    /// Its code, ending with `Halt`.
    pub code: Vec<OpCode>,
    pub debug: DebugInfo,
}

/// The syntax of a source file, by its extension: TypeScript with
/// decorators for `.ot` and `.ts`, JavaScript for `.js`.
pub fn syntax_for(path: &str) -> Syntax {
    if path.ends_with(".js") || path.ends_with(".jsx") {
        Syntax::Es(Default::default())
    } else {
        Syntax::Typescript(TsSyntax {
            decorators: true,
            tsx: path.ends_with(".tsx"),
            ..Default::default()
        })
    }
}

/// A module of the graph.
struct Module_ {
    path: PathBuf,
    /// The file as given (the entry) or as found.
    file: String,
    source: String,
    parsed: Parsed,
    prefix: String,
    /// What each `import`/`export ... from` names, resolved to a module.
    deps: HashMap<String, usize>,
}

/// Where an export comes from.
#[derive(Clone)]
enum Origin {
    /// A top-level binding of the module (or an import it re-exports).
    Local(String),
    /// `export { name as x } from "spec"`.
    From(String, String),
}

struct Linker<'a> {
    options: &'a ModuleOptions,
    modules: Vec<Module_>,
    by_path: HashMap<PathBuf, usize>,
    /// Modules in dependency order.
    order: Vec<usize>,
}

impl Compiler {
    /// Compile the program whose entry module is `entry` (with `source`),
    /// with every module it imports. The modules come back in the order
    /// they run: dependencies first, `entry` last. A program that imports
    /// nothing is one module, compiled as `compile_with_syntax` would.
    pub fn compile_program(
        &mut self,
        entry: &str,
        source: &str,
        options: &ModuleOptions,
    ) -> Result<Vec<CompiledModule>, CompileError> {
        let parsed = parse(source, Some(syntax_for(entry)))?;
        if module_requests(&parsed.program).is_empty() {
            let code = self.compile_parsed(&parsed, None)?;
            return Ok(vec![CompiledModule {
                file: entry.to_string(),
                source: source.to_string(),
                code,
                debug: std::mem::take(&mut self.debug_info),
            }]);
        }

        let mut linker = Linker {
            options,
            modules: Vec::new(),
            by_path: HashMap::new(),
            order: Vec::new(),
        };
        let entry_path = std::fs::canonicalize(entry).unwrap_or_else(|_| PathBuf::from(entry));
        linker.modules.push(Module_ {
            path: entry_path.clone(),
            file: entry.to_string(),
            source: source.to_string(),
            parsed,
            prefix: String::new(),
            deps: HashMap::new(),
        });
        linker.by_path.insert(entry_path, 0);
        linker.visit(0, &mut Vec::new())?;

        let exports = linker.export_tables()?;
        let mut compiled = Vec::new();
        for &m in &linker.order {
            let link = linker.link_for(m, &exports)?;
            let module = &linker.modules[m];
            let code = self
                .compile_parsed(&module.parsed, Some(link))
                .map_err(|e| e.in_file(&module.file, &module.source))?;
            compiled.push(CompiledModule {
                file: module.file.clone(),
                source: module.source.clone(),
                code,
                debug: std::mem::take(&mut self.debug_info),
            });
        }
        Ok(compiled)
    }
}

/// The modules a program asks for: (specifier, span of the request).
fn module_requests(program: &Program) -> Vec<(String, Span)> {
    let Program::Module(module) = program else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in &module.body {
        let ModuleItem::ModuleDecl(decl) = item else {
            continue;
        };
        let src = match decl {
            ModuleDecl::Import(i) if !i.type_only => Some(&i.src),
            ModuleDecl::ExportNamed(e) if !e.type_only => e.src.as_ref(),
            ModuleDecl::ExportAll(e) if !e.type_only => Some(&e.src),
            _ => None,
        };
        if let Some(src) = src {
            out.push((src.value.to_string_lossy().into_owned(), src.span));
        }
    }
    out
}

/// The name an export specifier or import specifier uses.
fn export_name(name: &ModuleExportName) -> String {
    let atom = name.atom();
    let s: &str = &atom;
    s.to_string()
}

impl Linker<'_> {
    fn error(&self, m: usize, span: Span, message: String) -> CompileError {
        let module = &self.modules[m];
        CompileError::single(Diagnostic::new(
            "error",
            message,
            position(&module.parsed.cm, span),
        ))
        .in_file(&module.file, &module.source)
    }

    /// Load the modules `m` imports, depth first, and append `m` to the
    /// order after them. `stack` is the chain of modules being loaded.
    fn visit(&mut self, m: usize, stack: &mut Vec<usize>) -> Result<(), CompileError> {
        stack.push(m);
        for (spec, span) in module_requests(&self.modules[m].parsed.program) {
            if self.modules[m].deps.contains_key(&spec) {
                continue;
            }
            let path = self.resolve(m, &spec).map_err(|e| self.error(m, span, e))?;
            let dep = match self.by_path.get(&path) {
                Some(&d) => {
                    if stack.contains(&d) {
                        let chain: Vec<&str> = stack
                            .iter()
                            .skip_while(|&&s| s != d)
                            .map(|&s| self.modules[s].file.as_str())
                            .chain(std::iter::once(self.modules[d].file.as_str()))
                            .collect();
                        return Err(self.error(
                            m,
                            span,
                            format!("circular import: {}", chain.join(" -> ")),
                        ));
                    }
                    d
                }
                None => {
                    let d = self.load(m, span, &spec, path)?;
                    self.visit(d, stack)?;
                    d
                }
            };
            self.modules[m].deps.insert(spec, dep);
        }
        stack.pop();
        self.order.push(m);
        Ok(())
    }

    /// Read and parse the module `spec` (at `path`), which module `m`
    /// imports at `span`.
    fn load(
        &mut self,
        m: usize,
        span: Span,
        spec: &str,
        path: PathBuf,
    ) -> Result<usize, CompileError> {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| self.error(m, span, format!("cannot read '{}': {}", path.display(), e)))?;
        let file = path.display().to_string();
        // A syntax error is reported in the module itself.
        let parsed =
            parse(&source, Some(syntax_for(&file))).map_err(|e| e.in_file(&file, &source))?;
        let prefix = if spec.starts_with("@rolls/") {
            format!("{spec}::")
        } else {
            format!("{}::", self.display_name(&path))
        };
        let id = self.modules.len();
        self.modules.push(Module_ {
            path: path.clone(),
            file,
            source,
            parsed,
            prefix,
            deps: HashMap::new(),
        });
        self.by_path.insert(path, id);
        Ok(id)
    }

    /// A short name for a module file: relative to the entry's directory.
    fn display_name(&self, path: &Path) -> String {
        let base = self.modules[0].path.parent().unwrap_or(Path::new(""));
        match path.strip_prefix(base) {
            Ok(rel) => format!("./{}", rel.display()),
            Err(_) => path.display().to_string(),
        }
    }

    /// The file `spec`, imported by module `m`, names.
    fn resolve(&self, m: usize, spec: &str) -> Result<PathBuf, String> {
        let importer = &self.modules[m].path;
        if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/') {
            let base = importer.parent().unwrap_or(Path::new("."));
            let target = base.join(spec);
            let candidates = [
                target.clone(),
                target.with_extension("ot"),
                target.with_extension("ts"),
                target.with_extension("js"),
                target.join("mod.ot"),
                target.join("index.ot"),
            ];
            return candidates
                .iter()
                .find(|c| c.is_file())
                .map(|c| std::fs::canonicalize(c).unwrap_or_else(|_| c.clone()))
                .ok_or_else(|| format!("cannot find module '{spec}' (from {})", base.display()));
        }
        if let Some(name) = spec.strip_prefix("@rolls/") {
            if name.is_empty() || name.contains('/') {
                return Err(format!("'{spec}' is not a roll name (@rolls/<name>)"));
            }
            let mut dirs: Vec<PathBuf> = Vec::new();
            if let Some(dir) = self.options.rolls.get(spec) {
                dirs.push(dir.clone());
            }
            // A roll next to the importing roll (<root>/<roll>/src/mod.ot).
            if let Some(root) = importer
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                && importer.parent().and_then(Path::file_name) == Some("src".as_ref())
            {
                dirs.push(root.join(name));
            }
            dirs.extend(self.options.search.iter().map(|d| d.join(name)));
            for dir in &dirs {
                let entry = dir.join("src").join("mod.ot");
                if entry.is_file() {
                    return Ok(std::fs::canonicalize(&entry).unwrap_or(entry));
                }
            }
            let looked: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
            return Err(format!(
                "cannot find roll '{spec}' (looked in: {}); pass --roll {spec}=DIR or set OITE_ROLLS_PATH",
                if looked.is_empty() {
                    "nowhere".to_string()
                } else {
                    looked.join(", ")
                }
            ));
        }
        Err(format!(
            "cannot import '{spec}': a module is a relative path ('./{spec}') or a roll ('@rolls/<name>')"
        ))
    }

    /// What each module exports: export name -> global name, following
    /// re-exports. Built in dependency order, so a module's dependencies
    /// are done before it.
    fn export_tables(&self) -> Result<Vec<HashMap<String, String>>, CompileError> {
        let mut tables: Vec<HashMap<String, String>> = vec![HashMap::new(); self.modules.len()];
        for &m in &self.order {
            let module = &self.modules[m];
            let Program::Module(ast) = &module.parsed.program else {
                continue;
            };
            let imports = self.imports_of(m, &tables)?;
            let mut origins: Vec<(String, Origin, Span)> = Vec::new();
            let mut star: Vec<usize> = Vec::new();
            for item in &ast.body {
                let ModuleItem::ModuleDecl(decl) = item else {
                    continue;
                };
                match decl {
                    ModuleDecl::ExportDecl(e) => {
                        for name in declared_names(&e.decl) {
                            origins.push((name.clone(), Origin::Local(name), e.span));
                        }
                    }
                    ModuleDecl::ExportDefaultDecl(_) | ModuleDecl::ExportDefaultExpr(_) => {
                        origins.push((
                            "default".into(),
                            Origin::Local("default".into()),
                            decl.span(),
                        ));
                    }
                    ModuleDecl::ExportNamed(e) if !e.type_only => {
                        let from = e
                            .src
                            .as_ref()
                            .map(|s| s.value.to_string_lossy().into_owned());
                        for spec in &e.specifiers {
                            match spec {
                                ExportSpecifier::Named(n) if !n.is_type_only => {
                                    let orig = export_name(&n.orig);
                                    let exported = n
                                        .exported
                                        .as_ref()
                                        .map(export_name)
                                        .unwrap_or_else(|| orig.clone());
                                    let origin = match &from {
                                        Some(f) => Origin::From(f.clone(), orig),
                                        None => Origin::Local(orig),
                                    };
                                    origins.push((exported, origin, n.span));
                                }
                                ExportSpecifier::Named(_) => {}
                                _ => {
                                    return Err(self.error(
                                        m,
                                        spec.span(),
                                        "unsupported: this form of `export ... from`".into(),
                                    ));
                                }
                            }
                        }
                    }
                    ModuleDecl::ExportAll(e) if !e.type_only => {
                        star.push(module.deps[&e.src.value.to_string_lossy().into_owned()]);
                    }
                    _ => {}
                }
            }
            let mut table = HashMap::new();
            // `export *`: every export but `default`; the module's own
            // exports, below, take precedence.
            for d in star {
                for (name, global) in &tables[d] {
                    if name != "default" {
                        table.insert(name.clone(), global.clone());
                    }
                }
            }
            for (name, origin, span) in origins {
                let global = match origin {
                    Origin::Local(local) => match imports.get(&local) {
                        Some(g) => g.clone(),
                        None => format!("{}{}", module.prefix, local),
                    },
                    Origin::From(spec, orig) => {
                        let d = module.deps[&spec];
                        tables[d].get(&orig).cloned().ok_or_else(|| {
                            self.error(m, span, format!("'{spec}' has no export named '{orig}'"))
                        })?
                    }
                };
                table.insert(name, global);
            }
            tables[m] = table;
        }
        Ok(tables)
    }

    /// Module `m`'s named and default imports: local name -> global name.
    fn imports_of(
        &self,
        m: usize,
        tables: &[HashMap<String, String>],
    ) -> Result<HashMap<String, String>, CompileError> {
        let mut out = HashMap::new();
        let module = &self.modules[m];
        let Program::Module(ast) = &module.parsed.program else {
            return Ok(out);
        };
        for item in &ast.body {
            let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
                continue;
            };
            if import.type_only {
                continue;
            }
            let spec = import.src.value.to_string_lossy().into_owned();
            let table = &tables[module.deps[&spec]];
            for s in &import.specifiers {
                let (local, name, span) = match s {
                    ImportSpecifier::Named(n) if !n.is_type_only => (
                        n.local.sym.to_string(),
                        n.imported
                            .as_ref()
                            .map(export_name)
                            .unwrap_or_else(|| n.local.sym.to_string()),
                        n.span,
                    ),
                    ImportSpecifier::Default(d) => {
                        (d.local.sym.to_string(), "default".to_string(), d.span)
                    }
                    _ => continue,
                };
                let global = table.get(&name).ok_or_else(|| {
                    let what = if name == "default" {
                        "default export".to_string()
                    } else {
                        format!("export named '{name}'")
                    };
                    self.error(m, span, format!("'{spec}' has no {what}"))
                })?;
                out.insert(local, global.clone());
            }
        }
        Ok(out)
    }

    fn link_for(
        &self,
        m: usize,
        tables: &[HashMap<String, String>],
    ) -> Result<ModuleLink, CompileError> {
        let module = &self.modules[m];
        let mut link = ModuleLink {
            prefix: module.prefix.clone(),
            imports: self.imports_of(m, tables)?,
            namespaces: HashMap::new(),
        };
        if let Program::Module(ast) = &module.parsed.program {
            for item in &ast.body {
                let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
                    continue;
                };
                for s in &import.specifiers {
                    if let ImportSpecifier::Namespace(ns) = s {
                        let spec = import.src.value.to_string_lossy().into_owned();
                        let mut members: Vec<(String, String)> = tables[module.deps[&spec]]
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        members.sort();
                        link.namespaces.insert(ns.local.sym.to_string(), members);
                    }
                }
            }
        }
        Ok(link)
    }
}

/// The names a declaration binds (`export function f`, `export const a = 1`).
fn declared_names(decl: &Decl) -> Vec<String> {
    match decl {
        Decl::Fn(f) if !f.declare => vec![f.ident.sym.to_string()],
        Decl::Class(c) if !c.declare => vec![c.ident.sym.to_string()],
        Decl::Var(v) if !v.declare => v
            .decls
            .iter()
            .flat_map(|d| super::scope::pattern_names(&d.name))
            .collect(),
        Decl::TsEnum(e) if !e.declare => vec![e.id.sym.to_string()],
        _ => Vec::new(),
    }
}

/// Concatenate a linked program's modules into one instruction stream:
/// each module but the last falls through into the next instead of
/// halting. Also where each module starts, for its source positions.
pub fn concat(
    modules: Vec<CompiledModule>,
    start: usize,
) -> (Vec<OpCode>, Vec<(usize, String, String, DebugInfo)>) {
    let mut code = Vec::new();
    let mut parts = Vec::new();
    let count = modules.len();
    for (i, mut m) in modules.into_iter().enumerate() {
        if i + 1 < count && matches!(m.code.last(), Some(OpCode::Halt)) {
            m.code.pop();
        }
        let offset = start + code.len();
        code.extend(crate::vm::rebase_program(m.code, offset));
        parts.push((offset, m.file, m.source, m.debug));
    }
    (code, parts)
}
