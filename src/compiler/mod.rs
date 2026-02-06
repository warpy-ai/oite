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

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            borrow_checker: BorrowChecker::new(),
        }
    }

    pub fn compile(&mut self, source: &str) -> Result<Vec<OpCode>, String> {
        self.compile_with_syntax(source, None)
    }

    pub fn compile_with_syntax(
        &mut self,
        source: &str,
        syntax_override: Option<Syntax>,
    ) -> Result<Vec<OpCode>, String> {
        let cm: Lrc<SourceMap> = Default::default();
        let fm = cm.new_source_file(
            FileName::Custom("main.ot".into()).into(),
            source.to_string(),
        );

        // Determine syntax based on file extension or override
        let syntax = syntax_override.unwrap_or_else(|| {
            // Default to TypeScript syntax to support type annotations
            Syntax::Typescript(Default::default())
        });

        let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
        let mut parser = Parser::new_from(lexer);
        let program = parser
            .parse_program()
            .map_err(|e| format!("Parsing error: {:?}", e))?;

        self.borrow_checker.enter_scope(); // Script vars at depth 1, globals at 0

        let result = match &program {
            Program::Module(module) => {
                let mut result = Ok(());
                for item in &module.body {
                    if let ModuleItem::Stmt(stmt) = item
                        && let Err(e) = self.borrow_checker.analyze_stmt(stmt)
                    {
                        result = Err(e);
                        break;
                    }
                }
                result
            }
            Program::Script(script) => {
                let mut result = Ok(());
                for stm in &script.body {
                    if let Err(e) = self.borrow_checker.analyze_stmt(stm) {
                        result = Err(e);
                        break;
                    }
                }
                result
            }
        };

        self.borrow_checker.exit_scope();
        result?;

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

struct LoopContext {
    start_addr: usize,
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
}

pub struct Codegen {
    pub instructions: Vec<OpCode>,
    scope_stack: Vec<Vec<String>>,
    in_function: bool,
    in_async_function: bool,
    /// Tracks which variables are available in the current scope chain.
    /// Used to detect "upvars" (variables captured from outer scopes).
    outer_scope_vars: HashSet<String>,
    /// Stack of loop contexts for nested loops (break/continue support)
    loop_stack: Vec<LoopContext>,
    /// Maps private field names to their indices for the current class
    private_field_indices: std::collections::HashMap<String, usize>,
    /// Maps private method names to their indices for the current class
    private_method_indices: std::collections::HashMap<String, usize>,
    /// Warnings collected during compilation
    pub warnings: Vec<String>,
}

impl Default for Codegen {
    fn default() -> Self {
        Self::new()
    }
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            scope_stack: vec![Vec::new()],
            in_function: false,
            in_async_function: false,
            outer_scope_vars: HashSet::new(),
            loop_stack: Vec::new(),
            private_field_indices: std::collections::HashMap::new(),
            private_method_indices: std::collections::HashMap::new(),
            warnings: Vec::new(),
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
            match item {
                ModuleItem::Stmt(stmt) => {
                    self.gen_stmt(stmt);
                }
                ModuleItem::ModuleDecl(decl) => {
                    self.gen_module_decl(decl);
                }
            }
        }
        self.instructions.push(OpCode::Halt);
        self.instructions.clone()
    }

    fn gen_module_decl(&mut self, decl: &ModuleDecl) {
        match decl {
            ModuleDecl::ExportDecl(export_decl) => {
                self.gen_decl(&export_decl.decl);
            }
            ModuleDecl::ExportDefaultDecl(export_default) => match &export_default.decl {
                DefaultDecl::Class(class_expr) => {
                    self.gen_class(
                        &class_expr.class,
                        class_expr.ident.as_ref().map(|id| id.sym.as_str()),
                    );
                }
                DefaultDecl::Fn(fn_expr) => {
                    let name = if let Some(id) = fn_expr.ident.clone() {
                        Some(id.sym.to_string())
                    } else {
                        None
                    };
                    self.gen_fn_decl(name, &fn_expr.function);
                }
                _ => {}
            },
            ModuleDecl::ExportDefaultExpr(export_default) => match &*export_default.expr {
                Expr::Fn(fn_expr) => {
                    let name = if let Some(id) = fn_expr.ident.clone() {
                        Some(id.sym.to_string())
                    } else {
                        None
                    };
                    self.gen_fn_decl(name, &fn_expr.function);
                }
                Expr::Class(class_expr) => {
                    self.gen_class(
                        &class_expr.class,
                        class_expr.ident.as_ref().map(|id| id.sym.as_str()),
                    );
                }
                _ => {}
            },
            ModuleDecl::Import(import) => {
                let src = import.src.value.to_string_lossy().into_owned();

                for spec in &import.specifiers {
                    match spec {
                        ImportSpecifier::Named(named) => {
                            let local = named.local.sym.to_string();
                            let imported = named
                                .imported
                                .as_ref()
                                .map(|i| {
                                    let atom = i.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                })
                                .unwrap_or_else(|| local.clone());

                            self.instructions
                                .push(OpCode::Push(JsValue::String(src.clone())));
                            self.instructions.push(OpCode::ImportAsync(src.clone()));
                            self.instructions.push(OpCode::GetExport {
                                name: imported.clone(),
                                is_default: false,
                            });
                            self.instructions.push(OpCode::Let(local));
                        }
                        ImportSpecifier::Default(default) => {
                            let local = default.local.sym.to_string();

                            self.instructions
                                .push(OpCode::Push(JsValue::String(src.clone())));
                            self.instructions.push(OpCode::ImportAsync(src.clone()));
                            self.instructions.push(OpCode::GetExport {
                                name: "default".to_string(),
                                is_default: true,
                            });
                            self.instructions.push(OpCode::Let(local));
                        }
                        ImportSpecifier::Namespace(ns) => {
                            let local = ns.local.sym.to_string();

                            self.instructions
                                .push(OpCode::Push(JsValue::String(src.clone())));
                            self.instructions.push(OpCode::ImportAsync(src.clone()));
                            self.instructions.push(OpCode::Let(local));
                        }
                    }
                }

                if import.specifiers.is_empty() {
                    self.instructions
                        .push(OpCode::Push(JsValue::String(src.clone())));
                    self.instructions.push(OpCode::ImportAsync(src.clone()));
                    self.instructions.push(OpCode::Pop);
                }

                if let Some(_with) = &import.with {
                    self.warnings.push(format!(
                        "Warning: Import assertions for '{}' are not fully supported",
                        import.src.value.to_string_lossy()
                    ));
                }
            }
            ModuleDecl::ExportNamed(named) => {
                if let Some(src) = &named.src {
                    let src_str = src.value.to_string_lossy().into_owned();
                    for spec in &named.specifiers {
                        match spec {
                            ExportSpecifier::Named(named) => {
                                let export_name = named
                                    .exported
                                    .as_ref()
                                    .map(|e| {
                                        let atom = e.atom();
                                        let s: &str = &atom;
                                        s.to_string()
                                    })
                                    .unwrap_or_else(|| {
                                        let atom = named.orig.atom();
                                        let s: &str = &atom;
                                        s.to_string()
                                    });
                                let _local_name = {
                                    let atom = named.orig.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                };

                                self.instructions
                                    .push(OpCode::Push(JsValue::String(src_str.clone())));
                                self.instructions.push(OpCode::ImportAsync(src_str.clone()));
                                self.instructions.push(OpCode::GetExport {
                                    name: export_name.clone(),
                                    is_default: false,
                                });
                                self.instructions.push(OpCode::Let(export_name.clone()));
                                self.instructions.push(OpCode::Load(export_name.clone()));
                                self.instructions.push(OpCode::Store(export_name));
                            }
                            ExportSpecifier::Default(_) => {
                                self.instructions
                                    .push(OpCode::Push(JsValue::String(src_str.clone())));
                                self.instructions.push(OpCode::ImportAsync(src_str.clone()));
                                self.instructions.push(OpCode::GetExport {
                                    name: "default".to_string(),
                                    is_default: true,
                                });
                                self.instructions.push(OpCode::Let("default".to_string()));
                                self.instructions.push(OpCode::Load("default".to_string()));
                                self.instructions.push(OpCode::Store("default".to_string()));
                            }
                            ExportSpecifier::Namespace(ns) => {
                                let name = {
                                    let atom = ns.name.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                };
                                self.instructions
                                    .push(OpCode::Push(JsValue::String(src_str.clone())));
                                self.instructions.push(OpCode::ImportAsync(src_str.clone()));
                                self.instructions.push(OpCode::Let(name.clone()));
                            }
                        }
                    }
                } else {
                    for spec in &named.specifiers {
                        match spec {
                            ExportSpecifier::Named(named) => {
                                let export_name = named
                                    .exported
                                    .as_ref()
                                    .map(|e| {
                                        let atom = e.atom();
                                        let s: &str = &atom;
                                        s.to_string()
                                    })
                                    .unwrap_or_else(|| {
                                        let atom = named.orig.atom();
                                        let s: &str = &atom;
                                        s.to_string()
                                    });
                                let local_name = {
                                    let atom = named.orig.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                };

                                self.instructions.push(OpCode::Load(local_name));
                                self.instructions.push(OpCode::Dup);
                                self.instructions.push(OpCode::Store(export_name));
                            }
                            ExportSpecifier::Default(_) => {
                                self.instructions.push(OpCode::Load("default".to_string()));
                                self.instructions.push(OpCode::Dup);
                                self.instructions.push(OpCode::Store("default".to_string()));
                            }
                            ExportSpecifier::Namespace(ns) => {
                                let name = {
                                    let atom = ns.name.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                };
                                self.instructions.push(OpCode::Load(name.clone()));
                                self.instructions.push(OpCode::Dup);
                                self.instructions.push(OpCode::Store(name));
                            }
                        }
                    }
                }
            }
            ModuleDecl::ExportAll(all) => {
                let src_str = all.src.value.to_string_lossy().into_owned();
                self.warnings.push(format!(
                    "Warning: 'export * from' is not yet fully implemented for '{}'",
                    src_str
                ));
                self.instructions
                    .push(OpCode::Push(JsValue::String(src_str.clone())));
                self.instructions.push(OpCode::ImportAsync(src_str.clone()));
                self.instructions.push(OpCode::Pop);
            }
            ModuleDecl::TsImportEquals(_) => {}
            ModuleDecl::TsExportAssignment(_) => {}
            ModuleDecl::TsNamespaceExport(_) => {}
        }
    }

    fn gen_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Fn(fn_decl) => {
                let name = fn_decl.ident.sym.to_string();
                self.gen_fn_decl(Some(name), &fn_decl.function);
            }
            Decl::Class(class_decl) => {
                self.gen_class(&class_decl.class, Some(class_decl.ident.sym.as_str()));
            }
            Decl::Var(var_decl) => {
                self.gen_var_decl(var_decl);
            }
            Decl::TsEnum(enum_decl) => {
                // Generate enum as a runtime object
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
                self.instructions.push(OpCode::Store(enum_name.clone()));
                self.outer_scope_vars.insert(enum_name);
            }
            Decl::TsModule(_) => {
                // TypeScript modules are compile-time only, skip
            }
            Decl::TsInterface(_) => {
                // Interfaces are compile-time only, skip
            }
            Decl::TsTypeAlias(_) => {
                // Type aliases are compile-time only, skip
            }
            _ => {}
        }
    }

    pub fn generate_script(&mut self, script: &Script) -> Vec<OpCode> {
        for stmt in &script.body {
            self.gen_stmt(stmt);
        }
        self.instructions.push(OpCode::Halt);
        self.instructions.clone()
    }

    fn gen_fn_decl(&mut self, name: Option<String>, fn_decl: &Function) {
        let is_async = fn_decl.is_async;

        // For named function declarations, store them in the current scope
        // Anonymous functions are not stored (they're just values)
        let has_name = name.is_some();
        let start_ip = if let Some(ref name) = name {
            // 1. Push function address and store it
            let ip = self.instructions.len() + 3; // +3 to skip Push, Let, and Jump
            self.instructions.push(OpCode::Push(JsValue::Function {
                address: ip,
                env: None, // Named function declarations typically don't capture
            }));
            self.instructions.push(OpCode::Let(name.clone()));

            // Track this function name in outer scope
            self.outer_scope_vars.insert(name.clone());

            // 2. Add jump to skip over function body
            let jump_target = self.instructions.len() + 1; // Will be updated after compiling body
            self.instructions.push(OpCode::Jump(jump_target));

            ip
        } else {
            // For anonymous functions, just push the function value on the stack
            // The actual function address will be set after the body
            self.instructions.len()
        };

        // 3. Compile function body
        self.in_function = true;
        self.in_async_function = is_async;

        // Inside the function body, we must pop arguments into locals
        // We process them in REVERSE order because of how they sit on the stack
        for param in fn_decl.params.iter().rev() {
            if let Pat::Ident(id) = &param.pat {
                let param_name = id.id.sym.to_string();
                // The value is already on the stack from the Caller
                // Parameters are new bindings in the function scope
                self.instructions.push(OpCode::Let(param_name));
            }
        }
        let stmts = &fn_decl.body.as_ref().unwrap().stmts;

        let mut last_instr_was_return = false;
        for s in stmts {
            let before = self.instructions.len();
            self.gen_stmt(s);
            // Check if the last instruction emitted was a Return
            last_instr_was_return = self.instructions.len() > before
                && matches!(self.instructions.last(), Some(OpCode::Return));
        }

        self.in_function = false;
        self.in_async_function = false;

        // If the last statement wasn't a return, we need to handle implicit return
        if !last_instr_was_return {
            if stmts.is_empty() {
                // Empty function body - return undefined
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                // For async functions, wrap in Promise.resolve()
                if is_async {
                    self.instructions
                        .push(OpCode::Push(JsValue::String("Promise".to_string())));
                    self.instructions.push(OpCode::Load("Promise".to_string()));
                    self.instructions
                        .push(OpCode::Push(JsValue::String("resolve".to_string())));
                    self.instructions
                        .push(OpCode::GetProp("resolve".to_string()));
                    // Stack: [undefined, Promise, PromiseObj, resolveFn]
                    // Pop PromiseObj and Promise, keeping resolveFn and undefined
                    self.instructions.push(OpCode::Pop);
                    self.instructions.push(OpCode::Pop);
                    // Stack: [undefined, resolveFn]
                    // Swap to get [resolveFn, undefined]
                    self.instructions.push(OpCode::Swap);
                    self.instructions.push(OpCode::Call(1));
                }
            } else {
                // Non-empty body but last statement wasn't a return
                // The last expression's result is on the stack, need to return it
                // For async functions, wrap in Promise.resolve()
                if is_async {
                    self.instructions
                        .push(OpCode::Push(JsValue::String("Promise".to_string())));
                    self.instructions.push(OpCode::Load("Promise".to_string()));
                    self.instructions
                        .push(OpCode::Push(JsValue::String("resolve".to_string())));
                    self.instructions
                        .push(OpCode::GetProp("resolve".to_string()));
                    // Stack: [returnValue, Promise, PromiseObj, resolveFn]
                    // Pop PromiseObj and Promise, keeping resolveFn
                    self.instructions.push(OpCode::Pop);
                    self.instructions.push(OpCode::Pop);
                    // Stack: [returnValue, resolveFn]
                    // Swap to get [resolveFn, returnValue]
                    self.instructions.push(OpCode::Swap);
                    self.instructions.push(OpCode::Call(1));
                }
            }
            self.instructions.push(OpCode::Return);
        }

        // 4. Update jump target to point after the function body (for named functions)
        if has_name {
            let current_len = self.instructions.len();
            if let OpCode::Jump(ref mut target) = self.instructions[start_ip - 1] {
                *target = current_len;
            }
        }
    }

    fn gen_var_decl(&mut self, var_decl: &VarDecl) {
        for decl in &var_decl.decls {
            if let Some(init) = &decl.init {
                self.gen_expr(init);
                self.gen_pattern_binding(&decl.name);
            }
        }
    }

    /// Generate code to bind a pattern to a value on the stack.
    /// The value to destructure should already be on top of the stack.
    fn gen_pattern_binding(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(id) => {
                // Simple variable binding
                let name = id.id.sym.to_string();
                self.instructions.push(OpCode::Let(name.clone()));
                self.outer_scope_vars.insert(name);
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
                                _ => continue,
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

                            self.instructions.push(OpCode::Let(key_name.clone()));
                            self.outer_scope_vars.insert(key_name);
                        }
                        swc_ecma_ast::ObjectPatProp::Rest(rest) => {
                            // { ...rest } - not fully implemented yet
                            if let Pat::Ident(id) = rest.arg.as_ref() {
                                let name = id.id.sym.to_string();
                                // For now, just bind the remaining object
                                self.instructions.push(OpCode::Let(name.clone()));
                                self.outer_scope_vars.insert(name);
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
            _ => {
                // Unsupported pattern - just pop the value
                self.instructions.push(OpCode::Pop);
            }
        }
    }

    fn gen_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return(ret_stmt) => {
                if let Some(arg) = &ret_stmt.arg {
                    self.gen_expr(arg); // Pushes the return value to stack
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Undefined));
                }
                // For async functions, wrap the return value in Promise.resolve()
                if self.in_async_function {
                    self.instructions
                        .push(OpCode::Push(JsValue::String("Promise".to_string())));
                    self.instructions.push(OpCode::Load("Promise".to_string()));
                    self.instructions
                        .push(OpCode::Push(JsValue::String("resolve".to_string())));
                    self.instructions
                        .push(OpCode::GetProp("resolve".to_string()));
                    self.instructions.push(OpCode::Swap);
                    self.instructions.push(OpCode::Call(1));
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
                self.gen_var_decl(var_decl);
            }
            Stmt::Decl(Decl::Fn(fn_decl)) => {
                let name = fn_decl.ident.sym.to_string();
                self.gen_fn_decl(Some(name), &fn_decl.function);
            }
            Stmt::Decl(Decl::Class(class_decl)) => {
                let class_name = class_decl.ident.sym.to_string();
                self.gen_class(&class_decl.class, Some(class_name.as_str()));
                self.instructions.push(OpCode::Let(class_name.clone()));
                self.outer_scope_vars.insert(class_name);
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
                self.outer_scope_vars.insert(enum_name);
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
                let loop_start = self.instructions.len();
                self.loop_stack.push(LoopContext {
                    start_addr: loop_start,
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                });
                self.gen_expr(&while_stmt.test);
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.gen_stmt(&while_stmt.body);
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                if let Some(loop_ctx) = self.loop_stack.pop() {
                    for break_idx in loop_ctx.break_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[break_idx] {
                            *addr = loop_end;
                        }
                    }
                    for cont_idx in loop_ctx.continue_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[cont_idx] {
                            *addr = loop_start;
                        }
                    }
                }
            }
            Stmt::Break(_) => {
                if let Some(loop_ctx) = self.loop_stack.last_mut() {
                    let jump_idx = self.instructions.len();
                    self.instructions.push(OpCode::Jump(0));
                    loop_ctx.break_jumps.push(jump_idx);
                }
            }
            Stmt::Continue(_) => {
                if let Some(loop_ctx) = self.loop_stack.last_mut() {
                    let jump_idx = self.instructions.len();
                    self.instructions.push(OpCode::Jump(0));
                    loop_ctx.continue_jumps.push(jump_idx);
                }
            }
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
                let loop_start = self.instructions.len();
                self.loop_stack.push(LoopContext {
                    start_addr: loop_start,
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                });
                if let Some(test) = &for_stmt.test {
                    self.gen_expr(test);
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
                }
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.gen_stmt(&for_stmt.body);
                let continue_target = self.instructions.len();
                if let Some(update) = &for_stmt.update {
                    self.gen_expr(update);
                    self.instructions.push(OpCode::Pop);
                }
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                if let Some(loop_ctx) = self.loop_stack.pop() {
                    for break_idx in loop_ctx.break_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[break_idx] {
                            *addr = loop_end;
                        }
                    }
                    for cont_idx in loop_ctx.continue_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[cont_idx] {
                            *addr = continue_target;
                        }
                    }
                }
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::ForOf(for_of_stmt) => {
                self.scope_stack.push(Vec::new());
                self.gen_expr(&for_of_stmt.right);
                let iter_name = "__for_of_iter__".to_string();
                self.instructions.push(OpCode::Let(iter_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(iter_name.clone());
                }
                self.instructions.push(OpCode::Push(JsValue::Number(0.0)));
                let idx_name = "__for_of_idx__".to_string();
                self.instructions.push(OpCode::Let(idx_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(idx_name.clone());
                }
                let loop_start = self.instructions.len();
                self.loop_stack.push(LoopContext {
                    start_addr: loop_start,
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                });
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
                if let Some(var_decl) = &for_of_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    let var_name = id.id.sym.to_string();
                    self.instructions.push(OpCode::Let(var_name.clone()));
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.push(var_name);
                    }
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
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                if let Some(loop_ctx) = self.loop_stack.pop() {
                    for break_idx in loop_ctx.break_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[break_idx] {
                            *addr = loop_end;
                        }
                    }
                    for cont_idx in loop_ctx.continue_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[cont_idx] {
                            *addr = continue_target;
                        }
                    }
                }
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::ForIn(for_in_stmt) => {
                self.scope_stack.push(Vec::new());
                self.gen_expr(&for_in_stmt.right);
                self.instructions.push(OpCode::Load("Object".to_string()));
                self.instructions.push(OpCode::GetProp("keys".to_string()));
                self.instructions.push(OpCode::Call(1));
                let keys_name = "__for_in_keys__".to_string();
                self.instructions.push(OpCode::Let(keys_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(keys_name.clone());
                }
                self.instructions.push(OpCode::Push(JsValue::Number(0.0)));
                let idx_name = "__for_in_idx__".to_string();
                self.instructions.push(OpCode::Let(idx_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(idx_name.clone());
                }
                let loop_start = self.instructions.len();
                self.loop_stack.push(LoopContext {
                    start_addr: loop_start,
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                });
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
                if let Some(var_decl) = &for_in_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    let var_name = id.id.sym.to_string();
                    self.instructions.push(OpCode::Let(var_name.clone()));
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.push(var_name);
                    }
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
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                if let Some(loop_ctx) = self.loop_stack.pop() {
                    for break_idx in loop_ctx.break_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[break_idx] {
                            *addr = loop_end;
                        }
                    }
                    for cont_idx in loop_ctx.continue_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[cont_idx] {
                            *addr = continue_target;
                        }
                    }
                }
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::DoWhile(do_while_stmt) => {
                let loop_start = self.instructions.len();
                self.loop_stack.push(LoopContext {
                    start_addr: loop_start,
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                });
                self.gen_stmt(&do_while_stmt.body);
                let continue_target = self.instructions.len();
                self.gen_expr(&do_while_stmt.test);
                self.instructions
                    .push(OpCode::JumpIfFalse(self.instructions.len() + 2));
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                if let Some(loop_ctx) = self.loop_stack.pop() {
                    for break_idx in loop_ctx.break_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[break_idx] {
                            *addr = loop_end;
                        }
                    }
                    for cont_idx in loop_ctx.continue_jumps {
                        if let OpCode::Jump(ref mut addr) = self.instructions[cont_idx] {
                            *addr = continue_target;
                        }
                    }
                }
            }
            Stmt::Empty(_) | Stmt::Debugger(_) | Stmt::With(_) => {
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

                // 1. Emit SetupTry with placeholder addresses (will backpatch)
                let setup_try_idx = self.instructions.len();
                self.instructions.push(OpCode::SetupTry {
                    catch_addr: 0,
                    finally_addr: 0,
                });

                // 2. Emit try block
                self.scope_stack.push(Vec::new());
                for s in &try_stmt.block.stmts {
                    self.gen_stmt(s);
                }
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

                        // Bind exception to catch parameter if present
                        if let Some(param) = &handler.param {
                            if let Pat::Ident(id) = param {
                                let param_name = id.id.sym.to_string();
                                // Exception value is on stack, bind it
                                self.instructions.push(OpCode::Let(param_name.clone()));
                                if let Some(scope) = self.scope_stack.last_mut() {
                                    scope.push(param_name);
                                }
                            }
                        } else {
                            // No catch parameter, pop the exception
                            self.instructions.push(OpCode::Pop);
                        }

                        // Generate catch block body
                        for s in &handler.body.stmts {
                            self.gen_stmt(s);
                        }

                        // Drop catch block scope variables
                        if let Some(locals) = self.scope_stack.pop() {
                            for name in locals.into_iter().rev() {
                                self.instructions.push(OpCode::Drop(name));
                            }
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
                        self.scope_stack.push(Vec::new());
                        for s in &finalizer.stmts {
                            self.gen_stmt(s);
                        }
                        // Drop finally block scope variables
                        if let Some(locals) = self.scope_stack.pop() {
                            for name in locals.into_iter().rev() {
                                self.instructions.push(OpCode::Drop(name));
                            }
                        }
                    }
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
                self.instructions.push(OpCode::Throw);
            }
            _ => {}
        }
    }

    fn gen_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Fn(fn_expr) => {
                let is_async = fn_expr.function.is_async;

                // Function expression: `function(a, b) { ... }` or `async function(a, b) { ... }`
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
                let prev_async = self.in_async_function;
                self.in_function = true;
                self.in_async_function = is_async;

                // Pop args into locals (reverse order)
                // Parameters are new bindings in the function scope
                for param in fn_expr.function.params.iter().rev() {
                    if let Pat::Ident(id) = &param.pat {
                        let param_name = id.id.sym.to_string();
                        self.instructions.push(OpCode::Let(param_name));
                    }
                }

                if let Some(body) = &fn_expr.function.body {
                    let stmts = &body.stmts;
                    let mut last_instr_was_return = false;

                    for s in stmts {
                        let before = self.instructions.len();
                        self.gen_stmt(s);
                        last_instr_was_return = self.instructions.len() > before
                            && matches!(self.instructions.last(), Some(OpCode::Return));
                    }

                    // For async functions with no return statement at the end, wrap the result
                    if is_async && !last_instr_was_return {
                        if stmts.is_empty() {
                            self.instructions.push(OpCode::Push(JsValue::Undefined));
                        }
                        // Wrap in Promise.resolve() and add Return
                        self.instructions
                            .push(OpCode::Push(JsValue::String("Promise".to_string())));
                        self.instructions.push(OpCode::Load("Promise".to_string()));
                        self.instructions
                            .push(OpCode::Push(JsValue::String("resolve".to_string())));
                        self.instructions
                            .push(OpCode::GetProp("resolve".to_string()));
                        // Stack: [returnValue, Promise, PromiseObj, resolveFn]
                        // Pop PromiseObj and Promise, keeping resolveFn
                        self.instructions.push(OpCode::Pop);
                        self.instructions.push(OpCode::Pop);
                        // Stack: [returnValue, resolveFn]
                        // Swap to get [resolveFn, returnValue]
                        self.instructions.push(OpCode::Swap);
                        self.instructions.push(OpCode::Call(1));
                        self.instructions.push(OpCode::Return);
                    }
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Undefined));
                    // For async functions with no body, wrap undefined in Promise.resolve()
                    if is_async {
                        self.instructions
                            .push(OpCode::Push(JsValue::String("Promise".to_string())));
                        self.instructions.push(OpCode::Load("Promise".to_string()));
                        self.instructions
                            .push(OpCode::Push(JsValue::String("resolve".to_string())));
                        self.instructions
                            .push(OpCode::GetProp("resolve".to_string()));
                        // Stack: [undefined, Promise, PromiseObj, resolveFn]
                        // Pop PromiseObj and Promise, keeping resolveFn
                        self.instructions.push(OpCode::Pop);
                        self.instructions.push(OpCode::Pop);
                        // Stack: [undefined, resolveFn]
                        // Swap to get [resolveFn, undefined]
                        self.instructions.push(OpCode::Swap);
                        self.instructions.push(OpCode::Call(1));
                    }
                    self.instructions.push(OpCode::Return);
                }
                self.in_function = prev_in_function;
                self.in_async_function = prev_async;

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
                let prev_async = self.in_async_function;
                self.in_function = true;
                self.in_async_function = arrow.is_async;

                // Pop args into locals (reverse order)
                // Parameters are new bindings in the function scope
                for param in arrow.params.iter().rev() {
                    if let Pat::Ident(id) = param {
                        let param_name = id.id.sym.to_string();
                        self.instructions.push(OpCode::Let(param_name));
                    } else {
                        println!("Warning: Non-identifier arrow params not supported yet.");
                    }
                }

                match &*arrow.body {
                    BlockStmtOrExpr::Expr(e) => {
                        // Expression-bodied arrows implicitly return the expression.
                        self.gen_expr(e);
                        // For async arrows, wrap the return value in Promise.resolve()
                        if arrow.is_async {
                            self.instructions
                                .push(OpCode::Push(JsValue::String("Promise".to_string())));
                            self.instructions.push(OpCode::Load("Promise".to_string()));
                            self.instructions
                                .push(OpCode::Push(JsValue::String("resolve".to_string())));
                            self.instructions
                                .push(OpCode::GetProp("resolve".to_string()));
                            // Stack: [returnValue, Promise, PromiseObj, resolveFn]
                            // Pop PromiseObj and Promise, keeping resolveFn
                            self.instructions.push(OpCode::Pop);
                            self.instructions.push(OpCode::Pop);
                            // Stack: [returnValue, resolveFn]
                            // Swap to get [resolveFn, returnValue]
                            self.instructions.push(OpCode::Swap);
                            self.instructions.push(OpCode::Call(1));
                        }
                        self.instructions.push(OpCode::Return);
                    }
                    BlockStmtOrExpr::BlockStmt(block) => {
                        let stmts = &block.stmts;
                        let mut last_instr_was_return = false;

                        for s in stmts {
                            let before = self.instructions.len();
                            self.gen_stmt(s);
                            last_instr_was_return = self.instructions.len() > before
                                && matches!(self.instructions.last(), Some(OpCode::Return));
                        }

                        if stmts.is_empty() {
                            self.instructions.push(OpCode::Push(JsValue::Undefined));
                        }

                        // For async arrows with no return statement at the end, wrap the result
                        if arrow.is_async && !last_instr_was_return {
                            self.instructions
                                .push(OpCode::Push(JsValue::String("Promise".to_string())));
                            self.instructions.push(OpCode::Load("Promise".to_string()));
                            self.instructions
                                .push(OpCode::Push(JsValue::String("resolve".to_string())));
                            self.instructions
                                .push(OpCode::GetProp("resolve".to_string()));
                            // Stack: [returnValue, Promise, PromiseObj, resolveFn]
                            // Pop PromiseObj and Promise, keeping resolveFn
                            self.instructions.push(OpCode::Pop);
                            self.instructions.push(OpCode::Pop);
                            // Stack: [returnValue, resolveFn]
                            // Swap to get [resolveFn, returnValue]
                            self.instructions.push(OpCode::Swap);
                            self.instructions.push(OpCode::Call(1));
                        }

                        if !last_instr_was_return {
                            self.instructions.push(OpCode::Return);
                        }
                    }
                }

                self.in_function = prev_in_function;
                self.in_async_function = prev_async;

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
                    BinaryOp::InstanceOf => self.instructions.push(OpCode::InstanceOf),
                    // Bitwise operators
                    BinaryOp::BitAnd => self.instructions.push(OpCode::BitAnd),
                    BinaryOp::BitOr => self.instructions.push(OpCode::BitOr),
                    BinaryOp::BitXor => self.instructions.push(OpCode::Xor),
                    BinaryOp::LShift => self.instructions.push(OpCode::ShiftLeft),
                    BinaryOp::RShift => self.instructions.push(OpCode::ShiftRight),
                    BinaryOp::ZeroFillRShift => self.instructions.push(OpCode::ShiftRightUnsigned),
                    BinaryOp::Exp => self.instructions.push(OpCode::Pow),
                    _ => println!("Warning: Operator {:?} not supported", bin.op),
                }
            }
            Expr::Unary(unary) => {
                match unary.op {
                    UnaryOp::TypeOf => {
                        self.gen_expr(&unary.arg);
                        self.instructions.push(OpCode::TypeOf);
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
                            _ => println!("Warning: Unary operator {:?} not supported", unary.op),
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
                if let Callee::Expr(callee_expr) = &call_expr.callee
                    && let Expr::Member(member) = callee_expr.as_ref()
                {
                    for arg in &call_expr.args {
                        self.gen_expr(&arg.expr);
                    }

                    self.gen_expr(&member.obj);

                    if let MemberProp::Ident(id) = &member.prop {
                        self.instructions
                            .push(OpCode::CallMethod(id.sym.to_string(), call_expr.args.len()));
                        return;
                    }
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
                // Load the function
                match &call_expr.callee {
                    Callee::Expr(expr) => self.gen_expr(expr),
                    Callee::Super(_) => {
                        // For super() calls, we need to:
                        // 1. Load __super__ from the frame's locals
                        // 2. The CallSuper opcode will use it
                        self.instructions.push(OpCode::LoadSuper);
                    }
                    Callee::Import(_) => {} // Handle import calls if needed
                }
                // Call it
                match &call_expr.callee {
                    Callee::Super(_) => {
                        // Use CallSuper opcode for super() calls
                        self.instructions.push(OpCode::CallSuper(arg_count));
                    }
                    _ => {
                        self.instructions.push(OpCode::Call(arg_count));
                    }
                }
            }
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
                                    // obj[key] = value
                                    // Stack: [obj, value, key]
                                    self.gen_expr(&computed.expr); // Push the key
                                    self.instructions.push(OpCode::SetPropComputed);
                                }
                                MemberProp::PrivateName(pn) => {
                                    // obj.#field = value - use SetPrivateProp
                                    let field_name = format!("#{}", pn.name);

                                    // If this is the first time seeing this private field, assign it an index
                                    if !self.private_field_indices.contains_key(&field_name) {
                                        let new_index = self.private_field_indices.len();
                                        self.private_field_indices
                                            .insert(field_name.clone(), new_index);
                                    }

                                    if let Some(field_index) =
                                        self.private_field_indices.get(&field_name)
                                    {
                                        self.instructions
                                            .push(OpCode::SetPrivateProp(*field_index));
                                    } else {
                                        println!(
                                            "Warning: Private field '{}' not found",
                                            field_name
                                        );
                                        // Pop the value
                                        self.instructions.push(OpCode::Pop);
                                    }
                                }
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
                                        _ => continue,
                                    };

                                    self.instructions.push(OpCode::Dup); // Duplicate Ptr
                                    self.gen_expr(&kv.value); // Push Value
                                    self.instructions.push(OpCode::SetProp(key)); // Consumes Value + 1 Ptr
                                }
                                Prop::Shorthand(ident) => {
                                    // { x } shorthand for { x: x }
                                    let key = ident.sym.to_string();
                                    self.instructions.push(OpCode::Dup);
                                    self.instructions.push(OpCode::Load(key.clone()));
                                    self.instructions.push(OpCode::SetProp(key));
                                }
                                Prop::Method(method) => {
                                    // { fn() {} } - inline method
                                    let key = match &method.key {
                                        PropName::Ident(id) => id.sym.to_string(),
                                        _ => continue,
                                    };
                                    self.instructions.push(OpCode::Dup);
                                    self.gen_fn_decl(None, &method.function);
                                    self.instructions.push(OpCode::SetProp(key));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Expr::SuperProp(super_prop) => {
                // Handle super.prop or super[expr]
                // Stack: [] -> [property_value]
                // For super.prop, we use GetSuperProp which looks up on the prototype chain

                match &super_prop.prop {
                    // Handle super.prop
                    swc_ecma_ast::SuperProp::Ident(id) => {
                        self.instructions
                            .push(OpCode::GetSuperProp(id.sym.to_string()));
                    }
                    // Handle super[expr]
                    swc_ecma_ast::SuperProp::Computed(_computed) => {
                        // For computed super properties, we need a different opcode
                        // For now, just push undefined
                        self.instructions.push(OpCode::Push(JsValue::Undefined));
                    }
                }
            }
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
                    // Handle #privateField
                    MemberProp::PrivateName(pn) => {
                        // Private field name in swc doesn't include the #
                        // We need to add it to our tracking
                        let field_name = format!("#{}", pn.name);

                        // If this is the first time seeing this private field in the current class context,
                        // assign it an index
                        if !self.private_field_indices.contains_key(&field_name) {
                            let new_index = self.private_field_indices.len();
                            self.private_field_indices
                                .insert(field_name.clone(), new_index);
                        }

                        if let Some(field_index) = self.private_field_indices.get(&field_name) {
                            self.instructions.push(OpCode::GetPrivateProp(*field_index));
                        } else {
                            println!("Warning: Private field '{}' not found", field_name);
                            self.instructions.push(OpCode::Push(JsValue::Undefined));
                        }
                    }
                }
            }
            Expr::This(_) => {
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
                        // Concatenate the expression result
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
                    let name = id.sym.to_string();
                    self.instructions.push(OpCode::Load(name.clone()));
                    if update_expr.prefix {
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if update_expr.op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        self.instructions.push(OpCode::Dup);
                        self.instructions.push(OpCode::Store(name));
                    } else {
                        self.instructions.push(OpCode::Dup);
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if update_expr.op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        self.instructions.push(OpCode::Store(name));
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
                            MemberProp::Ident(id) => {
                                self.instructions.push(OpCode::GetProp(id.sym.to_string()));
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                            _ => {}
                        }
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        // Stack: [obj, new_val] -> SetProp -> []
                        match &member.prop {
                            MemberProp::Ident(id) => {
                                self.instructions.push(OpCode::SetProp(id.sym.to_string()));
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::SetPropComputed);
                            }
                            _ => {}
                        }
                        // Reload the new value
                        self.gen_expr(&member.obj);
                        match &member.prop {
                            MemberProp::Ident(id) => {
                                self.instructions.push(OpCode::GetProp(id.sym.to_string()));
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                            _ => {}
                        }
                    } else {
                        // obj.prop++: get old value, save it, compute new, store back
                        // Stack: [obj, obj] -> GetProp -> [obj, old] -> Swap -> [old, obj]
                        // -> Dup -> [old, obj, obj] -> GetProp -> [old, obj, old2]
                        // -> +1 -> [old, obj, new] -> SetProp -> [old]
                        self.gen_expr(&member.obj);
                        self.instructions.push(OpCode::Dup);
                        match &member.prop {
                            MemberProp::Ident(id) => {
                                self.instructions.push(OpCode::GetProp(id.sym.to_string()));
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                            _ => {}
                        }
                        // Stack: [obj, old_val] -> Swap -> [old_val, obj]
                        self.instructions.push(OpCode::Swap);
                        self.instructions.push(OpCode::Dup);
                        match &member.prop {
                            MemberProp::Ident(id) => {
                                self.instructions.push(OpCode::GetProp(id.sym.to_string()));
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                            _ => {}
                        }
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        // Stack: [old_val, obj, new_val] -> SetProp -> [old_val]
                        match &member.prop {
                            MemberProp::Ident(id) => {
                                self.instructions.push(OpCode::SetProp(id.sym.to_string()));
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::SetPropComputed);
                            }
                            _ => {}
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
            _ => {}
        }
    }

    fn gen_class(&mut self, class: &Class, name: Option<&str>) {
        // Check if this class has a superclass
        let has_super = class.super_class.is_some();

        // Clear previous private field/method indices
        self.private_field_indices.clear();
        self.private_method_indices.clear();

        // Handle class decorators
        // Compile class decorators: @decorator class Foo {}
        // Each decorator is applied to the class after it's created
        let class_decorators: Vec<&Decorator> = class.decorators.iter().collect();

        // For now, we just store decorator count - actual application happens after class is created
        let _decorator_count = class_decorators.len();

        // For now, we don't parse private fields from class body in this swc version
        // Private fields would be handled as regular fields with special naming

        // Collect constructor params and body
        let mut constructor_params: Vec<String> = Vec::new();
        let mut constructor_body: Option<&BlockStmt> = None;

        // Collect private field declarations for initialization
        let mut private_field_decls: Vec<(String, &Expr)> = Vec::new();

        // Collect class property declarations
        let mut class_prop_decls: Vec<(String, &Expr)> = Vec::new();

        for member in &class.body {
            if let ClassMember::Constructor(ctor) = member {
                for param in &ctor.params {
                    match param {
                        ParamOrTsParamProp::Param(p) => {
                            if let Pat::Ident(id) = &p.pat {
                                constructor_params.push(id.id.sym.to_string());
                            }
                        }
                        ParamOrTsParamProp::TsParamProp(ts_prop) => {
                            if let TsParamPropParam::Ident(id) = &ts_prop.param {
                                constructor_params.push(id.id.sym.to_string());
                            }
                        }
                    }
                }
                constructor_body = ctor.body.as_ref();
            }

            // Collect private field declarations
            if let ClassMember::PrivateProp(prop) = member {
                let field_name = format!("#{}", prop.key.name);
                if !self.private_field_indices.contains_key(&field_name) {
                    let new_index = self.private_field_indices.len();
                    self.private_field_indices
                        .insert(field_name.clone(), new_index);
                }
                if let Some(value) = &prop.value {
                    private_field_decls.push((field_name, value.as_ref()));
                }
            }

            // Collect public class property declarations
            if let ClassMember::ClassProp(prop) = member {
                let prop_name = match &prop.key {
                    PropName::Ident(id) => id.sym.to_string(),
                    PropName::Str(s) => s.value.to_string_lossy().into_owned(),
                    PropName::Num(num) => num.value.to_string(),
                    _ => continue,
                };
                if let Some(value) = &prop.value {
                    class_prop_decls.push((prop_name, value.as_ref()));
                }
            }
        }

        // Store private field indices for use in constructor
        let _private_field_count = 0; // Will be set dynamically as we encounter private fields

        // Create constructor function
        let constructor_start = self.instructions.len() + 2;
        self.instructions.push(OpCode::Push(JsValue::Function {
            address: constructor_start,
            env: None,
        }));

        // Jump over constructor body
        let jump_idx = self.instructions.len();
        self.instructions.push(OpCode::Jump(0));

        // Constructor body
        let saved_in_function = self.in_function;
        self.in_function = true;

        for param in constructor_params.iter().rev() {
            self.instructions.push(OpCode::Let(param.clone()));
        }

        // Set up private field storage for this instance
        // Create storage array for private fields (one entry per field)
        // Simplified approach: create storage, then for each field, dup, push index, create object, swap, store
        self.instructions.push(OpCode::NewArray(16));
        // Stack: [storage]
        for i in 0..16 {
            // Dup storage to keep it on stack
            self.instructions.push(OpCode::Dup);
            // Stack: [storage, storage]
            // Push index
            self.instructions
                .push(OpCode::Push(JsValue::Number(i as f64)));
            // Stack: [storage, storage, index]
            // Create object (this will be the "WeakMap" for this field)
            self.instructions.push(OpCode::NewObject);
            // Stack: [storage, storage, index, field_map]
            // Swap to get [storage, storage, field_map, index]
            self.instructions.push(OpCode::Swap);
            // Stack: [storage, storage, field_map, index]
            // Store: pops index, value, array - stores value at index in array
            self.instructions.push(OpCode::StoreElement);
            // Stack: [storage]
        }

        // Store the private storage array in this.__private_storage__
        self.instructions.push(OpCode::LoadThis);
        // Stack: [this]
        self.instructions.push(OpCode::Swap);
        // Stack: [storage_array, this]
        self.instructions
            .push(OpCode::SetProp("__private_storage__".to_string()));
        // Stack: []

        // Store the private storage array in a temp for later use
        self.instructions
            .push(OpCode::Let("__private_storage__".to_string()));
        // Stack: []

        // Initialize private field declarations
        for (field_name, value_expr) in &private_field_decls {
            // Generate the value
            self.gen_expr(value_expr);
            // Stack: [value]

            // Get the field index
            if let Some(field_index) = self.private_field_indices.get(field_name) {
                // Load this
                self.instructions.push(OpCode::LoadThis);
                // Stack: [value, this]
                // Swap to get [this, value]
                self.instructions.push(OpCode::Swap);
                // Stack: [this, value]
                self.instructions.push(OpCode::SetPrivateProp(*field_index));
                // Stack: []
            }
        }

        // Initialize class property declarations
        for (prop_name, value_expr) in &class_prop_decls {
            // Generate the value
            self.gen_expr(value_expr);
            // Stack: [value]

            // Load this
            self.instructions.push(OpCode::LoadThis);
            // Stack: [value, this]
            // Swap to get [this, value]
            self.instructions.push(OpCode::Swap);
            // Stack: [this, value]
            // Set the property
            self.instructions.push(OpCode::SetProp(prop_name.clone()));
            // Stack: []
        }

        if let Some(body) = constructor_body {
            for stmt in &body.stmts {
                self.gen_stmt(stmt);
            }
        }

        self.instructions.push(OpCode::LoadThis);
        self.instructions.push(OpCode::Return);
        self.in_function = saved_in_function;

        // Backpatch jump
        let after_constructor = self.instructions.len();
        if let OpCode::Jump(ref mut addr) = self.instructions[jump_idx] {
            *addr = after_constructor;
        }

        // Stack: [constructor]

        // Save constructor to temp
        self.instructions.push(OpCode::Let("__ctor__".to_string()));
        // Stack: []

        // If there's a superclass, compile it and get its prototype
        if has_super {
            // Compile the superclass expression
            self.gen_expr(class.super_class.as_ref().unwrap());
            // Stack: [parent_wrapper]
            // Save parent wrapper to temp
            self.instructions
                .push(OpCode::Let("__parent__".to_string()));
            // Stack: []

            // Get parent's prototype: parent_wrapper.prototype
            self.instructions
                .push(OpCode::Load("__parent__".to_string()));
            // Stack: [parent_wrapper]
            self.instructions
                .push(OpCode::GetProp("prototype".to_string()));
            // Stack: [parent_prototype]
            // Save parent prototype for prototype chain
            self.instructions
                .push(OpCode::Let("__parent_proto__".to_string()));
            // Stack: []
        }

        // Create prototype object
        self.instructions.push(OpCode::NewObject);
        // Stack: [prototype]

        // Save prototype to temp
        self.instructions.push(OpCode::Let("__proto__".to_string()));
        // Stack: []

        // Set prototype.__proto__ = parent_prototype (for inheritance)
        if has_super {
            self.instructions
                .push(OpCode::Load("__proto__".to_string()));
            // Stack: [prototype]
            self.instructions
                .push(OpCode::Load("__parent_proto__".to_string()));
            // Stack: [prototype, parent_prototype]
            self.instructions
                .push(OpCode::SetProp("__proto__".to_string()));
            // Stack: []
        }

        // Create wrapper object for the class
        self.instructions.push(OpCode::NewObject);
        // Stack: [wrapper]
        // Store wrapper in temp for later retrieval (methods will consume the stack)
        self.instructions
            .push(OpCode::Let("__wrapper__".to_string()));
        // Stack: []

        // Set wrapper.name = class name (for decorator target.name)
        if let Some(class_name) = name {
            self.instructions
                .push(OpCode::Load("__wrapper__".to_string()));
            // Stack: [wrapper]
            self.instructions
                .push(OpCode::Push(JsValue::String(class_name.to_string())));
            // Stack: [wrapper, name_string]
            self.instructions.push(OpCode::SetProp("name".to_string()));
            // Stack: []
        }

        // Now set prototype.constructor = wrapper
        self.instructions
            .push(OpCode::Load("__proto__".to_string()));
        // Stack: [prototype]
        self.instructions
            .push(OpCode::Load("__wrapper__".to_string()));
        // Stack: [prototype, wrapper]
        self.instructions
            .push(OpCode::SetProp("constructor".to_string()));
        // Stack: []

        // Set wrapper.constructor = constructor
        self.instructions
            .push(OpCode::Load("__wrapper__".to_string()));
        // Stack: [wrapper]
        self.instructions.push(OpCode::Load("__ctor__".to_string()));
        // Stack: [wrapper, constructor]
        self.instructions
            .push(OpCode::SetProp("constructor".to_string()));
        // Stack: []

        // Set wrapper.prototype = prototype
        self.instructions
            .push(OpCode::Load("__wrapper__".to_string()));
        // Stack: [wrapper]
        self.instructions
            .push(OpCode::Load("__proto__".to_string()));
        // Stack: [wrapper, prototype]
        self.instructions
            .push(OpCode::SetProp("prototype".to_string()));
        // Stack: []

        // If there's a superclass, also store it in the wrapper for super() calls
        if has_super {
            self.instructions
                .push(OpCode::Load("__wrapper__".to_string()));
            // Stack: [wrapper]
            self.instructions
                .push(OpCode::Load("__parent__".to_string()));
            // Stack: [wrapper, parent]
            self.instructions
                .push(OpCode::SetProp("__super__".to_string()));
            // Stack: []
        }

        // Add methods to prototype
        for member in &class.body {
            if let ClassMember::Method(method) = member {
                // Determine the property name based on method kind
                let (prop_name, is_getter, is_setter) = match &method.key {
                    PropName::Ident(id) => {
                        let name = id.sym.to_string();
                        match method.kind {
                            MethodKind::Getter => (format!("getter:{}", name), true, false),
                            MethodKind::Setter => (format!("setter:{}", name), false, true),
                            MethodKind::Method => (name, false, false),
                        }
                    }
                    PropName::Str(s) => {
                        let name = s.value.to_string_lossy().into_owned();
                        match method.kind {
                            MethodKind::Getter => (format!("getter:{}", name), true, false),
                            MethodKind::Setter => (format!("setter:{}", name), false, true),
                            MethodKind::Method => (name, false, false),
                        }
                    }
                    PropName::Num(num) => {
                        let name = num.value.to_string();
                        match method.kind {
                            MethodKind::Getter => (format!("getter:{}", name), true, false),
                            MethodKind::Setter => (format!("setter:{}", name), false, true),
                            MethodKind::Method => (name, false, false),
                        }
                    }
                    _ => continue, // Skip computed names for now
                };

                let unique_name = format!("__method_{}", prop_name.replace(":", "_"));

                // Get parameters - getters have no params, setters have one param (value)
                let params: Vec<String> = if is_getter {
                    Vec::new() // Getters take no parameters
                } else if is_setter {
                    // Setters take one parameter (the value)
                    method
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
                        .collect()
                } else {
                    method
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
                        .collect()
                };

                // Push function placeholder
                let method_start = self.instructions.len() + 2;
                self.instructions.push(OpCode::Push(JsValue::Function {
                    address: method_start,
                    env: None,
                }));

                // Jump over method body
                let method_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::Jump(0));

                // Compile method body
                let saved_in_function = self.in_function;
                self.in_function = true;

                for param in params.iter().rev() {
                    self.instructions.push(OpCode::Let(param.clone()));
                }

                if let Some(body) = &method.function.body {
                    for stmt in &body.stmts {
                        self.gen_stmt(stmt);
                    }
                }

                self.instructions.push(OpCode::LoadThis);
                self.instructions.push(OpCode::Return);
                self.in_function = saved_in_function;

                // Backpatch method jump
                let after_method = self.instructions.len();
                if let OpCode::Jump(ref mut addr) = self.instructions[method_jump_idx] {
                    *addr = after_method;
                }

                // Store method in a temp
                self.instructions.push(OpCode::Let(unique_name.clone()));

                // Set prototype.method = method_function (or getter/setter)
                self.instructions
                    .push(OpCode::Load("__proto__".to_string()));
                // Stack: [prototype]
                self.instructions.push(OpCode::Load(unique_name.clone()));
                // Stack: [prototype, method]
                self.instructions.push(OpCode::SetProp(prop_name));
                // Stack: []
            }
        }

        // Restore wrapper to stack for return
        self.instructions
            .push(OpCode::Load("__wrapper__".to_string()));
        // Stack: [wrapper]

        // Apply class decorators (in reverse order, as per spec)
        // @decorator class Foo {} compiles to:
        // [class definition] -> [decorator] -> ApplyDecorator -> [decorated_class]
        for decorator in class_decorators.iter().rev() {
            // Compile the decorator expression
            self.gen_expr(&decorator.expr);
            // Stack should be: [wrapper, decorator]
            self.instructions.push(OpCode::ApplyDecorator);
            // Stack: [decorated_wrapper]
        }
    }
}
