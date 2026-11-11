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
mod narrow;
mod resolve;
mod stmt;
pub mod ty;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use swc_common::Span;
use swc_ecma_ast::*;
use ty::{DeclId, FnTy, ObjTy, Param, Prop, Show, Ty};

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
}

/// Check a program and return what it found, in source order.
pub fn check_program(program: &Program) -> Vec<TypeDiag> {
    let mut checker = Checker {
        decls: Vec::new(),
        scopes: vec![Scope::default()],
        fns: Vec::new(),
        diags: Vec::new(),
    };
    match program {
        Program::Module(module) => {
            let stmts: Vec<&Stmt> = module.body.iter().filter_map(module_item_stmt).collect();
            checker.hoist(stmts.iter().copied());
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
    let mut diags = checker.diags;
    diags.sort_by_key(|d| d.span.lo);
    diags
}

/// The statement of a module item, when it is one.
fn module_item_stmt(item: &ModuleItem) -> Option<&Stmt> {
    match item {
        ModuleItem::Stmt(stmt) => Some(stmt),
        ModuleItem::ModuleDecl(_) => None,
    }
}

impl Checker {
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
        self.scope()
            .vars
            .insert(name.to_string(), Var { ty, konst });
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
