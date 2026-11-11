//! Lowering one bytecode instruction to IR operations.

use super::*;

impl Lowerer {
    /// Lower a single instruction.
    pub(super) fn lower_instruction(&mut self, idx: usize, op: &OpCode) -> Result<(), LowerError> {
        match op {
            OpCode::LoadThis => {
                let dst = self.alloc_value(IrType::Object);
                self.emit(IrOp::LoadThis(dst));
                self.push(dst);
            }

            OpCode::Push(value) => {
                let (lit, ty) = self.jsvalue_to_literal(value);
                let dst = self.alloc_value(ty);
                self.emit(IrOp::Const(dst, lit));
                self.push(dst);
            }

            OpCode::Pop => {
                self.pop()?;
            }

            OpCode::Dup => {
                let top = self.peek()?;
                self.push(top);
            }

            OpCode::Swap => {
                // Swap top two values
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(b);
                self.push(a);
            }

            OpCode::Swap3 => {
                // Swap top three values: [a, b, c] -> [c, b, a]
                let c = self.pop()?;
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(c);
                self.push(b);
                self.push(a);
            }

            // Arithmetic operations (binary)
            OpCode::Add => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::AddAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Sub => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::SubAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Mul => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::MulAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Div => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::DivAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Mod => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::ModAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Neg => {
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::NegAny(dst, a));
                self.push(dst);
            }

            OpCode::TypeOfName(name) => match self.names.resolve(name)? {
                NameRef::Global => {
                    let dst = self.alloc_value(IrType::String);
                    self.emit(IrOp::TypeOfGlobal(dst, name.clone()));
                    self.push(dst);
                }
                // A local or function is always bound.
                _ => {
                    self.lower_instruction(idx, &OpCode::Load(name.clone()))?;
                    self.lower_instruction(idx, &OpCode::TypeOf)?;
                }
            },

            OpCode::ToStr => {
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::String);
                self.emit(IrOp::ToStr(dst, a));
                self.push(dst);
            }

