//! Expressions: the type of each, and the checks on assignments, calls
//! and `new`.

use super::narrow::path_of;
use super::resolve::prop_name;
use super::*;
use swc_common::Spanned;

impl Checker {
    /// The type of `e`. Reports what is wrong inside it.
    pub(super) fn expr(&mut self, e: &Expr) -> Ty {
        match e {
            Expr::Lit(lit) => match lit {
                Lit::Str(s) => Ty::StrLit(s.value.to_string_lossy().as_ref().into()),
                Lit::Num(_) => Ty::Number,
                Lit::Bool(_) => Ty::Boolean,
                Lit::Null(_) => Ty::Null,
                _ => Ty::Any,
            },
            Expr::Ident(id) => self.ident(id.sym.as_ref()),
            Expr::This(_) => match self.narrowed("this") {
                Some(ty) => ty,
                None => self.fns.last().map_or(Ty::Any, |ctx| ctx.this.clone()),
            },
            Expr::Paren(p) => self.expr(&p.expr),
            Expr::Tpl(tpl) => {
                for part in &tpl.exprs {
                    self.expr(part);
                }
                Ty::String
            }
            Expr::Array(arr) => self.array_literal(arr),
            Expr::Object(obj) => self.object_literal(obj),
            Expr::Fn(f) => {
                let sig = self.signature(&f.function);
                self.push_scope();
                if let Some(name) = &f.ident {
                    self.declare(name.sym.as_ref(), Ty::Function(sig.clone()), true);
                }
                self.check_function(&f.function, Ty::Any);
                self.scopes.pop();
                Ty::Function(sig)
            }
            Expr::Arrow(f) => {
                let sig = self.arrow_signature(f);
                // `(x: number) => x * 2` has no declared result, but
                // its body says what it is.
                match self.check_arrow(f) {
                    Some(body) if f.return_type.is_none() && !f.is_async && !f.is_generator => {
                        Ty::function(sig.params.clone(), body)
                    }
                    _ => Ty::Function(sig),
                }
            }
            Expr::Class(c) => {
                self.check_class(None, &c.class);
                Ty::Any
            }
            Expr::Unary(u) => {
                self.expr(&u.arg);
                match u.op {
                    UnaryOp::Bang | UnaryOp::Delete => Ty::Boolean,
                    UnaryOp::Minus | UnaryOp::Plus | UnaryOp::Tilde => Ty::Number,
                    UnaryOp::TypeOf => Ty::String,
                    UnaryOp::Void => Ty::Undefined,
                }
            }
            Expr::Update(u) => {
                self.expr(&u.arg);
                self.unguard(u.arg.span());
                Ty::Number
            }
            Expr::Bin(bin) => self.binary(bin),
            Expr::Assign(assign) => self.assignment(assign),
            Expr::Cond(cond) => {
                self.expr(&cond.test);
                let facts = self.facts(&cond.test);
                self.push_scope();
                self.apply(&facts.then);
                let cons = self.expr(&cond.cons);
                self.pop_scope();
                self.push_scope();
                self.apply(&facts.otherwise);
                let alt = self.expr(&cond.alt);
                self.pop_scope();
                Ty::union([cons, alt])
            }
            Expr::Seq(seq) => {
                let mut last = Ty::Undefined;
                for e in &seq.exprs {
                    last = self.expr(e);
                }
                last
            }
            Expr::Member(m) => self.member(m),
            Expr::OptChain(chain) => self.opt_chain(chain),
            Expr::Call(call) => self.call(call),
            Expr::New(new) => self.new_expr(new),
            Expr::Await(a) => {
                self.expr(&a.arg);
                Ty::Any
            }
            Expr::Yield(y) => {
                if let Some(arg) = &y.arg {
                    self.expr(arg);
                }
                Ty::Any
            }
            Expr::TaggedTpl(t) => {
                self.expr(&t.tag);
                for part in &t.tpl.exprs {
                    self.expr(part);
                }
                Ty::Any
            }
            Expr::SuperProp(_) | Expr::MetaProp(_) => Ty::Any,
            // A cast is checked when the program runs; here it is the
            // type it names.
            Expr::TsAs(cast) => {
                let found = self.expr(&cast.expr);
                self.cast(&found, &cast.type_ann, cast.span)
            }
            Expr::TsTypeAssertion(cast) => {
                let found = self.expr(&cast.expr);
                self.cast(&found, &cast.type_ann, cast.span)
            }
            Expr::TsNonNull(non_null) => {
                let ty = self.expr(&non_null.expr);
                let present = self.without_nullish(&ty);
                if present != ty {
                    self.guard(non_null.span, &present);
                }
                present
            }
            Expr::TsConstAssertion(e) => self.expr(&e.expr),
            Expr::TsInstantiation(e) => self.expr(&e.expr),
            Expr::TsSatisfies(e) => {
                let found = self.expr(&e.expr);
                let wanted = self.resolve(&e.type_ann);
                self.check_assignable(&found, &wanted, e.expr.span());
                found
            }
            _ => Ty::Any,
        }
    }

