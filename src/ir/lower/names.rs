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
    /// declaration that runs once. They resolve to that function directly.
    constant_functions: HashMap<String, usize>,
    /// The functions, by address, that are one function object made when
    /// the program starts rather than a new one each time their expression
    /// runs (see `Scopes::analyze`).
    made_once: HashSet<usize>,
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

    /// Whether the function at `addr` is made once, when the program
    /// starts, rather than each time its expression runs.
    pub(super) fn made_once(&self, addr: usize) -> bool {
        self.made_once.contains(&addr)
    }
}

/// Which names are local to which function, for `NameResolver`.
pub(super) struct Scopes {
    /// Function start address -> (end address, locals).
    functions: Vec<(usize, usize, HashSet<String>)>,
    constant_functions: HashMap<String, usize>,
    made_once: HashSet<usize>,
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
        // `Push(Function) ; Let(name)` in code that runs once (top-level
        // code outside any loop), always holds that one function. An
        // expression that runs more than once makes a new function each
        // time. A declaration in a function that the function only ever
        // calls (`Load(name) ; Call`) is never seen as a value, though, so
        // one function made at start stands for all of them.
        let mut assignments: HashMap<&str, usize> = HashMap::new();
        let mut read_as_value: HashSet<&str> = HashSet::new();
        for (i, op) in instructions.iter().enumerate() {
            match op {
                OpCode::Let(name) | OpCode::Store(name) => {
                    *assignments.entry(name.as_str()).or_default() += 1;
                }
                OpCode::Load(name) if !matches!(instructions.get(i + 1), Some(OpCode::Call(_))) => {
                    read_as_value.insert(name.as_str());
                }
                _ => {}
            }
        }
        let loops: Vec<(usize, usize)> = instructions
            .iter()
            .enumerate()
            .filter_map(|(i, op)| match op {
                OpCode::Jump(t) | OpCode::JumpIfFalse(t) if *t <= i => Some((*t, i)),
                _ => None,
            })
            .collect();
        let runs_once =
            |i: usize| owner(i).is_none() && !loops.iter().any(|&(t, j)| t <= i && i < j);
        let mut constant_functions = HashMap::new();
        let mut made_once = HashSet::new();
        for (i, w) in instructions.windows(2).enumerate() {
            if let [
                OpCode::Push(JsValue::Function { address, .. }),
                OpCode::Let(name),
            ] = w
                && assignments.get(name.as_str()) == Some(&1)
            {
                if runs_once(i) {
                    constant_functions.insert(name.clone(), *address);
                    made_once.insert(*address);
                } else if owner(i).is_some() && !read_as_value.contains(name.as_str()) {
                    made_once.insert(*address);
                }
            }
        }

        Self {
            functions,
            constant_functions,
            made_once,
        }
    }

    /// The resolver for the function starting at `addr`, or for top-level
    /// code when `addr` is `None`.
    pub(super) fn resolver_for(&self, addr: Option<usize>) -> NameResolver {
        let mut names = NameResolver {
            program: true,
            constant_functions: self.constant_functions.clone(),
            made_once: self.made_once.clone(),
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
