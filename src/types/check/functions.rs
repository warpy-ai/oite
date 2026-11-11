//! Function bodies: parameters are declared with their types, and
//! `return` is checked against the declared return type.

use super::resolve::pat_name;
use super::*;
use swc_common::Spanned;

impl Checker {
    /// Report that `src` is not assignable to `dst`, if it is not.
    pub(super) fn check_assignable(&mut self, src: &Ty, dst: &Ty, span: Span) {
        if self.assignable(src, dst) {
            return;
        }
        let mut message = format!(
            "Type '{}' is not assignable to type '{}'.",
            self.show_source(src, dst),
            self.show(dst)
        );
        if let Some(detail) = self.mismatch(src, dst) {
            message.push(' ');
            message.push_str(&detail);
        }
        self.report(span, message);
    }

    /// For two object types that are not assignable: the first property
    /// that is the reason.
    fn mismatch(&self, src: &Ty, dst: &Ty) -> Option<String> {
        let (src, dst) = (self.expand(src), self.expand(dst));
        if !matches!(dst, Ty::Object(_) | Ty::Named(_)) || self.class_of(&dst).is_some() {
            return None;
        }
        let have = self.members(&src)?;
        let want = self.members(&dst)?;
        for prop in &want.obj.props {
            match have.obj.get(&prop.name) {
                Some(found) if found.optional && !prop.optional => {
                    return Some(format!(
                        "Property '{}' is optional in the source but required in the target.",
                        prop.name
                    ));
                }
                Some(found) if !self.assignable(&found.ty, &prop.ty) => {
                    return Some(format!(
                        "Property '{}': type '{}' is not assignable to type '{}'.",
                        prop.name,
                        self.show_source(&found.ty, &prop.ty),
                        self.show(&prop.ty)
                    ));
                }
                None if !prop.optional && have.closed && have.obj.index.is_none() => {
                    return Some(format!("Property '{}' is missing.", prop.name));
                }
                _ => {}
            }
        }
        None
    }

    /// The class `ty` is an instance of, if it is a class type.
    fn class_of(&self, ty: &Ty) -> Option<DeclId> {
        match ty {
            Ty::Named(id) if self.class(*id).is_some() => Some(*id),
            _ => None,
        }
    }

    /// `src` as a message prints it next to `dst`: a string literal is
    /// shown as `string` unless `dst` is about literals.
    pub(super) fn show_source(&self, src: &Ty, dst: &Ty) -> String {
        let about_literals = self
            .expand(dst)
            .members()
            .iter()
            .any(|m| matches!(self.expand(m), Ty::StrLit(_)));
        if about_literals {
            self.show(src)
        } else {
            self.show(&src.widened())
        }
    }

