//! Bytecode → SSA IR lowering pass.
//!
//! This module transforms stack-based bytecode into register-based SSA form.
//! The algorithm:
//! 1. Scan bytecode to identify basic block boundaries (jump targets, after jumps)
//! 2. Abstract-interpret each instruction to track stack state
//! 3. Convert stack operations to explicit value assignments
//! 4. Insert phi nodes at CFG merge points

use crate::ir::{
    BasicBlock, BlockId, IrFunction, IrModule, IrOp, IrType, Literal, Terminator, ValueId,
};
use crate::vm::opcodes::OpCode;
use crate::vm::value::JsValue;
use std::collections::{HashMap, HashSet};

/// Errors that can occur during lowering.
#[derive(Debug)]
pub enum LowerError {
    /// Stack underflow during abstract interpretation
    StackUnderflow,
    /// Invalid jump target
    InvalidJumpTarget(usize),
    /// Unsupported opcode for lowering
    UnsupportedOpcode(String),
    /// Internal error
    Internal(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::StackUnderflow => write!(f, "Stack underflow during IR lowering"),
            LowerError::InvalidJumpTarget(addr) => write!(f, "Invalid jump target: {}", addr),
            LowerError::UnsupportedOpcode(op) => write!(f, "Unsupported opcode: {}", op),
            LowerError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for LowerError {}

/// Bytecode to SSA lowering context.
pub struct Lowerer {
    /// The function being built.
    func: IrFunction,
    /// Current basic block being populated.
    current_block: BlockId,
    /// Abstract stack: maps stack position to SSA value.
    stack: Vec<ValueId>,
    /// Mapping from instruction index to block ID (for jump targets).
    instr_to_block: HashMap<usize, BlockId>,
    /// Set of instruction indices that start a new basic block.
    block_starts: HashSet<usize>,
    /// Variable name to local slot mapping.
    var_to_slot: HashMap<String, u32>,
    /// Current value in each local slot (for SSA rename).
    local_values: HashMap<u32, ValueId>,
    /// Block entry states for phi node generation.
    block_entry_stacks: HashMap<BlockId, Vec<ValueId>>,
}

impl Lowerer {
    /// Create a new lowerer for a function.
    pub fn new(name: String) -> Self {
        let mut func = IrFunction::new(name);
        let entry = func.alloc_block();

        Self {
            func,
            current_block: entry,
            stack: Vec::new(),
            instr_to_block: HashMap::new(),
            block_starts: HashSet::new(),
            var_to_slot: HashMap::new(),
            local_values: HashMap::new(),
            block_entry_stacks: HashMap::new(),
        }
    }

    /// Create a new lowerer for an extracted function with parameters.
    pub fn new_with_params(name: String, param_names: &[String]) -> Self {
        let mut lowerer = Self::new(name);

        // Add parameters to the function and pre-populate the stack
        for param_name in param_names {
            let param_val = lowerer.alloc_value(IrType::Any);
            lowerer.func.params.push((param_name.clone(), IrType::Any));
            // Pre-populate stack with parameter values (they'll be popped by Let)
            lowerer.stack.push(param_val);
        }

        lowerer
    }

    /// Lower a sequence of bytecode instructions to SSA IR.
    pub fn lower(mut self, instructions: &[OpCode]) -> Result<IrFunction, LowerError> {
        // Pass 1: Identify basic block boundaries
        self.find_block_boundaries(instructions);

        // Pass 2: Create blocks for each boundary
        self.create_blocks(instructions);

        // Pass 3: Lower each instruction
        self.lower_instructions(instructions)?;

        // Pass 4: Compute predecessors and insert phi nodes
        self.func.compute_predecessors();

        Ok(self.func)
    }

    /// Find all instruction indices that start a new basic block.
    fn find_block_boundaries(&mut self, instructions: &[OpCode]) {
        // Entry block starts at 0
        self.block_starts.insert(0);

        for (i, op) in instructions.iter().enumerate() {
            match op {
                OpCode::Jump(target) => {
                    // Target is a block start
                    self.block_starts.insert(*target);
                    // Instruction after jump is a block start (if exists)
                    if i + 1 < instructions.len() {
                        self.block_starts.insert(i + 1);
                    }
                }
                OpCode::JumpIfFalse(target) => {
                    // Both branch targets are block starts
                    self.block_starts.insert(*target);
                    // Fall-through is also a block start
                    if i + 1 < instructions.len() {
                        self.block_starts.insert(i + 1);
                    }
                }
                OpCode::Return | OpCode::Halt => {
                    // Instruction after terminator is a block start
                    if i + 1 < instructions.len() {
                        self.block_starts.insert(i + 1);
                    }
                }
                OpCode::Call(_) | OpCode::CallMethod(_, _) => {
                    // Calls can throw, so next instruction could be a catch block
                    // For now, we don't split on calls
                }
                _ => {}
            }
        }
    }

    /// Create basic blocks for each boundary.
    fn create_blocks(&mut self, instructions: &[OpCode]) {
        // Sort block starts for deterministic ordering
        let mut starts: Vec<_> = self.block_starts.iter().copied().collect();
        starts.sort();

        // Entry block (index 0) is already created
        self.instr_to_block.insert(0, BlockId(0));

        // Create additional blocks
        for &start in &starts {
            if start != 0 && start < instructions.len() {
                let block_id = self.func.alloc_block();
                self.instr_to_block.insert(start, block_id);
            }
        }
    }

    /// Allocate a new SSA value with the given type.
    fn alloc_value(&mut self, ty: IrType) -> ValueId {
        self.func.alloc_value(ty)
    }

    /// Push a value onto the abstract stack.
    fn push(&mut self, value: ValueId) {
        self.stack.push(value);
    }

    /// Pop a value from the abstract stack.
    fn pop(&mut self) -> Result<ValueId, LowerError> {
        self.stack.pop().ok_or(LowerError::StackUnderflow)
    }

    /// Peek at the top of the stack without popping.
    fn peek(&self) -> Result<ValueId, LowerError> {
        self.stack.last().copied().ok_or(LowerError::StackUnderflow)
    }

    /// Emit an operation to the current block.
    fn emit(&mut self, op: IrOp) {
        self.func.block_mut(self.current_block).push(op);
    }

    /// Set the terminator for the current block.
    fn terminate(&mut self, term: Terminator) {
        self.func.block_mut(self.current_block).terminate(term);
    }

    /// Get or create a local slot for a variable.
    fn get_or_create_local(&mut self, name: &str) -> u32 {
        if let Some(&slot) = self.var_to_slot.get(name) {
            slot
        } else {
            let slot = self.func.add_local(name.to_string(), IrType::Any);
            self.var_to_slot.insert(name.to_string(), slot);
            slot
        }
    }

    /// Lower all instructions.
    fn lower_instructions(&mut self, instructions: &[OpCode]) -> Result<(), LowerError> {
        // Track stack state at each block entry for proper phi generation
        let mut block_stacks: HashMap<BlockId, Vec<ValueId>> = HashMap::new();
        block_stacks.insert(BlockId(0), Vec::new()); // Entry block starts with empty stack

        // Pre-compute reachability by following control flow
        let reachable_blocks = self.compute_reachable_blocks(instructions);

        for (i, op) in instructions.iter().enumerate() {
            // Check if we need to start a new block
            if i > 0 && self.block_starts.contains(&i) {
                let new_block = self.instr_to_block[&i];

                // If current block doesn't have a terminator, add a jump and save stack
                if matches!(
                    self.func.block(self.current_block).terminator,
                    Terminator::Unreachable
                ) && reachable_blocks.contains(&self.current_block)
                {
                    self.terminate(Terminator::Jump(new_block));
                    block_stacks
                        .entry(new_block)
                        .or_insert_with(|| self.stack.clone());
                }

                // Skip unreachable blocks (e.g., function bodies jumped over)
                if !reachable_blocks.contains(&new_block) {
                    self.current_block = new_block;
                    self.stack.clear();
                    continue;
                }

                // Restore stack state from saved entry (or use empty if not saved)
                self.stack = block_stacks.get(&new_block).cloned().unwrap_or_default();
                self.current_block = new_block;
            }

            // Skip if we're in an unreachable block
            if !reachable_blocks.contains(&self.current_block) {
                continue;
            }

            // Before lowering jumps, save target block stack state
            match op {
                OpCode::Jump(target) => {
                    if let Some(&target_block) = self.instr_to_block.get(target) {
                        block_stacks
                            .entry(target_block)
                            .or_insert_with(|| self.stack.clone());
                    }
                }
                OpCode::JumpIfFalse(target) => {
                    // Compute stack state after popping condition
                    let stack_after_pop: Vec<_> = if !self.stack.is_empty() {
                        self.stack[..self.stack.len() - 1].to_vec()
                    } else {
                        Vec::new()
                    };

                    // Save for false branch target
                    if let Some(&target_block) = self.instr_to_block.get(target) {
                        block_stacks
                            .entry(target_block)
                            .or_insert_with(|| stack_after_pop.clone());
                    }

                    // Save for fall-through (true branch)
                    if let Some(&fall_through) = self.instr_to_block.get(&(i + 1)) {
                        block_stacks.entry(fall_through).or_insert(stack_after_pop);
                    }
                }
                _ => {}
            }

            self.lower_instruction(i, op)?;
        }

        // Save block entry stacks for phi generation
        self.block_entry_stacks = block_stacks;

        // If the last block doesn't have a terminator, add one
        if matches!(
            self.func.block(self.current_block).terminator,
            Terminator::Unreachable
        ) {
            self.terminate(Terminator::Return(None));
        }

        Ok(())
    }

    /// Compute which blocks are reachable from entry by following control flow.
    fn compute_reachable_blocks(&self, instructions: &[OpCode]) -> HashSet<BlockId> {
        let mut reachable = HashSet::new();
        let mut worklist = vec![0usize]; // Start from instruction 0
        let mut visited_instrs = HashSet::new();

        while let Some(ip) = worklist.pop() {
            if ip >= instructions.len() || visited_instrs.contains(&ip) {
                continue;
            }
            visited_instrs.insert(ip);

            // Mark the block containing this instruction as reachable
            if let Some(&block) = self.instr_to_block.get(&ip) {
                reachable.insert(block);
            } else if ip == 0 {
                reachable.insert(BlockId(0));
            }

            match &instructions[ip] {
                OpCode::Jump(target) => {
                    worklist.push(*target);
                }
                OpCode::JumpIfFalse(target) => {
                    worklist.push(*target);
                    worklist.push(ip + 1); // Fall-through
                }
                OpCode::Return | OpCode::Halt => {
                    // No successors
                }
                _ => {
                    // Fall through to next instruction
                    worklist.push(ip + 1);
                }
            }
        }

        reachable
    }

    /// Lower a single instruction.
    fn lower_instruction(&mut self, idx: usize, op: &OpCode) -> Result<(), LowerError> {
        match op {
            OpCode::LoadThis => {
                let dst = self.alloc_value(IrType::Object);
                self.emit(IrOp::LoadThis(dst));
                self.push(dst);
            }

            OpCode::Push(value) => {
                let (lit, ty) = self.jsvalue_to_literal(value);
                let dst = self.alloc_value(ty);
                self.emit(IrOp::Const(dst, lit));
                self.push(dst);
            }

            OpCode::Pop => {
                self.pop()?;
            }

            OpCode::Dup => {
                let top = self.peek()?;
                self.push(top);
            }

            OpCode::Swap => {
                // Swap top two values
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(b);
                self.push(a);
            }

            OpCode::Swap3 => {
                // Swap top three values: [a, b, c] -> [c, b, a]
                let c = self.pop()?;
                let b = self.pop()?;
                let a = self.pop()?;
                self.push(c);
                self.push(b);
                self.push(a);
            }

            OpCode::GetPrivateProp(_) => {
                // For AOT compilation, private props aren't supported yet
                // Just push undefined
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(dst, Literal::Undefined));
                self.push(dst);
            }

            OpCode::SetPrivateProp(_) => {
                // For AOT compilation, private props aren't supported yet
                // Pop value and this
                let _ = self.pop()?;
                let _ = self.pop()?;
                // Push undefined as the result
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(dst, Literal::Undefined));
                self.push(dst);
            }

            // Arithmetic operations (binary)
            OpCode::Add => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::AddAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Sub => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::SubAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Mul => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::MulAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Div => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::DivAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Mod => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::ModAny(dst, a, b));
                self.push(dst);
            }

