//! Lexical scope resolution.
//!
//! Before code generation, this pass works out where every variable lives,
//! so closures share variables with the code that created them, as in
//! JavaScript:
//!
//! - A binding referenced only by the function that declares it is an
//!   ordinary local (`Let` / `Load` / `Store` by name).
//! - A binding declared directly at the top level is a global.
//! - A binding referenced from a nested function is *captured*. It lives in
//!   an environment object belonging to its scope (a function body, a block,
//!   or one iteration of a loop), and every access goes through that object,
//!   so the declaring code and all closures see the same variable.
//!
//! Environment objects form a chain: each one's `%parent` property is the
//! next enclosing environment. At any point in the generated code, the local
//! `%scope` holds the innermost environment. An access to a captured binding
//! walks `hops` parent links from there. Identifiers are matched to their
//! resolution by source span, which is unique per occurrence.

use std::collections::{HashMap, HashSet};
use swc_common::Span;
use swc_ecma_ast::*;
use swc_ecma_visit::{Visit, VisitWith};

/// Name of the local holding the innermost environment object.
pub const SCOPE_LOCAL: &str = "%scope";
/// Property linking an environment object to the enclosing one.
pub const PARENT_PROP: &str = "%parent";

/// A source span as a map key.
pub type SpanKey = (u32, u32);

pub fn key(span: Span) -> SpanKey {
    (span.lo.0, span.hi.0)
}

/// How an identifier occurrence is compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarRef {
    /// By name: a local of the current function, or a global.
    Plain,
    /// A property of the environment `hops` parent links above `%scope`.
    Env { hops: u32 },
}

/// The result of scope resolution for one program.
#[derive(Debug, Default)]
pub struct ScopeInfo {
    refs: HashMap<SpanKey, VarRef>,
    /// Scopes that need an environment object, by the span of the node that
    /// opens them (a function, block, loop or catch clause), with the names
    /// of their captured bindings.
    env_scopes: HashMap<SpanKey, Vec<String>>,
    /// Functions (by span) created as closures: they reach bindings outside
    /// themselves through the environment chain.
    closures: HashSet<SpanKey>,
    /// Functions (by span) whose code uses `%scope`.
    uses_env: HashSet<SpanKey>,
}

impl ScopeInfo {
    /// How the identifier at `span` is compiled.
    pub fn var(&self, span: Span) -> VarRef {
        self.refs.get(&key(span)).copied().unwrap_or(VarRef::Plain)
    }

    /// Captured bindings of the scope opened by the node at `span`, if it
    /// needs an environment object.
    pub fn env_scope(&self, span: Span) -> Option<&[String]> {
        self.env_scopes.get(&key(span)).map(Vec::as_slice)
    }

    pub fn is_closure(&self, span: Span) -> bool {
        self.closures.contains(&key(span))
    }

    pub fn uses_env(&self, span: Span) -> bool {
        self.uses_env.contains(&key(span))
    }

    /// Whether top-level code (outside any function) uses `%scope`.
    pub fn top_level_uses_env(&self) -> bool {
        self.uses_env.contains(&(0, 0))
    }

    pub fn analyze_module(module: &Module) -> Self {
        let mut r = Resolver::new();
        r.hoist_module(&module.body);
        module.visit_children_with(&mut r);
        r.finish()
    }

    pub fn analyze_script(script: &Script) -> Self {
        let mut r = Resolver::new();
        r.hoist_stmts(&script.body);
        script.visit_children_with(&mut r);
        r.finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Global,
    Function,
    Block,
}

struct Scope {
    kind: Kind,
    parent: Option<usize>,
    /// Span key of the opening node (`(0, 0)` for the global scope).
    key: SpanKey,
    /// The function this scope belongs to (`None` at the top level).
    func: Option<usize>,
    bindings: HashMap<String, usize>,
}

struct Binding {
    scope: usize,
    name: String,
    captured: bool,
}

struct Occurrence {
    span: SpanKey,
    scope: usize,
    name: String,
}

struct Resolver {
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
    occurrences: Vec<Occurrence>,
    current: usize,
    /// Function index -> span key of the function node.
    functions: Vec<SpanKey>,
    /// Function index -> enclosing function index.
    function_parent: Vec<Option<usize>>,
}

impl Resolver {
    fn new() -> Self {
        Self {
            scopes: vec![Scope {
                kind: Kind::Global,
                parent: None,
                key: (0, 0),
                func: None,
                bindings: HashMap::new(),
            }],
            bindings: Vec::new(),
            occurrences: Vec::new(),
            current: 0,
            functions: Vec::new(),
            function_parent: Vec::new(),
        }
    }