    /// `<found> as T`: the type it names. Unless the checker knows the
    /// value is one, it is checked when the program runs.
    fn cast(&mut self, found: &Ty, ann: &TsType, span: Span) -> Ty {
        let wanted = self.resolve(ann);
        if found.is_dynamic() || !self.assignable(found, &wanted) {
            self.guard(span, &wanted);
        }
        wanted
    }

    /// The type of the variable `name` here.
    pub(super) fn ident(&mut self, name: &str) -> Ty {
        if let Some(ty) = self.narrowed(name) {
            return ty;
        }
        match self.lookup_var(name) {
            Some(var) => var.ty.clone(),
            None => builtins::global(name).unwrap_or(Ty::Any),
        }
    }

    /// `ty` without `null` and `undefined`.
    fn without_nullish(&self, ty: &Ty) -> Ty {
        let expanded = self.expand(ty);
        if expanded.is_dynamic() {
            return expanded;
        }
        let members = expanded.members().iter().map(|m| self.expand(m));
        Ty::union(members.filter(|m| !m.is_nullish()))
    }

    fn array_literal(&mut self, arr: &ArrayLit) -> Ty {
        let mut elems = Vec::new();
        for elem in arr.elems.iter().flatten() {
            let ty = self.expr(&elem.expr);
            // What a spread adds is not followed.
            elems.push(if elem.spread.is_some() {
                Ty::Any
            } else {
                ty.widened()
            });
        }
        Ty::array(Ty::union(elems))
    }

    /// An object literal: an object type with its properties, or `any`
    /// if it has a spread or a computed key.
    fn object_literal(&mut self, obj: &ObjectLit) -> Ty {
        let mut ty = ObjTy::default();
        let mut known = true;
        for prop in &obj.props {
            let PropOrSpread::Prop(prop) = prop else {
                if let PropOrSpread::Spread(spread) = prop {
                    self.expr(&spread.expr);
                }
                known = false;
                continue;
            };
            let (name, value) = match prop.as_ref() {
                swc_ecma_ast::Prop::KeyValue(kv) => {
                    let value = self.expr(&kv.value).widened();
                    (prop_name(&kv.key), value)
                }
                swc_ecma_ast::Prop::Shorthand(id) => (
                    Some(id.sym.to_string()),
                    self.ident(id.sym.as_ref()).widened(),
                ),
                swc_ecma_ast::Prop::Method(m) => {
                    let sig = self.signature(&m.function);
                    self.check_function(&m.function, Ty::Any);
                    (prop_name(&m.key), Ty::Function(sig))
                }
                swc_ecma_ast::Prop::Getter(g) => {
                    if let Some(body) = &g.body {
                        self.check_detached_body(&body.stmts);
                    }
                    (prop_name(&g.key), self.annotation(g.type_ann.as_deref()))
                }
                swc_ecma_ast::Prop::Setter(s) => {
                    if let Some(body) = &s.body {
                        self.push_function_scope();
                        self.declare_pattern(&s.param, false);
                        self.fns.push(FnCtx {
                            ret: None,
                            this: Ty::Any,
                        });
                        for stmt in &body.stmts {
                            self.check_stmt(stmt);
                        }
                        self.fns.pop();
                        self.pop_scope();
                    }
                    continue;
                }
                swc_ecma_ast::Prop::Assign(_) => (None, Ty::Any),
            };
            match name {
                Some(name) => ty.set(Prop {
                    name,
                    ty: value,
                    optional: false,
                }),
                None => known = false,
            }
        }
        if known {
            Ty::Object(Rc::new(ty))
        } else {
            Ty::Any
        }
    }