            OpCode::Neg => {
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::NegAny(dst, a));
                self.push(dst);
            }

            OpCode::TypeOf => {
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::String);
                self.emit(IrOp::TypeOf(dst, a));
                self.push(dst);
            }

            OpCode::Delete(prop_name) => {
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::DeleteProp(dst, obj, prop_name.clone()));
                self.push(dst);
            }

            // Comparison operations
            OpCode::Eq | OpCode::EqEq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::EqStrict(dst, a, b));
                self.push(dst);
            }

            OpCode::Ne | OpCode::NeEq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::NeStrict(dst, a, b));
                self.push(dst);
            }

            OpCode::Lt => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::Lt(dst, a, b));
                self.push(dst);
            }

            OpCode::LtEq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::LtEq(dst, a, b));
                self.push(dst);
            }

            OpCode::Gt => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::Gt(dst, a, b));
                self.push(dst);
            }

            OpCode::GtEq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::GtEq(dst, a, b));
                self.push(dst);
            }

            // InstanceOf operator
            OpCode::InstanceOf => {
                // For AOT compilation, use a stub call
                // Pop constructor and object
                let ctor = self.pop()?;
                let obj = self.pop()?;
                // Call runtime stub
                let stub = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadGlobal(stub, "tscl_instanceof".to_string()));
                let result = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::Call(result, stub, vec![obj, ctor]));
                self.push(result);
            }

            // NewTarget - for AOT, use a stub
            OpCode::NewTarget => {
                // Call runtime stub
                let stub = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadGlobal(stub, "tscl_new_target".to_string()));
                let result = self.alloc_value(IrType::Any);
                self.emit(IrOp::Call(result, stub, vec![]));
                self.push(result);
            }

            // ApplyDecorator - for now, just call a runtime stub
            OpCode::ApplyDecorator => {
                let target = self.pop()?;
                let decorator = self.pop()?;
                // Call runtime stub for decorator application
                let stub = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadGlobal(stub, "tscl_apply_decorator".to_string()));
                let result = self.alloc_value(IrType::Any);
                self.emit(IrOp::Call(result, stub, vec![target.clone(), decorator]));
                self.push(result);
            }

            // Logical operations
            OpCode::Not => {
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::Not(dst, a));
                self.push(dst);
            }

            OpCode::And => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::And(dst, a, b));
                self.push(dst);
            }

            OpCode::Or => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Or(dst, a, b));
                self.push(dst);
            }

            // Variable operations (name-based)
            OpCode::Let(name) | OpCode::Store(name) => {
                let val = self.pop()?;
                let slot = self.get_or_create_local(name);
                self.emit(IrOp::StoreLocal(slot, val));
                self.local_values.insert(slot, val);
            }

            OpCode::Load(name) => {
                let slot = self.get_or_create_local(name);
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadLocal(dst, slot));
                self.local_values.insert(slot, dst);
                self.push(dst);
            }

            OpCode::Drop(_name) => {
                // Drop is a no-op in SSA form
            }

            // Indexed local operations
            OpCode::StoreLocal(slot) => {
                let val = self.pop()?;
                self.emit(IrOp::StoreLocal(*slot, val));
                self.local_values.insert(*slot, val);
            }

            OpCode::LoadLocal(slot) => {
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadLocal(dst, *slot));
                self.local_values.insert(*slot, dst);
                self.push(dst);
            }

            // Object operations
            OpCode::NewObject => {
                let dst = self.alloc_value(IrType::Object);
                self.emit(IrOp::NewObject(dst));
                self.push(dst);
            }

            OpCode::NewObjectWithProto => {
                // For AOT compilation, NewObjectWithProto is not fully supported
                // Just create a regular object
                let dst = self.alloc_value(IrType::Object);
                self.emit(IrOp::NewObject(dst));
                self.push(dst);
            }

            OpCode::GetProp(name) => {
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetProp(dst, obj, name.clone()));
                self.push(dst);
            }

            OpCode::SetProp(name) => {
                let val = self.pop()?;
                let obj = self.pop()?;
                self.emit(IrOp::SetProp(obj, name.clone(), val));
                // Push obj back for chaining
                self.push(obj);
            }

            OpCode::SetPropComputed => {
                // For now, just pop the values - the VM handles this
                let _key = self.pop()?;
                let _val = self.pop()?;
                let obj = self.pop()?;
                self.push(obj);
            }

            OpCode::GetPropComputed => {
                // For now, just pop the values and push undefined
                let _key = self.pop()?;
                let _obj = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.push(dst);
            }

            // Array operations
            OpCode::NewArray(_size) => {
                let dst = self.alloc_value(IrType::Array);
                self.emit(IrOp::NewArray(dst));
                self.push(dst);
            }

            OpCode::LoadElement => {
                let idx = self.pop()?;
                let arr = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetElement(dst, arr, idx));
                self.push(dst);
            }

            OpCode::StoreElement => {
                let val = self.pop()?;
                let idx = self.pop()?;
                let arr = self.pop()?;
                self.emit(IrOp::SetElement(arr, idx, val));
            }

            // Control flow
            OpCode::Jump(target) => {
                let target_block = self
                    .instr_to_block
                    .get(target)
                    .copied()
                    .ok_or_else(|| LowerError::InvalidJumpTarget(*target))?;
                self.terminate(Terminator::Jump(target_block));
            }

            OpCode::JumpIfFalse(target) => {
                let cond = self.pop()?;
                let false_block = self
                    .instr_to_block
                    .get(target)
                    .copied()
                    .ok_or_else(|| LowerError::InvalidJumpTarget(*target))?;

                // True block is the fall-through (next instruction)
                let true_block = self
                    .instr_to_block
                    .get(&(idx + 1))
                    .copied()
                    .ok_or_else(|| LowerError::InvalidJumpTarget(idx + 1))?;

                self.terminate(Terminator::Branch(cond, true_block, false_block));
            }

            OpCode::Return => {
                let ret_val = if self.stack.is_empty() {
                    None
                } else {
                    Some(self.pop()?)
                };
                self.terminate(Terminator::Return(ret_val));
            }

            OpCode::Halt => {
                // If there's a value on the stack, return it (for REPL-style scripts)
                let ret_val = if self.stack.is_empty() {
                    None
                } else {
                    Some(self.pop()?)
                };
                self.terminate(Terminator::Return(ret_val));
            }

            // Function calls
            OpCode::Call(argc) => {
                let func_val = self.pop()?;
                // Pop arguments in reverse order
                let mut args = Vec::with_capacity(*argc);
                for _ in 0..*argc {
                    args.push(self.pop()?);
                }
                args.reverse();

                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Call(dst, func_val, args));
                self.push(dst);
            }

            OpCode::CallMethod(name, argc) => {
                // VM pops receiver first (top of stack), then args below
                // Stack: [..., arg0, arg1, ..., argN, receiver]
                let obj = self.pop()?;

                // Pop arguments (they're below the receiver on the stack)
                let mut args = Vec::with_capacity(*argc);
                for _ in 0..*argc {
                    args.push(self.pop()?);
                }
                args.reverse();

                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::CallMethod(dst, obj, name.clone(), args));
                self.push(dst);
            }

            OpCode::MakeClosure(addr) => {
                // Pop environment object
                let env = self.pop()?;
                let dst = self.alloc_value(IrType::Function);
                self.emit(IrOp::MakeClosure(dst, *addr as u32, env));
                self.push(dst);
            }

            OpCode::Construct(argc) => {
                // Pop arguments
                let mut args = Vec::with_capacity(*argc);
                for _ in 0..*argc {
                    args.push(self.pop()?);
                }
                args.reverse();

                // Pop constructor
                let ctor = self.pop()?;

                // For now, treat construct as a call
                let dst = self.alloc_value(IrType::Object);
                self.emit(IrOp::Call(dst, ctor, args));
                self.push(dst);
            }

            OpCode::Require => {
                // Module require: pop path, push module
                let path = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                // Treat as a global function call for now
                self.emit(IrOp::LoadGlobal(dst, "require".to_string()));
                let result = self.alloc_value(IrType::Any);
                self.emit(IrOp::Call(result, dst, vec![path]));
                self.push(result);
            }

            OpCode::Print => {
                let val = self.pop()?;
                // Emit as method call on console
                let console = self.alloc_value(IrType::Object);
                self.emit(IrOp::LoadGlobal(console, "console".to_string()));
                let _result = self.alloc_value(IrType::Void);
                self.emit(IrOp::CallMethod(
                    _result,
                    console,
                    "log".to_string(),
                    vec![val],
                ));
            }

            // Exception handling - for now, we just skip these in IR lowering
            // Full exception handling in IR/LLVM would require landing pads
            OpCode::Throw => {
                // Pop the exception value and emit unreachable
                let _exc = self.pop()?;
                // In a full implementation, this would invoke unwinding
                // For now, we just mark it as unreachable since thrown exceptions
                // won't be compiled to native code yet
            }

            OpCode::SetupTry { .. } => {
                // Try block setup - skip in IR for now
                // Full implementation would need invoke instructions and landing pads
            }

            OpCode::PopTry => {
                // Pop try handler - skip in IR
            }

            OpCode::EnterFinally(_) => {
                // Enter finally - skip in IR
            }

            // === Class inheritance opcodes - skip in IR for now ===
            OpCode::SetProto => {
                // Prototype chain setup - skip in IR
                // Would need runtime support for prototype lookup
                let _proto = self.pop()?;
                let obj = self.pop()?;
                self.push(obj);
            }

            OpCode::LoadSuper => {
                // Load super constructor - emit undefined for now
                let val = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(val, Literal::Undefined));
                self.push(val);
            }

            OpCode::CallSuper(arg_count) => {
                // Super constructor call - skip for now
                let _super_ctor = self.pop()?;
                for _ in 0..*arg_count {
                    let _ = self.pop()?;
                }
                let val = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(val, Literal::Undefined));
                self.push(val);
            }

            OpCode::GetSuperProp(_name) => {
                // Get property from super - emit undefined for now
                let _super_obj = self.pop()?;
                let val = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(val, Literal::Undefined));
                self.push(val);
            }

            // === ES Modules ===
            // ES modules require async loading which isn't supported in AOT yet
            // These opcodes will work in JIT mode but emit stubs for AOT
            OpCode::ImportAsync(_specifier) => {
                // Pop the module specifier URL
                let _url = self.pop()?;
                // Emit undefined as a placeholder for the promise
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(dst, Literal::Undefined));
                self.push(dst);
            }

            OpCode::Await => {
                // Await requires async runtime support - emit undefined for now
                let _promise = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(dst, Literal::Undefined));
                self.push(dst);
            }

            OpCode::GetExport {
                name: _,
                is_default: _,
            } => {
                // Get export from module namespace - emit undefined for now
                let _namespace = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(dst, Literal::Undefined));
                self.push(dst);
            }

            OpCode::ModuleResolutionError { .. } => {
                // Module resolution error - pop and ignore for AOT
                let _specifier = self.pop()?;
                let _importer = self.pop()?;
                let _chain = self.pop()?;
            }

            // Bitwise operators - emit as number operations
            OpCode::BitAnd => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::BitAnd(dst, a, b));
                self.push(dst);
            }

            OpCode::BitOr => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::BitOr(dst, a, b));
                self.push(dst);
            }

            OpCode::Xor => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::Xor(dst, a, b));
                self.push(dst);
            }

            OpCode::ShiftLeft => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::Shl(dst, a, b));
                self.push(dst);
            }

            OpCode::ShiftRight => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::Shr(dst, a, b));
                self.push(dst);
            }

            OpCode::ShiftRightUnsigned => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::ShrU(dst, a, b));
                self.push(dst);
            }

            OpCode::Pow => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Number);
                self.emit(IrOp::Pow(dst, a, b));
                self.push(dst);
            }
        }

        Ok(())
    }

    /// Convert a JsValue to an IR literal.
    fn jsvalue_to_literal(&self, value: &JsValue) -> (Literal, IrType) {
        match value {
            JsValue::Number(n) => (Literal::Number(*n), IrType::Number),
            JsValue::String(s) => (Literal::String(s.clone()), IrType::String),
            JsValue::Boolean(b) => (Literal::Boolean(*b), IrType::Boolean),
            JsValue::Null => (Literal::Null, IrType::Any),
            JsValue::Undefined => (Literal::Undefined, IrType::Any),
            JsValue::Object(_) => (Literal::Null, IrType::Object),
            JsValue::Function { address, .. } => {
                (Literal::Number(*address as f64), IrType::Function)
            }
            JsValue::NativeFunction(_) => (Literal::Null, IrType::Function),
            JsValue::Accessor(_, _) => (Literal::Null, IrType::Any),
            JsValue::Promise(_) => (Literal::Null, IrType::Any),
        }
    }
}

