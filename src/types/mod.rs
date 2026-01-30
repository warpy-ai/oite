//! Type system with ownership, borrowing, and lifetime tracking.

pub mod checker;
pub mod convert;
pub mod error;
pub mod inference;
pub mod lifetime_constraints;
pub mod registry;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

pub use lifetime_constraints::{ConstraintSet, LifetimeConstraint, ProgramPoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVarId(pub u32);

impl fmt::Display for TypeVarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "?{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InferId(pub u32);

impl fmt::Display for InferId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LifetimeId(pub u32);

impl LifetimeId {
    pub const STATIC: LifetimeId = LifetimeId(0);

    pub fn is_static(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for LifetimeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            write!(f, "'static")
        } else {
            write!(f, "'l{}", self.0)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LifetimeParam {
    pub id: LifetimeId,
    pub name: String,
    pub bounds: Vec<LifetimeId>,
}

impl LifetimeParam {
    pub fn new(id: LifetimeId, name: String) -> Self {
        Self {
            id,
            name,
            bounds: Vec::new(),
        }
    }

    pub fn with_bounds(mut self, bounds: Vec<LifetimeId>) -> Self {
        self.bounds = bounds;
        self
    }
}

impl fmt::Display for LifetimeParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}", self.name)?;
        if !self.bounds.is_empty() {
            write!(f, ": ")?;
            for (i, bound) in self.bounds.iter().enumerate() {
                if i > 0 {
                    write!(f, " + ")?;
                }
                write!(f, "{}", bound)?;
            }
        }
        Ok(())
    }
}

static NEXT_TYPE_ID: AtomicU32 = AtomicU32::new(0);
static NEXT_TYPE_VAR_ID: AtomicU32 = AtomicU32::new(0);
static NEXT_INFER_ID: AtomicU32 = AtomicU32::new(0);
static NEXT_LIFETIME_ID: AtomicU32 = AtomicU32::new(1); // 0 reserved for 'static

pub fn fresh_type_id() -> TypeId {
    TypeId(NEXT_TYPE_ID.fetch_add(1, Ordering::SeqCst))
}

pub fn fresh_type_var_id() -> TypeVarId {
    TypeVarId(NEXT_TYPE_VAR_ID.fetch_add(1, Ordering::SeqCst))
}

pub fn fresh_infer_id() -> InferId {
    InferId(NEXT_INFER_ID.fetch_add(1, Ordering::SeqCst))
}

