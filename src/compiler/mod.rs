use crate::vm::opcodes::OpCode;
use std::collections::HashMap;
use swc_ecma_ast::*;
mod bindings;
pub mod borrow_ck;
mod classes;
mod control;
mod decls;
pub mod diagnostic;
mod env;
mod expr;
mod functions;
pub mod modules;
pub mod scope;
mod stmt;
use crate::compiler::borrow_ck::BorrowChecker;
use crate::compiler::modules::ModuleLink;
use crate::compiler::scope::{PARENT_PROP, SCOPE_LOCAL, ScopeInfo, VarRef};
use crate::runtime::intrinsics;
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
    /// What the type checker found in the last module compiled.
    pub type_diagnostics: Vec<Diagnostic>,
    /// What the type checker knows about the modules compiled so far: a
    /// module sees the types of what it imports.
    pub type_world: crate::types::check::TypeWorld,
    /// What the type checker found in each file of the last program
    /// compiled (`compile_program`).
    pub warnings: Vec<FileWarnings>,
}

/// What the type checker found in one file. For now these are warnings:
/// they do not stop compilation (docs/docs/types.md, "Rollout").
pub struct FileWarnings {
    pub file: String,
    pub source: String,
    pub diagnostics: Vec<Diagnostic>,
    /// The file is not part of the program being compiled, but something
    /// it imports from elsewhere (a roll).
    pub dependency: bool,
}

impl FileWarnings {
    /// The findings as `file:line:col: <kind>: message`, with the source
    /// line.
    pub fn render(&self, kind: &'static str) -> String {
        let diagnostics = self
            .diagnostics
            .iter()
            .map(|d| Diagnostic { kind, ..d.clone() })
            .collect();
        CompileError::new(diagnostics).render(&self.file, &self.source)
    }
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
            type_diagnostics: Vec::new(),
            type_world: Default::default(),
            warnings: Vec::new(),
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

        self.type_diagnostics =
            crate::types::check::check_program(program, &mut self.type_world, link.as_ref())
                .into_iter()
                .map(|d| Diagnostic::new("warning", d.message, position(d.span)))
                .collect();

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
}
