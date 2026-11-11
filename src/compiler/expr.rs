//! Expressions.

use super::*;

impl Codegen {
    pub(super) fn gen_expr(&mut self, expr: &Expr) {
        let saved = std::mem::replace(&mut self.current_span, expr.span());
        // A property access is located at the property (`o.x.y`: `y`), a
        // call at its callee's property, as in V8's stack traces.
        let at = match expr {
            Expr::Member(m) => m.prop.span(),
            Expr::Call(CallExpr {
                callee: Callee::Expr(callee),
                ..
            }) => match callee.as_ref() {
                Expr::Member(m) => m.prop.span(),
                _ => expr.span(),
            },
            _ => expr.span(),
        };
        let saved_mark = std::mem::replace(&mut self.mark_span, at);
        self.mark(at);
        self.gen_expr_inner(expr);
        self.emit_guard(expr.span());
        self.current_span = saved;
        self.mark_span = saved_mark;
        self.mark(saved_mark);
    }

    /// `[value] -> [value]`: if the type checker asked for the value at
    /// `span` to be checked when the program runs, check it.
    pub(super) fn emit_guard(&mut self, span: Span) {
        let Some(&kinds) = self.guards.get(&(span.lo.0, span.hi.0)) else {
            return;
        };
        let check = intrinsics::id("check_type").expect("the check_type intrinsic");
        self.instructions
            .push(OpCode::Push(JsValue::Number(kinds as f64)));
        self.instructions.push(OpCode::Intrinsic(check, 2));
    }

