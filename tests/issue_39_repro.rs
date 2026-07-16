//! Reproduction + regression probes for https://github.com/warpy-ai/oite/issues/39
//! Reported: BORROW ERROR: Cannot borrow moved variable 'process_path'

use oite::compiler::Compiler;

/// The real aphorio lib/index.ts (github.com/prettydiff/aphorio).
const APHORIO_INDEX: &str = include_str!("fixtures/aphorio_index.ts");

fn compile(src: &str) -> Result<(), String> {
    Compiler::new().compile(src).map(|_| ())
}

#[test]
fn issue_39_process_path_is_not_moved() {
    let mut compiler = Compiler::new();
    let result = compiler.compile(APHORIO_INDEX);
    assert!(result.is_ok(), "expected clean compile, got: {:?}", result);
}

/// Passing a variable to a function borrows it only for that statement, so a
/// later assignment must not report "assign while borrowed".
#[test]
fn borrow_is_released_after_call() {
    let r = compile(
        r#"
        let p = "";
        foo(p);
        p = "b";
        "#,
    );
    assert!(r.is_ok(), "assign after call should be fine, got: {:?}", r);
}

/// Repeated reads across statements must not accumulate borrows.
#[test]
fn repeated_reads_then_assign() {
    let r = compile(
        r#"
        let p = "";
        foo(p);
        bar(p);
        baz(p.length);
        p = "b";
        "#,
    );
    assert!(r.is_ok(), "repeated reads should be fine, got: {:?}", r);
}

/// Same shape as aphorio: read in an expression, then pass to a function.
#[test]
fn read_then_pass_to_function() {
    let r = compile(
        r#"
        let p = "";
        let q = cond ? `${p}test` : p;
        start(p);
        "#,
    );
    assert!(r.is_ok(), "read-then-pass should be fine, got: {:?}", r);
}

/// aphorio mutates `process_path` inside a do/while. Statement kinds the
/// checker skips entirely hide real code from analysis.
#[test]
fn do_while_body_is_analyzed() {
    let r = compile(
        r#"
        let p = "";
        do {
            p = p.replace("a", "b");
        } while (i > 0);
        start(p);
        "#,
    );
    assert!(r.is_ok(), "do/while should be fine, got: {:?}", r);
}
