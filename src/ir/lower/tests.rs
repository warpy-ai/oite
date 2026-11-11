use super::*;

#[test]
fn test_lower_simple_add() {
    // Push 1, Push 2, Add, Return
    let instructions = vec![
        OpCode::Push(JsValue::Number(1.0)),
        OpCode::Push(JsValue::Number(2.0)),
        OpCode::Add,
        OpCode::Return,
    ];

    let func = lower_function("test", &instructions).unwrap();

    // Should have one block
    assert_eq!(func.blocks.len(), 1);

    // Print IR for debugging
    println!("{}", func);

    // Check operations
    let block = &func.blocks[0];
    assert!(block.ops.len() >= 3); // Two constants + add
}

#[test]
fn test_lower_conditional() {
    // if (x < 10) { return 1 } else { return 2 }
    let instructions = vec![
        OpCode::Push(JsValue::Number(5.0)),
        OpCode::Push(JsValue::Number(10.0)),
        OpCode::Lt,
        OpCode::JumpIfFalse(6), // Jump to else branch
        OpCode::Push(JsValue::Number(1.0)),
        OpCode::Return,
        OpCode::Push(JsValue::Number(2.0)),
        OpCode::Return,
    ];

    let func = lower_function("test", &instructions).unwrap();

    // Should have 3 blocks: entry, then, else
    assert!(func.blocks.len() >= 2);

    println!("{}", func);
}

#[test]
fn test_lower_variable_access() {
    // let x = 42; return x;
    let instructions = vec![
        OpCode::Push(JsValue::Number(42.0)),
        OpCode::Let("x".to_string()),
        OpCode::Load("x".to_string()),
        OpCode::Return,
    ];

    let func = lower_function("test", &instructions).unwrap();

    println!("{}", func);

    // Should have local variable 'x'
    assert_eq!(func.locals.len(), 1);
    assert_eq!(func.locals[0].0, "x");
}

#[test]
fn test_lower_function_call() {
    // foo(1, 2)
    let instructions = vec![
        OpCode::Push(JsValue::Function {
            address: 100,
            env: None,
        }),
        OpCode::Push(JsValue::Number(1.0)),
        OpCode::Push(JsValue::Number(2.0)),
        OpCode::Call(2),
        OpCode::Return,
    ];

    let func = lower_function("test", &instructions).unwrap();

    println!("{}", func);

    // Should contain a Call operation
    let has_call = func.blocks[0]
        .ops
        .iter()
        .any(|op| matches!(op, IrOp::Call(_, _, _)));
    assert!(has_call);
}

#[test]
fn test_lower_loop() {
    // while (x < 10) { x = x + 1 }
    // The Store opcode consumes the value, so no Pop needed after it
    // 0: Push 0
    // 1: Let x
    // 2: Load x         <- loop header block
    // 3: Push 10
    // 4: Lt
    // 5: JumpIfFalse 10 <- exit loop
    // 6: Load x         <- loop body block
    // 7: Push 1
    // 8: Add
    // 9: Store x
    // 10: Jump 2        <- back to loop header
    // 11: Halt          <- loop exit block
    let instructions = vec![
        OpCode::Push(JsValue::Number(0.0)),  // 0
        OpCode::Let("x".to_string()),        // 1
        OpCode::Load("x".to_string()),       // 2 - loop header
        OpCode::Push(JsValue::Number(10.0)), // 3
        OpCode::Lt,                          // 4
        OpCode::JumpIfFalse(11),             // 5 - jump to exit
        OpCode::Load("x".to_string()),       // 6 - loop body
        OpCode::Push(JsValue::Number(1.0)),  // 7
        OpCode::Add,                         // 8
        OpCode::Store("x".to_string()),      // 9
        OpCode::Jump(2),                     // 10 - back edge
        OpCode::Halt,                        // 11 - exit
    ];

    let func = lower_function("test", &instructions).unwrap();

    println!("{}", func);

    // Should have multiple blocks due to the loop
    assert!(func.blocks.len() >= 3);
}

/// What the compiler emits, with types strict, for
/// `function f(n: number) { return n - 1; }`: the parameter is checked on
/// entry, and each read of it is a number the checker has proved.
fn typed_parameter() -> Vec<OpCode> {
    use crate::runtime::builtins::Kind;
    use crate::runtime::intrinsics;
    let number = JsValue::Number(Kind::Number.bit() as f64);
    let check = intrinsics::id("check_type").unwrap();
    let assume = intrinsics::id("assume_type").unwrap();
    vec![
        // The argument: a value the IR knows nothing about.
        OpCode::Load("argument".to_string()),
        OpCode::Let("n".to_string()),
        OpCode::Load("n".to_string()),
        OpCode::Push(number.clone()),
        OpCode::Intrinsic(check, 2),
        OpCode::Pop,
        OpCode::Load("n".to_string()),
        OpCode::Push(number),
        OpCode::Intrinsic(assume, 2),
        OpCode::Push(JsValue::Number(1.0)),
        OpCode::Sub,
        OpCode::Return,
    ]
}

#[test]
fn a_type_check_is_its_own_instruction() {
    let func = lower_function("f", &typed_parameter()).unwrap();
    let ir = func.to_string();
    assert!(ir.contains("check.type"), "{ir}");
    // Neither the check nor the assumption is a call into the runtime.
    assert!(!ir.contains("intrinsic"), "{ir}");
}

#[test]
fn a_proved_type_makes_arithmetic_typed() {
    let mut func = lower_function("f", &typed_parameter()).unwrap();
    crate::ir::typecheck::typecheck_function(&mut func);
    let ir = func.to_string();
    assert!(ir.contains("sub.num"), "{ir}");
    assert!(!ir.contains("sub.any"), "{ir}");
}

#[test]
fn the_check_survives_what_it_proved() {
    // Both reads of `n` are one value after optimization. The second is
    // known to be a number only because the first passed the check, so
    // the check must stay.
    let mut func = lower_function("f", &typed_parameter()).unwrap();
    crate::ir::typecheck::typecheck_function(&mut func);
    crate::ir::opt::optimize_function(&mut func);
    let ir = func.to_string();
    assert!(ir.contains("check.type"), "{ir}");
    assert!(ir.contains("sub.num"), "{ir}");
}

#[test]
fn a_check_of_a_known_number_is_dropped() {
    use crate::runtime::builtins::Kind;
    let check = crate::runtime::intrinsics::id("check_type").unwrap();
    // `(1 + 2) as number`
    let instructions = vec![
        OpCode::Push(JsValue::Number(1.0)),
        OpCode::Push(JsValue::Number(2.0)),
        OpCode::Add,
        OpCode::Push(JsValue::Number(Kind::Number.bit() as f64)),
        OpCode::Intrinsic(check, 2),
        OpCode::Return,
    ];
    let mut func = lower_function("f", &instructions).unwrap();
    crate::ir::typecheck::typecheck_function(&mut func);
    crate::ir::opt::optimize_function(&mut func);
    let ir = func.to_string();
    assert!(!ir.contains("check.type"), "{ir}");
}