    fn push_scope(&mut self, kind: Kind, span: Span) -> usize {
        let func = if kind == Kind::Function {
            let idx = self.functions.len();
            self.functions.push(key(span));
            self.function_parent.push(self.scopes[self.current].func);
            Some(idx)
        } else {
            self.scopes[self.current].func
        };
        self.scopes.push(Scope {
            kind,
            parent: Some(self.current),
            key: key(span),
            func,
            bindings: HashMap::new(),
        });
        self.current = self.scopes.len() - 1;
        self.current
    }

    fn pop_scope(&mut self, saved: usize) {
        self.current = saved;
    }

    /// Declare `name` in `scope` (once), without recording an occurrence.
    fn declare_in(&mut self, scope: usize, name: &str) {
        if !self.scopes[scope].bindings.contains_key(name) {
            let id = self.bindings.len();
            self.bindings.push(Binding {
                scope,
                name: name.to_string(),
                captured: false,
            });
            self.scopes[scope].bindings.insert(name.to_string(), id);
        }
    }

    /// The nearest scope that `var` declarations go to.
    fn var_scope(&self) -> usize {
        let mut s = self.current;
        loop {
            let scope = &self.scopes[s];
            if matches!(scope.kind, Kind::Function | Kind::Global) {
                return s;
            }
            s = scope.parent.unwrap();
        }
    }

    fn occurrence(&mut self, ident: &Ident) {
        self.occurrences.push(Occurrence {
            span: key(ident.span),
            scope: self.current,
            name: ident.sym.to_string(),
        });
    }

