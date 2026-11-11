//! Statements.

use super::resolve::pat_name;
use super::*;
use swc_common::Spanned;

impl Checker {
    pub(super) fn check_module_item(&mut self, item: &ModuleItem) {
        match item {
            ModuleItem::Stmt(stmt) => self.check_stmt(stmt),
            ModuleItem::ModuleDecl(decl) => match decl {
                ModuleDecl::ExportDecl(export) => self.check_decl(&export.decl),
                ModuleDecl::ExportDefaultExpr(export) => {
                    self.expr(&export.expr);
                }
                ModuleDecl::ExportDefaultDecl(export) => match &export.decl {
                    DefaultDecl::Fn(f) => self.check_function(&f.function, Ty::Any),
                    DefaultDecl::Class(c) => self.check_class(None, &c.class),
                    DefaultDecl::TsInterfaceDecl(_) => {}
                },
                // Imports were declared before anything else.
                _ => {}
            },
        }
    }

    /// Check a block's statements in a scope of their own.
    fn check_block(&mut self, stmts: &[Stmt]) {
        self.push_scope();
        self.hoist(stmts.iter());
        for stmt in stmts {
            self.check_stmt(stmt);
        }
        self.pop_scope();
    }

    /// Check `stmt` as a branch taken when `facts` hold.
    fn check_branch(&mut self, stmt: &Stmt, facts: &[(String, Ty)]) {
        self.push_scope();
        self.apply(facts);
        self.check_in_scope(stmt);
        self.pop_scope();
    }

    /// `if`: each branch is checked with what the condition establishes
    /// for it. Afterwards, a variable a branch assigned is whatever the
    /// branches that reach the end left it as; and if one branch always
    /// leaves, the rest runs only when the other one was taken.
    fn check_if(&mut self, i: &IfStmt) {
        self.expr(&i.test);
        let facts = self.facts(&i.test);
        // The variables and properties a branch assigns, and what each is
        // declared as. Only those with a declared type have anything to
        // track.
        let mut assigned = flow::assigned_places(&i.cons);
        if let Some(alt) = &i.alt {
            for place in flow::assigned_places(alt) {
                if !assigned.iter().any(|(path, _)| *path == place.0) {
                    assigned.push(place);
                }
            }
        }
        assigned.retain(|(path, place)| match place {
            Expr::Ident(_) => !self.declared(path).is_dynamic(),
            _ => true,
        });

        let then_exits = flow::exits(&i.cons);
        let else_exits = i.alt.as_deref().is_some_and(flow::exits);
        let mut ends: Vec<Vec<Ty>> = vec![Vec::new(); assigned.len()];
        let mut end_of_branch = |checker: &mut Self, leaves: bool| {
            if !leaves {
                for ((_, place), types) in assigned.iter().zip(ends.iter_mut()) {
                    types.push(checker.expr(place));
                }
            }
        };

        self.push_scope();
        self.apply(&facts.then);
        self.check_in_scope(&i.cons);
        end_of_branch(self, then_exits);
        self.pop_scope();

        self.push_scope();
        self.apply(&facts.otherwise);
        if let Some(alt) = &i.alt {
            self.check_in_scope(alt);
        }
        end_of_branch(self, else_exits);
        self.pop_scope();

        if then_exits && !else_exits {
            self.apply(&facts.otherwise);
        } else if else_exits && !then_exits {
            self.apply(&facts.then);
        }
        for ((path, place), types) in assigned.iter().zip(ends) {
            let merged = Ty::union(types);
            if !merged.is_dynamic() && merged != Ty::Never {
                if matches!(place, Expr::Ident(_)) {
                    self.forget(path);
                }
                self.narrow(path, merged);
            }
        }
    }

