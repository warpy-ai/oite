//| Lifetime constraints system
// |
// | Gemerate and represents constraints between lifetimes for validation
//

use super::LifetimeId;
use swc_common::Span;

#[derive(Debug, Clone)]
pub enum LifetimeConstraint {
    Outlives {
        longer: LifetimeId,
        shorter: LifetimeId,
        span: Span,
    },

    ValidAt {
        lifetime: LifetimeId,
        point: ProgramPoint,
        span: Span,
    },

    Equal {
        a: LifetimeId,
        b: LifetimeId,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProgramPoint {
    pub scope_depth: u32,
    pub statement_index: u32,
}

impl ProgramPoint {
    pub fn new(scope_depth: u32, statement_index: u32) -> Self {
        Self {
            scope_depth,
            statement_index,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConstraintSet {
    constraints: Vec<LifetimeConstraint>,
}

impl ConstraintSet {
    pub fn new() -> Self {
        Self {
            constraints: Vec::new(),
        }
    }

    pub fn add(&mut self, constraint: LifetimeConstraint) {
        self.constraints.push(constraint);
    }

    pub fn add_outlives(&mut self, longer: LifetimeId, shorter: LifetimeId, span: Span) {
        self.add(LifetimeConstraint::Outlives {
            longer,
            shorter,
            span,
        });
    }

    pub fn add_valid_at(&mut self, lifetime: LifetimeId, point: ProgramPoint, span: Span) {
        self.add(LifetimeConstraint::ValidAt {
            lifetime,
            point,
            span,
        });
    }

    pub fn add_equal(&mut self, a: LifetimeId, b: LifetimeId, span: Span) {
        self.add(LifetimeConstraint::Equal { a, b, span });
    }

    pub fn constraints(&self) -> &[LifetimeConstraint] {
        &self.constraints
    }

    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty()
    }

    pub fn len(&self) -> usize {
        self.constraints.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraint_set_creation() {
        let set = ConstraintSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_add_outlives() {
        let mut set = ConstraintSet::new();
        let a = LifetimeId(1);
        let b = LifetimeId(2);
        set.add_outlives(a, b, Span::default());
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_add_valid_at() {
        let mut set = ConstraintSet::new();
        let a = LifetimeId(1);
        let point = ProgramPoint::new(0, 0);
        set.add_valid_at(a, point, Span::default());
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_add_equal() {
        let mut set = ConstraintSet::new();
        let a = LifetimeId(1);
        let b = LifetimeId(2);
        set.add_equal(a, b, Span::default());
        assert_eq!(set.len(), 1);
    }
}
