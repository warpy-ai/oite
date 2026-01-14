use std::collections::HashMap;
use swc_ecma_ast::*;

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum VarKind {
    Primitive, // Numbers, Booleans (Copy semantics)
    Heap,      // Objects, Arrays, Functions (Move semantics)
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum VarState {
    Owned, // Currently valid and owned by this variable
    Moved, // Data has been transferred; variable is now a "tombstone"
}

#[derive(Clone, Debug)]
pub struct VarInfo {
    pub kind: VarKind,
    pub state: VarState,
    pub active_borrows: usize, // Number of active references to this data
}

pub struct BorrowChecker {
    // Variable Name -> Metadata
    symbols: HashMap<String, VarInfo>,
}

impl BorrowChecker {
    pub fn new() -> Self {
        Self {
            symbols: HashMap::new(),
        }
    }

    /// Entry point for statement analysis
    pub fn analyze_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Decl(Decl::Var(var_decl)) => {
                for decl in &var_decl.decls {
                    let name = decl.name.as_ident().unwrap().id.sym.to_string();

                    // 1. Determine the "Kind" (Type) based on the initializer
                    let kind = if let Some(init) = &decl.init {
                        self.determine_kind(init)
                    } else {
                        VarKind::Primitive
                    };

                    // 2. If initializing from another variable (e.g., let a = b),
                    // we must analyze the right-hand side first.
                    if let Some(init) = &decl.init {
                        self.analyze_expr(init)?;
                    }

                    // 3. Register the new variable
                    self.symbols.insert(
                        name,
                        VarInfo {
                            kind,
                            state: VarState::Owned,
                            active_borrows: 0,
                        },
                    );
                }
            }
            Stmt::Expr(expr_stmt) => {
                self.analyze_expr(&expr_stmt.expr)?;
            }
            Stmt::Block(block) => {
                // Future optimization: Handle scope-clearing here
                for s in &block.stmts {
                    self.analyze_stmt(s)?;
                }
            }
            Stmt::If(if_stmt) => {
                self.analyze_expr(&if_stmt.test)?;
                self.analyze_stmt(&if_stmt.cons)?;
                if let Some(alt) = &if_stmt.alt {
                    self.analyze_stmt(alt)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Determines if an expression results in a Stack (Primitive) or Heap (Object) value
    fn determine_kind(&self, expr: &Expr) -> VarKind {
        match expr {
            Expr::Object(_) | Expr::Array(_) | Expr::Fn(_) | Expr::Arrow(_) => VarKind::Heap,
            Expr::Lit(Lit::Num(_)) | Expr::Lit(Lit::Bool(_)) => VarKind::Primitive,
            Expr::Ident(id) => {
                // Inherit kind from source variable
                self.symbols
                    .get(&id.sym.to_string())
                    .map(|info| info.kind)
                    .unwrap_or(VarKind::Primitive)
            }
            _ => VarKind::Primitive,
        }
    }

    /// Recursively checks expressions for ownership violations
    fn analyze_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            // Context: Standalone identifier use (usually a Move/Transfer)
            Expr::Ident(id) => {
                self.process_move(&id.sym.to_string())?;
            }

            // Context: Member Access (Implicit Borrow - safe)
            Expr::Member(member) => {
                if let Expr::Ident(id) = member.obj.as_ref() {
                    self.process_implicit_borrow(&id.sym.to_string())?;
                } else {
                    self.analyze_expr(&member.obj)?;
                }
            }

            // Context: Explicit Borrow using 'void' operator
            Expr::Unary(un) if un.op == UnaryOp::Void => {
                if let Expr::Ident(id) = un.arg.as_ref() {
                    self.process_explicit_borrow(&id.sym.to_string())?;
                }
            }

            // Context: Assignment (Target remains owned, Right side is moved)
            Expr::Assign(assign) => {
                self.analyze_expr(&assign.right)?;
            }

            // Context: Binary Math (Usually involves primitives)
            Expr::Bin(bin) => {
                self.analyze_expr(&bin.left)?;
                self.analyze_expr(&bin.right)?;
            }

            // Context: Function Call (Arguments are Implicit Borrows in JS)
            Expr::Call(call) => {
                for arg in &call.args {
                    if let Expr::Ident(id) = arg.expr.as_ref() {
                        self.process_implicit_borrow(&id.sym.to_string())?;
                    } else {
                        self.analyze_expr(&arg.expr)?;
                    }
                }
                if let Callee::Expr(callee_expr) = &call.callee {
                    self.analyze_expr(callee_expr)?;
                }
            }

            // Context: Object Literal
            Expr::Object(obj) => {
                for prop in &obj.props {
                    if let PropOrSpread::Prop(p) = prop
                        && let Prop::KeyValue(kv) = p.as_ref()
                    {
                        self.analyze_expr(&kv.value)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    // --- Ownership Logic Core ---

    fn process_move(&mut self, name: &str) -> Result<(), String> {
        if let Some(info) = self.symbols.get_mut(name) {
            // 1. Check for Use-After-Move
            if info.state == VarState::Moved {
                return Err(format!("BORROW ERROR: Use of moved variable '{}'", name));
            }

            // 2. Check for Lifetime Violation (Cannot move if borrowed)
            if info.active_borrows > 0 {
                return Err(format!(
                    "LIFETIME ERROR: Cannot move '{}' while it has {} active borrow(s)",
                    name, info.active_borrows
                ));
            }

            // 3. Perform Move if it's a Heap type
            if info.kind == VarKind::Heap {
                info.state = VarState::Moved;
                println!(
                    "DEBUG: Heap Object '{}' MOVED (Ownership transferred)",
                    name
                );
            } else {
                println!("DEBUG: Primitive '{}' COPIED", name);
            }
        }
        Ok(())
    }

    fn process_implicit_borrow(&mut self, name: &str) -> Result<(), String> {
        if let Some(info) = self.symbols.get_mut(name) {
            if info.state == VarState::Moved {
                return Err(format!(
                    "BORROW ERROR: Cannot access moved variable '{}'",
                    name
                ));
            }
            // For now, implicit borrows (like obj.prop) increment borrow count
            // but we'll assume they are returned after the statement ends.
            println!("DEBUG: '{}' implicitly borrowed", name);
        }
        Ok(())
    }

    fn process_explicit_borrow(&mut self, name: &str) -> Result<(), String> {
        if let Some(info) = self.symbols.get_mut(name) {
            if info.state == VarState::Moved {
                return Err(format!(
                    "BORROW ERROR: Cannot borrow moved variable '{}'",
                    name
                ));
            }
            info.active_borrows += 1;
            println!(
                "DEBUG: '{}' EXPLICITLY BORROWED. Active loans: {}",
                name, info.active_borrows
            );
        }
        Ok(())
    }
}
