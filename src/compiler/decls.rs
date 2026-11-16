//! Programs and declarations: modules, scripts, imports and exports.

use super::*;

impl Codegen {
    pub fn generate(&mut self, module: &Module) -> Vec<OpCode> {
        self.top_level_prologue();
        self.namespace_imports();
        let top_stmts: Vec<Stmt> = module
            .body
            .iter()
            .filter_map(|item| match item {
                ModuleItem::Stmt(s) => Some(s.clone()),
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(e)) => {
                    Some(Stmt::Decl(e.decl.clone()))
                }
                _ => None,
            })
            .collect();
        for ident in scope::var_declared(&top_stmts) {
            self.instructions.push(OpCode::Push(JsValue::Undefined));
            self.store_var(&ident, true);
        }
        for item in &module.body {
            match item {
                ModuleItem::Stmt(Stmt::Decl(Decl::Fn(f))) => self.hoist_fn(f),
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                    decl: Decl::Fn(f),
                    ..
                })) => self.hoist_fn(f),
                _ => {}
            }
        }
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

    /// `import * as ns from "m"` in a linked module: `ns` is an object of
    /// `m`'s exports, made before the module's code runs (imports are
    /// hoisted).
    pub(super) fn namespace_imports(&mut self) {
        let Some(link) = &self.link else {
            return;
        };
        let mut namespaces: Vec<(String, Vec<(String, String)>)> = link
            .namespaces
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        namespaces.sort();
        let prefix = link.prefix.clone();
        for (local, members) in namespaces {
            self.instructions.push(OpCode::NewObject);
            // Each member is a getter of the binding, so it is live, as
            // the module's own name for it is.
            for (name, global) in members {
                self.instructions.push(OpCode::Dup);
                let value_idx = self.emit_function_value(Span::default(), 0);
                let jump_idx = self.emit_jump();
                let body_start = self.instructions.len();
                self.patch_function_value(value_idx, body_start);
                self.instructions.push(OpCode::Load(global));
                self.instructions.push(OpCode::Return);
                self.patch_jump_here(jump_idx);
                self.instructions
                    .push(OpCode::SetProp(format!("getter:{name}")));
            }
            self.instructions
                .push(OpCode::Let(format!("{prefix}{local}")));
        }
    }

    /// An `import` / `export` in a linked module (`modules.rs`). Imports and
    /// re-exports were resolved when the program was linked: references
    /// name the bindings themselves, so they compile to nothing. Only a
    /// declaration, and `export default`'s value, make code.
    pub(super) fn gen_linked_module_decl(&mut self, decl: &ModuleDecl) {
        let prefix = self
            .link
            .as_ref()
            .map(|l| l.prefix.clone())
            .unwrap_or_default();
        let default_export = OpCode::Let(format!("{prefix}default"));
        match decl {
            ModuleDecl::ExportDecl(export_decl) => self.gen_decl(&export_decl.decl),
            ModuleDecl::ExportDefaultDecl(export_default) => match &export_default.decl {
                DefaultDecl::Class(class_expr) => {
                    self.gen_class(
                        &class_expr.class,
                        class_expr.ident.as_ref().map(|id| id.sym.as_str()),
                    );
                    if let Some(ident) = &class_expr.ident {
                        self.instructions.push(OpCode::Dup);
                        self.store_var(ident, true);
                    }
                    self.instructions.push(default_export);
                }
                DefaultDecl::Fn(fn_expr) => {
                    self.gen_fn_decl(fn_expr.ident.as_ref(), &fn_expr.function);
                    if let Some(ident) = &fn_expr.ident {
                        self.load_var(ident);
                    }
                    self.instructions.push(default_export);
                }
                DefaultDecl::TsInterfaceDecl(_) => {}
            },
            ModuleDecl::ExportDefaultExpr(export_default) => {
                self.gen_expr(&export_default.expr);
                self.instructions.push(default_export);
            }
            ModuleDecl::Import(_)
            | ModuleDecl::ExportNamed(_)
            | ModuleDecl::ExportAll(_)
            | ModuleDecl::TsImportEquals(_)
            | ModuleDecl::TsExportAssignment(_)
            | ModuleDecl::TsNamespaceExport(_) => {}
        }
    }

    pub(super) fn gen_module_decl(&mut self, decl: &ModuleDecl) {
        if self.link.is_some() {
            self.gen_linked_module_decl(decl);
            return;
        }
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
                    self.gen_fn_decl(fn_expr.ident.as_ref(), &fn_expr.function);
                }
                DefaultDecl::TsInterfaceDecl(_) => {
                    // Interfaces are compile-time only
                }
            },
            ModuleDecl::ExportDefaultExpr(export_default) => match &*export_default.expr {
                Expr::Fn(fn_expr) => {
                    self.gen_fn_decl(fn_expr.ident.as_ref(), &fn_expr.function);
                }
                Expr::Class(class_expr) => {
                    self.gen_class(
                        &class_expr.class,
                        class_expr.ident.as_ref().map(|id| id.sym.as_str()),
                    );
                }
                _ => self.unsupported(
                    "`export default` of an expression other than a function or class",
                ),
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

    pub(super) fn gen_decl(&mut self, decl: &Decl) {
        // `declare ...` describes something defined elsewhere: no code.
        if scope::is_ambient(decl) {
            return;
        }
        match decl {
            Decl::Fn(fn_decl) => {
                if !self.is_hoisted(fn_decl) {
                    self.gen_fn_decl(Some(&fn_decl.ident), &fn_decl.function);
                }
            }
            Decl::Class(class_decl) => {
                self.gen_class(&class_decl.class, Some(class_decl.ident.sym.as_str()));
                self.store_var(&class_decl.ident, true);
            }
            Decl::Var(var_decl) => {
                self.gen_var_decl(var_decl);
            }
            Decl::TsEnum(enum_decl) => {
                // Generate enum as a runtime object

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

                // Bind the enum object to its name.
                self.store_var(&enum_decl.id, true);
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
            Decl::Using(_) => self.unsupported("`using` declarations"),
        }
    }

    pub fn generate_script(&mut self, script: &Script) -> Vec<OpCode> {
        self.top_level_prologue();
        self.hoist_body(&script.body, &[]);
        for stmt in &script.body {
            self.gen_stmt(stmt);
        }
        self.instructions.push(OpCode::Halt);
        self.instructions.clone()
    }
}