/// Information about an extracted function from bytecode.
#[derive(Debug, Clone)]
pub struct ExtractedFunction {
    /// Bytecode address where the function starts.
    pub address: usize,
    /// Bytecode address where the function ends (inclusive).
    pub end_address: usize,
    /// Whether this function captures variables (closure).
    pub has_env: bool,
    /// Number of parameters (detected from leading Let instructions).
    pub param_count: usize,
    /// Parameter names.
    pub param_names: Vec<String>,
    /// Variable name that is self-referenced in the function.
    pub self_reference_var: Option<String>,
    /// Whether the function has a self-reference.
    pub has_self_ref: bool,
}

/// Lower an entire bytecode module to SSA IR.
pub fn lower_module(instructions: &[OpCode]) -> Result<IrModule, LowerError> {
    let mut module = IrModule::new();

    // Step 1: Extract all function definitions from bytecode
    let extracted_funcs = extract_functions(instructions);

    // Step 1.5: Build a map of variable name -> function address
    // This is used to pre-initialize function references in extracted functions
    let mut func_var_addrs: HashMap<String, usize> = HashMap::new();
    for i in 0..instructions.len().saturating_sub(1) {
        if let OpCode::Push(JsValue::Function { address, .. }) = &instructions[i] {
            if let OpCode::Let(name) | OpCode::Store(name) = &instructions[i + 1] {
                func_var_addrs.insert(name.clone(), *address);
            }
        }
    }

    // Step 2: Lower each extracted function
    for func_info in &extracted_funcs {
        let func_bytecode = &instructions[func_info.address..=func_info.end_address];
        let func_name = format!("func_{}", func_info.address);

        // Lower with parameter info and base address for rebasing jump targets
        match lower_extracted_function(
            &func_name,
            func_bytecode,
            &func_info.param_names,
            func_info.address,
            func_info.self_reference_var.as_ref(),
            &func_var_addrs,
        ) {
            Ok(ir_func) => {
                module.add_function(ir_func);
            }
            Err(e) => {
                // Log but continue - some functions may have issues
                eprintln!("Warning: Failed to lower func_{}: {}", func_info.address, e);
            }
        }
    }

    // Step 3: Lower the main code (treating skipped function bodies as jumps)
    let lowerer = Lowerer::new("main".to_string());
    let main_func = lowerer.lower(instructions)?;
    module.add_function(main_func);

    // Store function address mappings in the module
    for (i, func_info) in extracted_funcs.iter().enumerate() {
        module.function_addrs.insert(func_info.address, i);
    }

    // Step 4: Detect user-defined main() function
    // Look for pattern: Push(Function { address: X, ... }) followed by Let("main")
    for i in 0..instructions.len().saturating_sub(1) {
        if let OpCode::Push(JsValue::Function { address, .. }) = &instructions[i] {
            if let OpCode::Let(name) = &instructions[i + 1] {
                if name == "main" {
                    module.user_main_addr = Some(*address);
                    break;
                }
            }
        }
    }

    Ok(module)
}

