//! Lowering a whole program: finding its functions in the bytecode and
//! lowering each.

use super::*;

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
pub(super) fn native_refusal(op: &OpCode) -> Option<&'static str> {
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

/// Lower an extracted function with known parameters.
pub(super) fn lower_extracted_function(
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
pub(super) fn rebase_jump_targets(instructions: &[OpCode], base_addr: usize) -> Vec<OpCode> {
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
pub(super) fn extract_functions(instructions: &[OpCode]) -> Vec<ExtractedFunction> {
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
pub(super) fn detect_self_reference(
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
pub(super) fn detect_function_params(
    start: usize,
    instructions: &[OpCode],
) -> (usize, Vec<String>) {
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
pub(super) fn find_function_end(start: usize, instructions: &[OpCode]) -> Option<usize> {
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
