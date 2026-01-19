//! Type Error Definitions
//!
//! Error types for type checking, inference, and borrow checking.

use std::fmt;

use super::Type;

/// Source code location span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub fn new(start: u32, end: u32, line: u32, col: u32) -> Self {
        Self {
            start,
            end,
            line,
            col,
        }
    }

    pub fn from_range(start: u32, end: u32) -> Self {
        Self {
            start,
            end,
            line: 0,
            col: 0,
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.col + 1)
    }
}

/// Borrow kind for conflict reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowKind {
    Immutable,
    Mutable,
}

impl fmt::Display for BorrowKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BorrowKind::Immutable => write!(f, "immutable"),
            BorrowKind::Mutable => write!(f, "mutable"),
        }
    }
}

/// Type checking errors.
#[derive(Debug, Clone)]
pub enum TypeError {
    /// Type mismatch: expected X, got Y.
    Mismatch {
        expected: Type,
        got: Type,
        span: Span,
    },

    /// Undefined variable.
    UndefinedVariable { name: String, span: Span },

    /// Undefined type.
    UndefinedType { name: String, span: Span },

    /// Trying to call a non-function.
    NotCallable { ty: Type, span: Span },

    /// Wrong number of arguments.
    WrongArgCount {
        expected: usize,
        got: usize,
        span: Span,
    },

    /// Cannot infer type.
    CannotInfer { span: Span },

    /// Using a value after it has been moved.
    UseAfterMove {
        var: String,
        moved_at: Span,
        used_at: Span,
    },

    /// Borrow conflict (e.g., mutable borrow while immutable borrow exists).
    BorrowConflict {
        var: String,
        existing: BorrowKind,
        new: BorrowKind,
        span: Span,
    },

    /// Borrow outlives the borrowed value.
    BorrowOutlives {
        var: String,
        borrow_span: Span,
        end_span: Span,
    },

    /// Assigning to immutable variable.
    ImmutableAssignment { var: String, span: Span },

    /// Field not found on type.
    FieldNotFound { ty: Type, field: String, span: Span },

    /// Index operation on non-array.
    NotIndexable { ty: Type, span: Span },

    /// Binary operation not supported for types.
    InvalidBinaryOp {
        op: String,
        left: Type,
        right: Type,
        span: Span,
    },

    /// Unary operation not supported for type.
    InvalidUnaryOp { op: String, ty: Type, span: Span },

    /// Cannot assign to expression.
    NotAssignable { span: Span },

    /// Type parameter count mismatch.
    TypeArgCountMismatch {
        expected: usize,
        got: usize,
        span: Span,
    },

    /// Recursive type without indirection.
    RecursiveType { name: String, span: Span },

    /// Generic inference failure.
    CannotInferTypeArg { param_name: String, span: Span },

    /// Return type mismatch.
    ReturnTypeMismatch {
        expected: Type,
        got: Type,
        span: Span,
    },

    /// Missing return statement.
    MissingReturn { expected: Type, span: Span },

    /// Unreachable code.
    UnreachableCode { span: Span },

    /// Duplicate field.
    DuplicateField { name: String, span: Span },

    /// Duplicate type parameter.
    DuplicateTypeParam { name: String, span: Span },

