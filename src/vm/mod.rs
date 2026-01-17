pub mod opcodes;
pub mod value;

/// Maximum call stack depth to prevent stack overflow in deeply recursive code
const MAX_CALL_STACK_DEPTH: usize = 1000;

use crate::stdlib::{
    native_byte_stream_length, native_byte_stream_patch_u32, native_byte_stream_to_array,
    native_byte_stream_write_f64, native_byte_stream_write_string, native_byte_stream_write_u8,
    native_byte_stream_write_u32, native_byte_stream_write_varint, native_create_byte_stream,
    native_log, native_read_file, native_require, native_set_timeout, native_write_binary_file,
    native_write_file,
    native_string_from_char_code,
};
use crate::vm::opcodes::OpCode;
use crate::vm::value::HeapData;
use crate::vm::value::HeapObject;
use crate::vm::value::JsValue;
use crate::vm::value::NativeFn;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

pub struct Frame {
    pub return_address: usize,
    pub locals: HashMap<String, JsValue>,
    pub indexed_locals: Vec<JsValue>,
    pub this_context: JsValue,
}

pub struct Task {
    pub function_ptr: JsValue,
    pub args: Vec<JsValue>,
}

pub struct TimerTask {
    due: Instant,
    task: Task,
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
    pub ip: usize, // Instruction Pointer
}

impl VM {
    pub fn new() -> Self {
        let mut vm = Self {
            stack: Vec::new(),
            call_stack: vec![Frame {
                return_address: 0,
                locals: HashMap::new(),
                indexed_locals: Vec::new(),
                this_context: JsValue::Undefined,
            }],
            heap: Vec::new(),
            native_functions: Vec::new(),
            task_queue: VecDeque::new(),
            timers: Vec::new(),
            program: Vec::new(),
            modules: HashMap::new(),
            ip: 0,
        };
        vm.setup_stdlib();
        vm
    }