    /// Check a function body that has no parameters or declared result.
    fn check_detached_body(&mut self, stmts: &[Stmt]) {
        self.push_function_scope();
        self.fns.push(FnCtx {
            ret: None,
            this: Ty::Any,
        });
        self.hoist(stmts.iter());
        for stmt in stmts {
            self.check_stmt(stmt);
        }
        self.fns.pop();
        self.pop_scope();
    }

    fn binary(&mut self, bin: &BinExpr) -> Ty {
        use BinaryOp::*;
        match bin.op {
            // The right side runs only when the left one held (or did
            // not), so it is checked with what that establishes.
            LogicalAnd | LogicalOr | NullishCoalescing => {
                let left = self.expr(&bin.left);
                let facts = self.facts(&bin.left);
                self.push_scope();
                match bin.op {
                    LogicalAnd => self.apply(&facts.then),
                    LogicalOr => self.apply(&facts.otherwise),
                    _ => {}
                }
                let right = self.expr(&bin.right);
                self.pop_scope();
                if left.is_dynamic() || right.is_dynamic() {
                    return Ty::Any;
                }
                match bin.op {
                    LogicalAnd => Ty::union([left, right]),
                    _ => Ty::union([self.without_nullish(&left), right]),
                }
            }
            _ => {
                let left = self.expr(&bin.left);
                let right = self.expr(&bin.right);
                self.binary_result(bin.op, &left, &right)
            }
        }
    }

    /// The type of `left op right` for an operator that is not logical.
    fn binary_result(&self, op: BinaryOp, left: &Ty, right: &Ty) -> Ty {
        use BinaryOp::*;
        match op {
            Add => {
                let is_string = |t: &Ty| matches!(self.expand(t), Ty::String | Ty::StrLit(_));
                let is_number = |t: &Ty| matches!(self.expand(t), Ty::Number);
                if is_string(left) || is_string(right) {
                    Ty::String
                } else if is_number(left) && is_number(right) {
                    Ty::Number
                } else {
                    Ty::Any
                }
            }
            Sub | Mul | Div | Mod | Exp | BitOr | BitAnd | BitXor | LShift | RShift
            | ZeroFillRShift => Ty::Number,
            EqEq | NotEq | EqEqEq | NotEqEq | Lt | LtEq | Gt | GtEq | In | InstanceOf => {
                Ty::Boolean
            }
            LogicalAnd | LogicalOr | NullishCoalescing => Ty::Any,
        }
    }