pub fn fresh_lifetime_id() -> LifetimeId {
    LifetimeId(NEXT_LIFETIME_ID.fetch_add(1, Ordering::SeqCst))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Type {
    Number,
    Boolean,
    Void,
    Never,
    String,
    Array(Box<Type>),
    Object(ObjectType),
    Function(Box<FunctionType>),
    Struct(TypeId),
    Enum(TypeId),
    Alias(TypeId),
    TypeVar(TypeVarId),
    Generic(TypeId, Vec<Type>),
    Ref(Box<Type>),
    MutRef(Box<Type>),
    RefWithLifetime(LifetimeId, Box<Type>),
    MutRefWithLifetime(LifetimeId, Box<Type>),
    Lifetime(LifetimeId),
    #[default]
    Any,
    Infer(InferId),
    Error,
}

impl Type {
    pub fn is_copy(&self) -> bool {
        matches!(self, Type::Number | Type::Boolean)
    }

    pub fn is_move(&self) -> bool {
        matches!(
            self,
            Type::String
                | Type::Array(_)
                | Type::Object(_)
                | Type::Function(_)
                | Type::Struct(_)
                | Type::Enum(_)
        )
    }

    pub fn is_reference(&self) -> bool {
        matches!(
            self,
            Type::Ref(_)
                | Type::MutRef(_)
                | Type::RefWithLifetime(_, _)
                | Type::MutRefWithLifetime(_, _)
        )
    }

    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Type::Number | Type::Boolean | Type::Void | Type::Never
        )
    }

    pub fn is_heap(&self) -> bool {
        matches!(
            self,
            Type::String | Type::Array(_) | Type::Object(_) | Type::Function(_) | Type::Struct(_)
        )
    }

    pub fn is_concrete(&self) -> bool {
        match self {
            Type::TypeVar(_) | Type::Infer(_) => false,
            Type::Array(inner) => inner.is_concrete(),
            Type::Object(obj) => obj.fields.values().all(|t| t.is_concrete()),
            Type::Function(func) => {
                func.params.iter().all(|(_, t)| t.is_concrete()) && func.return_ty.is_concrete()
            }
            Type::Ref(inner) | Type::MutRef(inner) => inner.is_concrete(),
            Type::RefWithLifetime(_, inner) | Type::MutRefWithLifetime(_, inner) => {
                inner.is_concrete()
            }
            Type::Generic(_, args) => args.iter().all(|t| t.is_concrete()),
            Type::Lifetime(_) => true,
            _ => true,
        }
    }

    pub fn deref(&self) -> Option<&Type> {
        match self {
            Type::Ref(inner)
            | Type::MutRef(inner)
            | Type::RefWithLifetime(_, inner)
            | Type::MutRefWithLifetime(_, inner) => Some(inner),
            _ => None,
        }
    }

    pub fn lifetime(&self) -> Option<LifetimeId> {
        match self {
            Type::RefWithLifetime(lt, _) | Type::MutRefWithLifetime(lt, _) => Some(*lt),
            Type::Lifetime(lt) => Some(*lt),
            _ => None,
        }
    }

    pub fn is_mut_ref(&self) -> bool {
        matches!(self, Type::MutRef(_) | Type::MutRefWithLifetime(_, _))
    }

    pub fn element_type(&self) -> Option<&Type> {
        match self {
            Type::Array(inner) => Some(inner),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Number => write!(f, "number"),
            Type::Boolean => write!(f, "boolean"),
            Type::Void => write!(f, "void"),
            Type::Never => write!(f, "never"),
            Type::String => write!(f, "string"),
            Type::Array(inner) => write!(f, "{}[]", inner),
            Type::Object(obj) => {
                write!(f, "{{ ")?;
                for (i, (name, ty)) in obj.fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", name, ty)?;
                }
                write!(f, " }}")
            }
            Type::Function(func) => write!(f, "{}", func),
            Type::Struct(id) => write!(f, "struct#{}", id),
            Type::Enum(id) => write!(f, "enum#{}", id),
            Type::Alias(id) => write!(f, "alias#{}", id),
            Type::TypeVar(id) => write!(f, "{}", id),
            Type::Generic(id, args) => {
                write!(f, "{}#<", id)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ">")
            }
            Type::Ref(inner) => write!(f, "Ref<{}>", inner),
            Type::MutRef(inner) => write!(f, "MutRef<{}>", inner),
            Type::RefWithLifetime(lt, inner) => write!(f, "&{} {}", lt, inner),
            Type::MutRefWithLifetime(lt, inner) => write!(f, "&{} mut {}", lt, inner),
            Type::Lifetime(lt) => write!(f, "{}", lt),
            Type::Any => write!(f, "any"),
            Type::Infer(id) => write!(f, "{}", id),
            Type::Error => write!(f, "<error>"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionType {
    pub params: Vec<(String, Type)>,
    pub return_ty: Type,
    pub lifetime_params: Vec<LifetimeParam>,
    pub type_params: Vec<TypeVarId>,
    pub is_method: bool,
}

impl FunctionType {
    pub fn new(params: Vec<(String, Type)>, return_ty: Type) -> Self {
        Self {
            params,
            return_ty,
            lifetime_params: Vec::new(),
            type_params: Vec::new(),
            is_method: false,
        }
    }

    pub fn with_lifetime_params(mut self, lifetime_params: Vec<LifetimeParam>) -> Self {
        self.lifetime_params = lifetime_params;
        self
    }

    pub fn with_type_params(mut self, type_params: Vec<TypeVarId>) -> Self {
        self.type_params = type_params;
        self
    }

    pub fn as_method(mut self) -> Self {
        self.is_method = true;
        self
    }

    pub fn arity(&self) -> usize {
        self.params.len()
    }
}

impl fmt::Display for FunctionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.lifetime_params.is_empty() || !self.type_params.is_empty() {
            write!(f, "<")?;
            for (i, lt) in self.lifetime_params.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", lt)?;
            }
            for (i, tp) in self.type_params.iter().enumerate() {
                if i > 0 || !self.lifetime_params.is_empty() {
                    write!(f, ", ")?;
                }
                write!(f, "{}", tp)?;
            }
            write!(f, ">")?;
        }
        write!(f, "(")?;
        for (i, (name, ty)) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {}", name, ty)?;
        }
        write!(f, ") => {}", self.return_ty)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ObjectType {
    pub fields: BTreeMap<String, Type>,
    pub exact: bool,
}