    pub fn setup_stdlib(&mut self) {
        // Register native functions
        let log_idx = self.register_native(native_log);
        let timeout_idx = self.register_native(native_set_timeout);
        let read_idx = self.register_native(native_read_file);
        let write_idx = self.register_native(native_write_file);
        let require_idx = self.register_native(native_require);

        // ByteStream native functions
        let create_byte_stream_idx = self.register_native(native_create_byte_stream);
        let write_u8_idx = self.register_native(native_byte_stream_write_u8);
        let write_varint_idx = self.register_native(native_byte_stream_write_varint);
        let write_string_idx = self.register_native(native_byte_stream_write_string);
        let write_u32_idx = self.register_native(native_byte_stream_write_u32);
        let write_f64_idx = self.register_native(native_byte_stream_write_f64);
        let patch_u32_idx = self.register_native(native_byte_stream_patch_u32);
        let stream_length_idx = self.register_native(native_byte_stream_length);
        let to_array_idx = self.register_native(native_byte_stream_to_array);
        let write_binary_file_idx = self.register_native(native_write_binary_file);
        
        // ASCII String Native Functions
        let string_from_char_code_idx = self.register_native(native_string_from_char_code);

        // console = { log: <native fn> }
        let console_ptr = self.heap.len();
        let mut console_props = HashMap::new();
        console_props.insert("log".to_string(), JsValue::NativeFunction(log_idx));
        self.heap.push(HeapObject {
            data: HeapData::Object(console_props),
        });

        // fs = { readFileSync, writeFileSync, writeBinaryFile }
        let fs_ptr = self.heap.len();
        let mut fs_props = HashMap::new();
        fs_props.insert(
            "readFileSync".to_string(),
            JsValue::NativeFunction(read_idx),
        );
        fs_props.insert(
            "writeFileSync".to_string(),
            JsValue::NativeFunction(write_idx),
        );
        fs_props.insert(
            "writeBinaryFile".to_string(),
            JsValue::NativeFunction(write_binary_file_idx),
        );
        self.heap.push(HeapObject {
            data: HeapData::Object(fs_props),
        });

        // ByteStream = { create, writeU8, writeVarint, writeString, writeU32, writeF64, patchU32, length, toArray }
        let byte_stream_ptr = self.heap.len();
        let mut byte_stream_props = HashMap::new();
        byte_stream_props.insert(
            "create".to_string(),
            JsValue::NativeFunction(create_byte_stream_idx),
        );
        byte_stream_props.insert("writeU8".to_string(), JsValue::NativeFunction(write_u8_idx));
        byte_stream_props.insert(
            "writeVarint".to_string(),
            JsValue::NativeFunction(write_varint_idx),
        );
        byte_stream_props.insert(
            "writeString".to_string(),
            JsValue::NativeFunction(write_string_idx),
        );
        byte_stream_props.insert(
            "writeU32".to_string(),
            JsValue::NativeFunction(write_u32_idx),
        );
        byte_stream_props.insert(
            "writeF64".to_string(),
            JsValue::NativeFunction(write_f64_idx),
        );
        byte_stream_props.insert(
            "patchU32".to_string(),
            JsValue::NativeFunction(patch_u32_idx),
        );
        byte_stream_props.insert(
            "length".to_string(),
            JsValue::NativeFunction(stream_length_idx),
        );
        byte_stream_props.insert("toArray".to_string(), JsValue::NativeFunction(to_array_idx));
        self.heap.push(HeapObject {
            data: HeapData::Object(byte_stream_props),
        });

        // Global bindings
        let globals = self.call_stack.first_mut().expect("Missing global frame");
        globals
            .locals
            .insert("console".into(), JsValue::Object(console_ptr));
        globals
            .locals
            .insert("setTimeout".into(), JsValue::NativeFunction(timeout_idx));
        globals
            .locals
            .insert("require".into(), JsValue::NativeFunction(require_idx));
        globals
            .locals
            .insert("ByteStream".into(), JsValue::Object(byte_stream_ptr));

        // Module registry
        self.modules
            .insert("fs".to_string(), JsValue::Object(fs_ptr));

        let string_ptr = self.heap.len();
        let mut string_props = HashMap::new();
        string_props.insert("fromCharCode".to_string(), JsValue::NativeFunction(string_from_char_code_idx));
        self.heap.push(HeapObject {
            data: HeapData::Object(string_props),
        });
        self.call_stack[0].locals.insert("String".into(), JsValue::Object(string_ptr));
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

            OpCode::SetProp(name) => {
                let value = self.stack.pop().unwrap();
                if let Some(JsValue::Object(ptr)) = self.stack.pop()
                    && let Some(HeapObject {
                        data: HeapData::Object(props),
                    }) = self.heap.get_mut(ptr)
                {
                    props.insert(name.to_string(), value);
                }
            }

            OpCode::GetProp(name) => {
                let target = self.stack.pop();

                match target {
                    Some(JsValue::Object(ptr)) => {
                        if let Some(heap_item) = self.heap.get(ptr) {
                            match &heap_item.data {
                                HeapData::Object(props) => {
                                    let val =
                                        props.get(&name).cloned().unwrap_or(JsValue::Undefined);
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
                self.stack.push(found.unwrap_or(JsValue::Undefined));
            }

            OpCode::LoadThis => {
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

                match callee {
                    JsValue::Function { address, env } => {
                        args.reverse();
                        for arg in &args {
                            self.stack.push(arg.clone());
                        }

                        let mut frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context: JsValue::Undefined,
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
                    _ => panic!("Target is not callable"),
                }
            }

            OpCode::Return => {
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

                // Stack layout: [..., this_obj, this_obj_copy, arg1, arg2, ..., constructor]
                let constructor = self.stack.pop().expect("Missing constructor");

                // Pop arguments
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.stack.pop().expect("Missing argument"));
                }
                args.reverse();

                // Pop the `this` object (the duplicate copy we made)
                let this_obj = self.stack.pop().expect("Missing this object");
                // Note: The original object copy remains on stack for return value

                match constructor {
                    JsValue::Function { address, env } => {
                        // Push args back for function prologue
                        for arg in &args {
                            self.stack.push(arg.clone());
                        }

                        // Create frame with `this` bound to the new object
                        let mut frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                            indexed_locals: Vec::new(),
                            this_context: this_obj,
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

                        self.call_stack.push(frame);
                        self.ip = address;
                        return ExecResult::ContinueNoIpInc;
                    }
                    _ => panic!("Constructor is not a function"),
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
                    JsValue::String(s) => match name.as_str() {
                        "trim" => {
                            let result = s.trim().to_string();
                            self.stack.push(JsValue::String(result));
                            self.ip += 1;
                            return ExecResult::Continue;
                        }
                        "includes" => {
                            // includes(searchString) - checks if string contains the search string
                            let search_value = self.stack.pop().unwrap_or(JsValue::Undefined);
                            let search_str = match search_value {
                                JsValue::String(ss) => ss,
                                JsValue::Number(n) => n.to_string(),
                                JsValue::Boolean(b) => b.to_string(),
                                JsValue::Null => "null".to_string(),
                                JsValue::Undefined => "undefined".to_string(),
                                _ => "".to_string(),
                            };
                            let found = s.contains(&search_str);
                            self.stack.push(JsValue::Boolean(found));
                            self.ip += 1;
                            return ExecResult::Continue;
                        }
                        "charCodeAt" => {
                            let idx_val = self.stack.pop().unwrap_or(JsValue::Number(0.0));
                            if let JsValue::Number(idx) = idx_val {
                                let code =
                                    s.chars().nth(idx as usize).map(|c| c as u32).unwrap_or(0);
                                self.stack.push(JsValue::Number(code as f64));
                            }
                            self.ip += 1;
                            return ExecResult::Continue;
                        }
                        "charAt" => {
                            let idx_val = self.stack.pop().unwrap_or(JsValue::Number(0.0));
                            if let JsValue::Number(idx) = idx_val {
                                let char = s
                                    .chars()
                                    .nth(idx as usize)
                                    .map(|c| c.to_string())
                                    .unwrap_or("".to_string());
                                self.stack.push(JsValue::String(char));
                            }
                            self.ip += 1;
                            return ExecResult::Continue;
                        }
                        "slice" => {
                            // slice(start, end?) - end is optional, defaults to string length
                            // Arguments are on stack in reverse order (last arg on top)
                            let end = if arg_count > 1 {
                                self.stack
                                    .pop()
                                    .and_then(|v| match v {
                                        JsValue::Number(n) => {
                                            // Handle negative indices: count from end
                                            let char_count = s.chars().count();
                                            if n < 0.0 {
                                                Some((char_count as f64 + n) as usize)
                                            } else {
                                                Some(n as usize)
                                            }
                                        }
                                        _ => None,
                                    })
                                    .unwrap_or(s.chars().count())
                            } else {
                                s.chars().count()
                            };
                            let start = self
                                .stack
                                .pop()
                                .and_then(|v| match v {
                                    JsValue::Number(n) => {
                                        // Handle negative indices: count from end
                                        let char_count = s.chars().count();
                                        if n < 0.0 {
                                            Some((char_count as f64 + n) as usize)
                                        } else {
                                            Some(n as usize)
                                        }
                                    }
                                    _ => None,
                                })
                                .unwrap_or(0);

                            // Clamp indices to valid range
                            let char_count = s.chars().count();
                            let start = start.min(char_count);
                            let end = end.min(char_count).max(start);

                            // Extract substring by character position
                            let result: String = s
                                .chars()
                                .enumerate()
                                .filter_map(|(i, ch)| {
                                    if i >= start && i < end {
                                        Some(ch)
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            self.stack.push(JsValue::String(result));
                            self.ip += 1;
                            return ExecResult::Continue;
                        }
                        _ => {
                            self.stack.push(JsValue::Undefined);
                            self.ip += 1;
                            return ExecResult::Continue;
                        }
                    },
                    JsValue::Object(ptr) => {
                        // Check if this is an array and handle array methods
                        if let Some(HeapObject {
                            data: HeapData::Array(arr),
                        }) = self.heap.get_mut(ptr)
                        {
                            match name.as_str() {
                                // Mutable methods
                                "push" => {
                                    // Collect all arguments
                                    let mut args = Vec::with_capacity(arg_count);
                                    for _ in 0..arg_count {
                                        args.push(self.stack.pop().expect("Missing argument"));
                                    }
                                    args.reverse();
                                    // Push all arguments to the array
                                    for arg in args {
                                        arr.push(arg);
                                    }
                                    // Return the new length
                                    self.stack.push(JsValue::Number(arr.len() as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "pop" => {
                                    let result = arr.pop().unwrap_or(JsValue::Undefined);
                                    self.stack.push(result);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "shift" => {
                                    let result = if arr.is_empty() {
                                        JsValue::Undefined
                                    } else {
                                        arr.remove(0)
                                    };
                                    self.stack.push(result);
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "unshift" => {
                                    // Collect all arguments
                                    let mut args = Vec::with_capacity(arg_count);
                                    for _ in 0..arg_count {
                                        args.push(self.stack.pop().expect("Missing argument"));
                                    }
                                    args.reverse();
                                    // Insert at the beginning (reverse order to maintain argument order)
                                    for arg in args.into_iter().rev() {
                                        arr.insert(0, arg);
                                    }
                                    // Return the new length
                                    self.stack.push(JsValue::Number(arr.len() as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "splice" => {
                                    // splice(start, deleteCount, ...items)
                                    // Collect arguments
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
                                    let items_to_insert: Vec<JsValue> =
                                        args.into_iter().skip(2).collect();

                                    // Create result array with deleted elements
                                    let deleted: Vec<JsValue> = if start < arr.len() {
                                        let end = (start + delete_count).min(arr.len());
                                        arr.drain(start..end).collect()
                                    } else {
                                        Vec::new()
                                    };

                                    // Insert new items at start position
                                    for (i, item) in items_to_insert.into_iter().enumerate() {
                                        arr.insert(start + i, item);
                                    }

                                    // Return array of deleted elements
                                    let deleted_ptr = self.heap.len();
                                    self.heap.push(HeapObject {
                                        data: HeapData::Array(deleted),
                                    });
                                    self.stack.push(JsValue::Object(deleted_ptr));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                // Read-only methods
                                "length" => {
                                    self.stack.push(JsValue::Number(arr.len() as f64));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "indexOf" => {
                                    // indexOf takes 1-2 arguments: (searchElement, fromIndex?)
                                    // Arguments are on stack in reverse order (last arg on top)
                                    let from_index = if arg_count > 1 {
                                        self.stack
                                            .pop()
                                            .and_then(|v| match v {
                                                JsValue::Number(n) => Some(n as usize),
                                                _ => None,
                                            })
                                            .unwrap_or(0)
                                    } else {
                                        0
                                    };
                                    let search_value =
                                        self.stack.pop().expect("Missing argument for indexOf");

                                    let result = arr
                                        .iter()
                                        .enumerate()
                                        .skip(from_index)
                                        .find(|(_, val)| **val == search_value)
                                        .map(|(i, _)| i as f64)
                                        .unwrap_or(-1.0);

                                    self.stack.push(JsValue::Number(result));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "includes" => {
                                    // includes takes 1 argument: searchElement
                                    let search_value =
                                        self.stack.pop().expect("Missing argument for includes");
                                    let found = arr.contains(&search_value);
                                    self.stack.push(JsValue::Boolean(found));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                "join" => {
                                    // join takes 0-1 arguments: (separator?)
                                    // If no argument, separator defaults to ","
                                    let separator = if arg_count > 0 {
                                        self.stack
                                            .pop()
                                            .map(|v| match v {
                                                JsValue::String(s) => s,
                                                JsValue::Undefined => ",".to_string(),
                                                _ => ",".to_string(),
                                            })
                                            .unwrap_or_else(|| ",".to_string())
                                    } else {
                                        ",".to_string()
                                    };

                                    let result = arr
                                        .iter()
                                        .map(|v| match v {
                                            JsValue::String(s) => s.clone(),
                                            JsValue::Number(n) => n.to_string(),
                                            JsValue::Boolean(b) => b.to_string(),
                                            JsValue::Null => "null".to_string(),
                                            JsValue::Undefined => "undefined".to_string(),
                                            _ => "".to_string(),
                                        })
                                        .collect::<Vec<String>>()
                                        .join(&separator);

                                    self.stack.push(JsValue::String(result));
                                    self.ip += 1;
                                    return ExecResult::Continue;
                                }
                                _ => {
                                    // Not an array method, fall through to object method lookup
                                }
                            }
                        }

                        // Lookup the method in the object (for non-array methods or if not an array)
                        let method = if let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get(ptr)
                        {
                            props.get(&name).cloned().unwrap_or(JsValue::Undefined)
                        } else {
                            JsValue::Undefined
                        };

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
