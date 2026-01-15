use crate::vm::opcodes::OpCode;
use std::collections::HashSet;
use swc_ecma_ast::*;
pub mod borrow_ck;
use crate::compiler::borrow_ck::BorrowChecker;
use crate::vm::value::JsValue;
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_parser::{Parser, StringInput, Syntax, lexer::Lexer};

pub struct Compiler {
    pub borrow_checker: BorrowChecker,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            borrow_checker: BorrowChecker::new(),
        }
    }

    pub fn compile(&mut self, source: &str) -> Result<Vec<OpCode>, String> {
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            FileName::Custom("main.tscl".into()).into(),
            source.to_string(),
        );
        let lexer = Lexer::new(
            Syntax::Es(Default::default()),
            Default::default(),
            StringInput::from(&*fm),
            None,
        );
        let mut parser = Parser::new_from(lexer);
        let program = parser
            .parse_program()
            .map_err(|e| format!("Parsing error: {:?}", e))?;

        match &program {
            Program::Module(module) => {
                for item in &module.body {
                    if let ModuleItem::Stmt(stmt) = item {
                        self.borrow_checker.analyze_stmt(stmt)?;
                    }
                }
            }
            Program::Script(script) => {
                for stm in &script.body {
                    self.borrow_checker.analyze_stmt(stm)?;
                }
            }
        }

        let mut codegen = Codegen::new();
        match &program {
            Program::Module(module) => {
                codegen.generate(module);
            }
            Program::Script(script) => {
                codegen.generate_script(script);
            }
        }

        Ok(codegen.instructions)
    }
}

/// Tracks loop context for break/continue statements
struct LoopContext {
    /// Address of the loop condition (for continue)
    start_addr: usize,
    /// Addresses of break jumps that need to be backpatched
    break_jumps: Vec<usize>,
}

