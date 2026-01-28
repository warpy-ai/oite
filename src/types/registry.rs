//! Type Registry
//!
//! Stores all named type definitions (structs, enums, aliases) and handles
//! generic instantiation with monomorphization caching.

use std::collections::HashMap;

use super::{
    EnumDef, StructDef, Type, TypeAlias, TypeId, TypeVarId, fresh_type_id, fresh_type_var_id,
};

/// The type registry stores all named type definitions.
#[derive(Debug, Default)]
pub struct TypeRegistry {
    /// Named struct definitions.
    pub structs: HashMap<TypeId, StructDef>,
    /// Named enum definitions.
    pub enums: HashMap<TypeId, EnumDef>,
    /// Type aliases.
    pub aliases: HashMap<TypeId, TypeAlias>,

    /// Name to TypeId lookup.
    name_to_id: HashMap<String, TypeId>,

    /// Monomorphization cache: (generic TypeId, concrete type args) -> instantiated TypeId.
    pub instantiations: HashMap<(TypeId, Vec<Type>), TypeId>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    // ========================================================================
    // Registration
    // ========================================================================

    /// Register a new struct definition.
    pub fn register_struct(&mut self, def: StructDef) -> TypeId {
        let id = def.id;
        let name = def.name.clone();
        self.structs.insert(id, def);
        self.name_to_id.insert(name, id);
        id
    }

    /// Register a new enum definition.
    pub fn register_enum(&mut self, def: EnumDef) -> TypeId {
        let id = def.id;
        let name = def.name.clone();
        self.enums.insert(id, def);
        self.name_to_id.insert(name, id);
        id
    }

    /// Register a new type alias.
    pub fn register_alias(&mut self, alias: TypeAlias) -> TypeId {
        let id = alias.id;
        let name = alias.name.clone();
        self.aliases.insert(id, alias);
        self.name_to_id.insert(name, id);
        id
    }

    // ========================================================================
    // Lookup
    // ========================================================================

    /// Look up a type by name.
    pub fn lookup_by_name(&self, name: &str) -> Option<TypeId> {
        self.name_to_id.get(name).copied()
    }

    /// Get a struct definition by ID.
    pub fn get_struct(&self, id: TypeId) -> Option<&StructDef> {
        self.structs.get(&id)
    }

    /// Get a struct definition mutably by ID.
    pub fn get_struct_mut(&mut self, id: TypeId) -> Option<&mut StructDef> {
        self.structs.get_mut(&id)
    }

    /// Get an enum definition by ID.
    pub fn get_enum(&self, id: TypeId) -> Option<&EnumDef> {
        self.enums.get(&id)
    }

    /// Get an enum definition mutably by ID.
    pub fn get_enum_mut(&mut self, id: TypeId) -> Option<&mut EnumDef> {
        self.enums.get_mut(&id)
    }

    /// Get a type alias by ID.
    pub fn get_alias(&self, id: TypeId) -> Option<&TypeAlias> {
        self.aliases.get(&id)
    }

    /// Get the name of a type by ID.
    pub fn get_name(&self, id: TypeId) -> Option<&str> {
        if let Some(s) = self.structs.get(&id) {
            return Some(&s.name);
        }
        if let Some(e) = self.enums.get(&id) {
            return Some(&e.name);
        }
        if let Some(a) = self.aliases.get(&id) {
            return Some(&a.name);
        }
        None
    }

    /// Check if a type ID is a struct.
    pub fn is_struct(&self, id: TypeId) -> bool {
        self.structs.contains_key(&id)
    }

    /// Check if a type ID is an enum.
    pub fn is_enum(&self, id: TypeId) -> bool {
        self.enums.contains_key(&id)
    }

    /// Check if a type ID is an alias.
    pub fn is_alias(&self, id: TypeId) -> bool {
        self.aliases.contains_key(&id)
    }

    // ========================================================================
    // Alias Resolution
    // ========================================================================

