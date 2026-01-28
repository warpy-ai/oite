//! Type Inference Engine
//!
//! Implements Hindley-Milner type inference with:
//! - Constraint generation from expressions
//! - Unification algorithm for solving constraints
//! - Flow-sensitive type narrowing

use std::collections::HashMap;

use super::error::{Span, TypeError, TypeErrors};
use super::registry::TypeRegistry;
use super::{FunctionType, InferId, ObjectType, Type, TypeContext, fresh_infer_id};

// ============================================================================
// Constraints
// ============================================================================

/// Type constraints generated during inference.
#[derive(Debug, Clone)]
pub enum Constraint {
    /// T1 = T2 (types must be equal).
    Equal(Type, Type, Span),

    /// T1 <: T2 (T1 is subtype of T2).
    Subtype(Type, Type, Span),

    /// T has field .name of type U.
    HasField(Type, String, Type, Span),

    /// T is callable with args and returns U.
    Callable(Type, Vec<Type>, Type, Span),

    /// T is indexable with index type I and element type E.
    Indexable(Type, Type, Type, Span),
}

impl Constraint {
    pub fn span(&self) -> Span {
        match self {
            Constraint::Equal(_, _, s) => *s,
            Constraint::Subtype(_, _, s) => *s,
            Constraint::HasField(_, _, _, s) => *s,
            Constraint::Callable(_, _, _, s) => *s,
            Constraint::Indexable(_, _, _, s) => *s,
        }
    }
}

// ============================================================================
// Type Inference Engine
// ============================================================================

/// The type inference engine.
pub struct InferenceEngine<'a> {
    /// Type registry for looking up named types.
    registry: &'a TypeRegistry,
    /// Current type context.
    context: TypeContext,
    /// Generated constraints.
    constraints: Vec<Constraint>,
    /// Errors encountered during inference.
    errors: TypeErrors,
    /// Substitution map: InferId -> Type.
    substitutions: HashMap<InferId, Type>,
}

impl<'a> InferenceEngine<'a> {
    pub fn new(registry: &'a TypeRegistry) -> Self {
        Self {
            registry,
            context: TypeContext::new(),
            constraints: Vec::new(),
            errors: TypeErrors::new(),
            substitutions: HashMap::new(),
        }
    }

    /// Create a fresh inference variable.
    pub fn fresh_var(&mut self) -> Type {
        Type::Infer(fresh_infer_id())
    }