impl ObjectType {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(mut self, name: String, ty: Type) -> Self {
        self.fields.insert(name, ty);
        self
    }

    pub fn exact(mut self) -> Self {
        self.exact = true;
        self
    }

    pub fn get_field(&self, name: &str) -> Option<&Type> {
        self.fields.get(name)
    }
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub id: TypeId,
    pub name: String,
    pub fields: Vec<(String, Type)>,
    pub type_params: Vec<TypeVarId>,
}

impl StructDef {
    pub fn new(id: TypeId, name: String) -> Self {
        Self {
            id,
            name,
            fields: Vec::new(),
            type_params: Vec::new(),
        }
    }

    pub fn with_field(mut self, name: String, ty: Type) -> Self {
        self.fields.push((name, ty));
        self
    }

    pub fn with_type_params(mut self, params: Vec<TypeVarId>) -> Self {
        self.type_params = params;
        self
    }

    pub fn get_field(&self, name: &str) -> Option<&Type> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|(n, _)| n == name)
    }
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub id: TypeId,
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub type_params: Vec<TypeVarId>,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub data: Option<Type>,
}

impl EnumDef {
    pub fn new(id: TypeId, name: String) -> Self {
        Self {
            id,
            name,
            variants: Vec::new(),
            type_params: Vec::new(),
        }
    }

    pub fn with_variant(mut self, name: String, data: Option<Type>) -> Self {
        self.variants.push(EnumVariant { name, data });
        self
    }
}

#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub id: TypeId,
    pub name: String,
    pub ty: Type,
    pub type_params: Vec<TypeVarId>,
}

impl TypeAlias {
    pub fn new(id: TypeId, name: String, ty: Type) -> Self {
        Self {
            id,
            name,
            ty,
            type_params: Vec::new(),
        }
    }

    pub fn with_type_params(mut self, params: Vec<TypeVarId>) -> Self {
        self.type_params = params;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Owned,
    Moved,
    Borrowed,
    BorrowedMut,
}

#[derive(Debug, Clone)]
pub struct VarType {
    pub ty: Type,
    pub ownership: Ownership,
    pub mutable: bool,
}

impl VarType {
    pub fn new(ty: Type) -> Self {
        Self {
            ty,
            ownership: Ownership::Owned,
            mutable: true,
        }
    }

    pub fn immutable(ty: Type) -> Self {
        Self {
            ty,
            ownership: Ownership::Owned,
            mutable: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TypeContext {
    pub variables: HashMap<String, VarType>,
    pub type_vars: HashMap<TypeVarId, Type>,
    pub infer_vars: HashMap<InferId, Type>,
    parent: Option<Box<TypeContext>>,
}

impl TypeContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn child(&self) -> Self {
        Self {
            variables: HashMap::new(),
            type_vars: HashMap::new(),
            infer_vars: HashMap::new(),
            parent: Some(Box::new(self.clone())),
        }
    }

    pub fn define(&mut self, name: String, var: VarType) {
        self.variables.insert(name, var);
    }

    pub fn lookup(&self, name: &str) -> Option<&VarType> {
        self.variables
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|parent| parent.lookup(name)))
    }

    pub fn lookup_mut(&mut self, name: &str) -> Option<&mut VarType> {
        if self.variables.contains_key(name) {
            self.variables.get_mut(name)
        } else {
            self.parent
                .as_mut()
                .and_then(|parent| parent.lookup_mut(name))
        }
    }

    pub fn bind_type_var(&mut self, var: TypeVarId, ty: Type) {
        self.type_vars.insert(var, ty);
    }

    pub fn resolve_type_var(&self, var: TypeVarId) -> Option<&Type> {
        self.type_vars.get(&var).or_else(|| {
            self.parent
                .as_ref()
                .and_then(|parent| parent.resolve_type_var(var))
        })
    }

    pub fn bind_infer(&mut self, var: InferId, ty: Type) {
        self.infer_vars.insert(var, ty);
    }

    pub fn resolve_infer(&self, var: InferId) -> Option<&Type> {
        self.infer_vars.get(&var).or_else(|| {
            self.parent
                .as_ref()
                .and_then(|parent| parent.resolve_infer(var))
        })
    }

