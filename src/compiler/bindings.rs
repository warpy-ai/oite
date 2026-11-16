//! Bindings: `var`/`let`/`const`, destructuring patterns and compound
//! assignment.

use super::*;

impl Codegen {
    pub(super) fn gen_var_decl(&mut self, var_decl: &VarDecl) {
        for decl in &var_decl.decls {
            if let Some(init) = &decl.init {
                // `let f = () => ...`: the function is named `f`.
                if let (Pat::Ident(id), Expr::Arrow(_) | Expr::Fn(FnExpr { ident: None, .. })) =
                    (&decl.name, init.as_ref())
                {
                    self.next_fn_name = Some(id.id.sym.to_string());
                }
                self.gen_expr(init);
                self.gen_pattern_binding(&decl.name);
            } else if var_decl.kind != VarDeclKind::Var {
                // `let x;` is `undefined`. (A `var` is bound to `undefined`
                // once, at the top of its function: `hoist_body`.)
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.gen_pattern_binding(&decl.name);
            }
        }
    }

    /// Generate code to bind a pattern to a value on the stack.
    /// The value to destructure should already be on top of the stack.
    pub(super) fn gen_pattern_binding(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(id) => {
                // Simple variable binding
                self.store_var(&id.id, true);
            }
            Pat::Object(obj_pat) => {
                // Object destructuring: let { x, y } = obj
                for (i, prop) in obj_pat.props.iter().enumerate() {
                    match prop {
                        swc_ecma_ast::ObjectPatProp::KeyValue(kv) => {
                            // { key: pattern } - extract key and bind to pattern
                            // Dup the object for each property (except the last)
                            if i < obj_pat.props.len() - 1 {
                                self.instructions.push(OpCode::Dup);
                            }
                            // Get property name
                            let key_name = match &kv.key {
                                swc_ecma_ast::PropName::Ident(id) => id.sym.to_string(),
                                swc_ecma_ast::PropName::Str(s) => {
                                    s.value.to_string_lossy().into_owned()
                                }
                                _ => {
                                    self.unsupported("computed keys in destructuring patterns");
                                    continue;
                                }
                            };
                            self.instructions.push(OpCode::GetProp(key_name));
                            // Recursively bind the value pattern
                            self.gen_pattern_binding(&kv.value);
                        }
                        swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                            // { x } or { x = default } - shorthand property
                            // Dup the object for each property (except the last)
                            if i < obj_pat.props.len() - 1 {
                                self.instructions.push(OpCode::Dup);
                            }
                            let key_name = assign.key.sym.to_string();
                            self.instructions.push(OpCode::GetProp(key_name.clone()));

                            // Handle default value if present
                            if let Some(default_val) = &assign.value {
                                // Check if value is undefined, use default if so
                                self.instructions.push(OpCode::Dup);
                                self.instructions.push(OpCode::Push(JsValue::Undefined));
                                self.instructions.push(OpCode::Eq);
                                let jump_idx = self.instructions.len();
                                self.instructions.push(OpCode::JumpIfFalse(0));
                                // Pop undefined, push default
                                self.instructions.push(OpCode::Pop);
                                self.gen_expr(default_val);
                                let end_addr = self.instructions.len();
                                if let OpCode::JumpIfFalse(ref mut addr) =
                                    self.instructions[jump_idx]
                                {
                                    *addr = end_addr;
                                }
                            }

                            self.store_var(&assign.key.id, true);
                        }
                        swc_ecma_ast::ObjectPatProp::Rest(rest) => {
                            // { ...rest } - not fully implemented yet
                            if let Pat::Ident(id) = rest.arg.as_ref() {
                                // For now, just bind the remaining object
                                self.store_var(&id.id, true);
                            }
                        }
                    }
                }
            }
            Pat::Array(arr_pat) => {
                // Array destructuring: let [a, b] = arr
                for (i, elem) in arr_pat.elems.iter().enumerate() {
                    if let Some(elem_pat) = elem {
                        // Dup the array for each element (except the last)
                        let is_last = i == arr_pat.elems.len() - 1
                            || arr_pat.elems.iter().skip(i + 1).all(|e| e.is_none());
                        if !is_last {
                            self.instructions.push(OpCode::Dup);
                        }
                        // Push index and load element
                        self.instructions
                            .push(OpCode::Push(JsValue::Number(i as f64)));
                        self.instructions.push(OpCode::GetPropComputed);
                        // Recursively bind the element pattern
                        self.gen_pattern_binding(elem_pat);
                    }
                }
            }
            Pat::Rest(rest) => {
                // ...rest pattern - bind remaining value
                self.gen_pattern_binding(&rest.arg);
            }
            Pat::Assign(assign) => {
                // pattern = default - handle default value
                self.instructions.push(OpCode::Dup);
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.instructions.push(OpCode::Eq);
                let jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                // Pop undefined, push default
                self.instructions.push(OpCode::Pop);
                self.gen_expr(&assign.right);
                let end_addr = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[jump_idx] {
                    *addr = end_addr;
                }
                self.gen_pattern_binding(&assign.left);
            }
            Pat::Expr(_) => {
                self.unsupported("expression as a binding pattern");
                self.instructions.push(OpCode::Pop);
            }
            Pat::Invalid(_) => {
                self.unsupported("invalid binding pattern");
                self.instructions.push(OpCode::Pop);
            }
        }
    }

    /// `x op= v`, `o.p op= v`, `o[k] op= v`: read, combine, write back,
    /// evaluating the target's object and key once. Before this, the
    /// operator was ignored and `x += 2` compiled as `x = 2`.
    pub(super) fn gen_compound_assign(&mut self, assign: &AssignExpr) {
        let op = match assign.op {
            AssignOp::AddAssign => OpCode::Add,
            AssignOp::SubAssign => OpCode::Sub,
            AssignOp::MulAssign => OpCode::Mul,
            AssignOp::DivAssign => OpCode::Div,
            AssignOp::ModAssign => OpCode::Mod,
            AssignOp::ExpAssign => OpCode::Pow,
            AssignOp::BitAndAssign => OpCode::BitAnd,
            AssignOp::BitOrAssign => OpCode::BitOr,
            AssignOp::BitXorAssign => OpCode::Xor,
            AssignOp::LShiftAssign => OpCode::ShiftLeft,
            AssignOp::RShiftAssign => OpCode::ShiftRight,
            AssignOp::ZeroFillRShiftAssign => OpCode::ShiftRightUnsigned,
            other => {
                self.unsupported(format!("assignment operator `{}`", other.as_str()));
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                return;
            }
        };
        let AssignTarget::Simple(target) = &assign.left else {
            self.unsupported("destructuring assignment");
            self.instructions.push(OpCode::Push(JsValue::Undefined));
            return;
        };
        match target {
            SimpleAssignTarget::Ident(id) => {
                self.load_var(&id.id);
                self.gen_expr(&assign.right);
                self.instructions.push(op);
                self.instructions.push(OpCode::Dup);
                self.store_var(&id.id, false);
            }
            SimpleAssignTarget::Member(member) => match &member.prop {
                MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                    // [obj] -> [obj, cur] -> [obj, cur, rhs] -> [obj, v]
                    self.gen_expr(&member.obj);
                    self.instructions.push(OpCode::Dup);
                    self.emit_member_get(&member.prop);
                    self.gen_expr(&assign.right);
                    self.instructions.push(op);
                    // [obj, v] -> [v, obj, v] -> SetProp -> [v]
                    self.instructions.push(OpCode::Dup);
                    self.instructions.push(OpCode::Swap3);
                    self.instructions.push(OpCode::Swap);
                    self.emit_member_set(&member.prop);
                }
                MemberProp::Computed(computed) => {
                    let obj = format!("%obj{}", self.instructions.len());
                    let key = format!("%key{}", self.instructions.len());
                    self.gen_expr(&member.obj);
                    self.instructions.push(OpCode::Let(obj.clone()));
                    self.gen_expr(&computed.expr);
                    self.instructions.push(OpCode::Let(key.clone()));
                    self.instructions.push(OpCode::Load(obj.clone()));
                    self.instructions.push(OpCode::Load(key.clone()));
                    self.instructions.push(OpCode::GetPropComputed);
                    self.gen_expr(&assign.right);
                    self.instructions.push(op);
                    // [v] -> [obj, v] -> [v, obj, v] -> [v, obj, v, key]
                    self.instructions.push(OpCode::Load(obj));
                    self.instructions.push(OpCode::Swap);
                    self.instructions.push(OpCode::Dup);
                    self.instructions.push(OpCode::Swap3);
                    self.instructions.push(OpCode::Swap);
                    self.instructions.push(OpCode::Load(key));
                    self.instructions.push(OpCode::SetPropComputed);
                }
            },
            _ => {
                self.unsupported("this assignment target");
                self.instructions.push(OpCode::Push(JsValue::Undefined));
            }
        }
    }

    // ---------------------------------------------------------------------
    // Variables and environments (see `scope.rs`)
    // ---------------------------------------------------------------------
}
