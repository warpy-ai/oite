//! The type checker (docs/docs/types.md, section 4).
//!
//! An annotation is a contract: the checker reports where a value of a
//! known type is used against one. Code with no annotations is dynamic
//! (`any`) and is never reported. The checker is deliberately
//! conservative: where it does not know a type, the type is `any`.
//!
//! It runs on the syntax tree, per module. Names imported from other
//! modules are `any`.

mod assign;
mod builtins;
mod classes;
mod expr;
mod flow;
mod functions;
mod guards;
mod narrow;
mod resolve;
mod stmt;
pub mod ty;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use swc_common::Span;
use swc_ecma_ast::*;
use ty::{DeclId, FnTy, ObjTy, Param, Prop, Show, Ty};

pub use guards::Guards;

/// Something the checker reports.
#[derive(Clone, Debug)]
pub struct TypeDiag {
    pub span: Span,
    pub message: String,
}

/// A declared type.
pub(super) enum TypeDecl {
    /// Named, not resolved yet.
    Pending(String),
    Alias {
        name: String,
        ty: Ty,
    },
    Interface {
        name: String,
        obj: ObjTy,
    },
    Class(ClassInfo),
}

pub(super) struct ClassInfo {
    pub name: String,
    pub parent: Option<DeclId>,
    /// Declared fields, accessors and methods of an instance (not the
    /// parent's).
    pub instance: ObjTy,
    pub statics: ObjTy,
    /// The constructor's signature, if the class declares one.
    pub ctor: Option<Rc<FnTy>>,
}

struct Var {
    /// The declared type; `any` for a variable with no annotation.
    ty: Ty,
    konst: bool,
    /// Always the function it was declared with: a function declaration,
    /// or a constant whose initialiser is a function.
    fixed_fn: bool,
}

#[derive(Default)]
struct Scope {
    vars: HashMap<String, Var>,
    types: HashMap<String, DeclId>,
    /// Type parameters in scope: they are `any` to the checker.
    type_params: HashSet<String>,
    /// What control flow has established here about a variable or a
    /// property path (`x`, `x.y`): `Some` narrows it, `None` resets it to
    /// its declared type.
    narrowed: HashMap<String, Option<Ty>>,
    /// Variables assigned in this scope: what outer scopes established
    /// about them and their properties no longer holds.
    assigned: HashSet<String>,
    /// The scope of a function body: what was established outside about
    /// a variable that can be reassigned does not hold inside.
    function: bool,
}

struct FnCtx {
    /// The declared return type, when `return` is checked against it.
    ret: Option<Ty>,
    this: Ty,
}

pub struct Checker {
    decls: Vec<TypeDecl>,
    scopes: Vec<Scope>,
    fns: Vec<FnCtx>,
    pub diags: Vec<TypeDiag>,
    /// Where the program checks a value when it runs.
    guards: Guards,
    /// The member expression checked last is a property of a string, a
    /// number or a boolean: a method the language provides.
    built_in_member: bool,
}

/// What the checker knows about the modules of a program checked so
/// far, by the global name the linker gives each top-level binding
/// (`compiler::modules`).
#[derive(Default)]
pub struct TypeWorld {
    decls: Vec<TypeDecl>,
    values: HashMap<String, Ty>,
    types: HashMap<String, DeclId>,
}

/// What the checker found in a program.
pub struct Checked {
    /// What it reports, in source order.
    pub diags: Vec<TypeDiag>,
    /// The values to check when the program runs.
    pub guards: Guards,
}

/// Check a program. With
/// `link`, the program is a module of a linked program: what it imports
/// has the types `world` recorded for the modules checked before it, and
/// its own top-level names are recorded for the modules after it.
pub fn check_program(
    program: &Program,
    world: &mut TypeWorld,
    link: Option<&crate::compiler::modules::ModuleLink>,
) -> Checked {
    let mut checker = Checker {
        decls: std::mem::take(&mut world.decls),
        scopes: vec![Scope::default()],
        fns: Vec::new(),
        diags: Vec::new(),
        guards: Guards::new(),
        built_in_member: false,
    };
    match program {
        Program::Module(module) => {
            checker.declare_imports(module, world, link);
            let decls: Vec<&Decl> = module
                .body
                .iter()
                .filter_map(|item| match item {
                    ModuleItem::Stmt(Stmt::Decl(decl)) => Some(decl),
                    ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export)) => Some(&export.decl),
                    _ => None,
                })
                .collect();
            checker.hoist_decls(&decls);
            for item in &module.body {
                checker.check_module_item(item);
            }
        }
        Program::Script(script) => {
            checker.hoist(script.body.iter());
            for stmt in &script.body {
                checker.check_stmt(stmt);
            }
        }
    }
    // What the module declares at its top level, for those that import
    // from it.
    let prefix = link.map_or("", |l| l.prefix.as_str());
    let top = &checker.scopes[0];
    for (name, var) in &top.vars {
        world
            .values
            .insert(format!("{prefix}{name}"), var.ty.clone());
    }
    for (name, id) in &top.types {
        world.types.insert(format!("{prefix}{name}"), *id);
    }
    world.decls = checker.decls;
    let mut diags = checker.diags;
    diags.sort_by_key(|d| d.span.lo);
    Checked {
        diags,
        guards: checker.guards,
    }
}

