//! SSA-form Intermediate Representation for tscl
//!
//! This module provides a register-based, typed IR for optimization and native
//! code generation. Key features:
//! - SSA form (single static assignment)
//! - Control flow graph with basic blocks
//! - Type annotations for optimization
//! - Ownership tracking for memory safety
//! - Designed for lowering from bytecode or direct AST compilation
//!
//! The IR serves as the bridge between:
//! - Bytecode (current VM format) or AST
//! - Native backends (Cranelift, LLVM)

pub mod format;
pub mod lower;
pub mod opt;
pub mod stubs;
pub mod typecheck;
pub mod verify;

use std::collections::HashMap;
use std::fmt;

// ============================================================================
// Type System
// ============================================================================

/// Unique identifier for a struct type in IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrStructId(pub u32);

impl fmt::Display for IrStructId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "struct#{}", self.0)
    }
}

/// Unique identifier for a field in a struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldId(pub u32);

impl fmt::Display for FieldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "field#{}", self.0)
    }
}

/// Unique identifier for a monomorphized function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonoFuncId(pub u32);

impl fmt::Display for MonoFuncId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mono#{}", self.0)
    }
}

/// IR type for values. Used for type inference and specialization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IrType {
    /// IEEE 754 double-precision float
    Number,
    /// UTF-8 string (heap-allocated)
    String,
    /// Boolean true/false
    Boolean,
    /// JavaScript-like object (heap-allocated)
    Object,
    /// JavaScript-like array (heap-allocated)
    Array,
    /// Typed array with element type
    TypedArray(Box<IrType>),
    /// Function closure
    Function,
    /// Named struct type (with known layout)
    Struct(IrStructId),
    /// Immutable reference (borrow): &T
    Ref(Box<IrType>),
    /// Mutable reference (borrow): &mut T
    MutRef(Box<IrType>),
    /// Dynamic type - requires runtime dispatch
    Any,
    /// Unreachable / bottom type
    Never,
    /// No value (void return)
    Void,
}

impl IrType {
    /// Check if this type is a heap-allocated reference type.
    pub fn is_heap_type(&self) -> bool {
        matches!(
            self,
            IrType::String
                | IrType::Object
                | IrType::Array
                | IrType::TypedArray(_)
                | IrType::Function
                | IrType::Struct(_)
        )
    }

    /// Alias for is_heap_type (legacy name).
    pub fn is_reference(&self) -> bool {
        self.is_heap_type()
    }

    /// Check if this type is a primitive (fits in a register).
    pub fn is_primitive(&self) -> bool {
        matches!(self, IrType::Number | IrType::Boolean)
    }

    /// Check if this is a concrete type (not Any or Never).
    pub fn is_concrete(&self) -> bool {
        !matches!(self, IrType::Any | IrType::Never)
    }

    /// Check if this type is a borrow (reference).
    pub fn is_borrow(&self) -> bool {
        matches!(self, IrType::Ref(_) | IrType::MutRef(_))
    }

    /// Check if this type has copy semantics (no ownership transfer).
    pub fn is_copy(&self) -> bool {
        matches!(self, IrType::Number | IrType::Boolean)
    }

    /// Check if this type has move semantics.
    pub fn is_move(&self) -> bool {
        self.is_heap_type()
    }

    /// Get the inner type for reference types.
    pub fn deref_type(&self) -> Option<&IrType> {
        match self {
            IrType::Ref(inner) | IrType::MutRef(inner) => Some(inner),
            _ => None,
        }
    }

    /// Get the element type for array types.
    pub fn element_type(&self) -> Option<&IrType> {
        match self {
            IrType::TypedArray(inner) => Some(inner),
            IrType::Array => Some(&IrType::Any),
            _ => None,
        }
    }

    /// Create an immutable reference to this type.
    pub fn as_ref(self) -> IrType {
        IrType::Ref(Box::new(self))
    }

    /// Create a mutable reference to this type.
    pub fn as_mut_ref(self) -> IrType {
        IrType::MutRef(Box::new(self))
    }
}

impl fmt::Display for IrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrType::Number => write!(f, "num"),
            IrType::String => write!(f, "str"),
            IrType::Boolean => write!(f, "bool"),
            IrType::Object => write!(f, "obj"),
            IrType::Array => write!(f, "arr"),
            IrType::TypedArray(elem) => write!(f, "{}[]", elem),
            IrType::Function => write!(f, "fn"),
            IrType::Struct(id) => write!(f, "{}", id),
            IrType::Ref(inner) => write!(f, "&{}", inner),
            IrType::MutRef(inner) => write!(f, "&mut {}", inner),
            IrType::Any => write!(f, "any"),
            IrType::Never => write!(f, "!"),
            IrType::Void => write!(f, "void"),
        }
    }
}

// ============================================================================
// Ownership System
// ============================================================================

/// Ownership state of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ownership {
    /// Value is owned by this binding (can be moved or borrowed).
    Owned,
    /// Value has been moved to another location (tombstone state).
    Moved,
    /// Value is borrowed immutably (read-only reference).
    BorrowedImm,
    /// Value is borrowed mutably (exclusive write access).
    BorrowedMut,
    /// Value is captured by a closure (async or sync).
    Captured,
}

impl fmt::Display for Ownership {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ownership::Owned => write!(f, "owned"),
            Ownership::Moved => write!(f, "moved"),
            Ownership::BorrowedImm => write!(f, "&"),
            Ownership::BorrowedMut => write!(f, "&mut"),
            Ownership::Captured => write!(f, "captured"),
        }
    }
}

/// Storage location for a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageLocation {
    /// Value lives on the stack (fast, automatic cleanup).
    Stack,
    /// Value lives on the heap (GC managed).
    Heap,
    /// Value is in a register (immediate, no address).
    Register,
}

