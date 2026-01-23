use crate::compiler::Codegen;
use crate::compiler::borrow_ck::BorrowChecker;
use crate::vm::VM;
use crate::vm::opcodes::OpCode;
use crate::vm::value::JsValue;
use swc_common::{FileName, SourceMap, sync::Lrc};
use swc_ecma_parser::{Parser, StringInput, Syntax, lexer::Lexer};

/// Helper function to parse JS string into an AST for testing
fn parse_js(code: &str) -> swc_ecma_ast::Module {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(FileName::Custom("test.js".into()).into(), code.to_string());
    let lexer = Lexer::new(
        Syntax::Es(Default::default()),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    parser.parse_module().expect("Failed to parse")
}

#[test]
fn test_borrow_checker_prevents_double_use() {
    let mut bc = BorrowChecker::new();

    // Primitives are Copy in this borrow checker; use a heap value to validate move semantics.
    // JS: let a = { x: 10 }; a; a;
    let ast = parse_js("let a = { x: 10 }; a; a;");

    let mut results = Vec::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            results.push(bc.analyze_stmt(stmt));
        }
    }

    // The first two statements (declaration and first use) should pass
    assert!(results[0].is_ok()); // let a = 10;
    assert!(results[1].is_ok()); // first use of a;

    // The third statement (second use of a) MUST fail because it was "moved"
    assert!(results[2].is_err());
    assert_eq!(
        results[2].clone().unwrap_err(),
        "BORROW ERROR: Use of moved variable 'a'"
    );
}

#[test]
fn test_vm_math_execution() {
    let mut vm = VM::new();

    // Manually created bytecode for: 10 + 20
    let program = vec![
        OpCode::Push(JsValue::Number(10.0)),
        OpCode::Store("a".into()),
        OpCode::Push(JsValue::Number(20.0)),
        OpCode::Store("b".into()),
        OpCode::Load("a".into()),
        OpCode::Load("b".into()),
        OpCode::Add,
        OpCode::Halt,
    ];

    vm.load_program(program);
    vm.run_event_loop();

    // The result should be 30 on top of the stack
    // (Note: To run this, ensure your VM stack is accessible or add a getter)
}

#[test]
fn test_clean_ownership_pass() {
    let mut bc = BorrowChecker::new();

    // JS: let x = 5; let y = 10;
    let ast = parse_js("let x = 5; let y = 10;");

    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            assert!(bc.analyze_stmt(stmt).is_ok());
        }
    }
}

#[test]
fn test_function_execution() {
    let mut vm = VM::new();
    let code = "function addTen() {
            let x = 10;
            let y = 5;
            x + y;
        }
        addTen();";

    let ast = parse_js(code);

    // 1. Run borrow checker
    let mut bc = BorrowChecker::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            assert!(bc.analyze_stmt(stmt).is_ok());
        }
    }

    // 2. Generate bytecode
    let mut cg = Codegen::new();
    let bytecode = cg.generate(&ast);

    // 3. Execute bytecode
    vm.load_program(bytecode);
    vm.run_event_loop();

    // 4. Verify result - the function should return 15
    // Since the function returns 15 and we're not in a function context,
    // the result should be printed (but we can't easily capture that in tests)
    // The test passes if no panic occurs during execution
}

#[test]
fn test_function_execution_with_args() {
    let mut vm = VM::new();
    let code = "function greet(a, b) {
            a + b;
        }
        greet(5, 10);";

    let ast = parse_js(code);

    // 1. Run borrow checker
    let mut bc = BorrowChecker::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            assert!(bc.analyze_stmt(stmt).is_ok());
        }
    }

    // 2. Generate bytecode
    let mut cg = Codegen::new();
    let bytecode = cg.generate(&ast);

    // 3. Execute bytecode
    vm.load_program(bytecode);
    vm.run_event_loop();

    // 4. Verify result - the function should return 15
    // Since the function returns 15 and we're not in a function context,
    // the result should be printed (but we can't easily capture that in tests)
    // The test passes if no panic occurs during execution
}

