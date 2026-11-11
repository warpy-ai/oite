//! Basic blocks: where they begin, `try` regions, resume points of async
//! functions, and lowering each reachable block.

use super::*;

impl Lowerer {
    /// Where each `try` statement's `catch` starts, and which `EnterFinally`
    /// ends each `finally`: the first one after the `finally` starts that
    /// is not the end of a nested `try`'s `finally`.
    pub(super) fn find_try_structure(&mut self, instructions: &[OpCode]) {
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
    pub(super) fn reachable_instructions(&self, instructions: &[OpCode]) -> HashSet<usize> {
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
    pub(super) fn build_resume_dispatch(&mut self) {
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
    pub(super) fn find_block_boundaries(&mut self, instructions: &[OpCode]) {
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
    pub(super) fn create_blocks(&mut self, instructions: &[OpCode]) {
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

    /// Lower all instructions.
    pub(super) fn lower_instructions(&mut self, instructions: &[OpCode]) -> Result<(), LowerError> {
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
    pub(super) fn try_slots(&mut self, setup: usize) -> (u32, u32) {
        (
            self.get_or_create_local(&format!("%thrown{}", setup)),
            self.get_or_create_local(&format!("%exception{}", setup)),
        )
    }

    /// `SetupTry` at `idx`. An exception in the `try` block goes to the
    /// `catch`; without one, or from inside the `catch`, it goes to a block
    /// that records it and enters the `finally`, whose end rethrows it.
    pub(super) fn lower_setup_try(
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
    pub(super) fn compute_reachable_blocks(&self, instructions: &[OpCode]) -> HashSet<BlockId> {
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
}