    /// Check `stmt` in the current scope (a block's statements without a
    /// scope of their own).
    fn check_in_scope(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(block) => {
                self.hoist(block.stmts.iter());
                for stmt in &block.stmts {
                    self.check_stmt(stmt);
                }
            }
            other => self.check_stmt(other),
        }
    }

    /// Before a loop: what was established about the variables the loop
    /// assigns does not hold on its later iterations.
    fn forget_assigned(&mut self, assigned: std::collections::HashSet<String>) {
        for name in assigned {
            self.forget(&name);
        }
    }

    pub(super) fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Block(block) => self.check_block(&block.stmts),
            Stmt::Decl(decl) => self.check_decl(decl),
            Stmt::Expr(e) => {
                self.expr(&e.expr);
            }
            Stmt::If(i) => self.check_if(i),
            Stmt::While(w) => {
                self.forget_assigned(flow::assigned_in(w));
                self.expr(&w.test);
                let facts = self.facts(&w.test);
                self.check_branch(&w.body, &facts.then);
            }
            Stmt::DoWhile(w) => {
                self.forget_assigned(flow::assigned_in(w));
                self.check_branch(&w.body, &[]);
                self.expr(&w.test);
            }
            Stmt::For(f) => {
                self.push_scope();
                match &f.init {
                    Some(VarDeclOrExpr::VarDecl(decl)) => self.check_var_decl(decl),
                    Some(VarDeclOrExpr::Expr(e)) => {
                        self.expr(e);
                    }
                    None => {}
                }
                self.forget_assigned(flow::assigned_in(f));
                let facts = match &f.test {
                    Some(test) => {
                        self.expr(test);
                        self.facts(test)
                    }
                    None => narrow::Facts::default(),
                };
                self.check_branch(&f.body, &facts.then);
                if let Some(update) = &f.update {
                    self.expr(update);
                }
                self.pop_scope();
            }
            Stmt::ForIn(f) => {
                self.forget_assigned(flow::assigned_in(f));
                self.expr(&f.right);
                self.push_scope();
                self.declare_for_head(&f.left);
                self.check_branch(&f.body, &[]);
                self.pop_scope();
            }
            Stmt::ForOf(f) => {
                self.forget_assigned(flow::assigned_in(f));
                self.expr(&f.right);
                self.push_scope();
                self.declare_for_head(&f.left);
                self.check_branch(&f.body, &[]);
                self.pop_scope();
            }
            Stmt::Return(r) => self.check_return(r),
            Stmt::Throw(t) => {
                self.expr(&t.arg);
            }
            Stmt::Try(t) => {
                self.check_block(&t.block.stmts);
                if let Some(handler) = &t.handler {
                    self.push_scope();
                    if let Some(param) = &handler.param {
                        self.declare_pattern(param, false);
                    }
                    self.hoist(handler.body.stmts.iter());
                    for stmt in &handler.body.stmts {
                        self.check_stmt(stmt);
                    }
                    self.pop_scope();
                }
                if let Some(finalizer) = &t.finalizer {
                    self.check_block(&finalizer.stmts);
                }
            }
            Stmt::Switch(s) => {
                self.expr(&s.discriminant);
                self.push_scope();
                for case in &s.cases {
                    if let Some(test) = &case.test {
                        self.expr(test);
                    }
                }
                let stmts = || s.cases.iter().flat_map(|c| c.cons.iter());
                self.hoist(stmts());
                for stmt in stmts() {
                    self.check_stmt(stmt);
                }
                self.pop_scope();
            }
            Stmt::Labeled(l) => self.check_stmt(&l.body),
            Stmt::With(w) => {
                self.expr(&w.obj);
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Empty(_) | Stmt::Debugger(_) => {}
        }
    }

    /// The variable of `for (x of ...)` / `for (x in ...)`.
    fn declare_for_head(&mut self, head: &ForHead) {
        match head {
            ForHead::VarDecl(decl) => {
                let konst = decl.kind == VarDeclKind::Const;
                for d in &decl.decls {
                    match &d.name {
                        Pat::Ident(id) if id.type_ann.is_some() => {
                            let ty = self.annotation(id.type_ann.as_deref());
                            self.declare(id.id.sym.as_ref(), ty, konst);
                        }
                        other => self.declare_pattern(other, konst),
                    }
                }
            }
            ForHead::Pat(pat) => {
                if let Some(name) = pat_name(pat) {
                    self.forget(name);
                }
            }
            ForHead::UsingDecl(_) => {}
        }
    }

    fn check_return(&mut self, r: &ReturnStmt) {
        let found = r.arg.as_ref().map(|arg| (self.expr(arg), arg.span()));
        let Some(declared) = self.fns.last().and_then(|ctx| ctx.ret.clone()) else {
            return;
        };
        match found {
            Some((found, span)) => self.check_assignable(&found, &declared, span),
            None => {
                if !self.assignable(&Ty::Undefined, &declared) {
                    let message = format!(
                        "A function whose declared type is '{}' must return a value.",
                        self.show(&declared)
                    );
                    self.report(r.span, message);
                }
            }
        }
    }

    pub(super) fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Var(var) => self.check_var_decl(var),
            Decl::Fn(f) => self.check_function(&f.function, Ty::Any),
            Decl::Class(c) => {
                let id = match self.lookup_var(c.ident.sym.as_ref()).map(|v| &v.ty) {
                    Some(Ty::ClassValue(id)) => Some(*id),
                    _ => None,
                };
                self.check_class(id, &c.class);
            }
            // Types were collected with the block; they have no code.
            Decl::TsInterface(_)
            | Decl::TsTypeAlias(_)
            | Decl::TsEnum(_)
            | Decl::TsModule(_)
            | Decl::Using(_) => {}
        }
    }

    pub(super) fn check_var_decl(&mut self, var: &VarDecl) {
        let konst = var.kind == VarDeclKind::Const;
        for d in &var.decls {
            self.check_declarator(d, konst);
            if let (true, Pat::Ident(id), Some(init)) = (konst, &d.name, &d.init)
                && is_function(init)
            {
                self.fix_fn(id.id.sym.as_ref());
            }
            // `var` exists, as `undefined`, before its initialiser runs.
            if let (Pat::Ident(id), Some(_)) = (&d.name, &d.init)
                && var.kind != VarDeclKind::Var
            {
                self.make_definite(id.id.sym.as_ref());
            }
        }
    }

    fn check_declarator(&mut self, d: &VarDeclarator, konst: bool) {
        match &d.name {
            Pat::Ident(id) => {
                let name = id.id.sym.as_ref();
                match &id.type_ann {
                    Some(ann) => {
                        let declared = self.resolve(&ann.type_ann);
                        let found = d.init.as_ref().map(|init| {
                            let found = self.expr(init);
                            self.check_assignable(&found, &declared, init.span());
                            found
                        });
                        self.declare(name, declared.clone(), konst);
                        if let Some(found) = found {
                            self.narrow_by_assignment(name, &found, &declared);
                        }
                    }
                    None => {
                        let found = d.init.as_ref().map(|init| (self.expr(init), init));
                        // No annotation: the variable is dynamic. A
                        // constant bound to a function, a class or a
                        // primitive literal is known for what it is.
                        let ty = match found {
                            Some((ty, init)) if konst && is_fixed(init) => ty.widened(),
                            _ => Ty::Any,
                        };
                        self.declare(name, ty, konst);
                    }
                }
            }
            pattern => {
                if let Some(init) = &d.init {
                    let found = self.expr(init);
                    let ann = match pattern {
                        Pat::Array(p) => p.type_ann.as_deref(),
                        Pat::Object(p) => p.type_ann.as_deref(),
                        _ => None,
                    };
                    if let Some(ann) = ann {
                        let declared = self.resolve(&ann.type_ann);
                        self.check_assignable(&found, &declared, init.span());
                    }
                }
                self.declare_pattern(pattern, konst);
            }
        }
    }

    /// After `name = <found>` on a variable declared as a union: from
    /// here it is the member that was assigned.
    pub(super) fn narrow_by_assignment(&mut self, name: &str, found: &Ty, declared: &Ty) {
        let is_union = matches!(self.expand(declared), Ty::Union(_));
        if is_union && !found.is_dynamic() && self.assignable(found, declared) {
            self.narrow(name, found.clone());
        }
    }
}

/// Whether a constant initialised with `init` has a type worth knowing
/// without an annotation: a function, a class or a primitive literal.
/// Whether `init` is a function written here.
fn is_function(init: &Expr) -> bool {
    match init {
        Expr::Fn(_) | Expr::Arrow(_) => true,
        Expr::Paren(p) => is_function(&p.expr),
        _ => false,
    }
}

fn is_fixed(init: &Expr) -> bool {
    match init {
        Expr::Fn(_) | Expr::Arrow(_) | Expr::Class(_) => true,
        Expr::Lit(Lit::Str(_) | Lit::Num(_) | Lit::Bool(_)) => true,
        Expr::Paren(p) => is_fixed(&p.expr),
        _ => false,
    }
}
