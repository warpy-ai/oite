//! The native object model (docs/docs/object-model.md): property names as
//! atoms, shapes shared by objects built the same way, and a dictionary
//! for objects used as hash tables.

pub mod atom;
pub mod dictionary;
#[allow(clippy::module_inception)]
pub mod object;
pub mod shape;
