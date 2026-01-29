//! Converts SWC TypeScript AST to tscl Type representation.

use swc_ecma_ast::{
    Expr, TsArrayType, TsFnOrConstructorType, TsFnParam, TsKeywordType, TsKeywordTypeKind, TsLit,
    TsLitType, TsType, TsTypeAnn, TsTypeLit, TsTypeParamDecl, TsTypeParamInstantiation, TsTypeRef,
    TsUnionOrIntersectionType, TsUnionType,
};

use super::error::{Span, TypeError};
use super::registry::TypeRegistry;
use super::{
    FunctionType, LifetimeId, LifetimeParam, ObjectType, Type, TypeVarId, fresh_type_var_id,
};
use std::collections::HashMap;

pub struct TypeConverter<'a> {
    registry: &'a TypeRegistry,
    type_vars: HashMap<String, TypeVarId>,
    lifetime_vars: HashMap<String, LifetimeId>,
}

impl<'a> TypeConverter<'a> {
    pub fn new(registry: &'a TypeRegistry) -> Self {
        Self {
            registry,
            type_vars: HashMap::new(),
            lifetime_vars: HashMap::new(),
        }
    }

    pub fn with_type_params(mut self, params: &[TypeVarId], names: &[String]) -> Self {
        for (name, id) in names.iter().zip(params.iter()) {
            self.type_vars.insert(name.clone(), *id);
        }
        self
    }

    pub fn with_lifetime_params(mut self, params: &[LifetimeParam]) -> Self {
        for param in params {
            self.lifetime_vars.insert(param.name.clone(), param.id);
        }
        self
    }

    fn resolve_lifetime(&self, name: &str) -> Result<LifetimeId, TypeError> {
        if name == "static" {
            return Ok(LifetimeId::STATIC);
        }
        self.lifetime_vars
            .get(name)
            .copied()
            .ok_or_else(|| TypeError::UndefinedLifetime {
                name: name.to_string(),
                span: Span::default(),
            })
    }

    pub fn convert_type_ann(&self, ann: &TsTypeAnn) -> Result<Type, TypeError> {
        self.convert(&ann.type_ann)
    }

    pub fn convert(&self, ts_type: &TsType) -> Result<Type, TypeError> {
        match ts_type {
            TsType::TsKeywordType(kw) => self.convert_keyword(kw),
            TsType::TsArrayType(arr) => self.convert_array(arr),
            TsType::TsTypeRef(ref_) => self.convert_type_ref(ref_),
            TsType::TsFnOrConstructorType(fn_type) => self.convert_fn_type(fn_type),
            TsType::TsTypeLit(lit) => self.convert_type_lit(lit),
            TsType::TsUnionOrIntersectionType(union) => {
                match union {
                    TsUnionOrIntersectionType::TsUnionType(u) => self.convert_union(u),
                    TsUnionOrIntersectionType::TsIntersectionType(_) => {
                        // For intersection types, just return the first type for now
                        Err(TypeError::UnsupportedType {
                            description: "intersection types not fully supported".to_string(),
                            span: Span::default(),
                        })
                    }
                }
            }
            _ => Err(TypeError::UnsupportedType {
                description: format!("{:?}", ts_type),
                span: Span::default(),
            }),
        }
    }

    fn convert_union(&self, union: &TsUnionType) -> Result<Type, TypeError> {
        let first_non_nullable = union.types.iter().find(|ty| {
            !matches!(
                ty.as_ref(),
                TsType::TsKeywordType(kw) if matches!(
                    kw.kind,
                    TsKeywordTypeKind::TsNullKeyword | TsKeywordTypeKind::TsUndefinedKeyword
                )
            )
        });

        match first_non_nullable {
            Some(ty) => self.convert(ty),
            None => Ok(Type::Void),
        }
    }

    fn convert_keyword(&self, kw: &TsKeywordType) -> Result<Type, TypeError> {
        match kw.kind {
            TsKeywordTypeKind::TsNumberKeyword => Ok(Type::Number),
            TsKeywordTypeKind::TsStringKeyword => Ok(Type::String),
            TsKeywordTypeKind::TsBooleanKeyword => Ok(Type::Boolean),
            TsKeywordTypeKind::TsVoidKeyword => Ok(Type::Void),
            TsKeywordTypeKind::TsNeverKeyword => Ok(Type::Never),
            TsKeywordTypeKind::TsAnyKeyword => Ok(Type::Any),
            TsKeywordTypeKind::TsUndefinedKeyword => Ok(Type::Void),
            TsKeywordTypeKind::TsNullKeyword => Ok(Type::Void),
            TsKeywordTypeKind::TsUnknownKeyword => Ok(Type::Any),
            TsKeywordTypeKind::TsObjectKeyword => Ok(Type::Object(ObjectType::default())),
            _ => Err(TypeError::UnsupportedType {
                description: format!("keyword type {:?}", kw.kind),
                span: Span::default(),
            }),
        }
    }

