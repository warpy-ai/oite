//! Borrow Checker
//!
//! Performs ownership and borrowing analysis with type information.
//! Integrates with the type system to determine:
//! - Copy vs Move semantics based on type
//! - Borrow tracking for Ref<T> and MutRef<T>
//! - Lifetime analysis for references

use std::collections::{HashMap, HashSet};
use swc_ecma_ast::*;

use crate::types::Type;
use crate::types::error::{BorrowKind, Span, TypeError, TypeErrors};
use crate::types::registry::TypeRegistry;

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum VarKind {
    Primitive,
    Heap,
    Borrow,
    BorrowMut,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum VarState {
    Owned,
    Moved,
    CapturedByAsync,
    Borrowed,
    BorrowedMut,
}

#[derive(Clone, Debug)]
pub struct VarInfo {
    pub ty: Type,
    pub kind: VarKind,
    pub state: VarState,
    pub immut_borrows: usize,
    pub mut_borrow: bool,
    pub def_span: Span,
    pub moved_span: Option<Span>,
    pub scope_depth: usize,
}

impl VarInfo {
    pub fn from_type(ty: Type, span: Span, scope_depth: usize) -> Self {
        let kind = Self::kind_from_type(&ty);
        Self {
            ty,
            kind,
            state: VarState::Owned,
            immut_borrows: 0,
            mut_borrow: false,
            def_span: span,
            moved_span: None,
            scope_depth,
        }
    }

    pub fn is_global(&self) -> bool {
        self.scope_depth == 0
    }

    fn kind_from_type(ty: &Type) -> VarKind {
        match ty {
            Type::Number | Type::Boolean | Type::Void => VarKind::Primitive,
            Type::Ref(_) => VarKind::Borrow,
            Type::MutRef(_) => VarKind::BorrowMut,
            Type::String
            | Type::Array(_)
            | Type::Object(_)
            | Type::Function(_)
            | Type::Struct(_)
            | Type::Enum(_) => VarKind::Heap,
            Type::Any => VarKind::Heap, // Conservative: treat any as heap
            _ => VarKind::Primitive,
        }
    }

    pub fn is_copy(&self) -> bool {
        self.kind == VarKind::Primitive
    }

    pub fn is_move(&self) -> bool {
        self.kind == VarKind::Heap
    }
}

#[allow(dead_code)]
pub struct BorrowChecker {
    symbols: HashMap<String, VarInfo>,
    registry: Option<TypeRegistry>,
    errors: TypeErrors,
    scope_depth: usize,
    scope_stack: Vec<HashSet<String>>,
}

impl Default for BorrowChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl BorrowChecker {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
            registry: None,
            errors: TypeErrors::new(),
            scope_depth: 0,
            scope_stack: vec![HashSet::new()],
        }
    }

    pub fn with_registry(registry: TypeRegistry) -> Self {
        Self {
            symbols: HashMap::new(),
            registry: Some(registry),
            errors: TypeErrors::new(),
            scope_depth: 0,
            scope_stack: vec![HashSet::new()],
        }
    }

    pub fn enter_scope(&mut self) {
        self.scope_depth += 1;
        self.scope_stack.push(HashSet::new());
    }

    pub fn exit_scope(&mut self) {
        if let Some(vars) = self.scope_stack.pop() {
            for name in vars {
                self.symbols.remove(&name);
            }
        }
        self.scope_depth = self.scope_depth.saturating_sub(1);
    }

    pub fn define(&mut self, name: String, ty: Type, span: Span) {
        let info = VarInfo::from_type(ty, span, self.scope_depth);
        self.symbols.insert(name.clone(), info);
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.insert(name);
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&VarInfo> {
        self.symbols.get(name)
    }

    pub fn lookup_mut(&mut self, name: &str) -> Option<&mut VarInfo> {
        self.symbols.get_mut(name)
    }

    pub fn errors(&self) -> &TypeErrors {
        &self.errors
    }

    pub fn take_errors(&mut self) -> TypeErrors {
        std::mem::take(&mut self.errors)
    }

    pub fn has_errors(&self) -> bool {
        self.errors.has_errors()
    }

    pub fn analyze_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Decl(Decl::Var(var_decl)) => {
                for decl in &var_decl.decls {
                    self.analyze_var_decl(decl)?;
                }
            }
            Stmt::Expr(expr_stmt) => {
                self.analyze_expr(&expr_stmt.expr)?;
            }
            Stmt::Block(block) => {
                self.enter_scope();
                for s in &block.stmts {
                    self.analyze_stmt(s)?;
                }
                self.exit_scope();
            }
            Stmt::If(if_stmt) => {
                self.analyze_expr(&if_stmt.test)?;
                self.analyze_stmt(&if_stmt.cons)?;
                if let Some(alt) = &if_stmt.alt {
                    self.analyze_stmt(alt)?;
                }
            }
            Stmt::While(while_stmt) => {
                self.analyze_expr(&while_stmt.test)?;
                self.analyze_stmt(&while_stmt.body)?;
            }
            Stmt::For(for_stmt) => {
                self.enter_scope();
                if let Some(init) = &for_stmt.init {
                    match init {
                        VarDeclOrExpr::VarDecl(var_decl) => {
                            for decl in &var_decl.decls {
                                self.analyze_var_decl(decl)?;
                            }
                        }
                        VarDeclOrExpr::Expr(expr) => {
                            self.analyze_expr(expr)?;
                        }
                    }
                }
                if let Some(test) = &for_stmt.test {
                    self.analyze_expr(test)?;
                }
                if let Some(update) = &for_stmt.update {
                    self.analyze_expr(update)?;
                }
                self.analyze_stmt(&for_stmt.body)?;
                self.exit_scope();
            }
            Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    self.analyze_expr(arg)?;
                }
            }
            Stmt::Throw(throw) => {
                self.analyze_expr(&throw.arg)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn analyze_var_decl(&mut self, decl: &VarDeclarator) -> Result<(), String> {
        let name = match &decl.name {
            Pat::Ident(ident) => ident.id.sym.to_string(),
            _ => return Ok(()),
        };

        let ty = self.determine_type(decl);

        if let Some(init) = &decl.init {
            // Bare identifier = ownership transfer; member access = borrow
            if let Expr::Ident(id) = init.as_ref() {
                self.process_move(id.sym.as_ref())?;
            } else {
                self.analyze_expr(init)?;
            }
        }

        self.define(name, ty, Span::default());

        Ok(())
    }

    fn determine_type(&self, decl: &VarDeclarator) -> Type {
        if let Pat::Ident(ident) = &decl.name
            && let Some(_ann) = &ident.type_ann
        {
            // Would convert TsType to Type here if we had the converter
            // For now, infer from initializer
        }

        if let Some(init) = &decl.init {
            return self.infer_type(init);
        }

        Type::Any
    }

    fn infer_type(&self, expr: &Expr) -> Type {
        match expr {
            Expr::Lit(Lit::Num(_)) => Type::Number,
            Expr::Lit(Lit::Str(_)) => Type::String,
            Expr::Lit(Lit::Bool(_)) => Type::Boolean,
            Expr::Array(_) => Type::Array(Box::new(Type::Any)),
            Expr::Object(_) => Type::Object(crate::types::ObjectType::default()),
            Expr::Arrow(_) | Expr::Fn(_) => {
                Type::Function(Box::new(crate::types::FunctionType::new(vec![], Type::Any)))
            }
            Expr::Ident(id) => {
                let name = id.sym.to_string();
                self.symbols
                    .get(&name)
                    .map(|info| info.ty.clone())
                    .unwrap_or(Type::Any)
            }
            _ => Type::Any,
        }
    }

    fn analyze_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Ident(id) => {
                self.process_use(id.sym.as_ref())?;
            }
            Expr::Member(member) => {
                // Member access is an implicit borrow
                if let Expr::Ident(id) = member.obj.as_ref() {
                    self.process_borrow(id.sym.as_ref(), false)?;
                } else {
                    self.analyze_expr(&member.obj)?;
                }
                if let MemberProp::Computed(c) = &member.prop {
                    self.analyze_expr(&c.expr)?;
                }
            }
            Expr::Assign(assign) => {
                if let AssignTarget::Simple(SimpleAssignTarget::Ident(id)) = &assign.left {
                    let name = id.id.sym.to_string();
                    if let Some(info) = self.symbols.get(&name)
                        && info.immut_borrows > 0
                    {
                        return Err(format!(
                            "BORROW ERROR: Cannot assign to '{}' while it is borrowed",
                            name
                        ));
                    }
                }
                if let Expr::Ident(id) = assign.right.as_ref() {
                    self.process_move(id.sym.as_ref())?;
                } else {
                    self.analyze_expr(&assign.right)?;
                }
            }
            Expr::Bin(bin) => {
                self.analyze_expr(&bin.left)?;
                self.analyze_expr(&bin.right)?;
            }
            Expr::Unary(un) => {
                self.analyze_expr(&un.arg)?;
            }
            Expr::Call(call) => {
                for arg in &call.args {
                    if let Expr::Ident(id) = arg.expr.as_ref() {
                        self.process_borrow(id.sym.as_ref(), false)?;
                    } else {
                        self.analyze_expr(&arg.expr)?;
                    }
                }
                if let Callee::Expr(callee_expr) = &call.callee {
                    self.analyze_expr(callee_expr)?;
                }
            }
            Expr::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    self.analyze_expr(&elem.expr)?;
                }
            }
            Expr::Object(obj) => {
                for prop in &obj.props {
                    if let PropOrSpread::Prop(p) = prop
                        && let Prop::KeyValue(kv) = p.as_ref()
                    {
                        self.analyze_expr(&kv.value)?;
                    }
                }
            }
            Expr::Arrow(arrow) => {
                self.analyze_closure(&arrow.params, &arrow.body)?;
            }
            Expr::Fn(fn_expr) => {
                self.analyze_fn_closure(fn_expr)?;
            }
            Expr::Cond(cond) => {
                self.analyze_expr(&cond.test)?;
                self.analyze_expr(&cond.cons)?;
                self.analyze_expr(&cond.alt)?;
            }
            Expr::Paren(paren) => {
                self.analyze_expr(&paren.expr)?;
            }
            Expr::Update(update) => {
                self.analyze_expr(&update.arg)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn process_use(&mut self, name: &str) -> Result<(), String> {
        if let Some(info) = self.symbols.get_mut(name) {
            if info.state == VarState::Moved {
                let moved_at = info.moved_span.unwrap_or_default();
                self.errors.push(TypeError::UseAfterMove {
                    var: name.to_string(),
                    moved_at,
                    used_at: Span::default(),
                });
                return Err(format!("BORROW ERROR: Use of moved variable '{}'", name));
            }

            if info.state == VarState::CapturedByAsync {
                return Err(format!(
                    "BORROW ERROR: '{}' was moved into an async closure! Cannot use after capture.",
                    name
                ));
            }
        }
        Ok(())
    }

    /// Mark variable as moved. Only for actual ownership transfers (e.g., `let y = x;`).
    fn process_move(&mut self, name: &str) -> Result<(), String> {
        if let Some(info) = self.symbols.get_mut(name) {
            if info.state == VarState::Moved {
                let moved_at = info.moved_span.unwrap_or_default();
                self.errors.push(TypeError::UseAfterMove {
                    var: name.to_string(),
                    moved_at,
                    used_at: Span::default(),
                });
                return Err(format!("BORROW ERROR: Use of moved variable '{}'", name));
            }

            if info.state == VarState::CapturedByAsync {
                return Err(format!(
                    "BORROW ERROR: '{}' was moved into an async closure! Cannot use after capture.",
                    name
                ));
            }

            if info.is_move() && info.immut_borrows == 0 && !info.mut_borrow && !info.is_global() {
                info.state = VarState::Moved;
                info.moved_span = Some(Span::default());
            }
        }
        Ok(())
    }

    fn process_borrow(&mut self, name: &str, mutable: bool) -> Result<(), String> {
        if let Some(info) = self.symbols.get_mut(name) {
            if info.state == VarState::Moved {
                return Err(format!(
                    "BORROW ERROR: Cannot borrow moved variable '{}'",
                    name
                ));
            }

            if info.state == VarState::CapturedByAsync {
                return Err(format!(
                    "BORROW ERROR: Cannot borrow '{}' - it was captured by an async closure!",
                    name
                ));
            }

            if mutable {
                if info.immut_borrows > 0 {
                    self.errors.push(TypeError::BorrowConflict {
                        var: name.to_string(),
                        existing: BorrowKind::Immutable,
                        new: BorrowKind::Mutable,
                        span: Span::default(),
                    });
                    return Err(format!(
                        "BORROW ERROR: Cannot borrow '{}' as mutable while it is already borrowed",
                        name
                    ));
                }
                if info.mut_borrow {
                    self.errors.push(TypeError::BorrowConflict {
                        var: name.to_string(),
                        existing: BorrowKind::Mutable,
                        new: BorrowKind::Mutable,
                        span: Span::default(),
                    });
                    return Err(format!(
                        "BORROW ERROR: Cannot borrow '{}' as mutable more than once",
                        name
                    ));
                }
                info.mut_borrow = true;
            } else {
                if info.mut_borrow {
                    self.errors.push(TypeError::BorrowConflict {
                        var: name.to_string(),
                        existing: BorrowKind::Mutable,
                        new: BorrowKind::Immutable,
                        span: Span::default(),
                    });
                    return Err(format!(
                        "BORROW ERROR: Cannot borrow '{}' as immutable while it is mutably borrowed",
                        name
                    ));
                }
                info.immut_borrows += 1;
            }
        }
        Ok(())
    }

    pub fn release_borrow(&mut self, name: &str, mutable: bool) {
        if let Some(info) = self.symbols.get_mut(name) {
            if mutable {
                info.mut_borrow = false;
            } else {
                info.immut_borrows = info.immut_borrows.saturating_sub(1);
            }
        }
    }

    fn analyze_closure(&mut self, params: &[Pat], body: &BlockStmtOrExpr) -> Result<(), String> {
        let param_names: HashSet<String> = params
            .iter()
            .filter_map(|p| {
                if let Pat::Ident(id) = p {
                    Some(id.id.sym.to_string())
                } else {
                    None
                }
            })
            .collect();

        let mut captured = HashSet::new();
        match body {
            BlockStmtOrExpr::Expr(e) => {
                self.scan_expr_for_captures(e, &param_names, &mut captured);
            }
            BlockStmtOrExpr::BlockStmt(block) => {
                for stmt in &block.stmts {
                    self.scan_stmt_for_captures(stmt, &param_names, &mut captured);
                }
            }
        }

        for var_name in &captured {
            self.process_capture(var_name)?;
        }

        Ok(())
    }

    fn analyze_fn_closure(&mut self, fn_expr: &FnExpr) -> Result<(), String> {
        let param_names: HashSet<String> = fn_expr
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

        let mut captured = HashSet::new();
        if let Some(body) = &fn_expr.function.body {
            for stmt in &body.stmts {
                self.scan_stmt_for_captures(stmt, &param_names, &mut captured);
            }
        }

        for var_name in &captured {
            self.process_capture(var_name)?;
        }

        Ok(())
    }

    fn process_capture(&mut self, name: &str) -> Result<(), String> {
        if let Some(info) = self.symbols.get_mut(name) {
            if info.is_global() {
                return Ok(());
            }

            if info.state == VarState::Moved || info.state == VarState::CapturedByAsync {
                return Err(format!(
                    "BORROW ERROR: Variable '{}' was already moved or captured",
                    name
                ));
            }

            if info.immut_borrows > 0 || info.mut_borrow {
                return Err(format!(
                    "LIFETIME ERROR: Cannot capture '{}' while it has active borrow(s)",
                    name
                ));
            }

            if info.is_move() {
                info.state = VarState::CapturedByAsync;
            }
        }
        Ok(())
    }

    fn scan_expr_for_captures(
        &self,
        expr: &Expr,
        local_vars: &HashSet<String>,
        captured: &mut HashSet<String>,
    ) {
        match expr {
            Expr::Ident(id) => {
                let name = id.sym.to_string();
                if !local_vars.contains(&name) && self.symbols.contains_key(&name) {
                    captured.insert(name);
                }
            }
            Expr::Bin(bin) => {
                self.scan_expr_for_captures(&bin.left, local_vars, captured);
                self.scan_expr_for_captures(&bin.right, local_vars, captured);
            }
            Expr::Call(call) => {
                for arg in &call.args {
                    self.scan_expr_for_captures(&arg.expr, local_vars, captured);
                }
                if let Callee::Expr(callee) = &call.callee {
                    self.scan_expr_for_captures(callee, local_vars, captured);
                }
            }
            Expr::Member(member) => {
                self.scan_expr_for_captures(&member.obj, local_vars, captured);
                if let MemberProp::Computed(c) = &member.prop {
                    self.scan_expr_for_captures(&c.expr, local_vars, captured);
                }
            }
            Expr::Object(obj) => {
                for prop in &obj.props {
                    if let PropOrSpread::Prop(p) = prop
                        && let Prop::KeyValue(kv) = p.as_ref()
                    {
                        self.scan_expr_for_captures(&kv.value, local_vars, captured);
                    }
                }
            }
            Expr::Array(arr) => {
                for elem in arr.elems.iter().flatten() {
                    self.scan_expr_for_captures(&elem.expr, local_vars, captured);
                }
            }
            Expr::Assign(assign) => {
                self.scan_expr_for_captures(&assign.right, local_vars, captured);
            }
            Expr::Unary(un) => {
                self.scan_expr_for_captures(&un.arg, local_vars, captured);
            }
            Expr::Cond(cond) => {
                self.scan_expr_for_captures(&cond.test, local_vars, captured);
                self.scan_expr_for_captures(&cond.cons, local_vars, captured);
                self.scan_expr_for_captures(&cond.alt, local_vars, captured);
            }
            Expr::Paren(paren) => {
                self.scan_expr_for_captures(&paren.expr, local_vars, captured);
            }
            _ => {}
        }
    }

    fn scan_stmt_for_captures(
        &self,
        stmt: &Stmt,
        local_vars: &HashSet<String>,
        captured: &mut HashSet<String>,
    ) {
        match stmt {
            Stmt::Expr(expr_stmt) => {
                self.scan_expr_for_captures(&expr_stmt.expr, local_vars, captured);
            }
            Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    self.scan_expr_for_captures(arg, local_vars, captured);
                }
            }
            Stmt::Block(block) => {
                for s in &block.stmts {
                    self.scan_stmt_for_captures(s, local_vars, captured);
                }
            }
            Stmt::Decl(Decl::Var(var_decl)) => {
                for decl in var_decl.decls.iter() {
                    if let Some(init) = &decl.init {
                        self.scan_expr_for_captures(init, local_vars, captured);
                    }
                }
            }
            Stmt::If(if_stmt) => {
                self.scan_expr_for_captures(&if_stmt.test, local_vars, captured);
                self.scan_stmt_for_captures(&if_stmt.cons, local_vars, captured);
                if let Some(alt) = &if_stmt.alt {
                    self.scan_stmt_for_captures(alt, local_vars, captured);
                }
            }
            Stmt::While(while_stmt) => {
                self.scan_expr_for_captures(&while_stmt.test, local_vars, captured);
                self.scan_stmt_for_captures(&while_stmt.body, local_vars, captured);
            }
            Stmt::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    match init {
                        VarDeclOrExpr::VarDecl(var_decl) => {
                            for decl in &var_decl.decls {
                                if let Some(init) = &decl.init {
                                    self.scan_expr_for_captures(init, local_vars, captured);
                                }
                            }
                        }
                        VarDeclOrExpr::Expr(expr) => {
                            self.scan_expr_for_captures(expr, local_vars, captured);
                        }
                    }
                }
                if let Some(test) = &for_stmt.test {
                    self.scan_expr_for_captures(test, local_vars, captured);
                }
                if let Some(update) = &for_stmt.update {
                    self.scan_expr_for_captures(update, local_vars, captured);
                }
                self.scan_stmt_for_captures(&for_stmt.body, local_vars, captured);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_copy() {
        let mut checker = BorrowChecker::new();
        checker.define("x".to_string(), Type::Number, Span::default());

        assert!(checker.process_use("x").is_ok());
        assert!(checker.process_use("x").is_ok());
    }

    #[test]
    fn test_heap_use_does_not_move() {
        let mut checker = BorrowChecker::new();
        checker.enter_scope();
        checker.define(
            "arr".to_string(),
            Type::Array(Box::new(Type::Number)),
            Span::default(),
        );

        assert!(checker.process_use("arr").is_ok());
        assert!(checker.process_use("arr").is_ok());
        assert!(checker.process_borrow("arr", false).is_ok());
    }

    #[test]
    fn test_heap_move_then_use_fails() {
        let mut checker = BorrowChecker::new();
        checker.enter_scope();
        checker.define(
            "arr".to_string(),
            Type::Array(Box::new(Type::Number)),
            Span::default(),
        );

        assert!(checker.process_move("arr").is_ok());
        assert!(checker.process_use("arr").is_err());
    }

    #[test]
    fn test_heap_move_then_borrow_fails() {
        let mut checker = BorrowChecker::new();
        checker.enter_scope();
        checker.define(
            "arr".to_string(),
            Type::Array(Box::new(Type::Number)),
            Span::default(),
        );

        assert!(checker.process_move("arr").is_ok());
        assert!(checker.process_borrow("arr", false).is_err());
    }

    #[test]
    fn test_double_move_fails() {
        let mut checker = BorrowChecker::new();
        checker.enter_scope();
        checker.define(
            "arr".to_string(),
            Type::Array(Box::new(Type::Number)),
            Span::default(),
        );

        assert!(checker.process_move("arr").is_ok());
        assert!(checker.process_move("arr").is_err());
    }

    #[test]
    fn test_borrow_conflict() {
        let mut checker = BorrowChecker::new();
        checker.define("x".to_string(), Type::String, Span::default());

        assert!(checker.process_borrow("x", false).is_ok());
        assert!(checker.process_borrow("x", true).is_err());
    }

    #[test]
    fn test_multiple_immutable_borrows() {
        let mut checker = BorrowChecker::new();
        checker.define("x".to_string(), Type::String, Span::default());

        assert!(checker.process_borrow("x", false).is_ok());
        assert!(checker.process_borrow("x", false).is_ok());
    }

    #[test]
    fn test_global_variables_not_moved() {
        let mut checker = BorrowChecker::new();
        checker.define("Pipeline".to_string(), Type::Any, Span::default());

        assert!(checker.process_use("Pipeline").is_ok());
        assert!(checker.process_use("Pipeline").is_ok());
        assert!(checker.process_borrow("Pipeline", false).is_ok());
        assert!(checker.process_move("Pipeline").is_ok());
        assert!(checker.process_use("Pipeline").is_ok()); // Globals aren't moved
    }

    #[test]
    fn test_primitive_move_is_copy() {
        let mut checker = BorrowChecker::new();
        checker.enter_scope();
        checker.define("x".to_string(), Type::Number, Span::default());

        assert!(checker.process_move("x").is_ok());
        assert!(checker.process_use("x").is_ok()); // Primitives are Copy
    }

    #[test]
    fn test_cannot_move_while_borrowed() {
        let mut checker = BorrowChecker::new();
        checker.enter_scope();
        checker.define(
            "arr".to_string(),
            Type::Array(Box::new(Type::Number)),
            Span::default(),
        );

        assert!(checker.process_borrow("arr", false).is_ok());
        assert!(checker.process_move("arr").is_ok()); // Blocked by borrow
        assert!(checker.process_use("arr").is_ok()); // Still valid
    }

    #[test]
    fn test_property_access_does_not_move() {
        let mut checker = BorrowChecker::new();
        checker.enter_scope();
        checker.define(
            "arr".to_string(),
            Type::Array(Box::new(Type::Any)),
            Span::default(),
        );
        checker.define("c".to_string(), Type::Any, Span::default());

        assert!(checker.process_borrow("arr", false).is_ok());
        assert!(checker.process_borrow("c", false).is_ok());
        assert!(checker.process_use("c").is_ok());
        assert!(checker.process_borrow("c", false).is_ok());
    }
}