    fn assignment(&mut self, assign: &AssignExpr) -> Ty {
        let value = self.expr(&assign.right);
        let (target, path) = match &assign.left {
            AssignTarget::Simple(SimpleAssignTarget::Ident(id)) => {
                let name = id.id.sym.to_string();
                (self.declared(&name), Some(name))
            }
            AssignTarget::Simple(SimpleAssignTarget::Member(m)) => {
                let path = path_of(&Expr::Member(m.clone()));
                (self.member_declared(m), path)
            }
            AssignTarget::Simple(SimpleAssignTarget::Paren(p)) => {
                self.expr(&p.expr);
                (Ty::Any, None)
            }
            AssignTarget::Simple(_) => (Ty::Any, None),
            AssignTarget::Pat(pat) => {
                for name in flow::assigned_in(pat) {
                    self.forget(&name);
                }
                return value;
            }
        };
        // `a += b` stores the result of `a + b`.
        let stored = match assign.op.to_update() {
            Some(op) => {
                let current = match &path {
                    Some(path) => self.narrowed(path).unwrap_or_else(|| target.clone()),
                    None => target.clone(),
                };
                match op {
                    BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing => {
                        Ty::union([self.without_nullish(&current), value.clone()])
                    }
                    op => self.binary_result(op, &current, &value),
                }
            }
            None => value.clone(),
        };
        match assign.op.to_update() {
            // What is stored is the result of the operation, which is
            // the value of the whole expression.
            Some(_) => {
                self.report_unassignable(&stored, &target, assign.right.span());
                self.guard_flow(&stored, &target, assign.span);
            }
            None => self.check_assignable(&stored, &target, assign.right.span()),
        }
        if let Some(path) = path {
            match &assign.left {
                AssignTarget::Simple(SimpleAssignTarget::Ident(_)) => {
                    self.forget(&path);
                    self.narrow_by_assignment(&path, &stored, &target);
                }
                _ => {
                    // A property: what was established about it and what
                    // is below it no longer holds.
                    let prefix = format!("{path}.");
                    let scope = self.scope();
                    scope.narrowed.retain(|p, _| !p.starts_with(&prefix));
                    scope.narrowed.insert(path.clone(), None);
                    self.narrow_by_assignment(&path, &stored, &target);
                }
            }
        }
        stored
    }

    /// The declared type of the property a member expression names (not
    /// what control flow narrowed it to): what a store is checked
    /// against.
    fn member_declared(&mut self, m: &MemberExpr) -> Ty {
        self.member_parts(m).0
    }

    /// The declared type of the property, and whether reading it reads
    /// the heap (which dynamic code can write): a property of an object
    /// or an element of an array, not `"abc".length`.
    fn member_parts(&mut self, m: &MemberExpr) -> (Ty, bool) {
        let object = self.expr(&m.obj);
        let built_in = match self.expand(&object) {
            Ty::String | Ty::StrLit(_) | Ty::Number | Ty::Boolean => true,
            Ty::Array(_) | Ty::Tuple(_) => matches!(m.prop, MemberProp::Ident(_)),
            _ => false,
        };
        let ty = match &m.prop {
            MemberProp::Ident(name) => self.member_type(&object, name.sym.as_ref()),
            MemberProp::Computed(c) => {
                let key = self.expr(&c.expr);
                // `pair[0]`: the element at that position.
                if let (Ty::Tuple(elems), Expr::Lit(Lit::Num(n))) =
                    (self.expand(&object), c.expr.as_ref())
                {
                    let elem = elems.get(n.value as usize).cloned();
                    return (elem.unwrap_or(Ty::Undefined), true);
                }
                self.element_type(&object, &key)
            }
            MemberProp::PrivateName(_) => Ty::Any,
        };
        self.built_in_member = matches!(
            self.expand(&object),
            Ty::String | Ty::StrLit(_) | Ty::Number | Ty::Boolean
        );
        (ty, !built_in)
    }

    fn member(&mut self, m: &MemberExpr) -> Ty {
        // `Math.x`, when the program has not declared `Math`.
        if let (Expr::Ident(obj), MemberProp::Ident(name)) = (m.obj.as_ref(), &m.prop)
            && obj.sym.as_ref() == "Math"
            && self.lookup_var("Math").is_none()
        {
            return builtins::math_member(name.sym.as_ref());
        }
        let (declared, heap) = self.member_parts(m);
        if heap {
            // Checked against the declared type: what control flow
            // established about a property does not survive a call.
            let read = self.chained(&m.obj, declared.clone());
            self.guard(m.span, &read);
        }
        match path_of(&Expr::Member(m.clone())).and_then(|path| self.narrowed(&path)) {
            Some(narrowed) => narrowed,
            None => declared,
        }
    }

