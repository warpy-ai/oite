//! Signatures of the native runtime's functions (`src/runtime/stubs.rs`).
//!
//! Both native backends declare the runtime from this one table, so a
//! signature cannot drift between them. Each entry must match the Rust
//! definition exactly.

/// Argument / return kinds in a runtime signature.
#[derive(Clone, Copy)]
pub enum Ty {
    /// A NaN-boxed value (`u64`).
    Val,
    /// A pointer.
    Ptr,
    /// A `usize` length or count.
    Len,
}

/// Every runtime function native code may call: (name, returns a value?, params).
pub const RUNTIME: &[(&str, bool, &[Ty])] = {
    use Ty::*;
    &[
        ("ot_alloc_object", true, &[]),
        ("ot_alloc_array", true, &[Len]),
        ("ot_alloc_string", true, &[Ptr, Len]),
        ("ot_make_function", true, &[Ptr]),
        ("ot_make_closure", true, &[Ptr, Val]),
        ("ot_get_prop", true, &[Val, Ptr, Len]),
        ("ot_set_prop", true, &[Val, Ptr, Len, Val]),
        ("ot_get_index", true, &[Val, Val]),
        ("ot_set_index", true, &[Val, Val, Val]),
        ("ot_delete_prop", true, &[Val, Ptr, Len]),
        ("ot_array_len", true, &[Val]),
        ("ot_array_push", true, &[Val, Val]),
        ("ot_get_global", true, &[Ptr, Len]),
        ("ot_typeof_global", true, &[Ptr, Len]),
        ("ot_set_global", true, &[Ptr, Len, Val]),
        ("ot_call", true, &[Val, Val, Ptr, Len]),
        ("ot_call_method", true, &[Val, Ptr, Len, Ptr, Len]),
        ("ot_intrinsic", true, &[Len, Ptr, Len]),
        ("ot_math_call", true, &[Len, Ptr, Len]),
        ("ot_array_spread", true, &[Val, Val]),
        ("ot_object_spread", true, &[Val, Val]),
        ("ot_add_any", true, &[Val, Val]),
        ("ot_sub_any", true, &[Val, Val]),
        ("ot_mul_any", true, &[Val, Val]),
        ("ot_div_any", true, &[Val, Val]),
        ("ot_mod_any", true, &[Val, Val]),
        ("ot_pow", true, &[Val, Val]),
        ("ot_neg", true, &[Val]),
        ("ot_bit_and", true, &[Val, Val]),
        ("ot_bit_or", true, &[Val, Val]),
        ("ot_bit_xor", true, &[Val, Val]),
        ("ot_shl", true, &[Val, Val]),
        ("ot_shr", true, &[Val, Val]),
        ("ot_shr_u", true, &[Val, Val]),
        ("ot_eq_strict", true, &[Val, Val]),
        ("ot_eq_loose", true, &[Val, Val]),
        ("ot_lt", true, &[Val, Val]),
        ("ot_gt", true, &[Val, Val]),
        ("ot_lte", true, &[Val, Val]),
        ("ot_gte", true, &[Val, Val]),
        ("ot_not", true, &[Val]),
        ("ot_truthy", true, &[Val]),
        ("ot_typeof", true, &[Val]),
        ("ot_to_boolean", true, &[Val]),
        ("ot_to_number", true, &[Val]),
        ("ot_runtime_exit", false, &[]),
        ("ot_throw", true, &[Val]),
        ("ot_take_exception", true, &[]),
        ("ot_rethrow", true, &[Val, Val]),
        ("ot_report_uncaught", false, &[]),
        ("ot_construct", true, &[Val, Ptr, Len]),
        ("ot_call_super", true, &[Val, Val, Ptr, Len]),
        ("ot_get_super_prop", true, &[Val, Val, Ptr, Len]),
        ("ot_instanceof", true, &[Val, Val]),
        ("ot_get_private", true, &[Val, Ptr, Len]),
        ("ot_to_str", true, &[Val]),
        ("ot_in", true, &[Val, Val]),
        ("ot_stack_init", false, &[]),
        ("ot_stack_overflow", true, &[]),
        ("ot_set_private", true, &[Val, Ptr, Len, Val]),
        ("ot_async_start", true, &[]),
        ("ot_async_resolve", true, &[Val, Val]),
        ("ot_async_reject", true, &[Val, Val]),
        ("ot_async_enter", true, &[Ptr, Len, Len]),
        ("ot_await", true, &[Val, Val, Val, Val, Val]),
        ("ot_await_result", true, &[Val]),
        ("ot_run_microtasks", false, &[]),
        ("ot_run_event_loop", false, &[]),
        ("ot_trace", false, &[Ptr]),
        ("ot_rest_args", true, &[Ptr, Len, Len]),
    ]
};
