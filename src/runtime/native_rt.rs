//! Crate root for the native runtime library.
//!
//! `build.rs` compiles this file with `rustc --crate-type staticlib` into the
//! library that `oitec build` links into every native executable. It is never
//! a module of the `oite` crate itself.
//!
//! It includes the same source files as `crate::runtime` (`abi.rs`, `heap.rs`,
//! `stubs.rs`), so the Cranelift JIT, which calls those functions in-process,
//! and LLVM AOT binaries run exactly the same runtime code. Those files only
//! refer to each other through `super::`, which here resolves to this root.

#![allow(dead_code)]
#![allow(clippy::all)]

#[path = "abi.rs"]
pub mod abi;
#[path = "heap.rs"]
pub mod heap;
#[path = "stubs.rs"]
pub mod stubs;
