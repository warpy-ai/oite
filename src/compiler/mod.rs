use crate::vm::opcodes::OpCode;
use std::collections::HashMap;
use swc_ecma_ast::*;
pub mod borrow_ck;
pub mod diagnostic;
pub mod modules;
pub mod scope;
use crate::compiler::borrow_ck::BorrowChecker;
use crate::compiler::modules::ModuleLink;
use crate::compiler::scope::{PARENT_PROP, SCOPE_LOCAL, ScopeInfo, VarRef};
use crate::vm::value::JsValue;
pub use diagnostic::{
    CompileError, DebugInfo, Diagnostic, FunctionRange, NO_POSITION, Position, SourceTable,
    format_frame,
};
use swc_common::{FileName, SourceMap, Span, Spanned, sync::Lrc};
use swc_ecma_parser::{Parser, StringInput, Syntax, lexer::Lexer};

pub struct Compiler {
    pub borrow_checker: BorrowChecker,
    /// Source positions of the last program compiled.
    pub debug_info: DebugInfo,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            borrow_checker: BorrowChecker::new(),
            debug_info: DebugInfo::default(),
        }
    }

    pub fn compile(&mut self, source: &str) -> Result<Vec<OpCode>, CompileError> {
        self.compile_with_syntax(source, None)
    }

    pub fn compile_with_syntax(
        &mut self,
        source: &str,
        syntax_override: Option<Syntax>,
    ) -> Result<Vec<OpCode>, CompileError> {
        let parsed = parse(source, syntax_override)?;
        self.compile_parsed(&parsed, None)
    }

    /// Compile a parsed program. With `link`, it is a module of a linked
    /// program (`modules.rs`): its top-level names get the module's prefix
    /// and its imports name the bindings they import.
    pub(crate) fn compile_parsed(
        &mut self,
        parsed: &Parsed,
        link: Option<ModuleLink>,
    ) -> Result<Vec<OpCode>, CompileError> {
        let (cm, fm, program) = (&parsed.cm, &parsed.fm, &parsed.program);
        let position = |span: Span| position(cm, span);

        self.borrow_checker.error_span = None;
        self.borrow_checker.error_note = None;
        self.borrow_checker.enter_scope(); // Script vars at depth 1, globals at 0

        let result = match program {
            Program::Module(module) => {
                let mut result = Ok(());
                for item in &module.body {
                    if let ModuleItem::Stmt(stmt) = item
                        && let Err(e) = self.borrow_checker.analyze_stmt(stmt)
                    {
                        result = Err(e);
                        break;
                    }
                }
                result
            }
            Program::Script(script) => {
                let mut result = Ok(());
                for stm in &script.body {
                    if let Err(e) = self.borrow_checker.analyze_stmt(stm) {
                        result = Err(e);
                        break;
                    }
                }
                result
            }
        };

        self.borrow_checker.exit_scope();
        if let Err(message) = result {
            let (kind, message) = diagnostic::split_borrow_message(&message);
            let mut d = Diagnostic::new(
                kind,
                message,
                self.borrow_checker.error_span.and_then(position),
            );
            d.note = self
                .borrow_checker
                .error_note
                .take()
                .and_then(|(note, span)| Some((note, position(span)?)));
            return Err(CompileError::single(d));
        }

        let mut codegen = Codegen::new();
        codegen.link = link;
        match program {
            Program::Module(module) => {
                codegen.scopes = ScopeInfo::analyze_module(module);
                codegen.generate(module);
            }
            Program::Script(script) => {
                codegen.scopes = ScopeInfo::analyze_script(script);
                codegen.generate_script(script);
            }
        }

        if !codegen.errors.is_empty() {
            let diagnostics = codegen
                .errors
                .iter()
                .map(|(span, msg)| Diagnostic::new("error", msg.clone(), position(*span)))
                .collect();
            return Err(CompileError::new(diagnostics));
        }

        // Each instruction's position: the last mark at or before it.
        let start = fm.start_pos.0;
        let mut positions = vec![NO_POSITION; codegen.instructions.len()];
        let mut marks = codegen.marks.iter().peekable();
        let mut current = NO_POSITION;
        for (i, slot) in positions.iter_mut().enumerate() {
            while let Some(&&(at, pos)) = marks.peek() {
                if at > i {
                    break;
                }
                current = if pos >= start {
                    pos - start
                } else {
                    NO_POSITION
                };
                marks.next();
            }
            *slot = current;
        }
        self.debug_info = DebugInfo {
            positions,
            functions: std::mem::take(&mut codegen.functions),
        };

        Ok(codegen.instructions)
    }
}

/// A parsed source file.
pub(crate) struct Parsed {
    pub cm: Lrc<SourceMap>,
    pub fm: Lrc<swc_common::SourceFile>,
    pub program: Program,
}

/// The line and column of `span`, if it is a real position.
pub(crate) fn position(cm: &SourceMap, span: Span) -> Option<Position> {
    (!span.is_dummy()).then(|| {
        let loc = cm.lookup_char_pos(span.lo);
        Position {
            line: loc.line,
            col: loc.col_display + 1,
        }
    })
}

/// Parse `source` (TypeScript unless `syntax` says otherwise). Syntax errors
/// the parser recovered from fail too.
pub(crate) fn parse(source: &str, syntax: Option<Syntax>) -> Result<Parsed, CompileError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        FileName::Custom("main.ot".into()).into(),
        blank_module_directives(source),
    );
    let syntax = syntax.unwrap_or_else(|| Syntax::Typescript(Default::default()));
    let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);
    let format_parse_error = |e: swc_ecma_parser::error::Error| {
        Diagnostic::new("syntax error", e.kind().msg(), position(&cm, e.span()))
    };
    let program = parser
        .parse_program()
        .map_err(|e| CompileError::single(format_parse_error(e)))?;
    // The parser recovers from some errors and keeps going; those must
    // still fail compilation instead of producing a partial program.
    let recovered: Vec<Diagnostic> = parser
        .take_errors()
        .into_iter()
        // Oite allows a top-level `return`: it sets the program's result
        // (see `oitec jit`).
        .filter(|e| {
            !matches!(
                e.kind(),
                swc_ecma_parser::error::SyntaxError::ReturnNotAllowed
            )
        })
        .map(format_parse_error)
        .collect();
    if !recovered.is_empty() {
        return Err(CompileError::new(recovered));
    }
    Ok(Parsed { cm, fm, program })
}