impl Checker {
    /// Declare what the module imports: with the type recorded for it
    /// when it comes from a module of the same program, otherwise as
    /// dynamic.
    fn declare_imports(
        &mut self,
        module: &Module,
        world: &TypeWorld,
        link: Option<&crate::compiler::modules::ModuleLink>,
    ) {
        for item in &module.body {
            let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item else {
                continue;
            };
            for spec in &import.specifiers {
                let local = match spec {
                    ImportSpecifier::Named(s) => &s.local,
                    ImportSpecifier::Default(s) => &s.local,
                    ImportSpecifier::Namespace(s) => &s.local,
                };
                let name = local.sym.as_ref();
                // `import type { T }`: only a type.
                if let Some(id) = link
                    .and_then(|l| l.type_imports.get(name))
                    .and_then(|global| world.types.get(global))
                {
                    self.scope().types.insert(name.to_string(), *id);
                    continue;
                }
                // `import * as ns`: an object with the module's exports.
                if let Some(members) = link.and_then(|l| l.namespaces.get(name)) {
                    let mut obj = ObjTy::default();
                    for (export, global) in members {
                        obj.set(Prop {
                            name: export.clone(),
                            ty: world.values.get(global).cloned().unwrap_or(Ty::Any),
                            optional: false,
                        });
                    }
                    // Anything else on it is not known.
                    obj.index = Some(Ty::Any);
                    self.declare(name, Ty::Object(Rc::new(obj)), true);
                    continue;
                }
                let global = link.and_then(|l| l.imports.get(name));
                let ty = global
                    .and_then(|g| world.values.get(g))
                    .cloned()
                    .unwrap_or(Ty::Any);
                // A class is also the type of its instances.
                if let Ty::ClassValue(id) = &ty {
                    self.scope().types.insert(name.to_string(), *id);
                }
                self.declare(name, ty, true);
            }
        }
    }

    fn report(&mut self, span: Span, message: impl Into<String>) {
        self.diags.push(TypeDiag {
            span,
            message: message.into(),
        });
    }

    /// `ty` as messages print it.
    fn show(&self, ty: &Ty) -> String {
        let names = |id: DeclId| self.decl_name(id).to_string();
        Show { ty, names: &names }.to_string()
    }

    fn decl_name(&self, id: DeclId) -> &str {
        match &self.decls[id.0] {
            TypeDecl::Pending(name)
            | TypeDecl::Alias { name, .. }
            | TypeDecl::Interface { name, .. } => name,
            TypeDecl::Class(class) => &class.name,
        }
    }

    fn class(&self, id: DeclId) -> Option<&ClassInfo> {
        match &self.decls[id.0] {
            TypeDecl::Class(class) => Some(class),
            _ => None,
        }
    }

    // --- scopes ---

    fn scope(&mut self) -> &mut Scope {
        self.scopes.last_mut().expect("a scope")
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn push_function_scope(&mut self) {
        self.scopes.push(Scope {
            function: true,
            ..Scope::default()
        });
    }

    /// Leave a scope. What it assigned is no longer known to its parent,
    /// unless the scope was a function body (which runs some other time).
    fn pop_scope(&mut self) {
        let scope = self.scopes.pop().expect("a scope");
        if scope.function {
            return;
        }
        for name in scope.assigned {
            if !scope.vars.contains_key(&name) {
                self.forget(&name);
            }
        }
    }

    fn declare(&mut self, name: &str, ty: Ty, konst: bool) {
        let var = Var {
            ty,
            konst,
            fixed_fn: false,
        };
        self.scope().vars.insert(name.to_string(), var);
    }

    /// The variable `name`, just declared here, always holds the
    /// function it was declared with.
    fn fix_fn(&mut self, name: &str) {
        if let Some(var) = self.scope().vars.get_mut(name) {
            var.fixed_fn = true;
        }
    }

    fn lookup_type(&self, name: &str) -> Option<TypeName> {
        for scope in self.scopes.iter().rev() {
            if scope.type_params.contains(name) {
                return Some(TypeName::Param);
            }
            if let Some(id) = scope.types.get(name) {
                return Some(TypeName::Declared(*id));
            }
        }
        None
    }

    fn lookup_var(&self, name: &str) -> Option<&Var> {
        self.scopes.iter().rev().find_map(|s| s.vars.get(name))
    }

    /// The declared type of the variable `name`: `any` if it has no
    /// annotation or is not a variable the checker knows.
    fn declared(&self, name: &str) -> Ty {
        self.lookup_var(name).map_or(Ty::Any, |v| v.ty.clone())
    }

    /// What control flow has established about `path` (`x` or `x.y.z`).
    fn narrowed(&self, path: &str) -> Option<Ty> {
        let root = path.split('.').next().unwrap_or(path);
        let reassignable = self.lookup_var(root).is_none_or(|v| !v.konst);
        for scope in self.scopes.iter().rev() {
            if let Some(entry) = scope.narrowed.get(path) {
                return entry.clone();
            }
            if scope.assigned.contains(root) || scope.vars.contains_key(root) {
                return None;
            }
            if scope.function && reassignable {
                return None;
            }
        }
        None
    }

    /// Control flow established that `path` has the type `ty` from here.
    fn narrow(&mut self, path: &str, ty: Ty) {
        self.scope().narrowed.insert(path.to_string(), Some(ty));
    }

    /// The variable `root` was assigned: nothing established about it or
    /// its properties holds any more.
    fn forget(&mut self, root: &str) {
        let scope = self.scope();
        let prefix = format!("{root}.");
        scope
            .narrowed
            .retain(|path, _| path != root && !path.starts_with(&prefix));
        scope.assigned.insert(root.to_string());
    }
}

pub(super) enum TypeName {
    /// A type parameter: `any`.
    Param,
    Declared(DeclId),
}
