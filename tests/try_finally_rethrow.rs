//! Regression tests: an exception thrown inside a `try` that has a `finally`
//! but no `catch` must be re-thrown after the finally runs, not swallowed.
//!
//! The VM parks the pending exception in `current_exception` when it enters a
//! finally-only handler; the `EnterFinally(true)` emitted at the end of every
//! finally block re-throws it. These tests observe control flow through a
//! global `log` string mutated by each block.

use oite::compiler::Compiler;
use oite::vm::value::JsValue;
use oite::vm::VM;

fn run(src: &str) -> VM {
    let mut compiler = Compiler::new();
    let bytecode = compiler.compile(src).expect("compile failed");
    let mut vm = VM::new();
    vm.load_program(bytecode);
    vm.run_until_halt();
    vm
}

fn global_str(vm: &VM, name: &str) -> String {
    match vm.call_stack.first().and_then(|f| f.locals.get(name)) {
        Some(JsValue::String(s)) => s.clone(),
        other => panic!("global '{}' is not a string: {:?}", name, other),
    }
}

/// The original repro: throw inside a called function's try/finally must
/// propagate to the caller's catch after the finally runs.
#[test]
fn throw_in_try_finally_propagates_to_caller() {
    let vm = run(
        r#"
        let log = "";
        function nested() {
            try {
                throw "inner";
            } finally {
                log = log + "F";
            }
        }
        try {
            nested();
        } catch (e) {
            log = log + "C:" + e;
        }
        "#,
    );
    assert_eq!(global_str(&vm, "log"), "FC:inner");
}

/// Same-frame version: the code after the inner try must not run.
#[test]
fn throw_in_try_finally_propagates_same_frame() {
    let vm = run(
        r#"
        let log = "";
        try {
            try {
                throw "boom";
            } finally {
                log = log + "F";
            }
            log = log + "X";
        } catch (e) {
            log = log + "C:" + e;
        }
        "#,
    );
    assert_eq!(global_str(&vm, "log"), "FC:boom");
}

/// A throw from inside a catch block must still run the finally, then
/// propagate the new exception outward.
#[test]
fn throw_in_catch_runs_finally_then_propagates() {
    let vm = run(
        r#"
        let log = "";
        try {
            try {
                throw "a";
            } catch (e) {
                throw "b";
            } finally {
                log = log + "F";
            }
        } catch (e) {
            log = log + "C:" + e;
        }
        "#,
    );
    assert_eq!(global_str(&vm, "log"), "FC:b");
}

/// No exception: try and finally each run once, and the EnterFinally at the
/// end of the finally must not re-throw anything.
#[test]
fn finally_without_exception_runs_once() {
    let vm = run(
        r#"
        let log = "";
        try {
            log = log + "T";
        } finally {
            log = log + "F";
        }
        log = log + "E";
        "#,
    );
    assert_eq!(global_str(&vm, "log"), "TFE");
}

/// Exception handled by catch: catch runs, finally runs once, nothing
/// propagates.
#[test]
fn catch_handles_then_finally_runs() {
    let vm = run(
        r#"
        let log = "";
        try {
            throw "x";
        } catch (e) {
            log = log + "C";
        } finally {
            log = log + "F";
        }
        log = log + "E";
        "#,
    );
    assert_eq!(global_str(&vm, "log"), "CFE");
}

/// When a catch completes normally, the finally handler the VM re-armed for
/// it must be popped — a later throw must not jump back into the old finally.
#[test]
fn no_stale_handler_after_catch_completes() {
    let vm = run(
        r#"
        let log = "";
        function f() {
            try {
                throw "a";
            } catch (e) {
                log = log + "A";
            } finally {
                log = log + "F1";
            }
            throw "b";
        }
        try {
            f();
        } catch (e) {
            log = log + "B:" + e;
        }
        "#,
    );
    assert_eq!(global_str(&vm, "log"), "AF1B:b");
}

/// With no enclosing handler, the exception must still escape the finally
/// (previously it was silently swallowed).
#[test]
#[should_panic(expected = "Uncaught exception")]
fn uncaught_after_finally_still_panics() {
    run(
        r#"
        try {
            throw "boom";
        } finally {
        }
        "#,
    );
}