/// Blank out `require ./path;` lines (the `;` is optional), keeping byte
/// offsets intact.
///
/// This is the self-hosted compiler's module directive (`compiler/*.ot`,
/// `bootstrap/*.ot`), parsed there as a `RequireStatement`. It is not valid
/// JavaScript, and the Rust toolchain loads those modules from a fixed list in
/// `main.rs`, so for this compiler the line has no meaning.
fn blank_module_directives(source: &str) -> String {
    source
        .split_inclusive('\n')
        .map(|line| {
            let body = line.trim_end_matches(['\n', '\r']);
            let t = body.trim();
            let is_directive = t.strip_prefix("require ./").is_some_and(|rest| {
                let path = rest.strip_suffix(';').unwrap_or(rest);
                !path.is_empty()
                    && path
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/')
            });
            if is_directive {
                let mut blank = " ".repeat(body.len());
                blank.push_str(&line[body.len()..]);
                blank
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// What a `break` or `continue` can jump to.
#[derive(Clone, Copy, PartialEq)]
enum TargetKind {
    /// A loop: the target of `continue`, and of an unlabeled `break`.
    Loop,
    /// A `switch`: the target of an unlabeled `break`.
    Switch,
    /// Any other labeled statement: only `break label` reaches it.
    Labeled,
}

/// A statement that `break`/`continue` can leave, innermost last.
struct LoopContext {
    kind: TargetKind,
    labels: Vec<String>,
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
    /// Saved `%scope` to restore before `break` (the statement's enclosing
    /// environment) and before `continue` (the current iteration's).
    break_env: Option<String>,
    continue_env: Option<String>,
    /// How many `try` regions were open when the statement began; a jump
    /// out of it closes the ones above.
    try_depth: usize,
}

/// Which part of a `try` statement code is compiled in.
#[derive(Clone, Copy, PartialEq)]
enum TryPart {
    Try,
    Catch,
    Finally,
}

/// An enclosing `try` statement. A `break`, `continue` or `return` that
/// leaves it pops its exception handler and runs its `finally` block.
#[derive(Clone)]
struct TryRegion {
    part: TryPart,
    /// `%scope` at the `try` statement.
    env: Option<String>,
    finalizer: Option<swc_ecma_ast::BlockStmt>,
}

impl TryRegion {
    /// Whether the VM has a handler installed for this region: always in
    /// the `try` block, and in a `catch` that a `finally` follows (the VM
    /// re-arms the finally-only handler when it enters the catch).
    fn has_handler(&self) -> bool {
        match self.part {
            TryPart::Try => true,
            TryPart::Catch => self.finalizer.is_some(),
            TryPart::Finally => false,
        }
    }
}

/// Jump targets and `try` regions; a function body starts with none.
#[derive(Default)]
struct ControlState {
    loops: Vec<LoopContext>,
    tries: Vec<TryRegion>,
    pending_labels: Vec<String>,
}

/// An instance field: its property key, its initializer, and its
/// decorators (each may have returned an initializer the value goes
/// through).
#[derive(Clone)]
struct ClassField {
    key: String,
    value: Option<Box<Expr>>,
    decorators: Vec<Span>,
}

/// What a function body belongs to; it decides the implicit result and
/// what runs before the body.
#[derive(Clone, Copy, PartialEq)]
enum FnKind {
    Plain,
    Method {
        is_static: bool,
    },
    /// A class constructor (`implicit`: the class declares none).
    Ctor {
        derived: bool,
        implicit: bool,
    },
}

/// Local holding an async function's promise (`vm::promise::PROMISE_LOCAL`).
const ASYNC_PROMISE: &str = "%promise";

pub struct Codegen {
    pub instructions: Vec<OpCode>,
    scope_stack: Vec<Vec<String>>,
    in_function: bool,
    in_async_function: bool,
    /// Stack of loop contexts for nested loops (break/continue support)
    loop_stack: Vec<LoopContext>,
    /// Enclosing `try` statements, innermost last.
    try_stack: Vec<TryRegion>,
    /// Labels of the loop about to be compiled (`outer: for (...)`).
    pending_labels: Vec<String>,
    /// Function declarations already defined at the top of their
    /// statement list (hoisting), by span.
    hoisted_fns: std::collections::HashSet<scope::SpanKey>,
    /// Instance fields of the class whose derived constructor is being
    /// compiled: `super(...)` initializes them once it returns.
    class_fields: Option<std::rc::Rc<Vec<ClassField>>>,
    /// Compiling a static method: `super` is the parent class itself, not
    /// its prototype.
    in_static_method: bool,
    /// Private names of the enclosing classes, innermost last: `#x` -> its
    /// property key, unique to the class that declares it.
    private_names: Vec<HashMap<String, String>>,
    /// Warnings collected during compilation
    pub warnings: Vec<String>,
    /// Hard errors collected during code generation. Compilation fails if
    /// any are present: a construct the compiler cannot lower must never be
    /// dropped silently.
    pub errors: Vec<(Span, String)>,
    /// Span of the innermost statement or expression being compiled, used
    /// to locate errors.
    current_span: Span,
    /// Source positions: from instruction `.0` on, code comes from byte
    /// `.1` (a `BytePos`), until the next mark.
    pub marks: Vec<(usize, u32)>,
    /// The position of the code being generated (what `marks` returns to
    /// after a nested expression).
    mark_span: Span,
    /// Function bodies and their names, for stack traces.
    pub functions: Vec<FunctionRange>,
    /// The name of the next function body (`function f`, `let f = () =>`,
    /// `C.method`).
    next_fn_name: Option<String>,
    /// The class whose members are being compiled.
    class_name: Option<String>,
    /// Where every variable lives (see `scope.rs`).
    pub scopes: ScopeInfo,
    /// Whether the code being generated keeps its innermost environment in
    /// `%scope` (some variable in reach is captured).
    env_active: bool,
    /// This program is a module of a linked program (`modules.rs`): how
    /// its top-level names and imports are named.
    pub(crate) link: Option<ModuleLink>,
}

impl Default for Codegen {
    fn default() -> Self {
        Self::new()
    }
}

impl Codegen {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            scope_stack: vec![Vec::new()],
            in_function: false,
            in_async_function: false,
            loop_stack: Vec::new(),
            try_stack: Vec::new(),
            pending_labels: Vec::new(),
            hoisted_fns: std::collections::HashSet::new(),
            class_fields: None,
            in_static_method: false,
            private_names: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            current_span: Span::default(),
            marks: Vec::new(),
            mark_span: Span::default(),
            functions: Vec::new(),
            next_fn_name: None,
            class_name: None,
            scopes: ScopeInfo::default(),
            env_active: false,
            link: None,
        }
    }

    /// `[value] -> [promise]`: an async function's result resolves its
    /// promise, which is what the function returns.
    fn emit_async_return(&mut self) {
        self.instructions
            .push(OpCode::Load(ASYNC_PROMISE.to_string()));
        self.instructions.push(OpCode::AsyncResolve);
    }

    /// `x op= v`, `o.p op= v`, `o[k] op= v`: read, combine, write back,
    /// evaluating the target's object and key once. Before this, the
    /// operator was ignored and `x += 2` compiled as `x = 2`.
    fn gen_compound_assign(&mut self, assign: &AssignExpr) {
        let op = match assign.op {
            AssignOp::AddAssign => OpCode::Add,
            AssignOp::SubAssign => OpCode::Sub,
            AssignOp::MulAssign => OpCode::Mul,
            AssignOp::DivAssign => OpCode::Div,
            AssignOp::ModAssign => OpCode::Mod,
            AssignOp::ExpAssign => OpCode::Pow,
            AssignOp::BitAndAssign => OpCode::BitAnd,
            AssignOp::BitOrAssign => OpCode::BitOr,
            AssignOp::BitXorAssign => OpCode::Xor,
            AssignOp::LShiftAssign => OpCode::ShiftLeft,
            AssignOp::RShiftAssign => OpCode::ShiftRight,
            AssignOp::ZeroFillRShiftAssign => OpCode::ShiftRightUnsigned,
            other => {
                self.unsupported(format!("assignment operator `{}`", other.as_str()));
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                return;
            }
        };
        let AssignTarget::Simple(target) = &assign.left else {
            self.unsupported("destructuring assignment");
            self.instructions.push(OpCode::Push(JsValue::Undefined));
            return;
        };
        match target {
            SimpleAssignTarget::Ident(id) => {
                self.load_var(&id.id);
                self.gen_expr(&assign.right);
                self.instructions.push(op);
                self.instructions.push(OpCode::Dup);
                self.store_var(&id.id, false);
            }
            SimpleAssignTarget::Member(member) => match &member.prop {
                MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                    // [obj] -> [obj, cur] -> [obj, cur, rhs] -> [obj, v]
                    self.gen_expr(&member.obj);
                    self.instructions.push(OpCode::Dup);
                    self.emit_member_get(&member.prop);
                    self.gen_expr(&assign.right);
                    self.instructions.push(op);
                    // [obj, v] -> [v, obj, v] -> SetProp -> [v]
                    self.instructions.push(OpCode::Dup);
                    self.instructions.push(OpCode::Swap3);
                    self.instructions.push(OpCode::Swap);
                    self.emit_member_set(&member.prop);
                }
                MemberProp::Computed(computed) => {
                    let obj = format!("%obj{}", self.instructions.len());
                    let key = format!("%key{}", self.instructions.len());
                    self.gen_expr(&member.obj);
                    self.instructions.push(OpCode::Let(obj.clone()));
                    self.gen_expr(&computed.expr);
                    self.instructions.push(OpCode::Let(key.clone()));
                    self.instructions.push(OpCode::Load(obj.clone()));
                    self.instructions.push(OpCode::Load(key.clone()));
                    self.instructions.push(OpCode::GetPropComputed);
                    self.gen_expr(&assign.right);
                    self.instructions.push(op);
                    // [v] -> [obj, v] -> [v, obj, v] -> [v, obj, v, key]
                    self.instructions.push(OpCode::Load(obj));
                    self.instructions.push(OpCode::Swap);
                    self.instructions.push(OpCode::Dup);
                    self.instructions.push(OpCode::Swap3);
                    self.instructions.push(OpCode::Swap);
                    self.instructions.push(OpCode::Load(key));
                    self.instructions.push(OpCode::SetPropComputed);
                }
            },
            _ => {
                self.unsupported("this assignment target");
                self.instructions.push(OpCode::Push(JsValue::Undefined));
            }
        }
    }

    // ---------------------------------------------------------------------
    // Variables and environments (see `scope.rs`)
    // ---------------------------------------------------------------------

    fn top_level_prologue(&mut self) {
        if self.scopes.top_level_uses_env() {
            self.instructions.push(OpCode::Push(JsValue::Undefined));
            self.instructions.push(OpCode::Let(SCOPE_LOCAL.to_string()));
            self.env_active = true;
        }
    }

    /// Push the environment `hops` parent links above `%scope`.
    fn emit_env(&mut self, hops: u32) {
        self.instructions
            .push(OpCode::Load(SCOPE_LOCAL.to_string()));
        for _ in 0..hops {
            self.instructions
                .push(OpCode::GetProp(PARENT_PROP.to_string()));
        }
    }

    /// `[] -> [value of ident]`
    /// The name a variable has in the program: a top-level binding of a
    /// linked module has the module's prefix, and an import names the
    /// binding it imports.
    fn var_name(&self, ident: &Ident) -> String {
        let name = ident.sym.to_string();
        match &self.link {
            Some(link) if self.scopes.is_module_level(ident.span) => {
                match link.imports.get(&name) {
                    Some(global) => global.clone(),
                    None => format!("{}{}", link.prefix, name),
                }
            }
            _ => name,
        }
    }

    fn load_var(&mut self, ident: &Ident) {
        let name = self.var_name(ident);
        match self.scopes.var(ident.span) {
            VarRef::Plain => self.instructions.push(OpCode::Load(name)),
            VarRef::Env { hops } => {
                self.emit_env(hops);
                self.instructions.push(OpCode::GetProp(name));
            }
        }
    }

    /// `[value] -> []`: assign to (or, with `declare`, bind) `ident`.
    fn store_var(&mut self, ident: &Ident, declare: bool) {
        if let Some(link) = &self.link
            && self.scopes.is_module_level(ident.span)
            && (link.imports.contains_key(&*ident.sym) || link.namespaces.contains_key(&*ident.sym))
        {
            let message = format!("cannot assign to '{}': it is an import", ident.sym);
            self.errors.push((ident.span, message));
        }
        let name = self.var_name(ident);
        match self.scopes.var(ident.span) {
            VarRef::Plain if declare => self.instructions.push(OpCode::Let(name)),
            VarRef::Plain => self.instructions.push(OpCode::Store(name)),
            VarRef::Env { hops } => {
                self.emit_env(hops);
                self.instructions.push(OpCode::Swap);
                self.instructions.push(OpCode::SetProp(name));
            }
        }
    }

    /// `[] -> [new environment whose parent is %scope]`
    fn emit_child_env(&mut self) {
        self.instructions.push(OpCode::NewObject);
        self.instructions.push(OpCode::Dup);
        self.instructions
            .push(OpCode::Load(SCOPE_LOCAL.to_string()));
        self.instructions
            .push(OpCode::SetProp(PARENT_PROP.to_string()));
    }

    /// Remember `%scope` in a fresh local, so it can be restored after a
    /// block, at a loop's continue and exit points, or in a catch handler.
    fn save_env(&mut self) -> Option<String> {
        if !self.env_active {
            return None;
        }
        let save = format!("%save{}", self.instructions.len());
        self.instructions
            .push(OpCode::Load(SCOPE_LOCAL.to_string()));
        self.instructions.push(OpCode::Let(save.clone()));
        Some(save)
    }

    fn restore_env(&mut self, save: &Option<String>) {
        if let Some(save) = save {
            self.instructions.push(OpCode::Load(save.clone()));
            self.instructions
                .push(OpCode::Store(SCOPE_LOCAL.to_string()));
        }
    }

    /// Enter the scope opened by the node at `span`: if it has captured
    /// bindings, a new environment becomes `%scope`. Returns what
    /// `exit_scope_env` needs to restore the enclosing one.
    fn enter_scope_env(&mut self, span: Span) -> Option<String> {
        self.scopes.env_scope(span)?;
        let save = self.save_env();
        self.emit_child_env();
        self.instructions
            .push(OpCode::Store(SCOPE_LOCAL.to_string()));
        save
    }

    fn exit_scope_env(&mut self, save: Option<String>) {
        self.restore_env(&save);
    }

    /// Start of a function body, after the parameters are bound: set up
    /// `%scope` (the closure's environment, then the function's own) and
    /// move captured parameters into it. Returns the previous `env_active`.
    fn function_prologue(&mut self, fn_span: Span, params: &[&Ident]) -> bool {
        let prev = self.env_active;
        self.env_active = self.scopes.uses_env(fn_span);
        if !self.env_active {
            return prev;
        }
        if self.scopes.is_closure(fn_span) {
            self.instructions.push(OpCode::LoadClosureEnv);
        } else {
            self.instructions.push(OpCode::Push(JsValue::Undefined));
        }
        self.instructions.push(OpCode::Let(SCOPE_LOCAL.to_string()));
        if let Some(captured) = self.scopes.env_scope(fn_span) {
            let captured: Vec<String> = captured.to_vec();
            self.emit_child_env();
            self.instructions
                .push(OpCode::Store(SCOPE_LOCAL.to_string()));
            for p in params {
                let name = p.sym.to_string();
                if captured.contains(&name) {
                    self.instructions.push(OpCode::Load(name.clone()));
                    self.store_var(p, true);
                }
            }
            // Arrow functions inside read this function's `this` from here.
            if captured.iter().any(|n| n == scope::THIS_LOCAL) {
                self.instructions.push(OpCode::LoadThis);
                self.store_var(&scope::this_ident(scope::this_decl_span(fn_span)), true);
            }
        }
        prev
    }

    /// Bind the arguments on the stack to `params` (in reverse order). The
    /// function must start with exactly these `Let`s: the VM and IR lowering
    /// count them as its parameters. Returns the parameter identifiers.
    fn bind_params<'p>(&mut self, params: &[&'p Pat]) -> Vec<&'p Ident> {
        // `...rest` (last): the caller passes an array of the extra arguments.
        if let Some((Pat::Rest(_), before)) = params.split_last() {
            self.instructions.push(OpCode::RestArgs(before.len()));
        }
        let mut idents = Vec::new();
        for (i, pat) in params.iter().enumerate().rev() {
            let ident = match pat {
                Pat::Ident(id) => Some(&id.id),
                Pat::Assign(a) => match a.left.as_ref() {
                    Pat::Ident(id) => Some(&id.id),
                    _ => None,
                },
                Pat::Rest(r) if i + 1 == params.len() => match r.arg.as_ref() {
                    Pat::Ident(id) => Some(&id.id),
                    _ => None,
                },
                _ => None,
            };
            match ident {
                Some(id) => {
                    self.instructions.push(OpCode::Let(id.sym.to_string()));
                    idents.push(id);
                }
                None => {
                    self.current_span = pat.span();
                    self.unsupported("destructuring function parameters")
                }
            }
        }
        idents
    }

    /// `function f(a = 1)`: after the prologue, replace an `undefined`
    /// argument with its default value.
    fn apply_param_defaults(&mut self, params: &[&Pat]) {
        for pat in params {
            if let Pat::Assign(a) = pat
                && let Pat::Ident(id) = a.left.as_ref()
            {
                self.load_var(&id.id);
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.instructions.push(OpCode::Eq);
                let skip = self.emit_jump_if_false();
                self.gen_expr(&a.right);
                self.store_var(&id.id, false);
                self.patch_jump_here(skip);
            }
        }
    }

    /// Push the function value for the body starting at `address`: a
    /// closure over `%scope` when the function captures variables.
    /// Returns the index of the instruction holding the address, for
    /// patching.
    fn emit_function_value(&mut self, fn_span: Span, address: usize) -> usize {
        if self.scopes.is_closure(fn_span) {
            self.instructions
                .push(OpCode::Load(SCOPE_LOCAL.to_string()));
            self.instructions.push(OpCode::MakeClosure(address));
        } else {
            self.instructions
                .push(OpCode::Push(JsValue::Function { address, env: None }));
        }
        self.instructions.len() - 1
    }

    fn patch_function_value(&mut self, idx: usize, address: usize) {
        match &mut self.instructions[idx] {
            OpCode::MakeClosure(a) => *a = address,
            OpCode::Push(JsValue::Function { address: a, .. }) => *a = address,
            other => unreachable!("not a function value: {:?}", other),
        }
    }

    /// `[] -> [new environment: a copy of %scope's bindings `names`]`,
    /// then made `%scope`. Gives each loop iteration its own bindings.
    fn emit_iteration_env(&mut self, names: &[String]) {
        self.instructions.push(OpCode::NewObject);
        self.instructions.push(OpCode::Dup);
        self.emit_env(1);
        self.instructions
            .push(OpCode::SetProp(PARENT_PROP.to_string()));
        for name in names {
            self.instructions.push(OpCode::Dup);
            self.emit_env(0);
            self.instructions.push(OpCode::GetProp(name.clone()));
            self.instructions.push(OpCode::SetProp(name.clone()));
        }
        self.instructions
            .push(OpCode::Store(SCOPE_LOCAL.to_string()));
    }

    /// Record that a construct cannot be compiled. Compilation will fail.
    fn unsupported(&mut self, what: impl std::fmt::Display) {
        self.errors
            .push((self.current_span, format!("unsupported: {}", what)));
    }

    /// Record an error in the program at the current statement.
    fn syntax_error(&mut self, what: impl std::fmt::Display) {
        self.errors.push((self.current_span, what.to_string()));
    }

    /// Enter a function body: loops, labels and `try` statements outside it
    /// are not reachable from inside.
    fn enter_function_control(&mut self) -> ControlState {
        ControlState {
            loops: std::mem::take(&mut self.loop_stack),
            tries: std::mem::take(&mut self.try_stack),
            pending_labels: std::mem::take(&mut self.pending_labels),
        }
    }

    fn leave_function_control(&mut self, saved: ControlState) {
        self.loop_stack = saved.loops;
        self.try_stack = saved.tries;
        self.pending_labels = saved.pending_labels;
    }

    /// Start a loop's jump target, taking the labels in front of it.
    fn push_loop(&mut self, break_env: Option<String>, continue_env: Option<String>) {
        let labels = std::mem::take(&mut self.pending_labels);
        self.loop_stack.push(LoopContext {
            kind: TargetKind::Loop,
            labels,
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            break_env,
            continue_env,
            try_depth: self.try_stack.len(),
        });
    }

    /// End the innermost jump target: `break` goes to `break_addr`,
    /// `continue` to `continue_addr`.
    fn pop_target(&mut self, break_addr: usize, continue_addr: usize) {
        if let Some(ctx) = self.loop_stack.pop() {
            for idx in ctx.break_jumps {
                if let OpCode::Jump(ref mut addr) = self.instructions[idx] {
                    *addr = break_addr;
                }
            }
            for idx in ctx.continue_jumps {
                if let OpCode::Jump(ref mut addr) = self.instructions[idx] {
                    *addr = continue_addr;
                }
            }
        }
    }

    /// Leave the `try` regions above `depth`, innermost first: pop each
    /// installed handler and run each pending `finally` block inline.
    fn unwind_tries(&mut self, depth: usize) {
        let saved = self.try_stack.clone();
        while self.try_stack.len() > depth {
            let region = self.try_stack.pop().unwrap();
            if region.has_handler() {
                self.instructions.push(OpCode::PopTry);
            }
            if region.part != TryPart::Finally
                && let Some(finalizer) = &region.finalizer
            {
                self.restore_env(&region.env);
                self.gen_finally_body(finalizer);
            }
        }
        self.try_stack = saved;
    }

    fn gen_finally_body(&mut self, finalizer: &swc_ecma_ast::BlockStmt) {
        self.scope_stack.push(Vec::new());
        let env = self.enter_scope_env(finalizer.span);
        self.hoist_functions(&finalizer.stmts);
        for s in &finalizer.stmts {
            self.gen_stmt(s);
        }
        self.exit_scope_env(env);
        if let Some(locals) = self.scope_stack.pop() {
            for name in locals.into_iter().rev() {
                self.instructions.push(OpCode::Drop(name));
            }
        }
    }

    /// Define the function declarations among `stmts` up front, so they
    /// can be called before their position in the source (hoisting). The
    /// declaration itself then compiles to nothing.
    fn hoist_functions<'a>(&mut self, stmts: impl IntoIterator<Item = &'a Stmt>) {
        for stmt in stmts {
            if let Stmt::Decl(Decl::Fn(f)) = stmt {
                self.hoist_fn(f);
            }
        }
    }

    /// Start of a function body (or the program): `var` names are bound to
    /// `undefined` (unless they are parameters), then function declarations
    /// are defined.
    fn hoist_body(&mut self, stmts: &[Stmt], params: &[String]) {
        for ident in scope::var_declared(stmts) {
            if !params.iter().any(|p| *p == *ident.sym) {
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.store_var(&ident, true);
            }
        }
        self.hoist_functions(stmts);
    }

    fn hoist_fn(&mut self, f: &swc_ecma_ast::FnDecl) {
        if f.declare {
            return;
        }
        self.hoisted_fns.insert(scope::key(f.function.span));
        self.gen_fn_decl(Some(&f.ident), &f.function);
    }

    fn is_hoisted(&self, f: &swc_ecma_ast::FnDecl) -> bool {
        self.hoisted_fns.contains(&scope::key(f.function.span))
    }

    /// `break [label]` or `continue [label]`.
    fn gen_jump_stmt(&mut self, label: Option<&Ident>, is_continue: bool) {
        let keyword = if is_continue { "continue" } else { "break" };
        let found = match label {
            Some(label) => {
                let name = label.sym.as_ref();
                match self
                    .loop_stack
                    .iter()
                    .rposition(|c| c.labels.iter().any(|l| l == name))
                {
                    Some(i) if is_continue && self.loop_stack[i].kind != TargetKind::Loop => {
                        self.syntax_error(format!("`continue {name}`: `{name}` is not a loop"));
                        return;
                    }
                    Some(i) => i,
                    None => {
                        self.syntax_error(format!(
                            "`{keyword} {name}`: no enclosing label `{name}`"
                        ));
                        return;
                    }
                }
            }
            None => match self.loop_stack.iter().rposition(|c| match c.kind {
                TargetKind::Loop => true,
                TargetKind::Switch => !is_continue,
                TargetKind::Labeled => false,
            }) {
                Some(i) => i,
                None => {
                    let place = if is_continue {
                        "a loop"
                    } else {
                        "a loop or `switch`"
                    };
                    self.syntax_error(format!("`{keyword}` outside of {place}"));
                    return;
                }
            },
        };
        let depth = self.loop_stack[found].try_depth;
        self.unwind_tries(depth);
        let env = if is_continue {
            self.loop_stack[found].continue_env.clone()
        } else {
            self.loop_stack[found].break_env.clone()
        };
        self.restore_env(&env);
        let jump_idx = self.emit_jump();
        let ctx = &mut self.loop_stack[found];
        if is_continue {
            ctx.continue_jumps.push(jump_idx);
        } else {
            ctx.break_jumps.push(jump_idx);
        }
    }

    /// Emit a jump with a placeholder target; patch it with `patch_jump`.
    fn emit_jump(&mut self) -> usize {
        self.instructions.push(OpCode::Jump(0));
        self.instructions.len() - 1
    }

    /// Emit a conditional jump with a placeholder target.
    fn emit_jump_if_false(&mut self) -> usize {
        self.instructions.push(OpCode::JumpIfFalse(0));
        self.instructions.len() - 1
    }

    /// Point a placeholder jump at the next instruction to be emitted.
    fn patch_jump_here(&mut self, idx: usize) {
        let here = self.instructions.len();
        match &mut self.instructions[idx] {
            OpCode::Jump(addr) | OpCode::JumpIfFalse(addr) => *addr = here,
            other => unreachable!("patch_jump_here on non-jump {:?}", other),
        }
    }

    /// With a value on top of the stack, emit jumps taken when it is `null`
    /// or `undefined`. The value stays on the stack on every path.
    ///
    /// Uses two strict comparisons rather than `== null`, so that every
    /// backend computes the same answer.
    fn emit_jumps_if_nullish(&mut self) -> Vec<usize> {
        let mut jumps = Vec::with_capacity(2);
        for nullish in [JsValue::Null, JsValue::Undefined] {
            self.instructions.push(OpCode::Dup);
            self.instructions.push(OpCode::Push(nullish));
            self.instructions.push(OpCode::Eq);
            self.instructions.push(OpCode::Not);
            jumps.push(self.emit_jump_if_false());
        }
        jumps
    }

    /// `a && b`, `a || b`, `a ?? b`: evaluate `b` only when `a` does not
    /// decide the result. The result is the deciding operand's value.
    fn gen_logical(&mut self, bin: &BinExpr) {
        self.gen_expr(&bin.left);
        match bin.op {
            BinaryOp::LogicalAnd => {
                // [a] -> falsy: keep a; truthy: replace with b
                self.instructions.push(OpCode::Dup);
                let to_end = self.emit_jump_if_false();
                self.instructions.push(OpCode::Pop);
                self.gen_expr(&bin.right);
                self.patch_jump_here(to_end);
            }
            BinaryOp::LogicalOr => {
                // [a] -> truthy: keep a; falsy: replace with b
                self.instructions.push(OpCode::Dup);
                let to_rhs = self.emit_jump_if_false();
                let to_end = self.emit_jump();
                self.patch_jump_here(to_rhs);
                self.instructions.push(OpCode::Pop);
                self.gen_expr(&bin.right);
                self.patch_jump_here(to_end);
            }
            BinaryOp::NullishCoalescing => {
                // [a] -> not null/undefined: keep a; else replace with b
                let to_rhs = self.emit_jumps_if_nullish();
                let to_end = self.emit_jump();
                for j in to_rhs {
                    self.patch_jump_here(j);
                }
                self.instructions.push(OpCode::Pop);
                self.gen_expr(&bin.right);
                self.patch_jump_here(to_end);
            }
            _ => unreachable!("gen_logical called with {:?}", bin.op),
        }
    }

    /// Optional chain `a?.b.c`, `a?.[k]`, `f?.()`, `o.m?.()`.
    ///
    /// If any `?.` link sees `null`/`undefined`, the whole chain evaluates to
    /// `undefined` and the rest of it is not evaluated.
    fn gen_opt_chain_expr(&mut self, chain: &OptChainExpr) {
        let mut to_undefined = Vec::new();
        self.gen_opt_chain(chain, &mut to_undefined);
        if to_undefined.is_empty() {
            return;
        }
        let to_end = self.emit_jump();
        for j in to_undefined {
            self.patch_jump_here(j);
        }
        // Every short-circuit jump leaves exactly the nullish value on the stack.
        self.instructions.push(OpCode::Pop);
        self.instructions.push(OpCode::Push(JsValue::Undefined));
        self.patch_jump_here(to_end);
    }

    /// Evaluate the object of a chain link. A nested `OptChain` is part of
    /// the same chain and shares its short-circuit target.
    fn gen_opt_chain_object(&mut self, expr: &Expr, to_undefined: &mut Vec<usize>) {
        match expr {
            Expr::OptChain(inner) => self.gen_opt_chain(inner, to_undefined),
            other => self.gen_expr(other),
        }
    }

    fn gen_opt_chain(&mut self, chain: &OptChainExpr, to_undefined: &mut Vec<usize>) {
        match chain.base.as_ref() {
            OptChainBase::Member(member) => {
                self.gen_opt_chain_object(&member.obj, to_undefined);
                if chain.optional {
                    to_undefined.extend(self.emit_jumps_if_nullish());
                }
                match &member.prop {
                    MemberProp::Ident(id) => {
                        self.instructions.push(OpCode::GetProp(id.sym.to_string()));
                    }
                    MemberProp::Computed(computed) => {
                        self.gen_expr(&computed.expr);
                        self.instructions.push(OpCode::GetPropComputed);
                    }
                    MemberProp::PrivateName(_) => {
                        self.unsupported("optional chaining on a private field (`?.#x`)");
                    }
                }
            }
            OptChainBase::Call(call) => {
                // Temporaries are named after the instruction index, which is
                // unique, and use a character no identifier can contain.
                let tmp = format!("%optcall{}", self.instructions.len());

                // A method callee (`o.m?.()` or `o?.m()`) keeps `o` as `this`.
                let method = match call.callee.as_ref() {
                    Expr::Member(m) => Some((m, false)),
                    Expr::OptChain(c) => match c.base.as_ref() {
                        OptChainBase::Member(m) => Some((m, c.optional)),
                        _ => None,
                    },
                    _ => None,
                };

                match method {
                    Some((member, obj_optional)) if matches!(member.prop, MemberProp::Ident(_)) => {
                        let MemberProp::Ident(name) = &member.prop else {
                            unreachable!()
                        };
                        let name = name.sym.to_string();
                        self.gen_opt_chain_object(&member.obj, to_undefined);
                        if obj_optional {
                            to_undefined.extend(self.emit_jumps_if_nullish());
                        }
                        self.instructions.push(OpCode::Let(tmp.clone()));
                        if chain.optional {
                            self.instructions.push(OpCode::Load(tmp.clone()));
                            self.instructions.push(OpCode::GetProp(name.clone()));
                            to_undefined.extend(self.emit_jumps_if_nullish());
                            self.instructions.push(OpCode::Pop);
                        }
                        for arg in &call.args {
                            self.gen_expr(&arg.expr);
                        }
                        self.instructions.push(OpCode::Load(tmp));
                        self.instructions
                            .push(OpCode::CallMethod(name, call.args.len()));
                    }
                    _ => {
                        self.gen_opt_chain_object(&call.callee, to_undefined);
                        if chain.optional {
                            to_undefined.extend(self.emit_jumps_if_nullish());
                        }
                        self.instructions.push(OpCode::Let(tmp.clone()));
                        for arg in &call.args {
                            self.gen_expr(&arg.expr);
                        }
                        self.instructions.push(OpCode::Load(tmp));
                        self.instructions.push(OpCode::Call(call.args.len()));
                    }
                }
            }
        }
    }

    pub fn generate(&mut self, module: &Module) -> Vec<OpCode> {
        self.top_level_prologue();
        self.namespace_imports();
        let top_stmts: Vec<Stmt> = module
            .body
            .iter()
            .filter_map(|item| match item {
                ModuleItem::Stmt(s) => Some(s.clone()),
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(e)) => {
                    Some(Stmt::Decl(e.decl.clone()))
                }
                _ => None,
            })
            .collect();
        for ident in scope::var_declared(&top_stmts) {
            self.instructions.push(OpCode::Push(JsValue::Undefined));
            self.store_var(&ident, true);
        }
        for item in &module.body {
            match item {
                ModuleItem::Stmt(Stmt::Decl(Decl::Fn(f))) => self.hoist_fn(f),
                ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                    decl: Decl::Fn(f),
                    ..
                })) => self.hoist_fn(f),
                _ => {}
            }
        }
        for item in &module.body {
            match item {
                ModuleItem::Stmt(stmt) => {
                    self.gen_stmt(stmt);
                }
                ModuleItem::ModuleDecl(decl) => {
                    self.gen_module_decl(decl);
                }
            }
        }
        self.instructions.push(OpCode::Halt);
        self.instructions.clone()
    }

    /// `import * as ns from "m"` in a linked module: `ns` is an object of
    /// `m`'s exports, made before the module's code runs (imports are
    /// hoisted).
    fn namespace_imports(&mut self) {
        let Some(link) = &self.link else {
            return;
        };
        let mut namespaces: Vec<(String, Vec<(String, String)>)> = link
            .namespaces
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        namespaces.sort();
        let prefix = link.prefix.clone();
        for (local, members) in namespaces {
            self.instructions.push(OpCode::NewObject);
            // Each member is a getter of the binding, so it is live, as
            // the module's own name for it is.
            for (name, global) in members {
                self.instructions.push(OpCode::Dup);
                let value_idx = self.emit_function_value(Span::default(), 0);
                let jump_idx = self.emit_jump();
                let body_start = self.instructions.len();
                self.patch_function_value(value_idx, body_start);
                self.instructions.push(OpCode::Load(global));
                self.instructions.push(OpCode::Return);
                self.patch_jump_here(jump_idx);
                self.instructions
                    .push(OpCode::SetProp(format!("getter:{name}")));
            }
            self.instructions
                .push(OpCode::Let(format!("{prefix}{local}")));
        }
    }

    /// An `import` / `export` in a linked module (`modules.rs`). Imports and
    /// re-exports were resolved when the program was linked: references
    /// name the bindings themselves, so they compile to nothing. Only a
    /// declaration, and `export default`'s value, make code.
    fn gen_linked_module_decl(&mut self, decl: &ModuleDecl) {
        let prefix = self
            .link
            .as_ref()
            .map(|l| l.prefix.clone())
            .unwrap_or_default();
        let default_export = OpCode::Let(format!("{prefix}default"));
        match decl {
            ModuleDecl::ExportDecl(export_decl) => self.gen_decl(&export_decl.decl),
            ModuleDecl::ExportDefaultDecl(export_default) => match &export_default.decl {
                DefaultDecl::Class(class_expr) => {
                    self.gen_class(
                        &class_expr.class,
                        class_expr.ident.as_ref().map(|id| id.sym.as_str()),
                    );
                    if let Some(ident) = &class_expr.ident {
                        self.instructions.push(OpCode::Dup);
                        self.store_var(ident, true);
                    }
                    self.instructions.push(default_export);
                }
                DefaultDecl::Fn(fn_expr) => {
                    self.gen_fn_decl(fn_expr.ident.as_ref(), &fn_expr.function);
                    if let Some(ident) = &fn_expr.ident {
                        self.load_var(ident);
                    }
                    self.instructions.push(default_export);
                }
                DefaultDecl::TsInterfaceDecl(_) => {}
            },
            ModuleDecl::ExportDefaultExpr(export_default) => {
                self.gen_expr(&export_default.expr);
                self.instructions.push(default_export);
            }
            ModuleDecl::Import(_)
            | ModuleDecl::ExportNamed(_)
            | ModuleDecl::ExportAll(_)
            | ModuleDecl::TsImportEquals(_)
            | ModuleDecl::TsExportAssignment(_)
            | ModuleDecl::TsNamespaceExport(_) => {}
        }
    }

    fn gen_module_decl(&mut self, decl: &ModuleDecl) {
        if self.link.is_some() {
            self.gen_linked_module_decl(decl);
            return;
        }
        match decl {
            ModuleDecl::ExportDecl(export_decl) => {
                self.gen_decl(&export_decl.decl);
            }
            ModuleDecl::ExportDefaultDecl(export_default) => match &export_default.decl {
                DefaultDecl::Class(class_expr) => {
                    self.gen_class(
                        &class_expr.class,
                        class_expr.ident.as_ref().map(|id| id.sym.as_str()),
                    );
                }
                DefaultDecl::Fn(fn_expr) => {
                    self.gen_fn_decl(fn_expr.ident.as_ref(), &fn_expr.function);
                }
                DefaultDecl::TsInterfaceDecl(_) => {
                    // Interfaces are compile-time only
                }
            },
            ModuleDecl::ExportDefaultExpr(export_default) => match &*export_default.expr {
                Expr::Fn(fn_expr) => {
                    self.gen_fn_decl(fn_expr.ident.as_ref(), &fn_expr.function);
                }
                Expr::Class(class_expr) => {
                    self.gen_class(
                        &class_expr.class,
                        class_expr.ident.as_ref().map(|id| id.sym.as_str()),
                    );
                }
                _ => self.unsupported(
                    "`export default` of an expression other than a function or class",
                ),
            },
            ModuleDecl::Import(import) => {
                let src = import.src.value.to_string_lossy().into_owned();

                for spec in &import.specifiers {
                    match spec {
                        ImportSpecifier::Named(named) => {
                            let local = named.local.sym.to_string();
                            let imported = named
                                .imported
                                .as_ref()
                                .map(|i| {
                                    let atom = i.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                })
                                .unwrap_or_else(|| local.clone());

                            self.instructions
                                .push(OpCode::Push(JsValue::String(src.clone())));
                            self.instructions.push(OpCode::ImportAsync(src.clone()));
                            self.instructions.push(OpCode::GetExport {
                                name: imported.clone(),
                                is_default: false,
                            });
                            self.instructions.push(OpCode::Let(local));
                        }
                        ImportSpecifier::Default(default) => {
                            let local = default.local.sym.to_string();

                            self.instructions
                                .push(OpCode::Push(JsValue::String(src.clone())));
                            self.instructions.push(OpCode::ImportAsync(src.clone()));
                            self.instructions.push(OpCode::GetExport {
                                name: "default".to_string(),
                                is_default: true,
                            });
                            self.instructions.push(OpCode::Let(local));
                        }
                        ImportSpecifier::Namespace(ns) => {
                            let local = ns.local.sym.to_string();

                            self.instructions
                                .push(OpCode::Push(JsValue::String(src.clone())));
                            self.instructions.push(OpCode::ImportAsync(src.clone()));
                            self.instructions.push(OpCode::Let(local));
                        }
                    }
                }

                if import.specifiers.is_empty() {
                    self.instructions
                        .push(OpCode::Push(JsValue::String(src.clone())));
                    self.instructions.push(OpCode::ImportAsync(src.clone()));
                    self.instructions.push(OpCode::Pop);
                }

                if let Some(_with) = &import.with {
                    self.warnings.push(format!(
                        "Warning: Import assertions for '{}' are not fully supported",
                        import.src.value.to_string_lossy()
                    ));
                }
            }
            ModuleDecl::ExportNamed(named) => {
                if let Some(src) = &named.src {
                    let src_str = src.value.to_string_lossy().into_owned();
                    for spec in &named.specifiers {
                        match spec {
                            ExportSpecifier::Named(named) => {
                                let export_name = named
                                    .exported
                                    .as_ref()
                                    .map(|e| {
                                        let atom = e.atom();
                                        let s: &str = &atom;
                                        s.to_string()
                                    })
                                    .unwrap_or_else(|| {
                                        let atom = named.orig.atom();
                                        let s: &str = &atom;
                                        s.to_string()
                                    });
                                let _local_name = {
                                    let atom = named.orig.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                };

                                self.instructions
                                    .push(OpCode::Push(JsValue::String(src_str.clone())));
                                self.instructions.push(OpCode::ImportAsync(src_str.clone()));
                                self.instructions.push(OpCode::GetExport {
                                    name: export_name.clone(),
                                    is_default: false,
                                });
                                self.instructions.push(OpCode::Let(export_name.clone()));
                                self.instructions.push(OpCode::Load(export_name.clone()));
                                self.instructions.push(OpCode::Store(export_name));
                            }
                            ExportSpecifier::Default(_) => {
                                self.instructions
                                    .push(OpCode::Push(JsValue::String(src_str.clone())));
                                self.instructions.push(OpCode::ImportAsync(src_str.clone()));
                                self.instructions.push(OpCode::GetExport {
                                    name: "default".to_string(),
                                    is_default: true,
                                });
                                self.instructions.push(OpCode::Let("default".to_string()));
                                self.instructions.push(OpCode::Load("default".to_string()));
                                self.instructions.push(OpCode::Store("default".to_string()));
                            }
                            ExportSpecifier::Namespace(ns) => {
                                let name = {
                                    let atom = ns.name.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                };
                                self.instructions
                                    .push(OpCode::Push(JsValue::String(src_str.clone())));
                                self.instructions.push(OpCode::ImportAsync(src_str.clone()));
                                self.instructions.push(OpCode::Let(name.clone()));
                            }
                        }
                    }
                } else {
                    for spec in &named.specifiers {
                        match spec {
                            ExportSpecifier::Named(named) => {
                                let export_name = named
                                    .exported
                                    .as_ref()
                                    .map(|e| {
                                        let atom = e.atom();
                                        let s: &str = &atom;
                                        s.to_string()
                                    })
                                    .unwrap_or_else(|| {
                                        let atom = named.orig.atom();
                                        let s: &str = &atom;
                                        s.to_string()
                                    });
                                let local_name = {
                                    let atom = named.orig.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                };

                                self.instructions.push(OpCode::Load(local_name));
                                self.instructions.push(OpCode::Dup);
                                self.instructions.push(OpCode::Store(export_name));
                            }
                            ExportSpecifier::Default(_) => {
                                self.instructions.push(OpCode::Load("default".to_string()));
                                self.instructions.push(OpCode::Dup);
                                self.instructions.push(OpCode::Store("default".to_string()));
                            }
                            ExportSpecifier::Namespace(ns) => {
                                let name = {
                                    let atom = ns.name.atom();
                                    let s: &str = &atom;
                                    s.to_string()
                                };
                                self.instructions.push(OpCode::Load(name.clone()));
                                self.instructions.push(OpCode::Dup);
                                self.instructions.push(OpCode::Store(name));
                            }
                        }
                    }
                }
            }
            ModuleDecl::ExportAll(all) => {
                let src_str = all.src.value.to_string_lossy().into_owned();
                self.warnings.push(format!(
                    "Warning: 'export * from' is not yet fully implemented for '{}'",
                    src_str
                ));
                self.instructions
                    .push(OpCode::Push(JsValue::String(src_str.clone())));
                self.instructions.push(OpCode::ImportAsync(src_str.clone()));
                self.instructions.push(OpCode::Pop);
            }
            ModuleDecl::TsImportEquals(_) => {}
            ModuleDecl::TsExportAssignment(_) => {}
            ModuleDecl::TsNamespaceExport(_) => {}
        }
    }

    fn gen_decl(&mut self, decl: &Decl) {
        // `declare ...` describes something defined elsewhere: no code.
        if scope::is_ambient(decl) {
            return;
        }
        match decl {
            Decl::Fn(fn_decl) => {
                if !self.is_hoisted(fn_decl) {
                    self.gen_fn_decl(Some(&fn_decl.ident), &fn_decl.function);
                }
            }
            Decl::Class(class_decl) => {
                self.gen_class(&class_decl.class, Some(class_decl.ident.sym.as_str()));
                self.store_var(&class_decl.ident, true);
            }
            Decl::Var(var_decl) => {
                self.gen_var_decl(var_decl);
            }
            Decl::TsEnum(enum_decl) => {
                // Generate enum as a runtime object

                // Create new object
                self.instructions.push(OpCode::NewObject);

                // Add each enum member as a property
                for member in &enum_decl.members {
                    // Duplicate the object reference for each property
                    self.instructions.push(OpCode::Dup);

                    // Get the member name
                    let member_name = match &member.id {
                        swc_ecma_ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
                        swc_ecma_ast::TsEnumMemberId::Str(s) => s
                            .value
                            .as_str()
                            .expect("Invalid string enum member")
                            .to_string(),
                    };

                    // Push the value
                    match &member.init {
                        Some(expr) => {
                            // Compute the value
                            self.gen_expr(expr);
                        }
                        None => {
                            // Auto-incrementing number for unvalued variants
                            let idx = enum_decl
                                .members
                                .iter()
                                .position(|m| m.id == member.id)
                                .unwrap_or(0);
                            self.instructions
                                .push(OpCode::Push(JsValue::Number(idx as f64)));
                        }
                    }

                    // Set the property
                    self.instructions.push(OpCode::SetProp(member_name));
                }

                // Bind the enum object to its name.
                self.store_var(&enum_decl.id, true);
            }
            Decl::TsModule(_) => {
                // TypeScript modules are compile-time only, skip
            }
            Decl::TsInterface(_) => {
                // Interfaces are compile-time only, skip
            }
            Decl::TsTypeAlias(_) => {
                // Type aliases are compile-time only, skip
            }
            Decl::Using(_) => self.unsupported("`using` declarations"),
        }
    }

    pub fn generate_script(&mut self, script: &Script) -> Vec<OpCode> {
        self.top_level_prologue();
        self.hoist_body(&script.body, &[]);
        for stmt in &script.body {
            self.gen_stmt(stmt);
        }
        self.instructions.push(OpCode::Halt);
        self.instructions.clone()
    }

    /// A function declaration (`name` given) or an anonymous function value
    /// (left on the stack).
    fn gen_fn_decl(&mut self, name: Option<&Ident>, fn_decl: &Function) {
        if let Some(ident) = name {
            self.next_fn_name = Some(ident.sym.to_string());
        }
        let is_async = fn_decl.is_async;
        if fn_decl.is_generator {
            self.unsupported("generator functions (`function*`)");
        }

        // [function value] ; bind the name ; jump over the body.
        let value_idx = self.emit_function_value(fn_decl.span, 0);
        if let Some(ident) = name {
            self.store_var(ident, true);
        }
        let jump_idx = self.emit_jump();
        let body_start = self.instructions.len();
        self.patch_function_value(value_idx, body_start);

        let pats: Vec<&Pat> = fn_decl.params.iter().map(|p| &p.pat).collect();
        let stmts: &[Stmt] = fn_decl.body.as_ref().map_or(&[], |b| b.stmts.as_slice());
        self.gen_function_body(fn_decl.span, &pats, stmts, is_async, FnKind::Plain, &[]);
        self.patch_jump_here(jump_idx);
    }

    /// The body of a function whose value was just emitted: parameters,
    /// the environment, hoisting, the statements and the implicit result.
    /// A constructor also initializes the instance `fields` (a derived one
    /// once `super(...)` returns) and results in `this`.
    fn gen_function_body(
        &mut self,
        span: Span,
        pats: &[&Pat],
        stmts: &[Stmt],
        is_async: bool,
        kind: FnKind,
        fields: &[ClassField],
    ) {
        let fn_start = self.instructions.len();
        let fn_name = self.next_fn_name.take();
        let prev_in_function = self.in_function;
        let prev_async = self.in_async_function;
        let prev_fields = self.class_fields.take();
        let prev_static = self.in_static_method;
        self.in_function = true;
        self.in_async_function = is_async;
        self.in_static_method = matches!(kind, FnKind::Method { is_static: true });
        let prev_control = self.enter_function_control();

        let implicit_super = matches!(
            kind,
            FnKind::Ctor {
                derived: true,
                implicit: true
            }
        );
        if implicit_super {
            // Must be first: the VM passes every argument to a function
            // that starts with it (`constructor(...args) { super(...args) }`).
            self.instructions.push(OpCode::ForwardArgs);
        }
        let params = self.bind_params(pats);
        let prev_env = self.function_prologue(span, &params);
        self.apply_param_defaults(pats);

        match kind {
            FnKind::Ctor { derived: false, .. } => self.emit_field_inits(fields),
            FnKind::Ctor { implicit: true, .. } => {
                self.load_var(&scope::super_ident(span));
                self.instructions.push(OpCode::SuperForward);
                self.instructions.push(OpCode::Pop);
                self.emit_field_inits(fields);
            }
            FnKind::Ctor { .. } => {
                self.class_fields = Some(std::rc::Rc::new(fields.to_vec()));
            }
            _ => {}
        }

        // An async function: its promise, and an implicit `try` around the
        // body whose `catch` rejects the promise.
        let async_try = if is_async {
            self.instructions.push(OpCode::AsyncStart);
            self.instructions
                .push(OpCode::Let(ASYNC_PROMISE.to_string()));
            let setup = self.instructions.len();
            self.instructions.push(OpCode::SetupTry {
                catch_addr: 0,
                finally_addr: 0,
            });
            self.try_stack.push(TryRegion {
                part: TryPart::Try,
                env: None,
                finalizer: None,
            });
            Some(setup)
        } else {
            None
        };

        let param_names: Vec<String> = pats.iter().flat_map(|p| scope::pattern_names(p)).collect();
        self.hoist_body(stmts, &param_names);
        let mut last_instr_was_return = false;
        for s in stmts {
            let before = self.instructions.len();
            self.gen_stmt(s);
            last_instr_was_return = self.instructions.len() > before
                && matches!(self.instructions.last(), Some(OpCode::Return));
        }

        if !last_instr_was_return {
            // Expression statements pop their value, so nothing is left on
            // the stack: the implicit result is `undefined` (`this` for a
            // constructor).
            if matches!(kind, FnKind::Ctor { .. }) {
                self.instructions.push(OpCode::LoadThis);
            } else {
                self.instructions.push(OpCode::Push(JsValue::Undefined));
            }
            if is_async {
                self.unwind_tries(0);
                self.emit_async_return();
            }
            self.instructions.push(OpCode::Return);
        }

        if let Some(setup) = async_try {
            self.try_stack.pop();
            // catch: [reason] -> the promise rejects with it.
            let catch_addr = self.instructions.len();
            self.instructions
                .push(OpCode::Load(ASYNC_PROMISE.to_string()));
            self.instructions.push(OpCode::AsyncReject);
            self.instructions.push(OpCode::Return);
            self.instructions[setup] = OpCode::SetupTry {
                catch_addr,
                finally_addr: 0,
            };
        }

        self.env_active = prev_env;
        self.in_function = prev_in_function;
        self.in_async_function = prev_async;
        self.class_fields = prev_fields;
        self.in_static_method = prev_static;
        self.leave_function_control(prev_control);
        self.functions.push(FunctionRange {
            start: fn_start,
            end: self.instructions.len(),
            name: fn_name,
        });
    }

    /// `this.name = initializer` for each instance field.
    fn emit_field_inits(&mut self, fields: &[ClassField]) {
        for field in fields {
            self.instructions.push(OpCode::LoadThis);
            match &field.value {
                Some(v) => self.gen_expr(v),
                None => self.instructions.push(OpCode::Push(JsValue::Undefined)),
            }
            // Initializers from field decorators, in source order (as
            // TypeScript runs them): `value = init.call(this, value)`.
            for span in &field.decorators {
                self.load_var(&scope::field_init_use(*span));
                self.instructions.push(OpCode::LoadThis);
                self.emit_call_initializer();
            }
            self.instructions.push(OpCode::SetProp(field.key.clone()));
        }
    }

    /// `[value, init, this] -> [init.call(this, value)]`, or `[value]` if
    /// `init` is undefined (the decorator returned nothing).
    fn emit_call_initializer(&mut self) {
        // [v, f, this] -> [v, this, f] to test f
        self.instructions.push(OpCode::Swap);
        self.instructions.push(OpCode::Dup);
        self.instructions.push(OpCode::Push(JsValue::Undefined));
        self.instructions.push(OpCode::Eq);
        let call = self.emit_jump_if_false();
        self.instructions.push(OpCode::Pop);
        self.instructions.push(OpCode::Pop);
        let end = self.emit_jump();
        self.patch_jump_here(call);
        // [v, this, f] -> [v, f, this]
        self.instructions.push(OpCode::Swap);
        self.instructions.push(OpCode::CallWithThis(1));
        self.patch_jump_here(end);
    }

    /// `{ kind, name, static, private }`, the context a decorator receives.
    fn emit_decorator_context(
        &mut self,
        kind: &str,
        name: Option<&str>,
        is_static: bool,
        is_private: bool,
    ) {
        self.instructions.push(OpCode::NewObject);
        let set = |cg: &mut Self, key: &str, v: JsValue| {
            cg.instructions.push(OpCode::Dup);
            cg.instructions.push(OpCode::Push(v));
            cg.instructions.push(OpCode::SetProp(key.to_string()));
        };
        set(self, "kind", JsValue::String(kind.to_string()));
        let name = match name {
            Some(n) => JsValue::String(n.to_string()),
            None => JsValue::Undefined,
        };
        set(self, "name", name);
        if kind != "class" {
            set(self, "static", JsValue::Boolean(is_static));
            set(self, "private", JsValue::Boolean(is_private));
        }
    }

    /// Decorators on the value at the top of the stack: their expressions
    /// are evaluated in source order, then applied last to first, each
    /// `value = decorator(value, context) ?? value`. [value] -> [value]
    #[allow(clippy::too_many_arguments)]
    fn emit_decorators(
        &mut self,
        decorators: &[Decorator],
        kind: &str,
        name: Option<&str>,
        is_static: bool,
        is_private: bool,
    ) {
        let temps: Vec<String> = decorators
            .iter()
            .map(|d| {
                self.gen_expr(&d.expr);
                let temp = format!("%decorator{}", d.span.lo.0);
                self.instructions.push(OpCode::Let(temp.clone()));
                temp
            })
            .collect();
        for temp in temps.iter().rev() {
            // [v] -> [v, v, context, decorator] -> Call -> [v, result]
            self.instructions.push(OpCode::Dup);
            self.emit_decorator_context(kind, name, is_static, is_private);
            self.instructions.push(OpCode::Load(temp.clone()));
            self.instructions.push(OpCode::Call(2));
            // [v, result] -> [result], or [v] if it is undefined
            self.instructions.push(OpCode::Dup);
            self.instructions.push(OpCode::Push(JsValue::Undefined));
            self.instructions.push(OpCode::Eq);
            let keep_result = self.emit_jump_if_false();
            self.instructions.push(OpCode::Pop);
            let end = self.emit_jump();
            self.patch_jump_here(keep_result);
            self.instructions.push(OpCode::Swap);
            self.instructions.push(OpCode::Pop);
            self.patch_jump_here(end);
        }
    }

    /// The property key of private name `#name`: `#name@<class>`, so that
    /// classes in a chain that both declare `#x` keep separate fields.
    fn private_key(&self, name: &str) -> String {
        self.private_names
            .iter()
            .rev()
            .find_map(|names| names.get(name).cloned())
            .unwrap_or_else(|| format!("#{}", name))
    }

    /// The property name of `obj.name` or `obj.#name`.
    fn member_key(&self, prop: &MemberProp) -> Option<String> {
        match prop {
            MemberProp::Ident(id) => Some(id.sym.to_string()),
            MemberProp::PrivateName(p) => Some(self.private_key(&p.name)),
            MemberProp::Computed(_) => None,
        }
    }

    /// `[obj] -> [obj.name]`; a private member the object lacks throws.
    fn emit_member_get(&mut self, prop: &MemberProp) {
        match prop {
            MemberProp::PrivateName(p) => {
                let key = self.private_key(&p.name);
                self.instructions.push(OpCode::GetPrivate(key));
            }
            _ => {
                let key = self.member_key(prop).unwrap_or_default();
                self.instructions.push(OpCode::GetProp(key));
            }
        }
    }

    /// `[obj, value] -> []` for `obj.name = value`.
    fn emit_member_set(&mut self, prop: &MemberProp) {
        match prop {
            MemberProp::PrivateName(p) => {
                let key = self.private_key(&p.name);
                self.instructions.push(OpCode::SetPrivate(key));
            }
            _ => {
                let key = self.member_key(prop).unwrap_or_default();
                self.instructions.push(OpCode::SetProp(key));
            }
        }
    }

    /// `[] -> [parent]` for `super` at `span`: the parent class itself in a
    /// static method, its prototype otherwise.
    fn emit_super_base(&mut self, span: Span) {
        self.load_var(&scope::super_ident(span));
        if !self.in_static_method {
            self.instructions
                .push(OpCode::GetProp("prototype".to_string()));
        }
    }

    /// `super.name` / `super[key]`: the property of the parent class (in a
    /// static method) or of its prototype.
    fn gen_super_prop(&mut self, sp: &swc_ecma_ast::SuperPropExpr) {
        match &sp.prop {
            swc_ecma_ast::SuperProp::Ident(id) => {
                // A getter found on the parent runs with this `this`.
                self.instructions.push(OpCode::LoadThis);
                self.emit_super_base(sp.obj.span);
                self.instructions
                    .push(OpCode::GetSuperProp(id.sym.to_string()));
            }
            swc_ecma_ast::SuperProp::Computed(c) => {
                self.emit_super_base(sp.obj.span);
                self.gen_expr(&c.expr);
                self.instructions.push(OpCode::GetPropComputed);
            }
        }
    }

    fn gen_var_decl(&mut self, var_decl: &VarDecl) {
        for decl in &var_decl.decls {
            if let Some(init) = &decl.init {
                // `let f = () => ...`: the function is named `f`.
                if let (Pat::Ident(id), Expr::Arrow(_) | Expr::Fn(FnExpr { ident: None, .. })) =
                    (&decl.name, init.as_ref())
                {
                    self.next_fn_name = Some(id.id.sym.to_string());
                }
                self.gen_expr(init);
                self.gen_pattern_binding(&decl.name);
            } else if var_decl.kind != VarDeclKind::Var {
                // `let x;` is `undefined`. (A `var` is bound to `undefined`
                // once, at the top of its function: `hoist_body`.)
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.gen_pattern_binding(&decl.name);
            }
        }
    }

    /// Generate code to bind a pattern to a value on the stack.
    /// The value to destructure should already be on top of the stack.
    fn gen_pattern_binding(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(id) => {
                // Simple variable binding
                self.store_var(&id.id, true);
            }
            Pat::Object(obj_pat) => {
                // Object destructuring: let { x, y } = obj
                for (i, prop) in obj_pat.props.iter().enumerate() {
                    match prop {
                        swc_ecma_ast::ObjectPatProp::KeyValue(kv) => {
                            // { key: pattern } - extract key and bind to pattern
                            // Dup the object for each property (except the last)
                            if i < obj_pat.props.len() - 1 {
                                self.instructions.push(OpCode::Dup);
                            }
                            // Get property name
                            let key_name = match &kv.key {
                                swc_ecma_ast::PropName::Ident(id) => id.sym.to_string(),
                                swc_ecma_ast::PropName::Str(s) => {
                                    s.value.to_string_lossy().into_owned()
                                }
                                _ => {
                                    self.unsupported("computed keys in destructuring patterns");
                                    continue;
                                }
                            };
                            self.instructions.push(OpCode::GetProp(key_name));
                            // Recursively bind the value pattern
                            self.gen_pattern_binding(&kv.value);
                        }
                        swc_ecma_ast::ObjectPatProp::Assign(assign) => {
                            // { x } or { x = default } - shorthand property
                            // Dup the object for each property (except the last)
                            if i < obj_pat.props.len() - 1 {
                                self.instructions.push(OpCode::Dup);
                            }
                            let key_name = assign.key.sym.to_string();
                            self.instructions.push(OpCode::GetProp(key_name.clone()));

                            // Handle default value if present
                            if let Some(default_val) = &assign.value {
                                // Check if value is undefined, use default if so
                                self.instructions.push(OpCode::Dup);
                                self.instructions.push(OpCode::Push(JsValue::Undefined));
                                self.instructions.push(OpCode::Eq);
                                let jump_idx = self.instructions.len();
                                self.instructions.push(OpCode::JumpIfFalse(0));
                                // Pop undefined, push default
                                self.instructions.push(OpCode::Pop);
                                self.gen_expr(default_val);
                                let end_addr = self.instructions.len();
                                if let OpCode::JumpIfFalse(ref mut addr) =
                                    self.instructions[jump_idx]
                                {
                                    *addr = end_addr;
                                }
                            }

                            self.store_var(&assign.key.id, true);
                        }
                        swc_ecma_ast::ObjectPatProp::Rest(rest) => {
                            // { ...rest } - not fully implemented yet
                            if let Pat::Ident(id) = rest.arg.as_ref() {
                                // For now, just bind the remaining object
                                self.store_var(&id.id, true);
                            }
                        }
                    }
                }
            }
            Pat::Array(arr_pat) => {
                // Array destructuring: let [a, b] = arr
                for (i, elem) in arr_pat.elems.iter().enumerate() {
                    if let Some(elem_pat) = elem {
                        // Dup the array for each element (except the last)
                        let is_last = i == arr_pat.elems.len() - 1
                            || arr_pat.elems.iter().skip(i + 1).all(|e| e.is_none());
                        if !is_last {
                            self.instructions.push(OpCode::Dup);
                        }
                        // Push index and load element
                        self.instructions
                            .push(OpCode::Push(JsValue::Number(i as f64)));
                        self.instructions.push(OpCode::GetPropComputed);
                        // Recursively bind the element pattern
                        self.gen_pattern_binding(elem_pat);
                    }
                }
            }
            Pat::Rest(rest) => {
                // ...rest pattern - bind remaining value
                self.gen_pattern_binding(&rest.arg);
            }
            Pat::Assign(assign) => {
                // pattern = default - handle default value
                self.instructions.push(OpCode::Dup);
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.instructions.push(OpCode::Eq);
                let jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                // Pop undefined, push default
                self.instructions.push(OpCode::Pop);
                self.gen_expr(&assign.right);
                let end_addr = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[jump_idx] {
                    *addr = end_addr;
                }
                self.gen_pattern_binding(&assign.left);
            }
            Pat::Expr(_) => {
                self.unsupported("expression as a binding pattern");
                self.instructions.push(OpCode::Pop);
            }
            Pat::Invalid(_) => {
                self.unsupported("invalid binding pattern");
                self.instructions.push(OpCode::Pop);
            }
        }
    }

    fn gen_stmt(&mut self, stmt: &Stmt) {
        // `declare ...` describes something defined elsewhere: no code.
        if let Stmt::Decl(d) = stmt
            && scope::is_ambient(d)
        {
            return;
        }
        let saved = std::mem::replace(&mut self.current_span, stmt.span());
        let saved_mark = std::mem::replace(&mut self.mark_span, stmt.span());
        self.mark(stmt.span());
        self.gen_stmt_inner(stmt);
        self.current_span = saved;
        self.mark_span = saved_mark;
        self.mark(saved_mark);
    }

    /// Code from here on comes from `span` (until the next mark); a dummy
    /// span is no position (byte 0 precedes every file).
    fn mark(&mut self, span: Span) {
        let at = self.instructions.len();
        match self.marks.last_mut() {
            Some(last) if last.0 == at => last.1 = span.lo.0,
            _ => self.marks.push((at, span.lo.0)),
        }
    }

    fn gen_stmt_inner(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return(ret_stmt) => {
                if let Some(arg) = &ret_stmt.arg {
                    self.gen_expr(arg); // Pushes the return value to stack
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Undefined));
                }
                // Leaving every `try` in this function: pop the handlers and
                // run the `finally` blocks, with the return value on the stack.
                self.unwind_tries(0);
                if self.in_async_function {
                    self.emit_async_return();
                }
                self.instructions.push(OpCode::Return);
            }
            // RECURSION: Handle the Block
            Stmt::Block(block) => {
                let env = self.enter_scope_env(block.span);
                self.scope_stack.push(Vec::new()); // Enter new scope
                self.hoist_functions(&block.stmts);
                for s in &block.stmts {
                    self.gen_stmt(s);
                }
                // Exit scope: Drop variables
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
                self.exit_scope_env(env);
            }
            Stmt::Decl(Decl::Var(var_decl)) => {
                self.gen_var_decl(var_decl);
            }
            Stmt::Decl(Decl::Fn(fn_decl)) => {
                if !self.is_hoisted(fn_decl) {
                    self.gen_fn_decl(Some(&fn_decl.ident), &fn_decl.function);
                }
            }
            Stmt::Decl(Decl::Class(class_decl)) => {
                let class_name = class_decl.ident.sym.to_string();
                self.gen_class(&class_decl.class, Some(class_name.as_str()));
                self.store_var(&class_decl.ident, true);
            }
            Stmt::Decl(Decl::TsEnum(enum_decl)) => {
                // Generate enum as a runtime object at statement level
                let enum_name = enum_decl.id.sym.to_string();

                // Create new object
                self.instructions.push(OpCode::NewObject);

                // Add each enum member as a property
                for member in &enum_decl.members {
                    // Duplicate the object reference for each property
                    self.instructions.push(OpCode::Dup);

                    // Get the member name
                    let member_name = match &member.id {
                        swc_ecma_ast::TsEnumMemberId::Ident(ident) => ident.sym.to_string(),
                        swc_ecma_ast::TsEnumMemberId::Str(s) => s
                            .value
                            .as_str()
                            .expect("Invalid string enum member")
                            .to_string(),
                    };

                    // Push the value
                    match &member.init {
                        Some(expr) => {
                            // Compute the value
                            self.gen_expr(expr);
                        }
                        None => {
                            // Auto-incrementing number for unvalued variants
                            let idx = enum_decl
                                .members
                                .iter()
                                .position(|m| m.id == member.id)
                                .unwrap_or(0);
                            self.instructions
                                .push(OpCode::Push(JsValue::Number(idx as f64)));
                        }
                    }

                    // Set the property
                    self.instructions.push(OpCode::SetProp(member_name));
                }

                // Store the enum object in a variable
                self.instructions.push(OpCode::Let(enum_name.clone()));
            }
            Stmt::Decl(Decl::TsModule(_)) => {
                // TypeScript modules are compile-time only, skip at runtime
            }
            Stmt::Decl(Decl::TsInterface(_)) => {
                // Interfaces are compile-time only, skip at runtime
            }
            Stmt::Decl(Decl::TsTypeAlias(_)) => {
                // Type aliases are compile-time only, skip at runtime
            }
            Stmt::Expr(expr_stmt) => {
                self.gen_expr(&expr_stmt.expr);
                // Expression statements (e.g. `foo();`) should always discard their result in JS.
                // This is critical for proper stack management - without this Pop, values from
                // expression statements (like assignments) accumulate on the stack and corrupt
                // the stack state when calling functions from within object literals or other
                // expressions that expect the stack to be clean.
                self.instructions.push(OpCode::Pop);
            }
            Stmt::While(while_stmt) => {
                let loop_env = self.save_env();
                let iter_env = loop_env.clone();
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                self.gen_expr(&while_stmt.test);
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.gen_stmt(&while_stmt.body);
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                self.pop_target(loop_end, loop_start);
            }
            Stmt::Break(brk) => self.gen_jump_stmt(brk.label.as_ref(), false),
            Stmt::Continue(cont) => self.gen_jump_stmt(cont.label.as_ref(), true),
            Stmt::If(if_stmt) => {
                self.gen_expr(&if_stmt.test);
                let else_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.gen_stmt(&if_stmt.cons);
                let has_else = if_stmt.alt.is_some();
                let end_jump_idx = if has_else {
                    Some(self.instructions.len())
                } else {
                    None
                };
                if has_else {
                    self.instructions.push(OpCode::Jump(0));
                }
                let else_start = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[else_jump_idx] {
                    *addr = else_start;
                }
                if let Some(alt) = &if_stmt.alt {
                    self.gen_stmt(alt);
                }
                if let Some(end_idx) = end_jump_idx {
                    let end_addr = self.instructions.len();
                    if let OpCode::Jump(ref mut addr) = self.instructions[end_idx] {
                        *addr = end_addr;
                    }
                }
            }
            Stmt::For(for_stmt) => {
                self.scope_stack.push(Vec::new());
                // `let` bindings of the loop head that closures capture get a
                // fresh environment per iteration, copied from the previous
                // one, as JavaScript specifies.
                let loop_env = self.save_env();
                let head: Option<Vec<String>> =
                    self.scopes.env_scope(for_stmt.span).map(|n| n.to_vec());
                if head.is_some() {
                    self.emit_child_env();
                    self.instructions
                        .push(OpCode::Store(SCOPE_LOCAL.to_string()));
                }
                if let Some(init) = &for_stmt.init {
                    match init {
                        swc_ecma_ast::VarDeclOrExpr::VarDecl(var_decl) => {
                            self.gen_var_decl(var_decl);
                        }
                        swc_ecma_ast::VarDeclOrExpr::Expr(expr) => {
                            self.gen_expr(expr);
                            self.instructions.push(OpCode::Pop);
                        }
                    }
                }
                let iter_env = match &head {
                    Some(names) => {
                        self.emit_iteration_env(names);
                        let save = format!("%iter{}", self.instructions.len());
                        self.instructions
                            .push(OpCode::Load(SCOPE_LOCAL.to_string()));
                        self.instructions.push(OpCode::Let(save.clone()));
                        Some(save)
                    }
                    None => loop_env.clone(),
                };
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                if let Some(test) = &for_stmt.test {
                    self.gen_expr(test);
                } else {
                    self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
                }
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.gen_stmt(&for_stmt.body);
                let continue_target = self.instructions.len();
                if let (Some(names), Some(save)) = (&head, &iter_env) {
                    self.emit_iteration_env(names);
                    self.instructions
                        .push(OpCode::Load(SCOPE_LOCAL.to_string()));
                    self.instructions.push(OpCode::Store(save.clone()));
                }
                if let Some(update) = &for_stmt.update {
                    self.gen_expr(update);
                    self.instructions.push(OpCode::Pop);
                }
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                self.pop_target(loop_end, continue_target);
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::ForOf(for_of_stmt) => {
                self.scope_stack.push(Vec::new());
                let loop_env = self.save_env();
                let iter_env = loop_env.clone();
                let per_iteration = self.scopes.env_scope(for_of_stmt.span).is_some();
                self.gen_expr(&for_of_stmt.right);
                // Unique names, so nested loops do not share them.
                let iter_name = format!("%forof_iter{}", self.instructions.len());
                self.instructions.push(OpCode::Let(iter_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(iter_name.clone());
                }
                self.instructions.push(OpCode::Push(JsValue::Number(0.0)));
                let idx_name = format!("%forof_idx{}", self.instructions.len());
                self.instructions.push(OpCode::Let(idx_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(idx_name.clone());
                }
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::Load(iter_name.clone()));
                self.instructions
                    .push(OpCode::GetProp("length".to_string()));
                self.instructions.push(OpCode::Lt);
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.instructions.push(OpCode::Load(iter_name.clone()));
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::LoadElement);
                if per_iteration {
                    // A fresh environment for this iteration's binding.
                    self.restore_env(&loop_env);
                    self.emit_child_env();
                    self.instructions
                        .push(OpCode::Store(SCOPE_LOCAL.to_string()));
                }
                if let Some(var_decl) = &for_of_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    self.store_var(&id.id, true);
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.push(id.id.sym.to_string());
                    }
                } else if let ForHead::Pat(pat) = &for_of_stmt.left
                    && let Pat::Ident(id) = pat.as_ref()
                {
                    self.store_var(&id.id, false);
                } else {
                    self.unsupported("this `for...of` binding");
                }
                self.gen_stmt(&for_of_stmt.body);
                let continue_target = self.instructions.len();
                if let Some(var_decl) = &for_of_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    let var_name = id.id.sym.to_string();
                    self.instructions.push(OpCode::Drop(var_name));
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.retain(|n| n != &id.id.sym.to_string());
                    }
                }
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                self.instructions.push(OpCode::Add);
                self.instructions.push(OpCode::Store(idx_name.clone()));
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                self.pop_target(loop_end, continue_target);
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::ForIn(for_in_stmt) => {
                self.scope_stack.push(Vec::new());
                let loop_env = self.save_env();
                let iter_env = loop_env.clone();
                let per_iteration = self.scopes.env_scope(for_in_stmt.span).is_some();
                self.gen_expr(&for_in_stmt.right);
                self.instructions.push(OpCode::Load("Object".to_string()));
                self.instructions.push(OpCode::GetProp("keys".to_string()));
                self.instructions.push(OpCode::Call(1));
                let keys_name = format!("%forin_keys{}", self.instructions.len());
                self.instructions.push(OpCode::Let(keys_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(keys_name.clone());
                }
                self.instructions.push(OpCode::Push(JsValue::Number(0.0)));
                let idx_name = format!("%forin_idx{}", self.instructions.len());
                self.instructions.push(OpCode::Let(idx_name.clone()));
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.push(idx_name.clone());
                }
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::Load(keys_name.clone()));
                self.instructions
                    .push(OpCode::GetProp("length".to_string()));
                self.instructions.push(OpCode::Lt);
                let exit_jump_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0));
                self.instructions.push(OpCode::Load(keys_name.clone()));
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::LoadElement);
                if per_iteration {
                    self.restore_env(&loop_env);
                    self.emit_child_env();
                    self.instructions
                        .push(OpCode::Store(SCOPE_LOCAL.to_string()));
                }
                if let Some(var_decl) = &for_in_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    self.store_var(&id.id, true);
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.push(id.id.sym.to_string());
                    }
                } else if let ForHead::Pat(pat) = &for_in_stmt.left
                    && let Pat::Ident(id) = pat.as_ref()
                {
                    self.store_var(&id.id, false);
                } else {
                    self.unsupported("this `for...in` binding");
                }
                self.gen_stmt(&for_in_stmt.body);
                let continue_target = self.instructions.len();
                if let Some(var_decl) = &for_in_stmt.left.as_var_decl()
                    && let Some(decl) = var_decl.decls.first()
                    && let Pat::Ident(id) = &decl.name
                {
                    let var_name = id.id.sym.to_string();
                    self.instructions.push(OpCode::Drop(var_name));
                    if let Some(scope) = self.scope_stack.last_mut() {
                        scope.retain(|n| n != &id.id.sym.to_string());
                    }
                }
                self.instructions.push(OpCode::Load(idx_name.clone()));
                self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                self.instructions.push(OpCode::Add);
                self.instructions.push(OpCode::Store(idx_name.clone()));
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[exit_jump_idx] {
                    *addr = loop_end;
                }
                self.pop_target(loop_end, continue_target);
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }
            }
            Stmt::DoWhile(do_while_stmt) => {
                let loop_env = self.save_env();
                let iter_env = loop_env.clone();
                let loop_start = self.instructions.len();
                self.push_loop(loop_env.clone(), iter_env.clone());
                self.gen_stmt(&do_while_stmt.body);
                let continue_target = self.instructions.len();
                self.gen_expr(&do_while_stmt.test);
                self.instructions
                    .push(OpCode::JumpIfFalse(self.instructions.len() + 2));
                self.instructions.push(OpCode::Jump(loop_start));
                let loop_end = self.instructions.len();
                self.restore_env(&loop_env);
                self.pop_target(loop_end, continue_target);
            }
            Stmt::With(_) => self.unsupported("`with` statements"),
            Stmt::Empty(_) | Stmt::Debugger(_) => {
                // Empty/debugger/with statements - do nothing
            }
            Stmt::Try(try_stmt) => {
                // Try-catch-finally statement
                //
                // Control flow:
                // - try-catch-finally: try -> PopTry -> finally -> end; on exception -> catch -> finally -> end
                // - try-catch: try -> PopTry -> end; on exception -> catch -> end
                // - try-finally: try -> PopTry -> finally -> end; on exception -> finally -> rethrow

                let has_catch = try_stmt.handler.is_some();
                let has_finally = try_stmt.finalizer.is_some();

                // An exception can arrive from inside a nested environment;
                // the handlers restore `%scope` from here.
                let try_env = self.save_env();

                // 1. Emit SetupTry with placeholder addresses (will backpatch)
                let setup_try_idx = self.instructions.len();
                self.instructions.push(OpCode::SetupTry {
                    catch_addr: 0,
                    finally_addr: 0,
                });

                // 2. Emit try block
                self.try_stack.push(TryRegion {
                    part: TryPart::Try,
                    env: try_env.clone(),
                    finalizer: try_stmt.finalizer.clone(),
                });
                let block_env = self.enter_scope_env(try_stmt.block.span);
                self.scope_stack.push(Vec::new());
                self.hoist_functions(&try_stmt.block.stmts);
                for s in &try_stmt.block.stmts {
                    self.gen_stmt(s);
                }
                self.exit_scope_env(block_env);
                self.try_stack.pop();
                // Drop try block scope variables
                if let Some(locals) = self.scope_stack.pop() {
                    for name in locals.into_iter().rev() {
                        self.instructions.push(OpCode::Drop(name));
                    }
                }

                // 3. PopTry - remove exception handler if no exception occurred
                self.instructions.push(OpCode::PopTry);

                // 4. Jump after try block (target depends on structure)
                let jump_after_try_idx = self.instructions.len();
                self.instructions.push(OpCode::Jump(0)); // Will backpatch

                // 5. Catch block (if present)
                let catch_addr = if has_catch {
                    let addr = self.instructions.len();
                    if let Some(handler) = &try_stmt.handler {
                        self.scope_stack.push(Vec::new());
                        // [exception]
                        self.restore_env(&try_env);
                        let catch_env = self.enter_scope_env(handler.span);

                        // Bind exception to catch parameter if present
                        if let Some(param) = &handler.param {
                            if let Pat::Ident(id) = param {
                                // Exception value is on stack, bind it
                                self.store_var(&id.id, true);
                                if let Some(scope) = self.scope_stack.last_mut() {
                                    scope.push(id.id.sym.to_string());
                                }
                            } else {
                                self.unsupported("destructuring in a `catch` parameter");
                            }
                        } else {
                            // No catch parameter, pop the exception
                            self.instructions.push(OpCode::Pop);
                        }

                        // Generate catch block body
                        self.try_stack.push(TryRegion {
                            part: TryPart::Catch,
                            env: try_env.clone(),
                            finalizer: try_stmt.finalizer.clone(),
                        });
                        self.hoist_functions(&handler.body.stmts);
                        for s in &handler.body.stmts {
                            self.gen_stmt(s);
                        }
                        self.try_stack.pop();
                        self.exit_scope_env(catch_env);

                        // Drop catch block scope variables
                        if let Some(locals) = self.scope_stack.pop() {
                            for name in locals.into_iter().rev() {
                                self.instructions.push(OpCode::Drop(name));
                            }
                        }

                        // The VM re-arms a finally-only handler when it
                        // dispatches to catch; pop it if the catch completes
                        // without throwing (it then falls through to finally).
                        if has_finally {
                            self.instructions.push(OpCode::PopTry);
                        }
                    }
                    addr
                } else {
                    0 // No catch block
                };

                // 6. Finally block (if present)
                let finally_addr = if has_finally {
                    let addr = self.instructions.len();
                    if let Some(finalizer) = &try_stmt.finalizer {
                        self.restore_env(&try_env);
                        self.try_stack.push(TryRegion {
                            part: TryPart::Finally,
                            env: try_env.clone(),
                            finalizer: None,
                        });
                        self.gen_finally_body(finalizer);
                        self.try_stack.pop();
                    }
                    // Re-throw a pending exception (throw with no catch, or a
                    // throw from inside catch) once the finally has run.
                    self.instructions.push(OpCode::EnterFinally(true));
                    addr
                } else {
                    0 // No finally block
                };

                // 7. End address - where normal execution continues
                let end_addr = self.instructions.len();

                // Backpatch SetupTry addresses
                if let OpCode::SetupTry {
                    catch_addr: ref mut c,
                    finally_addr: ref mut f,
                } = self.instructions[setup_try_idx]
                {
                    *c = catch_addr;
                    *f = finally_addr;
                }

                // Backpatch jump after try block
                // - If has finally: jump to finally
                // - If no finally: jump to end
                if let OpCode::Jump(ref mut addr) = self.instructions[jump_after_try_idx] {
                    *addr = if has_finally { finally_addr } else { end_addr };
                }
            }
            Stmt::Throw(throw_stmt) => {
                // Throw statement: push exception value and emit Throw opcode
                self.gen_expr(&throw_stmt.arg);
                // `throw new Error(...)` is located where the error is
                // made, as JavaScript stack traces are.
                if let Expr::New(new) = throw_stmt.arg.as_ref() {
                    self.mark(new.span);
                }
                self.instructions.push(OpCode::Throw);
            }
            Stmt::Switch(switch) => self.gen_switch(switch),
            Stmt::Labeled(labeled) => {
                let name = labeled.label.sym.to_string();
                if self.loop_stack.iter().any(|c| c.labels.contains(&name))
                    || self.pending_labels.contains(&name)
                {
                    self.syntax_error(format!("label `{name}` is already declared"));
                    return;
                }
                let mut body = labeled.body.as_ref();
                while let Stmt::Labeled(inner) = body {
                    body = inner.body.as_ref();
                }
                let is_loop = matches!(
                    body,
                    Stmt::For(_)
                        | Stmt::ForIn(_)
                        | Stmt::ForOf(_)
                        | Stmt::While(_)
                        | Stmt::DoWhile(_)
                );
                if is_loop {
                    // The loop takes the label: `continue name` reaches it.
                    self.pending_labels.push(name);
                    self.gen_stmt(&labeled.body);
                } else {
                    let mut labels = std::mem::take(&mut self.pending_labels);
                    labels.push(name);
                    let break_env = self.save_env();
                    self.loop_stack.push(LoopContext {
                        kind: TargetKind::Labeled,
                        labels,
                        break_jumps: Vec::new(),
                        continue_jumps: Vec::new(),
                        break_env,
                        continue_env: None,
                        try_depth: self.try_stack.len(),
                    });
                    self.gen_stmt(&labeled.body);
                    let end = self.instructions.len();
                    self.pop_target(end, end);
                }
            }
            Stmt::Decl(Decl::Using(_)) => self.unsupported("`using` declarations"),
        }
    }

    /// `switch`: the discriminant goes in a temporary; each `case` test is
    /// compared with `===` in source order, and the first match (or else
    /// `default`) jumps into the run of case bodies, which fall through.
    fn gen_switch(&mut self, switch: &swc_ecma_ast::SwitchStmt) {
        self.gen_expr(&switch.discriminant);
        let disc = format!("%switch{}", self.instructions.len());
        self.instructions.push(OpCode::Let(disc.clone()));
        self.scope_stack.push(vec![disc.clone()]);

        // The case bodies share one block scope.
        let break_env = self.save_env();
        if self.scopes.env_scope(switch.span).is_some() {
            self.emit_child_env();
            self.instructions
                .push(OpCode::Store(SCOPE_LOCAL.to_string()));
        }
        self.hoist_functions(switch.cases.iter().flat_map(|c| c.cons.iter()));
        self.loop_stack.push(LoopContext {
            kind: TargetKind::Switch,
            labels: Vec::new(),
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            break_env: break_env.clone(),
            continue_env: None,
            try_depth: self.try_stack.len(),
        });

        let mut matched_jumps = Vec::new();
        for case in &switch.cases {
            if let Some(test) = &case.test {
                self.instructions.push(OpCode::Load(disc.clone()));
                self.gen_expr(test);
                self.instructions.push(OpCode::Eq);
                let skip = self.emit_jump_if_false();
                matched_jumps.push(Some(self.emit_jump()));
                self.patch_jump_here(skip);
            } else {
                matched_jumps.push(None);
            }
        }
        let no_match = self.emit_jump();

        let mut default_addr = None;
        for (case, jump) in switch.cases.iter().zip(&matched_jumps) {
            match jump {
                Some(idx) => self.patch_jump_here(*idx),
                None => default_addr = Some(self.instructions.len()),
            }
            for s in &case.cons {
                self.gen_stmt(s);
            }
        }
        let end = self.instructions.len();
        if let OpCode::Jump(ref mut addr) = self.instructions[no_match] {
            *addr = default_addr.unwrap_or(end);
        }
        self.restore_env(&break_env);
        self.pop_target(end, end);
        if let Some(locals) = self.scope_stack.pop() {
            for name in locals.into_iter().rev() {
                self.instructions.push(OpCode::Drop(name));
            }
        }
    }

    fn gen_expr(&mut self, expr: &Expr) {
        let saved = std::mem::replace(&mut self.current_span, expr.span());
        // A property access is located at the property (`o.x.y`: `y`), a
        // call at its callee's property, as in V8's stack traces.
        let at = match expr {
            Expr::Member(m) => m.prop.span(),
            Expr::Call(CallExpr {
                callee: Callee::Expr(callee),
                ..
            }) => match callee.as_ref() {
                Expr::Member(m) => m.prop.span(),
                _ => expr.span(),
            },
            _ => expr.span(),
        };
        let saved_mark = std::mem::replace(&mut self.mark_span, at);
        self.mark(at);
        self.gen_expr_inner(expr);
        self.current_span = saved;
        self.mark_span = saved_mark;
        self.mark(saved_mark);
    }

    fn gen_expr_inner(&mut self, expr: &Expr) {
        match expr {
            Expr::Fn(fn_expr) => {
                // `function (a, b) { ... }`: a function value. A name is
                // bound for the function's own body; references to it are
                // always from inside, so it lives in an environment.
                if let Some(ident) = &fn_expr.ident {
                    self.next_fn_name = Some(ident.sym.to_string());
                }
                match &fn_expr.ident {
                    Some(ident) if self.scopes.env_scope(ident.span).is_some() => {
                        let env = self.enter_scope_env(ident.span);
                        self.gen_fn_decl(None, &fn_expr.function);
                        self.instructions.push(OpCode::Dup);
                        self.store_var(ident, true);
                        self.exit_scope_env(env);
                    }
                    _ => self.gen_fn_decl(None, &fn_expr.function),
                }
            }
            Expr::Arrow(arrow) => {
                // `(a, b) => expr` or `(a, b) => { ... }`; an expression body
                // is `{ return expr; }`.
                let value_idx = self.emit_function_value(arrow.span, 0);
                let jump_idx = self.emit_jump();
                let body_start = self.instructions.len();
                self.patch_function_value(value_idx, body_start);
                let pats: Vec<&Pat> = arrow.params.iter().collect();
                let owned;
                let stmts: &[Stmt] = match &*arrow.body {
                    BlockStmtOrExpr::BlockStmt(block) => &block.stmts,
                    BlockStmtOrExpr::Expr(e) => {
                        owned = [Stmt::Return(ReturnStmt {
                            span: e.span(),
                            arg: Some(e.clone()),
                        })];
                        &owned
                    }
                };
                self.gen_function_body(
                    arrow.span,
                    &pats,
                    stmts,
                    arrow.is_async,
                    FnKind::Plain,
                    &[],
                );
                self.patch_jump_here(jump_idx);
            }
            Expr::Lit(Lit::Num(num)) => {
                self.instructions
                    .push(OpCode::Push(JsValue::Number(num.value)));
            }
            Expr::Lit(Lit::Str(s)) => {
                self.instructions.push(OpCode::Push(JsValue::String(
                    s.value.to_string_lossy().to_string(),
                )));
            }
            Expr::Lit(Lit::Bool(b)) => {
                self.instructions
                    .push(OpCode::Push(JsValue::Boolean(b.value)));
            }
            Expr::Lit(Lit::Null(_)) => {
                self.instructions.push(OpCode::Push(JsValue::Null));
            }
            Expr::Ident(id) => {
                self.load_var(id);
            }
            Expr::Bin(bin)
                if matches!(
                    bin.op,
                    BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::NullishCoalescing
                ) =>
            {
                self.gen_logical(bin);
            }
            Expr::OptChain(chain) => {
                self.gen_opt_chain_expr(chain);
            }
            Expr::Bin(bin) if bin.op == BinaryOp::In => {
                // `key in obj`; `#x in obj` asks for the private member.
                match bin.left.as_ref() {
                    Expr::PrivateName(pn) => {
                        let key = self.private_key(&pn.name);
                        self.instructions.push(OpCode::Push(JsValue::String(key)));
                    }
                    left => self.gen_expr(left),
                }
                self.gen_expr(&bin.right);
                self.instructions.push(OpCode::In);
            }
            Expr::Bin(bin) => {
                self.gen_expr(&bin.left);
                self.gen_expr(&bin.right);
                match bin.op {
                    BinaryOp::Add => self.instructions.push(OpCode::Add),
                    BinaryOp::Sub => self.instructions.push(OpCode::Sub),
                    BinaryOp::Mul => self.instructions.push(OpCode::Mul),
                    BinaryOp::Div => self.instructions.push(OpCode::Div),
                    BinaryOp::Mod => self.instructions.push(OpCode::Mod),
                    BinaryOp::EqEq => self.instructions.push(OpCode::EqEq), // == (loose equality)
                    BinaryOp::EqEqEq => self.instructions.push(OpCode::Eq), // === (strict equality)
                    BinaryOp::NotEq => self.instructions.push(OpCode::NeEq), // != (loose inequality)
                    BinaryOp::NotEqEq => self.instructions.push(OpCode::Ne), // !== (strict inequality)
                    BinaryOp::Lt => self.instructions.push(OpCode::Lt),
                    BinaryOp::LtEq => self.instructions.push(OpCode::LtEq),
                    BinaryOp::Gt => self.instructions.push(OpCode::Gt),
                    BinaryOp::GtEq => self.instructions.push(OpCode::GtEq),
                    BinaryOp::InstanceOf => self.instructions.push(OpCode::InstanceOf),
                    // Bitwise operators
                    BinaryOp::BitAnd => self.instructions.push(OpCode::BitAnd),
                    BinaryOp::BitOr => self.instructions.push(OpCode::BitOr),
                    BinaryOp::BitXor => self.instructions.push(OpCode::Xor),
                    BinaryOp::LShift => self.instructions.push(OpCode::ShiftLeft),
                    BinaryOp::RShift => self.instructions.push(OpCode::ShiftRight),
                    BinaryOp::ZeroFillRShift => self.instructions.push(OpCode::ShiftRightUnsigned),
                    BinaryOp::Exp => self.instructions.push(OpCode::Pow),
                    other => self.unsupported(format!("binary operator `{}`", other.as_str())),
                }
            }
            Expr::Unary(unary) => {
                match unary.op {
                    UnaryOp::TypeOf => {
                        if let Expr::Ident(id) = unary.arg.as_ref()
                            && self.scopes.var(id.span) == VarRef::Plain
                        {
                            let name = self.var_name(id);
                            self.instructions.push(OpCode::TypeOfName(name));
                        } else {
                            self.gen_expr(&unary.arg);
                            self.instructions.push(OpCode::TypeOf);
                        }
                    }
                    UnaryOp::Delete => {
                        // delete operator - handle member expressions specially
                        if let Expr::Member(member) = unary.arg.as_ref() {
                            self.gen_expr(&member.obj);
                            if let MemberProp::Ident(id) = &member.prop {
                                self.instructions.push(OpCode::Delete(id.sym.to_string()));
                            } else {
                                // Computed property - evaluate and discard, return true
                                self.instructions.push(OpCode::Pop);
                                self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
                            }
                        } else {
                            // delete on non-member always returns true (but evaluates the expr)
                            self.gen_expr(&unary.arg);
                            self.instructions.push(OpCode::Pop);
                            self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
                        }
                    }
                    _ => {
                        self.gen_expr(&unary.arg);
                        match unary.op {
                            UnaryOp::Bang => self.instructions.push(OpCode::Not),
                            UnaryOp::Minus => self.instructions.push(OpCode::Neg),
                            UnaryOp::Plus => {} // +x is a no-op for numbers
                            UnaryOp::Tilde => {
                                // Bitwise NOT: ~x = -(x+1) approximately, or convert to i32 and flip bits
                                // For now, implement as: push -1, xor
                                self.instructions.push(OpCode::Push(JsValue::Number(-1.0)));
                                self.instructions.push(OpCode::Xor);
                            }
                            UnaryOp::Void => {
                                // void expr - evaluate and discard, push undefined
                                self.instructions.push(OpCode::Pop);
                                self.instructions.push(OpCode::Push(JsValue::Undefined));
                            }
                            other => {
                                self.unsupported(format!("unary operator `{}`", other.as_str()))
                            }
                        }
                    }
                }
            }
            Expr::Array(arr_lit) => {
                // Check if any elements are spread elements
                let has_spread = arr_lit
                    .elems
                    .iter()
                    .any(|elem| elem.as_ref().is_some_and(|e| e.spread.is_some()));

                if has_spread {
                    // Use dynamic approach: create empty array, then push/spread each element
                    self.instructions.push(OpCode::NewArray(0));
                    for expr_or_spread in arr_lit.elems.iter().flatten() {
                        self.gen_expr(&expr_or_spread.expr);
                        if expr_or_spread.spread.is_some() {
                            // Spread: Stack is [array, source_array]
                            self.instructions.push(OpCode::ArraySpread);
                        } else {
                            // Regular element: Stack is [array, value]
                            self.instructions.push(OpCode::ArrayPush);
                        }
                    }
                } else {
                    // Use static approach: create array with known size
                    let size = arr_lit.elems.len();
                    self.instructions.push(OpCode::NewArray(size));

                    for (i, elem) in arr_lit.elems.iter().enumerate() {
                        if let Some(expr_or_spread) = elem {
                            // 1. Dup the array pointer so we don't lose it
                            self.instructions.push(OpCode::Dup);
                            // 2. Push the Value
                            self.gen_expr(&expr_or_spread.expr);
                            // 3. Push the Index
                            self.instructions
                                .push(OpCode::Push(JsValue::Number(i as f64)));
                            // 4. Store it
                            self.instructions.push(OpCode::StoreElement);
                        }
                    }
                }
            }
            Expr::Call(call_expr) => {
                // `obj.name(args)`: a method call with `this = obj`. Only for
                // an identifier property: for `obj[k](args)` this used to
                // emit the arguments and object, then fall through and emit
                // them again, leaving junk on the stack.
                if let Callee::Expr(callee_expr) = &call_expr.callee
                    && let Expr::Member(member) = callee_expr.as_ref()
                    && let Some(key) = self.member_key(&member.prop)
                {
                    for arg in &call_expr.args {
                        self.gen_expr(&arg.expr);
                    }
                    self.gen_expr(&member.obj);
                    self.instructions
                        .push(OpCode::CallMethod(key, call_expr.args.len()));
                    return;
                }
                // `super.name(args)`: the parent's method, with this `this`.
                if let Callee::Expr(callee_expr) = &call_expr.callee
                    && let Expr::SuperProp(sp) = callee_expr.as_ref()
                {
                    for arg in &call_expr.args {
                        self.gen_expr(&arg.expr);
                    }
                    self.gen_super_prop(sp);
                    self.instructions.push(OpCode::LoadThis);
                    self.instructions
                        .push(OpCode::CallWithThis(call_expr.args.len()));
                    return;
                }
                // `super(args)`: the parent constructor runs on `this`; then
                // this class's instance fields are initialized.
                if let Callee::Super(sup) = &call_expr.callee {
                    for arg in &call_expr.args {
                        self.gen_expr(&arg.expr);
                    }
                    self.load_var(&scope::super_ident(sup.span));
                    self.instructions
                        .push(OpCode::CallSuper(call_expr.args.len()));
                    if let Some(fields) = self.class_fields.clone() {
                        self.emit_field_inits(&fields);
                    }
                    return;
                }
                // detect if this is a 'require' call
                if let Callee::Expr(expr) = &call_expr.callee
                    && let Expr::Ident(id) = expr.as_ref()
                    && id.sym == "require"
                    && let Some(arg) = call_expr.args.first()
                {
                    self.gen_expr(&arg.expr);
                    self.instructions.push(OpCode::Require);
                    return;
                }

                let arg_count = call_expr.args.len();
                for arg in &call_expr.args {
                    self.gen_expr(&arg.expr);
                }
                match &call_expr.callee {
                    Callee::Expr(expr) => self.gen_expr(expr),
                    Callee::Super(_) => unreachable!("handled above"),
                    Callee::Import(_) => self.unsupported("dynamic `import()`"),
                }
                self.instructions.push(OpCode::Call(arg_count));
            }
            Expr::Assign(assign_expr) if assign_expr.op != AssignOp::Assign => {
                self.gen_compound_assign(assign_expr);
            }
            Expr::Assign(assign_expr) => {
                // Handle different assignment targets
                match &assign_expr.left {
                    AssignTarget::Simple(simple) => match simple {
                        SimpleAssignTarget::Ident(binding_ident) => {
                            // Simple variable assignment: x = value
                            self.gen_expr(&assign_expr.right);
                            // JS assignment expressions evaluate to the assigned value.
                            // The store consumes the value, so `Dup` keeps one copy
                            // on the stack for expression context (e.g. `a = 1;`).
                            self.instructions.push(OpCode::Dup);
                            self.store_var(&binding_ident.id, false);
                        }
                        SimpleAssignTarget::Member(member_expr) => {
                            // Member assignment: obj.prop = value or this.prop = value
                            // Stack order for SetProp: [object, value] -> pops both, sets prop
                            self.gen_expr(&member_expr.obj); // Push the object
                            self.gen_expr(&assign_expr.right); // Push the value

                            // The store ops consume the object and value and
                            // push nothing, but an assignment expression
                            // evaluates to the value (the statement's `Pop`
                            // removes it). Keep a copy underneath:
                            // [obj, v] -> Dup -> [obj, v, v] -> Swap3 ->
                            // [v, v, obj] -> Swap -> [v, obj, v].
                            self.instructions.push(OpCode::Dup);
                            self.instructions.push(OpCode::Swap3);
                            self.instructions.push(OpCode::Swap);

                            match &member_expr.prop {
                                MemberProp::Ident(id) => {
                                    // obj.prop = value
                                    self.instructions.push(OpCode::SetProp(id.sym.to_string()));
                                }
                                MemberProp::Computed(computed) => {
                                    // obj[key] = value
                                    // Stack: [obj, value, key]
                                    self.gen_expr(&computed.expr); // Push the key
                                    self.instructions.push(OpCode::SetPropComputed);
                                }
                                MemberProp::PrivateName(pn) => {
                                    // obj.#name = value
                                    let key = self.private_key(&pn.name);
                                    self.instructions.push(OpCode::SetPrivate(key));
                                }
                            }
                        }
                        _ => self.unsupported("this assignment target"),
                    },
                    _ => self.unsupported("destructuring assignment"),
                }
            }
            Expr::Object(obj_lit) => {
                self.instructions.push(OpCode::NewObject);

                for prop in &obj_lit.props {
                    match prop {
                        PropOrSpread::Spread(spread) => {
                            // { ...source } - spread all properties from source object
                            self.gen_expr(&spread.expr);
                            self.instructions.push(OpCode::ObjectSpread);
                        }
                        PropOrSpread::Prop(prop_ptr) => {
                            match prop_ptr.as_ref() {
                                Prop::KeyValue(kv) => {
                                    let key = match &kv.key {
                                        PropName::Ident(id) => id.sym.to_string(),
                                        PropName::Str(s) => s
                                            .value
                                            .as_str()
                                            .expect("Invalid string key")
                                            .to_string(),
                                        PropName::Num(n) => n.value.to_string(),
                                        _ => {
                                            self.unsupported("computed object literal keys");
                                            continue;
                                        }
                                    };

                                    self.instructions.push(OpCode::Dup); // Duplicate Ptr
                                    self.gen_expr(&kv.value); // Push Value
                                    self.instructions.push(OpCode::SetProp(key)); // Consumes Value + 1 Ptr
                                }
                                Prop::Shorthand(ident) => {
                                    // { x } shorthand for { x: x }
                                    let key = ident.sym.to_string();
                                    self.instructions.push(OpCode::Dup);
                                    self.load_var(ident);
                                    self.instructions.push(OpCode::SetProp(key));
                                }
                                Prop::Method(method) => {
                                    // { fn() {} } - inline method
                                    let key = match &method.key {
                                        PropName::Ident(id) => id.sym.to_string(),
                                        _ => {
                                            self.unsupported(
                                                "computed object literal method names",
                                            );
                                            continue;
                                        }
                                    };
                                    self.instructions.push(OpCode::Dup);
                                    self.next_fn_name = Some(key.clone());
                                    self.gen_fn_decl(None, &method.function);
                                    self.instructions.push(OpCode::SetProp(key));
                                }
                                Prop::Getter(_) | Prop::Setter(_) => {
                                    self.unsupported("getters and setters in object literals")
                                }
                                Prop::Assign(_) => {
                                    self.unsupported("`key = value` in an object literal")
                                }
                            }
                        }
                    }
                }
            }
            Expr::SuperProp(super_prop) => self.gen_super_prop(super_prop),
            Expr::Member(member) => {
                // Regular obj.prop access
                // 1. Load the Object/Array
                self.gen_expr(&member.obj);

                match &member.prop {
                    // Handle obj.prop
                    MemberProp::Ident(id) => {
                        self.instructions.push(OpCode::GetProp(id.sym.to_string()));
                    }
                    // Handle arr[index]
                    MemberProp::Computed(computed) => {
                        self.gen_expr(&computed.expr); // Push the index expression
                        self.instructions.push(OpCode::GetPropComputed);
                    }
                    // `obj.#name`: a property named `#name`.
                    MemberProp::PrivateName(pn) => {
                        self.instructions
                            .push(OpCode::GetPrivate(self.private_key(&pn.name)));
                    }
                }
            }
            Expr::This(t) => {
                // In an arrow function: the enclosing function's `this`.
                if let VarRef::Env { .. } = self.scopes.var(t.span) {
                    self.load_var(&scope::this_ident(t.span));
                    return;
                }
                self.instructions.push(OpCode::LoadThis);
            }
            Expr::MetaProp(meta_prop) => {
                match meta_prop.kind {
                    MetaPropKind::NewTarget => {
                        // new.target - push the constructor that was called with new
                        self.instructions.push(OpCode::NewTarget);
                    }
                    MetaPropKind::ImportMeta => {
                        // import.meta - not yet supported
                        self.instructions.push(OpCode::Push(JsValue::Undefined));
                    }
                }
            }
            Expr::New(new_expr) => {
                // new Foo(arg1, arg2) compiles to [args..., Foo] Construct;
                // the VM creates `this`. (This also pushed a `NewObject`
                // and its `Dup` that nothing consumed, so two stale objects
                // were left behind by every `new`, displacing the
                // arguments of an enclosing call.)

                // 1. Push arguments
                let arg_count = new_expr.args.as_ref().map(|a| a.len()).unwrap_or(0);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        self.gen_expr(&arg.expr);
                    }
                }

                // 2. Push the constructor function
                self.gen_expr(&new_expr.callee);

                // 3. Call with construct semantics
                self.instructions.push(OpCode::Construct(arg_count));
            }
            Expr::Paren(paren_expr) => {
                // Parenthesized expression: just evaluate the inner expression
                self.gen_expr(&paren_expr.expr);
            }
            Expr::Cond(cond_expr) => {
                // Conditional expression: condition ? consequent : alternate
                // Stack: [condition, consequent, alternate] -> [result]

                // Compile condition
                self.gen_expr(&cond_expr.test);

                // Jump indices for jump if false and end
                let jump_if_false_idx = self.instructions.len();
                self.instructions.push(OpCode::JumpIfFalse(0)); // placeholder

                // Compile consequent
                self.gen_expr(&cond_expr.cons);

                // Jump to end after consequent
                let jump_end_idx = self.instructions.len();
                self.instructions.push(OpCode::Jump(0)); // placeholder

                // Backpatch jump_if_false
                let after_consequent = self.instructions.len();
                if let OpCode::JumpIfFalse(ref mut addr) = self.instructions[jump_if_false_idx] {
                    *addr = after_consequent;
                }

                // Compile alternate
                self.gen_expr(&cond_expr.alt);

                // Backpatch jump_end
                let after_alternate = self.instructions.len();
                if let OpCode::Jump(ref mut addr) = self.instructions[jump_end_idx] {
                    *addr = after_alternate;
                }
            }
            Expr::Tpl(tpl) => {
                // Template literal: `Hello ${name}!`
                // Compilation strategy:
                // 1. Push empty string as starting point
                // 2. For each quasi/expr pair:
                //    - Push the quasi string, concatenate
                //    - Compile the expr, concatenate
                // 3. Push the final quasi if exists

                // Handle empty template literal ``
                if tpl.quasis.is_empty() && tpl.exprs.is_empty() {
                    self.instructions
                        .push(OpCode::Push(JsValue::String("".to_string())));
                    return;
                }

                // Start with empty string
                self.instructions
                    .push(OpCode::Push(JsValue::String("".to_string())));

                // Iterate through quasis and exprs
                for (i, quasi) in tpl.quasis.iter().enumerate() {
                    // Push the quasi string (cooked value - escapes processed, or raw if not available)
                    let s_str = match quasi.cooked.as_ref() {
                        Some(wtf8) => String::from_utf8_lossy(wtf8.as_bytes()).into_owned(),
                        None => String::from_utf8_lossy(quasi.raw.as_bytes()).into_owned(),
                    };
                    self.instructions.push(OpCode::Push(JsValue::String(s_str)));
                    // Concatenate: "prefix" + result so far
                    self.instructions.push(OpCode::Add);

                    // If there's a corresponding expression, compile and concatenate it
                    if i < tpl.exprs.len() {
                        self.gen_expr(&tpl.exprs[i]);
                        // `${v}` is String(v): a `toString` comes first.
                        self.instructions.push(OpCode::ToStr);
                        self.instructions.push(OpCode::Add);
                    }
                }
            }
            Expr::Await(await_expr) => {
                if !self.in_async_function {
                    self.gen_expr(&await_expr.arg);
                } else {
                    self.gen_expr(&await_expr.arg);
                    self.instructions.push(OpCode::Await);
                }
            }
            Expr::Update(update_expr) => {
                if let Expr::Ident(id) = update_expr.arg.as_ref() {
                    self.load_var(id);
                    if update_expr.prefix {
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if update_expr.op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        self.instructions.push(OpCode::Dup);
                        self.store_var(id, false);
                    } else {
                        self.instructions.push(OpCode::Dup);
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if update_expr.op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        self.store_var(id, false);
                    }
                } else if let Expr::Member(member) = update_expr.arg.as_ref() {
                    // Member expression: obj.prop++ / ++obj.prop / obj[key]++ / ++obj[key]
                    let op = update_expr.op;
                    if update_expr.prefix {
                        // ++obj.prop: compute new value, store it, leave new value on stack
                        // Stack: [obj, obj] -> GetProp -> [obj, old] -> +1 -> [obj, new] -> SetProp -> []
                        // Then reload to get new value on stack
                        self.gen_expr(&member.obj);
                        self.instructions.push(OpCode::Dup);
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_get(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                        }
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        // Stack: [obj, new_val] -> SetProp -> []
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_set(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::SetPropComputed);
                            }
                        }
                        // Reload the new value
                        self.gen_expr(&member.obj);
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_get(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                        }
                    } else {
                        // obj.prop++: get old value, save it, compute new, store back
                        // Stack: [obj, obj] -> GetProp -> [obj, old] -> Swap -> [old, obj]
                        // -> Dup -> [old, obj, obj] -> GetProp -> [old, obj, old2]
                        // -> +1 -> [old, obj, new] -> SetProp -> [old]
                        self.gen_expr(&member.obj);
                        self.instructions.push(OpCode::Dup);
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_get(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                        }
                        // Stack: [obj, old_val] -> Swap -> [old_val, obj]
                        self.instructions.push(OpCode::Swap);
                        self.instructions.push(OpCode::Dup);
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_get(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::GetPropComputed);
                            }
                        }
                        self.instructions.push(OpCode::Push(JsValue::Number(1.0)));
                        if op == UpdateOp::PlusPlus {
                            self.instructions.push(OpCode::Add);
                        } else {
                            self.instructions.push(OpCode::Sub);
                        }
                        // Stack: [old_val, obj, new_val] -> SetProp -> [old_val]
                        match &member.prop {
                            MemberProp::Ident(_) | MemberProp::PrivateName(_) => {
                                self.emit_member_set(&member.prop);
                            }
                            MemberProp::Computed(c) => {
                                self.gen_expr(&c.expr);
                                self.instructions.push(OpCode::SetPropComputed);
                            }
                        }
                    }
                }
            }
            Expr::TsAs(ts_as) => {
                // TypeScript `as` type assertion - evaluate inner expression (no runtime effect)
                self.gen_expr(&ts_as.expr);
            }
            Expr::TsTypeAssertion(ts_assert) => {
                // TypeScript type assertion `<Type>expr` - evaluate inner expression
                self.gen_expr(&ts_assert.expr);
            }
            Expr::TsNonNull(ts_non_null) => {
                // TypeScript non-null assertion `expr!` - evaluate inner expression
                self.gen_expr(&ts_non_null.expr);
            }
            // More type-only wrappers with no runtime effect.
            Expr::TsConstAssertion(e) => self.gen_expr(&e.expr),
            Expr::TsSatisfies(e) => self.gen_expr(&e.expr),
            Expr::TsInstantiation(e) => self.gen_expr(&e.expr),
            Expr::Seq(seq) => {
                // `a, b, c`: evaluate all, keep the last value.
                for (i, e) in seq.exprs.iter().enumerate() {
                    self.gen_expr(e);
                    if i + 1 < seq.exprs.len() {
                        self.instructions.push(OpCode::Pop);
                    }
                }
            }
            Expr::Lit(Lit::Regex(_)) => self.unsupported("regular expression literals"),
            Expr::Lit(Lit::BigInt(_)) => self.unsupported("BigInt literals"),
            Expr::Lit(_) => self.unsupported("this literal"),
            Expr::TaggedTpl(_) => self.unsupported("tagged template literals"),
            Expr::Yield(_) => self.unsupported("`yield`"),
            Expr::Class(class_expr) => {
                let name = class_expr.ident.as_ref().map(|id| id.sym.to_string());
                self.gen_class(&class_expr.class, name.as_deref());
            }
            Expr::PrivateName(_) => self.unsupported("bare private name"),
            Expr::JSXMember(_)
            | Expr::JSXNamespacedName(_)
            | Expr::JSXEmpty(_)
            | Expr::JSXElement(_)
            | Expr::JSXFragment(_) => self.unsupported("JSX"),
            Expr::Invalid(_) => self.unsupported("invalid expression"),
        }
    }

    /// A class: a wrapper object (`constructor`, `prototype`, `name`, static
    /// members) whose `prototype` holds the methods. A derived class links
    /// both to its parent's, and binds the parent as `%super`, which the
    /// constructor and methods reach as a captured variable.
    /// Stack: [] -> [class]
    fn gen_class(&mut self, class: &Class, name: Option<&str>) {
        let prev_class_name = std::mem::replace(&mut self.class_name, name.map(str::to_string));
        self.gen_class_inner(class, name);
        self.class_name = prev_class_name;
    }

    fn gen_class_inner(&mut self, class: &Class, name: Option<&str>) {
        for member in &class.body {
            let member_decorated = match member {
                ClassMember::StaticBlock(_) => {
                    self.unsupported("static initialization blocks");
                    false
                }
                ClassMember::AutoAccessor(_) => {
                    self.unsupported("`accessor` class fields");
                    false
                }
                ClassMember::Method(m) => {
                    m.function.params.iter().any(|p| !p.decorators.is_empty())
                }
                ClassMember::PrivateMethod(m) => {
                    m.function.params.iter().any(|p| !p.decorators.is_empty())
                }
                ClassMember::Constructor(k) => k.params.iter().any(|p| match p {
                    ParamOrTsParamProp::Param(p) => !p.decorators.is_empty(),
                    ParamOrTsParamProp::TsParamProp(tp) => !tp.decorators.is_empty(),
                }),
                _ => false,
            };
            if member_decorated {
                self.unsupported("parameter decorators");
            }
        }
        let id = self.instructions.len();
        let wrapper = format!("%class{}", id);
        let proto = format!("%proto{}", id);
        let private: HashMap<String, String> = class
            .body
            .iter()
            .filter_map(|m| match m {
                ClassMember::PrivateProp(p) => Some(p.key.name.to_string()),
                ClassMember::PrivateMethod(m) => Some(m.key.name.to_string()),
                _ => None,
            })
            .map(|n| (n.clone(), format!("#{}@{}", n, class.span.lo.0)))
            .collect();
        self.private_names.push(private);

        // The class scope: `%super`, and what field decorators return.
        if let Some(parent) = &class.super_class {
            self.gen_expr(parent);
        }
        let class_env = self.enter_scope_env(class.span);
        if class.super_class.is_some() {
            self.store_var(&scope::super_ident(class.span), true);
        }
        let parent_span = class.super_class.as_ref().map(|_| class.span);

        // The prototype, inheriting from the parent's.
        self.instructions.push(OpCode::NewObject);
        self.instructions.push(OpCode::Let(proto.clone()));
        if let Some(ps) = parent_span {
            self.instructions.push(OpCode::Load(proto.clone()));
            self.load_var(&scope::super_ident(ps));
            self.instructions
                .push(OpCode::GetProp("prototype".to_string()));
            self.instructions
                .push(OpCode::SetProp("__proto__".to_string()));
        }

        // The class object; static members inherit from the parent class.
        self.instructions.push(OpCode::NewObject);
        self.instructions.push(OpCode::Let(wrapper.clone()));
        // Marks a class: `typeof` is "function", printed as `[class C]`.
        self.instructions.push(OpCode::Load(wrapper.clone()));
        self.instructions.push(OpCode::Push(JsValue::Boolean(true)));
        self.instructions
            .push(OpCode::SetProp("__class__".to_string()));
        if let Some(class_name) = name {
            self.instructions.push(OpCode::Load(wrapper.clone()));
            self.instructions
                .push(OpCode::Push(JsValue::String(class_name.to_string())));
            self.instructions.push(OpCode::SetProp("name".to_string()));
        }
        self.instructions.push(OpCode::Load(wrapper.clone()));
        self.instructions.push(OpCode::Load(proto.clone()));
        self.instructions
            .push(OpCode::SetProp("prototype".to_string()));
        self.instructions.push(OpCode::Load(proto.clone()));
        self.instructions.push(OpCode::Load(wrapper.clone()));
        self.instructions
            .push(OpCode::SetProp("constructor".to_string()));
        if let Some(ps) = parent_span {
            self.instructions.push(OpCode::Load(wrapper.clone()));
            self.load_var(&scope::super_ident(ps));
            self.instructions
                .push(OpCode::SetProp("__proto__".to_string()));
        }

        // The constructor, with the instance fields.
        let fields: Vec<ClassField> = class
            .body
            .iter()
            .filter_map(|m| match m {
                ClassMember::ClassProp(p) if !p.is_static => {
                    self.class_key(&p.key).map(|key| ClassField {
                        key,
                        value: p.value.clone(),
                        decorators: p.decorators.iter().map(|d| d.span).collect(),
                    })
                }
                ClassMember::PrivateProp(p) if !p.is_static => Some(ClassField {
                    key: self.private_key(&p.key.name),
                    value: p.value.clone(),
                    decorators: p.decorators.iter().map(|d| d.span).collect(),
                }),
                _ => None,
            })
            .collect();
        let ctor = class.body.iter().find_map(|m| match m {
            ClassMember::Constructor(k) => Some(k),
            _ => None,
        });
        let derived = class.super_class.is_some();
        self.instructions.push(OpCode::Load(wrapper.clone()));
        match ctor {
            Some(k) => {
                let owned: Vec<Pat> = k.params.iter().map(scope::ctor_param_pat).collect();
                let pats: Vec<&Pat> = owned.iter().collect();
                // `constructor(private x)` assigns `this.x = x` first, like
                // a field initialized from the parameter.
                let mut ctor_fields: Vec<ClassField> = k
                    .params
                    .iter()
                    .filter_map(scope::param_prop_ident)
                    .map(|id| ClassField {
                        key: id.sym.to_string(),
                        value: Some(Box::new(Expr::Ident(id))),
                        decorators: Vec::new(),
                    })
                    .collect();
                ctor_fields.extend(fields.iter().cloned());
                let stmts: &[Stmt] = k.body.as_ref().map_or(&[], |b| b.stmts.as_slice());
                let kind = FnKind::Ctor {
                    derived,
                    implicit: false,
                };
                self.next_fn_name = Some(format!("new {}", self.class_display_name()));
                self.gen_member_function(k.span, &pats, stmts, false, kind, &ctor_fields);
            }
            None => {
                let kind = FnKind::Ctor {
                    derived,
                    implicit: true,
                };
                let span = scope::implicit_ctor_span(class.span);
                self.next_fn_name = Some(format!("new {}", self.class_display_name()));
                self.gen_member_function(span, &[], &[], false, kind, &fields);
            }
        }
        self.instructions
            .push(OpCode::SetProp("constructor".to_string()));

        // Methods (on the class for static ones, which are decorated first,
        // as TypeScript does; then on the prototype), then fields.
        for static_pass in [true, false] {
            for member in &class.body {
                let (key, function, is_static, kind, shown, private) = match member {
                    ClassMember::Method(m) => {
                        let Some(name) = self.class_key(&m.key) else {
                            continue;
                        };
                        let key = match m.kind {
                            MethodKind::Getter => format!("getter:{}", name),
                            MethodKind::Setter => format!("setter:{}", name),
                            MethodKind::Method => name.clone(),
                        };
                        (key, &m.function, m.is_static, m.kind, name, false)
                    }
                    ClassMember::PrivateMethod(m) => {
                        let name = self.private_key(&m.key.name);
                        let key = match m.kind {
                            MethodKind::Getter => format!("getter:{}", name),
                            MethodKind::Setter => format!("setter:{}", name),
                            MethodKind::Method => name,
                        };
                        let shown = format!("#{}", m.key.name);
                        (key, &m.function, m.is_static, m.kind, shown, true)
                    }
                    _ => continue,
                };
                if is_static != static_pass {
                    continue;
                }
                if function.is_generator {
                    self.unsupported("generator methods");
                }
                let target = if is_static { &wrapper } else { &proto };
                self.instructions.push(OpCode::Load(target.clone()));
                let pats: Vec<&Pat> = function.params.iter().map(|p| &p.pat).collect();
                let stmts: &[Stmt] = function.body.as_ref().map_or(&[], |b| b.stmts.as_slice());
                self.next_fn_name = Some(format!("{}.{}", self.class_display_name(), shown));
                self.gen_member_function(
                    function.span,
                    &pats,
                    stmts,
                    function.is_async,
                    FnKind::Method { is_static },
                    &[],
                );
                let kind = match kind {
                    MethodKind::Getter => "getter",
                    MethodKind::Setter => "setter",
                    MethodKind::Method => "method",
                };
                self.emit_decorators(&function.decorators, kind, Some(&shown), is_static, private);
                self.instructions.push(OpCode::SetProp(key));
            }
        }
        // What each field decorator returns: an initializer for every
        // instance's value, kept in the class scope for the constructor.
        for member in &class.body {
            let (decorators, name, private) = match member {
                ClassMember::ClassProp(p) if !p.is_static => {
                    (&p.decorators, self.class_key(&p.key), false)
                }
                ClassMember::PrivateProp(p) if !p.is_static => {
                    (&p.decorators, Some(format!("#{}", p.key.name)), true)
                }
                _ => continue,
            };
            for d in decorators {
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.emit_decorator_context("field", name.as_deref(), false, private);
                self.gen_expr(&d.expr);
                self.instructions.push(OpCode::Call(2));
                self.store_var(&scope::field_init_ident(d.span), true);
            }
        }

        for member in &class.body {
            let (key, value, decorators, shown, private) = match member {
                ClassMember::ClassProp(p) if p.is_static => match self.class_key(&p.key) {
                    Some(k) => (k.clone(), &p.value, &p.decorators, k, false),
                    None => continue,
                },
                ClassMember::PrivateProp(p) if p.is_static => (
                    self.private_key(&p.key.name),
                    &p.value,
                    &p.decorators,
                    format!("#{}", p.key.name),
                    true,
                ),
                _ => continue,
            };
            self.instructions.push(OpCode::Load(wrapper.clone()));
            match value {
                Some(v) => self.gen_expr(v),
                None => self.instructions.push(OpCode::Push(JsValue::Undefined)),
            }
            // [C, v] -> each decorator's initializer, called with `this = C`
            for d in decorators.iter() {
                self.instructions.push(OpCode::Push(JsValue::Undefined));
                self.emit_decorator_context("field", Some(&shown), true, private);
                self.gen_expr(&d.expr);
                self.instructions.push(OpCode::Call(2));
                self.instructions.push(OpCode::Load(wrapper.clone()));
                self.emit_call_initializer();
            }
            self.instructions.push(OpCode::SetProp(key));
        }

        self.instructions.push(OpCode::Load(wrapper));
        self.exit_scope_env(class_env);
        self.private_names.pop();

        // Class decorators: `C = decorator(C, context) ?? C`.
        self.emit_decorators(&class.decorators, "class", name, false, false);
    }

    /// The class being compiled, as stack traces show it.
    fn class_display_name(&self) -> String {
        self.class_name
            .clone()
            .unwrap_or_else(|| "<anonymous>".to_string())
    }

    /// The name of a class member's key; computed keys are unsupported.
    fn class_key(&mut self, key: &PropName) -> Option<String> {
        match key {
            PropName::Ident(id) => Some(id.sym.to_string()),
            PropName::Str(s) => Some(s.value.to_string_lossy().into_owned()),
            PropName::Num(n) => Some(crate::stdlib::number_to_string(n.value)),
            _ => {
                self.unsupported("computed class member names");
                None
            }
        }
    }

    /// `[] -> [function]` for a constructor or method.
    fn gen_member_function(
        &mut self,
        span: Span,
        pats: &[&Pat],
        stmts: &[Stmt],
        is_async: bool,
        kind: FnKind,
        fields: &[ClassField],
    ) {
        let value_idx = self.emit_function_value(span, 0);
        let jump_idx = self.emit_jump();
        let body_start = self.instructions.len();
        self.patch_function_value(value_idx, body_start);
        self.gen_function_body(span, pats, stmts, is_async, kind, fields);
        self.patch_jump_here(jump_idx);
    }
}