    /// Declare the names a pattern binds as dynamic.
    pub(super) fn declare_pattern(&mut self, pat: &Pat, konst: bool) {
        match pat {
            Pat::Ident(id) => self.declare(id.id.sym.as_ref(), Ty::Any, konst),
            Pat::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    self.declare_pattern(elem, konst);
                }
            }
            Pat::Object(obj) => {
                for prop in &obj.props {
                    match prop {
                        ObjectPatProp::KeyValue(kv) => self.declare_pattern(&kv.value, konst),
                        ObjectPatProp::Assign(a) => {
                            if let Some(value) = &a.value {
                                self.expr(value);
                            }
                            self.declare(a.key.id.sym.as_ref(), Ty::Any, konst);
                        }
                        ObjectPatProp::Rest(rest) => self.declare_pattern(&rest.arg, konst),
                    }
                }
            }
            Pat::Rest(rest) => self.declare_pattern(&rest.arg, konst),
            Pat::Assign(assign) => {
                self.expr(&assign.right);
                self.declare_pattern(&assign.left, konst);
            }
            Pat::Expr(_) | Pat::Invalid(_) => {}
        }
    }

    /// Declare a parameter inside its function: with its declared type
    /// (an optional one may be `undefined`; a rest parameter is an
    /// array), after checking its default value.
    fn declare_param(&mut self, pat: &Pat, param: &Param) {
        if let Pat::Assign(assign) = pat {
            let found = self.expr(&assign.right);
            self.check_assignable(&found, &param.ty, assign.right.span());
            let inner = Param {
                optional: false,
                ..param.clone()
            };
            return self.declare_param(&assign.left, &inner);
        }
        let name = match pat {
            Pat::Rest(rest) => pat_name(&rest.arg),
            other => pat_name(other),
        };
        let Some(name) = name else {
            return self.declare_pattern(pat, false);
        };
        let ty = if param.rest {
            Ty::array(param.ty.clone())
        } else if param.optional && !param.ty.is_dynamic() {
            Ty::union([param.ty.clone(), Ty::Undefined])
        } else {
            param.ty.clone()
        };
        self.declare(name, ty, false);
    }

    /// Check the body of a function whose `this` is `this`.
    pub(super) fn check_function(&mut self, f: &Function, this: Ty) {
        let sig = self.signature(f);
        let Some(body) = &f.body else { return };
        self.push_function_scope();
        if let Some(params) = &f.type_params {
            for p in &params.params {
                self.scope().type_params.insert(p.name.sym.to_string());
            }
        }
        for (p, param) in f.params.iter().zip(&sig.params) {
            self.declare_param(&p.pat, param);
        }
        let checked_return = !f.is_async && !f.is_generator && f.return_type.is_some();
        self.fns.push(FnCtx {
            ret: checked_return.then(|| sig.ret.clone()),
            this,
        });
        self.hoist(body.stmts.iter());
        for stmt in &body.stmts {
            self.check_stmt(stmt);
        }
        self.fns.pop();
        self.pop_scope();
    }

    /// Check the body of an arrow function. `this` is the enclosing one.
    /// Returns the type of the body when it is an expression.
    pub(super) fn check_arrow(&mut self, f: &ArrowExpr) -> Option<Ty> {
        let sig = self.arrow_signature(f);
        let this = self.fns.last().map_or(Ty::Any, |ctx| ctx.this.clone());
        self.push_function_scope();
        if let Some(params) = &f.type_params {
            for p in &params.params {
                self.scope().type_params.insert(p.name.sym.to_string());
            }
        }
        for (pat, param) in f.params.iter().zip(&sig.params) {
            self.declare_param(pat, param);
        }
        let checked_return = !f.is_async && !f.is_generator && f.return_type.is_some();
        self.fns.push(FnCtx {
            ret: checked_return.then(|| sig.ret.clone()),
            this,
        });
        let mut body_type = None;
        match f.body.as_ref() {
            BlockStmtOrExpr::BlockStmt(body) => {
                self.hoist(body.stmts.iter());
                for stmt in &body.stmts {
                    self.check_stmt(stmt);
                }
            }
            BlockStmtOrExpr::Expr(value) => {
                let found = self.expr(value);
                if checked_return {
                    self.check_assignable(&found, &sig.ret, value.span());
                }
                body_type = Some(found.widened());
            }
        }
        self.fns.pop();
        self.pop_scope();
        body_type
    }

    /// Check a constructor's body.
    pub(super) fn check_constructor(&mut self, c: &Constructor, this: Ty) {
        let Some(body) = &c.body else { return };
        self.push_function_scope();
        for p in &c.params {
            match p {
                ParamOrTsParamProp::Param(p) => {
                    let param = self.param_of(&p.pat);
                    self.declare_param(&p.pat, &param);
                }
                ParamOrTsParamProp::TsParamProp(p) => match &p.param {
                    TsParamPropParam::Ident(id) => {
                        let ty = self.annotation(id.type_ann.as_deref());
                        self.declare(id.id.sym.as_ref(), ty, false);
                    }
                    TsParamPropParam::Assign(a) => {
                        let param = self.param_of(&a.left);
                        let found = self.expr(&a.right);
                        self.check_assignable(&found, &param.ty, a.right.span());
                        self.declare_param(&a.left, &param);
                    }
                },
            }
        }
        self.fns.push(FnCtx { ret: None, this });
        self.hoist(body.stmts.iter());
        for stmt in &body.stmts {
            self.check_stmt(stmt);
        }
        self.fns.pop();
        self.pop_scope();
    }
}
