//! Type Checker
//!
//! Performs type checking on the AST with:
//! - Constraint generation
//! - Type inference via unification
//! - Flow-sensitive analysis
//! - Monomorphization of generics

use std::collections::BTreeMap;
use swc_ecma_ast::*;

use super::convert::TypeConverter;
use super::error::{Span, TypeError, TypeErrors};
use super::inference::{InferenceEngine, TypeNarrower};
use super::registry::TypeRegistry;
use super::{FunctionType, ObjectType, Ownership, Type, TypeVarId, VarType, fresh_type_var_id};

// ============================================================================
// Type Checker
// ============================================================================

/// The main type checker.
pub struct TypeChecker<'a> {
    /// Type registry for named types.
    registry: &'a mut TypeRegistry,
    /// Inference engine for constraint solving.
    inference: InferenceEngine<'a>,
    /// Type narrowing for control flow.
    narrower: TypeNarrower,
    /// Accumulated errors.
    errors: TypeErrors,
    /// Current function return type (for checking returns).
    current_return_type: Option<Type>,
    /// Whether we're in strict mode (require all annotations).
    strict: bool,
}

impl<'a> TypeChecker<'a> {
    pub fn new(registry: &'a mut TypeRegistry) -> Self {
        // We need to create the inference engine with a reference to registry
        // that outlives the borrow. For now, use a static empty registry for inference.
        // In a real implementation, we'd restructure this to avoid the borrow conflict.
        let inference = InferenceEngine::new(unsafe {
            // SAFETY: We're creating a temporary reference for the inference engine.
            // The registry reference is valid for 'a.
            &*(registry as *const TypeRegistry)
        });

        Self {
            registry,
            inference,
            narrower: TypeNarrower::new(),
            errors: TypeErrors::new(),
            current_return_type: None,
            strict: false,
        }
    }

    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }

    /// Check a module (collection of statements).
    pub fn check_module(&mut self, module: &Module) -> Result<(), TypeErrors> {
        // First pass: collect type definitions
        for item in &module.body {
            if let ModuleItem::Stmt(stmt) = item {
                self.collect_type_def(stmt);
            }
        }

        // Second pass: collect function signatures
        for item in &module.body {
            if let ModuleItem::Stmt(stmt) = item {
                self.collect_fn_signature(stmt);
            }
        }

        // Third pass: type check bodies
        for item in &module.body {
            if let ModuleItem::Stmt(stmt) = item {
                self.check_stmt(stmt);
            }
        }

        // Solve constraints
        if let Err(mut errs) = self.inference.solve() {
            self.errors.errors.append(&mut errs.errors);
        }

        if self.errors.has_errors() {
            Err(std::mem::take(&mut self.errors))
        } else {
            Ok(())
        }
    }

    /// Check a script (for REPL-style input).
    pub fn check_script(&mut self, script: &Script) -> Result<(), TypeErrors> {
        for stmt in &script.body {
            self.check_stmt(stmt);
        }

        if let Err(mut errs) = self.inference.solve() {
            self.errors.errors.append(&mut errs.errors);
        }

        if self.errors.has_errors() {
            Err(std::mem::take(&mut self.errors))
        } else {
            Ok(())
        }
    }

    // ========================================================================
    // Collection Passes
    // ========================================================================

    fn collect_type_def(&mut self, stmt: &Stmt) {
        // Look for type declarations
        match stmt {
            Stmt::Decl(Decl::TsTypeAlias(alias)) => {
                let name = alias.id.sym.to_string();
                let id = super::fresh_type_id();

                // Convert type params
                let type_params: Vec<TypeVarId> = alias
                    .type_params
                    .as_ref()
                    .map(|p| p.params.iter().map(|_| fresh_type_var_id()).collect())
                    .unwrap_or_default();

                let param_names: Vec<String> = alias
                    .type_params
                    .as_ref()
                    .map(|p| {
                        p.params
                            .iter()
                            .map(|param| param.name.sym.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                // Convert the aliased type
                let converter =
                    TypeConverter::new(self.registry).with_type_params(&type_params, &param_names);

                if let Ok(ty) = converter.convert(&alias.type_ann) {
                    let type_alias = super::TypeAlias {
                        id,
                        name: name.clone(),
                        ty,
                        type_params,
                    };
                    self.registry.register_alias(type_alias);
                }
            }
            Stmt::Decl(Decl::TsInterface(iface)) => {
                // Convert interface to struct type
                let name = iface.id.sym.to_string();
                let id = super::fresh_type_id();

                let type_params: Vec<TypeVarId> = iface
                    .type_params
                    .as_ref()
                    .map(|p| p.params.iter().map(|_| fresh_type_var_id()).collect())
                    .unwrap_or_default();

                let mut def = super::StructDef::new(id, name).with_type_params(type_params.clone());

                let param_names: Vec<String> = iface
                    .type_params
                    .as_ref()
                    .map(|p| {
                        p.params
                            .iter()
                            .map(|param| param.name.sym.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                let converter =
                    TypeConverter::new(self.registry).with_type_params(&type_params, &param_names);

                for member in &iface.body.body {
                    if let TsTypeElement::TsPropertySignature(prop) = member
                        && let Expr::Ident(ident) = &*prop.key
                    {
                        let field_name = ident.sym.to_string();
                        let field_ty = prop
                            .type_ann
                            .as_ref()
                            .and_then(|ann| converter.convert(&ann.type_ann).ok())
                            .unwrap_or(Type::Any);
                        def = def.with_field(field_name, field_ty);
                    }
                }

                self.registry.register_struct(def);
            }
            _ => {}
        }
    }

    fn collect_fn_signature(&mut self, stmt: &Stmt) {
        if let Stmt::Decl(Decl::Fn(fn_decl)) = stmt {
            let name = fn_decl.ident.sym.to_string();

            // Convert parameter types
            let params: Vec<(String, Type)> = fn_decl
                .function
                .params
                .iter()
                .map(|p| {
                    let param_name = match &p.pat {
                        Pat::Ident(ident) => ident.id.sym.to_string(),
                        _ => "_".to_string(),
                    };
                    let param_ty = match &p.pat {
                        Pat::Ident(ident) => ident
                            .type_ann
                            .as_ref()
                            .and_then(|ann| {
                                TypeConverter::new(self.registry)
                                    .convert(&ann.type_ann)
                                    .ok()
                            })
                            .unwrap_or(if self.strict { Type::Error } else { Type::Any }),
                        _ => Type::Any,
                    };
                    (param_name, param_ty)
                })
                .collect();

            // Convert return type
            let return_ty = fn_decl
                .function
                .return_type
                .as_ref()
                .and_then(|ann| {
                    TypeConverter::new(self.registry)
                        .convert(&ann.type_ann)
                        .ok()
                })
                .unwrap_or(Type::Void);

            let func_type = FunctionType::new(params.clone(), return_ty.clone());

            // Register in context
            self.inference
                .context_mut()
                .define(name, VarType::new(Type::Function(Box::new(func_type))));
        }
    }

    // ========================================================================
    // Statement Checking
    // ========================================================================

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Decl(decl) => self.check_decl(decl),
            Stmt::Expr(expr_stmt) => {
                self.check_expr(&expr_stmt.expr);
            }
            Stmt::Return(ret) => self.check_return(ret),
            Stmt::If(if_stmt) => self.check_if(if_stmt),
            Stmt::While(while_stmt) => self.check_while(while_stmt),
            Stmt::For(for_stmt) => self.check_for(for_stmt),
            Stmt::Block(block) => {
                self.inference.enter_scope();
                for stmt in &block.stmts {
                    self.check_stmt(stmt);
                }
                self.inference.exit_scope();
            }
            Stmt::Throw(throw) => {
                self.check_expr(&throw.arg);
            }
            Stmt::Try(try_stmt) => self.check_try(try_stmt),
            _ => {}
        }
    }

    fn check_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Var(var_decl) => {
                for decl in &var_decl.decls {
                    self.check_var_declarator(decl, var_decl.kind);
                }
            }
            Decl::Fn(fn_decl) => {
                self.check_fn_decl(fn_decl);
            }
            _ => {}
        }
    }

    fn check_var_declarator(&mut self, decl: &VarDeclarator, kind: VarDeclKind) {
        let name = match &decl.name {
            Pat::Ident(ident) => ident.id.sym.to_string(),
            _ => return,
        };

        // Get declared type annotation
        let declared_ty = match &decl.name {
            Pat::Ident(ident) => ident.type_ann.as_ref().and_then(|ann| {
                TypeConverter::new(self.registry)
                    .convert(&ann.type_ann)
                    .ok()
            }),
            _ => None,
        };

        // Infer type from initializer
        let init_ty = decl.init.as_ref().map(|init| self.check_expr(init));

        // Determine final type
        let ty = match (declared_ty, init_ty) {
            (Some(decl_ty), Some(init)) => {
                // Both: check compatibility
                self.inference
                    .constrain_equal(init.clone(), decl_ty.clone(), Span::default());
                decl_ty
            }
            (Some(decl), None) => decl,
            (None, Some(init)) => init,
            (None, None) => {
                if self.strict {
                    self.errors.push(TypeError::CannotInfer {
                        span: Span::default(),
                    });
                    Type::Error
                } else {
                    self.inference.fresh_var()
                }
            }
        };

        // Register variable
        let mutable = kind != VarDeclKind::Const;
        let mut var_type = VarType::new(ty);
        var_type.mutable = mutable;
        self.inference.context_mut().define(name, var_type);
    }

    fn check_fn_decl(&mut self, fn_decl: &FnDecl) {
        // Get the registered function type
        let name = fn_decl.ident.sym.to_string();
        let func_ty = self.inference.context().lookup(&name).map(|v| v.ty.clone());

        let return_ty = match &func_ty {
            Some(Type::Function(f)) => f.return_ty.clone(),
            _ => Type::Void,
        };

        // Enter function scope
        self.inference.enter_scope();
        let old_return_type = self.current_return_type.take();
        self.current_return_type = Some(return_ty.clone());

        // Bind parameters
        if let Some(Type::Function(f)) = &func_ty {
            for (param_name, param_ty) in &f.params {
                self.inference
                    .context_mut()
                    .define(param_name.clone(), VarType::new(param_ty.clone()));
            }
        }

        // Check body
        if let Some(body) = &fn_decl.function.body {
            for stmt in &body.stmts {
                self.check_stmt(stmt);
            }
        }

        // Restore state
        self.current_return_type = old_return_type;
        self.inference.exit_scope();
    }

    fn check_return(&mut self, ret: &ReturnStmt) {
        let return_ty = ret
            .arg
            .as_ref()
            .map(|arg| self.check_expr(arg))
            .unwrap_or(Type::Void);

        if let Some(expected) = &self.current_return_type {
            self.inference
                .constrain_equal(return_ty, expected.clone(), Span::default());
        }
    }

    fn check_if(&mut self, if_stmt: &IfStmt) {
        let cond_ty = self.check_expr(&if_stmt.test);

        // Condition should be boolean (or coercible)
        self.inference
            .constrain_equal(cond_ty, Type::Boolean, Span::default());

        // Check consequent
        self.narrower.enter_branch();
        self.check_stmt(&if_stmt.cons);
        let then_narrowings = self.narrower.exit_branch();

        // Check alternate
        if let Some(alt) = &if_stmt.alt {
            self.narrower.enter_branch();
            self.check_stmt(alt);
            let else_narrowings = self.narrower.exit_branch();
            self.narrower
                .merge_branches(vec![then_narrowings, else_narrowings]);
        }
    }

    fn check_while(&mut self, while_stmt: &WhileStmt) {
        let cond_ty = self.check_expr(&while_stmt.test);
        self.inference
            .constrain_equal(cond_ty, Type::Boolean, Span::default());
        self.check_stmt(&while_stmt.body);
    }

    fn check_for(&mut self, for_stmt: &ForStmt) {
        self.inference.enter_scope();

        if let Some(init) = &for_stmt.init {
            match init {
                VarDeclOrExpr::VarDecl(var_decl) => {
                    for decl in &var_decl.decls {
                        self.check_var_declarator(decl, var_decl.kind);
                    }
                }
                VarDeclOrExpr::Expr(expr) => {
                    self.check_expr(expr);
                }
            }
        }

        if let Some(test) = &for_stmt.test {
            let cond_ty = self.check_expr(test);
            self.inference
                .constrain_equal(cond_ty, Type::Boolean, Span::default());
        }

        if let Some(update) = &for_stmt.update {
            self.check_expr(update);
        }

        self.check_stmt(&for_stmt.body);
        self.inference.exit_scope();
    }

    fn check_try(&mut self, try_stmt: &TryStmt) {
        self.check_stmt(&Stmt::Block(try_stmt.block.clone()));

        if let Some(handler) = &try_stmt.handler {
            self.inference.enter_scope();
            // Bind catch parameter
            if let Some(param) = &handler.param
                && let Pat::Ident(ident) = param
            {
                self.inference.context_mut().define(
                    ident.id.sym.to_string(),
                    VarType::new(Type::Any), // catch parameter is any
                );
            }
            self.check_stmt(&Stmt::Block(handler.body.clone()));
            self.inference.exit_scope();
        }

        if let Some(finalizer) = &try_stmt.finalizer {
            self.check_stmt(&Stmt::Block(finalizer.clone()));
        }
    }

    // ========================================================================
    // Expression Checking
    // ========================================================================

    fn check_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            Expr::Lit(lit) => self.check_lit(lit),
            Expr::Ident(ident) => self.check_ident(ident),
            Expr::Bin(bin) => self.check_binary(bin),
            Expr::Unary(unary) => self.check_unary(unary),
            Expr::Call(call) => self.check_call(call),
            Expr::Member(member) => self.check_member(member),
            Expr::Array(arr) => self.check_array(arr),
            Expr::Object(obj) => self.check_object(obj),
            Expr::Arrow(arrow) => self.check_arrow(arrow),
            Expr::Fn(fn_expr) => self.check_fn_expr(fn_expr),
            Expr::Paren(paren) => self.check_expr(&paren.expr),
            Expr::Assign(assign) => self.check_assign(assign),
            Expr::Update(update) => self.check_update(update),
            Expr::Cond(cond) => self.check_cond(cond),
            Expr::New(new) => self.check_new(new),
            Expr::Tpl(tpl) => self.check_template(tpl),
            Expr::This(_) => Type::Any, // TODO: proper this typing
            _ => Type::Any,
        }
    }

    fn check_lit(&mut self, lit: &Lit) -> Type {
        match lit {
            Lit::Num(_) => Type::Number,
            Lit::Str(_) => Type::String,
            Lit::Bool(_) => Type::Boolean,
            Lit::Null(_) => Type::Void,
            Lit::BigInt(_) => Type::Number, // Treat BigInt as number for now
            Lit::Regex(_) => Type::Any,     // RegExp type
            Lit::JSXText(_) => Type::String,
        }
    }

    fn check_ident(&mut self, ident: &Ident) -> Type {
        let name = ident.sym.to_string();

        // Check for narrowed type first
        if let Some(narrowed) = self.narrower.get_narrowed(&name) {
            return narrowed.clone();
        }

        // Look up in context
        if let Some(var_type) = self.inference.context().lookup(&name) {
            // Check ownership
            if var_type.ownership == Ownership::Moved {
                self.errors.push(TypeError::UseAfterMove {
                    var: name.clone(),
                    moved_at: Span::default(),
                    used_at: Span::default(),
                });
            }
            return var_type.ty.clone();
        }

        // Unknown variable
        if self.strict {
            self.errors.push(TypeError::UndefinedVariable {
                name,
                span: Span::default(),
            });
            Type::Error
        } else {
            Type::Any
        }
    }

    fn check_binary(&mut self, bin: &BinExpr) -> Type {
        let left_ty = self.check_expr(&bin.left);
        let right_ty = self.check_expr(&bin.right);

        match bin.op {
            // Arithmetic: both must be numbers, result is number
            BinaryOp::Add => {
                // Special case: string + string = string
                if matches!((&left_ty, &right_ty), (Type::String, _) | (_, Type::String)) {
                    return Type::String;
                }
                self.inference
                    .constrain_equal(left_ty, Type::Number, Span::default());
                self.inference
                    .constrain_equal(right_ty, Type::Number, Span::default());
                Type::Number
            }
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Exp => {
                self.inference
                    .constrain_equal(left_ty, Type::Number, Span::default());
                self.inference
                    .constrain_equal(right_ty, Type::Number, Span::default());
                Type::Number
            }

            // Bitwise: both must be numbers
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::LShift
            | BinaryOp::RShift
            | BinaryOp::ZeroFillRShift => {
                self.inference
                    .constrain_equal(left_ty, Type::Number, Span::default());
                self.inference
                    .constrain_equal(right_ty, Type::Number, Span::default());
                Type::Number
            }

            // Comparison: same type, result is boolean
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                self.inference
                    .constrain_equal(left_ty.clone(), right_ty, Span::default());
                Type::Boolean
            }

            // Equality: result is boolean
            BinaryOp::EqEq | BinaryOp::NotEq | BinaryOp::EqEqEq | BinaryOp::NotEqEq => {
                Type::Boolean
            }

            // Logical: both boolean, result is boolean
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr => {
                // Actually, these can short-circuit with any type
                // For now, return the left type (could be union)
                left_ty
            }

            BinaryOp::NullishCoalescing => {
                // Returns left if not null/undefined, otherwise right
                left_ty
            }

            BinaryOp::In | BinaryOp::InstanceOf => Type::Boolean,
        }
    }

    fn check_unary(&mut self, unary: &UnaryExpr) -> Type {
        let arg_ty = self.check_expr(&unary.arg);

        match unary.op {
            UnaryOp::Minus | UnaryOp::Plus => {
                self.inference
                    .constrain_equal(arg_ty, Type::Number, Span::default());
                Type::Number
            }
            UnaryOp::Bang => Type::Boolean,
            UnaryOp::Tilde => {
                self.inference
                    .constrain_equal(arg_ty, Type::Number, Span::default());
                Type::Number
            }
            UnaryOp::TypeOf => Type::String,
            UnaryOp::Void => Type::Void,
            UnaryOp::Delete => Type::Boolean,
        }
    }

    fn check_call(&mut self, call: &CallExpr) -> Type {
        let callee_ty = match &call.callee {
            Callee::Expr(expr) => self.check_expr(expr),
            _ => Type::Any,
        };

        let arg_types: Vec<Type> = call
            .args
            .iter()
            .map(|arg| self.check_expr(&arg.expr))
            .collect();

        // Create return type variable
        let return_ty = self.inference.fresh_var();

        // Add callable constraint
        self.inference
            .constrain_callable(callee_ty, arg_types, return_ty.clone(), Span::default());

        return_ty
    }

    fn check_member(&mut self, member: &MemberExpr) -> Type {
        let obj_ty = self.check_expr(&member.obj);

        let field_name = match &member.prop {
            MemberProp::Ident(ident) => ident.sym.to_string(),
            MemberProp::Computed(comp) => {
                // For computed properties, we need to check the index type
                let index_ty = self.check_expr(&comp.expr);

                // If it's an array, return element type
                if let Type::Array(elem) = &obj_ty {
                    self.inference
                        .constrain_equal(index_ty, Type::Number, Span::default());
                    return (**elem).clone();
                }

                return Type::Any;
            }
            MemberProp::PrivateName(p) => p.name.to_string(),
        };

        // Create field type variable
        let field_ty = self.inference.fresh_var();

        // Add field constraint
        self.inference
            .constrain_has_field(obj_ty, field_name, field_ty.clone(), Span::default());

        field_ty
    }

    fn check_array(&mut self, arr: &ArrayLit) -> Type {
        if arr.elems.is_empty() {
            return Type::Array(Box::new(self.inference.fresh_var()));
        }

        // Infer element type from elements
        let elem_ty = self.inference.fresh_var();

        for elem in arr.elems.iter().flatten() {
            let ty = self.check_expr(&elem.expr);
            self.inference
                .constrain_equal(ty, elem_ty.clone(), Span::default());
        }

        Type::Array(Box::new(elem_ty))
    }

    fn check_object(&mut self, obj: &ObjectLit) -> Type {
        let mut fields = BTreeMap::new();

        for prop in &obj.props {
            match prop {
                PropOrSpread::Prop(prop) => {
                    match &**prop {
                        Prop::KeyValue(kv) => {
                            let name = match &kv.key {
                                PropName::Ident(ident) => Some(ident.sym.to_string()),
                                PropName::Str(s) => {
                                    Some(String::from_utf8_lossy(s.value.as_bytes()).into_owned())
                                }
                                _ => None,
                            };
                            if let Some(name) = name {
                                let ty = self.check_expr(&kv.value);
                                fields.insert(name, ty);
                            }
                        }
                        Prop::Shorthand(ident) => {
                            let name = ident.sym.to_string();
                            let ty = self.check_ident(ident);
                            fields.insert(name, ty);
                        }
                        Prop::Method(method) => {
                            let name = match &method.key {
                                PropName::Ident(ident) => Some(ident.sym.to_string()),
                                _ => None,
                            };
                            if let Some(name) = name {
                                // Infer method type
                                let params: Vec<(String, Type)> = method
                                    .function
                                    .params
                                    .iter()
                                    .map(|p| {
                                        let param_name = match &p.pat {
                                            Pat::Ident(ident) => ident.id.sym.to_string(),
                                            _ => "_".to_string(),
                                        };
                                        (param_name, Type::Any)
                                    })
                                    .collect();

                                let return_ty = Type::Any;
                                fields.insert(
                                    name,
                                    Type::Function(Box::new(FunctionType::new(params, return_ty))),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                PropOrSpread::Spread(spread) => {
                    // Spread: merge fields from spread object
                    let spread_ty = self.check_expr(&spread.expr);
                    // For now, just use any for spread
                    let _ = spread_ty;
                }
            }
        }

        Type::Object(ObjectType {
            fields,
            exact: false,
        })
    }

    fn check_arrow(&mut self, arrow: &ArrowExpr) -> Type {
        // Convert parameter types
        let params: Vec<(String, Type)> = arrow
            .params
            .iter()
            .map(|p| {
                let name = match p {
                    Pat::Ident(ident) => ident.id.sym.to_string(),
                    _ => "_".to_string(),
                };
                let ty = match p {
                    Pat::Ident(ident) => ident
                        .type_ann
                        .as_ref()
                        .and_then(|ann| {
                            TypeConverter::new(self.registry)
                                .convert(&ann.type_ann)
                                .ok()
                        })
                        .unwrap_or_else(|| self.inference.fresh_var()),
                    _ => self.inference.fresh_var(),
                };
                (name, ty)
            })
            .collect();

        // Determine return type
        let return_ty = arrow
            .return_type
            .as_ref()
            .and_then(|ann| {
                TypeConverter::new(self.registry)
                    .convert(&ann.type_ann)
                    .ok()
            })
            .unwrap_or_else(|| self.inference.fresh_var());

        // Check body
        self.inference.enter_scope();
        let old_return_type = self.current_return_type.take();
        self.current_return_type = Some(return_ty.clone());

        for (name, ty) in &params {
            self.inference
                .context_mut()
                .define(name.clone(), VarType::new(ty.clone()));
        }

        let body_ty = match &*arrow.body {
            BlockStmtOrExpr::Expr(expr) => {
                let ty = self.check_expr(expr);
                // Expression body: type is the return type
                self.inference
                    .constrain_equal(ty, return_ty.clone(), Span::default());
                return_ty.clone()
            }
            BlockStmtOrExpr::BlockStmt(block) => {
                for stmt in &block.stmts {
                    self.check_stmt(stmt);
                }
                return_ty.clone()
            }
        };

        self.current_return_type = old_return_type;
        self.inference.exit_scope();

        Type::Function(Box::new(FunctionType::new(params, body_ty)))
    }

    fn check_fn_expr(&mut self, fn_expr: &FnExpr) -> Type {
        let params: Vec<(String, Type)> = fn_expr
            .function
            .params
            .iter()
            .map(|p| {
                let name = match &p.pat {
                    Pat::Ident(ident) => ident.id.sym.to_string(),
                    _ => "_".to_string(),
                };
                let ty = match &p.pat {
                    Pat::Ident(ident) => ident
                        .type_ann
                        .as_ref()
                        .and_then(|ann| {
                            TypeConverter::new(self.registry)
                                .convert(&ann.type_ann)
                                .ok()
                        })
                        .unwrap_or(Type::Any),
                    _ => Type::Any,
                };
                (name, ty)
            })
            .collect();

        let return_ty = fn_expr
            .function
            .return_type
            .as_ref()
            .and_then(|ann| {
                TypeConverter::new(self.registry)
                    .convert(&ann.type_ann)
                    .ok()
            })
            .unwrap_or(Type::Void);

        Type::Function(Box::new(FunctionType::new(params, return_ty)))
    }

    fn check_assign(&mut self, assign: &AssignExpr) -> Type {
        let right_ty = self.check_expr(&assign.right);

        // Get the left side type
        let left_ty = match &assign.left {
            AssignTarget::Simple(simple) => match simple {
                SimpleAssignTarget::Ident(ident) => {
                    let name = ident.id.sym.to_string();

                    // Check mutability
                    if let Some(var) = self.inference.context().lookup(&name) {
                        if !var.mutable {
                            self.errors.push(TypeError::ImmutableAssignment {
                                var: name.clone(),
                                span: Span::default(),
                            });
                        }
                        var.ty.clone()
                    } else {
                        Type::Any
                    }
                }
                SimpleAssignTarget::Member(member) => self.check_member(member),
                _ => Type::Any,
            },
            AssignTarget::Pat(_) => Type::Any,
        };

        // Type must match
        self.inference
            .constrain_equal(right_ty.clone(), left_ty, Span::default());

        right_ty
    }

    fn check_update(&mut self, update: &UpdateExpr) -> Type {
        let arg_ty = self.check_expr(&update.arg);
        self.inference
            .constrain_equal(arg_ty, Type::Number, Span::default());
        Type::Number
    }

    fn check_cond(&mut self, cond: &CondExpr) -> Type {
        let test_ty = self.check_expr(&cond.test);
        self.inference
            .constrain_equal(test_ty, Type::Boolean, Span::default());

        let cons_ty = self.check_expr(&cond.cons);
        let alt_ty = self.check_expr(&cond.alt);

        // Both branches must have same type
        self.inference
            .constrain_equal(cons_ty.clone(), alt_ty, Span::default());

        cons_ty
    }

    fn check_new(&mut self, new: &NewExpr) -> Type {
        let _callee_ty = self.check_expr(&new.callee);

        // For now, return any for new expressions
        // TODO: proper constructor typing
        Type::Any
    }

    fn check_template(&mut self, _tpl: &Tpl) -> Type {
        // Template literals always produce strings
        Type::String
    }

    // ========================================================================
    // Helpers
    // ========================================================================

    #[allow(dead_code)]
    fn span_from_expr(&self, _expr: Option<&Expr>) -> Span {
        Span::default()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_literals() {
        let mut registry = TypeRegistry::new();
        let mut checker = TypeChecker::new(&mut registry);

        // Number literal
        let num = Expr::Lit(Lit::Num(Number {
            span: Default::default(),
            value: 42.0,
            raw: None,
        }));
        assert_eq!(checker.check_expr(&num), Type::Number);

        // String literal
        let str_lit = Expr::Lit(Lit::Str(Str {
            span: Default::default(),
            value: "hello".into(),
            raw: None,
        }));
        assert_eq!(checker.check_expr(&str_lit), Type::String);

        // Boolean literal
        let bool_lit = Expr::Lit(Lit::Bool(Bool {
            span: Default::default(),
            value: true,
        }));
        assert_eq!(checker.check_expr(&bool_lit), Type::Boolean);
    }
}