    fn convert_array(&self, arr: &TsArrayType) -> Result<Type, TypeError> {
        let elem = self.convert(&arr.elem_type)?;
        Ok(Type::Array(Box::new(elem)))
    }

    fn convert_type_ref(&self, ref_: &TsTypeRef) -> Result<Type, TypeError> {
        let name = match &ref_.type_name {
            swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
            swc_ecma_ast::TsEntityName::TsQualifiedName(qn) => qn.right.sym.to_string(),
        };

        match name.as_str() {
            "Ref" => {
                let inner = self.extract_single_type_arg(&ref_.type_params)?;
                return Ok(Type::Ref(Box::new(inner)));
            }
            "MutRef" => {
                let inner = self.extract_single_type_arg(&ref_.type_params)?;
                return Ok(Type::MutRef(Box::new(inner)));
            }
            "RefL" => {
                let (lifetime, inner) = self.extract_lifetime_and_type(&ref_.type_params)?;
                return Ok(Type::RefWithLifetime(lifetime, Box::new(inner)));
            }
            "MutRefL" => {
                let (lifetime, inner) = self.extract_lifetime_and_type(&ref_.type_params)?;
                return Ok(Type::MutRefWithLifetime(lifetime, Box::new(inner)));
            }
            "Array" => {
                let inner = self.extract_single_type_arg(&ref_.type_params)?;
                return Ok(Type::Array(Box::new(inner)));
            }
            _ => {}
        }

        if let Some(&var_id) = self.type_vars.get(&name) {
            return Ok(Type::TypeVar(var_id));
        }

        if let Some(type_id) = self.registry.lookup_by_name(&name) {
            if let Some(params) = &ref_.type_params {
                let args: Vec<Type> = params
                    .params
                    .iter()
                    .map(|p| self.convert(p))
                    .collect::<Result<_, _>>()?;
                return Ok(Type::Generic(type_id, args));
            }
            if self.registry.is_struct(type_id) {
                return Ok(Type::Struct(type_id));
            }
            if self.registry.is_enum(type_id) {
                return Ok(Type::Enum(type_id));
            }
            if self.registry.is_alias(type_id) {
                return Ok(Type::Alias(type_id));
            }
        }

        Err(TypeError::UndefinedType {
            name,
            span: Span::default(),
        })
    }

    fn extract_single_type_arg(
        &self,
        params: &Option<Box<TsTypeParamInstantiation>>,
    ) -> Result<Type, TypeError> {
        match params {
            Some(p) if p.params.len() == 1 => self.convert(&p.params[0]),
            Some(p) => Err(TypeError::TypeArgCountMismatch {
                expected: 1,
                got: p.params.len(),
                span: Span::default(),
            }),
            None => Err(TypeError::TypeArgCountMismatch {
                expected: 1,
                got: 0,
                span: Span::default(),
            }),
        }
    }

    fn extract_lifetime_and_type(
        &self,
        params: &Option<Box<TsTypeParamInstantiation>>,
    ) -> Result<(LifetimeId, Type), TypeError> {
        match params {
            Some(p) if p.params.len() == 2 => {
                let lifetime = self.extract_lifetime_from_type(&p.params[0])?;
                let inner = self.convert(&p.params[1])?;
                Ok((lifetime, inner))
            }
            Some(p) => Err(TypeError::TypeArgCountMismatch {
                expected: 2,
                got: p.params.len(),
                span: Span::default(),
            }),
            None => Err(TypeError::TypeArgCountMismatch {
                expected: 2,
                got: 0,
                span: Span::default(),
            }),
        }
    }

    fn extract_lifetime_from_type(&self, ts_type: &TsType) -> Result<LifetimeId, TypeError> {
        if let TsType::TsLitType(TsLitType {
            lit: TsLit::Str(s), ..
        }) = ts_type
        {
            let lifetime_name = String::from_utf8_lossy(s.value.as_bytes()).to_string();
            return self.resolve_lifetime(&lifetime_name);
        }
        Err(TypeError::UnsupportedType {
            description: "expected string literal for lifetime".to_string(),
            span: Span::default(),
        })
    }

    fn convert_fn_type(&self, fn_type: &TsFnOrConstructorType) -> Result<Type, TypeError> {
        match fn_type {
            TsFnOrConstructorType::TsFnType(fn_ty) => {
                let params = self.convert_fn_params(&fn_ty.params)?;
                let return_ty = self.convert(&fn_ty.type_ann.type_ann)?;

                let type_params = self.convert_type_params(&fn_ty.type_params)?;

                Ok(Type::Function(Box::new(FunctionType {
                    params,
                    return_ty,
                    lifetime_params: Vec::new(),
                    type_params,
                    is_method: false,
                })))
            }
            TsFnOrConstructorType::TsConstructorType(_) => Err(TypeError::UnsupportedType {
                description: "constructor types".to_string(),
                span: Span::default(),
            }),
        }
    }

