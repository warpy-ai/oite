use crate::vm::opcodes::OpCode;
use swc_ecma_ast::*;
pub mod borrow_ck;
use crate::vm::value::JsValue;
pub struct Codegen {
    pub instructions: Vec<OpCode>,
    scope_stack: Vec<Vec<String>>,
    in_function: bool,
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            scope_stack: vec![Vec::new()],
            in_function: false,
        }
    }

    pub fn generate(&mut self, module: &Module) -> Vec<OpCode> {
        for item in &module.body {
            if let Some(stmt) = item.as_stmt() {
                self.gen_stmt(stmt);
            }
        }
        self.instructions.push(OpCode::Halt);
        self.instructions.clone()
    }

    fn gen_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return(ret_stmt) => {
                if let Some(arg) = &ret_stmt.arg {
                    self.gen_expr(arg); // Pushes the return value to stack
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Undefined));
                }
                self.instructions.push(OpCode::Return);
            }
            // RECURSION: Handle the Block
            Stmt::Block(block) => {
                self.scope_stack.push(Vec::new()); // Enter new scope
                for s in &block.stmts {
                    self.gen_stmt(s);
                }
                // Exit scope: Drop variables
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::Decl(Decl::Var(var_decl)) => {
                for decl in &var_decl.decls {
                    if let Some(init) = &decl.init {
                        let name = decl.name.as_ident().unwrap().sym.to_string();
                        self.gen_expr(init);
                        self.instructions.push(OpCode::Store(name));
                    }
                }
            }
            Stmt::Decl(Decl::Fn(fn_decl)) => {
                let name = fn_decl.ident.sym.to_string();

                // 1. Push function address and store it
                let start_ip = self.instructions.len() + 3; // +3 to skip Push, Store, and Jump
                self.instructions
                    .push(OpCode::Push(JsValue::Function(start_ip)));
                self.instructions.push(OpCode::Store(name));

                // 2. Add jump to skip over function body
                let jump_target = self.instructions.len() + 1; // Will be updated after compiling body
                self.instructions.push(OpCode::Jump(jump_target));

                // 3. Compile function body
                self.in_function = true;
                // NEW: Inside the function body, we must pop arguments into locals
                // We process them in REVERSE order because of how they sit on the stack
                for param in fn_decl.function.params.iter().rev() {
                    if let Pat::Ident(id) = &param.pat {
                        let param_name = id.id.sym.to_string();
                        // The value is already on the stack from the Caller
                        self.instructions.push(OpCode::Store(param_name));
                    }
                }
                let stmts = &fn_decl.function.body.as_ref().unwrap().stmts;
                for s in stmts {
                    self.gen_stmt(s);

                    // If this is the last statement and it's an expression (like a + b),
                    // we DON'T push Undefined. It will act as the implicit return value.
                }
                self.in_function = false;
                // At the very end of the function body, add an implicit return
                // This handles functions that don't have a 'return' statement
                if stmts.is_empty() {
                    self.instructions.push(OpCode::Push(JsValue::Undefined));
                }
                self.instructions.push(OpCode::Return);

                // 4. Update jump target to point after the function body
                let current_len = self.instructions.len();
                if let OpCode::Jump(ref mut target) = self.instructions[start_ip - 1] {
                    *target = current_len;
                }
            }
            Stmt::Expr(expr_stmt) => {
                self.gen_expr(&expr_stmt.expr);
                // Expression statements (e.g. `foo();`) should discard their result in JS.
                // Keep values on the stack only when inside a function, since the last
                // expression can become the implicit return value in our VM.
                if !self.in_function {
                    self.instructions.push(OpCode::Pop);
                }
            }
            // Inside gen_stmt match block
            Stmt::While(while_stmt) => {
                // 1. Record the start position (where we check the condition)
                let loop_start = self.instructions.len();

                // 2. Compile the condition
                self.gen_expr(&while_stmt.test);

                // 3. Jump to the end if the condition is false
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0)); // Placeholder

                // 4. Compile the loop body
                self.gen_stmt(&while_stmt.body);

                // 5. Jump back to the start to re-check the condition
                self.instructions.push(OpCode::Jump(loop_start));

                // 6. Backpatch the exit jump
                let loop_end = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
            }
            _ => {}
        }
    }

    fn gen_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Fn(fn_expr) => {
                // Function expression: `function(a, b) { ... }`
                //
                // We compile it similarly to a function declaration, but instead of storing it
                // in a variable, we leave the function pointer on the stack as the expression value.
                //
                // Layout:
                //   Push(Function(start_ip))
                //   Jump(after_body)
                // start_ip:
                //   Store params...
                //   ...body...
                //   Return
                // after_body:
                //   (execution continues, function value remains on stack)

                let start_ip = self.instructions.len() + 2; // Push + Jump
                self.instructions
                    .push(OpCode::Push(JsValue::Function(start_ip)));

                let jump_idx = self.instructions.len();
                self.instructions.push(OpCode::Jump(0)); // patched after body

                let prev_in_function = self.in_function;
                self.in_function = true;

                // Pop args into locals (reverse order)
                for param in fn_expr.function.params.iter().rev() {
                    if let Pat::Ident(id) = &param.pat {
                        let param_name = id.id.sym.to_string();
                        self.instructions.push(OpCode::Store(param_name));
                    }
                }

                if let Some(body) = &fn_expr.function.body {
                    for s in &body.stmts {
                        self.gen_stmt(s);
                    }
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Undefined));
                }

                self.instructions.push(OpCode::Return);
                self.in_function = prev_in_function;

                let after_body = self.instructions.len();
                if let OpCode::Jump(ref mut target) = self.instructions[jump_idx] {
                    *target = after_body;
                }
            }
            Expr::Arrow(arrow) => {
                // Arrow function: `(a, b) => expr` or `(a, b) => { ... }`
                // Compiled as a normal function value (like Expr::Fn), leaving the function
                // pointer on the stack.

                // (We ignore async/captures for now.)
                let start_ip = self.instructions.len() + 2; // Push + Jump
                self.instructions
                    .push(OpCode::Push(JsValue::Function(start_ip)));

                let jump_idx = self.instructions.len();
                self.instructions.push(OpCode::Jump(0)); // patched after body

                let prev_in_function = self.in_function;
                self.in_function = true;

                // Pop args into locals (reverse order)
                for param in arrow.params.iter().rev() {
                    if let Pat::Ident(id) = param {
                        let param_name = id.id.sym.to_string();
                        self.instructions.push(OpCode::Store(param_name));
                    } else {
                        println!("Warning: Non-identifier arrow params not supported yet.");
                    }
                }

                match &*arrow.body {
                    BlockStmtOrExpr::Expr(e) => {
                        // Expression-bodied arrows implicitly return the expression.
                        self.gen_expr(e);
                        self.instructions.push(OpCode::Return);
                    }
                    BlockStmtOrExpr::BlockStmt(block) => {
                        for s in &block.stmts {
                            self.gen_stmt(s);
                        }
                        if block.stmts.is_empty() {
                            self.instructions.push(OpCode::Push(JsValue::Undefined));
                        }
                        self.instructions.push(OpCode::Return);
                    }
                }

                self.in_function = prev_in_function;

                let after_body = self.instructions.len();
                if let OpCode::Jump(ref mut target) = self.instructions[jump_idx] {
                    *target = after_body;
                }
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
            Expr::Ident(id) => {
                self.instructions.push(OpCode::Load(id.sym.to_string()));
            }
            Expr::Bin(bin) => {
                self.gen_expr(&bin.left);
                self.gen_expr(&bin.right);
                match bin.op {
                    BinaryOp::Add => self.instructions.push(OpCode::Add),
                    BinaryOp::EqEqEq => self.instructions.push(OpCode::Eq),
                    BinaryOp::Lt => self.instructions.push(OpCode::Lt),
                    BinaryOp::Gt => self.instructions.push(OpCode::Gt),
                    BinaryOp::Sub => self.instructions.push(OpCode::Sub),
                    _ => println!("Warning: Operator {:?} not supported", bin.op),
                }
            }
            Expr::Array(arr_lit) => {
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
            Expr::Call(call_expr) => {
                let arg_count = call_expr.args.len();
                for arg in &call_expr.args {
                    self.gen_expr(&arg.expr);
                }
                // Load the function
                match &call_expr.callee {
                    Callee::Expr(expr) => self.gen_expr(expr),
                    Callee::Super(_) => {}  // Handle super calls if needed
                    Callee::Import(_) => {} // Handle import calls if needed
                }
                // Call it
                self.instructions.push(OpCode::Call(arg_count));
            }
            // Inside gen_expr in compiler/mod.rs
            Expr::Assign(assign_expr) => {
                // 1. Evaluate the right-hand side first
                self.gen_expr(&assign_expr.right);

                // 2. Extract the name from the left-hand side
                match &assign_expr.left {
                    AssignTarget::Simple(simple) => {
                        if let SimpleAssignTarget::Ident(binding_ident) = simple {
                            let name = binding_ident.id.sym.to_string();
                            // JS assignment expressions evaluate to the assigned value.
                            // Our `Store` opcode consumes the value, so `Dup` ensures one copy
                            // remains on the stack for expression context (e.g. `a = 1;`).
                            self.instructions.push(OpCode::Dup);
                            self.instructions.push(OpCode::Store(name));
                        }
                    }
                    _ => println!("Warning: Complex assignment targets not supported yet."),
                }
            }
            Expr::Object(obj_lit) => {
                self.instructions.push(OpCode::NewObject);

                for prop in &obj_lit.props {
                    if let PropOrSpread::Prop(prop_ptr) = prop
                        && let Prop::KeyValue(kv) = prop_ptr.as_ref()
                    {
                        let key = match &kv.key {
                            PropName::Ident(id) => id.sym.to_string(),
                            PropName::Str(s) => {
                                s.value.as_str().expect("Invalid string key").to_string()
                            }
                            _ => continue,
                        };

                        self.instructions.push(OpCode::Dup); // Duplicate Ptr
                        self.gen_expr(&kv.value); // Push Value
                        self.instructions.push(OpCode::SetProp(key)); // Consumes Value + 1 Ptr
                    }
                }
            }
            Expr::Member(member) => {
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
                        self.instructions.push(OpCode::LoadElement);
                    }
                    // Handle #privateField (Standard in modern JS, but let's skip for now)
                    MemberProp::PrivateName(_) => {
                        println!("Warning: Private class fields (#) are not yet supported.");
                    }
                }
            }
            _ => {}
        }
    }
}
