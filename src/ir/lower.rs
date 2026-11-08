//! Bytecode → SSA IR lowering pass.
//!
//! This module transforms stack-based bytecode into register-based SSA form.
//! The algorithm:
//! 1. Scan bytecode to identify basic block boundaries (jump targets, after jumps)
//! 2. Abstract-interpret each instruction to track stack state
//! 3. Convert stack operations to explicit value assignments
//! 4. Insert phi nodes at CFG merge points

use crate::ir::{BlockId, IrFunction, IrModule, IrOp, IrType, Literal, Terminator, ValueId};
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
    /// A language feature the native backends do not implement yet
    NotSupportedNatively(String),
    /// An error inside a named function
    InFunction(String, Box<LowerError>),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::StackUnderflow => write!(f, "Stack underflow during IR lowering"),
            LowerError::InvalidJumpTarget(addr) => write!(f, "Invalid jump target: {}", addr),
            LowerError::UnsupportedOpcode(op) => write!(f, "Unsupported opcode: {}", op),
            LowerError::Internal(msg) => write!(f, "Internal error: {}", msg),
            LowerError::NotSupportedNatively(what) => write!(
                f,
                "{} not supported by the JIT/AOT backends yet (the VM interpreter, `oitec <file>`, can run it for debugging)",
                what
            ),
            LowerError::InFunction(name, inner) => write!(f, "in function `{}`: {}", name, inner),
        }
    }
}

impl std::error::Error for LowerError {}

/// Error for a construct the native backends cannot compile yet.
fn native_unsupported(what: &str) -> LowerError {
    LowerError::NotSupportedNatively(what.to_string())
}

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
    /// How bytecode variable names map to locals, globals and functions.
    names: NameResolver,
    /// Enclosing `try` statements at the current instruction: where an
    /// exception goes, innermost last. Bytecode installs and removes
    /// handlers at run time (`SetupTry`, `PopTry`); code from the compiler
    /// is structured, so the handlers at each point are known statically and
    /// become each block's `unwind` target.
    handlers: Vec<BlockId>,
    /// Handlers at the entry of each block, set along the edges into it.
    block_handlers: HashMap<BlockId, Vec<BlockId>>,
    /// Instruction indices that start a `catch` block.
    catch_starts: HashSet<usize>,
    /// The `SetupTry` (by index) that each `EnterFinally` ends the
    /// `finally` of.
    finally_owner: HashMap<usize, usize>,
    /// Instruction indices that start the code after an `await` (a resume
    /// block of an async function).
    await_resumes: HashSet<usize>,
    /// Resume blocks by resume point.
    resume_points: Vec<BlockId>,
    /// Stack frames by instruction, for exception traces (`IrOp::Loc`).
    sites: Option<std::rc::Rc<Sites>>,
    /// Address of this function's first instruction in the program.
    base: usize,
    /// Address of the instruction being lowered.
    cur_ip: usize,
    /// The last `Loc` in each block.
    last_loc: HashMap<BlockId, u32>,
}

/// The stack frame (`f (file:2:14)`) of each instruction of a program,
/// numbered as `IrModule::sites`.
pub struct Sites {
    table: crate::compiler::SourceTable,
    ids: std::cell::RefCell<SiteIds>,
    /// Mark every op, not only those that can throw (debug info).
    every_op: bool,
}

#[derive(Default)]
struct SiteIds {
    by_ip: HashMap<usize, u32>,
    by_frame: HashMap<String, u32>,
    sites: Vec<crate::ir::Site>,
}

/// `IrOp::Loc` of code whose position is unknown.
pub const NO_SITE: u32 = u32::MAX;

impl Sites {
    pub fn new(table: crate::compiler::SourceTable) -> Self {
        Self {
            table,
            ids: Default::default(),
            every_op: false,
        }
    }

    /// Mark every op with its position, for debug info.
    pub fn every_op(mut self) -> Self {
        self.every_op = true;
        self
    }

    /// The site of instruction `ip`.
    fn site(&self, ip: usize) -> u32 {
        let mut ids = self.ids.borrow_mut();
        if let Some(&id) = ids.by_ip.get(&ip) {
            return id;
        }
        let id = match self.table.locate(ip) {
            Some((name, file, pos)) => {
                let frame = crate::compiler::format_frame(name, file, pos);
                match ids.by_frame.get(&frame) {
                    Some(&id) => id,
                    None => {
                        let id = ids.sites.len() as u32;
                        ids.sites.push(crate::ir::Site {
                            frame: frame.clone(),
                            name: name.map(str::to_string),
                            file: file.to_string(),
                            line: pos.line as u32,
                            col: pos.col as u32,
                        });
                        ids.by_frame.insert(frame, id);
                        id
                    }
                }
            }
            None => NO_SITE,
        };
        ids.by_ip.insert(ip, id);
        id
    }