impl fmt::Display for StorageLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageLocation::Stack => write!(f, "stack"),
            StorageLocation::Heap => write!(f, "heap"),
            StorageLocation::Register => write!(f, "reg"),
        }
    }
}

/// Lifetime identifier for tracking borrow scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LifetimeId(pub u32);

impl fmt::Display for LifetimeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'l{}", self.0)
    }
}

/// Complete value metadata including type, ownership, and storage.
#[derive(Debug, Clone)]
pub struct ValueInfo {
    pub ty: IrType,
    pub ownership: Ownership,
    pub storage: StorageLocation,
    /// Lifetime of this value (for borrow checking).
    pub lifetime: Option<LifetimeId>,
    /// For borrowed values, the source value being borrowed.
    pub borrowed_from: Option<ValueId>,
}

impl ValueInfo {
    pub fn new(ty: IrType) -> Self {
        // Determine default storage based on type
        let storage = if ty.is_reference() {
            StorageLocation::Heap
        } else if ty.is_primitive() {
            StorageLocation::Register
        } else {
            StorageLocation::Stack
        };

        Self {
            ty,
            ownership: Ownership::Owned,
            storage,
            lifetime: None,
            borrowed_from: None,
        }
    }

    pub fn with_ownership(mut self, ownership: Ownership) -> Self {
        self.ownership = ownership;
        self
    }

    pub fn with_storage(mut self, storage: StorageLocation) -> Self {
        self.storage = storage;
        self
    }

    pub fn with_lifetime(mut self, lifetime: LifetimeId) -> Self {
        self.lifetime = Some(lifetime);
        self
    }

    pub fn borrowed_from(mut self, source: ValueId) -> Self {
        self.borrowed_from = Some(source);
        self
    }
}

// ============================================================================
// SSA Values
// ============================================================================

/// Unique identifier for an SSA value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// An SSA value with its type.
#[derive(Debug, Clone)]
pub struct IrValue {
    pub id: ValueId,
    pub ty: IrType,
}

impl IrValue {
    pub fn new(id: ValueId, ty: IrType) -> Self {
        Self { id, ty }
    }
}

// ============================================================================
// Literals
// ============================================================================

/// Literal constant values.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Number(f64),
    String(String),
    Boolean(bool),
    Null,
    Undefined,
}

impl Literal {
    pub fn ir_type(&self) -> IrType {
        match self {
            Literal::Number(_) => IrType::Number,
            Literal::String(_) => IrType::String,
            Literal::Boolean(_) => IrType::Boolean,
            Literal::Null | Literal::Undefined => IrType::Any,
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Number(n) => write!(f, "{}", n),
            Literal::String(s) => write!(f, "\"{}\"", s.escape_debug()),
            Literal::Boolean(b) => write!(f, "{}", b),
            Literal::Null => write!(f, "null"),
            Literal::Undefined => write!(f, "undefined"),
        }
    }
}

// ============================================================================
// Block Identifiers
// ============================================================================

/// Unique identifier for a basic block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

// ============================================================================
// IR Operations
// ============================================================================

/// An IR instruction (operation).
#[derive(Debug, Clone)]
pub enum IrOp {
    // === Constants ===
    /// Load a constant value.
    Const(ValueId, Literal),

    // === Arithmetic (type-specialized) ===
    /// Add two numbers: dst = a + b
    AddNum(ValueId, ValueId, ValueId),
    /// Subtract two numbers: dst = a - b
    SubNum(ValueId, ValueId, ValueId),
    /// Multiply two numbers: dst = a * b
    MulNum(ValueId, ValueId, ValueId),
    /// Divide two numbers: dst = a / b
    DivNum(ValueId, ValueId, ValueId),
    /// Modulo two numbers: dst = a % b
    ModNum(ValueId, ValueId, ValueId),
    /// Negate a number: dst = -a
    NegNum(ValueId, ValueId),

    // === Arithmetic (dynamic dispatch for 'any' type) ===
    /// Dynamic add: dst = a + b (may be string concat)
    AddAny(ValueId, ValueId, ValueId),
    /// Dynamic subtract
    SubAny(ValueId, ValueId, ValueId),
    /// Dynamic multiply
    MulAny(ValueId, ValueId, ValueId),
    /// Dynamic divide
    DivAny(ValueId, ValueId, ValueId),
    /// Dynamic modulo
    ModAny(ValueId, ValueId, ValueId),
    /// Dynamic negate
    NegAny(ValueId, ValueId),

    // === Bitwise operations ===
    /// Bitwise AND: dst = a & b
    BitAnd(ValueId, ValueId, ValueId),
    /// Bitwise OR: dst = a | b
    BitOr(ValueId, ValueId, ValueId),
    /// Bitwise XOR: dst = a ^ b
    Xor(ValueId, ValueId, ValueId),
    /// Left shift: dst = a << b
    Shl(ValueId, ValueId, ValueId),
    /// Right shift (arithmetic): dst = a >> b
    Shr(ValueId, ValueId, ValueId),
    /// Right shift (logical): dst = a >>> b
    ShrU(ValueId, ValueId, ValueId),
    /// Power: dst = a ** b
    Pow(ValueId, ValueId, ValueId),

    // === Comparison ===
    /// Strict equality: dst = a === b
    EqStrict(ValueId, ValueId, ValueId),
    /// Strict inequality: dst = a !== b
    NeStrict(ValueId, ValueId, ValueId),
    /// Less than: dst = a < b
    Lt(ValueId, ValueId, ValueId),
    /// Less than or equal: dst = a <= b
    LtEq(ValueId, ValueId, ValueId),
    /// Greater than: dst = a > b
    Gt(ValueId, ValueId, ValueId),
    /// Greater than or equal: dst = a >= b
    GtEq(ValueId, ValueId, ValueId),

