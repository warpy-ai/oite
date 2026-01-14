pub mod opcodes;
pub mod value;
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
}

pub struct Task {
    pub function_ptr: JsValue,
    pub args: Vec<JsValue>,
}

struct TimerTask {
    due: Instant,
    task: Task,
}

pub struct VM {
    stack: Vec<JsValue>,
    pub call_stack: Vec<Frame>,
    pub heap: Vec<HeapObject>,
    pub native_functions: Vec<NativeFn>,
    pub task_queue: VecDeque<Task>,
    timers: Vec<TimerTask>,
    pub program: Vec<OpCode>,
    ip: usize, // Instruction Pointer
}

impl VM {
    pub fn new() -> Self {
        let mut vm = Self {
            stack: Vec::new(),
            call_stack: vec![Frame {
                return_address: 0,
                locals: HashMap::new(),
            }],
            heap: Vec::new(),
            native_functions: Vec::new(),
            task_queue: VecDeque::new(),
            timers: Vec::new(),
            program: Vec::new(),
            ip: 0,
        };
        vm.init_builtins();
        vm
    }

    fn init_builtins(&mut self) {
        // Native function table:
        // 0: console.log
        // 1: setTimeout
        self.native_functions.push(VM::native_log);
        self.native_functions.push(VM::native_set_timeout);

        // console = { log: <native 0> }
        let console_ptr = self.heap.len();
        let mut console_props = HashMap::new();
        console_props.insert("log".to_string(), JsValue::NativeFunction(0));
        self.heap.push(HeapObject {
            data: HeapData::Object(console_props),
        });

        // Global bindings
        let globals = self.call_stack.first_mut().expect("Missing global frame");
        globals
            .locals
            .insert("console".into(), JsValue::Object(console_ptr));
        globals
            .locals
            .insert("setTimeout".into(), JsValue::NativeFunction(1));
    }

    pub fn load_program(&mut self, bytecode: Vec<OpCode>) {
        self.program = bytecode;
        self.ip = 0;
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
        match task.function_ptr {
            JsValue::Function(address) => {
                // Push args in call order so the function prologue `Store(...)` consumes correctly.
                for arg in task.args {
                    self.stack.push(arg);
                }

                let frame = Frame {
                    return_address: usize::MAX, // sentinel: stop when returning
                    locals: HashMap::new(),
                };
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
                if let Some(JsValue::Object(ptr)) = self.stack.pop()
                    && let Some(heap_item) = self.heap.get(ptr)
                {
                    match &heap_item.data {
                        HeapData::Object(props) => {
                            let val = props.get(&name).cloned().unwrap_or(JsValue::Undefined);
                            self.stack.push(val);
                        }
                        HeapData::Array(_) => {
                            if name == "length" {
                                // TODO: array.length
                                self.stack.push(JsValue::Undefined);
                            } else {
                                self.stack.push(JsValue::Undefined);
                            }
                        }
                    }
                }
            }

            OpCode::Push(v) => self.stack.push(v),

            OpCode::Store(name) => {
                let val = self.stack.pop().unwrap();
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

            OpCode::Call(arg_count) => {
                let callee = self.stack.pop().expect("Missing callee");
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.stack.pop().expect("Missing argument"));
                }

                match callee {
                    JsValue::Function(address) => {
                        args.reverse();
                        for arg in &args {
                            self.stack.push(arg.clone());
                        }

                        let frame = Frame {
                            return_address: self.ip + 1,
                            locals: HashMap::new(),
                        };
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
                    println!("DEBUG: Memory Freed at Heap Index {}", ptr);
                }
            }

            OpCode::Add => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Number(a + b));
                } else {
                    self.stack.push(JsValue::Undefined);
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
                let condition = self.stack.pop().expect("Stack underflow in JumpIfFalse");
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

            OpCode::Lt => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Boolean(a < b));
                }
            }

            OpCode::Gt => {
                if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                    (self.stack.pop(), self.stack.pop())
                {
                    self.stack.push(JsValue::Boolean(a > b));
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
                let array_ptr = self.stack.pop().expect("Missing array pointer");
                if let (JsValue::Object(ptr), JsValue::Number(idx)) = (array_ptr, index_val) {
                    if let Some(heap_obj) = self.heap.get(ptr)
                        && let HeapData::Array(arr) = &heap_obj.data
                    {
                        let i = idx as usize;
                        let val = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                        self.stack.push(val);
                    } else {
                        self.stack.push(JsValue::Undefined);
                    }
                }
            }

            OpCode::Halt => return ExecResult::Stop,
        }

        self.ip += 1;
        ExecResult::Continue
    }
    // Example Native Function
    pub fn native_log(_vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        let output: Vec<String> = args.iter().map(|arg| format!("{:?}", arg)).collect();
        println!("LOG: {}", output.join(" "));
        JsValue::Undefined
    }
    pub fn native_set_timeout(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
        // Usage: setTimeout(callback, ms)
        if args.len() >= 1 {
            let callback = args[0].clone();
            let delay_ms = args
                .get(1)
                .and_then(|v| match v {
                    JsValue::Number(n) => Some(*n as u64),
                    _ => None,
                })
                .unwrap_or(0);

            vm.timers.push(TimerTask {
                due: Instant::now() + Duration::from_millis(delay_ms),
                task: Task {
                    function_ptr: callback,
                    args: vec![],
                },
            });
        }
        JsValue::Undefined
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecResult {
    Continue,
    ContinueNoIpInc,
    Stop,
}
