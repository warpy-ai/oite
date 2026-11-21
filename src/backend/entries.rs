//! The entry points of a compiled function, as both backends build them.
//!
//! Every function has the dynamic entry `i64 (this, argv, argc, env)`,
//! which is what a function object holds. A function that never looks at
//! its argument array also has a fast entry, `i64 (this, arg0 .. argN,
//! env)` with `N` the number of parameters it declares: its code is
//! compiled into the fast entry, and the dynamic entry unpacks the array
//! and calls it. A call that knows which function it calls uses the fast
//! entry, passing `undefined` for a missing argument and dropping an
//! extra one (the function could not have seen it).

use crate::ir::{IrFunction, IrOp};

/// More parameters than this, and a function keeps only its dynamic
/// entry.
pub const MAX_REGISTER_PARAMS: usize = 8;

/// Whether `func` can take its arguments in registers: it never looks at
/// the argument array itself (a rest parameter, forwarding every argument
/// to `super`, an async function's frame).
pub fn takes_registers(func: &IrFunction) -> bool {
    if func.name == "main" || func.is_async || func.params.len() > MAX_REGISTER_PARAMS {
        return false;
    }
    !func.blocks.iter().flat_map(|b| &b.ops).any(|op| {
        matches!(
            op,
            IrOp::RestArgs(_, _) | IrOp::SuperForward(_, _) | IrOp::IsResume(_)
        )
    })
}
