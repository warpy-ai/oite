//! SWC Type Conversion
//!
//! Converts SWC's TypeScript type AST (TsType) to tscl's Type representation.

use swc_ecma_ast::{
    Expr, TsArrayType, TsFnOrConstructorType, TsFnParam, TsKeywordType, TsKeywordTypeKind, TsType,
    TsTypeAnn, TsTypeLit, TsTypeParamDecl, TsTypeParamInstantiation, TsTypeRef,
    TsUnionOrIntersectionType, TsUnionType,
};

use super::error::{Span, TypeError};
use super::registry::TypeRegistry;
use super::{FunctionType, ObjectType, Type, TypeVarId, fresh_type_var_id};
use std::collections::HashMap;

/// Type converter context.
pub struct TypeConverter<'a> {
    /// Type registry for looking up named types.
    registry: &'a TypeRegistry,
    /// Type variable bindings (name -> TypeVarId).
    type_vars: HashMap<String, TypeVarId>,
}

impl<'a> TypeConverter<'a> {
    pub fn new(registry: &'a TypeRegistry) -> Self {
        Self {
            registry,
            type_vars: HashMap::new(),
        }
    }

    /// Add type variable bindings from type parameter declaration.
    pub fn with_type_params(mut self, params: &[TypeVarId], names: &[String]) -> Self {
        for (name, id) in names.iter().zip(params.iter()) {
            self.type_vars.insert(name.clone(), *id);
        }
        self
    }

    /// Convert SWC TsTypeAnn to tscl Type.
    pub fn convert_type_ann(&self, ann: &TsTypeAnn) -> Result<Type, TypeError> {
        self.convert(&ann.type_ann)
    }

    /// Convert SWC TsType to tscl Type.
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

    /// Convert union types: T | U | null, etc.
    /// For runtime, we simplify unions by taking the first non-null type.
    fn convert_union(&self, union: &TsUnionType) -> Result<Type, TypeError> {
        // Filter out null/undefined and take the first remaining type
        for ty in &union.types {
            if let TsType::TsKeywordType(kw) = ty.as_ref() {
                // Skip null and undefined
                if kw.kind == TsKeywordTypeKind::TsNullKeyword
                    || kw.kind == TsKeywordTypeKind::TsUndefinedKeyword
                {
                    continue;
                }
            }
            // Return the first non-null type
            return self.convert(ty);
        }
        // If all types were null/undefined, return void
        Ok(Type::Void)
    }

    /// Convert keyword types (number, string, boolean, etc.).
    fn convert_keyword(&self, kw: &TsKeywordType) -> Result<Type, TypeError> {
        match kw.kind {
            TsKeywordTypeKind::TsNumberKeyword => Ok(Type::Number),
            TsKeywordTypeKind::TsStringKeyword => Ok(Type::String),
            TsKeywordTypeKind::TsBooleanKeyword => Ok(Type::Boolean),
            TsKeywordTypeKind::TsVoidKeyword => Ok(Type::Void),
            TsKeywordTypeKind::TsNeverKeyword => Ok(Type::Never),
            TsKeywordTypeKind::TsAnyKeyword => Ok(Type::Any),
            TsKeywordTypeKind::TsUndefinedKeyword => Ok(Type::Void), // Treat undefined as void
            TsKeywordTypeKind::TsNullKeyword => Ok(Type::Void),      // Treat null as void for now
            TsKeywordTypeKind::TsUnknownKeyword => Ok(Type::Any),    // Treat unknown as any
            TsKeywordTypeKind::TsObjectKeyword => Ok(Type::Object(ObjectType::default())),
            _ => Err(TypeError::UnsupportedType {
                description: format!("keyword type {:?}", kw.kind),
                span: Span::default(),
            }),
        }
    }

    /// Convert array types: T[].
    fn convert_array(&self, arr: &TsArrayType) -> Result<Type, TypeError> {
        let elem = self.convert(&arr.elem_type)?;
        Ok(Type::Array(Box::new(elem)))
    }

    /// Convert type references (named types, generics, Ref<T>, MutRef<T>).
    fn convert_type_ref(&self, ref_: &TsTypeRef) -> Result<Type, TypeError> {
        // Get the type name
        let name = match &ref_.type_name {
            swc_ecma_ast::TsEntityName::Ident(ident) => ident.sym.to_string(),
            swc_ecma_ast::TsEntityName::TsQualifiedName(qn) => {
                // For qualified names like A.B, just use the last part for now
                qn.right.sym.to_string()
            }
        };

        // Handle special built-in types
        match name.as_str() {
            // Ref<T> -> Type::Ref(T)
            "Ref" => {
                let inner = self.extract_single_type_arg(&ref_.type_params)?;
                return Ok(Type::Ref(Box::new(inner)));
            }
            // MutRef<T> -> Type::MutRef(T)
            "MutRef" => {
                let inner = self.extract_single_type_arg(&ref_.type_params)?;
                return Ok(Type::MutRef(Box::new(inner)));
            }
            // Array<T> -> Type::Array(T)
            "Array" => {
                let inner = self.extract_single_type_arg(&ref_.type_params)?;
                return Ok(Type::Array(Box::new(inner)));
            }
            _ => {}
        }

        // Check if it's a type variable
        if let Some(&var_id) = self.type_vars.get(&name) {
            return Ok(Type::TypeVar(var_id));
        }

        // Look up in registry
        if let Some(type_id) = self.registry.lookup_by_name(&name) {
            // If there are type arguments, create Generic type
            if let Some(params) = &ref_.type_params {
                let args: Vec<Type> = params
                    .params
                    .iter()
                    .map(|p| self.convert(p))
                    .collect::<Result<_, _>>()?;
                return Ok(Type::Generic(type_id, args));
            }
            // Check what kind of type it is
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

        // Unknown type - return error or create a forward reference
        Err(TypeError::UndefinedType {
            name,
            span: Span::default(),
        })
    }

    /// Extract single type argument (for Ref<T>, etc.).
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

    /// Convert function types: (a: T, b: U) => R.
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

    /// Convert function parameters.
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

    /// Convert type parameter declarations to TypeVarIds.
    fn convert_type_params(
        &self,
        params: &Option<Box<TsTypeParamDecl>>,
    ) -> Result<Vec<TypeVarId>, TypeError> {
        match params {
            Some(decl) => {
                decl.params
                    .iter()
                    .map(|p| {
                        // If we already have this type var, return it
                        if let Some(&id) = self.type_vars.get(&p.name.sym.to_string()) {
                            Ok(id)
                        } else {
                            Ok(fresh_type_var_id())
                        }
                    })
                    .collect()
            }
            None => Ok(Vec::new()),
        }
    }

    /// Convert type literal (object type).
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

/// Extract type parameter names from a declaration.
pub fn extract_type_param_names(params: &Option<Box<TsTypeParamDecl>>) -> Vec<String> {
    match params {
        Some(decl) => decl.params.iter().map(|p| p.name.sym.to_string()).collect(),
        None => Vec::new(),
    }
}

/// Convert an optional type annotation, returning None if not present.
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

/// Convert a type annotation with default (Any if not present).
pub fn convert_type_ann_or_any(
    ann: &Option<Box<TsTypeAnn>>,
    registry: &TypeRegistry,
) -> Result<Type, TypeError> {
    convert_optional_type_ann(ann, registry).map(|t| t.unwrap_or(Type::Any))
}

// ============================================================================
// Tests
// ============================================================================

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