    /// Resolve type aliases to their underlying type.
    pub fn resolve_alias(&self, ty: &Type) -> Type {
        match ty {
            Type::Alias(id) => {
                if let Some(alias) = self.aliases.get(id) {
                    self.resolve_alias(&alias.ty)
                } else {
                    ty.clone()
                }
            }
            Type::Array(inner) => Type::Array(Box::new(self.resolve_alias(inner))),
            Type::Ref(inner) => Type::Ref(Box::new(self.resolve_alias(inner))),
            Type::MutRef(inner) => Type::MutRef(Box::new(self.resolve_alias(inner))),
            Type::Generic(id, args) => {
                // First resolve args
                let resolved_args: Vec<Type> = args.iter().map(|a| self.resolve_alias(a)).collect();
                // Then check if the base is an alias
                if let Some(alias) = self.aliases.get(id) {
                    // Substitute type params with args
                    let substituted =
                        self.substitute_type_params(&alias.ty, &alias.type_params, &resolved_args);
                    self.resolve_alias(&substituted)
                } else {
                    Type::Generic(*id, resolved_args)
                }
            }
            _ => ty.clone(),
        }
    }

    // ========================================================================
    // Monomorphization
    // ========================================================================

    /// Get or create a monomorphized instance of a generic type.
    pub fn instantiate(&mut self, id: TypeId, type_args: Vec<Type>) -> TypeId {
        // Check cache first
        let key = (id, type_args.clone());
        if let Some(&cached_id) = self.instantiations.get(&key) {
            return cached_id;
        }

        // Create new instantiation
        let new_id = fresh_type_id();

        if let Some(struct_def) = self.structs.get(&id).cloned() {
            // Substitute type parameters with concrete types
            let instantiated = StructDef {
                id: new_id,
                name: format!("{}<{}>", struct_def.name, format_type_args(&type_args)),
                fields: struct_def
                    .fields
                    .iter()
                    .map(|(name, ty)| {
                        let subst =
                            self.substitute_type_params(ty, &struct_def.type_params, &type_args);
                        (name.clone(), subst)
                    })
                    .collect(),
                type_params: Vec::new(), // Concrete, no params
            };
            self.structs.insert(new_id, instantiated);
        } else if let Some(enum_def) = self.enums.get(&id).cloned() {
            let instantiated = EnumDef {
                id: new_id,
                name: format!("{}<{}>", enum_def.name, format_type_args(&type_args)),
                variants: enum_def
                    .variants
                    .iter()
                    .map(|v| super::EnumVariant {
                        name: v.name.clone(),
                        data: v.data.as_ref().map(|ty| {
                            self.substitute_type_params(ty, &enum_def.type_params, &type_args)
                        }),
                    })
                    .collect(),
                type_params: Vec::new(),
            };
            self.enums.insert(new_id, instantiated);
        }

        self.instantiations.insert(key, new_id);
        new_id
    }

    /// Substitute type parameters with concrete types.
    fn substitute_type_params(&self, ty: &Type, params: &[TypeVarId], args: &[Type]) -> Type {
        match ty {
            Type::TypeVar(var) => {
                // Find which parameter this corresponds to
                if let Some(idx) = params.iter().position(|p| p == var)
                    && idx < args.len()
                {
                    return args[idx].clone();
                }
                ty.clone()
            }
            Type::Array(inner) => {
                Type::Array(Box::new(self.substitute_type_params(inner, params, args)))
            }
            Type::Ref(inner) => {
                Type::Ref(Box::new(self.substitute_type_params(inner, params, args)))
            }
            Type::MutRef(inner) => {
                Type::MutRef(Box::new(self.substitute_type_params(inner, params, args)))
            }
            Type::Object(obj) => Type::Object(super::ObjectType {
                fields: obj
                    .fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.substitute_type_params(v, params, args)))
                    .collect(),
                exact: obj.exact,
            }),
            Type::Function(func) => Type::Function(Box::new(super::FunctionType {
                params: func
                    .params
                    .iter()
                    .map(|(n, t)| (n.clone(), self.substitute_type_params(t, params, args)))
                    .collect(),
                return_ty: self.substitute_type_params(&func.return_ty, params, args),
                lifetime_params: func.lifetime_params.clone(),
                type_params: func.type_params.clone(),
                is_method: func.is_method,
            })),
            Type::Generic(id, inner_args) => {
                let substituted_args: Vec<Type> = inner_args
                    .iter()
                    .map(|a| self.substitute_type_params(a, params, args))
                    .collect();
                Type::Generic(*id, substituted_args)
            }
            _ => ty.clone(),
        }
    }
}