    // === Logical ===
    /// Logical NOT: dst = !a
    Not(ValueId, ValueId),
    /// Logical AND (short-circuit): dst = a && b
    And(ValueId, ValueId, ValueId),
    /// Logical OR (short-circuit): dst = a || b
    Or(ValueId, ValueId, ValueId),

    // === Local Variables ===
    /// Load from local slot: dst = locals[slot]
    LoadLocal(ValueId, u32),
    /// Store to local slot: locals[slot] = src
    StoreLocal(u32, ValueId),

    // === Global Variables ===
    /// Load global by name: dst = globals[name]
    LoadGlobal(ValueId, String),
    /// Store global by name: globals[name] = src
    StoreGlobal(String, ValueId),

    // === Object Operations ===
    /// Create new empty object: dst = {}
    NewObject(ValueId),
    /// Get property: dst = obj.name
    GetProp(ValueId, ValueId, String),
    /// Set property: obj.name = val
    SetProp(ValueId, String, ValueId),
    /// Get computed property: dst = obj[key]
    GetElement(ValueId, ValueId, ValueId),
    /// Set computed property: obj[key] = val
    SetElement(ValueId, ValueId, ValueId),

    // === Array Operations ===
    /// Create new array: dst = []
    NewArray(ValueId),
    /// Get array length: dst = arr.length
    ArrayLen(ValueId, ValueId),
    /// Push to array: arr.push(val)
    ArrayPush(ValueId, ValueId),

    // === Function Operations ===
    /// Call function: dst = func(args...)
    Call(ValueId, ValueId, Vec<ValueId>),
    /// Call method: dst = obj.method(args...)
    CallMethod(ValueId, ValueId, String, Vec<ValueId>),
    /// Create closure: dst = closure(func_id, env)
    MakeClosure(ValueId, u32, ValueId),

    // === Type Operations ===
    /// Type check: dst = typeof(val) == expected_type
    TypeCheck(ValueId, ValueId, IrType),
    /// Type guard (for narrowing): assert val is type, then dst = val
    TypeGuard(ValueId, ValueId, IrType),
    /// Convert to boolean: dst = ToBoolean(val)
    ToBool(ValueId, ValueId),
    /// Convert to number: dst = ToNumber(val)
    ToNum(ValueId, ValueId),
    /// TypeOf operator: dst = typeof(val) as string
    TypeOf(ValueId, ValueId),
    /// Delete property: dst = delete obj.prop (returns bool)
    DeleteProp(ValueId, ValueId, String),

    // === Phi Functions (SSA merge points) ===
    /// Phi function: dst = phi([(block1, val1), (block2, val2), ...])
    Phi(ValueId, Vec<(BlockId, ValueId)>),

    // === Misc ===
    /// Copy value: dst = src (for SSA rename)
    Copy(ValueId, ValueId),
    /// Load 'this' context
    LoadThis(ValueId),

    // === Borrow Operations ===
    /// Create immutable borrow: dst = &src
    Borrow(ValueId, ValueId),
    /// Create mutable borrow: dst = &mut src
    BorrowMut(ValueId, ValueId),
    /// Dereference: dst = *src
    Deref(ValueId, ValueId),
    /// Store through reference: *dst = src
    DerefStore(ValueId, ValueId),
    /// End a borrow's lifetime (for borrow checker)
    EndBorrow(ValueId),

    // === Struct Operations ===
    /// Create new struct: dst = StructType {}
    StructNew(ValueId, IrStructId),
    /// Get struct field: dst = src.field
    StructGetField(ValueId, ValueId, FieldId),
    /// Set struct field: dst.field = src
    StructSetField(ValueId, FieldId, ValueId),
    /// Get struct field by name (for unresolved field access)
    StructGetFieldNamed(ValueId, ValueId, String),
    /// Set struct field by name
    StructSetFieldNamed(ValueId, String, ValueId),

    // === Monomorphized Calls ===
    /// Call monomorphized function: dst = mono_func(args...)
    CallMono(ValueId, MonoFuncId, Vec<ValueId>),

    // === Move Operations ===
    /// Move value: dst = move src (marks src as moved)
    Move(ValueId, ValueId),
    /// Clone value: dst = clone src (for explicit copies of heap types)
    Clone(ValueId, ValueId),
}

