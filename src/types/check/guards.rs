//! Where a value must be checked when the program runs
//! (docs/docs/types.md, section 3).
//!
//! The checker trusts what it has proved. What it cannot prove is checked
//! when the program runs: a dynamic value that enters a typed place, a
//! typed parameter (its function may be called from dynamic code), a
//! value read from the heap, the result of a call, and a cast.
//!
//! A check is shallow: the kind of the value (`runtime::builtins::Kind`),
//! not what is inside it. An array's elements and an object's properties
//! are checked when they are read. A class type is checked as an object:
//! instances do not carry their class in a form compiled code can test
//! until class layouts (section 7).

use super::*;
use crate::runtime::builtins::Kind;

/// The spans of the expressions and parameters to check, with the kinds
/// each accepts (a set of `Kind::bit`).
pub type Guards = HashMap<(u32, u32), u32>;

const OBJECTS: u32 = Kind::Object.bit() | Kind::Array.bit() | Kind::Function.bit();

impl Checker {
    /// The kinds a value of type `ty` can have; `None` if it can be any
    /// (the type is dynamic, or says nothing a kind can tell).
    pub(super) fn kinds(&self, ty: &Ty) -> Option<u32> {
        self.kinds_at(ty, 0)
    }

    fn kinds_at(&self, ty: &Ty, depth: usize) -> Option<u32> {
        if depth > 12 {
            return None;
        }
        match self.expand(ty) {
            Ty::Any | Ty::Unknown | Ty::Never | Ty::Void => None,
            Ty::Number => Some(Kind::Number.bit()),
            Ty::String | Ty::StrLit(_) => Some(Kind::String.bit()),
            Ty::Boolean => Some(Kind::Boolean.bit()),
            Ty::Null => Some(Kind::Null.bit()),
            Ty::Undefined => Some(Kind::Undefined.bit()),
            Ty::Array(_) | Ty::Tuple(_) => Some(Kind::Array.bit()),
            Ty::Function(_) | Ty::ClassValue(_) => Some(Kind::Function.bit()),
            Ty::Named(id) if self.class(id).is_some() => Some(Kind::Object.bit()),
            // An object type is structural: an array or a function may
            // have the properties it asks for.
            Ty::Object(_) | Ty::Named(_) => Some(OBJECTS),
            Ty::Union(members) => {
                let mut all = 0;
                for m in members.iter() {
                    all |= self.kinds_at(m, depth + 1)?;
                }
                Some(all)
            }
        }
    }

    /// The value of the expression (or the parameter) at `span` must be
    /// a `ty` when the program runs.
    pub(super) fn guard(&mut self, span: Span, ty: &Ty) {
        let Some(kinds) = self.kinds(ty) else { return };
        // Two contracts on one expression: both hold.
        self.guards
            .entry((span.lo.0, span.hi.0))
            .and_modify(|k| *k &= kinds)
            .or_insert(kinds);
    }

    /// A value of type `src` enters a place declared as `dst`: checked
    /// when the program runs if the checker does not know what it is.
    pub(super) fn guard_flow(&mut self, src: &Ty, dst: &Ty, span: Span) {
        if src.is_dynamic() {
            self.guard(span, dst);
        }
    }

    /// The value of the expression at `span` is proved to be a `ty`.
    /// Recorded when that is one kind compiled code makes use of: a
    /// number, a boolean or a string.
    pub(super) fn proved(&mut self, span: Span, ty: &Ty) {
        let simple = [Kind::Number, Kind::Boolean, Kind::String].map(Kind::bit);
        if let Some(kinds) = self.kinds(ty)
            && simple.contains(&kinds)
        {
            self.facts.insert((span.lo.0, span.hi.0), kinds);
        }
    }

    /// The expression at `span` is not evaluated as a value (a callee, the
    /// target of an assignment): nothing to check there.
    pub(super) fn unguard(&mut self, span: Span) {
        self.guards.remove(&(span.lo.0, span.hi.0));
    }
}
