//! Runtime stubs callable from native-compiled code
//!
//! These `extern "C"` functions are the runtime of Oite's native backends.
//! The Cranelift JIT calls them in-process; LLVM AOT binaries link them from
//! the static library that `build.rs` compiles from these same files. Keeping
//! the semantics here, instead of in each backend, is what makes the two
//! backends agree.
//!
//! The calling convention is:
//! - All values are passed and returned as `u64` (NaN-boxed `OtValue`).
//! - Every function value, compiled or built in, has the signature
//!   `extern "C" fn(this: u64, argv: *const u64, argc: usize) -> u64`
//!   (see `heap::NativeFn`). Missing arguments read as `undefined`.
//! - Names (properties, globals) are passed as a UTF-8 pointer and length.

use super::abi::{EXCEPTION_BITS, OtValue, UNBOUND_BITS};
use super::builtins;
use super::heap::{
    HeapPtr, NativeArray, NativeFn, NativeFunction, NativeObject, NativeString, ObjectHeader,
    ObjectKind, heap,
};
use super::object::atom::{self, Atom, Role};
use super::object::object;
use super::object::shape::ACCESSOR;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;

mod async_fns;
mod builtin_fns;
mod calls;
mod classes;
mod exceptions;
mod fs;
mod globals;
mod inspect;
mod objects;
mod operators;
mod promises;
mod values;

pub use async_fns::*;
pub use builtin_fns::*;
pub use calls::*;
pub use classes::*;
pub use exceptions::*;
// Not in the runtime ABI table: nothing in `oitec` itself calls these.
#[allow(unused_imports)]
pub use fs::*;
pub use globals::*;
pub use inspect::*;
pub use objects::*;
pub use operators::*;
pub use promises::*;
pub use values::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_log() {
        ot_console_log(OtValue::number(42.0).to_bits());
        ot_console_log(OtValue::boolean(true).to_bits());
        ot_console_log(OtValue::null().to_bits());
    }

    #[test]
    fn test_arithmetic_stubs() {
        let a = OtValue::number(10.0).to_bits();
        let b = OtValue::number(3.0).to_bits();

        assert_eq!(OtValue::from_bits(ot_add_any(a, b)).as_number(), Some(13.0));
        assert_eq!(OtValue::from_bits(ot_sub_any(a, b)).as_number(), Some(7.0));
        assert_eq!(OtValue::from_bits(ot_mul_any(a, b)).as_number(), Some(30.0));

        let div = OtValue::from_bits(ot_div_any(a, b)).as_number().unwrap();
        assert!((div - 3.333333).abs() < 0.001);
    }

    #[test]
    fn test_object_property() {
        let obj_bits = ot_alloc_object();
        assert!(!OtValue::from_bits(obj_bits).is_undefined());

        let key = "foo";
        let value = OtValue::number(42.0).to_bits();

        ot_set_prop(obj_bits, key.as_ptr(), key.len(), value);
        let retrieved = ot_get_prop(obj_bits, key.as_ptr(), key.len());

        assert_eq!(OtValue::from_bits(retrieved).as_number(), Some(42.0));
    }
}
