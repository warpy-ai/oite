//! Library target for building runtime as static library
//!
//! This file exists solely to allow building the runtime as a static library
//! via `cargo rustc --lib --crate-type=staticlib`. The binary target (main.rs)
//! is built separately and doesn't depend on this.

// Suppress some clippy lints for this crate
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(clippy::redundant_slicing)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::type_complexity)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::module_inception)]
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::uninit_vec)]
#![allow(clippy::only_used_in_recursion)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::useless_conversion)]
#![allow(clippy::assertions_on_constants)]
#![allow(clippy::approx_constant)]
#![allow(clippy::unnecessary_cast)]
#![allow(clippy::redundant_clone)]
#![allow(clippy::wrong_self_convention)]
#![allow(clippy::field_reassign_with_default)]

// When vm_interop is enabled, include all modules for full functionality
#[cfg(feature = "vm_interop")]
pub mod backend;
#[cfg(feature = "vm_interop")]
pub mod build;
#[cfg(feature = "vm_interop")]
pub mod compiler;
#[cfg(feature = "vm_interop")]
pub mod ir;
#[cfg(feature = "vm_interop")]
pub mod loader;
#[cfg(feature = "vm_interop")]
pub mod stdlib;
#[cfg(feature = "vm_interop")]
pub mod types;
#[cfg(feature = "vm_interop")]
pub mod vm;

// Runtime is always included (it's needed for staticlib)
pub mod runtime;

// Re-export runtime for static library builds
pub use runtime::*;
