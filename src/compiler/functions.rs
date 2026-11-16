//! Functions: declarations, bodies, field initialisers, decorators,
//! member keys and `super`.

use super::*;

impl Codegen {
    /// A function declaration (`name` given) or an anonymous function value
    /// (left on the stack).
    pub(super) fn gen_fn_decl(&mut self, name: Option<&Ident>, fn_decl: &Function) {
        if let Some(ident) = name {
            self.next_fn_name = Some(ident.sym.to_string());
        }
        let is_async = fn_decl.is_async;
        if fn_decl.is_generator {
            self.unsupported("generator functions (`function*`)");
        }

        // [function value] ; bind the name ; jump over the body.
        let value_idx = self.emit_function_value(fn_decl.span, 0);
        if let Some(ident) = name {
            self.store_var(ident, true);
        }
        let jump_idx = self.emit_jump();
        let body_start = self.instructions.len();
        self.patch_function_value(value_idx, body_start);

        let pats: Vec<&Pat> = fn_decl.params.iter().map(|p| &p.pat).collect();
        let stmts: &[Stmt] = fn_decl.body.as_ref().map_or(&[], |b| b.stmts.as_slice());
        self.gen_function_body(fn_decl.span, &pats, stmts, is_async, FnKind::Plain, &[]);
        self.patch_jump_here(jump_idx);
    }

    /// The body of a function whose value was just emitted: parameters,
    /// the environment, hoisting, the statements and the implicit result.
    /// A constructor also initializes the instance `fields` (a derived one
    /// once `super(...)` returns) and results in `this`.
    pub(super) fn gen_function_body(
        &mut self,
        span: Span,
        pats: &[&Pat],
        stmts: &[Stmt],
        is_async: bool,
        kind: FnKind,
        fields: &[ClassField],
    ) {
        let fn_start = self.instructions.len();
        let fn_name = self.next_fn_name.take();
        let prev_in_function = self.in_function;
        let prev_async = self.in_async_function;
        let prev_fields = self.class_fields.take();
        let prev_static = self.in_static_method;
        self.in_function = true;
        self.in_async_function = is_async;
        self.in_static_method = matches!(kind, FnKind::Method { is_static: true });
        let prev_control = self.enter_function_control();

        let implicit_super = matches!(
            kind,
            FnKind::Ctor {
                derived: true,
                implicit: true
            }
        );
        if implicit_super {
            // Must be first: the VM passes every argument to a function
            // that starts with it (`constructor(...args) { super(...args) }`).
            self.instructions.push(OpCode::ForwardArgs);
        }
        let params = self.bind_params(pats);
        let prev_env = self.function_prologue(span, &params);
        self.apply_param_defaults(pats);
        // A typed parameter is checked: the caller may be dynamic code.
        for param in &params {
            if self
                .guards
                .contains_key(&(param.span.lo.0, param.span.hi.0))
            {
                self.mark(param.span);
                self.load_var(param);
                self.emit_guard(param.span);
                self.instructions.push(OpCode::Pop);
            }
        }

        match kind {
            FnKind::Ctor { derived: false, .. } => self.emit_field_inits(fields),
            FnKind::Ctor { implicit: true, .. } => {
                self.load_var(&scope::super_ident(span));
                self.instructions.push(OpCode::SuperForward);
                self.instructions.push(OpCode::Pop);
                self.emit_field_inits(fields);
            }
            FnKind::Ctor { .. } => {
                self.class_fields = Some(std::rc::Rc::new(fields.to_vec()));
            }
            _ => {}
        }

        // An async function: its promise, and an implicit `try` around the
        // body whose `catch` rejects the promise.
        let async_try = if is_async {
            self.instructions.push(OpCode::AsyncStart);
            self.instructions
                .push(OpCode::Let(ASYNC_PROMISE.to_string()));
            let setup = self.instructions.len();
            self.instructions.push(OpCode::SetupTry {
                catch_addr: 0,
                finally_addr: 0,
            });
            self.try_stack.push(TryRegion {
                part: TryPart::Try,
                env: None,
                finalizer: None,
            });
            Some(setup)
        } else {
            None
        };

        let param_names: Vec<String> = pats.iter().flat_map(|p| scope::pattern_names(p)).collect();
        self.hoist_body(stmts, &param_names);
        let mut last_instr_was_return = false;
        for s in stmts {
            let before = self.instructions.len();
            self.gen_stmt(s);
            last_instr_was_return = self.instructions.len() > before
                && matches!(self.instructions.last(), Some(OpCode::Return));
        }

        if !last_instr_was_return {
            // Expression statements pop their value, so nothing is left on
            // the stack: the implicit result is `undefined` (`this` for a
            // constructor).
            if matches!(kind, FnKind::Ctor { .. }) {
                self.instructions.push(OpCode::LoadThis);
            } else {
                self.instructions.push(OpCode::Push(JsValue::Undefined));
            }
            if is_async {
                self.unwind_tries(0);
                self.emit_async_return();
            }
            self.instructions.push(OpCode::Return);
        }

        if let Some(setup) = async_try {
            self.try_stack.pop();
            // catch: [reason] -> the promise rejects with it.
            let catch_addr = self.instructions.len();
            self.instructions
                .push(OpCode::Load(ASYNC_PROMISE.to_string()));
            self.instructions.push(OpCode::AsyncReject);
            self.instructions.push(OpCode::Return);
            self.instructions[setup] = OpCode::SetupTry {
                catch_addr,
                finally_addr: 0,
            };
        }

        self.env_active = prev_env;
        self.in_function = prev_in_function;
        self.in_async_function = prev_async;
        self.class_fields = prev_fields;
        self.in_static_method = prev_static;
        self.leave_function_control(prev_control);
        self.functions.push(FunctionRange {
            start: fn_start,
            end: self.instructions.len(),
            name: fn_name,
        });
    }

