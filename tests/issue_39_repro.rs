//! Reproduction for https://github.com/warpy-ai/oite/issues/39
//! Compiling a Node/TypeScript entrypoint reported:
//!   BORROW ERROR: Cannot borrow moved variable 'process_path'

use oite::compiler::Compiler;

/// The real aphorio lib/index.ts (github.com/prettydiff/aphorio).
const APHORIO_INDEX: &str = include_str!("fixtures/aphorio_index.ts");

#[test]
fn issue_39_process_path_is_not_moved() {
    let mut compiler = Compiler::new();
    let result = compiler.compile(APHORIO_INDEX);
    assert!(result.is_ok(), "expected clean compile, got: {:?}", result);
}
