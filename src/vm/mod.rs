/// Maximum call stack depth to prevent stack overflow in deeply recursive code
pub const MAX_CALL_STACK_DEPTH: usize = 1000;

pub mod errors;
pub mod module_cache;
pub mod opcodes;
pub mod promise;
pub mod property;
pub mod stdlib_setup;
pub mod trace;
pub mod value;

pub use crate::compiler::Compiler;
pub use crate::vm::module_cache::CachedModule;
pub use crate::vm::module_cache::ModuleCache;
pub use crate::vm::opcodes::OpCode;
pub use crate::vm::value::HeapData;
pub use crate::vm::value::HeapObject;
pub use crate::vm::value::JsValue;
pub use crate::vm::value::NativeFn;
pub use crate::vm::value::Promise;
pub use crate::vm::value::PromiseState;
pub use sha2::Digest;
pub use std::collections::{HashMap, VecDeque};
pub use std::fs;
pub use std::path::{Path, PathBuf};
pub use std::time::{Duration, Instant};
pub use swc_common::{FileName, input::StringInput};
pub use swc_ecma_parser::{Parser, Syntax, TsSyntax, lexer::Lexer};
pub use tokio::runtime::Runtime;
pub use tokio::sync::mpsc;

/// Parse module source and extract exports as a HashMap
fn parse_module_exports(source: &str, file_name: &str) -> HashMap<String, JsValue> {
    let mut exports = HashMap::new();

    let cm: swc_common::SourceMap = Default::default();
    let fm = cm.new_source_file(
        FileName::Custom(file_name.to_string()).into(),
        source.to_string(),
    );

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            decorators: true,
            tsx: false,
            dts: false,
            no_early_errors: false,
            disallow_ambiguous_jsx_like: true,
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );

    let mut parser = Parser::new_from(lexer);

    match parser.parse_module() {
        Ok(ast) => {
            for item in &ast.body {
                if let swc_ecma_ast::ModuleItem::ModuleDecl(decl) = item {
                    match decl {
                        swc_ecma_ast::ModuleDecl::ExportNamed(named) => {
                            if let Some(_src) = &named.src {
                                for spec in &named.specifiers {
                                    match spec {
                                        swc_ecma_ast::ExportSpecifier::Named(named) => {
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
                                            exports.insert(export_name, JsValue::Undefined);
                                        }
                                        swc_ecma_ast::ExportSpecifier::Default(_) => {
                                            exports
                                                .insert("default".to_string(), JsValue::Undefined);
                                        }
                                        swc_ecma_ast::ExportSpecifier::Namespace(ns) => {
                                            let atom = ns.name.atom();
                                            let s: &str = &atom;
                                            exports.insert(s.to_string(), JsValue::Undefined);
                                        }
                                    }
                                }
                            } else {
                                for spec in &named.specifiers {
                                    match spec {
                                        swc_ecma_ast::ExportSpecifier::Named(named) => {
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
                                            exports.insert(export_name, JsValue::Undefined);
                                        }
                                        swc_ecma_ast::ExportSpecifier::Default(_) => {
                                            exports
                                                .insert("default".to_string(), JsValue::Undefined);
                                        }
                                        swc_ecma_ast::ExportSpecifier::Namespace(ns) => {
                                            let atom = ns.name.atom();
                                            let s: &str = &atom;
                                            exports.insert(s.to_string(), JsValue::Undefined);
                                        }
                                    }
                                }
                            }
                        }
                        swc_ecma_ast::ModuleDecl::ExportAll(_all) => {
                            exports.insert("*".to_string(), JsValue::Undefined);
                        }
                        swc_ecma_ast::ModuleDecl::ExportDefaultDecl(_default_decl) => {
                            exports.insert("default".to_string(), JsValue::Undefined);
                        }
                        swc_ecma_ast::ModuleDecl::ExportDefaultExpr(_) => {
                            exports.insert("default".to_string(), JsValue::Undefined);
                        }
                        swc_ecma_ast::ModuleDecl::ExportDecl(decl) => {
                            use swc_ecma_ast::Decl::*;
                            match &decl.decl {
                                Fn(fn_decl) => {
                                    exports
                                        .insert(fn_decl.ident.sym.to_string(), JsValue::Undefined);
                                }
                                Var(var_decl) => {
                                    for declarator in &var_decl.decls {
                                        if let swc_ecma_ast::Pat::Ident(ident) = &declarator.name {
                                            exports.insert(
                                                ident.id.sym.to_string(),
                                                JsValue::Undefined,
                                            );
                                        }
                                    }
                                }
                                Class(class_decl) => {
                                    exports.insert(
                                        class_decl.ident.sym.to_string(),
                                        JsValue::Undefined,
                                    );
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                } else if let swc_ecma_ast::ModuleItem::Stmt(stmt) = item
                    && let swc_ecma_ast::Stmt::Decl(decl) = stmt
                {
                    match decl {
                        swc_ecma_ast::Decl::Var(var_decl) => {
                            for declarator in &var_decl.decls {
                                if let swc_ecma_ast::Pat::Ident(ident) = &declarator.name {
                                    exports.insert(ident.id.sym.to_string(), JsValue::Undefined);
                                }
                            }
                        }
                        swc_ecma_ast::Decl::Fn(fn_decl) => {
                            exports.insert(fn_decl.ident.sym.to_string(), JsValue::Undefined);
                        }
                        swc_ecma_ast::Decl::Class(class_decl) => {
                            exports.insert(class_decl.ident.sym.to_string(), JsValue::Undefined);
                        }
                        _ => {}
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to parse module for exports: {:?}", e);
        }
    }

    exports
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub return_address: usize,
    pub locals: HashMap<String, JsValue>,
    pub indexed_locals: Vec<JsValue>,
    pub this_context: JsValue,
    /// Stores the constructor that was called with new (for new.target)
    pub new_target: Option<JsValue>,
    /// Tracks whether super() has been called in a derived class constructor
    /// JavaScript requires super() to be called before accessing `this`
    pub super_called: bool,
    /// For async functions: where to resume after await
    pub resume_ip: Option<usize>,
    /// Captured-environment heap object for closures. Load/Store resolve
    /// through this pointer so mutations to captured variables are shared
    /// across calls and visible to every reference to the same closure.
    pub env: Option<usize>,
}

pub struct Task {
    pub function_ptr: JsValue,
    pub args: Vec<JsValue>,
}

pub struct TimerTask {
    id: usize,
    due: Instant,
    task: Task,
}

/// Exception handler entry for try/catch blocks
#[derive(Clone)]
pub struct ExceptionHandler {
    /// Address of catch block (0 = no catch)
    pub catch_addr: usize,
    /// Address of finally block (0 = no finally)
    pub finally_addr: usize,
    /// Stack depth when try block was entered (for unwinding)
    pub stack_depth: usize,
    /// Call stack depth when try block was entered
    pub call_stack_depth: usize,
}

pub struct VM {
    pub stack: Vec<JsValue>,
    pub call_stack: Vec<Frame>,
    pub heap: Vec<HeapObject>,
    pub native_functions: Vec<NativeFn>,
    pub task_queue: VecDeque<Task>,
    timers: Vec<TimerTask>,
    pub program: Vec<OpCode>,
    pub modules: HashMap<String, JsValue>,
    pub ip: usize,
    pub function_call_counts: HashMap<usize, u64>,
    pub total_instructions: u64,
    pub exception_handlers: Vec<ExceptionHandler>,
    pub current_exception: Option<JsValue>,
    /// An exception that reached a `call_sync` barrier.
    barrier_exception: Option<JsValue>,
    /// An exception a native function throws: set instead of returning,
    /// thrown by the VM when the function returns.
    pub native_throw: Option<JsValue>,
    /// Report an uncaught exception and exit (the `oitec` binary) instead
    /// of panicking (embedded VMs, tests).
    pub exit_on_uncaught: bool,
    /// Source positions of the loaded programs (see `trace.rs`).
    pub sources: crate::compiler::SourceTable,
    /// Where the last exception thrown has been: instruction pointers.
    exception_trace: Vec<usize>,
    traced_exception: Option<JsValue>,
    pub current_module_path: Option<PathBuf>,
    pub async_runtime: Option<Runtime>,
    pub async_task_tx: Option<mpsc::Sender<JsValue>>,
    pub module_cache: ModuleCache,
    pub compiler: Compiler,
    /// Async/await continuation state
    /// Promise reactions waiting to run.
    pub microtasks: std::collections::VecDeque<promise::Microtask>,
    /// Async functions waiting for a promise, by id.
    pub suspended: HashMap<usize, promise::Suspended>,
    /// `Promise.all` calls in progress, by id.
    pub promise_alls: HashMap<usize, promise::AllState>,
    pub next_async_id: usize,
    /// Rejected promises nothing reacted to (yet).
    pub unhandled_rejections: Vec<Promise>,
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

impl VM {
    pub fn new() -> Self {
        let mut vm = Self::new_bare();
        vm.setup_stdlib();
        vm
    }

    /// Create a new VM without stdlib (for benchmarking).
    pub fn new_bare() -> Self {
        let (tx, _) = mpsc::channel(100);
        Self {
            stack: Vec::new(),
            call_stack: vec![Frame {
                return_address: usize::MAX, // Set to MAX so global return stops execution
                locals: HashMap::new(),
                indexed_locals: Vec::new(),
                this_context: JsValue::Undefined,
                new_target: None,
                super_called: false,
                resume_ip: None,
                env: None,
            }],
            heap: Vec::new(),
            native_functions: Vec::new(),
            task_queue: VecDeque::new(),
            timers: Vec::new(),
            program: Vec::new(),
            modules: HashMap::new(),
            ip: 0,
            function_call_counts: HashMap::new(),
            total_instructions: 0,
            exception_handlers: Vec::new(),
            current_exception: None,
            barrier_exception: None,
            native_throw: None,
            exit_on_uncaught: false,
            sources: Default::default(),
            exception_trace: Vec::new(),
            traced_exception: None,
            current_module_path: None,
            async_runtime: None,
            async_task_tx: Some(tx),
            module_cache: ModuleCache::new(),
            compiler: Compiler::new(),
            microtasks: std::collections::VecDeque::new(),
            suspended: HashMap::new(),
            promise_alls: HashMap::new(),
            next_async_id: 0,
            unhandled_rejections: Vec::new(),
        }
    }

    /// Initialize the async runtime
    pub fn init_async(&mut self) {
        self.async_runtime = Some(Runtime::new().expect("Failed to create async runtime"));
    }

    /// Record a function call for profiling/tiered compilation.
    pub fn record_function_call(&mut self, func_addr: usize) {
        *self.function_call_counts.entry(func_addr).or_insert(0) += 1;
    }

    /// Get the call count for a function.
    pub fn get_call_count(&self, func_addr: usize) -> u64 {
        self.function_call_counts
            .get(&func_addr)
            .copied()
            .unwrap_or(0)
    }

    /// Get all function call counts (for identifying hot functions).
    pub fn get_hot_functions(&self, threshold: u64) -> Vec<(usize, u64)> {
        self.function_call_counts
            .iter()
            .filter(|&(_, &count)| count >= threshold)
            .map(|(&addr, &count)| (addr, count))
            .collect()
    }

    /// Reset profiling counters.
    pub fn reset_counters(&mut self) {
        self.function_call_counts.clear();
        self.total_instructions = 0;
    }

    /// Invalidate a specific module in the cache
    pub fn invalidate_module(&mut self, path: &PathBuf) {
        self.module_cache.invalidate(path);
    }

    /// Invalidate all cached modules (for hot reload)
    pub fn invalidate_all_modules(&mut self) {
        self.module_cache.invalidate_all();
    }

    /// Check if a module needs reloading
    pub fn module_needs_reload(&self, path: &PathBuf) -> bool {
        self.module_cache.should_reload(path)
    }

    /// Get the number of cached modules
    pub fn cached_modules_count(&self) -> usize {
        self.module_cache.len()
    }

    /// Get list of cached module paths (for debugging)
    pub fn cached_modules(&self) -> Vec<String> {
        self.module_cache
            .entries()
            .values()
            .map(|m| m.path.to_string_lossy().into_owned())
            .collect()
    }

    /// Get cache size in bytes
    pub fn cache_size(&self) -> usize {
        self.module_cache.cache_size_bytes()
    }

    /// Clear all cached modules
    pub fn clear_module_cache(&mut self) {
        self.module_cache.invalidate_all();
    }

    /// Check if a module is cached
    pub fn is_module_cached(&self, path: &PathBuf) -> bool {
        self.module_cache.get(path).is_some()
    }

    /// Get cache info for a specific module
    pub fn get_module_cache_info(&self, path: &PathBuf) -> Option<(String, String)> {
        self.module_cache.get(path).map(|cached| {
            (
                cached
                    .load_time
                    .elapsed()
                    .map(|d| format!("{:?}", d))
                    .unwrap_or_else(|_| "unknown".to_string()),
                cached.hash.clone(),
            )
        })
    }

    /// Compile and execute a module source, returning the export values
    /// This is used by the ImportAsync handler to actually run imported modules
    pub fn execute_module(
        &mut self,
        source: &str,
        path: &Path,
        export_names: &[String],
    ) -> Result<HashMap<String, JsValue>, String> {
        let syntax = if path.to_string_lossy().ends_with(".ts")
            || path.to_string_lossy().ends_with(".tsx")
        {
            Some(Syntax::Typescript(TsSyntax {
                decorators: true,
                tsx: path.to_string_lossy().ends_with(".tsx"),
                ..Default::default()
            }))
        } else if path.to_string_lossy().ends_with(".js")
            || path.to_string_lossy().ends_with(".jsx")
        {
            Some(Syntax::Es(Default::default()))
        } else {
            Some(Syntax::Typescript(TsSyntax {
                decorators: true,
                ..Default::default()
            }))
        };

        let bytecode = self
            .compiler
            .compile_with_syntax(source, syntax)
            .map_err(|e| e.render(&path.display().to_string(), source))?;

        // Save IP BEFORE appending program, because append_program modifies IP
        let saved_ip = self.ip;
        let saved_module_path = self.current_module_path.clone();
        // Save stack to prevent module execution from corrupting caller's stack
        let saved_stack = self.stack.clone();

        let start_offset = self.append_program(bytecode);
        let end_offset = self.program.len();
        let debug = std::mem::take(&mut self.compiler.debug_info);
        let file = path.display().to_string();
        self.add_debug_info(start_offset, &file, source, debug);

        self.current_module_path = Some(path.to_path_buf());
        self.ip = start_offset;

        // Execute only the module's bytecode, not the entire program
        // We run until we hit the module's Halt or reach the end of module bytecode
        loop {
            if self.ip >= end_offset {
                break;
            }
            if self.ip >= self.program.len() {
                break;
            }
            let result = self.exec_one();
            // Don't stop the entire VM on module Halt - just break from module execution
            if result == ExecResult::Stop {
                // Check if this is a Halt within the module
                if self.ip > 0 && self.ip <= end_offset + 5 {
                    // This is likely the module's Halt, break normally
                    break;
                }
                // Otherwise, it's a real stop
                break;
            }
        }

        self.ip = saved_ip;
        self.current_module_path = saved_module_path;
        // Restore stack to prevent module execution from corrupting caller's stack
        self.stack = saved_stack;

        let mut exports = HashMap::new();
        let global_locals = &self.call_stack[0].locals;

        for name in export_names {
            if let Some(value) = global_locals.get(name) {
                exports.insert(name.clone(), value.clone());
            } else {
                exports.insert(name.clone(), JsValue::Undefined);
            }
        }

        Ok(exports)
    }

    pub fn setup_stdlib(&mut self) {
        stdlib_setup::setup_stdlib(self);
    }

    /// Set script command-line arguments as __args__ global variable.
    pub fn set_script_args(&mut self, args: Vec<String>) {
        stdlib_setup::set_script_args(self, args);
    }

    pub fn register_native(&mut self, func: NativeFn) -> usize {
        let idx = self.native_functions.len();
        self.native_functions.push(func);
        idx
    }

    /// `setTimeout(callback, delay_ms)`; returns the timer's id.
    pub fn schedule_timer(&mut self, callback: JsValue, delay_ms: u64) -> usize {
        let id = self.next_async_id;
        self.next_async_id += 1;
        self.timers.push(TimerTask {
            id,
            due: Instant::now() + Duration::from_millis(delay_ms),
            task: Task {
                function_ptr: callback,
                args: vec![],
            },
        });
        id
    }

    /// `clearTimeout(id)`
    pub fn cancel_timer(&mut self, id: usize) {
        self.timers.retain(|t| t.id != id);
    }

    pub fn load_program(&mut self, bytecode: Vec<OpCode>) {
        self.program = bytecode;
        self.ip = 0;
        self.current_module_path = None;
    }

    pub fn load_program_with_path(&mut self, bytecode: Vec<OpCode>, path: PathBuf) {
        self.program = bytecode;
        self.ip = 0;
        self.current_module_path = Some(path);
    }

    /// Update the current module path (for relative imports)
    pub fn set_current_module_path(&mut self, path: PathBuf) {
        self.current_module_path = Some(path);
    }

    /// Append bytecode to the existing program and return the starting offset.
    /// This rebases all address-containing instructions so they point to the correct
    /// locations in the combined program.
    pub fn append_program(&mut self, bytecode: Vec<OpCode>) -> usize {
        let start_offset = self.program.len();
        self.program.extend(rebase_program(bytecode, start_offset));
        self.ip = start_offset;
        start_offset
    }

    pub fn run_event_loop(&mut self) {
        // 1) Run the initial script to completion, then its microtasks.
        self.run_until_halt();
        self.run_microtasks();

        // 2) Drain the event loop: timers -> task queue -> execute task,
        //    each followed by the microtasks it queued.
        loop {
            self.pump_timers();

            if let Some(task) = self.task_queue.pop_front() {
                self.execute_task(task);
                self.run_microtasks();
                continue;
            }

            // No immediate tasks left.
            if self.timers.is_empty() {
                break;
            }

            // Timers exist but none ready: sleep until the next one is due.
            if let Some(next_due) = self.next_timer_due() {
                let now = Instant::now();
                if next_due > now {
                    std::thread::sleep(next_due - now);
                }
            } else {
                // This shouldn't happen if timers is not empty, but handle it anyway
                break;
            }
        }

        // 3) A rejection nothing handled is reported like an uncaught
        //    exception.
        self.report_unhandled_rejection();
    }

    fn next_timer_due(&self) -> Option<Instant> {
        self.timers.iter().map(|t| t.due).min()
    }

    fn pump_timers(&mut self) {
        let now = Instant::now();
        // Move all due timers into the task queue.
        let mut i = 0;
        while i < self.timers.len() {
            if self.timers[i].due <= now {
                let timer = self.timers.remove(i);
                self.task_queue.push_back(timer.task);
            } else {
                i += 1;
            }
        }
    }

    // Property helpers moved to property.rs
    // Re-exporting for backward compatibility
    pub fn get_prop_with_proto_chain(&self, obj_ptr: usize, name: &str) -> JsValue {
        crate::vm::property::get_prop_with_proto_chain(self, obj_ptr, name)
    }

    pub fn find_setter_with_proto_chain(
        &self,
        obj_ptr: usize,
        name: &str,
    ) -> Option<(usize, Option<usize>)> {
        crate::vm::property::find_setter_with_proto_chain(self, obj_ptr, name)
    }

    /// Run a task (a timer's callback). An exception it throws is
    /// uncaught.
    fn execute_task(&mut self, task: Task) {
        if let Err(e) = self.call_value(&task.function_ptr, JsValue::Undefined, task.args) {
            self.report_uncaught(&e);
        }
    }

    /// Run `f(args)` with `this` to completion, from inside an instruction
    /// (implicit conversions call a user's `valueOf` / `toString`). An
    /// exception it throws comes back as `Err`, for the caller to throw.
    pub(crate) fn call_sync(
        &mut self,
        f: &JsValue,
        this: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let (address, env) = match f {
            JsValue::Function { address, env } => (*address, *env),
            JsValue::NativeFunction(idx) => {
                let func = self.native_functions[*idx];
                let result = func(self, args);
                return match self.native_throw.take() {
                    Some(e) => Err(e),
                    None => Ok(result),
                };
            }
            _ => return Ok(JsValue::Undefined),
        };
        if self.call_stack.len() >= MAX_CALL_STACK_DEPTH {
            return Err(self.make_error("RangeError", "Maximum call stack size exceeded".into()));
        }
        let saved_ip = self.ip;
        let handlers = self.exception_handlers.len();
        self.exception_handlers.push(ExceptionHandler {
            catch_addr: CALL_SYNC_BARRIER,
            finally_addr: 0,
            stack_depth: self.stack.len(),
            call_stack_depth: self.call_stack.len(),
        });
        self.push_call_args(&args, address);
        self.call_stack.push(Frame {
            return_address: usize::MAX,
            locals: HashMap::new(),
            indexed_locals: Vec::new(),
            this_context: this,
            new_target: None,
            super_called: false,
            resume_ip: None,
            env,
        });
        self.ip = address;
        self.run_until_return_sentinel();
        self.ip = saved_ip;
        if let Some(exception) = self.barrier_exception.take() {
            return Err(exception);
        }
        self.exception_handlers.truncate(handlers);
        Ok(self.stack.pop().unwrap_or(JsValue::Undefined))
    }

    /// The two operands of `-`, `*`, `/` or `%`, as numbers (`ToNumber`,
    /// through `valueOf` for an object). [a, b] -> []
    fn numeric_operands(&mut self) -> Result<(f64, f64), JsValue> {
        let b = self.stack.pop().unwrap_or(JsValue::Undefined);
        let a = self.stack.pop().unwrap_or(JsValue::Undefined);
        let mut num = |v: JsValue| -> Result<f64, JsValue> {
            if let JsValue::Number(n) = v {
                return Ok(n);
            }
            let p = self.to_primitive(&v, false)?.unwrap_or(v);
            Ok(match p {
                JsValue::Object(_) => {
                    crate::vm::errors::to_number(&JsValue::String(self.to_js_string(&p)))
                }
                other => crate::vm::errors::to_number(&other),
            })
        };
        let a = num(a)?;
        let b = num(b)?;
        Ok((a, b))
    }

    /// How the operands of `<`, `<=`, `>` or `>=` compare: as strings when
    /// both are strings after `ToPrimitive`, else as numbers (`None` when
    /// one is NaN). [a, b] -> []
    fn compare_operands(&mut self) -> Result<Option<std::cmp::Ordering>, JsValue> {
        let b = self.stack.pop().unwrap_or(JsValue::Undefined);
        let a = self.stack.pop().unwrap_or(JsValue::Undefined);
        if let (JsValue::Number(x), JsValue::Number(y)) = (&a, &b) {
            return Ok(x.partial_cmp(y));
        }
        let a = self.to_primitive(&a, false)?.unwrap_or(a);
        let b = self.to_primitive(&b, false)?.unwrap_or(b);
        if let (JsValue::String(x), JsValue::String(y)) = (&a, &b) {
            return Ok(Some(x.cmp(y)));
        }
        let num = |vm: &Self, v: &JsValue| match v {
            JsValue::Object(_) => {
                crate::vm::errors::to_number(&JsValue::String(vm.to_js_string(v)))
            }
            other => crate::vm::errors::to_number(other),
        };
        Ok(num(self, &a).partial_cmp(&num(self, &b)))
    }

    /// JavaScript `ToPrimitive` for an object with a user-defined
    /// `valueOf` or `toString` (string hint: `toString` first). `None`: no
    /// such method, so the value converts as before.
    pub(crate) fn to_primitive(
        &mut self,
        v: &JsValue,
        prefer_string: bool,
    ) -> Result<Option<JsValue>, JsValue> {
        let JsValue::Object(ptr) = v else {
            return Ok(None);
        };
        if !matches!(
            self.heap.get(*ptr).map(|o| &o.data),
            Some(HeapData::Object(_))
        ) {
            return Ok(None);
        }
        let order = if prefer_string {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for name in order {
            let method = self.get_prop_with_proto_chain(*ptr, name);
            if matches!(method, JsValue::Function { .. }) {
                let result = self.call_sync(&method, v.clone(), Vec::new())?;
                if !matches!(result, JsValue::Object(_)) {
                    return Ok(Some(result));
                }
            }
        }
        Ok(None)
    }

    fn run_until_return_sentinel(&mut self) {
        // Runs until the current frame returns to usize::MAX.
        loop {
            if self.ip >= self.program.len() {
                break;
            }
            if self.ip == usize::MAX {
                break;
            }
            if self.exec_one() == ExecResult::Stop {
                break;
            }
        }
    }

    pub fn run_until_halt(&mut self) {
        loop {
            if self.ip >= self.program.len() {
                break;
            }
            if self.exec_one() == ExecResult::Stop {
                break;
            }
        }
    }

    /// Unwind to the innermost exception handler and transfer control to its
    /// catch or finally block. Panics if no handler is installed. Every
    /// handled path sets `self.ip` and returns `ContinueNoIpInc`, so callers
    /// in `exec_one` must return the result as-is.
    fn throw_exception(&mut self, exception: JsValue) -> ExecResult {
        let catch_depth = self
            .exception_handlers
            .last()
            .map_or(1, |h| h.call_stack_depth);
        self.record_trace(&exception, catch_depth);
        if let Some(handler) = self.exception_handlers.pop() {
            // Unwind the stack to the handler's saved state
            self.stack.truncate(handler.stack_depth);

            // Unwind call stack if needed
            while self.call_stack.len() > handler.call_stack_depth {
                self.call_stack.pop();
            }

            // The barrier of a `call_sync`: hand the exception back to it.
            if handler.catch_addr == CALL_SYNC_BARRIER {
                self.barrier_exception = Some(exception);
                self.ip = usize::MAX;
                return ExecResult::Stop;
            }

            if handler.catch_addr != 0 {
                // We have a catch block - push exception and jump there
                self.stack.push(exception);
                self.ip = handler.catch_addr;

                // If there's a finally, we need to remember to run it
                // after the catch completes
                if handler.finally_addr != 0 {
                    // Re-push a handler for finally (catch_addr=0 means no catch, just finally)
                    self.exception_handlers.push(ExceptionHandler {
                        catch_addr: 0,
                        finally_addr: handler.finally_addr,
                        stack_depth: self.stack.len() - 1, // Exclude the exception value
                        call_stack_depth: handler.call_stack_depth,
                    });
                }
                return ExecResult::ContinueNoIpInc;
            } else if handler.finally_addr != 0 {
                // No catch, but there's a finally block. Store the exception;
                // the EnterFinally(true) at the end of the finally block
                // re-throws it once the finally has run.
                self.current_exception = Some(exception);
                self.ip = handler.finally_addr;
                return ExecResult::ContinueNoIpInc;
            }
        }

        self.report_uncaught(&exception)
    }

    /// Pop `n` call arguments, in call order.
    fn pop_args(&mut self, n: usize) -> Vec<JsValue> {
        let mut args: Vec<JsValue> = (0..n)
            .map(|_| self.stack.pop().unwrap_or(JsValue::Undefined))
            .collect();
        args.reverse();
        args
    }

    /// Push a shared built-in method's result, or throw its error. `None`
    /// means there is no such method.
    fn finish_builtin(
        &mut self,
        result: Option<
            Result<crate::runtime::builtins::Outcome<JsValue>, crate::runtime::builtins::Error>,
        >,
        receiver: JsValue,
        name: &str,
    ) -> ExecResult {
        use crate::runtime::builtins::Outcome;
        match result {
            Some(Ok(Outcome::Value(v))) => self.stack.push(v),
            Some(Ok(Outcome::This)) => self.stack.push(receiver),
            Some(Ok(Outcome::NewArray(elems))) => {
                let ptr = self.heap.len();
                self.heap.push(HeapObject {
                    data: HeapData::Array(elems),
                });
                self.stack.push(JsValue::Object(ptr));
            }
            Some(Err(e)) => return self.throw_error(e.kind, e.message),
            None => {
                let shown = crate::stdlib::inspect(self, &receiver);
                return self
                    .throw_error("TypeError", format!("{}.{} is not a function", shown, name));
            }
        }
        self.ip += 1;
        ExecResult::Continue
    }

    /// `super(...args)`: run the parent constructor on the current `this`.
    /// Its frame's `new_target` is the parent class, so a `super(...)` inside
    /// it reaches the grandparent.
    fn call_super(&mut self, super_ctor: JsValue, args: Vec<JsValue>) -> ExecResult {
        // super() must be called before accessing `this`
        if let Some(frame) = self.call_stack.last_mut() {
            frame.super_called = true;
        }
        let ctor_fn = match &super_ctor {
            JsValue::Function { .. } => super_ctor.clone(),
            JsValue::Object(ptr) => match self.heap.get(*ptr) {
                Some(HeapObject {
                    data: HeapData::Object(props),
                }) => props
                    .get("constructor")
                    .cloned()
                    .unwrap_or(JsValue::Undefined),
                _ => JsValue::Undefined,
            },
            _ => JsValue::Undefined,
        };
        let JsValue::Function { address, env } = ctor_fn else {
            return self.throw_error(
                "TypeError",
                "Super constructor is not a constructor".to_string(),
            );
        };
        let this_context = self
            .call_stack
            .last()
            .map(|f| f.this_context.clone())
            .unwrap_or(JsValue::Undefined);
        self.push_call_args(&args, address);
        let new_target = match super_ctor {
            JsValue::Object(_) => Some(super_ctor),
            _ => None,
        };
        self.call_stack.push(Frame {
            return_address: self.ip + 1,
            locals: HashMap::new(),
            indexed_locals: Vec::new(),
            this_context,
            new_target,
            super_called: false,
            resume_ip: None,
            env,
        });
        self.ip = address;
        ExecResult::ContinueNoIpInc
    }

    /// Number of parameters of the bytecode function at `address`: the
    /// `Let`s that open its body bind them, one per parameter.
    fn param_count(&self, address: usize) -> usize {
        self.program[address..]
            .iter()
            .take_while(|op| matches!(op, OpCode::Let(_)))
            .count()
    }

    /// Push call arguments for the function at `address`, matched to its
    /// parameter count: missing arguments are `undefined`, extra ones are
    /// dropped. Parameters are bound by popping the stack, so pushing the
    /// arguments as given shifted every parameter whenever the counts
    /// differed (`f(1)` against `function f(a, b)` bound `b = 1`).
    fn push_call_args(&mut self, args: &[JsValue], address: usize) {
        match self.program.get(address) {
            Some(OpCode::ForwardArgs) => {
                self.stack.extend(args.iter().cloned());
                self.stack.push(JsValue::Number(args.len() as f64));
                return;
            }
            Some(&OpCode::RestArgs(n)) => {
                for i in 0..n {
                    self.stack
                        .push(args.get(i).cloned().unwrap_or(JsValue::Undefined));
                }
                let rest = args.get(n..).unwrap_or(&[]).to_vec();
                let ptr = self.heap.len();
                self.heap.push(HeapObject {
                    data: HeapData::Array(rest),
                });
                self.stack.push(JsValue::Object(ptr));
                return;
            }
            _ => {}
        }
        let n = if address == 0 {
            args.len()
        } else {
            self.param_count(address)
        };
        for i in 0..n {
            self.stack
                .push(args.get(i).cloned().unwrap_or(JsValue::Undefined));
        }
    }

    fn exec_one(&mut self) -> ExecResult {
        if self.ip >= self.program.len() {
            return ExecResult::Stop;
        }
        let op = self.program[self.ip].clone();
        match op {
            OpCode::NewObject => {
                let ptr = self.heap.len();
                self.heap.push(HeapObject {
                    data: HeapData::Object(HashMap::new()),
                });
                self.stack.push(JsValue::Object(ptr));
            }

            OpCode::NewObjectWithProto => {
                // Stack: [prototype] -> creates new object with given prototype
                let proto = self
                    .stack
                    .pop()
                    .expect("NewObjectWithProto: missing prototype");
                let ptr = self.heap.len();
                self.heap.push(HeapObject {
                    data: HeapData::Object(HashMap::new()),
                });

                // Set the prototype
                if let JsValue::Object(proto_ptr) = proto
                    && let Some(heap_item) = self.heap.get_mut(ptr)
                    && let HeapData::Object(props) = &mut heap_item.data
                {
                    props.insert("__proto__".to_string(), JsValue::Object(proto_ptr));
                }

                self.stack.push(JsValue::Object(ptr));
            }

            OpCode::SetProp(name) => {
                let value = self.stack.pop().unwrap();
                let target = self.stack.pop().unwrap();
                if let JsValue::Object(ptr) = target {
                    // Check for setter in prototype chain
                    let setter_addr_and_env = self.find_setter_with_proto_chain(ptr, &name);

                    if let Some((address, env)) = setter_addr_and_env {
                        // Run to completion: a setter's result is discarded
                        // (left on the stack, it shifted every later value).
                        let setter = JsValue::Function { address, env };
                        if let Err(e) = self.call_sync(&setter, JsValue::Object(ptr), vec![value]) {
                            return self.throw_exception(e);
                        }
                        self.ip += 1;
                        return ExecResult::Continue;
                    }

                    // No setter found, store the value directly
                    if let Some(heap_item) = self.heap.get_mut(ptr)
                        && let HeapData::Object(props) = &mut heap_item.data
                    {
                        props.insert(name.to_string(), value);
                    }
                } else if matches!(target, JsValue::Null | JsValue::Undefined) {
                    let shown = self.to_js_string(&target);
                    return self.throw_error(
                        "TypeError",
                        format!("Cannot set properties of {shown} (setting '{name}')"),
                    );
                }
            }

            OpCode::SetPropComputed => {
                // Pops [obj, value, key] -> sets obj[key] = value
                let key_val = self.stack.pop().unwrap();
                let value = self.stack.pop().unwrap();
                let target = self.stack.pop().unwrap();

                // arr[i] = v on an array: set the element, growing the array.
                // (Previously only plain objects were handled, so this
                // assignment was silently ignored.)
                if let (JsValue::Object(ptr), JsValue::Number(n)) = (&target, &key_val)
                    && *n >= 0.0
                    && n.fract() == 0.0
                    && let Some(HeapObject {
                        data: HeapData::Array(arr),
                    }) = self.heap.get_mut(*ptr)
                {
                    let i = *n as usize;
                    if i >= arr.len() {
                        arr.resize(i + 1, JsValue::Undefined);
                    }
                    arr[i] = value;
                    self.ip += 1;
                    return ExecResult::Continue;
                }

                if matches!(target, JsValue::Null | JsValue::Undefined) {
                    let shown = self.to_js_string(&target);
                    let key = self.to_js_string(&key_val);
                    return self.throw_error(
                        "TypeError",
                        format!("Cannot set properties of {shown} (setting '{key}')"),
                    );
                }
                if let JsValue::Object(ptr) = target {
                    let key_name = self.to_js_string(&key_val);

                    if let Some(heap_item) = self.heap.get_mut(ptr)
                        && let HeapData::Object(props) = &mut heap_item.data
                    {
                        props.insert(key_name, value);
                    }
                }
            }

            OpCode::GetPropComputed => {
                // Pops [obj, key] -> pushes obj[key]
                if self.stack.len() < 2 {
                    panic!("GetPropComputed with insufficient stack at ip={}", self.ip);
                }
                let key_val = self.stack.pop().unwrap();
                let target = self.stack.pop().unwrap();
                if matches!(target, JsValue::Null | JsValue::Undefined) {
                    let shown = self.to_js_string(&target);
                    let key = self.to_js_string(&key_val);
                    return self.throw_error(
                        "TypeError",
                        format!("Cannot read properties of {shown} (reading '{key}')"),
                    );
                }

                match (target, key_val) {
                    (JsValue::Object(ptr), JsValue::Number(idx)) => {
                        // Array access: arr[index]
                        if let Some(heap_obj) = self.heap.get(ptr)
                            && let HeapData::Array(arr) = &heap_obj.data
                        {
                            let i = idx as usize;
                            let val = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                            self.stack.push(val.clone());
                            self.ip += 1;
                            return ExecResult::Continue;
                        }
                        self.stack.push(JsValue::Undefined);
                    }
                    (JsValue::Object(ptr), key_val) => {
                        let key_name = self.to_js_string(&key_val);

                        // Check if this is an array - handle string numeric indices
                        if let Some(heap_obj) = self.heap.get(ptr) {
                            match &heap_obj.data {
                                HeapData::Array(arr) => {
                                    // Try to parse as number for array access
                                    if let Ok(i) = key_name.parse::<usize>() {
                                        let val = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                                        self.stack.push(val);
                                    } else if key_name == "length" {
                                        self.stack.push(JsValue::Number(arr.len() as f64));
                                    } else if matches!(
                                        key_name.as_str(),
                                        "push"
                                            | "pop"
                                            | "shift"
                                            | "unshift"
                                            | "splice"
                                            | "slice"
                                            | "concat"
                                            | "join"
                                            | "indexOf"
                                            | "lastIndexOf"
                                            | "includes"
                                            | "reverse"
                                            | "fill"
                                            | "at"
                                            | "map"
                                            | "filter"
                                            | "forEach"
                                            | "reduce"
                                            | "find"
                                            | "findIndex"
                                            | "some"
                                            | "every"
                                            | "flat"
                                            | "flatMap"
                                            | "sort"
                                    ) {
                                        self.stack.push(JsValue::NativeFunction(0));
                                    } else {
                                        self.stack.push(JsValue::Undefined);
                                    }
                                }
                                _ => {
                                    // Object access: obj[key] - look up with prototype chain
                                    let value = self.get_prop_with_proto_chain(ptr, &key_name);
                                    self.stack.push(value.clone());
                                }
                            }
                        } else {
                            self.stack.push(JsValue::Undefined);
                        }
                    }
                    (JsValue::String(s), JsValue::Number(idx)) => {
                        // String char access: str[index]
                        // Use O(1) byte indexing for ASCII strings (common case)
                        let i = idx as usize;
                        let bytes = s.as_bytes();
                        let char_val = if i < bytes.len() {
                            let b = bytes[i];
                            if b < 128 {
                                // ASCII: O(1) fast path
                                JsValue::String((b as char).to_string())
                            } else {
                                // Non-ASCII: fallback to chars().nth() - O(n) but rare
                                s.chars()
                                    .nth(i)
                                    .map(|c| JsValue::String(c.to_string()))
                                    .unwrap_or(JsValue::Undefined)
                            }
                        } else {
                            JsValue::Undefined
                        };
                        self.stack.push(char_val);
                    }
                    _ => {
                        self.stack.push(JsValue::Undefined);
                    }
                }
            }

            OpCode::GetProp(name) => {
                let target = self.stack.pop();

                match target {
                    Some(JsValue::Object(ptr)) => {
                        if let Some(heap_item) = self.heap.get(ptr) {
                            match &heap_item.data {
                                HeapData::Object(_props) => {
                                    let getter_name = format!("getter:{}", name);
                                    let val = self.get_prop_with_proto_chain(ptr, &getter_name);

                                    if let JsValue::Function { address, env } = val {
                                        let this_context = JsValue::Object(ptr);
                                        // No arguments: a getter's parameters
                                        // are undefined (without this, they
                                        // popped the caller's values).
                                        self.push_call_args(&[], address);

                                        let frame = Frame {
                                            return_address: self.ip + 1,
                                            locals: HashMap::new(),
                                            indexed_locals: Vec::new(),
                                            this_context,
                                            new_target: None,
                                            super_called: false,
                                            resume_ip: None,
                                            env,
                                        };

                                        self.call_stack.push(frame);
                                        self.ip = address;
                                        return ExecResult::ContinueNoIpInc;
                                    }

                                    let val = self.get_prop_with_proto_chain(ptr, &name);
                                    self.stack.push(val);
                                }
                                HeapData::Array(arr) => {
                                    if name == "length" {
                                        self.stack.push(JsValue::Number(arr.len() as f64));
                                    } else if matches!(
                                        name.as_str(),
                                        "push"
                                            | "pop"
                                            | "shift"
                                            | "unshift"
                                            | "splice"
                                            | "slice"
                                            | "concat"
                                            | "join"
                                            | "indexOf"
                                            | "lastIndexOf"
                                            | "includes"
                                            | "reverse"
                                            | "fill"
                                            | "at"
                                            | "map"
                                            | "filter"
                                            | "forEach"
                                            | "reduce"
                                            | "find"
                                            | "findIndex"
                                            | "some"
                                            | "every"
                                            | "flat"
                                            | "flatMap"
                                            | "sort"
                                    ) {
                                        // Array methods are handled by CallMethod;
                                        // return a function-typed value so typeof reports "function"
                                        self.stack.push(JsValue::NativeFunction(0));
                                    } else {
                                        self.stack.push(JsValue::Undefined);
                                    }
                                }
                                HeapData::ByteStream(bytes) => {
                                    if name == "length" {
                                        self.stack.push(JsValue::Number(bytes.len() as f64));
                                    } else {
                                        self.stack.push(JsValue::Undefined);
                                    }
                                }
                                HeapData::Map(map) => {
                                    if name == "size" {
                                        self.stack.push(JsValue::Number(map.len() as f64));
                                    } else {
                                        self.stack.push(JsValue::Undefined);
                                    }
                                }
                                HeapData::Set(set) => {
                                    if name == "size" {
                                        self.stack.push(JsValue::Number(set.len() as f64));
                                    } else {
                                        self.stack.push(JsValue::Undefined);
                                    }
                                }
                            }
                        } else {
                            self.stack.push(JsValue::Undefined);
                        }
                    }
                    // Special case: looking up .prototype on a function value
                    Some(JsValue::Function {
                        address: _address,
                        env: _env,
                    }) if name == "prototype" => {
                        // Functions don't have a prototype property by default in our VM
                        // This returns undefined
                        self.stack.push(JsValue::Undefined);
                    }
                    Some(JsValue::String(s)) => {
                        if name == "length" {
                            self.stack.push(JsValue::Number(s.len() as f64));
                        } else {
                            self.stack.push(JsValue::Undefined);
                        }
                    }
                    // Promise methods are handled by CallMethod; a function-
                    // typed value so `typeof p.then` is "function".
                    Some(JsValue::Promise(_))
                        if matches!(name.as_str(), "then" | "catch" | "finally") =>
                    {
                        self.stack.push(JsValue::NativeFunction(0));
                    }
                    Some(target @ (JsValue::Null | JsValue::Undefined)) => {
                        let shown = self.to_js_string(&target);
                        return self.throw_error(
                            "TypeError",
                            format!("Cannot read properties of {shown} (reading '{name}')"),
                        );
                    }
                    _ => {
                        // Other primitives have no properties here.
                        self.stack.push(JsValue::Undefined);
                    }
                }
            }

            OpCode::Push(v) => self.stack.push(v),

            OpCode::Let(name) => {
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                if self.call_stack.is_empty() {
                    eprintln!("ERROR: Let opcode with empty call_stack at ip={}", self.ip);
                    eprintln!("Stack depth: {}", self.stack.len());
                    return ExecResult::Stop;
                }
                self.call_stack.last_mut().unwrap().locals.insert(name, val);
            }

            OpCode::Store(name) => {
                // Lexical scoping: a name is a local of the running function
                // or a global (frame 0). Captured variables never come
                // through here: the compiler accesses them through their
                // environment object. (This used to search every frame on
                // the call stack, so a function could read and overwrite its
                // caller's locals.)
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                let current = self.call_stack.len() - 1;
                let frame = if self.call_stack[current].locals.contains_key(&name)
                    || !self.call_stack[0].locals.contains_key(&name)
                {
                    current
                } else {
                    0
                };
                self.call_stack[frame].locals.insert(name, val);
            }

            OpCode::Load(name) => {
                let value = self
                    .call_stack
                    .last()
                    .and_then(|f| f.locals.get(&name))
                    .or_else(|| self.call_stack[0].locals.get(&name))
                    .cloned();
                match value {
                    Some(value) => self.stack.push(value),
                    // Compiler temporaries (`%...`) may be read before they
                    // are first written.
                    None if name.starts_with('%') => self.stack.push(JsValue::Undefined),
                    None => {
                        return self
                            .throw_error("ReferenceError", format!("{name} is not defined"));
                    }
                }
            }

            OpCode::LoadClosureEnv => {
                let env = self.call_stack.last().and_then(|f| f.env);
                self.stack
                    .push(env.map_or(JsValue::Undefined, JsValue::Object));
            }

            OpCode::LoadThis => {
                // Note: The super() check is disabled because it fires during constructor
                // setup (private field initialization) before the constructor body.
                // A proper implementation would need to track when we're in the
                // constructor body vs setup phase.
                let context = self.call_stack.last().unwrap().this_context.clone();
                self.stack.push(context);
            }

            OpCode::Call(arg_count) => {
                // Stack overflow protection
                if self.call_stack.len() >= MAX_CALL_STACK_DEPTH {
                    return self.throw_error("RangeError", "Maximum call stack size exceeded");
                }

                let callee = self.stack.pop().expect("Missing callee");
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.stack.pop().expect("Missing argument"));
                }
                args.reverse();

                match callee {
                    JsValue::Function { address, env } => {
                        // Record function call for tiered compilation
                        self.record_function_call(address);

                        self.push_call_args(&args, address);

                        // CLOSURE CONTEXT SWITCH: The frame keeps a pointer to the
                        // environment heap object. Load/Store resolve captured
                        // variables through it, so mutations persist across calls.
                        let frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context: JsValue::Undefined,
                            new_target: None,
                            super_called: false,
                            resume_ip: None,
                            env,
                        };

                        self.call_stack.push(frame);
                        self.ip = address;
                        return ExecResult::ContinueNoIpInc;
                    }
                    JsValue::NativeFunction(idx) => {
                        // (The arguments are in call order already; they
                        // were reversed a second time here, so `fetch(url,
                        // options)` got `(options, url)`.)
                        let func = self.native_functions[idx];
                        let result = func(self, args);
                        if let Some(e) = self.native_throw.take() {
                            return self.throw_exception(e);
                        }
                        self.stack.push(result);
                    }
                    JsValue::Object(ptr) if self.resolver_of(ptr).is_some() => {
                        // `resolve(v)` / `reject(e)` of a `new Promise`
                        let callee = JsValue::Object(ptr);
                        match self.call_value(&callee, JsValue::Undefined, args) {
                            Ok(v) => self.stack.push(v),
                            Err(e) => return self.throw_exception(e),
                        }
                    }
                    JsValue::Object(ptr) => {
                        // Check if object has a __call__ property (callable object like String)
                        if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(ptr)
                        {
                            if let Some(JsValue::NativeFunction(idx)) = props.get("__call__") {
                                let idx = *idx;
                                let func = self.native_functions[idx];
                                let result = func(self, args);
                                if let Some(e) = self.native_throw.take() {
                                    return self.throw_exception(e);
                                }
                                self.stack.push(result);
                            } else if let Some(JsValue::Function { address, env }) =
                                props.get("__call__")
                            {
                                let address = *address;
                                let env = *env;
                                self.push_call_args(&args, address);
                                let frame = Frame {
                                    return_address: self.ip + 1,
                                    locals: HashMap::new(),
                                    indexed_locals: Vec::new(),
                                    this_context: JsValue::Object(ptr),
                                    new_target: None,
                                    super_called: false,
                                    resume_ip: None,
                                    env,
                                };
                                self.call_stack.push(frame);
                                self.ip = address;
                                return ExecResult::ContinueNoIpInc;
                            } else {
                                let shown = crate::stdlib::inspect(self, &JsValue::Object(ptr));
                                return self.throw_error(
                                    "TypeError",
                                    format!("{shown} is not a function"),
                                );
                            }
                        } else {
                            panic!("Object reference invalid: Object({})", ptr);
                        }
                    }
                    other => {
                        let shown = crate::stdlib::inspect(self, &other);
                        return self.throw_error("TypeError", format!("{shown} is not a function"));
                    }
                }
            }

            OpCode::Return => {
                if self.call_stack.is_empty() {
                    return ExecResult::Stop;
                }
                let frame = self.call_stack.pop().expect("Missing frame");
                // A constructor (`new`, or `super(...)`) results in `this`
                // unless it returns an object of its own.
                if frame.new_target.is_some()
                    && !matches!(self.stack.last(), Some(JsValue::Object(_)))
                {
                    self.stack.pop();
                    self.stack.push(frame.this_context.clone());
                }
                self.ip = frame.return_address;
                if self.ip == usize::MAX {
                    return ExecResult::Stop;
                }
                return ExecResult::ContinueNoIpInc;
            }

            OpCode::Drop(name) => {
                self.call_stack.last_mut().unwrap().locals.remove(&name);
            }

            OpCode::Add => {
                let mut b = self.stack.pop().unwrap_or(JsValue::Undefined);
                let mut a = self.stack.pop().unwrap_or(JsValue::Undefined);
                // An object with its own `valueOf` / `toString` converts
                // through it first.
                for v in [&mut a, &mut b] {
                    match self.to_primitive(v, false) {
                        Ok(Some(p)) => *v = p,
                        Ok(None) => {}
                        Err(e) => return self.throw_exception(e),
                    }
                }
                let result = match (a, b) {
                    (JsValue::Number(x), JsValue::Number(y)) => JsValue::Number(x + y),
                    (JsValue::String(mut x), JsValue::String(y)) => {
                        x.push_str(&y);
                        JsValue::String(x)
                    }
                    // A string or object operand makes `+` concatenate.
                    (a, b)
                        if matches!(a, JsValue::String(_) | JsValue::Object(_))
                            || matches!(b, JsValue::String(_) | JsValue::Object(_)) =>
                    {
                        let mut s = self.to_js_string(&a);
                        s.push_str(&self.to_js_string(&b));
                        JsValue::String(s)
                    }
                    (a, b) => JsValue::Number(errors::to_number(&a) + errors::to_number(&b)),
                };
                self.stack.push(result);
            }
            OpCode::And => {
                let b = self.stack.pop().unwrap();
                let a = self.stack.pop().unwrap();
                // Logical AND: returns a if falsy, otherwise b (short-circuit)
                let a_truthy = match &a {
                    JsValue::Boolean(false) | JsValue::Null | JsValue::Undefined => false,
                    JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
                    JsValue::String(s) => !s.is_empty(),
                    _ => true, // Objects, functions are truthy
                };
                // Return a if falsy, b if a is truthy (JS semantics)
                if a_truthy {
                    self.stack.push(b);
                } else {
                    self.stack.push(a);
                }
            }

            OpCode::Or => {
                let b = self.stack.pop().expect("Missing right operand for ||");
                let a = self.stack.pop().expect("Missing left operand for ||");
                // Logical OR: returns a if truthy, otherwise b (short-circuit)
                let a_truthy = match &a {
                    JsValue::Boolean(false) | JsValue::Null | JsValue::Undefined => false,
                    JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
                    JsValue::String(s) => !s.is_empty(),
                    _ => true, // Objects, functions are truthy
                };
                // Return a if truthy, b otherwise (JS semantics)
                if a_truthy {
                    self.stack.push(a);
                } else {
                    self.stack.push(b);
                }
            }

            OpCode::Not => {
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                let is_falsy = match val {
                    JsValue::Boolean(b) => !b,
                    JsValue::Number(n) => n == 0.0 || n.is_nan(),
                    JsValue::Null | JsValue::Undefined => true,
                    JsValue::String(ref s) => s.is_empty(),
                    _ => false,
                };
                self.stack.push(JsValue::Boolean(is_falsy));
            }

            OpCode::Neg => {
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                match val {
                    JsValue::Number(n) => self.stack.push(JsValue::Number(-n)),
                    _ => self.stack.push(JsValue::Number(f64::NAN)),
                }
            }

            OpCode::ToStr => {
                let v = self.stack.pop().unwrap_or(JsValue::Undefined);
                let v = match self.to_primitive(&v, true) {
                    Ok(Some(p)) => p,
                    Ok(None) => v,
                    Err(e) => return self.throw_exception(e),
                };
                self.stack.push(JsValue::String(self.to_js_string(&v)));
            }

            OpCode::TypeOf => {
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                self.stack
                    .push(JsValue::String(self.typeof_value(&val).to_string()));
            }

            OpCode::TypeOfName(name) => {
                // `typeof x` of an unbound name is "undefined", not an error.
                let type_str = self
                    .call_stack
                    .last()
                    .and_then(|f| f.locals.get(&name))
                    .or_else(|| self.call_stack[0].locals.get(&name))
                    .cloned();
                let type_str = type_str.map_or("undefined", |v| self.typeof_value(&v));
                self.stack.push(JsValue::String(type_str.to_string()));
            }

            OpCode::Delete(ref prop_name) => {
                let obj_val = self.stack.pop().unwrap_or(JsValue::Undefined);
                if let JsValue::Object(obj_id) = obj_val {
                    if obj_id < self.heap.len() {
                        if let HeapData::Object(ref mut props) = self.heap[obj_id].data {
                            props.remove(prop_name);
                            self.stack.push(JsValue::Boolean(true));
                        } else {
                            self.stack.push(JsValue::Boolean(false));
                        }
                    } else {
                        self.stack.push(JsValue::Boolean(false));
                    }
                } else {
                    self.stack.push(JsValue::Boolean(false));
                }
            }

            OpCode::Sub => match self.numeric_operands() {
                Ok((a, b)) => self.stack.push(JsValue::Number(a - b)),
                Err(e) => return self.throw_exception(e),
            },

            OpCode::Mul => match self.numeric_operands() {
                Ok((a, b)) => self.stack.push(JsValue::Number(a * b)),
                Err(e) => return self.throw_exception(e),
            },

            // Bitwise operators: both operands as 32-bit integers.
            OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::Xor
            | OpCode::ShiftLeft
            | OpCode::ShiftRight
            | OpCode::ShiftRightUnsigned => {
                let (a, b) = match self.numeric_operands() {
                    Ok(ab) => ab,
                    Err(e) => return self.throw_exception(e),
                };
                let (x, y) = (to_int32(a), to_int32(b));
                let shift = (y & 31) as u32;
                let r = match op {
                    OpCode::BitAnd => (x & y) as f64,
                    OpCode::BitOr => (x | y) as f64,
                    OpCode::Xor => (x ^ y) as f64,
                    OpCode::ShiftLeft => x.wrapping_shl(shift) as f64,
                    OpCode::ShiftRight => x.wrapping_shr(shift) as f64,
                    _ => (x as u32).wrapping_shr(shift) as f64,
                };
                self.stack.push(JsValue::Number(r));
            }

            OpCode::Pow => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number(a.powf(b)));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::Div => match self.numeric_operands() {
                Ok((a, b)) => self.stack.push(JsValue::Number(a / b)),
                Err(e) => return self.throw_exception(e),
            },

            OpCode::Print => {
                let v = self.stack.pop().unwrap_or(JsValue::Undefined);
                println!("➜ {:?}", v);
            }

            OpCode::Pop => {
                let _ = self.stack.pop();
            }

            OpCode::Jump(address) => {
                self.ip = address;
                return ExecResult::ContinueNoIpInc;
            }

            OpCode::JumpIfFalse(target) => {
                let condition = self.stack.pop().unwrap_or(JsValue::Undefined);
                let is_falsy = match condition {
                    JsValue::Boolean(b) => !b,
                    JsValue::Number(n) => n == 0.0,
                    JsValue::Null | JsValue::Undefined => true,
                    _ => false,
                };
                if is_falsy {
                    self.ip = target;
                    return ExecResult::ContinueNoIpInc;
                }
                // If condition is truthy, continue to next instruction (don't jump)
            }

            OpCode::Dup => {
                let val = self.stack.last().expect("Stack underflow").clone();
                self.stack.push(val);
            }

            OpCode::Swap => {
                // Swap the top two elements on the stack
                let b = self.stack.pop().expect("Swap: missing second value");
                let a = self.stack.pop().expect("Swap: missing first value");
                self.stack.push(b);
                self.stack.push(a);
            }

            OpCode::Swap3 => {
                // Swap the top three elements: [a, b, c] -> [c, b, a]
                let c = self.stack.pop().expect("Swap3: missing third value");
                let b = self.stack.pop().expect("Swap3: missing second value");
                let a = self.stack.pop().expect("Swap3: missing first value");
                self.stack.push(c);
                self.stack.push(b);
                self.stack.push(a);
            }

            OpCode::Eq => {
                let b = self.stack.pop().unwrap();
                let a = self.stack.pop().unwrap();
                self.stack.push(JsValue::Boolean(a == b));
            }

            OpCode::EqEq => {
                // Loose equality (==): performs type coercion
                let b = self.stack.pop().unwrap();
                let a = self.stack.pop().unwrap();

                // If strictly equal, push true
                if a == b {
                    self.stack.push(JsValue::Boolean(true));
                } else {
                    // Otherwise, try type coercion
                    let result = match (&a, &b) {
                        // Number and String: convert string to number
                        (JsValue::Number(n), JsValue::String(s))
                        | (JsValue::String(s), JsValue::Number(n)) => {
                            // JavaScript ToNumber: trimmed, "" is 0, exact.
                            let t = s.trim();
                            let parsed = if t.is_empty() {
                                0.0
                            } else {
                                t.parse::<f64>().unwrap_or(f64::NAN)
                            };
                            *n == parsed
                        }
                        // Boolean and Number coercion
                        (JsValue::Boolean(true), JsValue::Number(n))
                        | (JsValue::Number(n), JsValue::Boolean(true)) => {
                            (*n - 1.0).abs() < f64::EPSILON
                        }
                        (JsValue::Boolean(false), JsValue::Number(n))
                        | (JsValue::Number(n), JsValue::Boolean(false)) => {
                            (*n - 0.0).abs() < f64::EPSILON
                        }
                        // Null and Undefined are equal to each other
                        (JsValue::Null, JsValue::Undefined)
                        | (JsValue::Undefined, JsValue::Null) => true,
                        // Everything else: not equal
                        _ => false,
                    };
                    self.stack.push(JsValue::Boolean(result));
                }
            }

            OpCode::Ne => {
                let b = self.stack.pop().unwrap();
                let a = self.stack.pop().unwrap();
                self.stack.push(JsValue::Boolean(a != b));
            }

            OpCode::NeEq => {
                // Loose inequality (!=): opposite of loose equality
                let b = self.stack.pop().unwrap();
                let a = self.stack.pop().unwrap();

                // If strictly equal, return false
                if a == b {
                    self.stack.push(JsValue::Boolean(false));
                } else {
                    // Otherwise, try type coercion
                    let result = match (&a, &b) {
                        // Number and String: convert string to number
                        (JsValue::Number(n), JsValue::String(s))
                        | (JsValue::String(s), JsValue::Number(n)) => s
                            .parse::<f64>()
                            .map(|parsed| (*n - parsed).abs() >= f64::EPSILON)
                            .unwrap_or(true),
                        // Boolean and Number coercion
                        (JsValue::Boolean(true), JsValue::Number(n))
                        | (JsValue::Number(n), JsValue::Boolean(true)) => {
                            (*n - 1.0).abs() >= f64::EPSILON
                        }
                        (JsValue::Boolean(false), JsValue::Number(n))
                        | (JsValue::Number(n), JsValue::Boolean(false)) => {
                            (*n - 0.0).abs() >= f64::EPSILON
                        }
                        // Null and Undefined are equal to each other
                        (JsValue::Null, JsValue::Undefined)
                        | (JsValue::Undefined, JsValue::Null) => false,
                        // Everything else: not equal
                        _ => true,
                    };
                    self.stack.push(JsValue::Boolean(result));
                }
            }

            OpCode::Lt | OpCode::LtEq | OpCode::Gt | OpCode::GtEq => {
                let ord = match self.compare_operands() {
                    Ok(ord) => ord,
                    Err(e) => return self.throw_exception(e),
                };
                let result = ord.is_some_and(|o| match op {
                    OpCode::Lt => o.is_lt(),
                    OpCode::LtEq => o.is_le(),
                    OpCode::Gt => o.is_gt(),
                    _ => o.is_ge(),
                });
                self.stack.push(JsValue::Boolean(result));
            }

            // Rust's `%` on doubles is JavaScript's: the sign of `a`, NaN
            // for a zero `b`.
            OpCode::Mod => match self.numeric_operands() {
                Ok((a, b)) => self.stack.push(JsValue::Number(a % b)),
                Err(e) => return self.throw_exception(e),
            },

            OpCode::StoreElement => {
                let index_val = self.stack.pop().unwrap();
                let value = self.stack.pop().unwrap();
                let array_ptr = self.stack.pop().unwrap();

                if let (JsValue::Object(ptr), JsValue::Number(idx)) = (array_ptr, index_val)
                    && let Some(HeapObject {
                        data: HeapData::Array(arr),
                    }) = self.heap.get_mut(ptr)
                {
                    let i = idx as usize;
                    // Extend array if needed (JS semantics)
                    if i >= arr.len() {
                        arr.resize(i + 1, JsValue::Undefined);
                    }
                    arr[i] = value;
                }
            }

            OpCode::NewArray(size) => {
                let ptr = self.heap.len();
                let elements = vec![JsValue::Undefined; size];
                self.heap.push(HeapObject {
                    data: HeapData::Array(elements),
                });
                self.stack.push(JsValue::Object(ptr));
            }

            OpCode::LoadElement => {
                let index_val = self.stack.pop().expect("Missing index");
                let target = self.stack.pop().expect("Missing target (array or String)");
                match (target, index_val) {
                    (JsValue::Object(ptr), JsValue::Number(idx)) => {
                        if let Some(heap_obj) = self.heap.get(ptr)
                            && let HeapData::Array(arr) = &heap_obj.data
                        {
                            let i = idx as usize;
                            let val = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                            self.stack.push(val);
                        }
                    }
                    // Handle string index for arrays (needed for for...in loops)
                    (JsValue::Object(ptr), JsValue::String(idx_str)) => {
                        if let Some(heap_obj) = self.heap.get(ptr) {
                            match &heap_obj.data {
                                HeapData::Array(arr) => {
                                    // Try to parse as number for array access
                                    if let Ok(i) = idx_str.parse::<usize>() {
                                        let val = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                                        self.stack.push(val);
                                    } else {
                                        self.stack.push(JsValue::Undefined);
                                    }
                                }
                                HeapData::Object(props) => {
                                    // Object property access by string key
                                    let val =
                                        props.get(&idx_str).cloned().unwrap_or(JsValue::Undefined);
                                    self.stack.push(val);
                                }
                                _ => {
                                    self.stack.push(JsValue::Undefined);
                                }
                            }
                        } else {
                            self.stack.push(JsValue::Undefined);
                        }
                    }
                    (JsValue::String(s), JsValue::Number(idx)) => {
                        let i = idx as usize;
                        let char_val = s
                            .chars()
                            .nth(i)
                            .map(|c| JsValue::String(c.to_string()))
                            .unwrap_or(JsValue::Undefined);
                        self.stack.push(char_val);
                    }
                    _ => {
                        self.stack.push(JsValue::Undefined);
                    }
                }
            }

            OpCode::ArrayPush => {
                // Pops [array, value] -> pushes value to array, pushes array back
                let value = self.stack.pop().expect("ArrayPush: missing value");
                let arr_val = self.stack.pop().expect("ArrayPush: missing array");
                if let JsValue::Object(ptr) = arr_val {
                    if let Some(HeapObject {
                        data: HeapData::Array(arr),
                    }) = self.heap.get_mut(ptr)
                    {
                        arr.push(value);
                    }
                    self.stack.push(JsValue::Object(ptr));
                } else {
                    self.stack.push(arr_val);
                }
            }

            OpCode::ArraySpread => {
                // [target, source] -> [target]: append source's elements (an
                // array's, or a string's characters) to the array target.
                let source = self.stack.pop().unwrap_or(JsValue::Undefined);
                let target = self.stack.pop().unwrap_or(JsValue::Undefined);
                let items: Vec<JsValue> = match &source {
                    JsValue::String(s) => {
                        s.chars().map(|c| JsValue::String(c.to_string())).collect()
                    }
                    JsValue::Object(ptr) => match self.heap.get(*ptr) {
                        Some(HeapObject {
                            data: HeapData::Array(arr),
                        }) => arr.clone(),
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                };
                let iterable = matches!(source, JsValue::String(_))
                    || matches!(&source, JsValue::Object(p) if matches!(
                        self.heap.get(*p),
                        Some(HeapObject { data: HeapData::Array(_) })
                    ));
                if !iterable {
                    let shown = crate::stdlib::inspect(self, &source);
                    return self.throw_error("TypeError", format!("{shown} is not iterable"));
                }
                if let JsValue::Object(ptr) = target
                    && let Some(HeapObject {
                        data: HeapData::Array(arr),
                    }) = self.heap.get_mut(ptr)
                {
                    arr.extend(items);
                }
                self.stack.push(target);
            }

            OpCode::ObjectSpread => {
                // Pops [target_obj, source_obj] -> copies all properties from source to target, pushes target
                let source_val = self.stack.pop().expect("ObjectSpread: missing source");
                let target_val = self.stack.pop().expect("ObjectSpread: missing target");

                if let (JsValue::Object(target_ptr), JsValue::Object(source_ptr)) =
                    (target_val, source_val)
                {
                    // The source's own properties; an accessor gives its
                    // getter's value. Internal keys (`__proto__`) stay.
                    let mut source_props: Vec<(String, JsValue)> = Vec::new();
                    let stored: Vec<(String, JsValue)> = match self.heap.get(source_ptr) {
                        Some(HeapObject {
                            data: HeapData::Object(props),
                        }) => props.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                        Some(HeapObject {
                            data: HeapData::Array(items),
                        }) => items
                            .iter()
                            .enumerate()
                            .map(|(i, v)| (i.to_string(), v.clone()))
                            .collect(),
                        _ => Vec::new(),
                    };
                    for (key, value) in stored {
                        if let Some(name) = key.strip_prefix("getter:") {
                            if crate::runtime::builtins::is_hidden_key(name) {
                                continue;
                            }
                            match self.call_sync(&value, JsValue::Object(source_ptr), Vec::new()) {
                                Ok(v) => source_props.push((name.to_string(), v)),
                                Err(e) => return self.throw_exception(e),
                            }
                        } else if !crate::runtime::builtins::is_hidden_key(&key) {
                            source_props.push((key, value));
                        }
                    }
                    // Then, insert into target object
                    if let Some(HeapObject {
                        data: HeapData::Object(target_props),
                    }) = self.heap.get_mut(target_ptr)
                    {
                        for (key, value) in source_props {
                            target_props.insert(key, value);
                        }
                    }
                    self.stack.push(JsValue::Object(target_ptr));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::Halt => return ExecResult::Stop,

            OpCode::MakeClosure(address) => {
                // Pop the environment object pointer from the stack and create
                // a Function value with the captured environment attached.
                // This is the "lifting" operation that moves stack variables to the heap.
                let env_ptr = self.stack.pop().expect("Missing environment object");
                if let JsValue::Object(ptr) = env_ptr {
                    self.stack.push(JsValue::Function {
                        address,
                        env: Some(ptr),
                    });
                } else {
                    panic!("MakeClosure expects an Object pointer on stack");
                }
            }

            OpCode::Construct(arg_count) => {
                // Stack overflow protection
                if self.call_stack.len() >= MAX_CALL_STACK_DEPTH {
                    return self.throw_error("RangeError", "Maximum call stack size exceeded");
                }

                // Stack layout: [..., arg1, arg2, ..., constructor]
                let constructor_val = self.stack.pop().expect("Missing constructor");

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.stack.pop().expect("Missing argument"));
                }
                args.reverse();

                // Extract the actual constructor function and prototype
                let (address, env, prototype, new_target_val) = match &constructor_val {
                    JsValue::Function { address, env } => {
                        // For a plain function, new.target is the function itself
                        (*address, *env, None::<JsValue>, constructor_val.clone())
                    }
                    JsValue::Object(ptr) => {
                        // Look for a "constructor" property and "prototype" property
                        if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(*ptr)
                        {
                            let ctor = props.get("constructor").cloned();
                            let proto = props.get("prototype").cloned();
                            match ctor {
                                Some(JsValue::Function { address, env }) => {
                                    // In ES6 JavaScript, new.target is the class itself (the constructor function)
                                    // The class wrapper has a 'constructor' property pointing to the constructor
                                    // So we need to use the wrapper as new-target, not the extracted constructor
                                    (address, env, proto, constructor_val.clone())
                                }
                                Some(JsValue::Object(_ptr)) => {
                                    // Constructor might be wrapped in another object
                                    (0usize, None, proto, constructor_val.clone())
                                }
                                Some(JsValue::NativeFunction(_native_idx)) => {
                                    // Native function constructor - use index 0 as placeholder
                                    // The native function will handle construction itself
                                    (0usize, None, proto, constructor_val.clone())
                                }
                                Some(_) => {
                                    let shown = crate::stdlib::inspect(self, &constructor_val);
                                    return self.throw_error(
                                        "TypeError",
                                        format!("{shown} is not a constructor"),
                                    );
                                }
                                None => {
                                    // No constructor property - this is a "constructor object" like Promise
                                    // For Promise-like objects, we treat the object itself as the constructor
                                    // and call a special constructor handler
                                    // For now, we'll panic with a helpful message
                                    // Create a placeholder that will be handled specially
                                    (0usize, None, proto, constructor_val.clone())
                                }
                            }
                        } else {
                            let shown = crate::stdlib::inspect(self, &constructor_val);
                            return self
                                .throw_error("TypeError", format!("{shown} is not a constructor"));
                        }
                    }
                    _ => {
                        let shown = crate::stdlib::inspect(self, &constructor_val);
                        return self
                            .throw_error("TypeError", format!("{shown} is not a constructor"));
                    }
                };

                // Create new object with prototype
                let this_ptr = self.heap.len();
                let this_obj = JsValue::Object(this_ptr);
                self.heap.push(HeapObject {
                    data: HeapData::Object(HashMap::new()),
                });

                // Set prototype if we have one
                if let Some(proto_val) = prototype
                    && let JsValue::Object(proto_ptr) = proto_val
                    && let Some(heap_item) = self.heap.get_mut(this_ptr)
                    && let HeapData::Object(props) = &mut heap_item.data
                {
                    props.insert("__proto__".to_string(), JsValue::Object(proto_ptr));
                }

                // Arguments for a compiled constructor's prologue. (A native
                // one takes them directly: pushing them too left them on the
                // stack, where the next call took one as its argument.)
                if address != 0 {
                    self.push_call_args(&args, address);
                }

                // Create frame with `this` bound to the new object
                let frame = Frame {
                    return_address: self.ip + 1,
                    locals: HashMap::new(),
                    indexed_locals: Vec::new(),
                    this_context: this_obj.clone(),
                    new_target: Some(new_target_val.clone()),
                    super_called: false,
                    resume_ip: None,
                    env,
                };

                // Check if this is a native function constructor
                if address == 0 {
                    // Native constructor - check constructor type by looking for __type__ property
                    let constructor_type = if let JsValue::Object(ptr) = &new_target_val {
                        if let Some(heap_obj) = self.heap.get(*ptr) {
                            if let HeapData::Object(props) = &heap_obj.data {
                                // Check for __type__ property first
                                if let Some(JsValue::String(t)) = props.get("__type__") {
                                    t.clone()
                                } else if props.contains_key("then") && props.contains_key("catch")
                                {
                                    "Promise".to_string()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };

                    if constructor_type == "Map" {
                        // Handle Map construction: new Map() or new Map(iterable)
                        let map_ptr = self.heap.len();
                        self.heap.push(HeapObject {
                            data: HeapData::Map(Vec::new()),
                        });
                        // If an iterable is passed, we'd need to iterate it - for now just create empty
                        self.stack.push(JsValue::Object(map_ptr));
                    } else if constructor_type == "Set" {
                        // Handle Set construction: new Set() or new Set(iterable)
                        let set_ptr = self.heap.len();
                        self.heap.push(HeapObject {
                            data: HeapData::Set(Vec::new()),
                        });
                        // If an iterable is passed, we'd need to iterate it - for now just create empty
                        self.stack.push(JsValue::Object(set_ptr));
                    } else if constructor_type == "Promise" {
                        // `new Promise(executor)`: the executor runs now.
                        let executor = args.first().cloned().unwrap_or(JsValue::Undefined);
                        match self.construct_promise(executor) {
                            Ok(p) => self.stack.push(JsValue::Promise(p)),
                            Err(e) => return self.throw_exception(e),
                        }
                    } else {
                        // Regular native constructor - push a frame with this_context
                        let native_frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context: this_obj.clone(),
                            new_target: Some(new_target_val.clone()),
                            super_called: false,
                            resume_ip: None,
                            env: None,
                        };
                        self.call_stack.push(native_frame);

                        let native_result = if let JsValue::Object(ptr) = &new_target_val {
                            if let Some(heap_obj) = self.heap.get(*ptr) {
                                if let HeapData::Object(props) = &heap_obj.data {
                                    if let Some(JsValue::NativeFunction(native_idx)) =
                                        props.get("constructor")
                                    {
                                        let func = self.native_functions[*native_idx];
                                        func(self, args.clone())
                                    } else {
                                        JsValue::Undefined
                                    }
                                } else {
                                    JsValue::Undefined
                                }
                            } else {
                                JsValue::Undefined
                            }
                        } else {
                            JsValue::Undefined
                        };

                        // Pop the native frame
                        self.call_stack.pop();

                        // Push result and continue
                        self.stack.push(native_result);
                    }
                } else {
                    // Regular function - just call, this is set in frame
                    self.call_stack.push(frame);
                    self.ip = address;
                    return ExecResult::ContinueNoIpInc;
                }
            }

            OpCode::Require => {
                let module_name = self.stack.pop().unwrap_or(JsValue::Undefined);
                let module = match module_name {
                    JsValue::String(module_name) => self
                        .modules
                        .get(&module_name)
                        .cloned()
                        .unwrap_or(JsValue::Undefined),
                    _ => JsValue::Undefined,
                };
                self.stack.push(module);
            }

            OpCode::CallMethod(name, arg_count) => {
                let reciever = self.stack.pop().expect("Missing reciever");

                match reciever {
                    // -- String methods --
                    // Core string methods needed for bootstrap compiler
                    // String methods are shared with native code
                    // (`runtime::builtins`).
                    JsValue::String(s) => {
                        let args = self.pop_args(arg_count);
                        let result = crate::runtime::builtins::string_method(
                            &VmHost(&self.heap),
                            &s,
                            &name,
                            &args,
                        );
                        let receiver = JsValue::String(s.clone());
                        return self.finish_builtin(result, receiver, &name);
                    }
                    JsValue::Object(ptr) => {
                        // Array methods are shared with native code
                        // (`runtime::builtins`). `push` and `pop` stay in
                        // place; the rest work on a copy that is written back.
                        if let Some(HeapObject {
                            data: HeapData::Array(_),
                        }) = self.heap.get(ptr)
                        {
                            let args = self.pop_args(arg_count);
                            let Some(HeapObject {
                                data: HeapData::Array(arr),
                            }) = self.heap.get_mut(ptr)
                            else {
                                unreachable!()
                            };
                            match name.as_str() {
                                "push" => {
                                    arr.extend(args);
                                    let n = arr.len();
                                    self.stack.push(JsValue::Number(n as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "pop" => {
                                    let v = arr.pop().unwrap_or(JsValue::Undefined);
                                    self.stack.push(v);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                _ => {}
                            }
                            let mut elems = arr.clone();
                            let result = crate::runtime::builtins::array_method(
                                &VmHost(&self.heap),
                                &mut elems,
                                &name,
                                &args,
                            );
                            if result.is_some() {
                                self.heap[ptr].data = HeapData::Array(elems);
                            }
                            return self.finish_builtin(result, JsValue::Object(ptr), &name);
                        }

                        // Check if this is a Map and handle Map methods
                        if let Some(HeapObject {
                            data: HeapData::Map(map),
                        }) = self.heap.get_mut(ptr)
                        {
                            match name.as_str() {
                                "get" => {
                                    let key = if arg_count > 0 {
                                        self.stack.pop().unwrap_or(JsValue::Undefined)
                                    } else {
                                        JsValue::Undefined
                                    };
                                    for _ in 1..arg_count {
                                        self.stack.pop();
                                    }
                                    let result = map
                                        .iter()
                                        .find(|(k, _)| match (k, &key) {
                                            (JsValue::Number(a), JsValue::Number(b)) => a == b,
                                            (JsValue::String(a), JsValue::String(b)) => a == b,
                                            (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
                                            (JsValue::Null, JsValue::Null) => true,
                                            (JsValue::Undefined, JsValue::Undefined) => true,
                                            (JsValue::Object(a), JsValue::Object(b)) => a == b,
                                            _ => false,
                                        })
                                        .map(|(_, v)| v.clone())
                                        .unwrap_or(JsValue::Undefined);
                                    self.stack.push(result);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "set" => {
                                    let mut args = Vec::with_capacity(arg_count);
                                    for _ in 0..arg_count {
                                        args.push(self.stack.pop().expect("Missing argument"));
                                    }
                                    args.reverse();
                                    let key = args.first().cloned().unwrap_or(JsValue::Undefined);
                                    let value = args.get(1).cloned().unwrap_or(JsValue::Undefined);

                                    // Remove existing key if present
                                    map.retain(|(k, _)| match (k, &key) {
                                        (JsValue::Number(a), JsValue::Number(b)) => a != b,
                                        (JsValue::String(a), JsValue::String(b)) => a != b,
                                        (JsValue::Boolean(a), JsValue::Boolean(b)) => a != b,
                                        (JsValue::Null, JsValue::Null) => false,
                                        (JsValue::Undefined, JsValue::Undefined) => false,
                                        (JsValue::Object(a), JsValue::Object(b)) => a != b,
                                        _ => true,
                                    });
                                    map.push((key, value));
                                    self.stack.push(JsValue::Object(ptr)); // Return the map itself
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "has" => {
                                    let key = if arg_count > 0 {
                                        self.stack.pop().unwrap_or(JsValue::Undefined)
                                    } else {
                                        JsValue::Undefined
                                    };
                                    for _ in 1..arg_count {
                                        self.stack.pop();
                                    }
                                    let found = map.iter().any(|(k, _)| match (k, &key) {
                                        (JsValue::Number(a), JsValue::Number(b)) => a == b,
                                        (JsValue::String(a), JsValue::String(b)) => a == b,
                                        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
                                        (JsValue::Null, JsValue::Null) => true,
                                        (JsValue::Undefined, JsValue::Undefined) => true,
                                        (JsValue::Object(a), JsValue::Object(b)) => a == b,
                                        _ => false,
                                    });
                                    self.stack.push(JsValue::Boolean(found));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "delete" => {
                                    let key = if arg_count > 0 {
                                        self.stack.pop().unwrap_or(JsValue::Undefined)
                                    } else {
                                        JsValue::Undefined
                                    };
                                    for _ in 1..arg_count {
                                        self.stack.pop();
                                    }
                                    let initial_len = map.len();
                                    map.retain(|(k, _)| match (k, &key) {
                                        (JsValue::Number(a), JsValue::Number(b)) => a != b,
                                        (JsValue::String(a), JsValue::String(b)) => a != b,
                                        (JsValue::Boolean(a), JsValue::Boolean(b)) => a != b,
                                        (JsValue::Null, JsValue::Null) => false,
                                        (JsValue::Undefined, JsValue::Undefined) => false,
                                        (JsValue::Object(a), JsValue::Object(b)) => a != b,
                                        _ => true,
                                    });
                                    self.stack.push(JsValue::Boolean(map.len() < initial_len));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "clear" => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    map.clear();
                                    self.stack.push(JsValue::Undefined);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "size" => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    self.stack.push(JsValue::Number(map.len() as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                _ => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    self.stack.push(JsValue::Undefined);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                            }
                        }

                        // Check if this is a Set and handle Set methods
                        if let Some(HeapObject {
                            data: HeapData::Set(set),
                        }) = self.heap.get_mut(ptr)
                        {
                            match name.as_str() {
                                "add" => {
                                    let value = if arg_count > 0 {
                                        self.stack.pop().unwrap_or(JsValue::Undefined)
                                    } else {
                                        JsValue::Undefined
                                    };
                                    for _ in 1..arg_count {
                                        self.stack.pop();
                                    }
                                    // Check if value already exists
                                    let exists = set.iter().any(|v| match (v, &value) {
                                        (JsValue::Number(a), JsValue::Number(b)) => a == b,
                                        (JsValue::String(a), JsValue::String(b)) => a == b,
                                        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
                                        (JsValue::Null, JsValue::Null) => true,
                                        (JsValue::Undefined, JsValue::Undefined) => true,
                                        (JsValue::Object(a), JsValue::Object(b)) => a == b,
                                        _ => false,
                                    });
                                    if !exists {
                                        set.push(value);
                                    }
                                    self.stack.push(JsValue::Object(ptr)); // Return the set itself
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "has" => {
                                    let value = if arg_count > 0 {
                                        self.stack.pop().unwrap_or(JsValue::Undefined)
                                    } else {
                                        JsValue::Undefined
                                    };
                                    for _ in 1..arg_count {
                                        self.stack.pop();
                                    }
                                    let found = set.iter().any(|v| match (v, &value) {
                                        (JsValue::Number(a), JsValue::Number(b)) => a == b,
                                        (JsValue::String(a), JsValue::String(b)) => a == b,
                                        (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
                                        (JsValue::Null, JsValue::Null) => true,
                                        (JsValue::Undefined, JsValue::Undefined) => true,
                                        (JsValue::Object(a), JsValue::Object(b)) => a == b,
                                        _ => false,
                                    });
                                    self.stack.push(JsValue::Boolean(found));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "delete" => {
                                    let value = if arg_count > 0 {
                                        self.stack.pop().unwrap_or(JsValue::Undefined)
                                    } else {
                                        JsValue::Undefined
                                    };
                                    for _ in 1..arg_count {
                                        self.stack.pop();
                                    }
                                    let initial_len = set.len();
                                    set.retain(|v| match (v, &value) {
                                        (JsValue::Number(a), JsValue::Number(b)) => a != b,
                                        (JsValue::String(a), JsValue::String(b)) => a != b,
                                        (JsValue::Boolean(a), JsValue::Boolean(b)) => a != b,
                                        (JsValue::Null, JsValue::Null) => false,
                                        (JsValue::Undefined, JsValue::Undefined) => false,
                                        (JsValue::Object(a), JsValue::Object(b)) => a != b,
                                        _ => true,
                                    });
                                    self.stack.push(JsValue::Boolean(set.len() < initial_len));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "clear" => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    set.clear();
                                    self.stack.push(JsValue::Undefined);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "size" => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    self.stack.push(JsValue::Number(set.len() as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                _ => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    self.stack.push(JsValue::Undefined);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                            }
                        }

                        // Lookup the method in the object through prototype chain
                        let mut method = self.get_prop_with_proto_chain(ptr, &name);
                        // An accessor: its getter gives the method (a
                        // namespace import's members are getters).
                        if matches!(method, JsValue::Undefined) {
                            let getter =
                                self.get_prop_with_proto_chain(ptr, &format!("getter:{name}"));
                            if !matches!(getter, JsValue::Undefined) {
                                match self.call_sync(&getter, JsValue::Object(ptr), Vec::new()) {
                                    Ok(v) => method = v,
                                    Err(e) => return self.throw_exception(e),
                                }
                            }
                        }

                        if let JsValue::NativeFunction(idx) = method {
                            // For native functions, call directly
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.stack.pop().expect("Missing argument"));
                            }
                            args.reverse();
                            let func = self.native_functions[idx];
                            let result = func(self, args);
                            if let Some(e) = self.native_throw.take() {
                                return self.throw_exception(e);
                            }
                            self.stack.push(result);
                            // Increment IP before returning since we return early
                            self.ip += 1;
                            return ExecResult::Continue;
                        } else if let JsValue::Function { address, env } = method {
                            // Stack overflow protection
                            if self.call_stack.len() >= MAX_CALL_STACK_DEPTH {
                                return self
                                    .throw_error("RangeError", "Maximum call stack size exceeded");
                            }

                            // Collect arguments
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.stack.pop().expect("Missing argument"));
                            }
                            args.reverse();

                            // Push arguments in call order
                            self.push_call_args(&args, address);

                            // Create new frame with `this` bound to the receiver object
                            let frame = Frame {
                                return_address: self.ip + 1,
                                locals: HashMap::new(),
                                indexed_locals: Vec::new(),
                                this_context: JsValue::Object(ptr),
                                new_target: None,
                                super_called: false,
                                resume_ip: None,
                                env,
                            };

                            self.call_stack.push(frame);
                            self.ip = address;
                            return ExecResult::ContinueNoIpInc;
                        }
                        let shown = crate::stdlib::inspect(self, &JsValue::Object(ptr));
                        return self
                            .throw_error("TypeError", format!("{shown}.{name} is not a function"));
                    }
                    // Handle Promise.then and Promise.catch methods
                    JsValue::Promise(promise) => {
                        let mut args = self.pop_args(arg_count).into_iter();
                        let mut arg = || args.next().unwrap_or(JsValue::Undefined);
                        let result = match name.as_str() {
                            "then" => {
                                let (f, r) = (arg(), arg());
                                self.promise_then(&promise, f, r)
                            }
                            "catch" => {
                                let r = arg();
                                self.promise_then(&promise, JsValue::Undefined, r)
                            }
                            "finally" => {
                                let on_finally = arg();
                                let result = Promise::new();
                                self.add_reaction(
                                    &promise,
                                    value::Reaction::Finally {
                                        on_finally,
                                        result: result.clone(),
                                    },
                                );
                                result
                            }
                            _ => {
                                return self.throw_error(
                                    "TypeError",
                                    format!("Promise.{name} is not a function"),
                                );
                            }
                        };
                        self.stack.push(JsValue::Promise(result));
                        self.ip += 1;
                        return ExecResult::Continue;
                    }
                    // `f.call(thisArg, ...args)`, `f.apply(thisArg, array)`
                    f @ (JsValue::Function { .. } | JsValue::NativeFunction(_))
                        if name == "call" || name == "apply" =>
                    {
                        let mut args = self.pop_args(arg_count);
                        let this = if args.is_empty() {
                            JsValue::Undefined
                        } else {
                            args.remove(0)
                        };
                        if name == "apply" {
                            args = match args.first() {
                                Some(JsValue::Object(ptr)) => match self.heap.get(*ptr) {
                                    Some(HeapObject {
                                        data: HeapData::Array(items),
                                    }) => items.clone(),
                                    _ => Vec::new(),
                                },
                                _ => Vec::new(),
                            };
                        }
                        match self.call_sync(&f, this, args) {
                            Ok(v) => self.stack.push(v),
                            Err(e) => return self.throw_exception(e),
                        }
                        self.ip += 1;
                        return ExecResult::Continue;
                    }
                    JsValue::Number(n) => {
                        let args = self.pop_args(arg_count);
                        let result = crate::runtime::builtins::number_method(
                            &VmHost(&self.heap),
                            n,
                            &name,
                            &args,
                        );
                        return self.finish_builtin(result, JsValue::Number(n), &name);
                    }
                    other => {
                        let _ = self.pop_args(arg_count);
                        let shown = self.to_js_string(&other);
                        if matches!(other, JsValue::Null | JsValue::Undefined) {
                            return self.throw_error(
                                "TypeError",
                                format!("Cannot read properties of {shown} (reading '{name}')"),
                            );
                        }
                        let shown = crate::stdlib::inspect(self, &other);
                        return self
                            .throw_error("TypeError", format!("{shown}.{name} is not a function"));
                    }
                }
            }

            OpCode::StoreLocal(idx) => {
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                let frame = self.call_stack.last_mut().unwrap();
                let idx = idx as usize;
                while frame.indexed_locals.len() <= idx {
                    frame.indexed_locals.push(JsValue::Undefined);
                }
                frame.indexed_locals[idx] = val;
            }

            OpCode::LoadLocal(idx) => {
                let frame = self.call_stack.last().unwrap();
                let val = frame
                    .indexed_locals
                    .get(idx as usize)
                    .cloned()
                    .unwrap_or(JsValue::Undefined);
                self.stack.push(val);
            }

            // === Exception handling ===
            OpCode::SetupTry {
                catch_addr,
                finally_addr,
            } => {
                // Record the current state for potential unwinding
                self.exception_handlers.push(ExceptionHandler {
                    catch_addr,
                    finally_addr,
                    stack_depth: self.stack.len(),
                    call_stack_depth: self.call_stack.len(),
                });
            }

            OpCode::PopTry => {
                // Remove the current try block handler
                self.exception_handlers.pop();
            }

            OpCode::Throw => {
                // Pop the exception value
                let exception = self.stack.pop().unwrap_or(JsValue::Undefined);
                return self.throw_exception(exception);
            }

            OpCode::EnterFinally(rethrow) => {
                // Emitted at the end of a finally block. If an exception was
                // pending when the finally was entered (try/finally with no
                // catch, or a throw from inside the catch block), re-throw it
                // now that the finally has completed.
                if rethrow && let Some(exc) = self.current_exception.take() {
                    return self.throw_exception(exc);
                }
                // No pending exception - fall through to the next instruction
            }

            // === Class inheritance ===
            OpCode::SetProto => {
                // Stack: [obj, proto] -> sets obj.__proto__ = proto, pushes obj
                let proto = self.stack.pop().expect("SetProto: missing proto");
                let obj = self.stack.pop().expect("SetProto: missing obj");

                if let JsValue::Object(obj_ptr) = obj {
                    if let Some(HeapObject {
                        data: HeapData::Object(props),
                    }) = self.heap.get_mut(obj_ptr)
                    {
                        props.insert("__proto__".to_string(), proto);
                    }
                    self.stack.push(JsValue::Object(obj_ptr));
                } else {
                    panic!("SetProto: expected object, got {:?}", obj);
                }
            }

            OpCode::CallSuper(arg_count) => {
                // Stack: [args..., super_constructor]
                let super_ctor = self.stack.pop().unwrap_or(JsValue::Undefined);
                let args = self.pop_args(arg_count);
                return self.call_super(super_ctor, args);
            }

            OpCode::ForwardArgs | OpCode::RestArgs(_) => {}

            OpCode::Intrinsic(id, argc) => {
                let args = self.pop_args(argc);
                let result = crate::runtime::intrinsics::call(&VmHost(&self.heap), id, &args);
                return self.finish_builtin(Some(result), JsValue::Undefined, "");
            }

            OpCode::SuperForward => {
                // Stack: [args..., count, parent]
                let super_ctor = self.stack.pop().unwrap_or(JsValue::Undefined);
                let count = match self.stack.pop() {
                    Some(JsValue::Number(n)) => n as usize,
                    _ => 0,
                };
                let args = self.pop_args(count);
                return self.call_super(super_ctor, args);
            }

            OpCode::GetPrivate(key) => {
                let target = self.stack.pop().unwrap_or(JsValue::Undefined);
                let getter_key = format!("getter:{key}");
                match target {
                    JsValue::Object(ptr) if self.has_prop_in_chain(ptr, &key) => {
                        let value = self.get_prop_with_proto_chain(ptr, &key);
                        self.stack.push(value);
                    }
                    JsValue::Object(ptr) if self.has_prop_in_chain(ptr, &getter_key) => {
                        let getter = self.get_prop_with_proto_chain(ptr, &getter_key);
                        match self.call_sync(&getter, JsValue::Object(ptr), Vec::new()) {
                            Ok(v) => self.stack.push(v),
                            Err(e) => return self.throw_exception(e),
                        }
                    }
                    _ => {
                        let name = errors::private_name(&key);
                        return self.throw_error(
                            "TypeError",
                            format!(
                                "Cannot read private member {name} from an object whose class did not declare it"
                            ),
                        );
                    }
                }
            }

            OpCode::SetPrivate(key) => {
                let value = self.stack.pop().unwrap_or(JsValue::Undefined);
                let target = self.stack.pop().unwrap_or(JsValue::Undefined);
                let owned = match target {
                    JsValue::Object(ptr) => match self.heap.get_mut(ptr) {
                        Some(HeapObject {
                            data: HeapData::Object(props),
                        }) if props.contains_key(&key) => {
                            props.insert(key.clone(), value);
                            true
                        }
                        _ => {
                            let setter_key = format!("setter:{key}");
                            if self.has_prop_in_chain(ptr, &setter_key) {
                                let setter = self.get_prop_with_proto_chain(ptr, &setter_key);
                                if let Err(e) =
                                    self.call_sync(&setter, JsValue::Object(ptr), vec![value])
                                {
                                    return self.throw_exception(e);
                                }
                                true
                            } else {
                                false
                            }
                        }
                    },
                    _ => false,
                };
                if !owned {
                    let name = errors::private_name(&key);
                    return self.throw_error(
                        "TypeError",
                        format!(
                            "Cannot write private member {name} to an object whose class did not declare it"
                        ),
                    );
                }
            }

            OpCode::In => {
                let target = self.stack.pop().unwrap_or(JsValue::Undefined);
                let key = self.stack.pop().unwrap_or(JsValue::Undefined);
                let key = self.to_js_string(&key);
                let found = match &target {
                    JsValue::Object(ptr) => match self.heap.get(*ptr).map(|o| &o.data) {
                        Some(HeapData::Array(items)) => {
                            key == "length" || key.parse::<usize>().is_ok_and(|i| i < items.len())
                        }
                        Some(HeapData::Object(_)) => {
                            self.has_prop_in_chain(*ptr, &key)
                                || self.has_prop_in_chain(*ptr, &format!("getter:{key}"))
                                || self.has_prop_in_chain(*ptr, &format!("setter:{key}"))
                        }
                        _ => false,
                    },
                    other => {
                        let shown = crate::stdlib::inspect(self, other);
                        return self.throw_error(
                            "TypeError",
                            format!("Cannot use 'in' operator to search for '{key}' in {shown}"),
                        );
                    }
                };
                self.stack.push(JsValue::Boolean(found));
            }

            OpCode::GetSuperProp(name) => {
                // Stack: [this, base]
                let base = self.stack.pop().unwrap_or(JsValue::Undefined);
                let this = self.stack.pop().unwrap_or(JsValue::Undefined);
                let JsValue::Object(ptr) = base else {
                    self.stack.push(JsValue::Undefined);
                    self.ip += 1;
                    return ExecResult::Continue;
                };
                let getter = self.get_prop_with_proto_chain(ptr, &format!("getter:{}", name));
                if let JsValue::Function { address, env } = getter {
                    self.push_call_args(&[], address);
                    self.call_stack.push(Frame {
                        return_address: self.ip + 1,
                        locals: HashMap::new(),
                        indexed_locals: Vec::new(),
                        this_context: this,
                        new_target: None,
                        super_called: false,
                        resume_ip: None,
                        env,
                    });
                    self.ip = address;
                    return ExecResult::ContinueNoIpInc;
                }
                let value = self.get_prop_with_proto_chain(ptr, &name);
                self.stack.push(value);
            }

            OpCode::CallWithThis(arg_count) => {
                // Stack: [args..., f, this]
                let this = self.stack.pop().unwrap_or(JsValue::Undefined);
                let callee = self.stack.pop().unwrap_or(JsValue::Undefined);
                let args = self.pop_args(arg_count);
                match callee {
                    JsValue::Function { address, env } => {
                        if self.call_stack.len() >= MAX_CALL_STACK_DEPTH {
                            return self
                                .throw_error("RangeError", "Maximum call stack size exceeded");
                        }
                        self.push_call_args(&args, address);
                        self.call_stack.push(Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context: this,
                            new_target: None,
                            super_called: false,
                            resume_ip: None,
                            env,
                        });
                        self.ip = address;
                        return ExecResult::ContinueNoIpInc;
                    }
                    JsValue::NativeFunction(idx) => {
                        let func = self.native_functions[idx];
                        let result = func(self, args);
                        if let Some(e) = self.native_throw.take() {
                            return self.throw_exception(e);
                        }
                        self.stack.push(result);
                    }
                    other => {
                        let shown = crate::stdlib::inspect(self, &other);
                        return self.throw_error("TypeError", format!("{shown} is not a function"));
                    }
                }
            }

            OpCode::InstanceOf => {
                // Stack: [constructor, object] -> pops both, pushes boolean
                let constructor = self.stack.pop().unwrap_or(JsValue::Undefined);
                let obj = self.stack.pop().unwrap_or(JsValue::Undefined);
                // `x instanceof Object` holds for every object.
                if self.call_stack[0].locals.get("Object") == Some(&constructor) {
                    let is_object = matches!(
                        obj,
                        JsValue::Object(_)
                            | JsValue::Promise(_)
                            | JsValue::Function { .. }
                            | JsValue::NativeFunction(_)
                    );
                    self.stack.push(JsValue::Boolean(is_object));
                    self.ip += 1;
                    return ExecResult::Continue;
                }

                let result = match (obj, constructor) {
                    // obj instanceof Function (special case: function is not an object)
                    (_, JsValue::Function { .. }) => {
                        // Functions always pass instanceof Function
                        // For other checks, we'd need function.prototype
                        true
                    }
                    (JsValue::Object(obj_ptr), JsValue::Object(ctor_ptr)) => {
                        // Get constructor.prototype
                        let proto = if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(ctor_ptr)
                        {
                            props.get("prototype").cloned()
                        } else {
                            None
                        };

                        // Walk the object's prototype chain looking for constructor.prototype
                        if let Some(JsValue::Object(target_proto)) = proto {
                            let mut current_ptr = Some(obj_ptr);
                            let mut depth = 0;
                            const MAX_PROTO_DEPTH: usize = 100;

                            while let Some(ptr) = current_ptr {
                                if depth > MAX_PROTO_DEPTH {
                                    break;
                                }
                                depth += 1;

                                if ptr == target_proto {
                                    break;
                                }

                                if let Some(HeapObject {
                                    data: HeapData::Object(props),
                                }) = self.heap.get(ptr)
                                {
                                    if let Some(JsValue::Object(proto_ptr)) = props.get("__proto__")
                                    {
                                        current_ptr = Some(*proto_ptr);
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }

                            // Check if we found the prototype
                            current_ptr == Some(target_proto)
                        } else {
                            false
                        }
                    }
                    _ => false,
                };

                self.stack.push(JsValue::Boolean(result));
            }

            OpCode::NewTarget => {
                // Push the new.target value from the current frame
                let new_target = self
                    .call_stack
                    .last()
                    .and_then(|frame| frame.new_target.clone())
                    .unwrap_or(JsValue::Undefined);
                self.stack.push(new_target);
            }

            OpCode::ImportAsync(_specifier) => {
                let specifier_str = match self.stack.pop() {
                    Some(JsValue::String(s)) => s,
                    Some(_) => {
                        self.stack.push(JsValue::Undefined);
                        return ExecResult::Continue;
                    }
                    None => {
                        self.stack.push(JsValue::Undefined);
                        return ExecResult::Continue;
                    }
                };

                let importer_path = self.current_module_path.clone();

                let resolved_path = {
                    let importer_dir = importer_path
                        .as_ref()
                        .and_then(|p| {
                            if p.is_file() {
                                p.parent()
                            } else if p.exists() && p.is_dir() {
                                Some(p)
                            } else {
                                Some(p)
                            }
                        })
                        .map(|p| {
                            if p.as_os_str().is_empty() {
                                Path::new(".")
                            } else {
                                p
                            }
                        })
                        .unwrap_or(Path::new("."));

                    let mut resolved = importer_dir.to_path_buf();

                    for component in specifier_str.split('/') {
                        match component {
                            "." => {}
                            ".." => {
                                if !resolved.as_os_str().is_empty() {
                                    resolved.pop();
                                }
                            }
                            "" if specifier_str.starts_with("./") => {}
                            "" if specifier_str.starts_with("../") => {}
                            _ => resolved.push(component),
                        };
                    }

                    let extensions = ["ot", "ts", "js"];
                    if resolved.as_os_str().is_empty() || specifier_str.ends_with('/') {
                        for ext in &extensions {
                            let index_path = resolved.join("index").with_extension(ext);
                            if index_path.exists() {
                                resolved = index_path;
                                break;
                            }
                        }
                    } else if !resolved.exists() {
                        for ext in &extensions {
                            let with_ext = resolved.with_extension(ext);
                            if with_ext.exists() {
                                resolved = with_ext;
                                break;
                            }
                        }
                    }
                    resolved
                };

                if !resolved_path.exists() {
                    eprintln!("Error: Module not found: {}", specifier_str);
                    self.stack.push(JsValue::Undefined);
                    return ExecResult::Continue;
                }

                // Check cache first
                let canonical_path = match fs::canonicalize(&resolved_path) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Error canonicalizing path: {}", e);
                        self.stack.push(JsValue::Undefined);
                        return ExecResult::Continue;
                    }
                };

                // Check if we have a valid cached version
                if let Some(cached) = self.module_cache.get(&canonical_path) {
                    // Cache hit - return cached namespace object
                    self.stack.push(JsValue::Object(cached.namespace_object));
                    // Fall through to ip += 1 at end of exec_one
                } else {
                    // Cache miss - load the module
                    let result = fs::read_to_string(&canonical_path)
                        .map_err(|e| format!("Failed to read module: {}", e));

                    match result {
                        Ok(source) => {
                            let hash = ModuleCache::compute_hash(&canonical_path);
                            let exports =
                                parse_module_exports(&source, &canonical_path.to_string_lossy());
                            let export_names: Vec<String> = exports.keys().cloned().collect();
                            let mut namespace_props = HashMap::new();
                            namespace_props.insert(
                                "__path__".to_string(),
                                JsValue::String(canonical_path.to_string_lossy().into_owned()),
                            );
                            namespace_props
                                .insert("__source__".to_string(), JsValue::String(source.clone()));
                            namespace_props
                                .insert("__hash__".to_string(), JsValue::String(hash.clone()));
                            match self.execute_module(&source, &canonical_path, &export_names) {
                                Ok(module_exports) => {
                                    for (name, value) in module_exports {
                                        namespace_props.insert(name, value);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Error executing module '{}': {}", specifier_str, e);
                                    for name in &export_names {
                                        namespace_props.insert(name.clone(), JsValue::Undefined);
                                    }
                                }
                            }
                            let namespace_ptr = self.heap.len();
                            self.heap.push(HeapObject {
                                data: HeapData::Object(namespace_props),
                            });
                            let cached_module = CachedModule {
                                path: canonical_path.clone(),
                                source,
                                hash,
                                load_time: std::time::SystemTime::now(),
                                namespace_object: namespace_ptr,
                            };
                            self.module_cache.insert(cached_module);
                            self.stack.push(JsValue::Object(namespace_ptr));
                        }
                        Err(e) => {
                            eprintln!("Error loading module '{}': {}", specifier_str, e);
                            self.stack.push(JsValue::Undefined);
                        }
                    }
                }
            }

            OpCode::Await => {
                // Stack: [value] -> [result]. Suspends the async function
                // until the value (as a promise) settles.
                let value = self.stack.pop().unwrap_or(JsValue::Undefined);
                let base = match self
                    .call_stack
                    .last()
                    .and_then(|f| f.locals.get(promise::STACK_BASE_LOCAL))
                {
                    Some(JsValue::Number(n)) => *n as usize,
                    // Not in an async function (top-level await): run
                    // microtasks until the promise settles.
                    _ => {
                        let p = self.to_promise(value);
                        self.run_microtasks();
                        match p.get_state() {
                            PromiseState::Rejected => {
                                let reason = p.get_value().unwrap_or(JsValue::Undefined);
                                return self.throw_exception(reason);
                            }
                            _ => self.stack.push(p.get_value().unwrap_or(JsValue::Undefined)),
                        }
                        self.ip += 1;
                        return ExecResult::Continue;
                    }
                };
                let p = self.to_promise(value);
                let frame = self.call_stack.pop().expect("await outside a frame");
                let depth = self.call_stack.len();
                let stack = self.stack.split_off(base.min(self.stack.len()));
                let mut handlers = Vec::new();
                while let Some(h) = self.exception_handlers.last() {
                    if h.call_stack_depth <= depth {
                        break;
                    }
                    let mut h = self.exception_handlers.pop().unwrap();
                    h.stack_depth = h.stack_depth.saturating_sub(base);
                    h.call_stack_depth -= depth;
                    handlers.push(h);
                }
                handlers.reverse();
                let own_promise = frame
                    .locals
                    .get(promise::PROMISE_LOCAL)
                    .cloned()
                    .unwrap_or(JsValue::Undefined);
                let return_address = frame.return_address;
                let id = self.next_async_id;
                self.next_async_id += 1;
                self.suspended.insert(
                    id,
                    promise::Suspended {
                        frame,
                        stack,
                        handlers,
                        resume_ip: self.ip + 1,
                    },
                );
                self.add_reaction(&p, value::Reaction::Resume(id));
                // Like `return`: the caller gets the function's promise.
                self.stack.push(own_promise);
                self.ip = return_address;
                if self.ip == usize::MAX {
                    return ExecResult::Stop;
                }
                return ExecResult::ContinueNoIpInc;
            }

            OpCode::AsyncStart => {
                // An async function's promise; its stack part starts here.
                let base = self.stack.len();
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.locals.insert(
                        promise::STACK_BASE_LOCAL.to_string(),
                        JsValue::Number(base as f64),
                    );
                }
                self.stack.push(JsValue::Promise(Promise::new()));
            }

            OpCode::AsyncResolve => {
                // [value, promise] -> [promise]
                let p = self.stack.pop().unwrap_or(JsValue::Undefined);
                let value = self.stack.pop().unwrap_or(JsValue::Undefined);
                if let JsValue::Promise(promise) = &p {
                    self.resolve_promise(promise, value);
                }
                self.stack.push(p);
            }

            OpCode::AsyncReject => {
                // [reason, promise] -> [promise]
                let p = self.stack.pop().unwrap_or(JsValue::Undefined);
                let reason = self.stack.pop().unwrap_or(JsValue::Undefined);
                if let JsValue::Promise(promise) = &p {
                    self.settle_promise(promise, reason, false);
                }
                self.stack.push(p);
            }

            OpCode::GetExport {
                name,
                is_default: _,
            } => {
                let namespace = match self.stack.pop() {
                    Some(JsValue::Object(ptr)) => {
                        if let Some(HeapObject {
                            data: HeapData::Object(props),
                            ..
                        }) = self.heap.get(ptr)
                        {
                            props.clone()
                        } else {
                            HashMap::new()
                        }
                    }
                    Some(_) => {
                        self.stack.push(JsValue::Undefined);
                        return ExecResult::Continue;
                    }
                    None => {
                        self.stack.push(JsValue::Undefined);
                        return ExecResult::Continue;
                    }
                };

                let export_value = namespace.get(&name).cloned().unwrap_or(JsValue::Undefined);
                self.stack.push(export_value);
            }

            OpCode::ModuleResolutionError {
                message,
                specifier,
                importer,
                dependency_chain: _,
            } => {
                let _specifier = self.stack.pop().unwrap_or(JsValue::Undefined);
                let _importer = self.stack.pop().unwrap_or(JsValue::Undefined);
                eprintln!(
                    "Error: Module resolution error: {} (trying to import {} from {})",
                    message, specifier, importer
                );
                self.stack.push(JsValue::String(format!(
                    "ModuleResolutionError: {} (importing {} from {})",
                    message, specifier, importer
                )));
            }
        }

        self.ip += 1;
        ExecResult::Continue
    }
    #[allow(dead_code)]
    fn native_write_bytecode_file(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        if let Some(JsValue::String(path)) = args.first() {
            match std::fs::write(
                path,
                vm.program
                    .iter()
                    .map(|op| format!("{:?}", op))
                    .collect::<Vec<String>>()
                    .join("\n")
                    .as_bytes(),
            ) {
                Ok(_) => JsValue::Undefined,
                Err(e) => JsValue::String(format!("Error writing bytecode file: {}", e)),
            }
        } else {
            JsValue::Undefined
        }
    }
}

/// The VM's value representation, for the shared built-in methods.
pub(crate) struct VmHost<'a>(pub(crate) &'a Vec<HeapObject>);

impl crate::runtime::builtins::Host for VmHost<'_> {
    type Value = JsValue;

    fn number(&self, v: &JsValue) -> Option<f64> {
        match v {
            JsValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn string<'a>(&'a self, v: &'a JsValue) -> Option<&'a str> {
        match v {
            JsValue::String(s) => Some(s),
            _ => None,
        }
    }

    fn to_string(&self, v: &JsValue) -> String {
        match v {
            JsValue::String(s) => s.clone(),
            JsValue::Number(n) => crate::stdlib::number_to_string(n),
            JsValue::Boolean(b) => b.to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Object(ptr) => match self.0.get(*ptr) {
                Some(HeapObject {
                    data: HeapData::Array(items),
                }) => items
                    .iter()
                    .map(|e| match e {
                        JsValue::Null | JsValue::Undefined => String::new(),
                        other => self.to_string(other),
                    })
                    .collect::<Vec<_>>()
                    .join(","),
                _ => "[object Object]".to_string(),
            },
            JsValue::Function { .. } | JsValue::NativeFunction(_) => "function".to_string(),
            JsValue::Promise(_) => "[object Promise]".to_string(),
            JsValue::Accessor(_, _) => String::new(),
        }
    }

    fn strict_equal(&self, a: &JsValue, b: &JsValue) -> bool {
        match (a, b) {
            (JsValue::Function { address: x, .. }, JsValue::Function { address: y, .. }) => x == y,
            _ => a == b,
        }
    }

    fn array_elements(&self, v: &JsValue) -> Option<Vec<JsValue>> {
        match v {
            JsValue::Object(ptr) => match self.0.get(*ptr) {
                Some(HeapObject {
                    data: HeapData::Array(items),
                }) => Some(items.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    fn is_array(&self, v: &JsValue) -> bool {
        matches!(
            v,
            JsValue::Object(ptr) if matches!(
                self.0.get(*ptr),
                Some(HeapObject { data: HeapData::Array(_) })
            )
        )
    }

    fn own_keys(&self, v: &JsValue) -> Vec<String> {
        let JsValue::Object(ptr) = v else {
            return Vec::new();
        };
        match self.0.get(*ptr) {
            Some(HeapObject {
                data: HeapData::Object(props),
            }) => {
                let mut names: Vec<String> = Vec::new();
                for k in props.keys() {
                    if let Some(name) = crate::runtime::builtins::own_key_name(k)
                        && !names.iter().any(|n| n == name)
                    {
                        names.push(name.to_string());
                    }
                }
                names
            }
            Some(HeapObject {
                data: HeapData::Array(items),
            }) => (0..items.len()).map(|i| i.to_string()).collect(),
            _ => Vec::new(),
        }
    }

    fn make_number(&self, n: f64) -> JsValue {
        JsValue::Number(n)
    }

    fn make_string(&self, s: String) -> JsValue {
        JsValue::String(s)
    }

    fn make_bool(&self, b: bool) -> JsValue {
        JsValue::Boolean(b)
    }

    fn undefined(&self) -> JsValue {
        JsValue::Undefined
    }
}

/// JavaScript `ToInt32`: the integer the bitwise operators work on.
/// `catch_addr` of the exception handler a `call_sync` installs.
const CALL_SYNC_BARRIER: usize = usize::MAX;

/// `bytecode` moved to start at `offset`: every instruction address in it
/// (jumps, functions, handlers) shifted by `offset`.
pub fn rebase_program(bytecode: Vec<OpCode>, offset: usize) -> Vec<OpCode> {
    let shift = |addr: usize| if addr != 0 { addr + offset } else { 0 };
    bytecode
        .into_iter()
        .map(|op| match op {
            OpCode::Jump(addr) => OpCode::Jump(addr + offset),
            OpCode::JumpIfFalse(addr) => OpCode::JumpIfFalse(addr + offset),
            OpCode::MakeClosure(addr) => OpCode::MakeClosure(addr + offset),
            OpCode::Push(JsValue::Function { address, env }) => OpCode::Push(JsValue::Function {
                address: address + offset,
                env,
            }),
            // 0 means "none".
            OpCode::SetupTry {
                catch_addr,
                finally_addr,
            } => OpCode::SetupTry {
                catch_addr: shift(catch_addr),
                finally_addr: shift(finally_addr),
            },
            other => other,
        })
        .collect()
}

/// The `typeof` of a value.
fn js_typeof(v: &JsValue) -> &'static str {
    match v {
        JsValue::Number(_) => "number",
        JsValue::String(_) => "string",
        JsValue::Boolean(_) => "boolean",
        JsValue::Object(_) => "object",
        JsValue::Function { .. } => "function",
        JsValue::NativeFunction(_) => "function",
        JsValue::Null => "object", // typeof null === "object" in JS
        JsValue::Undefined => "undefined",
        JsValue::Accessor(_, _) => "function",
        JsValue::Promise(_) => "object",
    }
}

/// JavaScript `ToInt32`: the integer part, modulo 2^32.
fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    n.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecResult {
    Continue,
    ContinueNoIpInc,
    Stop,
}
