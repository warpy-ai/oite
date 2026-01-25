/// Maximum call stack depth to prevent stack overflow in deeply recursive code
pub const MAX_CALL_STACK_DEPTH: usize = 1000;

pub mod opcodes;
pub mod value;
pub mod module_cache;
pub mod property;
pub mod stdlib_setup;

pub use crate::compiler::Compiler;
pub use crate::vm::opcodes::OpCode;
pub use crate::vm::value::HeapData;
pub use crate::vm::value::HeapObject;
pub use crate::vm::value::JsValue;
pub use crate::vm::value::NativeFn;
pub use crate::vm::value::Promise;
pub use crate::vm::value::PromiseState;
pub use crate::vm::value::AsyncContext;
pub use crate::vm::value::ContinuationCallback;
pub use crate::vm::module_cache::CachedModule;
pub use crate::vm::module_cache::ModuleCache;
pub use sha2::{Digest, Sha256};
pub use std::collections::{HashMap, VecDeque};
pub use std::fs;
pub use std::path::{Path, PathBuf};
pub use std::sync::{Arc, Mutex};
pub use std::time::{Duration, Instant, SystemTime};
pub use swc_common::{input::StringInput, BytePos, FileName};
pub use swc_ecma_parser::{lexer::Lexer, Parser, Syntax, TsSyntax};
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
                                                    let s: &str = &*atom;
                                                    s.to_string()
                                                })
                                                .unwrap_or_else(|| {
                                                    let atom = named.orig.atom();
                                                    let s: &str = &*atom;
                                                    s.to_string()
                                                });
                                            exports.insert(export_name, JsValue::Undefined);
                                        }
                                        swc_ecma_ast::ExportSpecifier::Default(_) => {
                                            exports.insert("default".to_string(), JsValue::Undefined);
                                        }
                                        swc_ecma_ast::ExportSpecifier::Namespace(ns) => {
                                            let atom = ns.name.atom();
                                            let s: &str = &*atom;
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
                                                    let s: &str = &*atom;
                                                    s.to_string()
                                                })
                                                .unwrap_or_else(|| {
                                                    let atom = named.orig.atom();
                                                    let s: &str = &*atom;
                                                    s.to_string()
                                                });
                                            exports.insert(export_name, JsValue::Undefined);
                                        }
                                        swc_ecma_ast::ExportSpecifier::Default(_) => {
                                            exports.insert("default".to_string(), JsValue::Undefined);
                                        }
                                        swc_ecma_ast::ExportSpecifier::Namespace(ns) => {
                                            let atom = ns.name.atom();
                                            let s: &str = &*atom;
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
                                    exports.insert(fn_decl.ident.sym.to_string(), JsValue::Undefined);
                                }
                                Var(var_decl) => {
                                    for declarator in &var_decl.decls {
                                        if let swc_ecma_ast::Pat::Ident(ident) = &declarator.name {
                                            exports.insert(ident.id.sym.to_string(), JsValue::Undefined);
                                        }
                                    }
                                }
                                Class(class_decl) => {
                                    exports.insert(class_decl.ident.sym.to_string(), JsValue::Undefined);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                } else if let swc_ecma_ast::ModuleItem::Stmt(stmt) = item {
                    if let swc_ecma_ast::Stmt::Decl(decl) = stmt {
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
}

pub struct Task {
    pub function_ptr: JsValue,
    pub args: Vec<JsValue>,
}

pub struct TimerTask {
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
    pub current_module_path: Option<PathBuf>,
    pub async_runtime: Option<Runtime>,
    pub async_task_tx: Option<mpsc::Sender<JsValue>>,
    pub module_cache: ModuleCache,
    pub compiler: Compiler,
    /// Async/await continuation state
    pub async_context: Option<AsyncContext>,
    /// Queue for resolved promise values to be processed
    pub resolved_queue: Vec<(ContinuationCallback, JsValue)>,
    /// Current promise being constructed (for resolve/reject callbacks)
    pub current_promise: Option<Promise>,
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
                return_address: usize::MAX,  // Set to MAX so global return stops execution
                locals: HashMap::new(),
                indexed_locals: Vec::new(),
                this_context: JsValue::Undefined,
                new_target: None,
                super_called: false,
                resume_ip: None,
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
            current_module_path: None,
            async_runtime: None,
            async_task_tx: Some(tx),
            module_cache: ModuleCache::new(),
            compiler: Compiler::new(),
            async_context: None,
            resolved_queue: Vec::new(),
            current_promise: None,
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
                cached.load_time.elapsed()
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
            .map_err(|e| format!("Failed to compile module {}: {}", path.display(), e))?;

        // Save IP BEFORE appending program, because append_program modifies IP
        let saved_ip = self.ip;
        let saved_module_path = self.current_module_path.clone();

        let start_offset = self.append_program(bytecode);
        let end_offset = self.program.len();

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

    /// Poll a promise until it's resolved (synchronous wait)
    /// Returns the resolved value or undefined if timeout/error
    pub fn poll_promise(&mut self, promise: &Promise, timeout_ms: u64) -> JsValue {
        let start = std::time::Instant::now();
        let sleep_duration = std::time::Duration::from_millis(1);

        loop {
            match promise.get_state() {
                PromiseState::Fulfilled => {
                    let value = promise.get_value().unwrap_or(JsValue::Undefined);
                    eprintln!("DEBUG poll_promise: fulfilled, value = {:?}", value);
                    return value;
                }
                PromiseState::Rejected => {
                    let value = promise.get_value().unwrap_or(JsValue::Undefined);
                    eprintln!("DEBUG poll_promise: rejected, value = {:?}", value);
                    return value;
                }
                PromiseState::Pending => {
                    let elapsed = start.elapsed().as_millis();
                    if elapsed > timeout_ms as u128 {
                        eprintln!("DEBUG poll_promise: timeout after {}ms", elapsed);
                        return JsValue::Undefined;
                    }
                    // Brief sleep to avoid busy-waiting
                    std::thread::sleep(sleep_duration);
                }
            }
        }
    }

    /// Register a callback to be invoked when a promise resolves
    pub fn register_promise_callback(
        &mut self,
        promise: &Promise,
        callback: Box<dyn FnOnce(JsValue) + Send>,
    ) {
        let promise = promise.clone();
        // Spawn a thread to wait for the promise
        std::thread::spawn(move || {
            let sleep_duration = std::time::Duration::from_millis(1);
            loop {
                match promise.get_state() {
                    PromiseState::Fulfilled => {
                        let value = promise.get_value().unwrap_or(JsValue::Undefined);
                        callback(value);
                        break;
                    }
                    PromiseState::Rejected => {
                        let value = promise.get_value().unwrap_or(JsValue::Undefined);
                        callback(value);
                        break;
                    }
                    PromiseState::Pending => {
                        std::thread::sleep(sleep_duration);
                    }
                }
            }
        });
    }

    /// Check if we're currently in an async context (has saved continuation)
    pub fn is_in_async_context(&self) -> bool {
        self.async_context.is_some()
    }

    /// Get the current async context if any
    pub fn get_async_context(&self) -> Option<&AsyncContext> {
        self.async_context.as_ref()
    }

    /// Set the async context (for awaiting)
    pub fn set_async_context(&mut self, context: Option<AsyncContext>) {
        self.async_context = context;
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

    pub fn schedule_timer(&mut self, callback: JsValue, delay_ms: u64) {
        self.timers.push(TimerTask {
            due: Instant::now() + Duration::from_millis(delay_ms),
            task: Task {
                function_ptr: callback,
                args: vec![],
            },
        });
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

        // Rebase all address-containing instructions
        for op in bytecode {
            let rebased_op = match op {
                OpCode::Jump(addr) => OpCode::Jump(addr + start_offset),
                OpCode::JumpIfFalse(addr) => OpCode::JumpIfFalse(addr + start_offset),
                OpCode::MakeClosure(addr) => OpCode::MakeClosure(addr + start_offset),
                OpCode::Push(JsValue::Function { address, env }) => {
                    OpCode::Push(JsValue::Function {
                        address: address + start_offset,
                        env,
                    })
                }
                OpCode::SetupTry {
                    catch_addr,
                    finally_addr,
                } => OpCode::SetupTry {
                    catch_addr: if catch_addr != 0 {
                        catch_addr + start_offset
                    } else {
                        0
                    },
                    finally_addr: if finally_addr != 0 {
                        finally_addr + start_offset
                    } else {
                        0
                    },
                },
                other => other,
            };
            self.program.push(rebased_op);
        }

        self.ip = start_offset;
        start_offset
    }

    pub fn run_event_loop(&mut self) {
        // 1) Run the initial script to completion.
        self.run_until_halt();

        // 2) Drain the event loop: timers -> task queue -> execute task.
        loop {
            self.pump_timers();

            if let Some(task) = self.task_queue.pop_front() {
                self.execute_task(task);
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

    fn execute_task(&mut self, task: Task) {
        // Stack overflow protection
        if self.call_stack.len() >= MAX_CALL_STACK_DEPTH {
            panic!(
                "Stack overflow: maximum call depth of {} exceeded",
                MAX_CALL_STACK_DEPTH
            );
        }

        match task.function_ptr {
            JsValue::Function { address, env } => {
                // Push args in call order so the function prologue `Store(...)` consumes correctly.
                for arg in task.args {
                    self.stack.push(arg);
                }

                let mut frame = Frame {
                    return_address: usize::MAX, // sentinel: stop when returning
                    locals: HashMap::new(),
                    indexed_locals: Vec::new(),
                    this_context: JsValue::Undefined,
                    new_target: None,
                    super_called: false,
                    resume_ip: None,
                };

                // CLOSURE MAGIC: If this function has captured variables (env),
                // load them into the new frame's locals. This is the key to
                // surviving the Stack Frame Paradox!
                if let Some(HeapObject {
                    data: HeapData::Object(props),
                }) = env.and_then(|ptr| self.heap.get(ptr))
                {
                    for (name, value) in props {
                        frame.locals.insert(name.clone(), value.clone());
                    }
                }

                self.call_stack.push(frame);
                self.ip = address;
                self.run_until_return_sentinel();
            }

            JsValue::NativeFunction(idx) => {
                let func = self.native_functions[idx];
                let _ = func(self, task.args);
            }

            _ => panic!("Target is not callable"),
        }
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
                if let JsValue::Object(proto_ptr) = proto {
                    if let Some(heap_item) = self.heap.get_mut(ptr) {
                        if let HeapData::Object(props) = &mut heap_item.data {
                            props.insert("__proto__".to_string(), JsValue::Object(proto_ptr));
                        }
                    }
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
                        self.stack.push(value.clone());
                        let this_context = JsValue::Object(ptr);
                        let mut frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context,
                            new_target: None,
                            super_called: false,
                            resume_ip: None,
                        };

                        if let Some(HeapObject {
                            data: HeapData::Object(env_props),
                        }) = env.and_then(|ptr| self.heap.get(ptr))
                        {
                            for (n, v) in env_props {
                                frame.locals.insert(n.clone(), v.clone());
                            }
                        }

                        self.call_stack.push(frame);
                        self.ip = address;
                        return ExecResult::ContinueNoIpInc;
                    }

                    // No setter found, store the value directly
                    if let Some(heap_item) = self.heap.get_mut(ptr) {
                        if let HeapData::Object(props) = &mut heap_item.data {
                            props.insert(name.to_string(), value);
                        }
                    }
                } else {
                    // Object was not an Object, silently ignore or could panic
                }
            }

            OpCode::SetPropComputed => {
                // Pops [obj, value, key] -> sets obj[key] = value
                let key_val = self.stack.pop().unwrap();
                let value = self.stack.pop().unwrap();
                let target = self.stack.pop().unwrap();

                if let JsValue::Object(ptr) = target {
                    // Convert key to string
                    let key_name = match &key_val {
                        JsValue::String(s) => s.clone(),
                        JsValue::Number(n) => n.to_string(),
                        JsValue::Object(_) => {
                            // For objects, use default string representation
                            format!("[object Object]")
                        }
                        _ => format!("{:?}", key_val),
                    };

                    if let Some(heap_item) = self.heap.get_mut(ptr) {
                        if let HeapData::Object(props) = &mut heap_item.data {
                            props.insert(key_name, value);
                        }
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

                match (target, key_val) {
                    (JsValue::Object(ptr), JsValue::Number(idx)) => {
                        // Array access: arr[index]
                        if let Some(heap_obj) = self.heap.get(ptr) {
                            if let HeapData::Array(arr) = &heap_obj.data {
                                let i = idx as usize;
                                let val = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                                self.stack.push(val.clone());
                                self.ip += 1;
                                return ExecResult::Continue;
                            }
                        }
                        self.stack.push(JsValue::Undefined);
                    }
                    (JsValue::Object(ptr), key_val) => {
                        // Object access: obj[key]
                        // Convert key to string
                        let key_name = match &key_val {
                            JsValue::String(s) => s.clone(),
                            JsValue::Number(n) => n.to_string(),
                            JsValue::Object(_) => {
                                format!("[object Object]")
                            }
                            _ => format!("{:?}", key_val),
                        };

                        // Look up the property with prototype chain
                        let value = self.get_prop_with_proto_chain(ptr, &key_name);
                        self.stack.push(value.clone());
                    }
                    (JsValue::String(s), JsValue::Number(idx)) => {
                        // String char access: str[index]
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

            OpCode::GetProp(name) => {
                let target = self.stack.pop();

                match target {
                    Some(JsValue::Object(ptr)) => {
                        if let Some(heap_item) = self.heap.get(ptr) {
                            match &heap_item.data {
                                HeapData::Object(props) => {
                                    let getter_name = format!("getter:{}", name);
                                    let val = self.get_prop_with_proto_chain(ptr, &getter_name);

                                    if let JsValue::Function { address, env } = val {
                                        let this_context = JsValue::Object(ptr);

                                        let mut frame = Frame {
                                            return_address: self.ip + 1,
                                            locals: HashMap::new(),
                                            indexed_locals: Vec::new(),
                                            this_context,
                                            new_target: None,
                                            super_called: false,
                                            resume_ip: None,
                                        };

                                        if let Some(HeapObject {
                                            data: HeapData::Object(env_props),
                                        }) = env.and_then(|ptr| self.heap.get(ptr))
                                        {
                                            for (n, v) in env_props {
                                                frame.locals.insert(n.clone(), v.clone());
                                            }
                                        }

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
                            }
                        } else {
                            self.stack.push(JsValue::Undefined);
                        }
                    }
                    // Special case: looking up .prototype on a function value
                    Some(JsValue::Function { address, env }) if name == "prototype" => {
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
                    _ => {
                        // For any other type, push undefined
                        self.stack.push(JsValue::Undefined);
                    }
                }
            }

            OpCode::Push(v) => self.stack.push(v),

            OpCode::Let(name) => {
                // Create a new binding in the CURRENT frame only (let declaration)
                // This shadows any outer variable with the same name
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                if self.call_stack.is_empty() {
                    eprintln!("ERROR: Let opcode with empty call_stack at ip={}", self.ip);
                    eprintln!("Stack depth: {}", self.stack.len());
                    return ExecResult::Stop;
                }
                self.call_stack.last_mut().unwrap().locals.insert(name, val);
            }

            OpCode::Store(name) => {
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                // Assign to an existing binding if found, otherwise create in current frame.
                let mut stored = false;
                for frame in self.call_stack.iter_mut().rev() {
                    if frame.locals.contains_key(&name) {
                        frame.locals.insert(name.clone(), val.clone());
                        stored = true;
                        break;
                    }
                }
                if !stored {
                    self.call_stack.last_mut().unwrap().locals.insert(name, val);
                }
            }

            OpCode::Load(name) => {
                // Search for variable from innermost to outermost frame.
                let mut found = None;
                for frame in self.call_stack.iter().rev() {
                    if let Some(v) = frame.locals.get(&name) {
                        found = Some(v.clone());
                        break;
                    }
                }
                let value = found.unwrap_or(JsValue::Undefined);
                self.stack.push(value);
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
                    panic!(
                        "Stack overflow: maximum call depth of {} exceeded",
                        MAX_CALL_STACK_DEPTH
                    );
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

                        for arg in &args {
                            self.stack.push(arg.clone());
                        }

                        let mut frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context: JsValue::Undefined,
                            new_target: None,
                            super_called: false,
                            resume_ip: None,
                        };

                        // CLOSURE CONTEXT SWITCH: Load captured variables from
                        // the environment heap object into the new frame's locals.
                        // This makes them available to the function body.
                        if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = env.and_then(|ptr| self.heap.get(ptr))
                        {
                            for (name, value) in props {
                                frame.locals.insert(name.clone(), value.clone());
                            }
                        }

                        self.call_stack.push(frame);
                        self.ip = address;
                        return ExecResult::ContinueNoIpInc;
                    }
                    JsValue::NativeFunction(idx) => {
                        args.reverse();
                        let func = self.native_functions[idx];
                        let result = func(self, args);
                        self.stack.push(result);
                    }
                    JsValue::Object(ptr) => {
                        // Check if object has a __call__ property (callable object like String)
                        if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(ptr)
                        {
                            if let Some(JsValue::NativeFunction(idx)) = props.get("__call__") {
                                let idx = *idx;
                                args.reverse();
                                let func = self.native_functions[idx];
                                let result = func(self, args);
                                self.stack.push(result);
                            } else if let Some(JsValue::Function { address, env }) = props.get("__call__") {
                                let address = *address;
                                let env = *env;
                                for arg in &args {
                                    self.stack.push(arg.clone());
                                }
                                let mut frame = Frame {
                                    return_address: self.ip + 1,
                                    locals: HashMap::new(),
                                    indexed_locals: Vec::new(),
                                    this_context: JsValue::Object(ptr),
                                    new_target: None,
                                    super_called: false,
                                    resume_ip: None,
                                };
                                if let Some(HeapObject {
                                    data: HeapData::Object(env_props),
                                }) = env.and_then(|ptr| self.heap.get(ptr))
                                {
                                    for (name, value) in env_props {
                                        frame.locals.insert(name.clone(), value.clone());
                                    }
                                }
                                self.call_stack.push(frame);
                                self.ip = address;
                                return ExecResult::ContinueNoIpInc;
                            } else {
                                panic!("Object is not callable (no __call__ property): Object({})", ptr);
                            }
                        } else {
                            panic!("Object reference invalid: Object({})", ptr);
                        }
                    }
                    other => {
                        // Print the last few instructions for context
                        let start = if self.ip > 5 { self.ip - 5 } else { 0 };
                        let end = (self.ip + 3).min(self.program.len());
                        eprintln!("Context around ip={}:", self.ip);
                        for i in start..end {
                            let marker = if i == self.ip { ">>>" } else { "   " };
                            eprintln!("{} {}: {:?}", marker, i, self.program.get(i));
                        }
                        panic!("Target is not callable: {:?} at ip={}, call_stack_depth={}", other, self.ip, self.call_stack.len());
                    }
                }
            }

            OpCode::Return => {
                if self.call_stack.is_empty() {
                    return ExecResult::Stop;
                }
                let frame = self.call_stack.pop().expect("Missing frame");
                self.ip = frame.return_address;
                if self.ip == usize::MAX {
                    return ExecResult::Stop;
                }
                return ExecResult::ContinueNoIpInc;
            }

            OpCode::Drop(name) => {
                if let Some(JsValue::Object(ptr)) =
                    self.call_stack.last_mut().unwrap().locals.remove(&name)
                    && let Some(HeapObject {
                        data: HeapData::Object(props),
                    }) = self.heap.get_mut(ptr)
                {
                    props.clear();
                }
            }

            OpCode::Add => {
                let b = self.stack.pop().unwrap();
                let a = self.stack.pop().unwrap();

                match (a, b) {
                    (JsValue::Number(a_num), JsValue::Number(b_num)) => {
                        self.stack.push(JsValue::Number(a_num + b_num));
                    }
                    (JsValue::String(mut a_str), JsValue::String(b_str)) => {
                        a_str.push_str(&b_str);
                        self.stack.push(JsValue::String(a_str));
                    }
                    (JsValue::String(a_str), b) => {
                        let b_str = match b {
                            JsValue::Number(n) => n.to_string(),
                            JsValue::Boolean(b) => b.to_string(),
                            JsValue::Null => "null".to_string(),
                            JsValue::Undefined => "undefined".to_string(),
                            JsValue::String(s) => s,
                            JsValue::Object(ptr) => format!("Object({})", ptr),
                            JsValue::Function { address, env } => {
                                format!("Function({})", address)
                            }
                            JsValue::NativeFunction(idx) => {
                                format!("NativeFunction({})", idx)
                            }
                            _ => "".to_string(),
                        };
                        self.stack.push(JsValue::String(a_str + &b_str[..]));
                    }
                    (a, JsValue::String(b_str)) => {
                        let a_str = match a {
                            JsValue::Number(n) => n.to_string(),
                            JsValue::Boolean(b) => b.to_string(),
                            JsValue::Null => "null".to_string(),
                            JsValue::Undefined => "undefined".to_string(),
                            JsValue::String(s) => s,
                            JsValue::Object(ptr) => format!("Object({})", ptr),
                            JsValue::Function { address, env } => {
                                format!("Function({})", address)
                            }
                            JsValue::NativeFunction(idx) => {
                                format!("NativeFunction({})", idx)
                            }
                            _ => "".to_string(),
                        };
                        self.stack.push(JsValue::String(a_str + &b_str[..]));
                    }
                    _ => {
                        self.stack.push(JsValue::Undefined);
                    }
                }
            }
            OpCode::And => {
                let b = self.stack.pop().unwrap();
                let a = self.stack.pop().unwrap();
                // Logical AND: both must be truthy
                let a_truthy = match a {
                    JsValue::Boolean(false) | JsValue::Null | JsValue::Undefined => false,
                    JsValue::Number(n) => n != 0.0,
                    _ => true, // Strings, objects, functions are truthy
                };
                let b_truthy = match b {
                    JsValue::Boolean(false) | JsValue::Null | JsValue::Undefined => false,
                    JsValue::Number(n) => n != 0.0,
                    _ => true,
                };
                self.stack.push(JsValue::Boolean(a_truthy && b_truthy));
            }

            OpCode::Or => {
                let b = self.stack.pop().expect("Missing right operand for ||");
                let a = self.stack.pop().expect("Missing left operand for ||");
                // Logical OR: at least one must be truthy
                let a_truthy = match a {
                    JsValue::Boolean(false) | JsValue::Null | JsValue::Undefined => false,
                    JsValue::Number(n) => n != 0.0,
                    _ => true, // Strings, objects, functions are truthy
                };
                let b_truthy = match b {
                    JsValue::Boolean(false) | JsValue::Null | JsValue::Undefined => false,
                    JsValue::Number(n) => n != 0.0,
                    _ => true,
                };
                self.stack.push(JsValue::Boolean(a_truthy || b_truthy));
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

            OpCode::TypeOf => {
                let val = self.stack.pop().unwrap_or(JsValue::Undefined);
                let type_str = match val {
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
                };
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

            OpCode::Sub => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number(a - b));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::Mul => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number(a * b));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::BitAnd => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number((a as i64 & b as i64) as f64));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::BitOr => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number((a as i64 | b as i64) as f64));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::Xor => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number((a as i64 ^ b as i64) as f64));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::ShiftLeft => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number(((a as i64) << (b as i64)) as f64));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::ShiftRight => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number(((a as i64) >> (b as i64)) as f64));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

            OpCode::ShiftRightUnsigned => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number((((a as u64) >> (b as u64)) as u64) as f64));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
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

            OpCode::Div => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number(a / b));
                } else {
                    self.stack.push(JsValue::Undefined);
                }
            }

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
                        | (JsValue::String(s), JsValue::Number(n)) => s
                            .parse::<f64>()
                            .map(|parsed| (*n - parsed).abs() < f64::EPSILON)
                            .unwrap_or(false),
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

            OpCode::Lt => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Boolean(a < b));
                } else {
                    self.stack.push(JsValue::Boolean(false));
                }
            }

            OpCode::LtEq => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Boolean(a <= b));
                } else {
                    self.stack.push(JsValue::Boolean(false));
                }
            }

            OpCode::Gt => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Boolean(a > b));
                } else {
                    self.stack.push(JsValue::Boolean(false));
                }
            }

            OpCode::GtEq => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Boolean(a >= b));
                } else {
                    self.stack.push(JsValue::Boolean(false));
                }
            }

            OpCode::Mod => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    if b == 0.0 {
                        self.stack.push(JsValue::Number(f64::NAN));
                    } else {
                        self.stack.push(JsValue::Number(a % b));
                    }
                } else {
                    self.stack.push(JsValue::Number(f64::NAN));
                }
            }

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
                    if i < arr.len() {
                        arr[i] = value;
                    }
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
                        if let Some(heap_obj) = self.heap.get(ptr) {
                            if let HeapData::Array(arr) = &heap_obj.data {
                                let i = idx as usize;
                                let val = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                                self.stack.push(val);
                            }
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
                    panic!(
                        "Stack overflow: maximum call depth of {} exceeded",
                        MAX_CALL_STACK_DEPTH
                    );
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
                        (
                            *address,
                            env.clone(),
                            None::<JsValue>,
                            constructor_val.clone(),
                        )
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
                                    (address, env.clone(), proto, constructor_val.clone())
                                }
                                Some(JsValue::Object(ptr)) => {
                                    // Constructor might be wrapped in another object
                                    (0usize, None, proto, constructor_val.clone())
                                }
                                Some(JsValue::NativeFunction(native_idx)) => {
                                    // Native function constructor - use index 0 as placeholder
                                    // The native function will handle construction itself
                                    (0usize, None, proto, constructor_val.clone())
                                }
                                Some(other) => {
                                    panic!("Constructor is not a Function, it's {:?}", other);
                                }
                                None => {
                                    // No constructor property - this is a "constructor object" like Promise
                                    // For Promise-like objects, we treat the object itself as the constructor
                                    // and call a special constructor handler
                                    // For now, we'll panic with a helpful message
                                    eprintln!("Warning: 'new' on object without constructor - treating as constructor object");
                                    // Create a placeholder that will be handled specially
                                    (0usize, None, proto, constructor_val.clone())
                                }
                            }
                        } else {
                            panic!("Constructor is not an object with properties");
                        }
                    }
                    _ => panic!("Constructor is not a function or class"),
                };

                // Create new object with prototype
                let this_ptr = self.heap.len();
                let this_obj = JsValue::Object(this_ptr);
                self.heap.push(HeapObject {
                    data: HeapData::Object(HashMap::new()),
                });

                // Set prototype if we have one
                if let Some(proto_val) = prototype {
                    if let JsValue::Object(proto_ptr) = proto_val {
                        if let Some(heap_item) = self.heap.get_mut(this_ptr) {
                            if let HeapData::Object(props) = &mut heap_item.data {
                                props.insert("__proto__".to_string(), JsValue::Object(proto_ptr));
                            }
                        }
                    }
                }

                // Push args back for function prologue
                for arg in &args {
                    self.stack.push(arg.clone());
                }

                // Create frame with `this` bound to the new object
                let mut frame = Frame {
                    return_address: self.ip + 1,
                    locals: HashMap::new(),
                    indexed_locals: Vec::new(),
                    this_context: this_obj.clone(),
                    new_target: Some(new_target_val.clone()),
                    super_called: false,
                    resume_ip: None,
                };

                // Load captured environment if present
                if let Some(HeapObject {
                    data: HeapData::Object(props),
                }) = env.and_then(|ptr| self.heap.get(ptr))
                {
                    for (name, value) in props {
                        frame.locals.insert(name.clone(), value.clone());
                    }
                }

                // Check if this is a native function constructor
                if address == 0 {
                    // Native constructor - check if it's Promise
                    let is_promise = if let JsValue::Object(ptr) = &new_target_val {
                        if let Some(heap_obj) = self.heap.get(*ptr) {
                            if let HeapData::Object(props) = &heap_obj.data {
                                // Check for Promise-specific properties
                                props.contains_key("then") && props.contains_key("catch")
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if is_promise {
                        // Handle Promise construction specially
                        // new Promise((resolve, reject) => { ... })
                        eprintln!("DEBUG: Construct - Promise detected");

                        // The executor should be the first argument
                        let executor = args.get(0).cloned().unwrap_or(JsValue::Undefined);
                        eprintln!("DEBUG: Construct - executor = {:?}", executor);

                        // Create a new pending promise
                        let promise = Promise::new();
                        eprintln!("DEBUG: Construct - created promise");

                        // If we have an executor function, call it synchronously
                        if let JsValue::Function { address: exec_addr, env } = executor {
                            eprintln!("DEBUG: Construct - calling executor at {}", exec_addr);

                            // Set the current promise so resolve/reject can access it
                            self.current_promise = Some(promise.clone());

                            // Create resolve function
                            let resolve_idx = self.register_native(|vm, args| {
                                let value = args.get(0).cloned().unwrap_or(JsValue::Undefined);
                                eprintln!("DEBUG: Construct resolve called with {:?}", value);
                                if let Some(p) = vm.current_promise.take() {
                                    p.set_value(value, true);
                                }
                                JsValue::Undefined
                            });

                            // Create reject function
                            let reject_idx = self.register_native(|vm, args| {
                                let reason = args.get(0).cloned().unwrap_or(JsValue::Undefined);
                                eprintln!("DEBUG: Construct reject called with {:?}", reason);
                                if let Some(p) = vm.current_promise.take() {
                                    p.set_value(reason, false);
                                }
                                JsValue::Undefined
                            });

                            // Create a frame for the executor
                            let mut exec_frame = Frame {
                                return_address: self.ip + 1,
                                locals: HashMap::new(),
                                indexed_locals: Vec::new(),
                                this_context: JsValue::Undefined,
                                new_target: Some(executor.clone()),
                                super_called: false,
                                resume_ip: None,
                            };

                            // Set up locals: resolve and reject
                            exec_frame.locals.insert("resolve".to_string(), JsValue::NativeFunction(resolve_idx));
                            exec_frame.locals.insert("reject".to_string(), JsValue::NativeFunction(reject_idx));

                            // Load captured environment
                            if let Some(env_ptr) = env {
                                if let Some(HeapObject {
                                    data: HeapData::Object(props),
                                }) = self.heap.get(env_ptr)
                                {
                                    for (name, value) in props {
                                        exec_frame.locals.insert(name.clone(), value.clone());
                                    }
                                }
                            }

                            // Push the frame and jump to executor
                            self.call_stack.push(exec_frame);
                            self.ip = exec_addr;
                            return ExecResult::ContinueNoIpInc;
                        }

                        // If no executor or invalid executor, just return the Promise
                        self.stack.push(JsValue::Promise(promise));
                    } else {
                        // Regular native constructor - push a frame with this_context
                        let mut native_frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context: this_obj.clone(),
                            new_target: Some(new_target_val.clone()),
                            super_called: false,
                            resume_ip: None,
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
                    JsValue::String(s) => {
                        match name.as_str() {
                            "length" => {
                                // Pop any args (shouldn't be any for length property)
                                for _ in 0..arg_count {
                                    self.stack.pop();
                                }
                                self.stack.push(JsValue::Number(s.chars().count() as f64));
                            }
                            "charCodeAt" => {
                                // Get char code at index
                                let index = if arg_count > 0 {
                                    match self.stack.pop() {
                                        Some(JsValue::Number(n)) => n as usize,
                                        _ => 0,
                                    }
                                } else {
                                    0
                                };
                                // Pop remaining args if any
                                for _ in 1..arg_count {
                                    self.stack.pop();
                                }
                                let result = s
                                    .chars()
                                    .nth(index)
                                    .map(|c| JsValue::Number(c as u32 as f64))
                                    .unwrap_or(JsValue::Number(f64::NAN));
                                self.stack.push(result);
                            }
                            "slice" => {
                                // Get start and end indices
                                let mut args = Vec::with_capacity(arg_count);
                                for _ in 0..arg_count {
                                    args.push(self.stack.pop().expect("Missing argument"));
                                }
                                args.reverse();

                                let len = s.chars().count() as i64;
                                let start = args
                                    .first()
                                    .and_then(|v| match v {
                                        JsValue::Number(n) => {
                                            let n = *n as i64;
                                            if n < 0 {
                                                Some((len + n).max(0) as usize)
                                            } else {
                                                Some(n as usize)
                                            }
                                        }
                                        _ => None,
                                    })
                                    .unwrap_or(0);
                                let end = args
                                    .get(1)
                                    .and_then(|v| match v {
                                        JsValue::Number(n) => {
                                            let n = *n as i64;
                                            if n < 0 {
                                                Some((len + n).max(0) as usize)
                                            } else {
                                                Some(n as usize)
                                            }
                                        }
                                        _ => None,
                                    })
                                    .unwrap_or(len as usize);

                                let result: String = s.chars().skip(start).take(end.saturating_sub(start)).collect();
                                self.stack.push(JsValue::String(result));
                            }
                            _ => {
                                // Unsupported string method - pop args and return undefined
                                for _ in 0..arg_count {
                                    self.stack.pop();
                                }
                                self.stack.push(JsValue::Undefined);
                            }
                        }
                        self.ip += 1;
                        return ExecResult::Continue;
                    }
                    JsValue::Object(ptr) => {
                        // Check if this is an array and handle array methods
                        if let Some(HeapObject {
                            data: HeapData::Array(arr),
                        }) = self.heap.get_mut(ptr)
                        {
                            // Handle splice inline since it needs heap access
                            if name == "splice" {
                                let mut args = Vec::with_capacity(arg_count);
                                for _ in 0..arg_count {
                                    args.push(self.stack.pop().expect("Missing argument"));
                                }
                                args.reverse();

                                let start = args
                                    .first()
                                    .and_then(|v| match v {
                                        JsValue::Number(n) => Some(*n as usize),
                                        _ => None,
                                    })
                                    .unwrap_or(0);
                                let delete_count = args
                                    .get(1)
                                    .and_then(|v| match v {
                                        JsValue::Number(n) => Some(*n as usize),
                                        _ => None,
                                    })
                                    .unwrap_or(0);
                                let items_to_insert: Vec<JsValue> = args.into_iter().skip(2).collect();

                                let deleted: Vec<JsValue> = if start < arr.len() {
                                    let end = (start + delete_count).min(arr.len());
                                    arr.drain(start..end).collect()
                                } else {
                                    Vec::new()
                                };

                                for (i, item) in items_to_insert.into_iter().enumerate() {
                                    arr.insert(start + i, item);
                                }

                                let deleted_ptr = self.heap.len();
                                self.heap.push(HeapObject {
                                    data: HeapData::Array(deleted),
                                });
                                self.stack.push(JsValue::Object(deleted_ptr));
                                self.ip += 1;
                                return ExecResult::Continue;
                            }

                            // For other array methods, provide basic support
                            // Note: Full array method support moved to @rolls/array
                            match name.as_str() {
                                "length" => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    self.stack.push(JsValue::Number(arr.len() as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "push" => {
                                    let mut args = Vec::with_capacity(arg_count);
                                    for _ in 0..arg_count {
                                        args.push(self.stack.pop().expect("Missing argument"));
                                    }
                                    args.reverse();
                                    for arg in args {
                                        arr.push(arg);
                                    }
                                    self.stack.push(JsValue::Number(arr.len() as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "pop" => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    let result = arr.pop().unwrap_or(JsValue::Undefined);
                                    self.stack.push(result);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "shift" => {
                                    for _ in 0..arg_count {
                                        self.stack.pop();
                                    }
                                    let result = if !arr.is_empty() {
                                        arr.remove(0)
                                    } else {
                                        JsValue::Undefined
                                    };
                                    self.stack.push(result);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "unshift" => {
                                    let mut args = Vec::with_capacity(arg_count);
                                    for _ in 0..arg_count {
                                        args.push(self.stack.pop().expect("Missing argument"));
                                    }
                                    args.reverse();
                                    for (i, arg) in args.into_iter().enumerate() {
                                        arr.insert(i, arg);
                                    }
                                    self.stack.push(JsValue::Number(arr.len() as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "join" => {
                                    // Get separator (default to ",")
                                    let separator = if arg_count > 0 {
                                        match self.stack.pop() {
                                            Some(JsValue::String(s)) => s,
                                            Some(JsValue::Number(n)) => n.to_string(),
                                            _ => ",".to_string(),
                                        }
                                    } else {
                                        ",".to_string()
                                    };
                                    // Pop any remaining args
                                    for _ in 1..arg_count {
                                        self.stack.pop();
                                    }
                                    // Join array elements into string
                                    let parts: Vec<String> = arr
                                        .iter()
                                        .map(|v| match v {
                                            JsValue::String(s) => s.clone(),
                                            JsValue::Number(n) => n.to_string(),
                                            JsValue::Boolean(b) => b.to_string(),
                                            JsValue::Null => "null".to_string(),
                                            JsValue::Undefined => "undefined".to_string(),
                                            _ => "".to_string(),
                                        })
                                        .collect();
                                    self.stack.push(JsValue::String(parts.join(&separator)));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                _ => {
                                    // Unsupported array method - pop args and return undefined
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
                        let method = self.get_prop_with_proto_chain(ptr, &name);

                        if let JsValue::NativeFunction(idx) = method {
                            // For native functions, call directly
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.stack.pop().expect("Missing argument"));
                            }
                            args.reverse();
                            let func = self.native_functions[idx];
                            let result = func(self, args);
                            self.stack.push(result);
                            // Increment IP before returning since we return early
                            self.ip += 1;
                            return ExecResult::Continue;
                        } else if let JsValue::Function { address, env } = method {
                            // Stack overflow protection
                            if self.call_stack.len() >= MAX_CALL_STACK_DEPTH {
                                panic!(
                                    "Stack overflow: maximum call depth of {} exceeded",
                                    MAX_CALL_STACK_DEPTH
                                );
                            }

                            // Collect arguments
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.stack.pop().expect("Missing argument"));
                            }
                            args.reverse();

                            // Push arguments in call order
                            for arg in &args {
                                self.stack.push(arg.clone());
                            }

                            // Create new frame with `this` bound to the receiver object
                            let mut frame = Frame {
                                return_address: self.ip + 1,
                                locals: HashMap::new(),
                                indexed_locals: Vec::new(),
                                this_context: JsValue::Object(ptr),
                                new_target: None,
                                super_called: false,
                                resume_ip: None,
                            };

                            // Load captured variables from environment
                            if let Some(HeapObject {
                                data: HeapData::Object(props),
                            }) = env.and_then(|ptr| self.heap.get(ptr))
                            {
                                for (name, value) in props {
                                    frame.locals.insert(name.clone(), value.clone());
                                }
                            }

                            self.call_stack.push(frame);
                            self.ip = address;
                            return ExecResult::ContinueNoIpInc;
                        }
                        panic!("Method {} not found on object", name);
                    }
                    // Handle Promise.then and Promise.catch methods
                    JsValue::Promise(promise) => {
                        match name.as_str() {
                            "then" => {
                                // promise.then(onFulfilled)
                                let on_fulfilled = self.stack.pop().unwrap_or(JsValue::Undefined);
                                let result_promise = promise.then(Some(on_fulfilled));
                                self.stack.push(JsValue::Promise(result_promise));
                                self.ip += 1;
                                return ExecResult::Continue;
                            }
                            "catch" => {
                                // promise.catch(onRejected)
                                let on_rejected = self.stack.pop().unwrap_or(JsValue::Undefined);
                                let result_promise = promise.catch(Some(on_rejected));
                                self.stack.push(JsValue::Promise(result_promise));
                                self.ip += 1;
                                return ExecResult::Continue;
                            }
                            _ => {
                                self.stack.push(JsValue::Undefined);
                                self.ip += 1;
                                return ExecResult::Continue;
                            }
                        }
                    }
                    _ => {
                        self.stack.push(JsValue::Undefined);
                        self.ip += 1;
                        return ExecResult::Continue;
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

                // Find a handler
                if let Some(handler) = self.exception_handlers.pop() {
                    // Unwind the stack to the handler's saved state
                    self.stack.truncate(handler.stack_depth);

                    // Unwind call stack if needed
                    while self.call_stack.len() > handler.call_stack_depth {
                        self.call_stack.pop();
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
                        // No catch, but there's a finally block
                        // Store exception for rethrow after finally
                        self.current_exception = Some(exception);
                        self.ip = handler.finally_addr;
                        return ExecResult::ContinueNoIpInc;
                    }
                }

                // No handler found - panic with uncaught exception
                panic!("Uncaught exception: {:?}", exception);
            }

            OpCode::EnterFinally(rethrow) => {
                // This opcode is emitted at the end of catch/try blocks
                // to ensure finally runs
                if rethrow {
                    // Rethrow the stored exception after finally completes
                    if let Some(exc) = self.current_exception.take() {
                        self.stack.push(exc);
                        // This will trigger another Throw
                        self.ip += 1;
                        return ExecResult::Continue;
                    }
                }
                // Just continue to finally block
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

            OpCode::LoadSuper => {
                // Load __super__ from current frame's new_target (wrapper object)
                // The wrapper object has __super__ property pointing to the parent class
                let frame = self.call_stack.last();

                let new_target = frame.and_then(|frame| frame.new_target.as_ref());

                let super_val = new_target
                    .and_then(|wrapper| {
                        if let JsValue::Object(ptr) = wrapper {
                            self.heap.get(*ptr).and_then(|obj| {
                                if let HeapData::Object(props) = &obj.data {
                                    props.get("__super__").cloned()
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        }
                    })
                    .unwrap_or(JsValue::Undefined);
                self.stack.push(super_val);
            }

            OpCode::CallSuper(arg_count) => {
                // Mark that super() has been called in the derived class constructor
                // This is required by JavaScript: super() must be called before accessing `this`
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.super_called = true;
                }

                // Call the super constructor with current this context
                // Stack: [args..., super_constructor]
                let super_ctor = self
                    .stack
                    .pop()
                    .expect("CallSuper: missing super constructor");
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.stack.pop().expect("CallSuper: missing argument"));
                }

                // Get the actual constructor function
                let ctor_fn = match super_ctor {
                    JsValue::Function { .. } => super_ctor.clone(),
                    JsValue::Object(ptr) => {
                        // Get constructor from object
                        if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(ptr)
                        {
                            props
                                .get("constructor")
                                .cloned()
                                .unwrap_or(JsValue::Undefined)
                        } else {
                            JsValue::Undefined
                        }
                    }
                    _ => panic!(
                        "CallSuper: super is not a constructor, got {:?}",
                        super_ctor
                    ),
                };

                if let JsValue::Function { address, env } = ctor_fn {
                    // Get current this context
                    let this_context = self.call_stack.last().unwrap().this_context.clone();

                    args.reverse();
                    for arg in &args {
                        self.stack.push(arg.clone());
                    }

                    let mut frame = Frame {
                        return_address: self.ip + 1,
                        locals: HashMap::new(),
                        indexed_locals: Vec::new(),
                        this_context,
                        new_target: None,
                        super_called: false,
                        resume_ip: None,
                    };

                    // Load captured variables from closure environment
                    if let Some(HeapObject {
                        data: HeapData::Object(props),
                    }) = env.and_then(|ptr| self.heap.get(ptr))
                    {
                        for (name, value) in props {
                            frame.locals.insert(name.clone(), value.clone());
                        }
                    }

                    self.call_stack.push(frame);
                    self.ip = address;
                    return ExecResult::ContinueNoIpInc;
                } else {
                    panic!("CallSuper: super constructor is not a function");
                }
            }

            OpCode::GetSuperProp(name) => {
                // Get property from super's prototype
                // Stack: [] -> [property_value]
                // For super.prop, we look up the property on the parent class's prototype
                // We get the parent class from the current frame's context or prototype chain

                // Try to get __super__ from new_target first (for super() calls in constructors)
                let super_obj = self
                    .call_stack
                    .last()
                    .and_then(|frame| frame.new_target.as_ref())
                    .and_then(|wrapper| {
                        if let JsValue::Object(ptr) = wrapper {
                            self.heap.get(*ptr).and_then(|obj| {
                                if let HeapData::Object(props) = &obj.data {
                                    props.get("__super__").cloned()
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        }
                    });

                // If new_target doesn't have __super__, try to get it from this_context's prototype chain
                let prop_val = if let Some(JsValue::Object(super_ptr)) = super_obj {
                    // Look for the property on the parent's prototype
                    let proto = {
                        if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(super_ptr)
                        {
                            props.get("prototype").cloned()
                        } else {
                            None
                        }
                    };

                    if let Some(JsValue::Object(proto_ptr)) = proto {
                        self.get_prop_with_proto_chain(proto_ptr, &name)
                    } else {
                        self.get_prop_with_proto_chain(super_ptr, &name)
                    }
                } else {
                    // Fallback: use this_context's prototype chain
                    // This handles super.prop() calls in methods where new_target is not set
                    let this_context = self.call_stack.last().map(|frame| &frame.this_context);

                    if let Some(JsValue::Object(this_ptr)) = this_context {
                        // Walk up the prototype chain to find the property
                        self.get_prop_with_proto_chain(*this_ptr, &name)
                    } else {
                        JsValue::Undefined
                    }
                };

                self.stack.push(prop_val);
            }

            // === Private fields ===
            OpCode::GetPrivateProp(field_index) => {
                // Stack: [this] -> pops this, looks up private field, pushes value
                let this_val = self.stack.pop().expect("GetPrivateProp: missing this");

                let field_value = match &this_val {
                    JsValue::Object(this_ptr) => {
                        // Get the private field storage from the instance
                        // We store "__private_storage__" on each instance pointing to the class's storage
                        let private_storage_ptr = if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(*this_ptr)
                        {
                            props.get("__private_storage__").cloned()
                        } else {
                            None
                        };

                        // Look up the private field in the class's private storage
                        if let Some(JsValue::Object(storage_ptr)) = private_storage_ptr {
                            if let Some(HeapObject {
                                data: HeapData::Array(field_map),
                            }) = self.heap.get(storage_ptr)
                            {
                                // Each entry is a WeakMap for one private field
                                if field_index >= field_map.len() {
                                    JsValue::Undefined
                                } else if let Some(JsValue::Object(weakmap_ptr)) =
                                    field_map.get(field_index)
                                {
                                    // Look up this instance in the WeakMap
                                    // For simplicity, we use a regular Map since Rust's
                                    // WeakMap equivalent isn't available in our VM
                                    if let Some(HeapObject {
                                        data: HeapData::Object(field_map),
                                    }) = self.heap.get(*weakmap_ptr)
                                    {
                                        let key = this_ptr.to_string();
                                        field_map.get(&key).cloned().unwrap_or(JsValue::Undefined)
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
                        }
                    }
                    _ => JsValue::Undefined,
                };

                self.stack.push(field_value);
            }

            OpCode::SetPrivateProp(field_index) => {
                // Stack: [value, this] -> pops both, sets private field
                let value = self.stack.pop().expect("SetPrivateProp: missing value");
                let this_val = self.stack.pop().expect("SetPrivateProp: missing this");

                if let JsValue::Object(this_ptr) = this_val {
                    // Get the private field storage info first (before any mutable borrows)
                    let weakmap_ptr = {
                        // Get the private field storage from the instance
                        let private_storage_ptr = if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(this_ptr)
                        {
                            props.get("__private_storage__").cloned()
                        } else {
                            None
                        };

                        if let Some(JsValue::Object(storage_ptr)) = private_storage_ptr {
                            if let Some(HeapObject {
                                data: HeapData::Array(field_map),
                            }) = self.heap.get(storage_ptr)
                            {
                                if field_index < field_map.len() {
                                    if let Some(JsValue::Object(w_ptr)) = field_map.get(field_index)
                                    {
                                        Some(*w_ptr)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };

                    // Now do the mutable operation
                    if let Some(w_ptr) = weakmap_ptr {
                        let key = this_ptr.to_string();
                        if let Some(heap_item) = self.heap.get_mut(w_ptr) {
                            if let HeapData::Object(field_map) = &mut heap_item.data {
                                field_map.insert(key, value);
                            }
                        }
                    }
                }
            }

            OpCode::InstanceOf => {
                // Stack: [constructor, object] -> pops both, pushes boolean
                let constructor = self.stack.pop().unwrap_or(JsValue::Undefined);
                let obj = self.stack.pop().unwrap_or(JsValue::Undefined);

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

            OpCode::ApplyDecorator => {
                // Apply a decorator to a target (class, method, or field: [decorator, target] ->)
                // Stack [decorated]
                // The decorator is called as: decorator(target)
                // NOTE: Stack order is [wrapper, decorator] (wrapper at bottom, decorator on top)
                // So first pop gets decorator, second pop gets target
                let decorator = self.stack.pop().expect("ApplyDecorator: missing decorator");
                let target = self.stack.pop().expect("ApplyDecorator: missing target");

                match decorator {
                    JsValue::Function { address, env } => {
                        // Clone target for use in the frame
                        let target_for_frame = target.clone();

                        // Call the decorator function with the target
                        // Stack should be: [decorator, target] -> Call(1) pops target as arg, decorator as callee
                        self.stack.push(decorator);
                        self.stack.push(target);

                        // Create a frame for the decorator call
                        let mut frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context: target_for_frame.clone(),
                            new_target: Some(target_for_frame),
                            super_called: false,
                            resume_ip: None,
                        };

                        // Load captured variables from environment
                        if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = env.and_then(|ptr| self.heap.get(ptr))
                        {
                            for (name, value) in props {
                                frame.locals.insert(name.clone(), value.clone());
                            }
                        }

                        self.call_stack.push(frame);
                        self.ip = address;
                        return ExecResult::ContinueNoIpInc;
                    }
                    _ => {
                        // If decorator is not a function, return target unchanged
                        self.stack.push(target);
                    }
                }
            }

            // === ES Modules ===
            OpCode::ImportAsync(specifier) => {
                let specifier_str = match self.stack.pop() {
                    Some(JsValue::String(s)) => s,
                    Some(_) => {
                        eprintln!("Error: ImportAsync expected string specifier");
                        self.stack.push(JsValue::Undefined);
                        return ExecResult::Continue;
                    }
                    None => {
                        eprintln!("Error: ImportAsync missing specifier on stack");
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

                    let extensions = ["tscl", "ts", "js"];
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
                    return ExecResult::Continue;
                }

                // Check if file exists but hash doesn't match (stale cache entry)
                let current_hash = ModuleCache::compute_hash(&canonical_path);
                if self.module_cache.has_content_hash(&canonical_path) {
                    // Hash mismatch - stale cache, invalidate
                    self.module_cache.invalidate(&canonical_path);
                }

                // Cache miss or stale - load the module
                let result = fs::read_to_string(&canonical_path)
                    .map_err(|e| format!("Failed to read module: {}", e));

                match result {
                    Ok(source) => {
                        let hash = ModuleCache::compute_hash(&canonical_path);

                        // Parse module and extract exports
                        let exports =
                            parse_module_exports(&source, &canonical_path.to_string_lossy());

                        let export_names: Vec<String> = exports.keys().cloned().collect();

                        let mut namespace_props = HashMap::new();

                        // Add metadata
                        namespace_props.insert(
                            "__path__".to_string(),
                            JsValue::String(canonical_path.to_string_lossy().into_owned()),
                        );
                        namespace_props
                            .insert("__source__".to_string(), JsValue::String(source.clone()));
                        namespace_props
                            .insert("__hash__".to_string(), JsValue::String(hash.clone()));

                        // Execute the module to populate exports
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

                        // Cache the module
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

            OpCode::Await => {
                // Stack: [promise] -> [result]
                let promise = match self.stack.pop() {
                    Some(JsValue::Promise(p)) => p,
                    Some(other) => {
                        // Non-promise values are passed through (thenable check simplified)
                        self.stack.push(other);
                        self.stack.push(JsValue::Undefined);
                        return ExecResult::Continue;
                    }
                    None => {
                        self.stack.push(JsValue::Undefined);
                        return ExecResult::Continue;
                    }
                };

                // Poll the promise synchronously (simplified implementation)
                let state = promise.get_state();
                eprintln!("DEBUG Await: promise state = {:?}", state);
                
                match state {
                    PromiseState::Fulfilled => {
                        let value = promise.get_value().unwrap_or(JsValue::Undefined);
                        eprintln!("DEBUG Await: fulfilled with value = {:?}", value);
                        self.stack.push(value);
                    }
                    PromiseState::Rejected => {
                        let value = promise.get_value().unwrap_or(JsValue::Undefined);
                        eprintln!("DEBUG Await: rejected with value = {:?}", value);
                        self.stack.push(value);
                    }
                    PromiseState::Pending => {
                        eprintln!("DEBUG Await: still pending, polling...");
                        // Poll until resolved (with timeout)
                        let result = self.poll_promise(&promise, 1000);
                        eprintln!("DEBUG Await: poll result = {:?}", result);
                        self.stack.push(result);
                    }
                }
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
    fn native_write_bytecode_file(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        if let Some(JsValue::String(path)) = args.get(0) {
            match std::fs::write(
                path,
                vm.program
                    .iter()
                    .map(|op| format!("{:?}", op))
                    .collect::<Vec<String>>()
                    .join("\n")
                    .as_bytes()
                    .to_vec(),
            ) {
                Ok(_) => JsValue::Undefined,
                Err(e) => JsValue::String(format!("Error writing bytecode file: {}", e)),
            }
        } else {
            JsValue::Undefined
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecResult {
    Continue,
    ContinueNoIpInc,
    Stop,
}