    /// Declarations of a block, hoisted to its start: functions, classes,
    /// `let` / `const`, and `var` (to the function scope).
    fn hoist_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.hoist_stmt(stmt);
        }
    }

    fn hoist_module(&mut self, items: &[ModuleItem]) {
        for item in items {
            match item {
                ModuleItem::Stmt(s) => self.hoist_stmt(s),
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(e)) => self.hoist_decl(&e.decl),
                ModuleItem::ModuleDecl(ModuleDecl::Import(import)) => {
                    for spec in &import.specifiers {
                        let local = match spec {
                            ImportSpecifier::Named(n) => &n.local,
                            ImportSpecifier::Default(d) => &d.local,
                            ImportSpecifier::Namespace(n) => &n.local,
                        };
                        self.declare_in(self.current, local.sym.as_ref());
                    }
                }
                _ => {}
            }
        }
    }

    fn hoist_stmt(&mut self, stmt: &Stmt) {
        if let Stmt::Decl(d) = stmt {
            self.hoist_decl(d);
        }
    }

    fn hoist_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Fn(f) => self.declare_in(self.current, f.ident.sym.as_ref()),
            Decl::Class(c) => self.declare_in(self.current, c.ident.sym.as_ref()),
            Decl::Var(v) => {
                let scope = if v.kind == VarDeclKind::Var {
                    self.var_scope()
                } else {
                    self.current
                };
                for d in &v.decls {
                    for name in pattern_names(&d.name) {
                        self.declare_in(scope, &name);
                    }
                }
            }
            _ => {}
        }
    }

    /// Record the binding identifiers of a declaration pattern.
    fn declaration(&mut self, pat: &Pat, var: bool) {
        let scope = if var { self.var_scope() } else { self.current };
        for ident in pattern_idents(pat) {
            self.declare_in(scope, ident.sym.as_ref());
            self.occurrence(&ident);
        }
        // Default values and computed keys are ordinary expressions.
        visit_pattern_exprs(pat, self);
    }

    /// The scope that binds `name`, seen from `scope`.
    fn lookup(&self, mut scope: usize, name: &str) -> Option<usize> {
        loop {
            if let Some(&b) = self.scopes[scope].bindings.get(name) {
                return Some(b);
            }
            scope = self.scopes[scope].parent?;
        }
    }

    fn finish(mut self) -> ScopeInfo {
        // 1. Mark bindings referenced from another function as captured.
        //    Top-level bindings are globals and never need an environment.
        let resolved: Vec<(usize, Option<usize>)> = self
            .occurrences
            .iter()
            .enumerate()
            .map(|(i, o)| (i, self.lookup(o.scope, &o.name)))
            .collect();
        for &(i, b) in &resolved {
            if let Some(b) = b {
                let binding_scope = self.bindings[b].scope;
                if self.scopes[binding_scope].kind == Kind::Global {
                    continue;
                }
                let occ_func = self.scopes[self.occurrences[i].scope].func;
                if occ_func != self.scopes[binding_scope].func {
                    self.bindings[b].captured = true;
                }
            }
        }

        // 2. Scopes with a captured binding need an environment object.
        let mut env_scope = vec![false; self.scopes.len()];
        let mut info = ScopeInfo::default();
        for b in &self.bindings {
            if b.captured {
                env_scope[b.scope] = true;
                info.env_scopes
                    .entry(self.scopes[b.scope].key)
                    .or_default()
                    .push(b.name.clone());
            }
        }
        for names in info.env_scopes.values_mut() {
            names.sort();
        }

        // 3. Resolve every occurrence, and find which functions are
        //    closures (reach through the chain past their own scopes).
        let mut closure = vec![false; self.functions.len()];
        let mut uses_env = vec![false; self.functions.len()];
        let mut top_level_uses_env = false;
        for &(i, b) in &resolved {
            let Some(b) = b else { continue };
            if !self.bindings[b].captured {
                continue;
            }
            let target = self.bindings[b].scope;
            let occ = &self.occurrences[i];
            // Count environment scopes between the occurrence and the
            // binding's scope, marking every function crossed on the way.
            let mut hops = 0;
            let mut s = occ.scope;
            while s != target {
                if env_scope[s] {
                    hops += 1;
                }
                if self.scopes[s].kind == Kind::Function {
                    closure[self.scopes[s].func.unwrap()] = true;
                }
                s = self.scopes[s].parent.unwrap();
            }
            info.refs.insert(occ.span, VarRef::Env { hops });
            match self.scopes[occ.scope].func {
                Some(f) => uses_env[f] = true,
                None => top_level_uses_env = true,
            }
        }

        // 4. A function uses `%scope` if it has an environment scope of its
        //    own, reads through the chain, or creates a closure.
        for (s, scope) in self.scopes.iter().enumerate() {
            if env_scope[s] {
                match scope.func {
                    Some(f) => uses_env[f] = true,
                    None => top_level_uses_env = true,
                }
            }
        }
        for (f, &is_closure) in closure.iter().enumerate() {
            if is_closure {
                uses_env[f] = true;
                match self.function_parent[f] {
                    Some(p) => uses_env[p] = true,
                    None => top_level_uses_env = true,
                }
            }
        }

        for (f, span) in self.functions.iter().enumerate() {
            if closure[f] {
                info.closures.insert(*span);
            }
            if uses_env[f] {
                info.uses_env.insert(*span);
            }
        }
        if top_level_uses_env {
            info.uses_env.insert((0, 0));
        }
        info
    }

    fn function_like(&mut self, span: Span, params: &[&Pat], body: Option<&BlockStmt>) {
        let saved = self.current;
        self.push_scope(Kind::Function, span);
        for p in params {
            self.declaration(p, false);
        }
        if let Some(body) = body {
            // The body block is the function scope itself.
            self.hoist_stmts(&body.stmts);
            for s in &body.stmts {
                s.visit_with(self);
            }
        }
        self.pop_scope(saved);
    }
}