    /// Add a constraint.
    pub fn add_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }

    /// Add an equality constraint.
    pub fn constrain_equal(&mut self, t1: Type, t2: Type, span: Span) {
        self.constraints.push(Constraint::Equal(t1, t2, span));
    }

    /// Add a subtype constraint.
    pub fn constrain_subtype(&mut self, sub: Type, sup: Type, span: Span) {
        self.constraints.push(Constraint::Subtype(sub, sup, span));
    }

    /// Add a field constraint.
    pub fn constrain_has_field(&mut self, ty: Type, field: String, field_ty: Type, span: Span) {
        self.constraints
            .push(Constraint::HasField(ty, field, field_ty, span));
    }

    /// Add a callable constraint.
    pub fn constrain_callable(&mut self, ty: Type, args: Vec<Type>, ret: Type, span: Span) {
        self.constraints
            .push(Constraint::Callable(ty, args, ret, span));
    }

    /// Get the current context.
    pub fn context(&self) -> &TypeContext {
        &self.context
    }

    /// Get mutable context.
    pub fn context_mut(&mut self) -> &mut TypeContext {
        &mut self.context
    }

    /// Enter a new scope.
    pub fn enter_scope(&mut self) {
        self.context = self.context.child();
    }

    /// Exit current scope (returns to parent).
    pub fn exit_scope(&mut self) {
        if let Some(parent) = self.context.parent.take() {
            self.context = *parent;
        }
    }

    // ========================================================================
    // Unification
    // ========================================================================

    /// Solve all constraints using unification.
    pub fn solve(&mut self) -> Result<(), TypeErrors> {
        // Process constraints until fixed point
        let mut changed = true;
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 1000;

        while changed && iterations < MAX_ITERATIONS {
            changed = false;
            iterations += 1;

            let constraints = std::mem::take(&mut self.constraints);
            for constraint in constraints {
                match self.solve_constraint(constraint) {
                    Ok(true) => changed = true,
                    Ok(false) => {}
                    Err(e) => self.errors.push(e),
                }
            }
        }

        if self.errors.has_errors() {
            Err(std::mem::take(&mut self.errors))
        } else {
            Ok(())
        }
    }

    /// Solve a single constraint, returning true if progress was made.
    fn solve_constraint(&mut self, constraint: Constraint) -> Result<bool, TypeError> {
        match constraint {
            Constraint::Equal(t1, t2, span) => self.unify(&t1, &t2, span),
            Constraint::Subtype(sub, sup, span) => self.check_subtype(&sub, &sup, span),
            Constraint::HasField(ty, field, field_ty, span) => {
                self.check_has_field(&ty, &field, &field_ty, span)
            }
            Constraint::Callable(ty, args, ret, span) => {
                self.check_callable(&ty, &args, &ret, span)
            }
            Constraint::Indexable(ty, idx, elem, span) => {
                self.check_indexable(&ty, &idx, &elem, span)
            }
        }
    }

    /// Unify two types, making them equal.
    fn unify(&mut self, t1: &Type, t2: &Type, span: Span) -> Result<bool, TypeError> {
        // Apply current substitutions
        let t1 = self.apply_substitutions(t1);
        let t2 = self.apply_substitutions(t2);

        // If equal, nothing to do
        if t1 == t2 {
            return Ok(false);
        }

        match (&t1, &t2) {
            // Inference variables can be unified with anything
            (Type::Infer(id), ty) | (ty, Type::Infer(id)) => {
                // Occurs check: prevent infinite types
                if self.occurs_in(*id, ty) {
                    return Err(TypeError::RecursiveType {
                        name: format!("{}", id),
                        span,
                    });
                }
                self.substitutions.insert(*id, ty.clone());
                Ok(true)
            }

            // Any unifies with anything (escape hatch)
            (Type::Any, _) | (_, Type::Any) => Ok(false),

            // Error propagates
            (Type::Error, _) | (_, Type::Error) => Ok(false),

            // Arrays must have same element type
            (Type::Array(e1), Type::Array(e2)) => self.unify(e1, e2, span),

            // Refs must have same inner type
            (Type::Ref(t1), Type::Ref(t2)) => self.unify(t1, t2, span),
            (Type::MutRef(t1), Type::MutRef(t2)) => self.unify(t1, t2, span),

            // Functions must have same param and return types
            (Type::Function(f1), Type::Function(f2)) => {
                if f1.params.len() != f2.params.len() {
                    return Err(TypeError::WrongArgCount {
                        expected: f1.params.len(),
                        got: f2.params.len(),
                        span,
                    });
                }
                let mut changed = false;
                for ((_, t1), (_, t2)) in f1.params.iter().zip(f2.params.iter()) {
                    changed |= self.unify(t1, t2, span)?;
                }
                changed |= self.unify(&f1.return_ty, &f2.return_ty, span)?;
                Ok(changed)
            }

            // Objects: structural equality
            (Type::Object(o1), Type::Object(o2)) => {
                let mut changed = false;
                // All fields in o1 must exist in o2 with same type
                for (name, ty1) in &o1.fields {
                    if let Some(ty2) = o2.fields.get(name) {
                        changed |= self.unify(ty1, ty2, span)?;
                    } else if o2.exact {
                        return Err(TypeError::FieldNotFound {
                            ty: t2.clone(),
                            field: name.clone(),
                            span,
                        });
                    }
                }
                // If o1 is exact, o2 can't have extra fields
                if o1.exact {
                    for name in o2.fields.keys() {
                        if !o1.fields.contains_key(name) {
                            return Err(TypeError::FieldNotFound {
                                ty: t1.clone(),
                                field: name.clone(),
                                span,
                            });
                        }
                    }
                }
                Ok(changed)
            }

            // Generics: check base type and all args
            (Type::Generic(id1, args1), Type::Generic(id2, args2)) if id1 == id2 => {
                if args1.len() != args2.len() {
                    return Err(TypeError::TypeArgCountMismatch {
                        expected: args1.len(),
                        got: args2.len(),
                        span,
                    });
                }
                let mut changed = false;
                for (a1, a2) in args1.iter().zip(args2.iter()) {
                    changed |= self.unify(a1, a2, span)?;
                }
                Ok(changed)
            }

            // Type variables: defer to context
            (Type::TypeVar(v1), Type::TypeVar(v2)) if v1 == v2 => Ok(false),

            // Primitives must be identical
            _ => Err(TypeError::Mismatch {
                expected: t2.clone(),
                got: t1.clone(),
                span,
            }),
        }
    }

    /// Check subtype relationship.
    fn check_subtype(&mut self, sub: &Type, sup: &Type, span: Span) -> Result<bool, TypeError> {
        // For now, use equality. Could be extended for structural subtyping.
        self.unify(sub, sup, span)
    }

    /// Check that a type has a field.
    fn check_has_field(
        &mut self,
        ty: &Type,
        field: &str,
        field_ty: &Type,
        span: Span,
    ) -> Result<bool, TypeError> {
        let ty = self.apply_substitutions(ty);

        match &ty {
            Type::Infer(_) => {
                // Defer constraint
                self.constraints.push(Constraint::HasField(
                    ty,
                    field.to_string(),
                    field_ty.clone(),
                    span,
                ));
                Ok(false)
            }
            Type::Object(obj) => {
                if let Some(actual_ty) = obj.fields.get(field) {
                    self.unify(actual_ty, field_ty, span)
                } else {
                    Err(TypeError::FieldNotFound {
                        ty: ty.clone(),
                        field: field.to_string(),
                        span,
                    })
                }
            }
            Type::Struct(id) => {
                if let Some(def) = self.registry.get_struct(*id) {
                    if let Some(actual_ty) = def.get_field(field) {
                        self.unify(actual_ty, field_ty, span)
                    } else {
                        Err(TypeError::FieldNotFound {
                            ty: ty.clone(),
                            field: field.to_string(),
                            span,
                        })
                    }
                } else {
                    Ok(false)
                }
            }
            Type::Any => Ok(false),
            _ => Err(TypeError::FieldNotFound {
                ty: ty.clone(),
                field: field.to_string(),
                span,
            }),
        }
    }

    /// Check that a type is callable.
    fn check_callable(
        &mut self,
        ty: &Type,
        args: &[Type],
        ret: &Type,
        span: Span,
    ) -> Result<bool, TypeError> {
        let ty = self.apply_substitutions(ty);

        match &ty {
            Type::Infer(_) => {
                // Defer constraint
                self.constraints
                    .push(Constraint::Callable(ty, args.to_vec(), ret.clone(), span));
                Ok(false)
            }
            Type::Function(func) => {
                if func.params.len() != args.len() {
                    return Err(TypeError::WrongArgCount {
                        expected: func.params.len(),
                        got: args.len(),
                        span,
                    });
                }
                let mut changed = false;
                for ((_, param_ty), arg_ty) in func.params.iter().zip(args.iter()) {
                    changed |= self.unify(param_ty, arg_ty, span)?;
                }
                changed |= self.unify(&func.return_ty, ret, span)?;
                Ok(changed)
            }
            Type::Any => Ok(false),
            _ => Err(TypeError::NotCallable {
                ty: ty.clone(),
                span,
            }),
        }
    }

    /// Check that a type is indexable.
    fn check_indexable(
        &mut self,
        ty: &Type,
        idx: &Type,
        elem: &Type,
        span: Span,
    ) -> Result<bool, TypeError> {
        let ty = self.apply_substitutions(ty);

        match &ty {
            Type::Infer(_) => {
                // Defer constraint
                self.constraints
                    .push(Constraint::Indexable(ty, idx.clone(), elem.clone(), span));
                Ok(false)
            }
            Type::Array(arr_elem) => {
                let mut changed = self.unify(idx, &Type::Number, span)?;
                changed |= self.unify(arr_elem, elem, span)?;
                Ok(changed)
            }
            Type::Any => Ok(false),
            _ => Err(TypeError::NotIndexable {
                ty: ty.clone(),
                span,
            }),
        }
    }

    // ========================================================================
    // Substitution
    // ========================================================================

    /// Apply current substitutions to a type.
    pub fn apply_substitutions(&self, ty: &Type) -> Type {
        match ty {
            Type::Infer(id) => {
                if let Some(sub) = self.substitutions.get(id) {
                    self.apply_substitutions(sub)
                } else {
                    ty.clone()
                }
            }
            Type::TypeVar(id) => {
                if let Some(bound) = self.context.resolve_type_var(*id) {
                    self.apply_substitutions(bound)
                } else {
                    ty.clone()
                }
            }
            Type::Array(inner) => Type::Array(Box::new(self.apply_substitutions(inner))),
            Type::Ref(inner) => Type::Ref(Box::new(self.apply_substitutions(inner))),
            Type::MutRef(inner) => Type::MutRef(Box::new(self.apply_substitutions(inner))),
            Type::Object(obj) => Type::Object(ObjectType {
                fields: obj
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.apply_substitutions(v)))
                    .collect(),
                exact: obj.exact,
            }),
            Type::Function(func) => Type::Function(Box::new(FunctionType {
                params: func
                    .params
                    .iter()
                    .map(|(n, t)| (n.clone(), self.apply_substitutions(t)))
                    .collect(),
                return_ty: self.apply_substitutions(&func.return_ty),
                lifetime_params: func.lifetime_params.clone(),
                type_params: func.type_params.clone(),
                is_method: func.is_method,
            })),
            Type::Generic(id, args) => Type::Generic(
                *id,
                args.iter().map(|a| self.apply_substitutions(a)).collect(),
            ),
            _ => ty.clone(),
        }
    }

    /// Check if an inference variable occurs in a type (for occurs check).
    fn occurs_in(&self, id: InferId, ty: &Type) -> bool {
        match ty {
            Type::Infer(other_id) => {
                if id == *other_id {
                    return true;
                }
                if let Some(sub) = self.substitutions.get(other_id) {
                    return self.occurs_in(id, sub);
                }
                false
            }
            Type::Array(inner) => self.occurs_in(id, inner),
            Type::Ref(inner) | Type::MutRef(inner) => self.occurs_in(id, inner),
            Type::Object(obj) => obj.fields.values().any(|t| self.occurs_in(id, t)),
            Type::Function(func) => {
                func.params.iter().any(|(_, t)| self.occurs_in(id, t))
                    || self.occurs_in(id, &func.return_ty)
            }
            Type::Generic(_, args) => args.iter().any(|a| self.occurs_in(id, a)),
            _ => false,
        }
    }

    /// Get the final resolved type for an inference variable.
    pub fn resolve(&self, ty: &Type) -> Type {
        self.apply_substitutions(ty)
    }
}