    pub(super) fn gen_expr_inner(&mut self, expr: &Expr) {
        match expr {
            Expr::Fn(fn_expr) => {
                // `function (a, b) { ... }`: a function value. A name is
                // bound for the function's own body; references to it are
                // always from inside, so it lives in an environment.
                if let Some(ident) = &fn_expr.ident {
                    self.next_fn_name = Some(ident.sym.to_string());
                }
                match &fn_expr.ident {
                    Some(ident) if self.scopes.env_scope(ident.span).is_some() => {
                        let env = self.enter_scope_env(ident.span);
                        self.gen_fn_decl(None, &fn_expr.function);
                        self.instructions.push(OpCode::Dup);
                        self.store_var(ident, true);
                        self.exit_scope_env(env);
                    }
                    _ => self.gen_fn_decl(None, &fn_expr.function),
                }
            }
            Expr::Arrow(arrow) => {
                // `(a, b) => expr` or `(a, b) => { ... }`; an expression body
                // is `{ return expr; }`.
                let value_idx = self.emit_function_value(arrow.span, 0);
                let jump_idx = self.emit_jump();
                let body_start = self.instructions.len();
                self.patch_function_value(value_idx, body_start);
                let pats: Vec<&Pat> = arrow.params.iter().collect();
                let owned;
                let stmts: &[Stmt] = match &*arrow.body {
                    BlockStmtOrExpr::BlockStmt(block) => &block.stmts,
                    BlockStmtOrExpr::Expr(e) => {
                        owned = [Stmt::Return(ReturnStmt {
                            span: e.span(),
                            arg: Some(e.clone()),
                        })];
                        &owned
                    }
                };
                self.gen_function_body(
                    arrow.span,
                    &pats,
                    stmts,
                    arrow.is_async,
                    FnKind::Plain,
                    &[],
                );
                self.patch_jump_here(jump_idx);
            }
            Expr::Lit(Lit::Num(num)) => {
                self.instructions
                    .push(OpCode::Push(JsValue::Number(num.value)));
            }
            Expr::Lit(Lit::Str(s)) => {
                self.instructions.push(OpCode::Push(JsValue::String(
                    s.value.to_string_lossy().to_string(),
                )));
            }
            Expr::Lit(Lit::Bool(b)) => {
                self.instructions
                    .push(OpCode::Push(JsValue::Boolean(b.value)));
            }
            Expr::Lit(Lit::Null(_)) => {
                self.instructions.push(OpCode::Push(JsValue::Null));
            }
            Expr::Ident(id) => {
                self.load_var(id);
            }
            Expr::Bin(bin)
                if matches!(
                    bin.op,
                    BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
                ) =>
            {
                self.gen_logical(bin);
            }
            Expr::OptChain(chain) => {
                self.gen_opt_chain_expr(chain);
            }
            Expr::Bin(bin) if bin.op == BinaryOp::In => {
                // `key in obj`; `#x in obj` asks for the private member.
                match bin.left.as_ref() {
                    Expr::PrivateName(pn) => {
                        let key = self.private_key(&pn.name);
                        self.instructions.push(OpCode::Push(JsValue::String(key)));
                    }
                    left => self.gen_expr(left),
                }
                self.gen_expr(&bin.right);
                self.instructions.push(OpCode::In);
            }
            Expr::Bin(bin) => {
                self.gen_expr(&bin.left);
                self.gen_expr(&bin.right);
                match bin.op {
                    BinaryOp::Add => self.instructions.push(OpCode::Add),
                    BinaryOp::Sub => self.instructions.push(OpCode::Sub),
                    BinaryOp::Mul => self.instructions.push(OpCode::Mul),
                    BinaryOp::Div => self.instructions.push(OpCode::Div),
                    BinaryOp::Mod => self.instructions.push(OpCode::Mod),
                    BinaryOp::EqEq => self.instructions.push(OpCode::EqEq), // == (loose equality)
                    BinaryOp::EqEqEq => self.instructions.push(OpCode::Eq), // === (strict equality)
                    BinaryOp::NotEq => self.instructions.push(OpCode::NeEq), // != (loose inequality)
                    BinaryOp::NotEqEq => self.instructions.push(OpCode::Ne), // !== (strict inequality)
                    BinaryOp::Lt => self.instructions.push(OpCode::Lt),
                    BinaryOp::LtEq => self.instructions.push(OpCode::LtEq),
                    BinaryOp::Gt => self.instructions.push(OpCode::Gt),
                    BinaryOp::GtEq => self.instructions.push(OpCode::GtEq),
                    BinaryOp::InstanceOf => self.instructions.push(OpCode::InstanceOf),
                    // Bitwise operators
                    BinaryOp::BitAnd => self.instructions.push(OpCode::BitAnd),
                    BinaryOp::BitOr => self.instructions.push(OpCode::BitOr),
                    BinaryOp::BitXor => self.instructions.push(OpCode::Xor),
                    BinaryOp::LShift => self.instructions.push(OpCode::ShiftLeft),
                    BinaryOp::RShift => self.instructions.push(OpCode::ShiftRight),
                    BinaryOp::ZeroFillRShift => self.instructions.push(OpCode::ShiftRightUnsigned),
                    BinaryOp::Exp => self.instructions.push(OpCode::Pow),
                    other => self.unsupported(format!("binary operator `{}`", other.as_str())),
                }
            }
            Expr::Unary(unary) => {
                match unary.op {
                    UnaryOp::TypeOf => {
                        if let Expr::Ident(id) = unary.arg.as_ref()
                            && self.scopes.var(id.span) == VarRef::Plain
                        {
                            let name = self.var_name(id);
                            self.instructions.push(OpCode::TypeOfName(name));
                        } else {
                            self.gen_expr(&unary.arg);
                            self.instructions.push(OpCode::TypeOf);
                        }
                    }
                    UnaryOp::Delete => {
                        // delete operator - handle member expressions specially
                        if let Expr::Member(member) = unary.arg.as_ref() {
                            self.gen_expr(&member.obj);
                            if let MemberProp::Ident(id) = &member.prop {
                                self.instructions.push(OpCode::Delete(id.sym.to_string()));
                            } else {
                                // Computed property - evaluate and discard, return true
                                self.instructions.push(OpCode::Pop);
                                self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
                            }
                        } else {
                            // delete on non-member always returns true (but evaluates the expr)
                            self.gen_expr(&unary.arg);
                            self.instructions.push(OpCode::Pop);
                            self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
                        }
                    }
                    _ => {
                        self.gen_expr(&unary.arg);
                        match unary.op {
                            UnaryOp::Bang => self.instructions.push(OpCode::Not),
                            UnaryOp::Minus => self.instructions.push(OpCode::Neg),
                            UnaryOp::Plus => {} // +x is a no-op for numbers
                            UnaryOp::Tilde => {
                                // Bitwise NOT: ~x = -(x+1) approximately, or convert to i32 and flip bits
                                // For now, implement as: push -1, xor
                                self.instructions.push(OpCode::Push(JsValue::Number(-1.0)));
                                self.instructions.push(OpCode::Xor);
                            }
                            UnaryOp::Void => {
                                // void expr - evaluate and discard, push undefined
                                self.instructions.push(OpCode::Pop);
                                self.instructions.push(OpCode::Push(JsValue::Undefined));
                            }
                            other => {
                                self.unsupported(format!("unary operator `{}`", other.as_str()))
                            }
                        }
                    }
                }
            }
            Expr::Array(arr_lit) => {
                // Check if any elements are spread elements
                let has_spread = arr_lit
                    .elems
                    .iter()
                    .any(|elem| elem.as_ref().is_some_and(|e| e.spread.is_some()));

                if has_spread {
                    // Use dynamic approach: create empty array, then push/spread each element
                    self.instructions.push(OpCode::NewArray(0));
                    for expr_or_spread in arr_lit.elems.iter().flatten() {
                        self.gen_expr(&expr_or_spread.expr);
                        if expr_or_spread.spread.is_some() {
                            // Spread: Stack is [array, source_array]
                            self.instructions.push(OpCode::ArraySpread);
                        } else {
                            // Regular element: Stack is [array, value]
                            self.instructions.push(OpCode::ArrayPush);
                        }
                    }
                } else {
                    // Use static approach: create array with known size
                    let size = arr_lit.elems.len();
                    self.instructions.push(OpCode::NewArray(size));

                    for (i, elem) in arr_lit.elems.iter().enumerate() {
                        if let Some(expr_or_spread) = elem {
                            // 1. Dup the array pointer so we don't lose it
                            self.instructions.push(OpCode::Dup);
                            // 2. Push the Value
                            self.gen_expr(&expr_or_spread.expr);
                            // 3. Push the Index
                            self.instructions
                                .push(OpCode::Push(JsValue::Number(i as f64)));
                            // 4. Store it
                            self.instructions.push(OpCode::StoreElement);
                        }
                    }
                }
            }
            Expr::Call(call_expr) => {
                // `obj.name(args)`: a method call with `this = obj`. Only for
                // an identifier property: for `obj[k](args)` this used to
                // emit the arguments and object, then fall through and emit
                // them again, leaving junk on the stack.
                if let Callee::Expr(callee_expr) = &call_expr.callee
                    && let Expr::Member(member) = callee_expr.as_ref()
                    && let Some(key) = self.member_key(&member.prop)
                {
                    for arg in &call_expr.args {
                        self.gen_expr(&arg.expr);
                    }
                    self.gen_expr(&member.obj);
                    self.instructions
                        .push(OpCode::CallMethod(key, call_expr.args.len()));
                    return;
                }
                // `super.name(args)`: the parent's method, with this `this`.
                if let Callee::Expr(callee_expr) = &call_expr.callee
                    && let Expr::SuperProp(sp) = callee_expr.as_ref()
                {
                    for arg in &call_expr.args {
                        self.gen_expr(&arg.expr);
                    }
                    self.gen_super_prop(sp);
                    self.instructions.push(OpCode::LoadThis);
                    self.instructions
                        .push(OpCode::CallWithThis(call_expr.args.len()));
                    return;
                }
                // `super(args)`: the parent constructor runs on `this`; then
                // this class's instance fields are initialized.
                if let Callee::Super(sup) = &call_expr.callee {
                    for arg in &call_expr.args {
                        self.gen_expr(&arg.expr);
                    }
                    self.load_var(&scope::super_ident(sup.span));
                    self.instructions
                        .push(OpCode::CallSuper(call_expr.args.len()));
                    if let Some(fields) = self.class_fields.clone() {
                        self.emit_field_inits(&fields);
                    }
                    return;
                }
                // `__intrinsic_<name>(args)`: an operation core provides.
                if let Callee::Expr(expr) = &call_expr.callee
                    && let Expr::Ident(id) = expr.as_ref()
                    && let Some(name) = id.sym.strip_prefix(intrinsics::PREFIX)
                {
                    let Some(intrinsic) = intrinsics::id(name) else {
                        let message = format!("unknown intrinsic '{}'", id.sym);
                        self.errors.push((id.span, message));
                        return;
                    };
                    for arg in &call_expr.args {
                        if arg.spread.is_some() {
                            let message = format!("'{}' takes no spread arguments", id.sym);
                            self.errors.push((arg.expr.span(), message));
                        }
                        self.gen_expr(&arg.expr);
                    }
                    self.instructions
                        .push(OpCode::Intrinsic(intrinsic, call_expr.args.len()));
                    return;
                }
                // detect if this is a 'require' call
                if let Callee::Expr(expr) = &call_expr.callee
                    && let Expr::Ident(id) = expr.as_ref()
                    && id.sym == "require"
                    && let Some(arg) = call_expr.args.first()
                {
                    self.gen_expr(&arg.expr);
                    self.instructions.push(OpCode::Require);
                    return;
                }

                let arg_count = call_expr.args.len();
                for arg in &call_expr.args {
                    self.gen_expr(&arg.expr);
                }
                match &call_expr.callee {
                    Callee::Expr(expr) => self.gen_expr(expr),
                    Callee::Super(_) => unreachable!("handled above"),
                    Callee::Import(_) => self.unsupported("dynamic `import()`"),
                }
                self.instructions.push(OpCode::Call(arg_count));
            }
            Expr::Assign(assign_expr) if assign_expr.op != AssignOp::Assign => {
                self.gen_compound_assign(assign_expr);
            }
            Expr::Assign(assign_expr) => {
                // Handle different assignment targets
                match &assign_expr.left {
                    AssignTarget::Simple(simple) => match simple {
                        SimpleAssignTarget::Ident(binding_ident) => {
                            // Simple variable assignment: x = value
                            self.gen_expr(&assign_expr.right);
                            // JS assignment expressions evaluate to the assigned value.
                            // The store consumes the value, so `Dup` keeps one copy
                            // on the stack for expression context (e.g. `a = 1;`).
                            self.instructions.push(OpCode::Dup);
                            self.store_var(&binding_ident.id, false);
                        }
                        SimpleAssignTarget::Member(member_expr) => {
                            // Member assignment: obj.prop = value or this.prop = value
                            // Stack order for SetProp: [object, value] -> pops both, sets prop
                            self.gen_expr(&member_expr.obj); // Push the object
                            self.gen_expr(&assign_expr.right); // Push the value

                            // The store ops consume the object and value and
                            // push nothing, but an assignment expression
                            // evaluates to the value (the statement's `Pop`
                            // removes it). Keep a copy underneath:
                            // [obj, v] -> Dup -> [obj, v, v] -> Swap3 ->
                            // [v, v, obj] -> Swap -> [v, obj, v].
                            self.instructions.push(OpCode::Dup);
                            self.instructions.push(OpCode::Swap3);
                            self.instructions.push(OpCode::Swap);

                            match &member_expr.prop {
                                MemberProp::Ident(id) => {
                                    // obj.prop = value
                                    self.instructions.push(OpCode::SetProp(id.sym.to_string()));
                                }
                                MemberProp::Computed(computed) => {
                                    // obj[key] = value
                                    // Stack: [obj, value, key]
                                    self.gen_expr(&computed.expr); // Push the key
                                    self.instructions.push(OpCode::SetPropComputed);
                                }
                                MemberProp::PrivateName(pn) => {
                                    // obj.#name = value
                                    let key = self.private_key(&pn.name);
                                    self.instructions.push(OpCode::SetPrivate(key));
                                }
                            }
                        }
                        _ => self.unsupported("this assignment target"),
                    },
                    _ => self.unsupported("destructuring assignment"),
                }
            }
            Expr::Object(obj_lit) => {
                self.instructions.push(OpCode::NewObject);

                for prop in &obj_lit.props {
                    match prop {
                        PropOrSpread::Spread(spread) => {
                            // { ...source } - spread all properties from source object
                            self.gen_expr(&spread.expr);
                            self.instructions.push(OpCode::ObjectSpread);
                        }
                        PropOrSpread::Prop(prop_ptr) => {
                            match prop_ptr.as_ref() {
                                Prop::KeyValue(kv) => {
                                    let key = match &kv.key {
                                        PropName::Ident(id) => id.sym.to_string(),
                                        PropName::Str(s) => s
                                            .value
                                            .as_str()
                                            .expect("Invalid string key")
                                            .to_string(),
                                        PropName::Num(n) => n.value.to_string(),
                                        _ => {
                                            self.unsupported("computed object literal keys");
                                            continue;
                                        }
                                    };

                                    self.instructions.push(OpCode::Dup); // Duplicate Ptr
                                    self.gen_expr(&kv.value); // Push Value
                                    self.instructions.push(OpCode::SetProp(key)); // Consumes Value + 1 Ptr
                                }
                                Prop::Shorthand(ident) => {
                                    // { x } shorthand for { x: x }
                                    let key = ident.sym.to_string();
                                    self.instructions.push(OpCode::Dup);
                                    self.load_var(ident);
                                    self.instructions.push(OpCode::SetProp(key));
                                }
                                Prop::Method(method) => {
                                    // { fn() {} } - inline method
                                    let key = match &method.key {
                                        PropName::Ident(id) => id.sym.to_string(),
                                        _ => {
                                            self.unsupported(
                                                "computed object literal method names",
                                            );
                                            continue;
                                        }
                                    };
                                    self.instructions.push(OpCode::Dup);
                                    self.next_fn_name = Some(key.clone());
                                    self.gen_fn_decl(None, &method.function);
                                    self.instructions.push(OpCode::SetProp(key));
                                }
                                Prop::Getter(_) | Prop::Setter(_) => {
                                    self.unsupported("getters and setters in object literals")
                                }
                                Prop::Assign(_) => {
                                    self.unsupported("`key = value` in an object literal")
                                }
                            }
                        }
                    }
                }
            }
            Expr::SuperProp(super_prop) => self.gen_super_prop(super_prop),
            Expr::Member(member) => {
                // Regular obj.prop access
                // 1. Load the Object/Array
                self.gen_expr(&member.obj);

                match &member.prop {
                    // Handle obj.prop
                    MemberProp::Ident(id) => {
                        self.instructions.push(OpCode::GetProp(id.sym.to_string()));
                    }
                    // Handle arr[index]
                    MemberProp::Computed(computed) => {
                        self.gen_expr(&computed.expr); // Push the index expression
                        self.instructions.push(OpCode::GetPropComputed);
                    }
                    // `obj.#name`: a property named `#name`.
                    MemberProp::PrivateName(pn) => {
                        self.instructions
                            .push(OpCode::GetPrivate(self.private_key(&pn.name)));
                    }
                }
            }
            Expr::This(t) => {
                // In an arrow function: the enclosing function's `this`.
                if let VarRef::Env { .. } = self.scopes.var(t.span) {
                    self.load_var(&scope::this_ident(t.span));
                    return;
                }
                self.instructions.push(OpCode::LoadThis);
            }
            Expr::MetaProp(meta_prop) => {
                match meta_prop.kind {
                    MetaPropKind::NewTarget => {
                        // new.target - push the constructor that was called with new
                        self.instructions.push(OpCode::NewTarget);
                    }
                    MetaPropKind::ImportMeta => {
                        // import.meta - not yet supported
                        self.instructions.push(OpCode::Push(JsValue::Undefined));
                    }
                }
            }
            Expr::New(new_expr) => {
                // new Foo(arg1, arg2) compiles to [args..., Foo] Construct;
                // the VM creates `this`. (This also pushed a `NewObject`
                // and its `Dup` that nothing consumed, so two stale objects
                // were left behind by every `new`, displacing the
                // arguments of an enclosing call.)

                // 1. Push arguments
                let arg_count = new_expr.args.as_ref().map(|a| a.len()).unwrap_or(0);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        self.gen_expr(&arg.expr);
                    }
                }

                // 2. Push the constructor function
                self.gen_expr(&new_expr.callee);

                // 3. Call with construct semantics
                self.instructions.push(OpCode::Construct(arg_count));
            }
            Expr::Paren(paren_expr) => {
                // Parenthesized expression: just evaluate the inner expression
                self.gen_expr(&paren_expr.expr);
            }
            Expr::Cond(cond_expr) => {
                // Conditional expression: condition ? consequent : alternate
                // Stack: [condition, consequent, alternate] -> [result]

                // Compile condition
                self.gen_expr(&cond_expr.test);

                // Jump indices for jump if false and end
                let jump_if_false_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0)); // placeholder

                // Compile consequent
                self.gen_expr(&cond_expr.cons);

                // Jump to end after consequent
                let jump_end_idx = self.instructions.len();
                self.instructions.push(OpCode::Jump(0)); // placeholder

                // Backpatch jump_if_false
                let after_consequent = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[jump_if_false_idx] {
                    *addr = after_consequent;
                }

                // Compile alternate
                self.gen_expr(&cond_expr.alt);

                // Backpatch jump_end
                let after_alternate = self.instructions.len();
                if let OpCode::Jump(ref mut addr) = self.instructions[jump_end_idx] {
                    *addr = after_alternate;
                }
            }
            Expr::Tpl(tpl) => {
                // Template literal: `Hello ${name}!`
                // Compilation strategy:
                // 1. Push empty string as starting point
                // 2. For each quasi/expr pair:
                //    - Push the quasi string, concatenate
                //    - Compile the expr, concatenate
                // 3. Push the final quasi if exists

                // Handle empty template literal ``
                if tpl.quasis.is_empty() && tpl.exprs.is_empty() {
                    self.instructions
                        .push(OpCode::Push(JsValue::String("".to_string())));
                    return;
                }

                // Start with empty string
                self.instructions
                    .push(OpCode::Push(JsValue::String("".to_string())));

                // Iterate through quasis and exprs
                for (i, quasi) in tpl.quasis.iter().enumerate() {
                    // Push the quasi string (cooked value - escapes processed, or raw if not available)
                    let s_str = match quasi.cooked.as_ref() {
                        Some(wtf8) => String::from_utf8_lossy(wtf8.as_bytes()).into_owned(),
                        None => String::from_utf8_lossy(quasi.raw.as_bytes()).into_owned(),
                    };
                    self.instructions.push(OpCode::Push(JsValue::String(s_str)));
                    // Concatenate: "prefix" + result so far
                    self.instructions.push(OpCode::Add);

                    // If there's a corresponding expression, compile and concatenate it
                    if i < tpl.exprs.len() {
                        self.gen_expr(&tpl.exprs[i]);
                        // `${v}` is String(v): a `toString` comes first.
                        self.instructions.push(OpCode::ToStr);
                        self.instructions.push(OpCode::Add);
                    }
                }
            }
            Expr::Await(await_expr) => {
                if !self.in_async_function {
                    self.gen_expr(&await_expr.arg);
                } else {
                    self.gen_expr(&await_expr.arg);
                    self.instructions.push(OpCode::Await);
                }
            }
            Expr::Update(update_expr) => {
                if let Expr::Ident(id) = update_expr.arg.as_ref() {
                    self.load_var(id);
                    if update_expr.prefix {
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if update_expr.op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        self.instructions.push(OpCode::Dup);
                        self.store_var(id, false);
                    } else {
                        self.instructions.push(OpCode::Dup);
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if update_expr.op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        self.store_var(id, false);
                    }
                } else if let Expr::Member(member) = update_expr.arg.as_ref() {
                    // Member expression: obj.prop++ / ++obj.prop / obj[key]++ / ++obj[key]
                    let op = update_expr.op;
                    if update_expr.prefix {
                        // ++obj.prop: compute new value, store it, leave new value on stack
                        // Stack: [obj, obj] -> GetProp -> [obj, old] -> +1 -> [obj, new] -> SetProp -> []
                        // Then reload to get new value on stack
                        self.gen_expr(&member.obj);
                        self.instructions.push(OpCode::Dup);
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_get(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                        }
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        // Stack: [obj, new_val] -> SetProp -> []
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_set(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::SetPropComputed);
                            }
                        }
                        // Reload the new value
                        self.gen_expr(&member.obj);
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_get(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                        }
                    } else {
                        // obj.prop++: get old value, save it, compute new, store back
                        // Stack: [obj, obj] -> GetProp -> [obj, old] -> Swap -> [old, obj]
                        // -> Dup -> [old, obj, obj] -> GetProp -> [old, obj, old2]
                        // -> +1 -> [old, obj, new] -> SetProp -> [old]
                        self.gen_expr(&member.obj);
                        self.instructions.push(OpCode::Dup);
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_get(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                        }
                        // Stack: [obj, old_val] -> Swap -> [old_val, obj]
                        self.instructions.push(OpCode::Swap);
                        self.instructions.push(OpCode::Dup);
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_get(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                        }
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        // Stack: [old_val, obj, new_val] -> SetProp -> [old_val]
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_set(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::SetPropComputed);
                            }
                        }
                    }
                }
            }
            Expr::TsAs(ts_as) => {
                // TypeScript `as` type assertion - evaluate inner expression (no runtime effect)
                self.gen_expr(&ts_as.expr);
            }
            Expr::TsTypeAssertion(ts_assert) => {
                // TypeScript type assertion `<Type>expr` - evaluate inner expression
                self.gen_expr(&ts_assert.expr);
            }
            Expr::TsNonNull(ts_non_null) => {
                // TypeScript non-null assertion `expr!` - evaluate inner expression
                self.gen_expr(&ts_non_null.expr);
            }
            // More type-only wrappers with no runtime effect.
            Expr::TsConstAssertion(e) => self.gen_expr(&e.expr),
            Expr::TsSatisfies(e) => self.gen_expr(&e.expr),
            Expr::TsInstantiation(e) => self.gen_expr(&e.expr),
            Expr::Seq(seq) => {
                // `a, b, c`: evaluate all, keep the last value.
                for (i, e) in seq.exprs.iter().enumerate() {
                    self.gen_expr(e);
                    if i + 1 < seq.exprs.len() {
                        self.instructions.push(OpCode::Pop);
                    }
                }
            }
            Expr::Lit(Lit::Regex(_)) => self.unsupported("regular expression literals"),
            Expr::Lit(Lit::BigInt(_)) => self.unsupported("BigInt literals"),
            Expr::Lit(_) => self.unsupported("this literal"),
            Expr::TaggedTpl(_) => self.unsupported("tagged template literals"),
            Expr::Yield(_) => self.unsupported("`yield`"),
            Expr::Class(class_expr) => {
                let name = class_expr.ident.as_ref().map(|id| id.sym.to_string());
                self.gen_class(&class_expr.class, name.as_deref());
            }
            Expr::PrivateName(_) => self.unsupported("bare private name"),
            Expr::JSXMember(_)
            | Expr::JSXNamespacedName(_)
            | Expr::JSXEmpty(_)
            | Expr::JSXElement(_)
            | Expr::JSXFragment(_) => self.unsupported("JSX"),
            Expr::Invalid(_) => self.unsupported("invalid expression"),
        }
    }
}