fn format_type_args(args: &[Type]) -> String {
    args.iter()
        .map(|t| format!("{}", t))
        .collect::<Vec<_>>()
        .join(", ")
}

// ============================================================================
// Built-in Types
// ============================================================================

impl TypeRegistry {
    /// Initialize with built-in type definitions.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        // Add Vec<T> as an alias for T[]
        let vec_t = fresh_type_id();
        let t_param = fresh_type_var_id();
        registry.register_alias(TypeAlias {
            id: vec_t,
            name: "Vec".to_string(),
            ty: Type::Array(Box::new(Type::TypeVar(t_param))),
            type_params: vec![t_param],
        });

        // Add Option<T> as T | null (simplified as nullable T)
        // For now we represent this as a union type or just T
        let option_t = fresh_type_id();
        let option_param = fresh_type_var_id();
        registry.register_alias(TypeAlias {
            id: option_t,
            name: "Option".to_string(),
            ty: Type::TypeVar(option_param), // Simplified - real impl would use union
            type_params: vec![option_param],
        });

        registry
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_struct_registration() {
        let mut registry = TypeRegistry::new();
        let id = fresh_type_id();
        let def = StructDef::new(id, "Point".to_string())
            .with_field("x".to_string(), Type::Number)
            .with_field("y".to_string(), Type::Number);

        registry.register_struct(def);

        assert!(registry.lookup_by_name("Point").is_some());
        assert_eq!(registry.lookup_by_name("Point"), Some(id));

        let retrieved = registry.get_struct(id).unwrap();
        assert_eq!(retrieved.name, "Point");
        assert_eq!(retrieved.fields.len(), 2);
    }

    #[test]
    fn test_alias_resolution() {
        let mut registry = TypeRegistry::new();

        // Create alias: type MyNum = number
        let id = fresh_type_id();
        let alias = TypeAlias::new(id, "MyNum".to_string(), Type::Number);
        registry.register_alias(alias);

        // Resolve
        let ty = Type::Alias(id);
        let resolved = registry.resolve_alias(&ty);
        assert_eq!(resolved, Type::Number);
    }

    #[test]
    fn test_monomorphization() {
        let mut registry = TypeRegistry::new();

        // Create generic struct: struct Box<T> { value: T }
        let t_param = fresh_type_var_id();
        let id = fresh_type_id();
        let def = StructDef::new(id, "Box".to_string())
            .with_type_params(vec![t_param])
            .with_field("value".to_string(), Type::TypeVar(t_param));
        registry.register_struct(def);

        // Instantiate Box<number>
        let instantiated_id = registry.instantiate(id, vec![Type::Number]);

        // Check the instantiated struct
        let instantiated = registry.get_struct(instantiated_id).unwrap();
        assert!(instantiated.name.contains("number"));
        assert_eq!(instantiated.fields[0].1, Type::Number);

        // Same instantiation should return cached ID
        let cached_id = registry.instantiate(id, vec![Type::Number]);
        assert_eq!(instantiated_id, cached_id);

        // Different type args should give different ID
        let string_id = registry.instantiate(id, vec![Type::String]);
        assert_ne!(instantiated_id, string_id);
    }
}