/// Lower an extracted function with known parameters.
fn lower_extracted_function(
    name: &str,
    instructions: &[OpCode],
    param_names: &[String],
    base_addr: usize,
    self_ref_var: Option<&String>,
    func_var_addrs: &HashMap<String, usize>,
) -> Result<IrFunction, LowerError> {
    // Rebase jump targets to be relative to the function start
    let rebased = rebase_jump_targets(instructions, base_addr);
    let mut lowerer = Lowerer::new_with_params(name.to_string(), param_names);

    for param_name in param_names {
        lowerer.get_or_create_local(param_name);
    }

    if let Some(var_name) = self_ref_var {
        let slot = lowerer.get_or_create_local(var_name);
        let funct_addr_val = lowerer.alloc_value(IrType::Any);
        let addr_num = base_addr as f64;

        lowerer.emit(IrOp::Const(funct_addr_val, Literal::Number(addr_num)));
        lowerer.emit(IrOp::StoreLocal(slot, funct_addr_val));
        lowerer.local_values.insert(slot, funct_addr_val);
    }

    // Pre-initialize any referenced function variables from outer scope
    // Scan the bytecode for Load instructions that reference known functions
    let mut initialized_vars: HashSet<String> = HashSet::new();
    if let Some(var_name) = self_ref_var {
        initialized_vars.insert(var_name.clone());
    }
    for param_name in param_names {
        initialized_vars.insert(param_name.clone());
    }

    for op in &rebased {
        if let OpCode::Load(var_name) = op {
            if !initialized_vars.contains(var_name) {
                if let Some(&func_addr) = func_var_addrs.get(var_name) {
                    // This variable references an outer function - pre-initialize it
                    let slot = lowerer.get_or_create_local(var_name);
                    let func_addr_val = lowerer.alloc_value(IrType::Function);
                    lowerer.emit(IrOp::Const(
                        func_addr_val,
                        Literal::Number(func_addr as f64),
                    ));
                    lowerer.emit(IrOp::StoreLocal(slot, func_addr_val));
                    lowerer.local_values.insert(slot, func_addr_val);
                    initialized_vars.insert(var_name.clone());
                }
            }
        }
    }

    lowerer.lower(&rebased)
}