    /// `this.name = initializer` for each instance field.
    pub(super) fn emit_field_inits(&mut self, fields: &[ClassField]) {
        for field in fields {
            self.instructions.push(OpCode::LoadThis);
            match &field.value {
                Some(v) => self.gen_expr(v),
                None => self.instructions.push(OpCode::Push(JsValue::Undefined)),
            }
            // Initializers from field decorators, in source order (as
            // TypeScript runs them): `value = init.call(this, value)`.
            for span in &field.decorators {
                self.load_var(&scope::field_init_use(*span));
                self.instructions.push(OpCode::LoadThis);
                self.emit_call_initializer();
            }
            self.instructions.push(OpCode::SetProp(field.key.clone()));
        }
    }

    /// `[value, init, this] -> [init.call(this, value)]`, or `[value]` if
    /// `init` is undefined (the decorator returned nothing).
    pub(super) fn emit_call_initializer(&mut self) {
        // [v, f, this] -> [v, this, f] to test f
        self.instructions.push(OpCode::Swap);
        self.instructions.push(OpCode::Dup);
        self.instructions.push(OpCode::Push(JsValue::Undefined));
        self.instructions.push(OpCode::Eq);
        let call = self.emit_jump_if_false();
        self.instructions.push(OpCode::Pop);
        self.instructions.push(OpCode::Pop);
        let end = self.emit_jump();
        self.patch_jump_here(call);
        // [v, this, f] -> [v, f, this]
        self.instructions.push(OpCode::Swap);
        self.instructions.push(OpCode::CallWithThis(1));
        self.patch_jump_here(end);
    }

    /// `{ kind, name, static, private }`, the context a decorator receives.
    pub(super) fn emit_decorator_context(
        &mut self,
        kind: &str,
        name: Option<&str>,
        is_static: bool,
        is_private: bool,
    ) {
        self.instructions.push(OpCode::NewObject);
        let set = |cg: &mut Self, key: &str, v: JsValue| {
            cg.instructions.push(OpCode::Dup);
            cg.instructions.push(OpCode::Push(v));
            cg.instructions.push(OpCode::SetProp(key.to_string()));
        };
        set(self, "kind", JsValue::String(kind.to_string()));
        let name = match name {
            Some(n) => JsValue::String(n.to_string()),
            None => JsValue::Undefined,
        };
        set(self, "name", name);
        if kind != "class" {
            set(self, "static", JsValue::Boolean(is_static));
            set(self, "private", JsValue::Boolean(is_private));
        }
    }

