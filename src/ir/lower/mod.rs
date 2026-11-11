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

mod blocks;
mod instr;
mod module;
mod names;
mod sites;
#[cfg(test)]
mod tests;

pub use module::*;
pub use names::*;
pub use sites::*;

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
}