impl IrOp {
    /// Get the destination value (if any) of this operation.
    pub fn dest(&self) -> Option<ValueId> {
        match self {
            IrOp::Const(d, _)
            | IrOp::AddNum(d, _, _)
            | IrOp::SubNum(d, _, _)
            | IrOp::MulNum(d, _, _)
            | IrOp::DivNum(d, _, _)
            | IrOp::ModNum(d, _, _)
            | IrOp::NegNum(d, _)
            | IrOp::AddAny(d, _, _)
            | IrOp::SubAny(d, _, _)
            | IrOp::MulAny(d, _, _)
            | IrOp::DivAny(d, _, _)
            | IrOp::ModAny(d, _, _)
            | IrOp::NegAny(d, _)
            | IrOp::EqStrict(d, _, _)
            | IrOp::NeStrict(d, _, _)
            | IrOp::Lt(d, _, _)
            | IrOp::LtEq(d, _, _)
            | IrOp::Gt(d, _, _)
            | IrOp::GtEq(d, _, _)
            | IrOp::Not(d, _)
            | IrOp::And(d, _, _)
            | IrOp::Or(d, _, _)
            | IrOp::BitAnd(d, _, _)
            | IrOp::BitOr(d, _, _)
            | IrOp::Xor(d, _, _)
            | IrOp::Shl(d, _, _)
            | IrOp::Shr(d, _, _)
            | IrOp::ShrU(d, _, _)
            | IrOp::Pow(d, _, _)
            | IrOp::LoadLocal(d, _)
            | IrOp::LoadGlobal(d, _)
            | IrOp::NewObject(d)
            | IrOp::GetProp(d, _, _)
            | IrOp::GetElement(d, _, _)
            | IrOp::NewArray(d)
            | IrOp::ArrayLen(d, _)
            | IrOp::Call(d, _, _)
            | IrOp::CallMethod(d, _, _, _)
            | IrOp::MakeClosure(d, _, _)
            | IrOp::TypeCheck(d, _, _)
            | IrOp::TypeGuard(d, _, _)
            | IrOp::ToBool(d, _)
            | IrOp::ToNum(d, _)
            | IrOp::Phi(d, _)
            | IrOp::Copy(d, _)
            | IrOp::LoadThis(d)
            // Borrow operations
            | IrOp::Borrow(d, _)
            | IrOp::BorrowMut(d, _)
            | IrOp::Deref(d, _)
            // Struct operations
            | IrOp::StructNew(d, _)
            | IrOp::StructGetField(d, _, _)
            | IrOp::StructGetFieldNamed(d, _, _)
            // Monomorphized calls
            | IrOp::CallMono(d, _, _)
            // Move operations
            | IrOp::Move(d, _)
            | IrOp::Clone(d, _)
            // Type operations
            | IrOp::TypeOf(d, _)
            | IrOp::DeleteProp(d, _, _) => Some(*d),

            IrOp::StoreLocal(_, _)
            | IrOp::StoreGlobal(_, _)
            | IrOp::SetProp(_, _, _)
            | IrOp::SetElement(_, _, _)
            | IrOp::ArrayPush(_, _)
            // Borrow operations without dest
            | IrOp::DerefStore(_, _)
            | IrOp::EndBorrow(_)
            // Struct operations without dest
            | IrOp::StructSetField(_, _, _)
            | IrOp::StructSetFieldNamed(_, _, _) => None,
        }
    }

    /// Get all values used by this operation.
    pub fn uses(&self) -> Vec<ValueId> {
        match self {
            IrOp::Const(_, _) => vec![],
            IrOp::AddNum(_, a, b)
            | IrOp::SubNum(_, a, b)
            | IrOp::MulNum(_, a, b)
            | IrOp::DivNum(_, a, b)
            | IrOp::ModNum(_, a, b)
            | IrOp::AddAny(_, a, b)
            | IrOp::SubAny(_, a, b)
            | IrOp::MulAny(_, a, b)
            | IrOp::DivAny(_, a, b)
            | IrOp::ModAny(_, a, b)
            | IrOp::EqStrict(_, a, b)
            | IrOp::NeStrict(_, a, b)
            | IrOp::Lt(_, a, b)
            | IrOp::LtEq(_, a, b)
            | IrOp::Gt(_, a, b)
            | IrOp::GtEq(_, a, b)
            | IrOp::And(_, a, b)
            | IrOp::Or(_, a, b)
            | IrOp::BitAnd(_, a, b)
            | IrOp::BitOr(_, a, b)
            | IrOp::Xor(_, a, b)
            | IrOp::Shl(_, a, b)
            | IrOp::Shr(_, a, b)
            | IrOp::ShrU(_, a, b)
            | IrOp::Pow(_, a, b)
            // Borrow store
            | IrOp::DerefStore(a, b) => vec![*a, *b],

            IrOp::NegNum(_, a)
            | IrOp::NegAny(_, a)
            | IrOp::Not(_, a)
            | IrOp::ToBool(_, a)
            | IrOp::ToNum(_, a)
            | IrOp::Copy(_, a)
            | IrOp::ArrayLen(_, a)
            | IrOp::TypeCheck(_, a, _)
            | IrOp::TypeGuard(_, a, _)
            // Borrow operations
            | IrOp::Borrow(_, a)
            | IrOp::BorrowMut(_, a)
            | IrOp::Deref(_, a)
            | IrOp::EndBorrow(a)
            // Move operations
            | IrOp::Move(_, a)
            | IrOp::Clone(_, a) => vec![*a],

            IrOp::LoadLocal(_, _) | IrOp::LoadGlobal(_, _) | IrOp::LoadThis(_) => vec![],
            IrOp::StoreLocal(_, v) | IrOp::StoreGlobal(_, v) => vec![*v],

            IrOp::NewObject(_) | IrOp::NewArray(_) => vec![],
            // Struct new
            IrOp::StructNew(_, _) => vec![],

            IrOp::GetProp(_, obj, _) => vec![*obj],
            IrOp::SetProp(obj, _, val) => vec![*obj, *val],
            IrOp::TypeOf(_, val) => vec![*val],
            IrOp::DeleteProp(_, obj, _) => vec![*obj],
            IrOp::GetElement(_, obj, key) => vec![*obj, *key],
            IrOp::SetElement(obj, key, val) => vec![*obj, *key, *val],
            IrOp::ArrayPush(arr, val) => vec![*arr, *val],

            // Struct field operations
            IrOp::StructGetField(_, src, _) => vec![*src],
            IrOp::StructGetFieldNamed(_, src, _) => vec![*src],
            IrOp::StructSetField(dst, _, val) => vec![*dst, *val],
            IrOp::StructSetFieldNamed(dst, _, val) => vec![*dst, *val],

            IrOp::Call(_, func, args) => {
                let mut uses = vec![*func];
                uses.extend(args.iter().copied());
                uses
            }
            IrOp::CallMethod(_, obj, _, args) => {
                let mut uses = vec![*obj];
                uses.extend(args.iter().copied());
                uses
            }
            IrOp::CallMono(_, _, args) => args.clone(),
            IrOp::MakeClosure(_, _, env) => vec![*env],

            IrOp::Phi(_, entries) => entries.iter().map(|(_, v)| *v).collect(),
        }
    }
}

// ============================================================================
// Block Terminator
// ============================================================================

