//! Dictionary mode: the properties of an object used as a hash table (many
//! properties, or deletions). Keeps insertion order.

use super::atom::Atom;
use std::collections::HashMap;

#[derive(Default)]
pub struct Dict {
    /// Insertion order; a removed entry leaves `None`.
    entries: Vec<Option<(Atom, u8, u64)>>,
    index: HashMap<Atom, usize>,
}

impl Dict {
    pub fn get(&self, atom: Atom) -> Option<(u64, u8)> {
        let (_, flags, value) = self.entries[*self.index.get(&atom)?]?;
        Some((value, flags))
    }

    /// Add `atom`, or replace its value and flags in place.
    pub fn set(&mut self, atom: Atom, flags: u8, value: u64) {
        match self.index.get(&atom) {
            Some(i) => self.entries[*i] = Some((atom, flags, value)),
            None => {
                self.index.insert(atom, self.entries.len());
                self.entries.push(Some((atom, flags, value)));
            }
        }
    }

    pub fn remove(&mut self, atom: Atom) -> bool {
        match self.index.remove(&atom) {
            Some(i) => {
                self.entries[i] = None;
                true
            }
            None => false,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (Atom, u8, u64)> + '_ {
        self.entries.iter().flatten().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_insertion_order() {
        let mut d = Dict::default();
        d.set(3, 0, 30);
        d.set(1, 0, 10);
        d.set(2, 0, 20);
        d.set(1, 0, 11);
        assert!(d.remove(3));
        assert!(!d.remove(3));
        d.set(3, 0, 31);
        let all: Vec<_> = d.iter().map(|(a, _, v)| (a, v)).collect();
        assert_eq!(all, vec![(1, 11), (2, 20), (3, 31)]);
        assert_eq!(d.get(2), Some((20, 0)));
        assert_eq!(d.get(9), None);
    }
}