    fn take(&self) -> Vec<crate::ir::Site> {
        std::mem::take(&mut self.ids.borrow_mut().sites)
    }
}

/// Where a bytecode variable name lives in native code.
#[derive(Debug, Clone, Default)]
pub struct NameResolver {
    /// Whether whole-program scope information is available. Without it
    /// (`lower_function`, lowering one function on its own) every name is a
    /// local, as before.
    program: bool,
    /// Names local to the function being lowered (parameters and `let`s in
    /// its own body). Top-level code has none: its variables are globals,
    /// shared with every function.
    locals: HashSet<String>,
    /// Locals of enclosing functions. Native code has no closure
    /// environments yet, so a reference to one is refused.
    enclosing: HashSet<String>,
    /// Names assigned exactly once in the whole program, by a function
    /// declaration. They resolve to that function directly.
    constant_functions: HashMap<String, usize>,
}

enum NameRef {
    Local,
    Function(usize),
    Global,
}

impl NameResolver {
    fn resolve(&self, name: &str) -> Result<NameRef, LowerError> {
        // Compiler-generated temporaries (`%optcall12`) are always local.
        if !self.program || name.starts_with('%') || self.locals.contains(name) {
            return Ok(NameRef::Local);
        }
        if let Some(&addr) = self.constant_functions.get(name) {
            return Ok(NameRef::Function(addr));
        }
        if self.enclosing.contains(name) {
            return Err(native_unsupported(
                "closures that capture a variable of an enclosing function",
            ));
        }
        Ok(NameRef::Global)
    }
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
            names: NameResolver::default(),
            handlers: Vec::new(),
            block_handlers: HashMap::new(),
            catch_starts: HashSet::new(),
            finally_owner: HashMap::new(),
            await_resumes: HashSet::new(),
            resume_points: Vec::new(),
            sites: None,
            base: 0,
            cur_ip: 0,
            last_loc: HashMap::new(),
        }
    }

    /// Use this resolver for variable names.
    pub fn with_names(mut self, names: NameResolver) -> Self {
        self.names = names;
        self
    }

    /// Mark the ops that can throw with their stack frame; the function
    /// starts at instruction `base` of the program.
    fn with_sites(mut self, sites: Option<std::rc::Rc<Sites>>, base: usize) -> Self {
        self.sites = sites;
        self.base = base;
        self
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
        self.find_try_structure(instructions);

        // Pass 2: Create blocks for each boundary
        self.create_blocks(instructions);

        // An async function that awaits is compiled as a resumable
        // function: block 0 dispatches to the start or to a resume block.
        let reachable = self.reachable_instructions(instructions);
        if reachable
            .iter()
            .any(|&i| matches!(instructions[i], OpCode::Await))
        {
            if self.func.name == "main" {
                return Err(native_unsupported("top-level `await`"));
            }
            self.func.is_async = true;
            let start = self.func.alloc_block();
            self.instr_to_block.insert(0, start);
            self.current_block = start;
        }

        // Pass 3: Lower each instruction
        self.lower_instructions(instructions)?;
        if self.func.is_async {
            self.build_resume_dispatch();
        }

        // Pass 4: Compute predecessors and insert phi nodes
        self.func.compute_predecessors();

        Ok(self.func)
    }

    /// Where each `try` statement's `catch` starts, and which `EnterFinally`
    /// ends each `finally`: the first one after the `finally` starts that
    /// is not the end of a nested `try`'s `finally`.
    fn find_try_structure(&mut self, instructions: &[OpCode]) {
        for (s, op) in instructions.iter().enumerate() {
            let OpCode::SetupTry {
                catch_addr,
                finally_addr,
            } = op
            else {
                continue;
            };
            if *catch_addr != 0 {
                self.catch_starts.insert(*catch_addr);
            }
            if *finally_addr == 0 {
                continue;
            }
            let mut depth = 0;
            for (j, op) in instructions.iter().enumerate().skip(*finally_addr) {
                match op {
                    OpCode::SetupTry { finally_addr, .. } if *finally_addr != 0 => depth += 1,
                    OpCode::EnterFinally(_) if depth == 0 => {
                        self.finally_owner.insert(j, s);
                        break;
                    }
                    OpCode::EnterFinally(_) => depth -= 1,
                    _ => {}
                }
            }
        }
    }

    /// Instructions reachable from the start (through jumps and handlers).
    fn reachable_instructions(&self, instructions: &[OpCode]) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut work = vec![0usize];
        while let Some(ip) = work.pop() {
            if ip >= instructions.len() || !seen.insert(ip) {
                continue;
            }
            match &instructions[ip] {
                OpCode::Jump(t) => work.push(*t),
                OpCode::JumpIfFalse(t) => {
                    work.push(*t);
                    work.push(ip + 1);
                }
                OpCode::Return | OpCode::Halt => {}
                OpCode::SetupTry {
                    catch_addr,
                    finally_addr,
                } => {
                    for a in [*catch_addr, *finally_addr] {
                        if a != 0 {
                            work.push(a);
                        }
                    }
                    work.push(ip + 1);
                }
                _ => work.push(ip + 1),
            }
        }
        seen
    }

    /// Block 0 of a resumable function: a fresh call goes to the start; a
    /// resumed one to the resume block of its resume point.
    fn build_resume_dispatch(&mut self) {
        let start = self.instr_to_block[&0];
        let entry = BlockId(0);
        let is_resume = self.alloc_value(IrType::Boolean);
        self.func.block_mut(entry).push(IrOp::IsResume(is_resume));
        let mut test = self.func.alloc_block();
        self.func
            .block_mut(entry)
            .terminate(Terminator::Branch(is_resume, test, start));
        let point = self.alloc_value(IrType::Any);
        self.func.block_mut(test).push(IrOp::ResumePoint(point));
        let points = self.resume_points.clone();
        for (k, block) in points.iter().enumerate() {
            let n = self.alloc_value(IrType::Number);
            let eq = self.alloc_value(IrType::Boolean);
            let next = self.func.alloc_block();
            let b = self.func.block_mut(test);
            b.push(IrOp::Const(n, Literal::Number(k as f64)));
            b.push(IrOp::EqStrict(eq, point, n));
            b.terminate(Terminator::Branch(eq, *block, next));
            test = next;
        }
        self.func.block_mut(test).terminate(Terminator::Unreachable);
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
                // The handlers change here, and a block has one unwind
                // target; `catch` and `finally` are entered by exceptions.
                OpCode::SetupTry {
                    catch_addr,
                    finally_addr,
                } => {
                    self.block_starts.insert(i + 1);
                    for addr in [*catch_addr, *finally_addr] {
                        if addr != 0 {
                            self.block_starts.insert(addr);
                        }
                    }
                }
                // The code after an `await` is where the function resumes.
                OpCode::PopTry | OpCode::Await => {
                    self.block_starts.insert(i + 1);
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

    /// Pop `n` values, returned in push order (call arguments).
    fn pop_n(&mut self, n: usize) -> Result<Vec<ValueId>, LowerError> {
        let mut values = Vec::with_capacity(n);
        for _ in 0..n {
            values.push(self.pop()?);
        }
        values.reverse();
        Ok(values)
    }

    /// Peek at the top of the stack without popping.
    fn peek(&self) -> Result<ValueId, LowerError> {
        self.stack.last().copied().ok_or(LowerError::StackUnderflow)
    }

    /// Emit an operation to the current block.
    fn emit(&mut self, op: IrOp) {
        if let Some(sites) = &self.sites
            && (sites.every_op || op.can_throw())
        {
            let site = sites.site(self.cur_ip);
            if self.last_loc.insert(self.current_block, site) != Some(site) {
                self.func
                    .block_mut(self.current_block)
                    .push(IrOp::Loc(site));
            }
        }
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

    /// Local slot used to carry operand-stack position `depth` across a block
    /// boundary. The name cannot collide with a source identifier.
    fn merge_slot(&mut self, depth: usize) -> u32 {
        self.get_or_create_local(&format!("%stack{}", depth))
    }

    /// Store the abstract stack into merge slots before an edge leaves the
    /// current block.
    ///
    /// Bytecode leaves values on the operand stack across jumps (`?:`, `&&`,
    /// `||`, `??`, `?.`). Each predecessor of a merge block holds a different
    /// SSA value in the same stack position, so the values cannot be reused
    /// directly; they travel through local slots instead, the same way
    /// variables do.
    fn spill_stack(&mut self, values: &[ValueId]) {
        for (depth, &value) in values.iter().enumerate() {
            let slot = self.merge_slot(depth);
            self.emit(IrOp::StoreLocal(slot, value));
        }
    }

    /// Rebuild the abstract stack at a block entry from the merge slots.
    fn reload_stack(&mut self, depth: usize) {
        self.stack.clear();
        for d in 0..depth {
            let slot = self.merge_slot(d);
            let dst = self.alloc_value(IrType::Any);
            self.emit(IrOp::LoadLocal(dst, slot));
            self.stack.push(dst);
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
            self.cur_ip = self.base + i;
            // Check if we need to start a new block
            if i > 0 && self.block_starts.contains(&i) {
                let new_block = self.instr_to_block[&i];

                // If current block doesn't have a terminator, add a jump and save stack
                if matches!(
                    self.func.block(self.current_block).terminator,
                    Terminator::Unreachable
                ) && reachable_blocks.contains(&self.current_block)
                {
                    let stack = self.stack.clone();
                    self.spill_stack(&stack);
                    self.terminate(Terminator::Jump(new_block));
                    block_stacks.entry(new_block).or_insert(stack);
                    self.block_handlers
                        .entry(new_block)
                        .or_insert_with(|| self.handlers.clone());
                }

                // Skip unreachable blocks (e.g., function bodies jumped over)
                if !reachable_blocks.contains(&new_block) {
                    self.current_block = new_block;
                    self.stack.clear();
                    continue;
                }

                // Restore stack state from the merge slots written by the
                // block's predecessors (empty if none was recorded).
                let depth = block_stacks.get(&new_block).map_or(0, |s| s.len());
                self.current_block = new_block;
                self.reload_stack(depth);
                self.handlers = self
                    .block_handlers
                    .get(&new_block)
                    .cloned()
                    .unwrap_or_default();
                self.func.block_mut(new_block).unwind = self.handlers.last().copied();
                if self.catch_starts.contains(&i) {
                    // The VM pushes the exception for its catch block.
                    let exception = self.alloc_value(IrType::Any);
                    self.emit(IrOp::TakeException(exception));
                    self.push(exception);
                }
                if self.await_resumes.contains(&i) {
                    // The awaited value (a rejection throws here).
                    let value = self.alloc_value(IrType::Any);
                    self.emit(IrOp::AwaitResult(value));
                    self.push(value);
                }
            }

            // Skip if we're in an unreachable block
            if !reachable_blocks.contains(&self.current_block) {
                continue;
            }

            // Before lowering jumps, save target block stack state
            match op {
                // `await`: the operand stack (without the awaited value)
                // carries over to the resume block, through the frame.
                OpCode::Await if self.func.is_async => {
                    let below = self.stack[..self.stack.len().saturating_sub(1)].to_vec();
                    self.spill_stack(&below);
                    if let Some(&resume) = self.instr_to_block.get(&(i + 1)) {
                        block_stacks.entry(resume).or_insert(below);
                        self.block_handlers
                            .entry(resume)
                            .or_insert_with(|| self.handlers.clone());
                        self.await_resumes.insert(i + 1);
                    }
                }
                OpCode::Jump(target) => {
                    let stack = self.stack.clone();
                    self.spill_stack(&stack);
                    if let Some(&target_block) = self.instr_to_block.get(target) {
                        block_stacks.entry(target_block).or_insert(stack);
                        self.block_handlers
                            .entry(target_block)
                            .or_insert_with(|| self.handlers.clone());
                    }
                }
                OpCode::JumpIfFalse(target) => {
                    // Compute stack state after popping condition
                    let stack_after_pop: Vec<_> = if !self.stack.is_empty() {
                        self.stack[..self.stack.len() - 1].to_vec()
                    } else {
                        Vec::new()
                    };
                    self.spill_stack(&stack_after_pop);

                    // Save for false branch target
                    if let Some(&target_block) = self.instr_to_block.get(target) {
                        block_stacks
                            .entry(target_block)
                            .or_insert_with(|| stack_after_pop.clone());
                        self.block_handlers
                            .entry(target_block)
                            .or_insert_with(|| self.handlers.clone());
                    }

                    // Save for fall-through (true branch)
                    if let Some(&fall_through) = self.instr_to_block.get(&(i + 1)) {
                        block_stacks.entry(fall_through).or_insert(stack_after_pop);
                        self.block_handlers
                            .entry(fall_through)
                            .or_insert_with(|| self.handlers.clone());
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

    /// Locals of the `try` at `setup`: whether its `finally` was entered by
    /// an exception, and that exception.
    fn try_slots(&mut self, setup: usize) -> (u32, u32) {
        (
            self.get_or_create_local(&format!("%thrown{}", setup)),
            self.get_or_create_local(&format!("%exception{}", setup)),
        )
    }

    /// `SetupTry` at `idx`. An exception in the `try` block goes to the
    /// `catch`; without one, or from inside the `catch`, it goes to a block
    /// that records it and enters the `finally`, whose end rethrows it.
    fn lower_setup_try(
        &mut self,
        idx: usize,
        catch_addr: usize,
        finally_addr: usize,
    ) -> Result<(), LowerError> {
        if !self.stack.is_empty() {
            return Err(native_unsupported("`try` with values on the operand stack"));
        }
        let outer = self.handlers.clone();
        let block_at = |l: &Self, addr: usize| {
            l.instr_to_block
                .get(&addr)
                .copied()
                .ok_or(LowerError::InvalidJumpTarget(addr))
        };

        let mut into_finally = None;
        if finally_addr != 0 {
            let finally_block = block_at(self, finally_addr)?;
            let (thrown_slot, exc_slot) = self.try_slots(idx);
            let no = self.alloc_value(IrType::Boolean);
            self.emit(IrOp::Const(no, Literal::Boolean(false)));
            self.emit(IrOp::StoreLocal(thrown_slot, no));
            let undefined = self.alloc_value(IrType::Any);
            self.emit(IrOp::Const(undefined, Literal::Undefined));
            self.emit(IrOp::StoreLocal(exc_slot, undefined));

            let block = self.func.alloc_block();
            let exception = self.alloc_value(IrType::Any);
            let yes = self.alloc_value(IrType::Boolean);
            let b = self.func.block_mut(block);
            b.push(IrOp::TakeException(exception));
            b.push(IrOp::StoreLocal(exc_slot, exception));
            b.push(IrOp::Const(yes, Literal::Boolean(true)));
            b.push(IrOp::StoreLocal(thrown_slot, yes));
            b.terminate(Terminator::Jump(finally_block));
            b.unwind = outer.last().copied();
            self.block_handlers
                .entry(finally_block)
                .or_insert_with(|| outer.clone());
            into_finally = Some(block);
        }

        let target = if catch_addr != 0 {
            let catch_block = block_at(self, catch_addr)?;
            let mut in_catch = outer;
            in_catch.extend(into_finally);
            self.block_handlers.insert(catch_block, in_catch);
            catch_block
        } else {
            into_finally.ok_or(LowerError::InvalidJumpTarget(idx))?
        };
        self.handlers.push(target);
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
                OpCode::SetupTry {
                    catch_addr,
                    finally_addr,
                } => {
                    // Exceptions enter the catch and finally blocks.
                    for addr in [*catch_addr, *finally_addr] {
                        if addr != 0 {
                            worklist.push(addr);
                        }
                    }
                    worklist.push(ip + 1);
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

            OpCode::TypeOfName(name) => match self.names.resolve(name)? {
                NameRef::Global => {
                    let dst = self.alloc_value(IrType::String);
                    self.emit(IrOp::TypeOfGlobal(dst, name.clone()));
                    self.push(dst);
                }
                // A local or function is always bound.
                _ => {
                    self.lower_instruction(idx, &OpCode::Load(name.clone()))?;
                    self.lower_instruction(idx, &OpCode::TypeOf)?;
                }
            },

            OpCode::ToStr => {
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::String);
                self.emit(IrOp::ToStr(dst, a));
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
            OpCode::Eq | OpCode::EqEq | OpCode::Ne | OpCode::NeEq => {
                let b = self.pop()?;
                let a = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(match op {
                    OpCode::Eq => IrOp::EqStrict(dst, a, b),
                    OpCode::Ne => IrOp::NeStrict(dst, a, b),
                    // `==` / `!=` are loose; lowering them as strict made
                    // `null == undefined` false in native code.
                    OpCode::EqEq => IrOp::EqLoose(dst, a, b),
                    _ => IrOp::NeLoose(dst, a, b),
                });
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

            OpCode::In => {
                // [key, obj]
                let obj = self.pop()?;
                let key = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::In(dst, key, obj));
                self.push(dst);
            }
            OpCode::InstanceOf => {
                // [value, ctor]
                let ctor = self.pop()?;
                let value = self.pop()?;
                let dst = self.alloc_value(IrType::Boolean);
                self.emit(IrOp::InstanceOf(dst, value, ctor));
                self.push(dst);
            }
            OpCode::NewTarget => {
                return Err(native_unsupported("`new.target`"));
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
                match self.names.resolve(name)? {
                    NameRef::Local => {
                        let slot = self.get_or_create_local(name);
                        self.emit(IrOp::StoreLocal(slot, val));
                        self.local_values.insert(slot, val);
                    }
                    NameRef::Function(_) | NameRef::Global => {
                        self.emit(IrOp::StoreGlobal(name.clone(), val));
                    }
                }
            }

            OpCode::Load(name) => {
                let dst = match self.names.resolve(name)? {
                    NameRef::Local => {
                        let slot = self.get_or_create_local(name);
                        let dst = self.alloc_value(IrType::Any);
                        self.emit(IrOp::LoadLocal(dst, slot));
                        self.local_values.insert(slot, dst);
                        dst
                    }
                    NameRef::Function(addr) => {
                        let dst = self.alloc_value(IrType::Function);
                        self.emit(IrOp::Const(dst, Literal::Function(addr)));
                        dst
                    }
                    NameRef::Global => {
                        let dst = self.alloc_value(IrType::Any);
                        self.emit(IrOp::LoadGlobal(dst, name.clone()));
                        dst
                    }
                };
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
                return Err(native_unsupported("classes"));
            }

            OpCode::GetProp(name) => {
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetProp(dst, obj, name.clone()));
                self.push(dst);
            }

            // Stack effects below match the VM's exactly; an extra or missing
            // value desynchronises everything after it.
            OpCode::SetProp(name) => {
                // [obj, value] -> []
                let val = self.pop()?;
                let obj = self.pop()?;
                self.emit(IrOp::SetProp(obj, name.clone(), val));
            }

            OpCode::SetPropComputed | OpCode::StoreElement => {
                // [obj, value, key] -> []
                let key = self.pop()?;
                let val = self.pop()?;
                let obj = self.pop()?;
                self.emit(IrOp::SetElement(obj, key, val));
            }

            OpCode::GetPropComputed | OpCode::LoadElement => {
                // [obj, key] -> [obj[key]]
                let key = self.pop()?;
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetElement(dst, obj, key));
                self.push(dst);
            }

            // Array operations
            OpCode::NewArray(size) => {
                // The VM creates `size` undefined elements, which the
                // following stores then fill.
                let dst = self.alloc_value(IrType::Array);
                self.emit(IrOp::NewArray(dst));
                if *size > 0 {
                    let len = self.alloc_value(IrType::Number);
                    self.emit(IrOp::Const(len, Literal::Number(*size as f64)));
                    self.emit(IrOp::SetProp(dst, "length".to_string(), len));
                }
                self.push(dst);
            }

            // Control flow
            OpCode::Jump(target) => {
                let target_block = self
                    .instr_to_block
                    .get(target)
                    .copied()
                    .ok_or(LowerError::InvalidJumpTarget(*target))?;
                self.terminate(Terminator::Jump(target_block));
            }

            OpCode::JumpIfFalse(target) => {
                let cond = self.pop()?;
                let false_block = self
                    .instr_to_block
                    .get(target)
                    .copied()
                    .ok_or(LowerError::InvalidJumpTarget(*target))?;

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

            OpCode::LoadClosureEnv => {
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadEnv(dst));
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
                // [args..., ctor]
                let ctor = self.pop()?;
                let args = self.pop_n(*argc)?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::Construct(dst, ctor, args));
                self.push(dst);
            }
            OpCode::CallSuper(argc) => {
                // [args..., parent]
                let parent = self.pop()?;
                let args = self.pop_n(*argc)?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::CallSuper(dst, parent, args));
                self.push(dst);
            }
            OpCode::RestArgs(n) => {
                // The arguments from the `n`th on, as an array; the first
                // `Let` binds it.
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::RestArgs(dst, *n as u32));
                self.push(dst);
            }
            OpCode::ForwardArgs => {
                // The VM's caller pushes the arguments and their count here;
                // native code reads its own. This stands for the count.
                let placeholder = self.alloc_value(IrType::Any);
                self.emit(IrOp::Const(placeholder, Literal::Undefined));
                self.push(placeholder);
            }
            OpCode::SuperForward => {
                // [count placeholder, parent]
                let parent = self.pop()?;
                self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::SuperForward(dst, parent));
                self.push(dst);
            }
            OpCode::CallWithThis(argc) => {
                // [args..., f, this]
                let this = self.pop()?;
                let func = self.pop()?;
                let args = self.pop_n(*argc)?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::CallWithThis(dst, func, this, args));
                self.push(dst);
            }
            OpCode::GetPrivate(key) => {
                let obj = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetPrivate(dst, obj, key.clone()));
                self.push(dst);
            }
            OpCode::SetPrivate(key) => {
                let value = self.pop()?;
                let obj = self.pop()?;
                self.emit(IrOp::SetPrivate(obj, key.clone(), value));
            }
            OpCode::GetSuperProp(name) => {
                // [this, base]
                let base = self.pop()?;
                let this = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::GetSuperProp(dst, this, base, name.clone()));
                self.push(dst);
            }

            OpCode::Require => {
                return Err(native_unsupported("`require`"));
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

            OpCode::Throw => {
                let value = self.pop()?;
                self.emit(IrOp::Throw(value));
            }
            OpCode::SetupTry {
                catch_addr,
                finally_addr,
            } => self.lower_setup_try(idx, *catch_addr, *finally_addr)?,
            OpCode::PopTry => {
                self.handlers.pop();
            }
            OpCode::EnterFinally(rethrow) => {
                if *rethrow && let Some(&setup) = self.finally_owner.get(&idx) {
                    let (thrown_slot, exc_slot) = self.try_slots(setup);
                    let thrown = self.alloc_value(IrType::Any);
                    self.emit(IrOp::LoadLocal(thrown, thrown_slot));
                    let exception = self.alloc_value(IrType::Any);
                    self.emit(IrOp::LoadLocal(exception, exc_slot));
                    self.emit(IrOp::Rethrow(thrown, exception));
                }
            }

            // Not yet supported by the native backends. These used to lower
            // to no-ops or `undefined`, so native code silently computed
            // wrong answers. Refusing is the only behaviour that cannot
            // produce a wrong result.
            OpCode::SetProto => {
                return Err(native_unsupported("`SetProto`"));
            }
            OpCode::ImportAsync(_)
            | OpCode::GetExport { .. }
            | OpCode::ModuleResolutionError { .. } => {
                return Err(native_unsupported("modules (`import`)"));
            }
            OpCode::Await => {
                if !self.func.is_async {
                    return Err(native_unsupported("top-level `await`"));
                }
                let value = self.pop()?;
                let resume = self
                    .instr_to_block
                    .get(&(idx + 1))
                    .copied()
                    .ok_or(LowerError::InvalidJumpTarget(idx + 1))?;
                let k = self.resume_points.len() as u32;
                self.resume_points.push(resume);
                self.emit(IrOp::Await(value, k));
                // Suspended: the caller gets the function's promise.
                let slot = self.get_or_create_local("%promise");
                let promise = self.alloc_value(IrType::Any);
                self.emit(IrOp::LoadLocal(promise, slot));
                self.terminate(Terminator::Return(Some(promise)));
            }
            OpCode::AsyncStart => {
                let dst = self.alloc_value(IrType::Any);
                self.emit(IrOp::AsyncStart(dst));
                self.push(dst);
            }
            OpCode::AsyncResolve | OpCode::AsyncReject => {
                // [value, promise] -> [promise]
                let promise = self.pop()?;
                let value = self.pop()?;
                let dst = self.alloc_value(IrType::Any);
                self.emit(if matches!(op, OpCode::AsyncResolve) {
                    IrOp::AsyncResolve(dst, promise, value)
                } else {
                    IrOp::AsyncReject(dst, promise, value)
                });
                self.push(dst);
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

            // Spread operations - not yet supported in IR, fall back to interpreter
            OpCode::ArrayPush | OpCode::ArraySpread | OpCode::ObjectSpread => {
                // For now, these operations require runtime support
                // and are handled by the interpreter
                return Err(LowerError::UnsupportedOpcode(format!("{:?}", op)));
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
            JsValue::Function { address, .. } => (Literal::Function(*address), IrType::Function),
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
/// Constructs the native backends refuse, whichever function they are in.
fn native_refusal(op: &OpCode) -> Option<&'static str> {
    Some(match op {
        OpCode::NewObjectWithProto | OpCode::SetProto => "`SetProto`",
        OpCode::NewTarget => "`new.target`",
        OpCode::ImportAsync(_)
        | OpCode::GetExport { .. }
        | OpCode::ModuleResolutionError { .. } => "modules (`import`)",
        OpCode::Require => "`require`",
        _ => return None,
    })
}

pub fn lower_module(instructions: &[OpCode]) -> Result<IrModule, LowerError> {
    lower_module_with_sites(instructions, None)
}

/// `lower_module`, with each op that can throw marked with its stack
/// frame (`IrOp::Loc`) from `sites`.
pub fn lower_module_with_sites(
    instructions: &[OpCode],
    sites: Option<Sites>,
) -> Result<IrModule, LowerError> {
    let sites = sites.map(std::rc::Rc::new);
    let mut module = IrModule::new();

    // Refuse unsupported constructs up front, so the message names the
    // construct rather than whatever lowering error it would cause first.
    if let Some(what) = instructions.iter().find_map(native_refusal) {
        return Err(native_unsupported(what));
    }

    // Step 1: Extract all function definitions from bytecode
    let extracted_funcs = extract_functions(instructions);
    let scopes = Scopes::analyze(instructions, &extracted_funcs);

    // Step 2: Lower each extracted function
    for func_info in &extracted_funcs {
        let func_bytecode = &instructions[func_info.address..=func_info.end_address];
        let func_name = format!("func_{}", func_info.address);

        // A function that cannot be lowered fails the whole module. Skipping
        // it would also shift the indices recorded in `function_addrs` below,
        // so calls would resolve to the wrong function.
        let ir_func = lower_extracted_function(
            &func_name,
            func_bytecode,
            &func_info.param_names,
            func_info.address,
            scopes.resolver_for(Some(func_info.address)),
            sites.clone(),
        )
        .map_err(|e| {
            let name = func_info
                .self_reference_var
                .as_deref()
                .unwrap_or(&func_name);
            LowerError::InFunction(name.to_string(), Box::new(e))
        })?;
        module.add_function(ir_func);
    }

    // Step 3: Lower the main code (treating skipped function bodies as jumps)
    let lowerer = Lowerer::new("main".to_string())
        .with_names(scopes.resolver_for(None))
        .with_sites(sites.clone(), 0);
    let main_func = lowerer.lower(instructions)?;
    module.add_function(main_func);
    if let Some(sites) = &sites {
        module.sites = sites.take();
    }

    // Store function address mappings in the module
    for (i, func_info) in extracted_funcs.iter().enumerate() {
        module.function_addrs.insert(func_info.address, i);
    }

    Ok(module)
}

/// Which names are local to which function, for `NameResolver`.
struct Scopes {
    /// Function start address -> (end address, locals).
    functions: Vec<(usize, usize, HashSet<String>)>,
    constant_functions: HashMap<String, usize>,
}

impl Scopes {
    fn analyze(instructions: &[OpCode], funcs: &[ExtractedFunction]) -> Self {
        let ranges: Vec<(usize, usize)> =
            funcs.iter().map(|f| (f.address, f.end_address)).collect();

        // The innermost function containing instruction `i`, if any.
        let owner = |i: usize| {
            ranges
                .iter()
                .filter(|(start, end)| *start <= i && i <= *end)
                .min_by_key(|(start, end)| end - start)
                .map(|(start, _)| *start)
        };

        let mut functions = Vec::new();
        for f in funcs {
            let mut locals: HashSet<String> = f.param_names.iter().cloned().collect();
            for i in f.address..=f.end_address.min(instructions.len().saturating_sub(1)) {
                if let OpCode::Let(name) = &instructions[i]
                    && owner(i) == Some(f.address)
                {
                    locals.insert(name.clone());
                }
            }
            functions.push((f.address, f.end_address, locals));
        }

        // A name assigned exactly once in the program, by
        // `Push(Function) ; Let(name)`, always holds that function.
        let mut assignments: HashMap<&str, usize> = HashMap::new();
        for op in instructions {
            if let OpCode::Let(name) | OpCode::Store(name) = op {
                *assignments.entry(name.as_str()).or_default() += 1;
            }
        }
        let mut constant_functions = HashMap::new();
        for w in instructions.windows(2) {
            if let [
                OpCode::Push(JsValue::Function { address, .. }),
                OpCode::Let(name),
            ] = w
                && assignments.get(name.as_str()) == Some(&1)
            {
                constant_functions.insert(name.clone(), *address);
            }
        }

        Self {
            functions,
            constant_functions,
        }
    }

    /// The resolver for the function starting at `addr`, or for top-level
    /// code when `addr` is `None`.
    fn resolver_for(&self, addr: Option<usize>) -> NameResolver {
        let mut names = NameResolver {
            program: true,
            constant_functions: self.constant_functions.clone(),
            ..Default::default()
        };
        let Some(addr) = addr else {
            return names;
        };
        let Some((_, end, locals)) = self.functions.iter().find(|(a, _, _)| *a == addr) else {
            return names;
        };
        names.locals = locals.clone();
        for (start, other_end, other_locals) in &self.functions {
            let encloses = *start < addr && end <= other_end;
            if encloses {
                names
                    .enclosing
                    .extend(other_locals.difference(locals).cloned());
            }
        }
        names
    }
}

/// Lower an extracted function with known parameters.
fn lower_extracted_function(
    name: &str,
    instructions: &[OpCode],
    param_names: &[String],
    base_addr: usize,
    names: NameResolver,
    sites: Option<std::rc::Rc<Sites>>,
) -> Result<IrFunction, LowerError> {
    // Rebase jump targets to be relative to the function start
    let rebased = rebase_jump_targets(instructions, base_addr);
    let mut lowerer = Lowerer::new_with_params(name.to_string(), param_names)
        .with_names(names)
        .with_sites(sites, base_addr);

    for param_name in param_names {
        lowerer.get_or_create_local(param_name);
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
            // 0 means "none".
            OpCode::SetupTry {
                catch_addr,
                finally_addr,
            } => {
                let rebase = |a: usize| {
                    if a == 0 {
                        0
                    } else {
                        a.saturating_sub(base_addr)
                    }
                };
                OpCode::SetupTry {
                    catch_addr: rebase(*catch_addr),
                    finally_addr: rebase(*finally_addr),
                }
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
        let found = match op {
            OpCode::Push(JsValue::Function { address, env }) => Some((*address, env.is_some())),
            OpCode::MakeClosure(address) => Some((*address, true)),
            _ => None,
        };
        if let Some((address, has_env)) = found {
            let address = &address;
            // The compiler emits `Jump(end)` immediately before every function
            // body, so the body is exactly `address..end`. Fall back to the
            // heuristic only if that pattern is missing.
            let end = match address.checked_sub(1).map(|i| &instructions[i]) {
                Some(OpCode::Jump(end)) if *end > *address => Some(end - 1),
                _ => find_function_end(*address, instructions),
            };
            if let Some(end_addr) = end {
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
                    has_env,
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
            OpCode::Load(name) | OpCode::TypeOfName(name) if name == var_name => return true,
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
    // `...rest` is not an argument: `RestArgs` makes it from the rest.
    let (start, rest) = match instructions.get(start) {
        Some(OpCode::RestArgs(_)) => (start + 1, true),
        _ => (start, false),
    };

    for i in start..instructions.len() {
        match &instructions[i] {
            OpCode::Let(name) => {
                params.push(name.clone());
            }
            _ => break, // Stop at first non-Let instruction
        }
    }

    if rest {
        // The first `Let` binds the rest parameter.
        if !params.is_empty() {
            params.remove(0);
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
