//! Type System for tscl
//!
//! This module provides a statically-typed language core with:
//! - TypeScript-style type annotations
//! - Rust-style ownership and borrowing semantics
//! - Hindley-Milner type inference
//! - Generics with monomorphization
//!
//! Type annotations use familiar syntax:
//! ```typescript
//! let x: number = 42;
//! function add(a: number, b: number): number { return a + b; }
//! ```
//!
//! Borrowing uses wrapper types (parsed by SWC):
//! ```typescript
//! function read(buf: Ref<Buffer>): number { ... }      // &Buffer
//! function write(buf: MutRef<Buffer>): void { ... }    // &mut Buffer
//! ```

pub mod checker;
pub mod convert;
pub mod error;
pub mod inference;
pub mod registry;

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// Type Identifiers
// ============================================================================

/// Unique identifier for user-defined types (structs, enums, aliases).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

impl fmt::Display for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

/// Unique identifier for type variables (generics, inference placeholders).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVarId(pub u32);

impl fmt::Display for TypeVarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "?{}", self.0)
    }
}

/// Unique identifier for inference variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InferId(pub u32);

impl fmt::Display for InferId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_{}", self.0)
    }
}

/// Unique identifier for lifetime parameters ('a, 'b, 'static).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LifetimeId(pub u32);

impl LifetimeId {
    /// The 'static lifetime - lives for the entire program duration.
    /// Reserved as ID 0.
    pub const STATIC: LifetimeId = LifetimeId(0);

    /// Check if this is the 'static lifetime.
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

/// A lifetime parameter with a name binding.
/// Used in function signatures like `fn find<'a>(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifetimeParam {
    /// Unique identifier for this lifetime.
    pub id: LifetimeId,
    /// The name as written in source (e.g., "a" for 'a).
    pub name: String,
    /// Bounds: lifetimes this must outlive (for 'a: 'b syntax).
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

/// Global counter for generating unique type IDs.
static NEXT_TYPE_ID: AtomicU32 = AtomicU32::new(0);
static NEXT_TYPE_VAR_ID: AtomicU32 = AtomicU32::new(0);
static NEXT_INFER_ID: AtomicU32 = AtomicU32::new(0);
/// Starts at 1 because 0 is reserved for 'static.
static NEXT_LIFETIME_ID: AtomicU32 = AtomicU32::new(1);

pub fn fresh_type_id() -> TypeId {
    TypeId(NEXT_TYPE_ID.fetch_add(1, Ordering::SeqCst))
}

pub fn fresh_type_var_id() -> TypeVarId {
    TypeVarId(NEXT_TYPE_VAR_ID.fetch_add(1, Ordering::SeqCst))
}

pub fn fresh_infer_id() -> InferId {
    InferId(NEXT_INFER_ID.fetch_add(1, Ordering::SeqCst))
}

/// Generate a fresh lifetime ID. ID 0 is reserved for 'static.
pub fn fresh_lifetime_id() -> LifetimeId {
    LifetimeId(NEXT_LIFETIME_ID.fetch_add(1, Ordering::SeqCst))
}

// ============================================================================
// Core Type Representation
// ============================================================================

/// The core type enum representing all types in tscl.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Type {
    // === Primitives (Copy semantics) ===
    /// IEEE 754 double-precision float (like JavaScript's number).
    Number,
    /// Boolean true/false.
    Boolean,
    /// No value (function returns nothing).
    Void,
    /// Unreachable / bottom type (never returns).
    Never,

    // === Heap types (Move semantics) ===
    /// UTF-8 string (heap-allocated).
    String,
    /// Homogeneous array: T[].
    Array(Box<Type>),
    /// Object type with named fields: { a: T, b: U }.
    Object(ObjectType),
    /// Function type: (params) => return.
    Function(Box<FunctionType>),

    // === User-defined types ===
    /// Named struct type.
    Struct(TypeId),
    /// Named enum type.
    Enum(TypeId),
    /// Type alias (resolved during checking).
    Alias(TypeId),

