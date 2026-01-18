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
pub mod heap;
pub mod stubs;

pub use abi::TsclValue;
pub use heap::{NativeHeap, HeapPtr};