#[test]
fn test_move_tracking_full_flow() {
    use swc_common::{FileName, SourceMap, sync::Lrc};
    use swc_ecma_parser::{Parser, StringInput, Syntax, lexer::Lexer};

    let source = r#"
        let data = [1, 2, 3];
        let other = data;
        console.log(data.length);
    "#;

    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        FileName::Custom("test.ot".into()).into(),
        source.to_string(),
    );
    let syntax = Syntax::Typescript(Default::default());
    let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);
    let program = parser.parse_program().unwrap();

    let mut checker = BorrowChecker::new();
    checker.enter_scope();

    let mut found_error = false;
    match &program {
        swc_ecma_ast::Program::Script(script) => {
            for stmt in &script.body {
                if let Err(e) = checker.analyze_stmt(stmt) {
                    assert!(e.contains("Cannot borrow moved variable"));
                    found_error = true;
                    break;
                }
            }
        }
        _ => panic!("Expected Script"),
    }

    checker.exit_scope();
    assert!(
        found_error,
        "Expected borrow error for use of moved variable"
    );
}

#[test]
fn test_member_access_borrows_not_moves() {
    use swc_common::{FileName, SourceMap, sync::Lrc};
    use swc_ecma_parser::{Parser, StringInput, Syntax, lexer::Lexer};

    let source = r#"
        let arr = [{kind: "test"}];
        let c = arr[0];
        console.log(c.kind);
        console.log(arr.length);
    "#;

    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        FileName::Custom("test.ot".into()).into(),
        source.to_string(),
    );
    let syntax = Syntax::Typescript(Default::default());
    let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);
    let program = parser.parse_program().unwrap();

    let mut checker = BorrowChecker::new();
    checker.enter_scope();

    match &program {
        swc_ecma_ast::Program::Script(script) => {
            for stmt in &script.body {
                checker
                    .analyze_stmt(stmt)
                    .expect("Member access should borrow, not move");
            }
        }
        _ => panic!("Expected Script"),
    }

    checker.exit_scope();
}
