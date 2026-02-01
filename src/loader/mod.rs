//! Bytecode loader module for loading pre-compiled .bc files
//!
//! This module provides functionality to decode binary bytecode files
//! produced by the bootstrap compiler (bootstrap/emitter.ot).

mod decoder;

pub use decoder::BytecodeDecoder;