// ============================================================================
// Flow-Sensitive Type Narrowing
// ============================================================================

/// Type narrowing for control flow.
pub struct TypeNarrower {
    /// Stack of narrowing contexts (for nested conditionals).
    narrowings: Vec<HashMap<String, Type>>,
}

impl TypeNarrower {
    pub fn new() -> Self {
        Self {
            narrowings: vec![HashMap::new()],
        }
    }

    /// Enter a conditional branch.
    pub fn enter_branch(&mut self) {
        let current = self.narrowings.last().cloned().unwrap_or_default();
        self.narrowings.push(current);
    }

    /// Exit a conditional branch.
    pub fn exit_branch(&mut self) -> HashMap<String, Type> {
        self.narrowings.pop().unwrap_or_default()
    }

    /// Narrow a variable's type.
    pub fn narrow(&mut self, name: String, ty: Type) {
        if let Some(current) = self.narrowings.last_mut() {
            current.insert(name, ty);
        }
    }

    /// Get the narrowed type for a variable.
    pub fn get_narrowed(&self, name: &str) -> Option<&Type> {
        for context in self.narrowings.iter().rev() {
            if let Some(ty) = context.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// Merge narrowings from multiple branches (union of possibilities).
    pub fn merge_branches(&mut self, _branches: Vec<HashMap<String, Type>>) {
        // For now, just clear narrowings after merge (conservative)
        // A more sophisticated approach would compute intersection/union
        if let Some(current) = self.narrowings.last_mut() {
            current.clear();
        }
    }
}

impl Default for TypeNarrower {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unify_primitives() {
        let registry = TypeRegistry::new();
        let mut engine = InferenceEngine::new(&registry);

        // Same primitives unify
        assert!(
            engine
                .unify(&Type::Number, &Type::Number, Span::default())
                .is_ok()
        );
        assert!(
            engine
                .unify(&Type::String, &Type::String, Span::default())
                .is_ok()
        );

        // Different primitives don't unify
        assert!(
            engine
                .unify(&Type::Number, &Type::String, Span::default())
                .is_err()
        );
    }

    #[test]
    fn test_unify_infer() {
        let registry = TypeRegistry::new();
        let mut engine = InferenceEngine::new(&registry);

        let var = engine.fresh_var();

        // Infer variable unifies with concrete type
        assert!(engine.unify(&var, &Type::Number, Span::default()).is_ok());

        // After unification, variable resolves to the concrete type
        assert_eq!(engine.resolve(&var), Type::Number);
    }

    #[test]
    fn test_unify_arrays() {
        let registry = TypeRegistry::new();
        let mut engine = InferenceEngine::new(&registry);

        let arr1 = Type::Array(Box::new(Type::Number));
        let arr2 = Type::Array(Box::new(Type::Number));
        let arr3 = Type::Array(Box::new(Type::String));

        assert!(engine.unify(&arr1, &arr2, Span::default()).is_ok());
        assert!(engine.unify(&arr1, &arr3, Span::default()).is_err());
    }

    #[test]
    fn test_constraint_solving() {
        let registry = TypeRegistry::new();
        let mut engine = InferenceEngine::new(&registry);

        let var = engine.fresh_var();

        // Add constraints
        engine.constrain_equal(var.clone(), Type::Number, Span::default());

        // Solve
        assert!(engine.solve().is_ok());

        // Variable should be resolved
        assert_eq!(engine.resolve(&var), Type::Number);
    }

    #[test]
    fn test_occurs_check() {
        let registry = TypeRegistry::new();
        let mut engine = InferenceEngine::new(&registry);

        let var = engine.fresh_var();
        let recursive = Type::Array(Box::new(var.clone()));

        // Trying to unify var with Array<var> should fail (infinite type)
        assert!(engine.unify(&var, &recursive, Span::default()).is_err());
    }
}