/// Rebase jump targets in bytecode to be relative to a base address.
/// For extracted functions, base_addr is the function's start address.
fn rebase_jump_targets(instructions: &[OpCode], base_addr: usize) -> Vec<OpCode> {
    instructions
        .iter()
        .map(|op| match op {
            OpCode::Jump(target) => {
                let rebased = target.saturating_sub(base_addr);
                OpCode::Jump(rebased)
            }
            OpCode::JumpIfFalse(target) => {
                let rebased = target.saturating_sub(base_addr);
                OpCode::JumpIfFalse(rebased)
            }
            other => other.clone(),
        })
        .collect()
}

/// Extract function definitions from bytecode.
///
/// Detects patterns like:
/// ```text
/// Push(Function { address: X, env: ... })
/// Let("name") or Store("name")
/// Jump(Y)  <- Jumps over the function body
/// [X] Let("param1")  <- Parameter binding
/// [X+1] Let("param2")  <- Another parameter
/// ... function body ...
/// [Y-1] Return
/// [Y] ... main code continues ...
/// ```
fn extract_functions(instructions: &[OpCode]) -> Vec<ExtractedFunction> {
    let mut functions = Vec::new();

    for (i, op) in instructions.iter().enumerate() {
        if let OpCode::Push(JsValue::Function { address, env }) = op {
            // Find the end of this function (the Return before the next main code)
            if let Some(end_addr) = find_function_end(*address, instructions) {
                // Detect parameters: consecutive Let instructions at function start
                let (param_count, param_names) = detect_function_params(*address, instructions);

                let func_var_name = if i + 1 < instructions.len() {
                    match &instructions[i + 1] {
                        OpCode::Let(name) | OpCode::Store(name) => Some(name.clone()),
                        _ => None,
                    }
                } else {
                    None
                };

                let has_self_ref = if let Some(ref var_name) = func_var_name {
                    detect_self_reference(*address, end_addr, var_name, instructions)
                } else {
                    false
                };

                functions.push(ExtractedFunction {
                    address: *address,
                    end_address: end_addr,
                    has_env: env.is_some(),
                    param_count,
                    param_names,
                    self_reference_var: func_var_name,
                    has_self_ref,
                });
            }
        }
    }

    // Sort by address and deduplicate
    functions.sort_by_key(|f| f.address);
    functions.dedup_by_key(|f| f.address);

    functions
}