    /// Unsupported type syntax.
    UnsupportedType { description: String, span: Span },
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeError::Mismatch {
                expected,
                got,
                span,
            } => {
                write!(
                    f,
                    "type mismatch at {}: expected {}, got {}",
                    span, expected, got
                )
            }
            TypeError::UndefinedVariable { name, span } => {
                write!(f, "undefined variable '{}' at {}", name, span)
            }
            TypeError::UndefinedType { name, span } => {
                write!(f, "undefined type '{}' at {}", name, span)
            }
            TypeError::NotCallable { ty, span } => {
                write!(f, "type '{}' is not callable at {}", ty, span)
            }
            TypeError::WrongArgCount {
                expected,
                got,
                span,
            } => {
                write!(
                    f,
                    "wrong number of arguments at {}: expected {}, got {}",
                    span, expected, got
                )
            }
            TypeError::CannotInfer { span } => {
                write!(f, "cannot infer type at {}", span)
            }
            TypeError::UseAfterMove {
                var,
                moved_at,
                used_at,
            } => {
                write!(
                    f,
                    "use of moved value '{}' at {} (moved at {})",
                    var, used_at, moved_at
                )
            }
            TypeError::BorrowConflict {
                var,
                existing,
                new,
                span,
            } => {
                write!(
                    f,
                    "cannot borrow '{}' as {} at {}: already borrowed as {}",
                    var, new, span, existing
                )
            }
            TypeError::BorrowOutlives {
                var,
                borrow_span,
                end_span,
            } => {
                write!(
                    f,
                    "borrow of '{}' at {} outlives its scope ending at {}",
                    var, borrow_span, end_span
                )
            }
            TypeError::ImmutableAssignment { var, span } => {
                write!(
                    f,
                    "cannot assign to immutable variable '{}' at {}",
                    var, span
                )
            }
            TypeError::FieldNotFound { ty, field, span } => {
                write!(
                    f,
                    "field '{}' not found on type '{}' at {}",
                    field, ty, span
                )
            }
            TypeError::NotIndexable { ty, span } => {
                write!(f, "type '{}' is not indexable at {}", ty, span)
            }
            TypeError::InvalidBinaryOp {
                op,
                left,
                right,
                span,
            } => {
                write!(
                    f,
                    "invalid binary operation '{}' between '{}' and '{}' at {}",
                    op, left, right, span
                )
            }
            TypeError::InvalidUnaryOp { op, ty, span } => {
                write!(
                    f,
                    "invalid unary operation '{}' on '{}' at {}",
                    op, ty, span
                )
            }
            TypeError::NotAssignable { span } => {
                write!(f, "expression is not assignable at {}", span)
            }
            TypeError::TypeArgCountMismatch {
                expected,
                got,
                span,
            } => {
                write!(
                    f,
                    "type argument count mismatch at {}: expected {}, got {}",
                    span, expected, got
                )
            }
            TypeError::RecursiveType { name, span } => {
                write!(
                    f,
                    "recursive type '{}' without indirection at {}",
                    name, span
                )
            }
            TypeError::CannotInferTypeArg { param_name, span } => {
                write!(f, "cannot infer type argument '{}' at {}", param_name, span)
            }
            TypeError::ReturnTypeMismatch {
                expected,
                got,
                span,
            } => {
                write!(
                    f,
                    "return type mismatch at {}: expected {}, got {}",
                    span, expected, got
                )
            }
            TypeError::MissingReturn { expected, span } => {
                write!(
                    f,
                    "missing return statement for type '{}' at {}",
                    expected, span
                )
            }
            TypeError::UnreachableCode { span } => {
                write!(f, "unreachable code at {}", span)
            }
            TypeError::DuplicateField { name, span } => {
                write!(f, "duplicate field '{}' at {}", name, span)
            }
            TypeError::DuplicateTypeParam { name, span } => {
                write!(f, "duplicate type parameter '{}' at {}", name, span)
            }
            TypeError::UnsupportedType { description, span } => {
                write!(f, "unsupported type {} at {}", description, span)
            }
        }
    }
}

impl std::error::Error for TypeError {}

/// Collection of type errors.
#[derive(Debug, Default)]
pub struct TypeErrors {
    pub errors: Vec<TypeError>,
}

impl TypeErrors {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn len(&self) -> usize {
        self.errors.len()
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TypeError> {
        self.errors.iter()
    }
}

impl fmt::Display for TypeErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, error) in self.errors.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "error: {}", error)?;
        }
        Ok(())
    }
}

impl std::error::Error for TypeErrors {}

impl IntoIterator for TypeErrors {
    type Item = TypeError;
    type IntoIter = std::vec::IntoIter<TypeError>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

impl<'a> IntoIterator for &'a TypeErrors {
    type Item = &'a TypeError;
    type IntoIter = std::slice::Iter<'a, TypeError>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.iter()
    }
}
