//! Control flow facts the checker needs from syntax alone: whether a
//! statement always leaves, and which variables a piece of code assigns.

use std::collections::HashSet;
use swc_ecma_ast::*;
use swc_ecma_visit::{Visit, VisitWith};

/// Whether control never continues past `stmt`: it returns, throws,
/// breaks or continues on every path.
pub(super) fn exits(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Block(block) => block.stmts.last().is_some_and(exits),
        Stmt::If(i) => exits(&i.cons) && i.alt.as_deref().is_some_and(exits),
        _ => false,
    }
}

/// The variables assigned anywhere in `node`: the roots of the targets of
/// its assignments and updates (`x` for `x = 1`, `x.y = 1` and `x++`).
pub(super) fn assigned_in<N: VisitWith<Assigned>>(node: &N) -> HashSet<String> {
    let mut visitor = Assigned::default();
    node.visit_with(&mut visitor);
    visitor.names
}

/// The variables and property paths `node` assigns directly (`x = 1`,
/// `this.a.b = 1`, `x++`), each with an expression that reads it.
pub(super) fn assigned_places<N: VisitWith<Assigned>>(node: &N) -> Vec<(String, Expr)> {
    let mut visitor = Assigned::default();
    node.visit_with(&mut visitor);
    visitor.places
}

#[derive(Default)]
pub(super) struct Assigned {
    names: HashSet<String>,
    places: Vec<(String, Expr)>,
}

impl Assigned {
    /// Record `expr` as a place that is assigned, if it is a variable or
    /// a property path.
    fn place(&mut self, expr: Expr) {
        if let Some(path) = super::narrow::path_of(&expr)
            && !self.places.iter().any(|(p, _)| *p == path)
        {
            self.places.push((path, expr));
        }
    }

    fn root(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(id) => {
                self.names.insert(id.sym.to_string());
            }
            Expr::Member(m) => self.root(&m.obj),
            Expr::Paren(p) => self.root(&p.expr),
            Expr::TsNonNull(e) => self.root(&e.expr),
            Expr::TsAs(e) => self.root(&e.expr),
            _ => {}
        }
    }

    fn pat(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(id) => {
                self.names.insert(id.id.sym.to_string());
            }
            Pat::Array(arr) => arr.elems.iter().flatten().for_each(|p| self.pat(p)),
            Pat::Object(obj) => {
                for prop in &obj.props {
                    match prop {
                        ObjectPatProp::KeyValue(kv) => self.pat(&kv.value),
                        ObjectPatProp::Assign(a) => {
                            self.names.insert(a.key.id.sym.to_string());
                        }
                        ObjectPatProp::Rest(rest) => self.pat(&rest.arg),
                    }
                }
            }
            Pat::Rest(rest) => self.pat(&rest.arg),
            Pat::Assign(assign) => self.pat(&assign.left),
            Pat::Expr(expr) => self.root(expr),
            Pat::Invalid(_) => {}
        }
    }
}

impl Visit for Assigned {
    fn visit_assign_expr(&mut self, assign: &AssignExpr) {
        match &assign.left {
            AssignTarget::Simple(simple) => match simple {
                SimpleAssignTarget::Ident(id) => {
                    self.names.insert(id.id.sym.to_string());
                    self.place(Expr::Ident(id.id.clone()));
                }
                SimpleAssignTarget::Member(m) => {
                    self.root(&m.obj);
                    self.place(Expr::Member(m.clone()));
                }
                SimpleAssignTarget::Paren(p) => self.root(&p.expr),
                _ => {}
            },
            AssignTarget::Pat(pat) => match pat {
                AssignTargetPat::Array(arr) => self.pat(&Pat::Array(arr.clone())),
                AssignTargetPat::Object(obj) => self.pat(&Pat::Object(obj.clone())),
                AssignTargetPat::Invalid(_) => {}
            },
        }
        assign.visit_children_with(self);
    }

    fn visit_update_expr(&mut self, update: &UpdateExpr) {
        self.root(&update.arg);
        self.place((*update.arg).clone());
        update.visit_children_with(self);
    }
}