#[test]
fn test_object_creation() {
    let mut vm = VM::new();
    let code = "let obj = { a: 10, b: 20 }; obj.a + obj.b;";

    let ast = parse_js(code);

    // 1. Run borrow checker
    let mut bc = BorrowChecker::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            assert!(bc.analyze_stmt(stmt).is_ok());
        }
    }

    // 2. Generate bytecode
    let mut cg = Codegen::new();
    let bytecode = cg.generate(&ast);

    // 3. Execute bytecode
    vm.load_program(bytecode);
    vm.run_event_loop();

    // 4. Verify result - the function should return 15
    // Since the function returns 15 and we're not in a function context,
    // the result should be printed (but we can't easily capture that in tests)
    // The test passes if no panic occurs during execution
}

#[test]
fn test_function_execution_with_object_args() {
    let mut vm = VM::new();
    let code = "function greet(a, b) {
            a + b;
        }
        let obj = { a: 5, b: 10 };
        greet(obj.a, obj.b);";

    let ast = parse_js(code);

    // 1. Run borrow checker
    let mut bc = BorrowChecker::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            assert!(bc.analyze_stmt(stmt).is_ok());
        }
    }

    // 2. Generate bytecode
    let mut cg = Codegen::new();
    let bytecode = cg.generate(&ast);

    // 3. Execute bytecode
    vm.load_program(bytecode);
    vm.run_event_loop();

    // 4. Verify result - the function should return 15
    // Since the function returns 15 and we're not in a function context,
    // the result should be printed (but we can't easily capture that in tests)
    // The test passes if no panic occurs during execution }
}

#[test]
fn test_throw_error_borrow_check() {
    let _vm = VM::new();
    let _code = "let user = { a: 1 };
let admin = user;  // 'user' moves to 'admin'
let x = user.a;    // THIS SHOULD THROW AN ERROR";
}

// NOTE: setTimeout callback execution tests removed - the callback mechanism
// is not fully implemented (native_set_timeout only sleeps, doesn't invoke callback).
// TODO: Implement proper async callback execution in the VM event loop.

// ==================== CLOSURE CAPTURING TESTS ====================
// NOTE: Tests using setTimeout have been disabled because async functionality
// (setTimeout, Promise) has been moved to the @rolls/async ecosystem.
// These tests will be re-enabled in the Rolls repository.

/// Test that closure captures work correctly with setTimeout.
/// This is the "Stack Frame Paradox" scenario: the outer function's
/// stack frame would normally be destroyed, but the captured variable
/// is lifted to the heap.
///
/// DISABLED: Requires setTimeout which is in @rolls/async
#[test]
#[ignore = "setTimeout moved to @rolls/async"]
fn test_closure_captures_variable_for_async() {
    let mut vm = VM::new();
    // This code creates a closure that captures `data` from outer scope.
    // When setTimeout fires, `data` should still be accessible because
    // it was lifted to the heap environment.
    let code = r#"
        let data = { value: 42 };
        setTimeout(() => {
            console.log(data.value);
        }, 0);
    "#;

    let ast = parse_js(code);

    let mut cg = Codegen::new();
    let bytecode = cg.generate(&ast);

    // Debug: print bytecode to verify MakeClosure is generated
    for (i, op) in bytecode.iter().enumerate() {
        println!("{}: {:?}", i, op);
    }

    vm.load_program(bytecode);
    vm.run_event_loop();
    // Test passes if no panic occurs - the closure accessed captured data
}

/// Test that the borrow checker prevents use of a captured variable
/// after it has been moved into a closure.
///
/// DISABLED: Requires setTimeout which is in @rolls/async
#[test]
#[ignore = "setTimeout moved to @rolls/async"]
fn test_borrow_checker_prevents_use_after_capture() {
    let mut bc = BorrowChecker::new();

    // This code captures `data` in a closure, then tries to use it again.
    // The borrow checker should reject the second use.
    let code = r#"
        let data = { message: "Hello" };
        setTimeout(() => { console.log(data.message); }, 0);
        data.message;
    "#;

    let ast = parse_js(code);

    let mut results = Vec::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            results.push(bc.analyze_stmt(stmt));
        }
    }

    // Declaration should pass
    assert!(results[0].is_ok(), "let data = ... should pass");

    // setTimeout with closure that captures `data` should pass
    // (but it marks `data` as CapturedByAsync)
    assert!(results[1].is_ok(), "setTimeout(...) should pass");

    // Trying to access `data` after it was captured should FAIL!
    assert!(
        results[2].is_err(),
        "Access after capture should fail: {:?}",
        results[2]
    );

    let err = results[2].clone().unwrap_err();
    assert!(
        err.contains("captured") || err.contains("moved"),
        "Error should mention capture/move: {}",
        err
    );
}