    /// `ty`, or `undefined` if the expression it was read from is inside
    /// an optional chain (`a?.b.c` is `undefined` when `a` is).
    fn chained(&self, object: &Expr, ty: Ty) -> Ty {
        let mut cur = object;
        loop {
            match cur {
                Expr::OptChain(_) => return Ty::union([ty, Ty::Undefined]),
                Expr::Member(m) => cur = &m.obj,
                Expr::Call(CallExpr {
                    callee: Callee::Expr(callee),
                    ..
                }) => cur = callee,
                _ => return ty,
            }
        }
    }

    fn opt_chain(&mut self, chain: &OptChainExpr) -> Ty {
        match chain.base.as_ref() {
            OptChainBase::Member(m) => {
                let object = self.expr(&m.obj);
                let present = self.without_nullish(&object);
                let ty = match &m.prop {
                    MemberProp::Ident(name) => self.member_type(&present, name.sym.as_ref()),
                    MemberProp::Computed(c) => {
                        let key = self.expr(&c.expr);
                        self.element_type(&present, &key)
                    }
                    MemberProp::PrivateName(_) => Ty::Any,
                };
                if ty.is_dynamic() {
                    Ty::Any
                } else {
                    let ty = Ty::union([ty, Ty::Undefined]);
                    self.guard(chain.span, &ty);
                    ty
                }
            }
            OptChainBase::Call(call) => {
                self.expr(&call.callee);
                for arg in &call.args {
                    self.expr(&arg.expr);
                }
                Ty::Any
            }
        }
    }

    /// The type of `object.name`.
    pub(super) fn member_type(&self, object: &Ty, name: &str) -> Ty {
        let object = self.expand(object);
        match &object {
            Ty::String | Ty::StrLit(_) => builtins::string_member(name).unwrap_or(Ty::Any),
            Ty::Number => builtins::number_member(name).unwrap_or(Ty::Any),
            Ty::Array(elem) => builtins::array_member(elem, name).unwrap_or(Ty::Any),
            Ty::Tuple(elems) => {
                builtins::array_member(&Ty::tuple_element(elems), name).unwrap_or(Ty::Any)
            }
            Ty::ClassValue(id) => self.static_member(*id, name).map_or(Ty::Any, |p| p.ty),
            Ty::Object(_) | Ty::Named(_) => {
                let Some(members) = self.members(&object) else {
                    return Ty::Any;
                };
                match members.obj.get(name) {
                    Some(prop) if prop.optional && !prop.ty.is_dynamic() => {
                        Ty::union([prop.ty.clone(), Ty::Undefined])
                    }
                    Some(prop) => prop.ty.clone(),
                    None => members.obj.index.clone().unwrap_or(Ty::Any),
                }
            }
            // The property of whichever member the value is; reading it
            // from `null` or `undefined` throws, so those do not count.
            Ty::Union(members) => {
                let mut types = Vec::new();
                for m in members.iter().filter(|m| !self.expand(m).is_nullish()) {
                    let ty = self.member_type(m, name);
                    if ty.is_dynamic() {
                        return Ty::Any;
                    }
                    types.push(ty);
                }
                if types.is_empty() {
                    Ty::Any
                } else {
                    Ty::union(types)
                }
            }
            _ => Ty::Any,
        }
    }

    /// The type of `object[key]`.
    fn element_type(&self, object: &Ty, key: &Ty) -> Ty {
        let object = self.expand(object);
        match (&object, self.expand(key)) {
            (_, Ty::StrLit(name)) => self.member_type(&object, &name),
            (Ty::Array(elem), Ty::Number) => (**elem).clone(),
            (Ty::String | Ty::StrLit(_), Ty::Number) => Ty::String,
            _ => Ty::Any,
        }
    }

