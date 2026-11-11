//! Environments and variables: where a variable lives, how a scope or
//! function gets its environment object, and how parameters are bound.

use super::*;

impl Codegen {
    /// `[value] -> [promise]`: an async function's result resolves its
    /// promise, which is what the function returns.
    pub(super) fn emit_async_return(&mut self) {
        self.instructions
            .push(OpCode::Load(ASYNC_PROMISE.to_string()));
        self.instructions.push(OpCode::AsyncResolve);
    }

    pub(super) fn top_level_prologue(&mut self) {
        if self.scopes.top_level_uses_env() {
            self.instructions.push(OpCode::Push(JsValue::Undefined));
            self.instructions.push(OpCode::Let(SCOPE_LOCAL.to_string()));
            self.env_active = true;
        }
    }

    /// Push the environment `hops` parent links above `%scope`.
    pub(super) fn emit_env(&mut self, hops: u32) {
        self.instructions
            .push(OpCode::Load(SCOPE_LOCAL.to_string()));
        for _ in 0..hops {
            self.instructions
                .push(OpCode::GetProp(PARENT_PROP.to_string()));
        }
    }

    /// `[] -> [value of ident]`
    /// The name a variable has in the program: a top-level binding of a
    /// linked module has the module's prefix, and an import names the
    /// binding it imports.
    pub(super) fn var_name(&self, ident: &Ident) -> String {
        let name = ident.sym.to_string();
        match &self.link {
            Some(link) if self.scopes.is_module_level(ident.span) => {
                match link.imports.get(&name) {
                    Some(global) => global.clone(),
                    None => format!("{}{}", link.prefix, name),
                }
            }
            _ => name,
        }
    }

    pub(super) fn load_var(&mut self, ident: &Ident) {
        if ident.sym.starts_with(intrinsics::PREFIX) {
            let message = format!("'{}' is an intrinsic: it can only be called", ident.sym);
            self.errors.push((ident.span, message));
        }
        let name = self.var_name(ident);
        match self.scopes.var(ident.span) {
            VarRef::Plain => self.instructions.push(OpCode::Load(name)),
            VarRef::Env { hops } => {
                self.emit_env(hops);
                self.instructions.push(OpCode::GetProp(name));
            }
        }
    }

    /// `[value] -> []`: assign to (or, with `declare`, bind) `ident`.
    pub(super) fn store_var(&mut self, ident: &Ident, declare: bool) {
        if let Some(link) = &self.link
            && self.scopes.is_module_level(ident.span)
            && (link.imports.contains_key(&*ident.sym) || link.namespaces.contains_key(&*ident.sym))
        {
            let message = format!("cannot assign to '{}': it is an import", ident.sym);
            self.errors.push((ident.span, message));
        }
        let name = self.var_name(ident);
        match self.scopes.var(ident.span) {
            VarRef::Plain if declare => self.instructions.push(OpCode::Let(name)),
            VarRef::Plain => self.instructions.push(OpCode::Store(name)),
            VarRef::Env { hops } => {
                self.emit_env(hops);
                self.instructions.push(OpCode::Swap);
                self.instructions.push(OpCode::SetProp(name));
            }
        }
    }

    /// `[] -> [new environment whose parent is %scope]`
    pub(super) fn emit_child_env(&mut self) {
        self.instructions.push(OpCode::NewObject);
        self.instructions.push(OpCode::Dup);
        self.instructions
            .push(OpCode::Load(SCOPE_LOCAL.to_string()));
        self.instructions
            .push(OpCode::SetProp(PARENT_PROP.to_string()));
    }

    /// Remember `%scope` in a fresh local, so it can be restored after a
    /// block, at a loop's continue and exit points, or in a catch handler.
    pub(super) fn save_env(&mut self) -> Option<String> {
        if !self.env_active {
            return None;
        }
        let save = format!("%save{}", self.instructions.len());
        self.instructions
            .push(OpCode::Load(SCOPE_LOCAL.to_string()));
        self.instructions.push(OpCode::Let(save.clone()));
        Some(save)
    }

    pub(super) fn restore_env(&mut self, save: &Option<String>) {
        if let Some(save) = save {
            self.instructions.push(OpCode::Load(save.clone()));
            self.instructions
                .push(OpCode::Store(SCOPE_LOCAL.to_string()));
        }
    }

    /// Enter the scope opened by the node at `span`: if it has captured
    /// bindings, a new environment becomes `%scope`. Returns what
    /// `exit_scope_env` needs to restore the enclosing one.
    pub(super) fn enter_scope_env(&mut self, span: Span) -> Option<String> {
        self.scopes.env_scope(span)?;
        let save = self.save_env();
        self.emit_child_env();
        self.instructions
            .push(OpCode::Store(SCOPE_LOCAL.to_string()));
        save
    }

    pub(super) fn exit_scope_env(&mut self, save: Option<String>) {
        self.restore_env(&save);
    }