impl Visit for Resolver {
    fn visit_fn_decl(&mut self, f: &FnDecl) {
        self.declare_in(self.current, f.ident.sym.as_ref());
        self.occurrence(&f.ident);
        f.function.visit_with(self);
    }

    fn visit_fn_expr(&mut self, f: &FnExpr) {
        // The code generator does not bind a function expression's own
        // name, so it is not declared here either: references to it resolve
        // outward, as they always have.
        f.function.visit_with(self);
    }

    fn visit_function(&mut self, f: &Function) {
        let params: Vec<&Pat> = f.params.iter().map(|p| &p.pat).collect();
        self.function_like(f.span, &params, f.body.as_ref());
    }

    fn visit_arrow_expr(&mut self, a: &ArrowExpr) {
        let saved = self.current;
        self.push_scope(Kind::Function, a.span);
        for p in &a.params {
            self.declaration(p, false);
        }
        match &*a.body {
            BlockStmtOrExpr::BlockStmt(body) => {
                self.hoist_stmts(&body.stmts);
                for s in &body.stmts {
                    s.visit_with(self);
                }
            }
            BlockStmtOrExpr::Expr(e) => e.visit_with(self),
        }
        self.pop_scope(saved);
    }

    fn visit_constructor(&mut self, c: &Constructor) {
        let params: Vec<&Pat> = c
            .params
            .iter()
            .filter_map(|p| match p {
                ParamOrTsParamProp::Param(p) => Some(&p.pat),
                ParamOrTsParamProp::TsParamProp(_) => None,
            })
            .collect();
        self.function_like(c.span, &params, c.body.as_ref());
    }

    fn visit_class_decl(&mut self, c: &ClassDecl) {
        self.declare_in(self.current, c.ident.sym.as_ref());
        self.occurrence(&c.ident);
        c.class.visit_with(self);
    }

    fn visit_block_stmt(&mut self, b: &BlockStmt) {
        let saved = self.current;
        self.push_scope(Kind::Block, b.span);
        self.hoist_stmts(&b.stmts);
        for s in &b.stmts {
            s.visit_with(self);
        }
        self.pop_scope(saved);
    }

    fn visit_for_stmt(&mut self, f: &ForStmt) {
        let saved = self.current;
        self.push_scope(Kind::Block, f.span);
        f.init.visit_with(self);
        f.test.visit_with(self);
        f.update.visit_with(self);
        f.body.visit_with(self);
        self.pop_scope(saved);
    }

    fn visit_for_of_stmt(&mut self, f: &ForOfStmt) {
        f.right.visit_with(self);
        let saved = self.current;
        self.push_scope(Kind::Block, f.span);
        f.left.visit_with(self);
        f.body.visit_with(self);
        self.pop_scope(saved);
    }

    fn visit_for_in_stmt(&mut self, f: &ForInStmt) {
        f.right.visit_with(self);
        let saved = self.current;
        self.push_scope(Kind::Block, f.span);
        f.left.visit_with(self);
        f.body.visit_with(self);
        self.pop_scope(saved);
    }

    fn visit_catch_clause(&mut self, c: &CatchClause) {
        let saved = self.current;
        self.push_scope(Kind::Block, c.span);
        if let Some(p) = &c.param {
            self.declaration(p, false);
        }
        self.hoist_stmts(&c.body.stmts);
        for s in &c.body.stmts {
            s.visit_with(self);
        }
        self.pop_scope(saved);
    }

    fn visit_var_decl(&mut self, v: &VarDecl) {
        let var = v.kind == VarDeclKind::Var;
        for d in &v.decls {
            self.declaration(&d.name, var);
            d.init.visit_with(self);
        }
    }

    fn visit_expr(&mut self, e: &Expr) {
        match e {
            Expr::Ident(ident) => self.occurrence(ident),
            _ => e.visit_children_with(self),
        }
    }