            OpCode::TypeOf => {
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::String);
                self.emit(IrOp::TypeOf(dst, a));
                self.push(dst);
            }

            OpCode::Delete(prop_name) => {
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::DeleteProp(dst, obj, prop_name.clone()));
                self.push(dst);
            }

            // Comparison operations
            OpCode::Eq | OpCode::EqEq | OpCode::Ne | OpCode::NeEq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(match op {
                    OpCode::Eq => IrOp::EqStrict(dst, a, b),
                    OpCode::Ne => IrOp::NeStrict(dst, a, b),
                    // `==` / `!=` are loose; lowering them as strict made
                    // `null == undefined` false in native code.
                    OpCode::EqEq => IrOp::EqLoose(dst, a, b),
                    _ => IrOp::NeLoose(dst, a, b),
                });
                self.push(dst);
            }

            OpCode::Lt => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::Lt(dst, a, b));
                self.push(dst);
            }

            OpCode::LtEq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::LtEq(dst, a, b));
                self.push(dst);
            }

            OpCode::Gt => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::Gt(dst, a, b));
                self.push(dst);
            }

            OpCode::GtEq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::GtEq(dst, a, b));
                self.push(dst);
            }

            OpCode::In => {
                // [key, obj]
                let obj = self.pop()?;
                let key = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::In(dst, key, obj));
                self.push(dst);
            }
            OpCode::InstanceOf => {
                // [value, ctor]
                let ctor = self.pop()?;
                let value = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::InstanceOf(dst, value, ctor));
                self.push(dst);
            }
            OpCode::NewTarget => {
                return Err(native_unsupported("`new.target`"));
            }

            // Logical operations
            OpCode::Not => {
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::Not(dst, a));
                self.push(dst);
            }

            OpCode::And => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::And(dst, a, b));
                self.push(dst);
            }

            OpCode::Or => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Or(dst, a, b));
                self.push(dst);
            }

            // Variable operations (name-based)
            OpCode::Let(name) | OpCode::Store(name) => {
                let val = self.pop()?;
                match self.names.resolve(name)? {
                    NameRef::Local => {
                        let slot = self.get_or_create_local(name);
                        self.emit(IrOp::StoreLocal(slot, val));
                        self.local_values.insert(slot, val);
                    }
                    NameRef::Function(_) | NameRef::Global => {
                        self.emit(IrOp::StoreGlobal(name.clone(), val));
                    }
                }
            }

            OpCode::Load(name) => {
                let dst = match self.names.resolve(name)? {
                    NameRef::Local => {
                        let slot = self.get_or_create_local(name);
                        let dst = self.alloc_value(IrType::Any);
                        self.emit(IrOp::LoadLocal(dst, slot));
                        self.local_values.insert(slot, dst);
                        dst
                    }
                    NameRef::Function(addr) => {
                        let dst = self.alloc_value(IrType::Function);
                        self.emit(IrOp::Const(dst, Literal::Function(addr)));
                        dst
                    }
                    NameRef::Global => {
                        let dst = self.alloc_value(IrType::Any);
                        self.emit(IrOp::LoadGlobal(dst, name.clone()));
                        dst
                    }
                };
                self.push(dst);
            }

            OpCode::Drop(_name) => {
                // Drop is a no-op in SSA form
            }

            // Indexed local operations
            OpCode::StoreLocal(slot) => {
                let val = self.pop()?;
                self.emit(IrOp::StoreLocal(*slot, val));
                self.local_values.insert(*slot, val);
            }

            OpCode::LoadLocal(slot) => {
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadLocal(dst, *slot));
                self.local_values.insert(*slot, dst);
                self.push(dst);
            }

            // Object operations
            OpCode::NewObject => {
                let dst = self.alloc_value(IrType::Object);
                self.emit(IrOp::NewObject(dst));
                self.push(dst);
            }

            OpCode::NewObjectWithProto => {
                return Err(native_unsupported("classes"));
            }

            OpCode::GetProp(name) => {
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetProp(dst, obj, name.clone()));
                self.push(dst);
            }

            // Stack effects below match the VM's exactly; an extra or missing
            // value desynchronises everything after it.
            OpCode::SetProp(name) => {
                // [obj, value] -> []
                let val = self.pop()?;
                let obj = self.pop()?;
                self.emit(IrOp::SetProp(obj, name.clone(), val));
            }

            OpCode::SetPropComputed | OpCode::StoreElement => {
                // [obj, value, key] -> []
                let key = self.pop()?;
                let val = self.pop()?;
                let obj = self.pop()?;
                self.emit(IrOp::SetElement(obj, key, val));
            }

            OpCode::GetPropComputed | OpCode::LoadElement => {
                // [obj, key] -> [obj[key]]
                let key = self.pop()?;
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetElement(dst, obj, key));
                self.push(dst);
            }

            // Array operations
            OpCode::NewArray(size) => {
                // The VM creates `size` undefined elements, which the
                // following stores then fill.
                let dst = self.alloc_value(IrType::Array);
                self.emit(IrOp::NewArray(dst));
                if *size > 0 {
                    let len = self.alloc_value(IrType::Number);
                    self.emit(IrOp::Const(len, Literal::Number(*size as f64)));
                    self.emit(IrOp::SetProp(dst, "length".to_string(), len));
                }
                self.push(dst);
            }

            // Control flow
            OpCode::Jump(target) => {
                let target_block = self
                    .instr_to_block
                    .get(target)
                    .copied()
                    .ok_or(LowerError::InvalidJumpTarget(*target))?;
                self.terminate(Terminator::Jump(target_block));
            }

            OpCode::JumpIfFalse(target) => {
                let cond = self.pop()?;
                let false_block = self
                    .instr_to_block
                    .get(target)
                    .copied()
                    .ok_or(LowerError::InvalidJumpTarget(*target))?;

                // True block is the fall-through (next instruction)
                let true_block = self
                    .instr_to_block
                    .get(&(idx + 1))
                    .copied()
                    .ok_or_else(|| LowerError::InvalidJumpTarget(idx + 1))?;

                self.terminate(Terminator::Branch(cond, true_block, false_block));
            }

            OpCode::Return => {
                let ret_val = if self.stack.is_empty() {
                    None
                } else {
                    Some(self.pop()?)
                };
                self.terminate(Terminator::Return(ret_val));
            }

            OpCode::Halt => {
                // If there's a value on the stack, return it (for REPL-style scripts)
                let ret_val = if self.stack.is_empty() {
                    None
                } else {
                    Some(self.pop()?)
                };
                self.terminate(Terminator::Return(ret_val));
            }

            // Function calls
            OpCode::Call(argc) => {
                let func_val = self.pop()?;
                // Pop arguments in reverse order
                let mut args = Vec::with_capacity(*argc);
                for _ in 0..*argc {
                    args.push(self.pop()?);
                }
                args.reverse();

                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Call(dst, func_val, args));
                self.push(dst);
            }

            OpCode::ArraySpread | OpCode::ObjectSpread => {
                let source = self.pop()?;
                let target = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(if matches!(op, OpCode::ArraySpread) {
                    IrOp::ArraySpread(dst, target, source)
                } else {
                    IrOp::ObjectSpread(dst, target, source)
                });
                self.push(dst);
            }

            OpCode::Intrinsic(id, argc) => {
                let mut args = Vec::with_capacity(*argc);
                for _ in 0..*argc {
                    args.push(self.pop()?);
                }
                args.reverse();
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Intrinsic(dst, *id, args));
                self.push(dst);
            }

            OpCode::CallMethod(name, argc) => {
                // VM pops receiver first (top of stack), then args below
                // Stack: [..., arg0, arg1, ..., argN, receiver]
                let obj = self.pop()?;

                // Pop arguments (they're below the receiver on the stack)
                let mut args = Vec::with_capacity(*argc);
                for _ in 0..*argc {
                    args.push(self.pop()?);
                }
                args.reverse();

                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::CallMethod(dst, obj, name.clone(), args));
                self.push(dst);
            }

            OpCode::LoadClosureEnv => {
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadEnv(dst));
                self.push(dst);
            }

            OpCode::MakeClosure(addr) => {
                // Pop environment object
                let env = self.pop()?;
                let dst = self.alloc_value(IrType::Function);
                self.emit(IrOp::MakeClosure(dst, *addr as u32, env));
                self.push(dst);
            }

            OpCode::Construct(argc) => {
                // [args..., ctor]
                let ctor = self.pop()?;
                let args = self.pop_n(*argc)?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Construct(dst, ctor, args));
                self.push(dst);
            }
            OpCode::CallSuper(argc) => {
                // [args..., parent]
                let parent = self.pop()?;
                let args = self.pop_n(*argc)?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::CallSuper(dst, parent, args));
                self.push(dst);
            }
            OpCode::RestArgs(n) => {
                // The arguments from the `n`th on, as an array; the first
                // `Let` binds it.
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::RestArgs(dst, *n as u32));
                self.push(dst);
            }
            OpCode::ForwardArgs => {
                // The VM's caller pushes the arguments and their count here;
                // native code reads its own. This stands for the count.
                let placeholder = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(placeholder, Literal::Undefined));
                self.push(placeholder);
            }
            OpCode::SuperForward => {
                // [count placeholder, parent]
                let parent = self.pop()?;
                self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::SuperForward(dst, parent));
                self.push(dst);
            }
            OpCode::CallWithThis(argc) => {
                // [args..., f, this]
                let this = self.pop()?;
                let func = self.pop()?;
                let args = self.pop_n(*argc)?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::CallWithThis(dst, func, this, args));
                self.push(dst);
            }
            OpCode::GetPrivate(key) => {
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetPrivate(dst, obj, key.clone()));
                self.push(dst);
            }
            OpCode::SetPrivate(key) => {
                let value = self.pop()?;
                let obj = self.pop()?;
                self.emit(IrOp::SetPrivate(obj, key.clone(), value));
            }
            OpCode::GetSuperProp(name) => {
                // [this, base]
                let base = self.pop()?;
                let this = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetSuperProp(dst, this, base, name.clone()));
                self.push(dst);
            }

            OpCode::Require => {
                return Err(native_unsupported("`require`"));
            }

            OpCode::Print => {
                let val = self.pop()?;
                // Emit as method call on console
                let console = self.alloc_value(IrType::Object);
                self.emit(IrOp::LoadGlobal(console, "console".to_string()));
                let _result = self.alloc_value(IrType::Void);
                self.emit(IrOp::CallMethod(
                    _result,
                    console,
                    "log".to_string(),
                    vec![val],
                ));
            }

            OpCode::Throw => {
                let value = self.pop()?;
                self.emit(IrOp::Throw(value));
            }
            OpCode::SetupTry {
                catch_addr,
                finally_addr,
            } => self.lower_setup_try(idx, *catch_addr, *finally_addr)?,
            OpCode::PopTry => {
                self.handlers.pop();
            }
            OpCode::EnterFinally(rethrow) => {
                if *rethrow && let Some(&setup) = self.finally_owner.get(&idx) {
                    let (thrown_slot, exc_slot) = self.try_slots(setup);
                    let thrown = self.alloc_value(IrType::Any);
                    self.emit(IrOp::LoadLocal(thrown, thrown_slot));
                    let exception = self.alloc_value(IrType::Any);
                    self.emit(IrOp::LoadLocal(exception, exc_slot));
                    self.emit(IrOp::Rethrow(thrown, exception));
                }
            }

            // Not yet supported by the native backends. These used to lower
            // to no-ops or `undefined`, so native code silently computed
            // wrong answers. Refusing is the only behaviour that cannot
            // produce a wrong result.
            OpCode::SetProto => {
                return Err(native_unsupported("`SetProto`"));
            }
            OpCode::ImportAsync(_)
            | OpCode::GetExport { .. }
            | OpCode::ModuleResolutionError { .. } => {
                return Err(native_unsupported("modules (`import`)"));
            }
            OpCode::Await => {
                if !self.func.is_async {
                    return Err(native_unsupported("top-level `await`"));
                }
                let value = self.pop()?;
                let resume = self
                    .instr_to_block
                    .get(&(idx + 1))
                    .copied()
                    .ok_or(LowerError::InvalidJumpTarget(idx + 1))?;
                let k = self.resume_points.len() as u32;
                self.resume_points.push(resume);
                self.emit(IrOp::Await(value, k));
                // Suspended: the caller gets the function's promise.
                let slot = self.get_or_create_local("%promise");
                let promise = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadLocal(promise, slot));
                self.terminate(Terminator::Return(Some(promise)));
            }
            OpCode::AsyncStart => {
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::AsyncStart(dst));
                self.push(dst);
            }
            OpCode::AsyncResolve | OpCode::AsyncReject => {
                // [value, promise] -> [promise]
                let promise = self.pop()?;
                let value = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(if matches!(op, OpCode::AsyncResolve) {
                    IrOp::AsyncResolve(dst, promise, value)
                } else {
                    IrOp::AsyncReject(dst, promise, value)
                });
                self.push(dst);
            }

            // Bitwise operators - emit as number operations
            OpCode::BitAnd => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::BitAnd(dst, a, b));
                self.push(dst);
            }

            OpCode::BitOr => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::BitOr(dst, a, b));
                self.push(dst);
            }

            OpCode::Xor => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::Xor(dst, a, b));
                self.push(dst);
            }

            OpCode::ShiftLeft => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::Shl(dst, a, b));
                self.push(dst);
            }

            OpCode::ShiftRight => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::Shr(dst, a, b));
                self.push(dst);
            }

            OpCode::ShiftRightUnsigned => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::ShrU(dst, a, b));
                self.push(dst);
            }

            OpCode::Pow => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::Pow(dst, a, b));
                self.push(dst);
            }

            // `[..., value]` in an array literal with a spread.
            OpCode::ArrayPush => {
                let value = self.pop()?;
                let arr = self.pop()?;
                self.emit(IrOp::ArrayPush(arr, value));
                self.push(arr);
            }
        }

        Ok(())
    }

    /// Convert a JsValue to an IR literal.
    pub(super) fn jsvalue_to_literal(&self, value: &JsValue) -> (Literal, IrType) {
        match value {
            JsValue::Number(n) => (Literal::Number(*n), IrType::Number),
            JsValue::String(s) => (Literal::String(s.clone()), IrType::String),
            JsValue::Boolean(b) => (Literal::Boolean(*b), IrType::Boolean),
            JsValue::Null => (Literal::Null, IrType::Any),
            JsValue::Undefined => (Literal::Undefined, IrType::Any),
            JsValue::Object(_) => (Literal::Null, IrType::Object),
            JsValue::Function { address, .. } => (Literal::Function(*address), IrType::Function),
            JsValue::NativeFunction(_) => (Literal::Null, IrType::Function),
            JsValue::Accessor(_, _) => (Literal::Null, IrType::Any),
            JsValue::Promise(_) => (Literal::Null, IrType::Any),
        }
    }
}
