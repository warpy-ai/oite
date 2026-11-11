//! Declarations: the types, classes and functions a block declares are
//! known throughout it, so they are collected before its statements are
//! checked.

use super::resolve::prop_name;
use super::*;
use swc_common::Spanned;

impl Checker {
    /// Declare what `stmts` declare: first the names of the types, then
    /// what each type is (they may refer to each other), then the
    /// functions and classes as values.
    pub(super) fn hoist<'s>(&mut self, stmts: impl Iterator<Item = &'s Stmt>) {
        let decls: Vec<&Decl> = stmts
            .filter_map(|s| match s {
                Stmt::Decl(d) => Some(d),
                _ => None,
            })
            .collect();
        self.hoist_decls(&decls);
    }

    pub(super) fn hoist_decls(&mut self, decls: &[&Decl]) {
        let mut ids = Vec::new();
        for decl in decls {
            let name = match decl {
                Decl::TsInterface(i) => i.id.sym.to_string(),
                Decl::TsTypeAlias(a) => a.id.sym.to_string(),
                Decl::Class(c) => c.ident.sym.to_string(),
                _ => {
                    ids.push(None);
                    continue;
                }
            };
            // A second `interface X` in the same block adds to the first.
            let existing = match decl {
                Decl::TsInterface(_) => {
                    self.scopes.last().and_then(|s| s.types.get(&name)).copied()
                }
                _ => None,
            };
            let id = existing.unwrap_or_else(|| {
                self.decls.push(TypeDecl::Pending(name.clone()));
                DeclId(self.decls.len() - 1)
            });
            self.scope().types.insert(name, id);
            ids.push(Some(id));
        }

        for (decl, id) in decls.iter().zip(&ids) {
            let Some(id) = *id else { continue };
            let resolved = match decl {
                Decl::TsTypeAlias(a) => {
                    let ty = self.with_type_params(a.type_params.as_deref(), |this| {
                        this.resolve(&a.type_ann)
                    });
                    TypeDecl::Alias {
                        name: a.id.sym.to_string(),
                        ty,
                    }
                }
                Decl::TsInterface(i) => self.interface(id, i),
                Decl::Class(c) => {
                    let name = c.ident.sym.to_string();
                    TypeDecl::Class(self.class_info(name, &c.class))
                }
                _ => continue,
            };
            self.decls[id.0] = resolved;
        }

        for (decl, id) in decls.iter().zip(&ids) {
            match decl {
                Decl::Fn(f) => {
                    let sig = self.signature(&f.function);
                    self.declare(f.ident.sym.as_ref(), Ty::Function(sig), true);
                }
                Decl::Class(c) => {
                    let ty = id.map_or(Ty::Any, Ty::ClassValue);
                    self.declare(c.ident.sym.as_ref(), ty, true);
                }
                Decl::TsEnum(e) => self.declare(e.id.sym.as_ref(), Ty::Any, true),
                _ => {}
            }
        }
    }

    /// An interface: the members of the interfaces it extends, then its
    /// own. If it extends something the checker cannot see into, or has
    /// members it does not model, it is `any`.
    fn interface(&mut self, id: DeclId, i: &TsInterfaceDecl) -> TypeDecl {
        let name = i.id.sym.to_string();
        let dynamic = TypeDecl::Alias {
            name: name.clone(),
            ty: Ty::Any,
        };
        // An earlier declaration of the same interface in this block.
        let mut obj = match &self.decls[id.0] {
            TypeDecl::Interface { obj, .. } => obj.clone(),
            TypeDecl::Alias { .. } => return dynamic,
            _ => ObjTy::default(),
        };
        let resolved = self.with_type_params(i.type_params.as_deref(), |this| {
            for parent in &i.extends {
                let Expr::Ident(ident) = parent.expr.as_ref() else {
                    return None;
                };
                let Some(TypeName::Declared(parent_id)) = this.lookup_type(ident.sym.as_ref())
                else {
                    return None;
                };
                let parent_ty = this.expand(&Ty::Named(parent_id));
                let members = this.members(&parent_ty)?;
                if !members.closed {
                    return None;
                }
                for prop in members.obj.props {
                    obj.set(prop);
                }
                if members.obj.index.is_some() {
                    obj.index = members.obj.index;
                }
            }
            let own = this.object_members(&i.body.body)?;
            for prop in own.props {
                obj.set(prop);
            }
            if own.index.is_some() {
                obj.index = own.index;
            }
            Some(obj)
        });
        match resolved {
            Some(obj) => TypeDecl::Interface { name, obj },
            None => dynamic,
        }
    }

    /// What a class declares: its parent, its fields, accessors and
    /// methods with their types, and its constructor's signature.
    pub(super) fn class_info(&mut self, name: String, class: &Class) -> ClassInfo {
        let parent = match class.super_class.as_deref() {
            Some(Expr::Ident(ident)) => match self.lookup_type(ident.sym.as_ref()) {
                Some(TypeName::Declared(id)) => Some(id),
                _ => None,
            },
            _ => None,
        };
        self.with_type_params(class.type_params.as_deref(), |this| {
            let mut info = ClassInfo {
                name,
                parent,
                instance: ObjTy::default(),
                statics: ObjTy::default(),
                ctor: None,
            };
            for member in &class.body {
                match member {
                    ClassMember::ClassProp(p) => {
                        let Some(name) = prop_name(&p.key) else {
                            continue;
                        };
                        let prop = Prop {
                            name,
                            ty: this.annotation(p.type_ann.as_deref()),
                            optional: p.is_optional,
                        };
                        let side = if p.is_static {
                            &mut info.statics
                        } else {
                            &mut info.instance
                        };
                        side.set(prop);
                    }
                    ClassMember::Method(m) => {
                        let Some(name) = prop_name(&m.key) else {
                            continue;
                        };
                        let sig = this.signature(&m.function);
                        let ty = match m.kind {
                            MethodKind::Method => Ty::Function(sig),
                            MethodKind::Getter => sig.ret.clone(),
                            // A setter alone says nothing about reads.
                            MethodKind::Setter => continue,
                        };
                        let side = if m.is_static {
                            &mut info.statics
                        } else {
                            &mut info.instance
                        };
                        side.set(Prop {
                            name,
                            ty,
                            optional: m.is_optional,
                        });
                    }
                    ClassMember::Constructor(c) => {
                        let mut params = Vec::new();
                        for p in &c.params {
                            match p {
                                ParamOrTsParamProp::Param(p) => params.push(this.param_of(&p.pat)),
                                // `constructor(public x: number)` is also
                                // a field.
                                ParamOrTsParamProp::TsParamProp(p) => {
                                    let param = match &p.param {
                                        TsParamPropParam::Ident(id) => Param {
                                            name: id.id.sym.to_string(),
                                            ty: this.annotation(id.type_ann.as_deref()),
                                            optional: id.id.optional,
                                            rest: false,
                                        },
                                        TsParamPropParam::Assign(a) => Param {
                                            optional: true,
                                            ..this.param_of(&a.left)
                                        },
                                    };
                                    info.instance.set(Prop {
                                        name: param.name.clone(),
                                        ty: param.ty.clone(),
                                        optional: false,
                                    });
                                    params.push(param);
                                }
                            }
                        }
                        info.ctor = Some(Rc::new(FnTy {
                            params,
                            ret: Ty::Any,
                        }));
                    }
                    _ => {}
                }
            }
            info
        })
    }

    /// The constructor signature `new` on class `id` is checked against:
    /// its own, or the nearest parent's.
    pub(super) fn constructor_of(&self, id: DeclId) -> Option<Rc<FnTy>> {
        let mut cur = Some(id);
        for _ in 0..64 {
            let class = self.class(cur?)?;
            if let Some(ctor) = &class.ctor {
                return Some(ctor.clone());
            }
            // A parent the checker cannot see: nothing to check against.
            cur = Some(class.parent?);
        }
        None
    }

    /// The static member `name` of class `id`, looking through its
    /// parents.
    pub(super) fn static_member(&self, id: DeclId, name: &str) -> Option<Prop> {
        let mut cur = Some(id);
        for _ in 0..64 {
            let class = self.class(cur?)?;
            if let Some(prop) = class.statics.get(name) {
                return Some(prop.clone());
            }
            cur = class.parent;
        }
        None
    }

    /// Check the bodies of a class: field initialisers against their
    /// declared types, and each method and the constructor as functions
    /// whose `this` is an instance.
    pub(super) fn check_class(&mut self, id: Option<DeclId>, class: &Class) {
        let this = id.map_or(Ty::Any, Ty::Named);
        if let Some(parent) = &class.super_class {
            self.expr(parent);
        }
        self.with_type_params(class.type_params.as_deref(), |checker| {
            for member in &class.body {
                match member {
                    ClassMember::ClassProp(p) => {
                        let Some(value) = &p.value else { continue };
                        let declared = checker.annotation(p.type_ann.as_deref());
                        checker.fns.push(FnCtx {
                            ret: None,
                            this: if p.is_static { Ty::Any } else { this.clone() },
                        });
                        let found = checker.expr(value);
                        checker.fns.pop();
                        checker.check_assignable(&found, &declared, value.span());
                    }
                    ClassMember::PrivateProp(p) => {
                        let Some(value) = &p.value else { continue };
                        let declared = checker.annotation(p.type_ann.as_deref());
                        checker.fns.push(FnCtx {
                            ret: None,
                            this: if p.is_static { Ty::Any } else { this.clone() },
                        });
                        let found = checker.expr(value);
                        checker.fns.pop();
                        checker.check_assignable(&found, &declared, value.span());
                    }
                    ClassMember::Method(m) => {
                        let receiver = if m.is_static { Ty::Any } else { this.clone() };
                        checker.check_function(&m.function, receiver);
                    }
                    ClassMember::PrivateMethod(m) => {
                        let receiver = if m.is_static { Ty::Any } else { this.clone() };
                        checker.check_function(&m.function, receiver);
                    }
                    ClassMember::Constructor(c) => checker.check_constructor(c, this.clone()),
                    _ => {}
                }
            }
        });
    }
}
