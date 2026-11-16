//! Control flow: errors for unsupported syntax, loops and labels, `try`
//! regions, hoisting, jumps, logical operators and optional chains.

use super::*;

impl Codegen {
    /// Record that a construct cannot be compiled. Compilation will fail.
    pub(super) fn unsupported(&mut self, what: impl std::fmt::Display) {
        self.errors
            .push((self.current_span, format!("unsupported: {}", what)));
    }

    /// Record an error in the program at the current statement.
    pub(super) fn syntax_error(&mut self, what: impl std::fmt::Display) {
        self.errors.push((self.current_span, what.to_string()));
    }

    /// Enter a function body: loops, labels and `try` statements outside it
    /// are not reachable from inside.
    pub(super) fn enter_function_control(&mut self) -> ControlState {
        ControlState {
            loops: std::mem::take(&mut self.loop_stack),
            tries: std::mem::take(&mut self.try_stack),
            pending_labels: std::mem::take(&mut self.pending_labels),
        }
    }

    pub(super) fn leave_function_control(&mut self, saved: ControlState) {
        self.loop_stack = saved.loops;
        self.try_stack = saved.tries;
        self.pending_labels = saved.pending_labels;
    }

    /// Start a loop's jump target, taking the labels in front of it.
    pub(super) fn push_loop(&mut self, break_env: Option<String>, continue_env: Option<String>) {
        let labels = std::mem::take(&mut self.pending_labels);
        self.loop_stack.push(LoopContext {
            kind: TargetKind::Loop,
            labels,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            break_env,
            continue_env,
            try_depth: self.try_stack.len(),
        });
    }

    /// End the innermost jump target: `break` goes to `break_addr`,
    /// `continue` to `continue_addr`.
    pub(super) fn pop_target(&mut self, break_addr: usize, continue_addr: usize) {
        if let Some(ctx) = self.loop_stack.pop() {
            for idx in ctx.break_jumps {
                if let OpCode::Jump(ref mut addr) = self.instructions[idx] {
                    *addr = break_addr;
                }
            }
            for idx in ctx.continue_jumps {
                if let OpCode::Jump(ref mut addr) = self.instructions[idx] {
                    *addr = continue_addr;
                }
            }
        }
    }

    /// Leave the `try` regions above `depth`, innermost first: pop each
    /// installed handler and run each pending `finally` block inline.
    pub(super) fn unwind_tries(&mut self, depth: usize) {
        let saved = self.try_stack.clone();
        while self.try_stack.len() > depth {
            let region = self.try_stack.pop().unwrap();
            if region.has_handler() {
                self.instructions.push(OpCode::PopTry);
            }
            if region.part != TryPart::Finally
                && let Some(finalizer) = &region.finalizer
            {
                self.restore_env(&region.env);
                self.gen_finally_body(finalizer);
            }
        }
        self.try_stack = saved;
    }

    pub(super) fn gen_finally_body(&mut self, finalizer: &swc_ecma_ast::BlockStmt) {
        self.scope_stack.push(Vec::new());
        let env = self.enter_scope_env(finalizer.span);
        self.hoist_functions(&finalizer.stmts);
        for s in &finalizer.stmts {
            self.gen_stmt(s);
        }
        self.exit_scope_env(env);
        if let Some(locals) = self.scope_stack.pop() {
            for name in locals.into_iter().rev() {
                self.instructions.push(OpCode::Drop(name));
            }
        }
    }