pub struct Codegen {
    pub instructions: Vec<OpCode>,
    scope_stack: Vec<Vec<String>>,
    in_function: bool,
    /// Tracks which variables are available in the current scope chain.
    /// Used to detect "upvars" (variables captured from outer scopes).
    outer_scope_vars: HashSet<String>,
    /// Stack of loop contexts for nested loops (break/continue support)
    loop_stack: Vec<LoopContext>,
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            scope_stack: vec![Vec::new()],
            in_function: false,
            outer_scope_vars: HashSet::new(),
            loop_stack: Vec::new(),
        }
    }

    /// Collects all free variables (identifiers used but not defined) in a function body.
    /// These are candidates for "capture" from the enclosing scope.
    fn collect_free_vars_in_body(&self, body: &BlockStmt, params: &[String]) -> HashSet<String> {
        let mut free_vars = HashSet::new();
        let param_set: HashSet<_> = params.iter().cloned().collect();

        for stmt in &body.stmts {
            self.collect_free_vars_in_stmt(stmt, &param_set, &mut free_vars);
        }
        free_vars
    }

    fn collect_free_vars_in_stmt(
        &self,
        stmt: &Stmt,
        local_vars: &HashSet<String>,
        free_vars: &mut HashSet<String>,
    ) {
        match stmt {
            Stmt::Expr(expr_stmt) => {
                self.collect_free_vars_in_expr(&expr_stmt.expr, local_vars, free_vars);
            }
            Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    self.collect_free_vars_in_expr(arg, local_vars, free_vars);
                }
            }
            Stmt::Block(block) => {
                for s in &block.stmts {
                    self.collect_free_vars_in_stmt(s, local_vars, free_vars);
                }
            }
            Stmt::Decl(Decl::Var(var_decl)) => {
                for init in var_decl.decls.iter().filter_map(|d| d.init.as_ref()) {
                    self.collect_free_vars_in_expr(init, local_vars, free_vars);
                }
            }
            Stmt::While(while_stmt) => {
                self.collect_free_vars_in_expr(&while_stmt.test, local_vars, free_vars);
                self.collect_free_vars_in_stmt(&while_stmt.body, local_vars, free_vars);
            }
            _ => {}
        }
    }

    fn collect_free_vars_in_expr(
        &self,
        expr: &Expr,
        local_vars: &HashSet<String>,
        free_vars: &mut HashSet<String>,
    ) {
        match expr {
            Expr::Ident(id) => {
                let name = id.sym.to_string();
                // If not a local/param AND exists in outer scope, it's a captured var
                if !local_vars.contains(&name) && self.outer_scope_vars.contains(&name) {
                    free_vars.insert(name);
                }
            }
            Expr::Bin(bin) => {
                self.collect_free_vars_in_expr(&bin.left, local_vars, free_vars);
                self.collect_free_vars_in_expr(&bin.right, local_vars, free_vars);
            }
            Expr::Call(call) => {
                for arg in &call.args {
                    self.collect_free_vars_in_expr(&arg.expr, local_vars, free_vars);
                }
                if let Callee::Expr(callee) = &call.callee {
                    self.collect_free_vars_in_expr(callee, local_vars, free_vars);
                }
            }
            Expr::Member(member) => {
                self.collect_free_vars_in_expr(&member.obj, local_vars, free_vars);
                if let MemberProp::Computed(computed) = &member.prop {
                    self.collect_free_vars_in_expr(&computed.expr, local_vars, free_vars);
                }
            }
            Expr::Object(obj) => {
                for prop in &obj.props {
                    if let PropOrSpread::Prop(p) = prop
                        && let Prop::KeyValue(kv) = p.as_ref()
                    {
                        self.collect_free_vars_in_expr(&kv.value, local_vars, free_vars);
                    }
                }
            }
            Expr::Array(arr) => {
                for e in arr.elems.iter().flatten() {
                    self.collect_free_vars_in_expr(&e.expr, local_vars, free_vars);
                }
            }
            Expr::Assign(assign) => {
                self.collect_free_vars_in_expr(&assign.right, local_vars, free_vars);
            }
            _ => {}
        }
    }

    /// Collects free vars from an arrow function expression body
    fn collect_free_vars_in_arrow_body(
        &self,
        body: &BlockStmtOrExpr,
        params: &[String],
    ) -> HashSet<String> {
        let mut free_vars = HashSet::new();
        let param_set: HashSet<_> = params.iter().cloned().collect();

        match body {
            BlockStmtOrExpr::Expr(e) => {
                self.collect_free_vars_in_expr(e, &param_set, &mut free_vars);
            }
            BlockStmtOrExpr::BlockStmt(block) => {
                for stmt in &block.stmts {
                    self.collect_free_vars_in_stmt(stmt, &param_set, &mut free_vars);
                }
            }
        }
        free_vars
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

    pub fn generate_script(&mut self, script: &Script) -> Vec<OpCode> {
        for stmt in &script.body {
            self.gen_stmt(stmt);
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
                        self.instructions.push(OpCode::Store(name.clone()));
                        // Track this variable so inner functions can capture it
                        self.outer_scope_vars.insert(name);
                    }
                }
            }
            Stmt::Decl(Decl::Fn(fn_decl)) => {
                let name = fn_decl.ident.sym.to_string();

                // Function declarations are hoisted and don't typically capture outer scope vars
                // (they're defined at the top level or function level). We'll support captures
                // for consistency but top-level functions usually won't have any.

                // 1. Push function address and store it
                let start_ip = self.instructions.len() + 3; // +3 to skip Push, Store, and Jump
                self.instructions.push(OpCode::Push(JsValue::Function {
                    address: start_ip,
                    env: None, // Named function declarations typically don't capture
                }));
                self.instructions.push(OpCode::Store(name.clone()));

                // Track this function name in outer scope
                self.outer_scope_vars.insert(name);

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

                // 2. Push loop context for break/continue support
                self.loop_stack.push(LoopContext {
                    start_addr: loop_start,
                    break_jumps: Vec::new(),
                });

                // 3. Compile the condition
                self.gen_expr(&while_stmt.test);

                // 4. Jump to the end if the condition is false
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0)); // Placeholder

                // 5. Compile the loop body
                self.gen_stmt(&while_stmt.body);

                // 6. Jump back to the start to re-check the condition
                self.instructions.push(OpCode::Jump(loop_start));

                // 7. Backpatch the exit jump and all break jumps
                let loop_end = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }

                // 8. Pop loop context and backpatch break jumps
                if let Some(loop_ctx) = self.loop_stack.pop() {
                    for break_idx in loop_ctx.break_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[break_idx] {
                            *addr = loop_end;
                        }
                    }
                }
            }
            Stmt::Break(_) => {
                // Break: jump to end of current loop (address will be backpatched)
                if let Some(loop_ctx) = self.loop_stack.last_mut() {
                    let jump_idx = self.instructions.len();
                    self.instructions.push(OpCode::Jump(0)); // Placeholder, will be backpatched
                    loop_ctx.break_jumps.push(jump_idx);
                }
                // If not in a loop, silently ignore (could add error handling)
            }
            Stmt::Continue(_) => {
                // Continue: jump back to loop condition
                if let Some(loop_ctx) = self.loop_stack.last() {
                    self.instructions.push(OpCode::Jump(loop_ctx.start_addr));
                }
                // If not in a loop, silently ignore (could add error handling)
            }
            Stmt::If(if_stmt) => {
                // Compile the condition
                self.gen_expr(&if_stmt.test);

                // Jump to else branch (or end) if condition is false
                let else_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0)); // Placeholder

                // Compile the if body
                self.gen_stmt(&if_stmt.cons);

                // If there's an else branch, we need to jump over it after the if body
                let has_else = if_stmt.alt.is_some();
                let end_jump_idx = if has_else {
                    Some(self.instructions.len())
                } else {
                    None
                };
                if has_else {
                    self.instructions.push(OpCode::Jump(0)); // Placeholder - jump to end
                }

                // Backpatch the jump to else branch (or end if no else)
                let else_start = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[else_jump_idx] {
                    *addr = else_start;
                }

                // Compile the else branch if it exists
                if let Some(alt) = &if_stmt.alt {
                    self.gen_stmt(alt);
                }

                // Backpatch the jump over else branch (if it exists)
                if let Some(end_idx) = end_jump_idx {
                    let end_addr = self.instructions.len();
                    if let OpCode::Jump(ref mut addr) = self.instructions[end_idx] {
                        *addr = end_addr;
                    }
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
                // CLOSURE CAPTURING: Like arrow functions, we detect and lift captured variables.

                // 1. Collect parameter names
                let params: Vec<String> = fn_expr
                    .function
                    .params
                    .iter()
                    .filter_map(|p| {
                        if let Pat::Ident(id) = &p.pat {
                            Some(id.id.sym.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();

                // 2. Detect captured variables from outer scopes
                let captured_vars = if let Some(body) = &fn_expr.function.body {
                    self.collect_free_vars_in_body(body, &params)
                } else {
                    HashSet::new()
                };
                let has_captures = !captured_vars.is_empty();

                if has_captures {
                    println!(
                        "DEBUG: Function expression captures variables: {:?}",
                        captured_vars.iter().collect::<Vec<_>>()
                    );

                    // Create Environment Object
                    self.instructions.push(OpCode::NewObject);

                    // Move captured variables into the Environment Object
                    for var_name in &captured_vars {
                        self.instructions.push(OpCode::Dup);
                        self.instructions.push(OpCode::Load(var_name.clone()));
                        self.instructions.push(OpCode::SetProp(var_name.clone()));
                    }

                    let start_ip = self.instructions.len() + 2;
                    self.instructions.push(OpCode::MakeClosure(start_ip));
                } else {
                    let start_ip = self.instructions.len() + 2;
                    self.instructions.push(OpCode::Push(JsValue::Function {
                        address: start_ip,
                        env: None,
                    }));
                }

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
                //
                // CLOSURE CAPTURING: If this arrow references variables from an outer scope,
                // we "lift" those variables to the heap by creating an Environment Object.
                // This solves the Stack Frame Paradox for async callbacks like setTimeout.

                // 1. Collect parameter names
                let params: Vec<String> = arrow
                    .params
                    .iter()
                    .filter_map(|p| {
                        if let Pat::Ident(id) = p {
                            Some(id.id.sym.to_string())
                        } else {
                            None
                        }
                    })
                    .collect();

                // 2. Detect captured variables (upvars) from outer scopes
                let captured_vars = self.collect_free_vars_in_arrow_body(&arrow.body, &params);
                let has_captures = !captured_vars.is_empty();

                if has_captures {
                    println!(
                        "DEBUG: Arrow captures variables: {:?}",
                        captured_vars.iter().collect::<Vec<_>>()
                    );

                    // 3. Create Environment Object on the Heap
                    self.instructions.push(OpCode::NewObject);

                    // 4. Move captured variables into the Environment Object
                    for var_name in &captured_vars {
                        self.instructions.push(OpCode::Dup); // Keep env ptr
                        self.instructions.push(OpCode::Load(var_name.clone())); // Load value
                        self.instructions.push(OpCode::SetProp(var_name.clone())); // Store in env
                    }

                    // 5. Calculate function body start address
                    // Layout: ... MakeClosure Jump [body...] ...
                    let start_ip = self.instructions.len() + 2; // MakeClosure + Jump
                    self.instructions.push(OpCode::MakeClosure(start_ip));
                } else {
                    // No captures: simple function (no environment needed)
                    let start_ip = self.instructions.len() + 2; // Push + Jump
                    self.instructions.push(OpCode::Push(JsValue::Function {
                        address: start_ip,
                        env: None,
                    }));
                }

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
            Expr::Lit(Lit::Bool(b)) => {
                self.instructions
                    .push(OpCode::Push(JsValue::Boolean(b.value)));
            }
            Expr::Lit(Lit::Null(_)) => {
                self.instructions.push(OpCode::Push(JsValue::Null));
            }
            Expr::Ident(id) => {
                self.instructions.push(OpCode::Load(id.sym.to_string()));
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
                    BinaryOp::LogicalAnd => self.instructions.push(OpCode::And),
                    BinaryOp::LogicalOr => self.instructions.push(OpCode::Or),
                    _ => println!("Warning: Operator {:?} not supported", bin.op),
                }
            }
            Expr::Unary(unary) => {
                self.gen_expr(&unary.arg);
                match unary.op {
                    UnaryOp::Bang => self.instructions.push(OpCode::Not),
                    UnaryOp::Minus => self.instructions.push(OpCode::Neg),
                    _ => println!("Warning: Unary operator {:?} not supported", unary.op),
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
                if let Callee::Expr(callee_expr) = &call_expr.callee {
                    if let Expr::Member(member) = callee_expr.as_ref() {
                        for arg in &call_expr.args {
                            self.gen_expr(&arg.expr);
                        }

                        self.gen_expr(&member.obj);

                        if let MemberProp::Ident(id) = &member.prop {
                            self.instructions.push(OpCode::CallMethod(
                                id.sym.to_string(),
                                call_expr.args.len() as usize,
                            ));
                            return;
                        }
                    }
                }
                // detect if this is a 'require' call
                if let Callee::Expr(expr) = &call_expr.callee {
                    if let Expr::Ident(id) = expr.as_ref() {
                        if id.sym.to_string() == "require" {
                            if let Some(arg) = call_expr.args.first() {
                                self.gen_expr(&arg.expr);
                                self.instructions.push(OpCode::Require);
                                return;
                            }
                        }
                    }
                }

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
                // Handle different assignment targets
                match &assign_expr.left {
                    AssignTarget::Simple(simple) => match simple {
                        SimpleAssignTarget::Ident(binding_ident) => {
                            // Simple variable assignment: x = value
                            self.gen_expr(&assign_expr.right);
                            let name = binding_ident.id.sym.to_string();
                            // JS assignment expressions evaluate to the assigned value.
                            // Our `Store` opcode consumes the value, so `Dup` ensures one copy
                            // remains on the stack for expression context (e.g. `a = 1;`).
                            self.instructions.push(OpCode::Dup);
                            self.instructions.push(OpCode::Store(name));
                        }
                        SimpleAssignTarget::Member(member_expr) => {
                            // Member assignment: obj.prop = value or this.prop = value
                            // Stack order for SetProp: [object, value] -> pops both, sets prop
                            self.gen_expr(&member_expr.obj); // Push the object
                            self.gen_expr(&assign_expr.right); // Push the value

                            match &member_expr.prop {
                                MemberProp::Ident(id) => {
                                    // obj.prop = value
                                    self.instructions.push(OpCode::SetProp(id.sym.to_string()));
                                }
                                MemberProp::Computed(computed) => {
                                    // obj[key] = value - need StoreElement
                                    // Stack: [obj, value, index]
                                    self.gen_expr(&computed.expr); // Push the index
                                    self.instructions.push(OpCode::StoreElement);
                                }
                                _ => println!("Warning: Private field assignment not supported."),
                            }
                        }
                        _ => println!("Warning: Complex assignment target not supported."),
                    },
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
            Expr::This(_) => {
                self.instructions.push(OpCode::LoadThis);
            }
            Expr::New(new_expr) => {
                // new Foo(arg1, arg2) compiles to:
                // 1. Create new empty object that will be `this`
                self.instructions.push(OpCode::NewObject);
                self.instructions.push(OpCode::Dup); // Keep a copy for return value

                // 2. Push arguments
                let arg_count = new_expr.args.as_ref().map(|a| a.len()).unwrap_or(0);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        self.gen_expr(&arg.expr);
                    }
                }

                // 3. Push the constructor function
                self.gen_expr(&new_expr.callee);

                // 4. Call with construct semantics
                self.instructions.push(OpCode::Construct(arg_count));
            }
            Expr::Paren(paren_expr) => {
                // Parenthesized expression: just evaluate the inner expression
                self.gen_expr(&paren_expr.expr);
            }
            _ => {}
        }
    }
}