    fn call(&mut self, call: &CallExpr) -> Ty {
        let callee = match &call.callee {
            Callee::Expr(callee) => self.expr(callee),
            Callee::Super(_) | Callee::Import(_) => Ty::Any,
        };
        // Whether the function called is known to be the one declared:
        // a function declaration, a constant whose initialiser is a
        // function, or a method of a string or a number. Anything else
        // can come from dynamic code, so its result is checked.
        let known = match &call.callee {
            Callee::Expr(callee) => match callee.as_ref() {
                Expr::Ident(id) => match self.lookup_var(id.sym.as_ref()) {
                    Some(var) => var.fixed_fn,
                    None => true,
                },
                Expr::Member(m) => {
                    self.unguard(m.span);
                    let math = matches!(m.obj.as_ref(), Expr::Ident(o)
                        if o.sym.as_ref() == "Math" && self.lookup_var("Math").is_none());
                    math || self.built_in_member
                }
                _ => false,
            },
            _ => true,
        };
        match self.expand(&callee) {
            Ty::Function(sig) => {
                let ret = self.check_arguments(&sig, &call.args, call.span);
                if let (false, Callee::Expr(callee)) = (known, &call.callee) {
                    let result = self.chained(callee, ret.clone());
                    self.guard(call.span, &result);
                }
                ret
            }
            other => {
                for arg in &call.args {
                    self.expr(&arg.expr);
                }
                let not_callable = matches!(
                    other,
                    Ty::Number
                        | Ty::String
                        | Ty::StrLit(_)
                        | Ty::Boolean
                        | Ty::Null
                        | Ty::Undefined
                        | Ty::Array(_)
                        | Ty::Tuple(_)
                );
                if not_callable {
                    let message = format!(
                        "This expression is not callable. Type '{}' has no call signatures.",
                        self.show(&other.widened())
                    );
                    self.report(call.span, message);
                }
                Ty::Any
            }
        }
    }

    fn new_expr(&mut self, new: &NewExpr) -> Ty {
        let callee = self.expr(&new.callee);
        let args = new.args.as_deref().unwrap_or(&[]);
        match callee {
            Ty::ClassValue(id) => {
                match self.constructor_of(id) {
                    Some(sig) => {
                        self.check_arguments(&sig, args, new.span);
                    }
                    None => {
                        for arg in args {
                            self.expr(&arg.expr);
                        }
                    }
                }
                Ty::Named(id)
            }
            _ => {
                for arg in args {
                    self.expr(&arg.expr);
                }
                Ty::Any
            }
        }
    }

    /// Check the arguments of a call against `sig`; the call's type.
    fn check_arguments(&mut self, sig: &FnTy, args: &[ExprOrSpread], span: Span) -> Ty {
        let spread_at = args.iter().position(|a| a.spread.is_some());
        for (i, arg) in args.iter().enumerate() {
            let found = self.expr(&arg.expr);
            // After a spread the positions are not known.
            if spread_at.is_some_and(|s| i >= s) {
                continue;
            }
            let Some(wanted) = sig.param(i) else { continue };
            let optional = sig.params.get(i).is_some_and(|p| p.optional);
            let wanted = if optional && !wanted.is_dynamic() {
                Ty::union([wanted.clone(), Ty::Undefined])
            } else {
                wanted.clone()
            };
            if !self.assignable(&found, &wanted) {
                let message = format!(
                    "Argument of type '{}' is not assignable to parameter of type '{}'.",
                    self.show_source(&found, &wanted),
                    self.show(&wanted)
                );
                self.report(arg.expr.span(), message);
            }
        }
        let required = sig.required();
        if spread_at.is_none() && args.len() < required {
            // Only parameters with a declared type are a contract.
            let missing_typed = sig.params[args.len()..required]
                .iter()
                .any(|p| !p.ty.is_dynamic());
            if missing_typed {
                let message = format!("Expected {} arguments, but got {}.", required, args.len());
                self.report(span, message);
            }
        }
        sig.ret.clone()
    }
}
