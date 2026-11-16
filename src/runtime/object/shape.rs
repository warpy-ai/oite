//! Shapes (hidden classes): the layout shared by objects that got the same
//! properties in the same order. A shape is immutable and lives for the
//! whole run; adding a property moves an object to the next shape.

use super::super::heap::MAX_INLINE_SLOTS;
use super::atom::Atom;
use std::cell::RefCell;

/// The property is an accessor: its slot holds a getter and setter pair
/// (`object::accessor`).
pub const ACCESSOR: u8 = 1;

/// An object with more properties than this becomes a dictionary.
pub const MAX_PROPS: usize = 64;

#[derive(Clone, Copy)]
pub struct Prop {
    pub atom: Atom,
    pub flags: u8,
}

pub struct Shape {
    /// How many slots objects of this shape store in themselves. Objects
    /// of one shape agree on it, so a slot's place is a property of the
    /// shape.
    pub inline_slots: u8,
    /// The properties in insertion order: property `i` is in slot `i`.
    pub props: Vec<Prop>,
    /// The shapes reached by adding one property.
    transitions: RefCell<Vec<(Atom, u8, *const Shape)>>,
}

impl Shape {
    fn new(inline_slots: u8, props: Vec<Prop>) -> *const Shape {
        Box::leak(Box::new(Shape {
            inline_slots,
            props,
            transitions: RefCell::new(Vec::new()),
        }))
    }

    /// The slot and flags of `atom`.
    pub fn find(&self, atom: Atom) -> Option<(usize, u8)> {
        self.props
            .iter()
            .position(|p| p.atom == atom)
            .map(|i| (i, self.props[i].flags))
    }

    /// The shape with `atom` added in the next slot.
    pub fn with(&self, atom: Atom, flags: u8) -> *const Shape {
        let mut transitions = self.transitions.borrow_mut();
        if let Some((_, _, s)) = transitions
            .iter()
            .find(|(a, f, _)| *a == atom && *f == flags)
        {
            return *s;
        }
        let mut props = self.props.clone();
        props.push(Prop { atom, flags });
        let next = Shape::new(self.inline_slots, props);
        transitions.push((atom, flags, next));
        next
    }
}

thread_local! {
    static EMPTY: [*const Shape; MAX_INLINE_SLOTS + 1] =
        std::array::from_fn(|slots| Shape::new(slots as u8, Vec::new()));
}

/// The shape of an object with no properties and `inline_slots` slots in
/// the object itself.
pub fn empty(inline_slots: usize) -> *const Shape {
    EMPTY.with(|s| s[inline_slots.min(MAX_INLINE_SLOTS)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_order_same_shape() {
        assert_ne!(empty(2), empty(4));
        let e = unsafe { &*empty(4) };
        let a = e.with(100, 0);
        let ab = unsafe { &*a }.with(101, 0);
        assert_eq!(e.with(100, 0), a);
        assert_eq!(unsafe { &*a }.with(101, 0), ab);
        // Another order, or other flags, is another shape.
        let b = e.with(101, 0);
        assert_ne!(unsafe { &*b }.with(100, 0), ab);
        assert_ne!(e.with(100, ACCESSOR), a);
        assert_eq!(unsafe { &*ab }.find(101), Some((1, 0)));
        assert_eq!(unsafe { &*ab }.find(102), None);
    }
}