    fn visit_simple_assign_target(&mut self, t: &SimpleAssignTarget) {
        match t {
            SimpleAssignTarget::Ident(b) => self.occurrence(&b.id),
            _ => t.visit_children_with(self),
        }
    }

    fn visit_prop(&mut self, p: &Prop) {
        match p {
            Prop::Shorthand(ident) => self.occurrence(ident),
            _ => p.visit_children_with(self),
        }
    }

    // Names that are not variable references.
    fn visit_member_prop(&mut self, p: &MemberProp) {
        if let MemberProp::Computed(c) = p {
            c.visit_with(self);
        }
    }

    fn visit_super_prop(&mut self, p: &SuperProp) {
        if let SuperProp::Computed(c) = p {
            c.visit_with(self);
        }
    }

    fn visit_prop_name(&mut self, p: &PropName) {
        if let PropName::Computed(c) = p {
            c.visit_with(self);
        }
    }

    fn visit_labeled_stmt(&mut self, l: &LabeledStmt) {
        l.body.visit_with(self);
    }

    fn visit_break_stmt(&mut self, _: &BreakStmt) {}

    fn visit_continue_stmt(&mut self, _: &ContinueStmt) {}

    // Types never refer to runtime variables.
    fn visit_ts_type(&mut self, _: &TsType) {}

    fn visit_ts_type_ann(&mut self, _: &TsTypeAnn) {}

    fn visit_ts_interface_decl(&mut self, _: &TsInterfaceDecl) {}

    fn visit_ts_type_alias_decl(&mut self, _: &TsTypeAliasDecl) {}
}

/// Names bound by a declaration pattern.
fn pattern_names(pat: &Pat) -> Vec<String> {
    pattern_idents(pat)
        .into_iter()
        .map(|i| i.sym.to_string())
        .collect()
}

/// Binding identifiers of a declaration pattern.
fn pattern_idents(pat: &Pat) -> Vec<Ident> {
    let mut out = Vec::new();
    collect_idents(pat, &mut out);
    out
}

fn collect_idents(pat: &Pat, out: &mut Vec<Ident>) {
    match pat {
        Pat::Ident(b) => out.push(b.id.clone()),
        Pat::Array(a) => {
            for p in a.elems.iter().flatten() {
                collect_idents(p, out);
            }
        }
        Pat::Object(o) => {
            for prop in &o.props {
                match prop {
                    ObjectPatProp::KeyValue(kv) => collect_idents(&kv.value, out),
                    ObjectPatProp::Assign(a) => out.push(a.key.id.clone()),
                    ObjectPatProp::Rest(r) => collect_idents(&r.arg, out),
                }
            }
        }
        Pat::Assign(a) => collect_idents(&a.left, out),
        Pat::Rest(r) => collect_idents(&r.arg, out),
        Pat::Expr(_) | Pat::Invalid(_) => {}
    }
}

/// Visit the expressions inside a declaration pattern (defaults, computed
/// keys) as references.
fn visit_pattern_exprs(pat: &Pat, r: &mut Resolver) {
    match pat {
        Pat::Ident(_) | Pat::Invalid(_) => {}
        Pat::Array(a) => {
            for p in a.elems.iter().flatten() {
                visit_pattern_exprs(p, r);
            }
        }
        Pat::Object(o) => {
            for prop in &o.props {
                match prop {
                    ObjectPatProp::KeyValue(kv) => {
                        kv.key.visit_with(r);
                        visit_pattern_exprs(&kv.value, r);
                    }
                    ObjectPatProp::Assign(a) => a.value.visit_with(r),
                    ObjectPatProp::Rest(rest) => visit_pattern_exprs(&rest.arg, r),
                }
            }
        }
        Pat::Assign(a) => {
            visit_pattern_exprs(&a.left, r);
            a.right.visit_with(r);
        }
        Pat::Rest(rest) => visit_pattern_exprs(&rest.arg, r),
        Pat::Expr(e) => e.visit_with(r),
    }
}