    fn convert_fn_params(&self, params: &[TsFnParam]) -> Result<Vec<(String, Type)>, TypeError> {
        params
            .iter()
            .map(|p| {
                let name = match p {
                    TsFnParam::Ident(ident) => ident.id.sym.to_string(),
                    TsFnParam::Array(_) => "_".to_string(),
                    TsFnParam::Rest(_) => "rest".to_string(),
                    TsFnParam::Object(_) => "_".to_string(),
                };
                let ty = match p {
                    TsFnParam::Ident(ident) => ident
                        .type_ann
                        .as_ref()
                        .map(|ann| self.convert(&ann.type_ann))
                        .transpose()?
                        .unwrap_or(Type::Any),
                    _ => Type::Any,
                };
                Ok((name, ty))
            })
            .collect()
    }

    fn convert_type_params(
        &self,
        params: &Option<Box<TsTypeParamDecl>>,
    ) -> Result<Vec<TypeVarId>, TypeError> {
        match params {
            Some(decl) => decl
                .params
                .iter()
                .map(|p| {
                    if let Some(&id) = self.type_vars.get(&p.name.sym.to_string()) {
                        Ok(id)
                    } else {
                        Ok(fresh_type_var_id())
                    }
                })
                .collect(),
            None => Ok(Vec::new()),
        }
    }

    fn convert_type_lit(&self, lit: &TsTypeLit) -> Result<Type, TypeError> {
        let mut fields = std::collections::BTreeMap::new();

        for member in &lit.members {
            if let swc_ecma_ast::TsTypeElement::TsPropertySignature(prop) = member {
                let name = match &*prop.key {
                    Expr::Ident(ident) => ident.sym.to_string(),
                    _ => continue,
                };
                let ty = prop
                    .type_ann
                    .as_ref()
                    .map(|ann| self.convert(&ann.type_ann))
                    .transpose()?
                    .unwrap_or(Type::Any);
                fields.insert(name, ty);
            }
        }

        Ok(Type::Object(ObjectType {
            fields,
            exact: false,
        }))
    }
}

pub fn extract_type_param_names(params: &Option<Box<TsTypeParamDecl>>) -> Vec<String> {
    match params {
        Some(decl) => decl.params.iter().map(|p| p.name.sym.to_string()).collect(),
        None => Vec::new(),
    }
}

pub fn convert_optional_type_ann(
    ann: &Option<Box<TsTypeAnn>>,
    registry: &TypeRegistry,
) -> Result<Option<Type>, TypeError> {
    match ann {
        Some(a) => {
            let converter = TypeConverter::new(registry);
            Ok(Some(converter.convert_type_ann(a)?))
        }
        None => Ok(None),
    }
}

pub fn convert_type_ann_or_any(
    ann: &Option<Box<TsTypeAnn>>,
    registry: &TypeRegistry,
) -> Result<Type, TypeError> {
    convert_optional_type_ann(ann, registry).map(|t| t.unwrap_or(Type::Any))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keyword_type(kind: TsKeywordTypeKind) -> TsType {
        TsType::TsKeywordType(TsKeywordType {
            span: Default::default(),
            kind,
        })
    }

    #[test]
    fn test_convert_primitives() {
        let registry = TypeRegistry::new();
        let converter = TypeConverter::new(&registry);

        let number = make_keyword_type(TsKeywordTypeKind::TsNumberKeyword);
        assert_eq!(converter.convert(&number).unwrap(), Type::Number);

        let string = make_keyword_type(TsKeywordTypeKind::TsStringKeyword);
        assert_eq!(converter.convert(&string).unwrap(), Type::String);

        let boolean = make_keyword_type(TsKeywordTypeKind::TsBooleanKeyword);
        assert_eq!(converter.convert(&boolean).unwrap(), Type::Boolean);

        let void_ty = make_keyword_type(TsKeywordTypeKind::TsVoidKeyword);
        assert_eq!(converter.convert(&void_ty).unwrap(), Type::Void);

        let any = make_keyword_type(TsKeywordTypeKind::TsAnyKeyword);
        assert_eq!(converter.convert(&any).unwrap(), Type::Any);
    }

    #[test]
    fn test_convert_array() {
        let registry = TypeRegistry::new();
        let converter = TypeConverter::new(&registry);

        let arr = TsType::TsArrayType(TsArrayType {
            span: Default::default(),
            elem_type: Box::new(make_keyword_type(TsKeywordTypeKind::TsNumberKeyword)),
        });

        let result = converter.convert(&arr).unwrap();
        assert_eq!(result, Type::Array(Box::new(Type::Number)));
    }
}
