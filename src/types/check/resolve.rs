//! From type syntax to types: annotations, object types, function
//! signatures.

use super::*;

impl Checker {
    /// The type an annotation names, or `any` when there is none.
    pub(super) fn annotation(&mut self, ann: Option<&TsTypeAnn>) -> Ty {
        match ann {
            Some(ann) => self.resolve(&ann.type_ann),
            None => Ty::Any,
        }
    }

    /// A type the checker does not model yet: it is `any`, and says so.
    fn unsupported(&mut self, span: Span, what: &str) -> Ty {
        self.report(
            span,
            format!("{what} are not supported yet; the type is treated as `any`"),
        );
        Ty::Any
    }

    pub(super) fn resolve(&mut self, ty: &TsType) -> Ty {
        match ty {
            TsType::TsKeywordType(kw) => match kw.kind {
                TsKeywordTypeKind::TsNumberKeyword => Ty::Number,
                TsKeywordTypeKind::TsStringKeyword => Ty::String,
                TsKeywordTypeKind::TsBooleanKeyword => Ty::Boolean,
                TsKeywordTypeKind::TsVoidKeyword => Ty::Void,
                TsKeywordTypeKind::TsNeverKeyword => Ty::Never,
                TsKeywordTypeKind::TsUndefinedKeyword => Ty::Undefined,
                TsKeywordTypeKind::TsNullKeyword => Ty::Null,
                TsKeywordTypeKind::TsUnknownKeyword => Ty::Unknown,
                TsKeywordTypeKind::TsObjectKeyword => Ty::Object(Rc::new(ObjTy::default())),
                _ => Ty::Any,
            },
            TsType::TsArrayType(arr) => Ty::array(self.resolve(&arr.elem_type)),
            TsType::TsParenthesizedType(p) => self.resolve(&p.type_ann),
            TsType::TsOptionalType(o) => Ty::union([self.resolve(&o.type_ann), Ty::Undefined]),
            TsType::TsTypeRef(r) => self.resolve_ref(r),
            TsType::TsTypeLit(lit) => self.object_type(&lit.members),
            TsType::TsLitType(lit) => match &lit.lit {
                TsLit::Str(s) => Ty::StrLit(s.value.to_string_lossy().as_ref().into()),
                TsLit::Number(_) => Ty::Number,
                TsLit::Bool(_) => Ty::Boolean,
                TsLit::Tpl(_) => Ty::String,
                TsLit::BigInt(_) => Ty::Any,
            },
            TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsUnionType(u)) => {
                let members: Vec<Ty> = u.types.iter().map(|t| self.resolve(t)).collect();
                Ty::union(members)
            }
            TsType::TsUnionOrIntersectionType(TsUnionOrIntersectionType::TsIntersectionType(i)) => {
                self.unsupported(i.span, "intersection types")
            }
            TsType::TsFnOrConstructorType(TsFnOrConstructorType::TsFnType(f)) => {
                let params = f.params.iter().map(|p| self.fn_type_param(p)).collect();
                let ret = self.resolve(&f.type_ann.type_ann);
                Ty::function(params, ret)
            }
            TsType::TsTypeOperator(op) if op.op == TsTypeOperatorOp::ReadOnly => {
                self.resolve(&op.type_ann)
            }
            TsType::TsTypePredicate(_) => Ty::Boolean,
            TsType::TsTupleType(t) => self.unsupported(t.span, "tuple types"),
            TsType::TsConditionalType(t) => self.unsupported(t.span, "conditional types"),
            TsType::TsMappedType(t) => self.unsupported(t.span, "mapped types"),
            _ => Ty::Any,
        }
    }

    fn resolve_ref(&mut self, r: &TsTypeRef) -> Ty {
        let TsEntityName::Ident(ident) = &r.type_name else {
            return Ty::Any;
        };
        let name = ident.sym.as_ref();
        let args: Vec<&TsType> = r
            .type_params
            .as_ref()
            .map(|p| p.params.iter().map(|t| t.as_ref()).collect())
            .unwrap_or_default();
        match self.lookup_type(name) {
            Some(TypeName::Param) => return Ty::Any,
            Some(TypeName::Declared(id)) => return Ty::Named(id),
            None => {}
        }
        match (name, args.as_slice()) {
            ("Array" | "ReadonlyArray", [elem]) => Ty::array(self.resolve(elem)),
            ("Array" | "ReadonlyArray", []) => Ty::array(Ty::Any),
            // Borrows are the borrow checker's business: `Ref<T>` is a `T`.
            ("Ref" | "MutRef" | "Readonly", [inner]) => self.resolve(inner),
            ("RefL" | "MutRefL", [_, inner]) => self.resolve(inner),
            _ => Ty::Any,
        }
    }

    /// The object type with the members of a type literal or interface
    /// body.
    pub(super) fn object_type(&mut self, members: &[TsTypeElement]) -> Ty {
        match self.object_members(members) {
            Some(obj) => Ty::Object(Rc::new(obj)),
            None => Ty::Any,
        }
    }

    /// The members of a type literal or interface body; `None` when it
    /// has members the checker does not model (call signatures).
    pub(super) fn object_members(&mut self, members: &[TsTypeElement]) -> Option<ObjTy> {
        let mut obj = ObjTy::default();
        for member in members {
            match member {
                TsTypeElement::TsPropertySignature(p) => {
                    let name = prop_key_name(&p.key, p.computed)?;
                    let ty = self.annotation(p.type_ann.as_deref());
                    obj.set(Prop {
                        name,
                        ty,
                        optional: p.optional,
                    });
                }
                TsTypeElement::TsMethodSignature(m) => {
                    let name = prop_key_name(&m.key, m.computed)?;
                    let params = m.params.iter().map(|p| self.fn_type_param(p)).collect();
                    let ret = self.annotation(m.type_ann.as_deref());
                    obj.set(Prop {
                        name,
                        ty: Ty::function(params, ret),
                        optional: m.optional,
                    });
                }
                TsTypeElement::TsGetterSignature(g) => {
                    let name = prop_key_name(&g.key, g.computed)?;
                    let ty = self.annotation(g.type_ann.as_deref());
                    obj.set(Prop {
                        name,
                        ty,
                        optional: false,
                    });
                }
                TsTypeElement::TsSetterSignature(_) => {}
                TsTypeElement::TsIndexSignature(i) => {
                    obj.index = Some(self.annotation(i.type_ann.as_deref()));
                }
                TsTypeElement::TsCallSignatureDecl(_)
                | TsTypeElement::TsConstructSignatureDecl(_) => return None,
            }
        }
        Some(obj)
    }

    fn fn_type_param(&mut self, param: &TsFnParam) -> Param {
        match param {
            TsFnParam::Ident(id) => Param {
                name: id.id.sym.to_string(),
                ty: self.annotation(id.type_ann.as_deref()),
                optional: id.id.optional,
                rest: false,
            },
            TsFnParam::Rest(rest) => Param {
                name: "rest".to_string(),
                ty: self.rest_element(rest.type_ann.as_deref()),
                optional: false,
                rest: true,
            },
            TsFnParam::Array(p) => Param {
                name: "arg".to_string(),
                ty: self.annotation(p.type_ann.as_deref()),
                optional: p.optional,
                rest: false,
            },
            TsFnParam::Object(p) => Param {
                name: "arg".to_string(),
                ty: self.annotation(p.type_ann.as_deref()),
                optional: p.optional,
                rest: false,
            },
        }
    }

    /// The element type of a rest parameter annotated `T[]`.
    fn rest_element(&mut self, ann: Option<&TsTypeAnn>) -> Ty {
        let ty = self.annotation(ann);
        match self.expand(&ty) {
            Ty::Array(elem) => (*elem).clone(),
            _ => Ty::Any,
        }
    }

    /// A parameter of a function or method, from its pattern.
    pub(super) fn param_of(&mut self, pat: &Pat) -> Param {
        match pat {
            Pat::Ident(id) => Param {
                name: id.id.sym.to_string(),
                ty: self.annotation(id.type_ann.as_deref()),
                optional: id.id.optional,
                rest: false,
            },
            Pat::Assign(assign) => Param {
                optional: true,
                ..self.param_of(&assign.left)
            },
            Pat::Rest(rest) => Param {
                name: pat_name(&rest.arg).unwrap_or("rest").to_string(),
                ty: self.rest_element(rest.type_ann.as_deref()),
                optional: false,
                rest: true,
            },
            Pat::Array(p) => Param {
                name: "arg".to_string(),
                ty: self.annotation(p.type_ann.as_deref()),
                optional: p.optional,
                rest: false,
            },
            Pat::Object(p) => Param {
                name: "arg".to_string(),
                ty: self.annotation(p.type_ann.as_deref()),
                optional: p.optional,
                rest: false,
            },
            _ => Param {
                name: "arg".to_string(),
                ty: Ty::Any,
                optional: false,
                rest: false,
            },
        }
    }

    /// The signature of a function. The return type of an `async` or
    /// generator function is not modelled: it is `any`.
    pub(super) fn signature(&mut self, f: &Function) -> Rc<FnTy> {
        self.with_type_params(f.type_params.as_deref(), |this| {
            let params = f.params.iter().map(|p| this.param_of(&p.pat)).collect();
            let ret = if f.is_async || f.is_generator {
                Ty::Any
            } else {
                this.annotation(f.return_type.as_deref())
            };
            Rc::new(FnTy { params, ret })
        })
    }

    pub(super) fn arrow_signature(&mut self, f: &ArrowExpr) -> Rc<FnTy> {
        self.with_type_params(f.type_params.as_deref(), |this| {
            let params = f.params.iter().map(|p| this.param_of(p)).collect();
            let ret = if f.is_async || f.is_generator {
                Ty::Any
            } else {
                this.annotation(f.return_type.as_deref())
            };
            Rc::new(FnTy { params, ret })
        })
    }

    /// Run `f` with the type parameters `params` in scope (as `any`).
    pub(super) fn with_type_params<R>(
        &mut self,
        params: Option<&TsTypeParamDecl>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let Some(params) = params else {
            return f(self);
        };
        self.push_scope();
        for p in &params.params {
            self.scope().type_params.insert(p.name.sym.to_string());
        }
        let result = f(self);
        self.scopes.pop();
        result
    }
}

/// The name a property key gives, when it is fixed.
pub(super) fn prop_key_name(key: &Expr, computed: bool) -> Option<String> {
    match key {
        Expr::Ident(id) if !computed => Some(id.sym.to_string()),
        Expr::Lit(Lit::Str(s)) => Some(s.value.to_string_lossy().into_owned()),
        Expr::Lit(Lit::Num(n)) => Some(n.value.to_string()),
        _ => None,
    }
}

/// The name a property name gives, when it is fixed.
pub(super) fn prop_name(name: &PropName) -> Option<String> {
    match name {
        PropName::Ident(id) => Some(id.sym.to_string()),
        PropName::Str(s) => Some(s.value.to_string_lossy().into_owned()),
        PropName::Num(n) => Some(n.value.to_string()),
        _ => None,
    }
}

/// The name a pattern binds, when it is a plain identifier.
pub(super) fn pat_name(pat: &Pat) -> Option<&str> {
    match pat {
        Pat::Ident(id) => Some(id.id.sym.as_ref()),
        _ => None,
    }
}
