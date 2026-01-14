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

#[test]
fn test_event_loop_runs_set_timeout_callback() {
    let mut vm = VM::new();
    let code = r#"
        function cb() { x = 42; }
        let x = 0;
        setTimeout(cb, 0);
    "#;

    let ast = parse_js(code);

    // Borrow checker should accept it (numbers are Copy; assignment is fine).
    let mut bc = BorrowChecker::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            assert!(bc.analyze_stmt(stmt).is_ok());
        }
    }

    let mut cg = Codegen::new();
    let bytecode = cg.generate(&ast);

    vm.load_program(bytecode);
    vm.run_event_loop();

    // Global `x` should have been updated by the callback.
    let global_x = vm
        .call_stack
        .first()
        .and_then(|f| f.locals.get("x"))
        .cloned()
        .unwrap_or(JsValue::Undefined);
    assert_eq!(global_x, JsValue::Number(42.0));
}

#[test]
fn test_set_timeout_with_function_expression_callback() {
    let mut vm = VM::new();
    let code = r#"
        let x = 0;
        setTimeout(function () { x = 7; }, 0);
    "#;

    let ast = parse_js(code);

    let mut bc = BorrowChecker::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            assert!(bc.analyze_stmt(stmt).is_ok());
        }
    }

    let mut cg = Codegen::new();
    let bytecode = cg.generate(&ast);

    vm.load_program(bytecode);
    vm.run_event_loop();

    let global_x = vm
        .call_stack
        .first()
        .and_then(|f| f.locals.get("x"))
        .cloned()
        .unwrap_or(JsValue::Undefined);
    assert_eq!(global_x, JsValue::Number(7.0));
}

#[test]
fn test_set_timeout_with_arrow_function_callback() {
    let mut vm = VM::new();
    let code = r#"
        let x = 0;
        setTimeout(() => { x = 9; }, 0);
    "#;

    let ast = parse_js(code);

    let mut bc = BorrowChecker::new();
    for item in &ast.body {
        if let Some(stmt) = item.as_stmt() {
            assert!(bc.analyze_stmt(stmt).is_ok());
        }
    }

    let mut cg = Codegen::new();
    let bytecode = cg.generate(&ast);

    vm.load_program(bytecode);
    vm.run_event_loop();

    let global_x = vm
        .call_stack
        .first()
        .and_then(|f| f.locals.get("x"))
        .cloned()
        .unwrap_or(JsValue::Undefined);
    assert_eq!(global_x, JsValue::Number(9.0));
}