    /// Start of a function body, after the parameters are bound: set up
    /// `%scope` (the closure's environment, then the function's own) and
    /// move captured parameters into it. Returns the previous `env_active`.
    pub(super) fn function_prologue(&mut self, fn_span: Span, params: &[&Ident]) -> bool {
        let prev = self.env_active;
        self.env_active = self.scopes.uses_env(fn_span);
        if !self.env_active {
            return prev;
        }
        if self.scopes.is_closure(fn_span) {
            self.instructions.push(OpCode::LoadClosureEnv);
        } else {
            self.instructions.push(OpCode::Push(JsValue::Undefined));
        }
        self.instructions.push(OpCode::Let(SCOPE_LOCAL.to_string()));
        if let Some(captured) = self.scopes.env_scope(fn_span) {
            let captured: Vec<String> = captured.to_vec();
            self.emit_child_env();
            self.instructions
                .push(OpCode::Store(SCOPE_LOCAL.to_string()));
            for p in params {
                let name = p.sym.to_string();
                if captured.contains(&name) {
                    self.instructions.push(OpCode::Load(name.clone()));
                    self.store_var(p, true);
                }
            }
            // Arrow functions inside read this function's `this` from here.
            if captured.iter().any(|n| n == scope::THIS_LOCAL) {
                self.instructions.push(OpCode::LoadThis);
                self.store_var(&scope::this_ident(scope::this_decl_span(fn_span)), true);
            }
        }
        prev
    }

    /// Bind the arguments on the stack to `params` (in reverse order). The
    /// function must start with exactly these `Let`s: the VM and IR lowering
    /// count them as its parameters. Returns the parameter identifiers.
    pub(super) fn bind_params<'p>(&mut self, params: &[&'p Pat]) -> Vec<&'p Ident> {
        // `...rest` (last): the caller passes an array of the extra arguments.
        if let Some((Pat::Rest(_), before)) = params.split_last() {
            self.instructions.push(OpCode::RestArgs(before.len()));
        }
        let mut idents = Vec::new();
        for (i, pat) in params.iter().enumerate().rev() {
            let ident = match pat {
                Pat::Ident(id) => Some(&id.id),
                Pat::Assign(a) => match a.left.as_ref() {
                    Pat::Ident(id) => Some(&id.id),
                    _ => None,
                },
                Pat::Rest(r) if i + 1 == params.len() => match r.arg.as_ref() {
                    Pat::Ident(id) => Some(&id.id),
                    _ => None,
                },
                _ => None,
            };
            match ident {
                Some(id) => {
                    self.instructions.push(OpCode::Let(id.sym.to_string()));
                    idents.push(id);
                }
                None => {
                    self.current_span = pat.span();
                    self.unsupported("destructuring function parameters")
                }
            }
        }
        idents
    }

    /// `function f(a = 1)`: after the prologue, replace an `undefined`
    /// argument with its default value.
    pub(super) fn apply_param_defaults(&mut self, params: &[&Pat]) {
        for pat in params {
            if let Pat::Assign(a) = pat
                && let Pat::Ident(id) = a.left.as_ref()
            {
                self.load_var(&id.id);
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.instructions.push(OpCode::Eq);
                let skip = self.emit_jump_if_false();
                self.gen_expr(&a.right);
                self.store_var(&id.id, false);
                self.patch_jump_here(skip);
            }
        }
    }

    /// Push the function value for the body starting at `address`: a
    /// closure over `%scope` when the function captures variables.
    /// Returns the index of the instruction holding the address, for
    /// patching.
    pub(super) fn emit_function_value(&mut self, fn_span: Span, address: usize) -> usize {
        if self.scopes.is_closure(fn_span) {
            self.instructions
                .push(OpCode::Load(SCOPE_LOCAL.to_string()));
            self.instructions.push(OpCode::MakeClosure(address));
        } else {
            self.instructions
                .push(OpCode::Push(JsValue::Function { address, env: None }));
        }
        self.instructions.len() - 1
    }

    pub(super) fn patch_function_value(&mut self, idx: usize, address: usize) {
        match &mut self.instructions[idx] {
            OpCode::MakeClosure(a) => *a = address,
            OpCode::Push(JsValue::Function { address: a, .. }) => *a = address,
            other => unreachable!("not a function value: {:?}", other),
        }
    }

    /// `[] -> [new environment: a copy of %scope's bindings `names`]`,
    /// then made `%scope`. Gives each loop iteration its own bindings.
    pub(super) fn emit_iteration_env(&mut self, names: &[String]) {
        self.instructions.push(OpCode::NewObject);
        self.instructions.push(OpCode::Dup);
        self.emit_env(1);
        self.instructions
            .push(OpCode::SetProp(PARENT_PROP.to_string()));
        for name in names {
            self.instructions.push(OpCode::Dup);
            self.emit_env(0);
            self.instructions.push(OpCode::GetProp(name.clone()));
            self.instructions.push(OpCode::SetProp(name.clone()));
        }
        self.instructions
            .push(OpCode::Store(SCOPE_LOCAL.to_string()));
    }
}