/// Detect if a function has a self-reference.
fn detect_self_reference(
    start: usize,
    end: usize,
    var_name: &str,
    instructions: &[OpCode],
) -> bool {
    for i in start..=end.min(instructions.len().saturating_sub(1)) {
        match &instructions[i] {
            OpCode::Load(name) if name == var_name => return true,
            OpCode::CallMethod(name, _) if name == var_name => return true,
            _ => {}
        }
    }

    false
}

/// Detect function parameters from leading Let instructions.
/// Returns (count, names).
fn detect_function_params(start: usize, instructions: &[OpCode]) -> (usize, Vec<String>) {
    let mut params = Vec::new();

    for i in start..instructions.len() {
        match &instructions[i] {
            OpCode::Let(name) => {
                params.push(name.clone());
            }
            _ => break, // Stop at first non-Let instruction
        }
    }

    (params.len(), params)
}

/// Find the end address of a function body.
///
/// A function ends at the last Return instruction before we hit either:
/// - Another function's body
/// - Code that jumps back into the main flow
fn find_function_end(start: usize, instructions: &[OpCode]) -> Option<usize> {
    let mut last_return = None;

    for i in start..instructions.len() {
        match &instructions[i] {
            OpCode::Return => {
                last_return = Some(i);
                // Check if next instruction might be different function or main code
                if i + 1 < instructions.len() {
                    // If next op is a new Let after Return, this is likely end of function
                    if let OpCode::Push(JsValue::Function { .. }) =
                        &instructions[i.saturating_sub(1).max(start)]
                    {
                        // Nested function - keep going
                        continue;
                    }
                }
            }
            OpCode::Halt => {
                // Halt means end of program - function must end before this
                return last_return;
            }
            _ => {}
        }
    }

    // If we reach the end without finding a clear boundary, use last Return
    last_return
}

/// Lower a single function's bytecode to SSA IR.
pub fn lower_function(name: &str, instructions: &[OpCode]) -> Result<IrFunction, LowerError> {
    let lowerer = Lowerer::new(name.to_string());
    lowerer.lower(instructions)
}

#[cfg(test)]
mod tests {
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
}