/// How a basic block ends (control flow).
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Unconditional jump to another block.
    Jump(BlockId),
    /// Conditional branch: if cond then true_block else false_block.
    Branch(ValueId, BlockId, BlockId),
    /// Return from function with optional value.
    Return(Option<ValueId>),
    /// Unreachable (after infinite loops, etc.)
    Unreachable,
}

impl Terminator {
    /// Get all successor blocks.
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Terminator::Jump(target) => vec![*target],
            Terminator::Branch(_, t, f) => vec![*t, *f],
            Terminator::Return(_) | Terminator::Unreachable => vec![],
        }
    }

    /// Get all values used by this terminator.
    pub fn uses(&self) -> Vec<ValueId> {
        match self {
            Terminator::Branch(cond, _, _) => vec![*cond],
            Terminator::Return(Some(v)) => vec![*v],
            _ => vec![],
        }
    }
}

// ============================================================================
// Basic Block
// ============================================================================

/// A basic block in the control flow graph.
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    /// Operations in this block (excluding terminator).
    pub ops: Vec<IrOp>,
    /// How this block ends.
    pub terminator: Terminator,
    /// Predecessor blocks (filled during CFG construction).
    pub predecessors: Vec<BlockId>,
}

impl BasicBlock {
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            ops: Vec::new(),
            terminator: Terminator::Unreachable,
            predecessors: Vec::new(),
        }
    }

    /// Add an operation to this block.
    pub fn push(&mut self, op: IrOp) {
        self.ops.push(op);
    }

    /// Set the terminator for this block.
    pub fn terminate(&mut self, term: Terminator) {
        self.terminator = term;
    }
}

// ============================================================================
// Function
// ============================================================================

/// An IR function.
#[derive(Debug, Clone)]
pub struct IrFunction {
    /// Function name (empty for anonymous).
    pub name: String,
    /// Parameter names and types.
    pub params: Vec<(String, IrType)>,
    /// Return type.
    pub return_ty: IrType,
    /// Basic blocks (first is entry).
    pub blocks: Vec<BasicBlock>,
    /// Local variable slots.
    pub locals: Vec<(String, IrType)>,
    /// Next value ID to allocate.
    next_value: u32,
    /// Next block ID to allocate.
    next_block: u32,
    /// Next lifetime ID to allocate.
    next_lifetime: u32,
    /// Value types (for type inference).
    pub value_types: HashMap<ValueId, IrType>,
    /// Value ownership and storage info (for borrow checking).
    pub value_info: HashMap<ValueId, ValueInfo>,
}

impl IrFunction {
    pub fn new(name: String) -> Self {
        Self {
            name,
            params: Vec::new(),
            return_ty: IrType::Any,
            blocks: Vec::new(),
            locals: Vec::new(),
            next_value: 0,
            next_block: 0,
            next_lifetime: 0,
            value_types: HashMap::new(),
            value_info: HashMap::new(),
        }
    }

    /// Allocate a new value ID with type.
    pub fn alloc_value(&mut self, ty: IrType) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.value_types.insert(id, ty.clone());
        self.value_info.insert(id, ValueInfo::new(ty));
        id
    }

    /// Allocate a new value with explicit ownership info.
    pub fn alloc_value_with_info(&mut self, info: ValueInfo) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.value_types.insert(id, info.ty.clone());
        self.value_info.insert(id, info);
        id
    }

    /// Allocate a new lifetime ID.
    pub fn alloc_lifetime(&mut self) -> LifetimeId {
        let id = LifetimeId(self.next_lifetime);
        self.next_lifetime += 1;
        id
    }

    /// Mark a value as moved.
    pub fn mark_moved(&mut self, val: ValueId) {
        if let Some(info) = self.value_info.get_mut(&val) {
            info.ownership = Ownership::Moved;
        }
    }

    /// Mark a value as borrowed immutably.
    pub fn mark_borrowed(&mut self, val: ValueId, lifetime: LifetimeId) {
        if let Some(info) = self.value_info.get_mut(&val) {
            info.ownership = Ownership::BorrowedImm;
            info.lifetime = Some(lifetime);
        }
    }

    /// Get ownership state of a value.
    pub fn get_ownership(&self, val: ValueId) -> Option<Ownership> {
        self.value_info.get(&val).map(|info| info.ownership)
    }

    /// Check if a value is still valid (not moved).
    pub fn is_valid(&self, val: ValueId) -> bool {
        self.value_info
            .get(&val)
            .map(|info| info.ownership != Ownership::Moved)
            .unwrap_or(false)
    }

    /// Allocate a new block.
    pub fn alloc_block(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        self.blocks.push(BasicBlock::new(id));
        id
    }

    /// Get a mutable reference to a block by ID.
    pub fn block_mut(&mut self, id: BlockId) -> &mut BasicBlock {
        &mut self.blocks[id.0 as usize]
    }

    /// Get a reference to a block by ID.
    pub fn block(&self, id: BlockId) -> &BasicBlock {
        &self.blocks[id.0 as usize]
    }

    /// Get the entry block ID.
    pub fn entry_block(&self) -> BlockId {
        BlockId(0)
    }

    /// Add a local variable and return its slot index.
    pub fn add_local(&mut self, name: String, ty: IrType) -> u32 {
        let slot = self.locals.len() as u32;
        self.locals.push((name, ty));
        slot
    }

    /// Compute predecessor information for all blocks.
    pub fn compute_predecessors(&mut self) {
        // Clear existing predecessors
        for block in &mut self.blocks {
            block.predecessors.clear();
        }

        // Collect edges first to avoid borrow issues
        let mut edges: Vec<(BlockId, BlockId)> = Vec::new();
        for block in &self.blocks {
            for succ in block.terminator.successors() {
                edges.push((block.id, succ));
            }
        }

        // Add predecessors
        for (pred, succ) in edges {
            self.blocks[succ.0 as usize].predecessors.push(pred);
        }
    }
}

