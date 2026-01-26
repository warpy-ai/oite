//! Library target for building runtime as static library
//!
//! This file exists solely to allow building the runtime as a static library
//! via `cargo rustc --lib --crate-type=staticlib`. The binary target (main.rs)
//! is built separately and doesn't depend on this.

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