    pub fn substitute(&self, ty: &Type) -> Type {
        match ty {
            Type::TypeVar(var) => self
                .resolve_type_var(*var)
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            Type::Infer(var) => self
                .resolve_infer(*var)
                .map(|t| self.substitute(t))
                .unwrap_or_else(|| ty.clone()),
            Type::Array(inner) => Type::Array(Box::new(self.substitute(inner))),
            Type::Ref(inner) => Type::Ref(Box::new(self.substitute(inner))),
            Type::MutRef(inner) => Type::MutRef(Box::new(self.substitute(inner))),
            Type::RefWithLifetime(lt, inner) => {
                Type::RefWithLifetime(*lt, Box::new(self.substitute(inner)))
            }
            Type::MutRefWithLifetime(lt, inner) => {
                Type::MutRefWithLifetime(*lt, Box::new(self.substitute(inner)))
            }
            Type::Object(obj) => Type::Object(ObjectType {
                fields: obj
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.substitute(v)))
                    .collect(),
                exact: obj.exact,
            }),
            Type::Function(func) => Type::Function(Box::new(FunctionType {
                params: func
                    .params
                    .iter()
                    .map(|(n, t)| (n.clone(), self.substitute(t)))
                    .collect(),
                return_ty: self.substitute(&func.return_ty),
                lifetime_params: func.lifetime_params.clone(),
                type_params: func.type_params.clone(),
                is_method: func.is_method,
            })),
            Type::Generic(id, args) => {
                Type::Generic(*id, args.iter().map(|t| self.substitute(t)).collect())
            }
            _ => ty.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_display() {
        assert_eq!(format!("{}", Type::Number), "number");
        assert_eq!(format!("{}", Type::String), "string");
        assert_eq!(
            format!("{}", Type::Array(Box::new(Type::Number))),
            "number[]"
        );
        assert_eq!(
            format!("{}", Type::Ref(Box::new(Type::String))),
            "Ref<string>"
        );
    }

    #[test]
    fn test_type_properties() {
        assert!(Type::Number.is_copy());
        assert!(Type::Boolean.is_copy());
        assert!(!Type::String.is_copy());

        assert!(Type::String.is_move());
        assert!(Type::Array(Box::new(Type::Number)).is_move());
        assert!(!Type::Number.is_move());

        assert!(Type::Ref(Box::new(Type::String)).is_reference());
        assert!(Type::MutRef(Box::new(Type::String)).is_reference());
        assert!(!Type::String.is_reference());
    }

    #[test]
    fn test_function_type() {
        let func = FunctionType::new(
            vec![
                ("a".to_string(), Type::Number),
                ("b".to_string(), Type::Number),
            ],
            Type::Number,
        );
        assert_eq!(func.arity(), 2);
        assert_eq!(format!("{}", func), "(a: number, b: number) => number");
        assert!(func.lifetime_params.is_empty());
    }

    #[test]
    fn test_function_type_with_lifetimes() {
        let lt = fresh_lifetime_id();
        let lt_param = LifetimeParam::new(lt, "a".to_string());

        let func = FunctionType::new(
            vec![(
                "x".to_string(),
                Type::RefWithLifetime(lt, Box::new(Type::Number)),
            )],
            Type::RefWithLifetime(lt, Box::new(Type::Number)),
        )
        .with_lifetime_params(vec![lt_param]);

        assert_eq!(func.lifetime_params.len(), 1);
        assert_eq!(func.lifetime_params[0].name, "a");

        // Display should show lifetime params
        let display = format!("{}", func);
        assert!(display.starts_with("<'a>"));
        assert!(display.contains("&'l"));
    }

    #[test]
    fn test_type_context() {
        let mut ctx = TypeContext::new();
        ctx.define("x".to_string(), VarType::new(Type::Number));

        assert!(ctx.lookup("x").is_some());
        assert_eq!(ctx.lookup("x").unwrap().ty, Type::Number);

        let child = ctx.child();
        assert!(child.lookup("x").is_some());
    }