// ============================================================================
// Module (Top-Level)
// ============================================================================

/// A struct type definition with known layout.
#[derive(Debug, Clone)]
pub struct IrStructDef {
    pub id: IrStructId,
    pub name: String,
    /// Fields in declaration order: (name, type, offset).
    pub fields: Vec<(String, IrType, u32)>,
    /// Total size in bytes.
    pub size: u32,
    /// Alignment requirement.
    pub alignment: u32,
}

impl IrStructDef {
    pub fn new(id: IrStructId, name: String) -> Self {
        Self {
            id,
            name,
            fields: Vec::new(),
            size: 0,
            alignment: 8, // Default to 8-byte alignment
        }
    }

    /// Add a field and compute its offset.
    pub fn add_field(&mut self, name: String, ty: IrType) -> FieldId {
        let field_id = FieldId(self.fields.len() as u32);
        let field_size = Self::type_size(&ty);
        let field_align = Self::type_alignment(&ty);

        // Align the current size to the field's alignment
        let offset = (self.size + field_align - 1) & !(field_align - 1);
        self.fields.push((name, ty, offset));

        // Update total size
        self.size = offset + field_size;

        // Update alignment to max of all fields
        self.alignment = self.alignment.max(field_align);

        field_id
    }

    /// Get field by name.
    pub fn get_field(&self, name: &str) -> Option<(FieldId, &IrType, u32)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, (n, _, _))| n == name)
            .map(|(i, (_, ty, offset))| (FieldId(i as u32), ty, *offset))
    }

    /// Get field by ID.
    pub fn get_field_by_id(&self, id: FieldId) -> Option<(&str, &IrType, u32)> {
        self.fields
            .get(id.0 as usize)
            .map(|(name, ty, offset)| (name.as_str(), ty, *offset))
    }

    /// Estimate size of a type in bytes.
    fn type_size(ty: &IrType) -> u32 {
        match ty {
            IrType::Number => 8,
            IrType::Boolean => 1,
            IrType::Void => 0,
            IrType::Never => 0,
            // Reference types are pointers
            IrType::String
            | IrType::Object
            | IrType::Array
            | IrType::TypedArray(_)
            | IrType::Function
            | IrType::Struct(_)
            | IrType::Ref(_)
            | IrType::MutRef(_) => 8,
            IrType::Any => 16, // Tagged value: tag + payload
        }
    }

    /// Get alignment requirement for a type.
    fn type_alignment(ty: &IrType) -> u32 {
        match ty {
            IrType::Number => 8,
            IrType::Boolean => 1,
            IrType::Void | IrType::Never => 1,
            _ => 8, // All reference types and Any are 8-byte aligned
        }
    }

    /// Finalize the struct (pad to alignment).
    pub fn finalize(&mut self) {
        self.size = (self.size + self.alignment - 1) & !(self.alignment - 1);
    }
}

impl fmt::Display for IrStructDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "struct {} {{ // size: {}, align: {}",
            self.name, self.size, self.alignment
        )?;
        for (name, ty, offset) in &self.fields {
            writeln!(f, "    {}: {} // offset: {}", name, ty, offset)?;
        }
        write!(f, "}}")
    }
}

/// An IR module containing functions.
#[derive(Debug)]
pub struct IrModule {
    /// Functions in this module.
    pub functions: Vec<IrFunction>,
    /// Global variable names.
    pub globals: Vec<String>,
    /// Struct definitions.
    pub structs: HashMap<IrStructId, IrStructDef>,
    /// Next struct ID to allocate.
    next_struct_id: u32,
    /// Monomorphization cache: (generic func id, type args) -> mono func id.
    pub mono_cache: HashMap<(usize, Vec<IrType>), MonoFuncId>,
    /// Next mono func ID.
    next_mono_id: u32,
    /// Mapping from bytecode address to function index (for extracted functions).
    pub function_addrs: HashMap<usize, usize>,
    /// Bytecode address of user-defined main() function, if any.
    pub user_main_addr: Option<usize>,
}

impl IrModule {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            globals: Vec::new(),
            structs: HashMap::new(),
            next_struct_id: 0,
            mono_cache: HashMap::new(),
            next_mono_id: 0,
            function_addrs: HashMap::new(),
            user_main_addr: None,
        }
    }

    /// Get a function by its bytecode address.
    pub fn get_function_by_addr(&self, addr: usize) -> Option<&IrFunction> {
        self.function_addrs
            .get(&addr)
            .and_then(|&idx| self.functions.get(idx))
    }

    /// Get the function index for a bytecode address.
    pub fn get_function_idx_by_addr(&self, addr: usize) -> Option<usize> {
        self.function_addrs.get(&addr).copied()
    }

    /// Add a function and return its index.
    pub fn add_function(&mut self, func: IrFunction) -> usize {
        let idx = self.functions.len();
        self.functions.push(func);
        idx
    }

    /// Define a new struct type.
    pub fn define_struct(&mut self, name: String) -> IrStructId {
        let id = IrStructId(self.next_struct_id);
        self.next_struct_id += 1;
        self.structs.insert(id, IrStructDef::new(id, name));
        id
    }

    /// Get a struct definition by ID.
    pub fn get_struct(&self, id: IrStructId) -> Option<&IrStructDef> {
        self.structs.get(&id)
    }

    /// Get a mutable struct definition by ID.
    pub fn get_struct_mut(&mut self, id: IrStructId) -> Option<&mut IrStructDef> {
        self.structs.get_mut(&id)
    }

    /// Get or create a monomorphized function ID.
    pub fn get_or_create_mono(&mut self, func_idx: usize, type_args: Vec<IrType>) -> MonoFuncId {
        let key = (func_idx, type_args);
        if let Some(&id) = self.mono_cache.get(&key) {
            return id;
        }
        let id = MonoFuncId(self.next_mono_id);
        self.next_mono_id += 1;
        self.mono_cache.insert(key, id);
        id
    }
}