    /// Define the function declarations among `stmts` up front, so they
    /// can be called before their position in the source (hoisting). The
    /// declaration itself then compiles to nothing.
    pub(super) fn hoist_functions<'a>(&mut self, stmts: impl IntoIterator<Item = &'a Stmt>) {
        for stmt in stmts {
            if let Stmt::Decl(Decl::Fn(f)) = stmt {
                self.hoist_fn(f);
            }
        }
    }

    /// Start of a function body (or the program): `var` names are bound to
    /// `undefined` (unless they are parameters), then function declarations
    /// are defined.
    pub(super) fn hoist_body(&mut self, stmts: &[Stmt], params: &[String]) {
        for ident in scope::var_declared(stmts) {
            if !params.iter().any(|p| *p == *ident.sym) {
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.store_var(&ident, true);
            }
        }
        self.hoist_functions(stmts);
    }

    pub(super) fn hoist_fn(&mut self, f: &swc_ecma_ast::FnDecl) {
        if f.declare {
            return;
        }
        self.hoisted_fns.insert(scope::key(f.function.span));
        self.gen_fn_decl(Some(&f.ident), &f.function);
    }

    pub(super) fn is_hoisted(&self, f: &swc_ecma_ast::FnDecl) -> bool {
        self.hoisted_fns.contains(&scope::key(f.function.span))
    }

    /// `break [label]` or `continue [label]`.
    pub(super) fn gen_jump_stmt(&mut self, label: Option<&Ident>, is_continue: bool) {
        let keyword = if is_continue { "continue" } else { "break" };
        let found = match label {
            Some(label) => {
                let name = label.sym.as_ref();
                match self
                    .loop_stack
                    .iter()
                    .rposition(|c| c.labels.iter().any(|l| l == name))
                {
                    Some(i) if is_continue && self.loop_stack[i].kind != TargetKind::Loop => {
                        self.syntax_error(format!("`continue {name}`: `{name}` is not a loop"));
                        return;
                    }
                    Some(i) => i,
                    None => {
                        self.syntax_error(format!(
                            "`{keyword} {name}`: no enclosing label `{name}`"
                        ));
                        return;
                    }
                }
            }
            None => match self.loop_stack.iter().rposition(|c| match c.kind {
                TargetKind::Loop => true,
                TargetKind::Switch => !is_continue,
                TargetKind::Labeled => false,
            }) {
                Some(i) => i,
                None => {
                    let place = if is_continue {
                        "a loop"
                    } else {
                        "a loop or `switch`"
                    };
                    self.syntax_error(format!("`{keyword}` outside of {place}"));
                    return;
                }
            },
        };
        let depth = self.loop_stack[found].try_depth;
        self.unwind_tries(depth);
        let env = if is_continue {
            self.loop_stack[found].continue_env.clone()
        } else {
            self.loop_stack[found].break_env.clone()
        };
        self.restore_env(&env);
        let jump_idx = self.emit_jump();
        let ctx = &mut self.loop_stack[found];
        if is_continue {
            ctx.continue_jumps.push(jump_idx);
        } else {
            ctx.break_jumps.push(jump_idx);
        }
    }

    /// Emit a jump with a placeholder target; patch it with `patch_jump`.
    pub(super) fn emit_jump(&mut self) -> usize {
        self.instructions.push(OpCode::Jump(0));
        self.instructions.len() - 1
    }

    /// Emit a conditional jump with a placeholder target.
    pub(super) fn emit_jump_if_false(&mut self) -> usize {
        self.instructions.push(OpCode::JumpIfFalse(0));
        self.instructions.len() - 1
    }

    /// Point a placeholder jump at the next instruction to be emitted.
    pub(super) fn patch_jump_here(&mut self, idx: usize) {
        let here = self.instructions.len();
        match &mut self.instructions[idx] {
            OpCode::Jump(addr) | OpCode::JumpIfFalse(addr) => *addr = here,
            other => unreachable!("patch_jump_here on non-jump {:?}", other),
        }
    }

    /// With a value on top of the stack, emit jumps taken when it is `null`
    /// or `undefined`. The value stays on the stack on every path.
    ///
    /// Uses two strict comparisons rather than `== null`, so that every
    /// backend computes the same answer.
    pub(super) fn emit_jumps_if_nullish(&mut self) -> Vec<usize> {
        let mut jumps = Vec::with_capacity(2);
        for nullish in [JsValue::Null, JsValue::Undefined] {
            self.instructions.push(OpCode::Dup);
            self.instructions.push(OpCode::Push(nullish));
            self.instructions.push(OpCode::Eq);
            self.instructions.push(OpCode::Not);
            jumps.push(self.emit_jump_if_false());
        }
        jumps
    }

    /// `a && b`, `a || b`, `a ?? b`: evaluate `b` only when `a` does not
    /// decide the result. The result is the deciding operand's value.
    pub(super) fn gen_logical(&mut self, bin: &BinExpr) {
        self.gen_expr(&bin.left);
        match bin.op {
            BinaryOp::LogicalAnd => {
                // [a] -> falsy: keep a; truthy: replace with b
                self.instructions.push(OpCode::Dup);
                let to_end = self.emit_jump_if_false();
                self.instructions.push(OpCode::Pop);
                self.gen_expr(&bin.right);
                self.patch_jump_here(to_end);
            }
            BinaryOp::LogicalOr => {
                // [a] -> truthy: keep a; falsy: replace with b
                self.instructions.push(OpCode::Dup);
                let to_rhs = self.emit_jump_if_false();
                let to_end = self.emit_jump();
                self.patch_jump_here(to_rhs);
                self.instructions.push(OpCode::Pop);
                self.gen_expr(&bin.right);
                self.patch_jump_here(to_end);
            }
            BinaryOp::NullishCoalescing => {
                // [a] -> not null/undefined: keep a; else replace with b
                let to_rhs = self.emit_jumps_if_nullish();
                let to_end = self.emit_jump();
                for j in to_rhs {
                    self.patch_jump_here(j);
                }
                self.instructions.push(OpCode::Pop);
                self.gen_expr(&bin.right);
                self.patch_jump_here(to_end);
            }
            _ => unreachable!("gen_logical called with {:?}", bin.op),
        }
    }

    /// Optional chain `a?.b.c`, `a?.[k]`, `f?.()`, `o.m?.()`.
    ///
    /// If any `?.` link sees `null`/`undefined`, the whole chain evaluates to
    /// `undefined` and the rest of it is not evaluated.
    pub(super) fn gen_opt_chain_expr(&mut self, chain: &OptChainExpr) {
        let mut to_undefined = Vec::new();
        self.gen_opt_chain(chain, &mut to_undefined);
        if to_undefined.is_empty() {
            return;
        }
        let to_end = self.emit_jump();
        for j in to_undefined {
            self.patch_jump_here(j);
        }
        // Every short-circuit jump leaves exactly the nullish value on the stack.
        self.instructions.push(OpCode::Pop);
        self.instructions.push(OpCode::Push(JsValue::Undefined));
        self.patch_jump_here(to_end);
    }

    /// Evaluate the object of a chain link. A nested `OptChain` is part of
    /// the same chain and shares its short-circuit target.
    pub(super) fn gen_opt_chain_object(&mut self, expr: &Expr, to_undefined: &mut Vec<usize>) {
        match expr {
            Expr::OptChain(inner) => self.gen_opt_chain(inner, to_undefined),
            other => self.gen_expr(other),
        }
    }

    pub(super) fn gen_opt_chain(&mut self, chain: &OptChainExpr, to_undefined: &mut Vec<usize>) {
        match chain.base.as_ref() {
            OptChainBase::Member(member) => {
                self.gen_opt_chain_object(&member.obj, to_undefined);
                if chain.optional {
                    to_undefined.extend(self.emit_jumps_if_nullish());
                }
                match &member.prop {
                    MemberProp::Ident(id) => {
                        self.instructions.push(OpCode::GetProp(id.sym.to_string()));
                    }
                    MemberProp::Computed(computed) => {
                        self.gen_expr(&computed.expr);
                        self.instructions.push(OpCode::GetPropComputed);
                    }
                    MemberProp::PrivateName(_) => {
                        self.unsupported("optional chaining on a private field (`?.#x`)");
                    }
                }
            }
            OptChainBase::Call(call) => {
                // Temporaries are named after the instruction index, which is
                // unique, and use a character no identifier can contain.
                let tmp = format!("%optcall{}", self.instructions.len());

                // A method callee (`o.m?.()` or `o?.m()`) keeps `o` as `this`.
                let method = match call.callee.as_ref() {
                    Expr::Member(m) => Some((m, false)),
                    Expr::OptChain(c) => match c.base.as_ref() {
                        OptChainBase::Member(m) => Some((m, c.optional)),
                        _ => None,
                    },
                    _ => None,
                };

                match method {
                    Some((member, obj_optional)) if matches!(member.prop, MemberProp::Ident(_)) => {
                        let MemberProp::Ident(name) = &member.prop else {
                            unreachable!()
                        };
                        let name = name.sym.to_string();
                        self.gen_opt_chain_object(&member.obj, to_undefined);
                        if obj_optional {
                            to_undefined.extend(self.emit_jumps_if_nullish());
                        }
                        self.instructions.push(OpCode::Let(tmp.clone()));
                        if chain.optional {
                            self.instructions.push(OpCode::Load(tmp.clone()));
                            self.instructions.push(OpCode::GetProp(name.clone()));
                            to_undefined.extend(self.emit_jumps_if_nullish());
                            self.instructions.push(OpCode::Pop);
                        }
                        for arg in &call.args {
                            self.gen_expr(&arg.expr);
                        }
                        self.instructions.push(OpCode::Load(tmp));
                        self.instructions
                            .push(OpCode::CallMethod(name, call.args.len()));
                    }
                    _ => {
                        self.gen_opt_chain_object(&call.callee, to_undefined);
                        if chain.optional {
                            to_undefined.extend(self.emit_jumps_if_nullish());
                        }
                        self.instructions.push(OpCode::Let(tmp.clone()));
                        for arg in &call.args {
                            self.gen_expr(&arg.expr);
                        }
                        self.instructions.push(OpCode::Load(tmp));
                        self.instructions.push(OpCode::Call(call.args.len()));
                    }
                }
            }
        }
    }
}
