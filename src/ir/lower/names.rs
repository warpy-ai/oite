//! Names: whether a name in a function is a local, a known function or a
//! global.

use super::*;

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

pub(super) enum NameRef {
    Local,
    Function(usize),
    Global,
}

impl NameResolver {
    pub(super) fn resolve(&self, name: &str) -> Result<NameRef, LowerError> {
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

/// Which names are local to which function, for `NameResolver`.
pub(super) struct Scopes {
    /// Function start address -> (end address, locals).
    functions: Vec<(usize, usize, HashSet<String>)>,
    constant_functions: HashMap<String, usize>,
}

impl Scopes {
    pub(super) fn analyze(instructions: &[OpCode], funcs: &[ExtractedFunction]) -> Self {
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
    pub(super) fn resolver_for(&self, addr: Option<usize>) -> NameResolver {
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