impl Default for IrModule {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Pretty Printing
// ============================================================================

impl fmt::Display for IrOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrOp::Const(d, lit) => write!(f, "{} = const {}", d, lit),
            IrOp::AddNum(d, a, b) => write!(f, "{} = add.num {}, {}", d, a, b),
            IrOp::SubNum(d, a, b) => write!(f, "{} = sub.num {}, {}", d, a, b),
            IrOp::MulNum(d, a, b) => write!(f, "{} = mul.num {}, {}", d, a, b),
            IrOp::DivNum(d, a, b) => write!(f, "{} = div.num {}, {}", d, a, b),
            IrOp::ModNum(d, a, b) => write!(f, "{} = mod.num {}, {}", d, a, b),
            IrOp::NegNum(d, a) => write!(f, "{} = neg.num {}", d, a),
            IrOp::AddAny(d, a, b) => write!(f, "{} = add.any {}, {}", d, a, b),
            IrOp::SubAny(d, a, b) => write!(f, "{} = sub.any {}, {}", d, a, b),
            IrOp::MulAny(d, a, b) => write!(f, "{} = mul.any {}, {}", d, a, b),
            IrOp::DivAny(d, a, b) => write!(f, "{} = div.any {}, {}", d, a, b),
            IrOp::ModAny(d, a, b) => write!(f, "{} = mod.any {}, {}", d, a, b),
            IrOp::NegAny(d, a) => write!(f, "{} = neg.any {}", d, a),
            IrOp::EqStrict(d, a, b) => write!(f, "{} = eq.strict {}, {}", d, a, b),
            IrOp::NeStrict(d, a, b) => write!(f, "{} = ne.strict {}, {}", d, a, b),
            IrOp::Lt(d, a, b) => write!(f, "{} = lt {}, {}", d, a, b),
            IrOp::LtEq(d, a, b) => write!(f, "{} = le {}, {}", d, a, b),
            IrOp::Gt(d, a, b) => write!(f, "{} = gt {}, {}", d, a, b),
            IrOp::GtEq(d, a, b) => write!(f, "{} = ge {}, {}", d, a, b),
            IrOp::Not(d, a) => write!(f, "{} = not {}", d, a),
            IrOp::And(d, a, b) => write!(f, "{} = and {}, {}", d, a, b),
            IrOp::Or(d, a, b) => write!(f, "{} = or {}, {}", d, a, b),
            IrOp::BitAnd(d, a, b) => write!(f, "{} = and {}, {}", d, a, b),
            IrOp::BitOr(d, a, b) => write!(f, "{} = or {}, {}", d, a, b),
            IrOp::Xor(d, a, b) => write!(f, "{} = xor {}, {}", d, a, b),
            IrOp::Shl(d, a, b) => write!(f, "{} = shl {}, {}", d, a, b),
            IrOp::Shr(d, a, b) => write!(f, "{} = shr {}, {}", d, a, b),
            IrOp::ShrU(d, a, b) => write!(f, "{} = shr.u {}, {}", d, a, b),
            IrOp::Pow(d, a, b) => write!(f, "{} = pow {}, {}", d, a, b),
            IrOp::LoadLocal(d, slot) => write!(f, "{} = load.local ${}", d, slot),
            IrOp::StoreLocal(slot, v) => write!(f, "store.local ${}, {}", slot, v),
            IrOp::LoadGlobal(d, name) => write!(f, "{} = load.global @{}", d, name),
            IrOp::StoreGlobal(name, v) => write!(f, "store.global @{}, {}", name, v),
            IrOp::NewObject(d) => write!(f, "{} = new.object", d),
            IrOp::GetProp(d, obj, name) => write!(f, "{} = get.prop {}, .{}", d, obj, name),
            IrOp::SetProp(obj, name, val) => write!(f, "set.prop {}, .{}, {}", obj, name, val),
            IrOp::GetElement(d, obj, key) => write!(f, "{} = get.elem {}, [{}]", d, obj, key),
            IrOp::SetElement(obj, key, val) => write!(f, "set.elem {}, [{}], {}", obj, key, val),
            IrOp::NewArray(d) => write!(f, "{} = new.array", d),
            IrOp::ArrayLen(d, arr) => write!(f, "{} = array.len {}", d, arr),
            IrOp::ArrayPush(arr, val) => write!(f, "array.push {}, {}", arr, val),
            IrOp::Call(d, func, args) => {
                let args_str: Vec<_> = args.iter().map(|a| format!("{}", a)).collect();
                write!(f, "{} = call {}({})", d, func, args_str.join(", "))
            }
            IrOp::CallMethod(d, obj, method, args) => {
                let args_str: Vec<_> = args.iter().map(|a| format!("{}", a)).collect();
                write!(
                    f,
                    "{} = call.method {}.{}({})",
                    d,
                    obj,
                    method,
                    args_str.join(", ")
                )
            }
            IrOp::MakeClosure(d, func_id, env) => {
                write!(f, "{} = make.closure func#{}, {}", d, func_id, env)
            }
            IrOp::TypeCheck(d, v, ty) => write!(f, "{} = typecheck {}, {}", d, v, ty),
            IrOp::TypeGuard(d, v, ty) => write!(f, "{} = typeguard {}, {}", d, v, ty),
            IrOp::ToBool(d, v) => write!(f, "{} = to.bool {}", d, v),
            IrOp::ToNum(d, v) => write!(f, "{} = to.num {}", d, v),
            IrOp::Phi(d, entries) => {
                let entries_str: Vec<_> = entries
                    .iter()
                    .map(|(b, v)| format!("[{}: {}]", b, v))
                    .collect();
                write!(f, "{} = phi {}", d, entries_str.join(", "))
            }
            IrOp::Copy(d, s) => write!(f, "{} = copy {}", d, s),
            IrOp::LoadThis(d) => write!(f, "{} = load.this", d),
            // Borrow operations
            IrOp::Borrow(d, s) => write!(f, "{} = borrow {}", d, s),
            IrOp::BorrowMut(d, s) => write!(f, "{} = borrow.mut {}", d, s),
            IrOp::Deref(d, s) => write!(f, "{} = deref {}", d, s),
            IrOp::DerefStore(dst, val) => write!(f, "deref.store {}, {}", dst, val),
            IrOp::EndBorrow(v) => write!(f, "end.borrow {}", v),
            // Struct operations
            IrOp::StructNew(d, id) => write!(f, "{} = struct.new {}", d, id),
            IrOp::StructGetField(d, src, field) => {
                write!(f, "{} = struct.get {}, {}", d, src, field)
            }
            IrOp::StructSetField(dst, field, val) => {
                write!(f, "struct.set {}, {}, {}", dst, field, val)
            }
            IrOp::StructGetFieldNamed(d, src, name) => {
                write!(f, "{} = struct.get {}, .{}", d, src, name)
            }
            IrOp::StructSetFieldNamed(dst, name, val) => {
                write!(f, "struct.set {}, .{}, {}", dst, name, val)
            }
            // Monomorphized calls
            IrOp::CallMono(d, mono_id, args) => {
                let args_str: Vec<_> = args.iter().map(|a| format!("{}", a)).collect();
                write!(f, "{} = call.mono {}({})", d, mono_id, args_str.join(", "))
            }
            // Move operations
            IrOp::Move(d, s) => write!(f, "{} = move {}", d, s),
            IrOp::Clone(d, s) => write!(f, "{} = clone {}", d, s),
            // Type operations
            IrOp::TypeOf(d, v) => write!(f, "{} = typeof {}", d, v),
            IrOp::DeleteProp(d, obj, prop) => write!(f, "{} = delete {}.{}", d, obj, prop),
        }
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Terminator::Jump(target) => write!(f, "jump {}", target),
            Terminator::Branch(cond, t, fa) => write!(f, "branch {}, {}, {}", cond, t, fa),
            Terminator::Return(Some(v)) => write!(f, "return {}", v),
            Terminator::Return(None) => write!(f, "return"),
            Terminator::Unreachable => write!(f, "unreachable"),
        }
    }
}

