//! Classes.

use super::*;

impl Codegen {
    /// A class: a wrapper object (`constructor`, `prototype`, `name`, static
    /// members) whose `prototype` holds the methods. A derived class links
    /// both to its parent's, and binds the parent as `%super`, which the
    /// constructor and methods reach as a captured variable.
    /// Stack: [] -> [class]
    pub(super) fn gen_class(&mut self, class: &Class, name: Option<&str>) {
        let prev_class_name = std::mem::replace(&mut self.class_name, name.map(str::to_string));
        self.gen_class_inner(class, name);
        self.class_name = prev_class_name;
    }

    pub(super) fn gen_class_inner(&mut self, class: &Class, name: Option<&str>) {
        for member in &class.body {
            let member_decorated = match member {
                ClassMember::StaticBlock(_) => {
                    self.unsupported("static initialization blocks");
                    false
                }
                ClassMember::AutoAccessor(_) => {
                    self.unsupported("`accessor` class fields");
                    false
                }
                ClassMember::Method(m) => {
                    m.function.params.iter().any(|p| !p.decorators.is_empty())
                }
                ClassMember::PrivateMethod(m) => {
                    m.function.params.iter().any(|p| !p.decorators.is_empty())
                }
                ClassMember::Constructor(k) => k.params.iter().any(|p| match p {
                    ParamOrTsParamProp::Param(p) => !p.decorators.is_empty(),
                    ParamOrTsParamProp::TsParamProp(tp) => !tp.decorators.is_empty(),
                }),
                _ => false,
            };
            if member_decorated {
                self.unsupported("parameter decorators");
            }
        }
        let id = self.instructions.len();
        let wrapper = format!("%class{}", id);
        let proto = format!("%proto{}", id);
        let private: HashMap<String, String> = class
            .body
            .iter()
            .filter_map(|m| match m {
                ClassMember::PrivateProp(p) => Some(p.key.name.to_string()),
                ClassMember::PrivateMethod(m) => Some(m.key.name.to_string()),
                _ => None,
            })
            .map(|n| (n.clone(), format!("#{}@{}", n, class.span.lo.0)))
            .collect();
        self.private_names.push(private);

        // The class scope: `%super`, and what field decorators return.
        if let Some(parent) = &class.super_class {
            self.gen_expr(parent);
        }
        let class_env = self.enter_scope_env(class.span);
        if class.super_class.is_some() {
            self.store_var(&scope::super_ident(class.span), true);
        }
        let parent_span = class.super_class.as_ref().map(|_| class.span);

        // The prototype, inheriting from the parent's.
        self.instructions.push(OpCode::NewObject);
        self.instructions.push(OpCode::Let(proto.clone()));
        if let Some(ps) = parent_span {
            self.instructions.push(OpCode::Load(proto.clone()));
            self.load_var(&scope::super_ident(ps));
            self.instructions
                .push(OpCode::GetProp("prototype".to_string()));
            self.instructions
                .push(OpCode::SetProp("__proto__".to_string()));
        }

        // The class object; static members inherit from the parent class.
        self.instructions.push(OpCode::NewObject);
        self.instructions.push(OpCode::Let(wrapper.clone()));
        // Marks a class: `typeof` is "function", printed as `[class C]`.
        self.instructions.push(OpCode::Load(wrapper.clone()));
        self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
        self.instructions
            .push(OpCode::SetProp("__class__".to_string()));
        if let Some(class_name) = name {
            self.instructions.push(OpCode::Load(wrapper.clone()));
            self.instructions
                .push(OpCode::Push(JsValue::String(class_name.to_string())));
            self.instructions.push(OpCode::SetProp("name".to_string()));
        }
        self.instructions.push(OpCode::Load(wrapper.clone()));
        self.instructions.push(OpCode::Load(proto.clone()));
        self.instructions
            .push(OpCode::SetProp("prototype".to_string()));
        self.instructions.push(OpCode::Load(proto.clone()));
        self.instructions.push(OpCode::Load(wrapper.clone()));
        self.instructions
            .push(OpCode::SetProp("constructor".to_string()));
        if let Some(ps) = parent_span {
            self.instructions.push(OpCode::Load(wrapper.clone()));
            self.load_var(&scope::super_ident(ps));
            self.instructions
                .push(OpCode::SetProp("__proto__".to_string()));
        }

        // The constructor, with the instance fields.
        let fields: Vec<ClassField> = class
            .body
            .iter()
            .filter_map(|m| match m {
                ClassMember::ClassProp(p) if !p.is_static => {
                    self.class_key(&p.key).map(|key| ClassField {
                        key,
                        value: p.value.clone(),
                        decorators: p.decorators.iter().map(|d| d.span).collect(),
                    })
                }
                ClassMember::PrivateProp(p) if !p.is_static => Some(ClassField {
                    key: self.private_key(&p.key.name),
                    value: p.value.clone(),
                    decorators: p.decorators.iter().map(|d| d.span).collect(),
                }),
                _ => None,
            })
            .collect();
        let ctor = class.body.iter().find_map(|m| match m {
            ClassMember::Constructor(k) => Some(k),
            _ => None,
        });
        let derived = class.super_class.is_some();
        self.instructions.push(OpCode::Load(wrapper.clone()));
        match ctor {
            Some(k) => {
                let owned: Vec<Pat> = k.params.iter().map(scope::ctor_param_pat).collect();
                let pats: Vec<&Pat> = owned.iter().collect();
                // `constructor(private x)` assigns `this.x = x` first, like
                // a field initialized from the parameter.
                let mut ctor_fields: Vec<ClassField> = k
                    .params
                    .iter()
                    .filter_map(scope::param_prop_ident)
                    .map(|id| ClassField {
                        key: id.sym.to_string(),
                        value: Some(Box::new(Expr::Ident(id))),
                        decorators: Vec::new(),
                    })
                    .collect();
                ctor_fields.extend(fields.iter().cloned());
                let stmts: &[Stmt] = k.body.as_ref().map_or(&[], |b| b.stmts.as_slice());
                let kind = FnKind::Ctor {
                    derived,
                    implicit: false,
                };
                self.next_fn_name = Some(format!("new {}", self.class_display_name()));
                self.gen_member_function(k.span, &pats, stmts, false, kind, &ctor_fields);
            }
            None => {
                let kind = FnKind::Ctor {
                    derived,
                    implicit: true,
                };
                let span = scope::implicit_ctor_span(class.span);
                self.next_fn_name = Some(format!("new {}", self.class_display_name()));
                self.gen_member_function(span, &[], &[], false, kind, &fields);
            }
        }
        self.instructions
            .push(OpCode::SetProp("constructor".to_string()));

        // Methods (on the class for static ones, which are decorated first,
        // as TypeScript does; then on the prototype), then fields.
        for static_pass in [true, false] {
            for member in &class.body {
                let (key, function, is_static, kind, shown, private) = match member {
                    ClassMember::Method(m) => {
                        let Some(name) = self.class_key(&m.key) else {
                            continue;
                        };
                        let key = match m.kind {
                            MethodKind::Getter => format!("getter:{}", name),
                            MethodKind::Setter => format!("setter:{}", name),
                            MethodKind::Method => name.clone(),
                        };
                        (key, &m.function, m.is_static, m.kind, name, false)
                    }
                    ClassMember::PrivateMethod(m) => {
                        let name = self.private_key(&m.key.name);
                        let key = match m.kind {
                            MethodKind::Getter => format!("getter:{}", name),
                            MethodKind::Setter => format!("setter:{}", name),
                            MethodKind::Method => name,
                        };
                        let shown = format!("#{}", m.key.name);
                        (key, &m.function, m.is_static, m.kind, shown, true)
                    }
                    _ => continue,
                };
                if is_static != static_pass {
                    continue;
                }
                if function.is_generator {
                    self.unsupported("generator methods");
                }
                let target = if is_static { &wrapper } else { &proto };
                self.instructions.push(OpCode::Load(target.clone()));
                let pats: Vec<&Pat> = function.params.iter().map(|p| &p.pat).collect();
                let stmts: &[Stmt] = function.body.as_ref().map_or(&[], |b| b.stmts.as_slice());
                self.next_fn_name = Some(format!("{}.{}", self.class_display_name(), shown));
                self.gen_member_function(
                    function.span,
                    &pats,
                    stmts,
                    function.is_async,
                    FnKind::Method { is_static },
                    &[],
                );
                let kind = match kind {
                    MethodKind::Getter => "getter",
                    MethodKind::Setter => "setter",
                    MethodKind::Method => "method",
                };
                self.emit_decorators(&function.decorators, kind, Some(&shown), is_static, private);
                self.instructions.push(OpCode::SetProp(key));
            }
        }
        // What each field decorator returns: an initializer for every
        // instance's value, kept in the class scope for the constructor.
        for member in &class.body {
            let (decorators, name, private) = match member {
                ClassMember::ClassProp(p) if !p.is_static => {
                    (&p.decorators, self.class_key(&p.key), false)
                }
                ClassMember::PrivateProp(p) if !p.is_static => {
                    (&p.decorators, Some(format!("#{}", p.key.name)), true)
                }
                _ => continue,
            };
            for d in decorators {
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.emit_decorator_context("field", name.as_deref(), false, private);
                self.gen_expr(&d.expr);
                self.instructions.push(OpCode::Call(2));
                self.store_var(&scope::field_init_ident(d.span), true);
            }
        }

        for member in &class.body {
            let (key, value, decorators, shown, private) = match member {
                ClassMember::ClassProp(p) if p.is_static => match self.class_key(&p.key) {
                    Some(k) => (k.clone(), &p.value, &p.decorators, k, false),
                    None => continue,
                },
                ClassMember::PrivateProp(p) if p.is_static => (
                    self.private_key(&p.key.name),
                    &p.value,
                    &p.decorators,
                    format!("#{}", p.key.name),
                    true,
                ),
                _ => continue,
            };
            self.instructions.push(OpCode::Load(wrapper.clone()));
            match value {
                Some(v) => self.gen_expr(v),
                None => self.instructions.push(OpCode::Push(JsValue::Undefined)),
            }
            // [C, v] -> each decorator's initializer, called with `this = C`
            for d in decorators.iter() {
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.emit_decorator_context("field", Some(&shown), true, private);
                self.gen_expr(&d.expr);
                self.instructions.push(OpCode::Call(2));
                self.instructions.push(OpCode::Load(wrapper.clone()));
                self.emit_call_initializer();
            }
            self.instructions.push(OpCode::SetProp(key));
        }

        self.instructions.push(OpCode::Load(wrapper));
        self.exit_scope_env(class_env);
        self.private_names.pop();

        // Class decorators: `C = decorator(C, context) ?? C`.
        self.emit_decorators(&class.decorators, "class", name, false, false);
    }

    /// The class being compiled, as stack traces show it.
    pub(super) fn class_display_name(&self) -> String {
        self.class_name
            .clone()
            .unwrap_or_else(|| "<anonymous>".to_string())
    }

    /// The name of a class member's key; computed keys are unsupported.
    pub(super) fn class_key(&mut self, key: &PropName) -> Option<String> {
        match key {
            PropName::Ident(id) => Some(id.sym.to_string()),
            PropName::Str(s) => Some(s.value.to_string_lossy().into_owned()),
            PropName::Num(n) => Some(crate::stdlib::number_to_string(n.value)),
            _ => {
                self.unsupported("computed class member names");
                None
            }
        }
    }
}