    #[test]
    fn test_substitution() {
        let mut ctx = TypeContext::new();
        let var = fresh_type_var_id();
        ctx.bind_type_var(var, Type::Number);

        let ty = Type::Array(Box::new(Type::TypeVar(var)));
        let substituted = ctx.substitute(&ty);
        assert_eq!(substituted, Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn test_lifetime_id() {
        // Test 'static lifetime
        assert_eq!(LifetimeId::STATIC.0, 0);
        assert!(LifetimeId::STATIC.is_static());
        assert_eq!(format!("{}", LifetimeId::STATIC), "'static");

        // Test fresh lifetime IDs
        let lt1 = fresh_lifetime_id();
        let lt2 = fresh_lifetime_id();
        assert!(!lt1.is_static());
        assert!(!lt2.is_static());
        assert_ne!(lt1, lt2); // Fresh IDs should be unique
        assert!(lt1.0 > 0); // Fresh IDs start at 1
        assert!(lt2.0 > 0);
    }

    #[test]
    fn test_lifetime_param() {
        let lt = fresh_lifetime_id();
        let param = LifetimeParam::new(lt, "a".to_string());
        assert_eq!(param.name, "a");
        assert_eq!(param.id, lt);
        assert!(param.bounds.is_empty());
        assert_eq!(format!("{}", param), "'a");

        // Test with bounds
        let lt2 = fresh_lifetime_id();
        let param_with_bounds = LifetimeParam::new(lt, "a".to_string()).with_bounds(vec![lt2]);
        assert_eq!(param_with_bounds.bounds.len(), 1);
    }

    #[test]
    fn test_lifetime_display() {
        // Static lifetime
        assert_eq!(format!("{}", LifetimeId::STATIC), "'static");

        // Non-static lifetimes show their ID
        let lt = LifetimeId(5);
        assert_eq!(format!("{}", lt), "'l5");

        // Lifetime param with bounds
        let param = LifetimeParam {
            id: LifetimeId(1),
            name: "a".to_string(),
            bounds: vec![LifetimeId::STATIC],
        };
        assert_eq!(format!("{}", param), "'a: 'static");
    }

    #[test]
    fn test_ref_with_lifetime() {
        let lt = fresh_lifetime_id();

        // RefWithLifetime
        let ref_ty = Type::RefWithLifetime(lt, Box::new(Type::Number));
        assert!(ref_ty.is_reference());
        assert!(!ref_ty.is_mut_ref());
        assert_eq!(ref_ty.deref(), Some(&Type::Number));
        assert_eq!(ref_ty.lifetime(), Some(lt));
        assert!(ref_ty.is_concrete());

        // MutRefWithLifetime
        let mut_ref_ty = Type::MutRefWithLifetime(lt, Box::new(Type::String));
        assert!(mut_ref_ty.is_reference());
        assert!(mut_ref_ty.is_mut_ref());
        assert_eq!(mut_ref_ty.deref(), Some(&Type::String));
        assert_eq!(mut_ref_ty.lifetime(), Some(lt));

        // Lifetime type
        let lt_ty = Type::Lifetime(LifetimeId::STATIC);
        assert_eq!(lt_ty.lifetime(), Some(LifetimeId::STATIC));
        assert!(lt_ty.is_concrete());
    }

    #[test]
    fn test_ref_with_lifetime_display() {
        let lt = LifetimeId(1);

        let ref_ty = Type::RefWithLifetime(lt, Box::new(Type::Number));
        assert_eq!(format!("{}", ref_ty), "&'l1 number");

        let mut_ref_ty = Type::MutRefWithLifetime(lt, Box::new(Type::String));
        assert_eq!(format!("{}", mut_ref_ty), "&'l1 mut string");

        let static_ref = Type::RefWithLifetime(LifetimeId::STATIC, Box::new(Type::String));
        assert_eq!(format!("{}", static_ref), "&'static string");
    }

    #[test]
    fn test_ref_with_lifetime_substitute() {
        let mut ctx = TypeContext::new();
        let var = fresh_type_var_id();
        ctx.bind_type_var(var, Type::Number);

        let lt = fresh_lifetime_id();
        let ty = Type::RefWithLifetime(lt, Box::new(Type::TypeVar(var)));
        let substituted = ctx.substitute(&ty);

        // Should substitute inner type but preserve lifetime
        match substituted {
            Type::RefWithLifetime(sub_lt, inner) => {
                assert_eq!(sub_lt, lt);
                assert_eq!(*inner, Type::Number);
            }
            _ => panic!("Expected RefWithLifetime"),
        }
    }
}
