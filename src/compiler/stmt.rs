//! Statements.

use super::*;

impl Codegen {
    pub(super) fn gen_stmt(&mut self, stmt: &Stmt) {
        // `declare ...` describes something defined elsewhere: no code.
        if let Stmt::Decl(d) = stmt
            && scope::is_ambient(d)
        {
            return;
        }
        let saved = std::mem::replace(&mut self.current_span, stmt.span());
        let saved_mark = std::mem::replace(&mut self.mark_span, stmt.span());
        self.mark(stmt.span());
        self.gen_stmt_inner(stmt);
        self.current_span = saved;
        self.mark_span = saved_mark;
        self.mark(saved_mark);
    }

    /// Code from here on comes from `span` (until the next mark); a dummy
    /// span is no position (byte 0 precedes every file).
    pub(super) fn mark(&mut self, span: Span) {
        let at = self.instructions.len();
        match self.marks.last_mut() {
            Some(last) if last.0 == at => last.1 = span.lo.0,
            _ => self.marks.push((at, span.lo.0)),
        }
    }

    pub(super) fn gen_stmt_inner(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return(ret_stmt) => {
                if let Some(arg) = &ret_stmt.arg {
                    self.gen_expr(arg); // Pushes the return value to stack
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Undefined));
                }
                // Leaving every `try` in this function: pop the handlers and
                // run the `finally` blocks, with the return value on the stack.
                self.unwind_tries(0);
                if self.in_async_function {
                    self.emit_async_return();
                }
                self.instructions.push(OpCode::Return);
            }
            // RECURSION: Handle the Block
            Stmt::Block(block) => {
                let env = self.enter_scope_env(block.span);
                self.scope_stack.push(Vec::new()); // Enter new scope
                self.hoist_functions(&block.stmts);
                for s in &block.stmts {
                    self.gen_stmt(s);
                }
                // Exit scope: Drop variables
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
                self.exit_scope_env(env);
            }
            Stmt::Decl(Decl::Var(var_decl)) => {
                self.gen_var_decl(var_decl);
            }
            Stmt::Decl(Decl::Fn(fn_decl)) => {
                if !self.is_hoisted(fn_decl) {
                    self.gen_fn_decl(Some(&fn_decl.ident), &fn_decl.function);
                }
            }
            Stmt::Decl(Decl::Class(class_decl)) => {
                let class_name = class_decl.ident.sym.to_string();
                self.gen_class(&class_decl.class, Some(class_name.as_str()));
                self.store_var(&class_decl.ident, true);
            }
            Stmt::Decl(Decl::TsEnum(enum_decl)) => {
                // Generate enum as a runtime object at statement level
                let enum_name = enum_decl.id.sym.to_string();

                // Create new object
                self.instructions.push(OpCode::NewObject);

                // Add each enum member as a property
                for member in &enum_decl.members {
                    // Duplicate the object reference for each property
                    self.instructions.push(OpCode::Dup);

                    // Get the member name
                    let member_name = match &member.id {
                        swc_ecma_ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
                        swc_ecma_ast::TsEnumMemberId::Str(s) => s
                            .value
                            .as_str()
                            .expect("Invalid string enum member")
                            .to_string(),
                    };

                    // Push the value
                    match &member.init {
                        Some(expr) => {
                            // Compute the value
                            self.gen_expr(expr);
                        }
                        None => {
                            // Auto-incrementing number for unvalued variants
                            let idx = enum_decl
                                .members
                                .iter()
                                .position(|m| m.id == member.id)
                                .unwrap_or(0);
                            self.instructions
                                .push(OpCode::Push(JsValue::Number(idx as f64)));
                        }
                    }

                    // Set the property
                    self.instructions.push(OpCode::SetProp(member_name));
                }

                // Store the enum object in a variable
                self.instructions.push(OpCode::Let(enum_name.clone()));
            }
            Stmt::Decl(Decl::TsModule(_)) => {
                // TypeScript modules are compile-time only, skip at runtime
            }
            Stmt::Decl(Decl::TsInterface(_)) => {
                // Interfaces are compile-time only, skip at runtime
            }
            Stmt::Decl(Decl::TsTypeAlias(_)) => {
                // Type aliases are compile-time only, skip at runtime
            }
            Stmt::Expr(expr_stmt) => {
                self.gen_expr(&expr_stmt.expr);
                // Expression statements (e.g. `foo();`) should always discard their result in JS.
                // This is critical for proper stack management - without this Pop, values from
                // expression statements (like assignments) accumulate on the stack and corrupt
                // the stack state when calling functions from within object literals or other
                // expressions that expect the stack to be clean.
                self.instructions.push(OpCode::Pop);
            }
            Stmt::While(while_stmt) => {
                let loop_env = self.save_env();
                let iter_env = loop_env.clone();
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                self.gen_expr(&while_stmt.test);
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.gen_stmt(&while_stmt.body);
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                self.pop_target(loop_end, loop_start);
            }
            Stmt::Break(brk) => self.gen_jump_stmt(brk.label.as_ref(), false),
            Stmt::Continue(cont) => self.gen_jump_stmt(cont.label.as_ref(), true),
            Stmt::If(if_stmt) => {
                self.gen_expr(&if_stmt.test);
                let else_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.gen_stmt(&if_stmt.cons);
                let has_else = if_stmt.alt.is_some();
                let end_jump_idx = if has_else {
                    Some(self.instructions.len())
                } else {
                    None
                };
                if has_else {
                    self.instructions.push(OpCode::Jump(0));
                }
                let else_start = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[else_jump_idx] {
                    *addr = else_start;
                }
                if let Some(alt) = &if_stmt.alt {
                    self.gen_stmt(alt);
                }
                if let Some(end_idx) = end_jump_idx {
                    let end_addr = self.instructions.len();
                    if let OpCode::Jump(ref mut addr) = self.instructions[end_idx] {
                        *addr = end_addr;
                    }
                }
            }
            Stmt::For(for_stmt) => {
                self.scope_stack.push(Vec::new());
                // `let` bindings of the loop head that closures capture get a
                // fresh environment per iteration, copied from the previous
                // one, as JavaScript specifies.
                let loop_env = self.save_env();
                let head: Option<Vec<String>> =
                    self.scopes.env_scope(for_stmt.span).map(|n| n.to_vec());
                if head.is_some() {
                    self.emit_child_env();
                    self.instructions
                        .push(OpCode::Store(SCOPE_LOCAL.to_string()));
                }
                if let Some(init) = &for_stmt.init {
                    match init {
                        swc_ecma_ast::VarDeclOrExpr::VarDecl(var_decl) => {
                            self.gen_var_decl(var_decl);
                        }
                        swc_ecma_ast::VarDeclOrExpr::Expr(expr) => {
                            self.gen_expr(expr);
                            self.instructions.push(OpCode::Pop);
                        }
                    }
                }
                let iter_env = match &head {
                    Some(names) => {
                        self.emit_iteration_env(names);
                        let save = format!("%iter{}", self.instructions.len());
                        self.instructions
                            .push(OpCode::Load(SCOPE_LOCAL.to_string()));
                        self.instructions.push(OpCode::Let(save.clone()));
                        Some(save)
                    }
                    None => loop_env.clone(),
                };
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                if let Some(test) = &for_stmt.test {
                    self.gen_expr(test);
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
                }
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.gen_stmt(&for_stmt.body);
                let continue_target = self.instructions.len();
                if let (Some(names), Some(save)) = (&head, &iter_env) {
                    self.emit_iteration_env(names);
                    self.instructions
                        .push(OpCode::Load(SCOPE_LOCAL.to_string()));
                    self.instructions.push(OpCode::Store(save.clone()));
                }
                if let Some(update) = &for_stmt.update {
                    self.gen_expr(update);
                    self.instructions.push(OpCode::Pop);
                }
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                self.pop_target(loop_end, continue_target);
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::ForOf(for_of_stmt) => {
                self.scope_stack.push(Vec::new());
                let loop_env = self.save_env();
                let iter_env = loop_env.clone();
                let per_iteration = self.scopes.env_scope(for_of_stmt.span).is_some();
                self.gen_expr(&for_of_stmt.right);
                // Unique names, so nested loops do not share them.
                let iter_name = format!("%forof_iter{}", self.instructions.len());
                self.instructions.push(OpCode::Let(iter_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(iter_name.clone());
                }
                self.instructions.push(OpCode::Push(JsValue::Number(0.0)));
                let idx_name = format!("%forof_idx{}", self.instructions.len());
                self.instructions.push(OpCode::Let(idx_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(idx_name.clone());
                }
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::Load(iter_name.clone()));
                self.instructions
                    .push(OpCode::GetProp("length".to_string()));
                self.instructions.push(OpCode::Lt);
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.instructions.push(OpCode::Load(iter_name.clone()));
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::LoadElement);
                if per_iteration {
                    // A fresh environment for this iteration's binding.
                    self.restore_env(&loop_env);
                    self.emit_child_env();
                    self.instructions
                        .push(OpCode::Store(SCOPE_LOCAL.to_string()));
                }
                if let Some(var_decl) = &for_of_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    self.store_var(&id.id, true);
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.push(id.id.sym.to_string());
                    }
                } else if let ForHead::Pat(pat) = &for_of_stmt.left
                    && let Pat::Ident(id) = pat.as_ref()
                {
                    self.store_var(&id.id, false);
                } else {
                    self.unsupported("this `for...of` binding");
                }
                self.gen_stmt(&for_of_stmt.body);
                let continue_target = self.instructions.len();
                if let Some(var_decl) = &for_of_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    let var_name = id.id.sym.to_string();
                    self.instructions.push(OpCode::Drop(var_name));
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.retain(|n| n != &id.id.sym.to_string());
                    }
                }
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                self.instructions.push(OpCode::Add);
                self.instructions.push(OpCode::Store(idx_name.clone()));
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                self.pop_target(loop_end, continue_target);
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::ForIn(for_in_stmt) => {
                self.scope_stack.push(Vec::new());
                let loop_env = self.save_env();
                let iter_env = loop_env.clone();
                let per_iteration = self.scopes.env_scope(for_in_stmt.span).is_some();
                self.gen_expr(&for_in_stmt.right);
                self.instructions.push(OpCode::Load("Object".to_string()));
                self.instructions.push(OpCode::GetProp("keys".to_string()));
                self.instructions.push(OpCode::Call(1));
                let keys_name = format!("%forin_keys{}", self.instructions.len());
                self.instructions.push(OpCode::Let(keys_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(keys_name.clone());
                }
                self.instructions.push(OpCode::Push(JsValue::Number(0.0)));
                let idx_name = format!("%forin_idx{}", self.instructions.len());
                self.instructions.push(OpCode::Let(idx_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(idx_name.clone());
                }
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::Load(keys_name.clone()));
                self.instructions
                    .push(OpCode::GetProp("length".to_string()));
                self.instructions.push(OpCode::Lt);
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.instructions.push(OpCode::Load(keys_name.clone()));
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::LoadElement);
                if per_iteration {
                    self.restore_env(&loop_env);
                    self.emit_child_env();
                    self.instructions
                        .push(OpCode::Store(SCOPE_LOCAL.to_string()));
                }
                if let Some(var_decl) = &for_in_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    self.store_var(&id.id, true);
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.push(id.id.sym.to_string());
                    }
                } else if let ForHead::Pat(pat) = &for_in_stmt.left
                    && let Pat::Ident(id) = pat.as_ref()
                {
                    self.store_var(&id.id, false);
                } else {
                    self.unsupported("this `for...in` binding");
                }
                self.gen_stmt(&for_in_stmt.body);
                let continue_target = self.instructions.len();
                if let Some(var_decl) = &for_in_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    let var_name = id.id.sym.to_string();
                    self.instructions.push(OpCode::Drop(var_name));
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.retain(|n| n != &id.id.sym.to_string());
                    }
                }
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                self.instructions.push(OpCode::Add);
                self.instructions.push(OpCode::Store(idx_name.clone()));
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                self.pop_target(loop_end, continue_target);
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::DoWhile(do_while_stmt) => {
                let loop_env = self.save_env();
                let iter_env = loop_env.clone();
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                self.gen_stmt(&do_while_stmt.body);
                let continue_target = self.instructions.len();
                self.gen_expr(&do_while_stmt.test);
                self.instructions
                    .push(OpCode::JumpIfFalse(self.instructions.len() + 2));
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                self.pop_target(loop_end, continue_target);
            }
            Stmt::With(_) => self.unsupported("`with` statements"),
            Stmt::Empty(_) | Stmt::Debugger(_) => {
                // Empty/debugger/with statements - do nothing
            }
            Stmt::Try(try_stmt) => {
                // Try-catch-finally statement
                //
                // Control flow:
                // - try-catch-finally: try -> PopTry -> finally -> end; on exception -> catch -> finally -> end
                // - try-catch: try -> PopTry -> end; on exception -> catch -> end
                // - try-finally: try -> PopTry -> finally -> end; on exception -> finally -> rethrow

                let has_catch = try_stmt.handler.is_some();
                let has_finally = try_stmt.finalizer.is_some();

                // An exception can arrive from inside a nested environment;
                // the handlers restore `%scope` from here.
                let try_env = self.save_env();

                // 1. Emit SetupTry with placeholder addresses (will backpatch)
                let setup_try_idx = self.instructions.len();
                self.instructions.push(OpCode::SetupTry {
                    catch_addr: 0,
                    finally_addr: 0,
                });

                // 2. Emit try block
                self.try_stack.push(TryRegion {
                    part: TryPart::Try,
                    env: try_env.clone(),
                    finalizer: try_stmt.finalizer.clone(),
                });
                let block_env = self.enter_scope_env(try_stmt.block.span);
                self.scope_stack.push(Vec::new());
                self.hoist_functions(&try_stmt.block.stmts);
                for s in &try_stmt.block.stmts {
                    self.gen_stmt(s);
                }
                self.exit_scope_env(block_env);
                self.try_stack.pop();
                // Drop try block scope variables
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }

                // 3. PopTry - remove exception handler if no exception occurred
                self.instructions.push(OpCode::PopTry);

                // 4. Jump after try block (target depends on structure)
                let jump_after_try_idx = self.instructions.len();
                self.instructions.push(OpCode::Jump(0)); // Will backpatch

                // 5. Catch block (if present)
                let catch_addr = if has_catch {
                    let addr = self.instructions.len();
                    if let Some(handler) = &try_stmt.handler {
                        self.scope_stack.push(Vec::new());
                        // [exception]
                        self.restore_env(&try_env);
                        let catch_env = self.enter_scope_env(handler.span);

                        // Bind exception to catch parameter if present
                        if let Some(param) = &handler.param {
                            if let Pat::Ident(id) = param {
                                // Exception value is on stack, bind it
                                self.store_var(&id.id, true);
                                if let Some(scope) = self.scope_stack.last_mut() {
                                    scope.push(id.id.sym.to_string());
                                }
                            } else {
                                self.unsupported("destructuring in a `catch` parameter");
                            }
                        } else {
                            // No catch parameter, pop the exception
                            self.instructions.push(OpCode::Pop);
                        }

                        // Generate catch block body
                        self.try_stack.push(TryRegion {
                            part: TryPart::Catch,
                            env: try_env.clone(),
                            finalizer: try_stmt.finalizer.clone(),
                        });
                        self.hoist_functions(&handler.body.stmts);
                        for s in &handler.body.stmts {
                            self.gen_stmt(s);
                        }
                        self.try_stack.pop();
                        self.exit_scope_env(catch_env);

                        // Drop catch block scope variables
                        if let Some(locals) = self.scope_stack.pop() {
                            for name in locals.into_iter().rev() {
                                self.instructions.push(OpCode::Drop(name));
                            }
                        }

                        // The VM re-arms a finally-only handler when it
                        // dispatches to catch; pop it if the catch completes
                        // without throwing (it then falls through to finally).
                        if has_finally {
                            self.instructions.push(OpCode::PopTry);
                        }
                    }
                    addr
                } else {
                    0 // No catch block
                };

                // 6. Finally block (if present)
                let finally_addr = if has_finally {
                    let addr = self.instructions.len();
                    if let Some(finalizer) = &try_stmt.finalizer {
                        self.restore_env(&try_env);
                        self.try_stack.push(TryRegion {
                            part: TryPart::Finally,
                            env: try_env.clone(),
                            finalizer: None,
                        });
                        self.gen_finally_body(finalizer);
                        self.try_stack.pop();
                    }
                    // Re-throw a pending exception (throw with no catch, or a
                    // throw from inside catch) once the finally has run.
                    self.instructions.push(OpCode::EnterFinally(true));
                    addr
                } else {
                    0 // No finally block
                };

                // 7. End address - where normal execution continues
                let end_addr = self.instructions.len();

                // Backpatch SetupTry addresses
                if let OpCode::SetupTry {
                    catch_addr: ref mut c,
                    finally_addr: ref mut f,
                } = self.instructions[setup_try_idx]
                {
                    *c = catch_addr;
                    *f = finally_addr;
                }

                // Backpatch jump after try block
                // - If has finally: jump to finally
                // - If no finally: jump to end
                if let OpCode::Jump(ref mut addr) = self.instructions[jump_after_try_idx] {
                    *addr = if has_finally { finally_addr } else { end_addr };
                }
            }
            Stmt::Throw(throw_stmt) => {
                // Throw statement: push exception value and emit Throw opcode
                self.gen_expr(&throw_stmt.arg);
                // `throw new Error(...)` is located where the error is
                // made, as JavaScript stack traces are.
                if let Expr::New(new) = throw_stmt.arg.as_ref() {
                    self.mark(new.span);
                }
                self.instructions.push(OpCode::Throw);
            }
            Stmt::Switch(switch) => self.gen_switch(switch),
            Stmt::Labeled(labeled) => {
                let name = labeled.label.sym.to_string();
                if self.loop_stack.iter().any(|c| c.labels.contains(&name))
                    || self.pending_labels.contains(&name)
                {
                    self.syntax_error(format!("label `{name}` is already declared"));
                    return;
                }
                let mut body = labeled.body.as_ref();
                while let Stmt::Labeled(inner) = body {
                    body = inner.body.as_ref();
                }
                let is_loop = matches!(
                    body,
                    Stmt::For(_)
                        | Stmt::ForIn(_)
                        | Stmt::ForOf(_)
                        | Stmt::While(_)
                        | Stmt::DoWhile(_)
                );
                if is_loop {
                    // The loop takes the label: `continue name` reaches it.
                    self.pending_labels.push(name);
                    self.gen_stmt(&labeled.body);
                } else {
                    let mut labels = std::mem::take(&mut self.pending_labels);
                    labels.push(name);
                    let break_env = self.save_env();
                    self.loop_stack.push(LoopContext {
                        kind: TargetKind::Labeled,
                        labels,
                        break_jumps: Vec::new(),
                        continue_jumps: Vec::new(),
                        break_env,
                        continue_env: None,
                        try_depth: self.try_stack.len(),
                    });
                    self.gen_stmt(&labeled.body);
                    let end = self.instructions.len();
                    self.pop_target(end, end);
                }
            }
            Stmt::Decl(Decl::Using(_)) => self.unsupported("`using` declarations"),
        }
    }

    /// `switch`: the discriminant goes in a temporary; each `case` test is
    /// compared with `===` in source order, and the first match (or else
    /// `default`) jumps into the run of case bodies, which fall through.
    pub(super) fn gen_switch(&mut self, switch: &swc_ecma_ast::SwitchStmt) {
        self.gen_expr(&switch.discriminant);
        let disc = format!("%switch{}", self.instructions.len());
        self.instructions.push(OpCode::Let(disc.clone()));
        self.scope_stack.push(vec![disc.clone()]);

        // The case bodies share one block scope.
        let break_env = self.save_env();
        if self.scopes.env_scope(switch.span).is_some() {
            self.emit_child_env();
            self.instructions
                .push(OpCode::Store(SCOPE_LOCAL.to_string()));
        }
        self.hoist_functions(switch.cases.iter().flat_map(|c| c.cons.iter()));
        self.loop_stack.push(LoopContext {
            kind: TargetKind::Switch,
            labels: Vec::new(),
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            break_env: break_env.clone(),
            continue_env: None,
            try_depth: self.try_stack.len(),
        });

        let mut matched_jumps = Vec::new();
        for case in &switch.cases {
            if let Some(test) = &case.test {
                self.instructions.push(OpCode::Load(disc.clone()));
                self.gen_expr(test);
                self.instructions.push(OpCode::Eq);
                let skip = self.emit_jump_if_false();
                matched_jumps.push(Some(self.emit_jump()));
                self.patch_jump_here(skip);
            } else {
                matched_jumps.push(None);
            }
        }
        let no_match = self.emit_jump();

        let mut default_addr = None;
        for (case, jump) in switch.cases.iter().zip(&matched_jumps) {
            match jump {
                Some(idx) => self.patch_jump_here(*idx),
                None => default_addr = Some(self.instructions.len()),
            }
            for s in &case.cons {
                self.gen_stmt(s);
            }
        }
        let end = self.instructions.len();
        if let OpCode::Jump(ref mut addr) = self.instructions[no_match] {
            *addr = default_addr.unwrap_or(end);
        }
        self.restore_env(&break_env);
        self.pop_target(end, end);
        if let Some(locals) = self.scope_stack.pop() {
            for name in locals.into_iter().rev() {
                self.instructions.push(OpCode::Drop(name));
            }
        }
    }
}