    /// Decorators on the value at the top of the stack: their expressions
    /// are evaluated in source order, then applied last to first, each
    /// `value = decorator(value, context) ?? value`. [value] -> [value]
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_decorators(
        &mut self,
        decorators: &[Decorator],
        kind: &str,
        name: Option<&str>,
        is_static: bool,
        is_private: bool,
    ) {
        let temps: Vec<String> = decorators
            .iter()
            .map(|d| {
                self.gen_expr(&d.expr);
                let temp = format!("%decorator{}", d.span.lo.0);
                self.instructions.push(OpCode::Let(temp.clone()));
                temp
            })
            .collect();
        for temp in temps.iter().rev() {
            // [v] -> [v, v, context, decorator] -> Call -> [v, result]
            self.instructions.push(OpCode::Dup);
            self.emit_decorator_context(kind, name, is_static, is_private);
            self.instructions.push(OpCode::Load(temp.clone()));
            self.instructions.push(OpCode::Call(2));
            // [v, result] -> [result], or [v] if it is undefined
            self.instructions.push(OpCode::Dup);
            self.instructions.push(OpCode::Push(JsValue::Undefined));
            self.instructions.push(OpCode::Eq);
            let keep_result = self.emit_jump_if_false();
            self.instructions.push(OpCode::Pop);
            let end = self.emit_jump();
            self.patch_jump_here(keep_result);
            self.instructions.push(OpCode::Swap);
            self.instructions.push(OpCode::Pop);
            self.patch_jump_here(end);
        }
    }

    /// The property key of private name `#name`: `#name@<class>`, so that
    /// classes in a chain that both declare `#x` keep separate fields.
    pub(super) fn private_key(&self, name: &str) -> String {
        self.private_names
            .iter()
            .rev()
            .find_map(|names| names.get(name).cloned())
            .unwrap_or_else(|| format!("#{}", name))
    }

    /// The property name of `obj.name` or `obj.#name`.
    pub(super) fn member_key(&self, prop: &MemberProp) -> Option<String> {
        match prop {
            MemberProp::Ident(id) => Some(id.sym.to_string()),
            MemberProp::PrivateName(p) => Some(self.private_key(&p.name)),
            MemberProp::Computed(_) => None,
        }
    }

    /// `[obj] -> [obj.name]`; a private member the object lacks throws.
    pub(super) fn emit_member_get(&mut self, prop: &MemberProp) {
        match prop {
            MemberProp::PrivateName(p) => {
                let key = self.private_key(&p.name);
                self.instructions.push(OpCode::GetPrivate(key));
            }
            _ => {
                let key = self.member_key(prop).unwrap_or_default();
                self.instructions.push(OpCode::GetProp(key));
            }
        }
    }

    /// `[obj, value] -> []` for `obj.name = value`.
    pub(super) fn emit_member_set(&mut self, prop: &MemberProp) {
        match prop {
            MemberProp::PrivateName(p) => {
                let key = self.private_key(&p.name);
                self.instructions.push(OpCode::SetPrivate(key));
            }
            _ => {
                let key = self.member_key(prop).unwrap_or_default();
                self.instructions.push(OpCode::SetProp(key));
            }
        }
    }

    /// `[] -> [parent]` for `super` at `span`: the parent class itself in a
    /// static method, its prototype otherwise.
    pub(super) fn emit_super_base(&mut self, span: Span) {
        self.load_var(&scope::super_ident(span));
        if !self.in_static_method {
            self.instructions
                .push(OpCode::GetProp("prototype".to_string()));
        }
    }

    /// `super.name` / `super[key]`: the property of the parent class (in a
    /// static method) or of its prototype.
    pub(super) fn gen_super_prop(&mut self, sp: &swc_ecma_ast::SuperPropExpr) {
        match &sp.prop {
            swc_ecma_ast::SuperProp::Ident(id) => {
                // A getter found on the parent runs with this `this`.
                self.instructions.push(OpCode::LoadThis);
                self.emit_super_base(sp.obj.span);
                self.instructions
                    .push(OpCode::GetSuperProp(id.sym.to_string()));
            }
            swc_ecma_ast::SuperProp::Computed(c) => {
                self.emit_super_base(sp.obj.span);
                self.gen_expr(&c.expr);
                self.instructions.push(OpCode::GetPropComputed);
            }
        }
    }

    /// `[] -> [function]` for a constructor or method.
    pub(super) fn gen_member_function(
        &mut self,
        span: Span,
        pats: &[&Pat],
        stmts: &[Stmt],
        is_async: bool,
        kind: FnKind,
        fields: &[ClassField],
    ) {
        let value_idx = self.emit_function_value(span, 0);
        let jump_idx = self.emit_jump();
        let body_start = self.instructions.len();
        self.patch_function_value(value_idx, body_start);
        self.gen_function_body(span, pats, stmts, is_async, kind, fields);
        self.patch_jump_here(jump_idx);
    }
}