    // === Generics ===
    /// Unresolved type variable (e.g., T in `function id<T>(x: T): T`).
    TypeVar(TypeVarId),
    /// Applied generic type (e.g., Array<number>).
    Generic(TypeId, Vec<Type>),

    // === References (Borrow semantics) ===
    /// Immutable borrow: Ref<T> compiles to &T semantics.
    Ref(Box<Type>),
    /// Mutable borrow: MutRef<T> compiles to &mut T semantics.
    MutRef(Box<Type>),

    // === Special ===
    /// Dynamic type (escape hatch, disables optimizations).
    #[default]
    Any,
    /// Inference placeholder (resolved during type checking).
    Infer(InferId),
    /// Error type (for error recovery).
    Error,
}

impl Type {
    /// Check if this type has Copy semantics (no ownership transfer).
    pub fn is_copy(&self) -> bool {
        matches!(self, Type::Number | Type::Boolean)
    }

    /// Check if this type has Move semantics (ownership transfer on assignment).
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

    /// Check if this type is a reference (borrow).
    pub fn is_reference(&self) -> bool {
        matches!(self, Type::Ref(_) | Type::MutRef(_))
    }

    /// Check if this type is a primitive.
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Type::Number | Type::Boolean | Type::Void | Type::Never
        )
    }

    /// Check if this type needs heap allocation.
    pub fn is_heap(&self) -> bool {
        matches!(
            self,
            Type::String | Type::Array(_) | Type::Object(_) | Type::Function(_) | Type::Struct(_)
        )
    }

    /// Check if this type is concrete (no unresolved variables).
    pub fn is_concrete(&self) -> bool {
        match self {
            Type::TypeVar(_) | Type::Infer(_) => false,
            Type::Array(inner) => inner.is_concrete(),
            Type::Object(obj) => obj.fields.values().all(|t| t.is_concrete()),
            Type::Function(func) => {
                func.params.iter().all(|(_, t)| t.is_concrete()) && func.return_ty.is_concrete()
            }
            Type::Ref(inner) | Type::MutRef(inner) => inner.is_concrete(),
            Type::Generic(_, args) => args.iter().all(|t| t.is_concrete()),
            _ => true,
        }
    }

    /// Get the inner type for reference types.
    pub fn deref(&self) -> Option<&Type> {
        match self {
            Type::Ref(inner) | Type::MutRef(inner) => Some(inner),
            _ => None,
        }
    }

    /// Get element type for array types.
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
            Type::Any => write!(f, "any"),
            Type::Infer(id) => write!(f, "{}", id),
            Type::Error => write!(f, "<error>"),
        }
    }
}

// ============================================================================
// Function Type
// ============================================================================

/// Function type: (params) => return.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionType {
    /// Parameter names and types.
    pub params: Vec<(String, Type)>,
    /// Return type.
    pub return_ty: Type,
    /// Generic type parameters (e.g., T, U in `<T, U>`).
    pub type_params: Vec<TypeVarId>,
    /// Whether this function is a method (has implicit `this`).
    pub is_method: bool,
}

impl FunctionType {
    pub fn new(params: Vec<(String, Type)>, return_ty: Type) -> Self {
        Self {
            params,
            return_ty,
            type_params: Vec::new(),
            is_method: false,
        }
    }

    pub fn with_type_params(mut self, type_params: Vec<TypeVarId>) -> Self {
        self.type_params = type_params;
        self
    }

    pub fn as_method(mut self) -> Self {
        self.is_method = true;
        self
    }

    /// Get the arity (number of parameters).
    pub fn arity(&self) -> usize {
        self.params.len()
    }
}

impl fmt::Display for FunctionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

// ============================================================================
// Object Type
// ============================================================================

/// Object type with named fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ObjectType {
    /// Field name -> type mapping (BTreeMap for Hash/Eq).
    pub fields: BTreeMap<String, Type>,
    /// Whether this is an exact type (no extra fields allowed).
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

    /// Get the type of a field.
    pub fn get_field(&self, name: &str) -> Option<&Type> {
        self.fields.get(name)
    }
}

// ============================================================================
// Struct Definition
// ============================================================================

