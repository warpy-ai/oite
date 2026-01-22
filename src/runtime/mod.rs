//! Runtime kernel for native code execution
//!
//! This module provides the foundational runtime primitives that native-compiled
//! tscl code calls into. It separates:
//! - Memory allocation and GC (heap.rs)
//! - Value representation for native interop (abi.rs)
//! - Extern "C" stubs callable from JIT/AOT code (stubs.rs)
//!
//! The VM interpreter continues to use JsValue/HeapObject for backwards compatibility.
//! Native code uses TsclValue (NaN-boxed) for efficient representation.

pub mod abi;
pub mod abi_tests;
pub mod abi_version;
pub mod heap;
pub mod stubs;
pub mod r#async;
pub mod http;

pub use abi::TsclValue;
pub use abi_version::ABI_VERSION;
pub use heap::{HeapPtr, NativeHeap};

// Provide rust_eh_personality for panic handling when linking standalone
#[cfg(not(feature = "vm_interop"))]
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality(
    _version: u32,
    _actions: u32,
    _exception_class: u64,
    _exception_object: *mut std::ffi::c_void,
    _context: *mut std::ffi::c_void,
) -> u32 {
    // Minimal panic personality function for standalone builds
    // Returns 0 to indicate we can't handle the exception (abort on panic)
    0
}