impl fmt::Display for BasicBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.id)?;
        for op in &self.ops {
            writeln!(f, "    {}", op)?;
        }
        writeln!(f, "    {}", self.terminator)?;
        Ok(())
    }
}

impl fmt::Display for IrFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Function signature
        let params_str: Vec<_> = self
            .params
            .iter()
            .map(|(name, ty)| format!("{}: {}", name, ty))
            .collect();
        writeln!(
            f,
            "fn {}({}) -> {} {{",
            self.name,
            params_str.join(", "),
            self.return_ty
        )?;

        // Locals
        if !self.locals.is_empty() {
            for (i, (name, ty)) in self.locals.iter().enumerate() {
                writeln!(f, "    local ${}: {} = {}", i, ty, name)?;
            }
            writeln!(f)?;
        }

        // Blocks
        for block in &self.blocks {
            write!(f, "{}", block)?;
        }

        writeln!(f, "}}")?;
        Ok(())
    }
}

impl fmt::Display for IrModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for func in &self.functions {
            writeln!(f, "{}", func)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_function() {
        let mut func = IrFunction::new("add".to_string());
        func.params.push(("a".to_string(), IrType::Number));
        func.params.push(("b".to_string(), IrType::Number));
        func.return_ty = IrType::Number;

        let entry = func.alloc_block();
        let a = func.alloc_value(IrType::Number);
        let b = func.alloc_value(IrType::Number);
        let result = func.alloc_value(IrType::Number);

        func.add_local("a".to_string(), IrType::Number);
        func.add_local("b".to_string(), IrType::Number);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::LoadLocal(a, 0));
            block.push(IrOp::LoadLocal(b, 1));
            block.push(IrOp::AddNum(result, a, b));
            block.terminate(Terminator::Return(Some(result)));
        }

        let ir_str = format!("{}", func);
        assert!(ir_str.contains("fn add"));
        assert!(ir_str.contains("add.num"));
        assert!(ir_str.contains("return"));
    }

    #[test]
    fn test_control_flow() {
        let mut func = IrFunction::new("abs".to_string());
        func.params.push(("x".to_string(), IrType::Number));
        func.return_ty = IrType::Number;

        let entry = func.alloc_block();
        let then_block = func.alloc_block();
        let else_block = func.alloc_block();

        let x = func.alloc_value(IrType::Number);
        let zero = func.alloc_value(IrType::Number);
        let cond = func.alloc_value(IrType::Boolean);
        let neg_x = func.alloc_value(IrType::Number);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::LoadLocal(x, 0));
            block.push(IrOp::Const(zero, Literal::Number(0.0)));
            block.push(IrOp::Lt(cond, x, zero));
            block.terminate(Terminator::Branch(cond, then_block, else_block));
        }

        {
            let block = func.block_mut(then_block);
            block.push(IrOp::NegNum(neg_x, x));
            block.terminate(Terminator::Return(Some(neg_x)));
        }

        {
            let block = func.block_mut(else_block);
            block.terminate(Terminator::Return(Some(x)));
        }

        func.compute_predecessors();

        assert_eq!(func.block(then_block).predecessors.len(), 1);
        assert_eq!(func.block(else_block).predecessors.len(), 1);
    }
}