/// A named struct type definition.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub id: TypeId,
    pub name: String,
    /// Fields in declaration order.
    pub fields: Vec<(String, Type)>,
    /// Generic type parameters.
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

    /// Get the type of a field by name.
    pub fn get_field(&self, name: &str) -> Option<&Type> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    /// Get field index by name.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|(n, _)| n == name)
    }
}

// ============================================================================
// Enum Definition
// ============================================================================

/// A named enum type definition.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub id: TypeId,
    pub name: String,
    /// Variants with optional associated data.
    pub variants: Vec<EnumVariant>,
    /// Generic type parameters.
    pub type_params: Vec<TypeVarId>,
}

/// A single enum variant.
#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    /// Associated data (if any).
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

// ============================================================================
// Type Alias
// ============================================================================

/// A type alias definition.
#[derive(Debug, Clone)]
pub struct TypeAlias {
    pub id: TypeId,
    pub name: String,
    /// The aliased type.
    pub ty: Type,
    /// Generic type parameters.
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

// ============================================================================
// Ownership Tracking
// ============================================================================

/// Ownership state for type checking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    /// Value is owned.
    Owned,
    /// Value has been moved.
    Moved,
    /// Value is borrowed immutably.
    Borrowed,
    /// Value is borrowed mutably.
    BorrowedMut,
}

/// Variable information for type checking.
#[derive(Debug, Clone)]
pub struct VarType {
    /// The type of the variable.
    pub ty: Type,
    /// Current ownership state.
    pub ownership: Ownership,
    /// Whether the variable is mutable.
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

// ============================================================================
// Type Context
// ============================================================================

/// Type checking context with variable bindings.
#[derive(Debug, Clone, Default)]
pub struct TypeContext {
    /// Variable name -> type info.
    pub variables: HashMap<String, VarType>,
    /// Type variable bindings (for generics).
    pub type_vars: HashMap<TypeVarId, Type>,
    /// Inference variable solutions.
    pub infer_vars: HashMap<InferId, Type>,
    /// Parent context (for scopes).
    parent: Option<Box<TypeContext>>,
}

impl TypeContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a child scope.
    pub fn child(&self) -> Self {
        Self {
            variables: HashMap::new(),
            type_vars: HashMap::new(),
            infer_vars: HashMap::new(),
            parent: Some(Box::new(self.clone())),
        }
    }

    /// Define a variable in this scope.
    pub fn define(&mut self, name: String, var: VarType) {
        self.variables.insert(name, var);
    }

    /// Look up a variable (searches parent scopes).
    pub fn lookup(&self, name: &str) -> Option<&VarType> {
        self.variables
            .get(name)
            .or_else(|| self.parent.as_ref().and_then(|parent| parent.lookup(name)))
    }

    /// Look up a variable mutably.
    pub fn lookup_mut(&mut self, name: &str) -> Option<&mut VarType> {
        if self.variables.contains_key(name) {
            self.variables.get_mut(name)
        } else {
            self.parent
                .as_mut()
                .and_then(|parent| parent.lookup_mut(name))
        }
    }

    /// Bind a type variable to a concrete type.
    pub fn bind_type_var(&mut self, var: TypeVarId, ty: Type) {
        self.type_vars.insert(var, ty);
    }

    /// Resolve a type variable.
    pub fn resolve_type_var(&self, var: TypeVarId) -> Option<&Type> {
        self.type_vars.get(&var).or_else(|| {
            self.parent
                .as_ref()
                .and_then(|parent| parent.resolve_type_var(var))
        })
    }

    /// Bind an inference variable.
    pub fn bind_infer(&mut self, var: InferId, ty: Type) {
        self.infer_vars.insert(var, ty);
    }

    /// Resolve an inference variable.
    pub fn resolve_infer(&self, var: InferId) -> Option<&Type> {
        self.infer_vars.get(&var).or_else(|| {
            self.parent
                .as_ref()
                .and_then(|parent| parent.resolve_infer(var))
        })
    }

    /// Substitute all type variables and inference variables in a type.
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

// ============================================================================
// Tests
// ============================================================================

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
}
