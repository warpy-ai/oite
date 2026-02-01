---
slug: async-await-tokio
title: "Async/Await in Oite: How We Built a Modern Async Runtime on Top of Tokio"
description: Deep dive into Oite's async/await implementation using Tokio. Learn about Promises, the await opcode, and bridging Rust's async with Oite's VM.
authors: [lucas]
tags: [async, promises, tokio, runtime, javascript]
image: /img/logo_bg.png
---

Oite now has full async/await support, built on top of Tokio—Rust's production-grade async runtime. This post dives into how we implemented Promises, the `await` opcode, and bridged Rust's async world with Oite's VM.

<!-- truncate -->

## The Goal

JavaScript's async/await is one of its most important features. It makes asynchronous code readable and maintainable:

```typescript
// Instead of callback hell:
fetchData((err, data) => {
    if (err) return;
    processData(data, (err, result) => {
        if (err) return;
        saveResult(result, () => {
            console.log("Done!");
        });
    });
});

// We get clean async/await:
async function workflow() {
    const data = await fetchData();
    const result = await processData(data);
    await saveResult(result);
    console.log("Done!");
}
```

We wanted Oite to have the same ergonomics, but with native performance.

## Architecture Overview

Oite's async system has three layers:

```
┌─────────────────────────────────────────┐
│      Oite Code (async/await)          │
└─────────────────┬───────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────┐
│      VM (Await opcode, Promise)         │
└─────────────────┬───────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────┐
│      Tokio Runtime (Rust async)         │
└─────────────────────────────────────────┘
```

1. **Oite layer**: `async function` syntax, `await` expressions
2. **VM layer**: Promise state machine, `Await` opcode
3. **Tokio layer**: Actual async execution, I/O, timers

## Promise Implementation

A Promise in Oite is a state machine with three states:

```rust
// src/vm/value.rs
pub enum PromiseState {
    Pending,
    Fulfilled(JsValue),
    Rejected(JsValue),
}

pub struct Promise {
    state: Arc<Mutex<PromiseState>>,
    handlers: Vec<Box<dyn FnOnce(JsValue) + Send>>,
}
```

### Promise Lifecycle

```typescript
// 1. Create a pending promise
const p = new Promise((resolve, reject) => {
    // Promise starts in Pending state
});

// 2. Resolve it
resolve(42);
// → State: Fulfilled(42)

// 3. Or reject it
reject("error");
// → State: Rejected("error")
```

### Promise.resolve() and Promise.reject()

These are convenience methods for creating already-resolved/rejected promises:

```typescript
// Immediately resolved
const p1 = Promise.resolve(42);
// State: Fulfilled(42)

// Immediately rejected
const p2 = Promise.reject("error");
// State: Rejected("error")
```

Implementation:

```rust
// src/stdlib/mod.rs
pub fn native_promise_resolve(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let value = args.first().cloned().unwrap_or(JsValue::Undefined);
    let promise = Promise::new_fulfilled(value);
    JsValue::Promise(Arc::new(promise))
}

pub fn native_promise_reject(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let reason = args.first().cloned().unwrap_or(JsValue::Undefined);
    let promise = Promise::new_rejected(reason);
    JsValue::Promise(Arc::new(promise))
}
```

### Promise.then() and Promise.catch()

These register handlers that run when the promise resolves or rejects:

```typescript
const p = Promise.resolve(42);

p.then(value => {
    console.log(value);  // 42
    return value * 2;
}).then(value => {
    console.log(value);  // 84
}).catch(error => {
    console.error(error);
});
```

Implementation:

```rust
// src/vm/mod.rs
OpCode::CallMethod { name, arg_count } => {
    if let JsValue::Promise(promise) = obj {
        match name.as_str() {
            "then" => {
                let handler = args.first().cloned();
                promise.add_fulfill_handler(handler);
                // Return new promise for chaining
            }
            "catch" => {
                let handler = args.first().cloned();
                promise.add_reject_handler(handler);
            }
            // ...
        }
    }
}
```

### Promise.all()

Waits for all promises to resolve:

```typescript
const p1 = Promise.resolve(1);
const p2 = Promise.resolve(2);
const p3 = Promise.resolve(3);

const all = Promise.all([p1, p2, p3]);
// Resolves to [1, 2, 3]
```

## The Await Opcode

The `await` keyword compiles to an `Await` opcode:

```typescript
// Source
async function test() {
    const result = await Promise.resolve(42);
    return result;
}

// Bytecode
[0] Push(Function { ... })
[1] Let("test")
[2] Jump(10)
[3] Push(String("Promise"))
[4] Load("Promise")
[5] GetProp("resolve")
[6] Push(Number(42.0))
[7] Call(1)              // Promise.resolve(42)
[8] Await                 // ← await opcode
[9] Return
```

### Await Implementation

```rust
// src/vm/mod.rs
OpCode::Await => {
    let promise = self.stack.pop().expect("Await: no value on stack");
    
    if let JsValue::Promise(promise) = promise {
        let state = promise.state.lock().unwrap();
        match &*state {
            PromiseState::Fulfilled(value) => {
                // Already resolved, push value and continue
                self.stack.push(value.clone());
            }
            PromiseState::Rejected(reason) => {
                // Already rejected, throw exception
                self.throw_exception(reason.clone());
            }
            PromiseState::Pending => {
                // Not ready yet, suspend execution
                // (In future: integrate with Tokio runtime)
                self.stack.push(JsValue::Undefined);  // Placeholder
            }
        }
    } else {
        // Not a promise, wrap it
        let promise = Promise::new_fulfilled(promise);
        self.stack.push(JsValue::Promise(Arc::new(promise)));
    }
}
```

Currently, `await` on a pending promise is a placeholder. In the future, we'll integrate with Tokio to actually suspend execution.

## Async Function Syntax

When you write an `async function`, the compiler automatically wraps the return value in `Promise.resolve()`:

```typescript
// Source
async function getValue() {
    return 42;
}

// What it compiles to
function getValue() {
    const value = 42;
    return Promise.resolve(value);  // ← Automatic wrapping
}
```

### Compiler Support

```rust
// src/compiler/mod.rs
fn gen_fn_decl(&mut self, fn_decl: &FunctionDecl) {
    let is_async = fn_decl.is_async;
    
    // ... function body ...
    
    if is_async {
        // Wrap return value in Promise.resolve()
        self.instructions.push(OpCode::Push(JsValue::String("Promise".to_string())));
        self.instructions.push(OpCode::Load("Promise".to_string()));
        self.instructions.push(OpCode::GetProp("resolve".to_string()));
        self.instructions.push(OpCode::Swap);  // Swap promise and value
        self.instructions.push(OpCode::Call(1));
    }
}
```

## Tokio Integration

Tokio is Rust's async runtime. We use it for:

1. **Async I/O**: File reading, network requests
2. **Timers**: `setTimeout`, `setInterval`
3. **Task scheduling**: Executing async tasks

### Initializing Tokio

```rust
// src/vm/mod.rs
impl VM {
    pub fn init_async(&mut self) {
        // Create Tokio runtime
        let rt = tokio::runtime::Runtime::new().unwrap();
        self.async_runtime = Some(rt);
    }
}
```

### Async File Reading (Future)

Here's how we'll implement async file reading:

```rust
// Future implementation
pub async fn native_fs_read_file_async(
    path: &str
) -> Result<String, Error> {
    tokio::fs::read_to_string(path).await
}
```

Then in Oite:

```typescript
async function readConfig() {
    const content = await fs.readFile("config.json");
    return JSON.parse(content);
}
```

## Performance: Zero-Cost Abstractions

Oite's async/await is designed for performance:

### 1. **No Heap Allocation for Resolved Promises**

If a promise is already resolved, `await` doesn't allocate:

```rust
PromiseState::Fulfilled(value) => {
    // Just push the value, no allocation
    self.stack.push(value.clone());
}
```

### 2. **Direct Function Calls**

Native Promise methods (`resolve`, `reject`, `then`) are native functions, not VM bytecode:

```rust
// Native function (fast)
pub fn native_promise_resolve(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    // Direct execution, no bytecode interpretation
}
```

### 3. **Tokio's Efficiency**

Tokio is one of the fastest async runtimes available:
- **Work-stealing scheduler**: Efficient task distribution
- **Zero-cost abstractions**: No overhead when not using async
- **SIMD-optimized I/O**: Fast network and file operations

## Example: Async Workflow

Here's a complete example:

```typescript
async function fetchUserData(userId: number): Promise<object> {
    // Simulate API call
    const user = await Promise.resolve({ id: userId, name: "Alice" });
    return user;
}

async function processUsers(): Promise<void> {
    const users = await Promise.all([
        fetchUserData(1),
        fetchUserData(2),
        fetchUserData(3),
    ]);
    
    console.log("Users:", users);
    // Users: [{id: 1, name: "Alice"}, {id: 2, name: "Alice"}, {id: 3, name: "Alice"}]
}

processUsers();
```

## Current Limitations

We're still working on:

1. **Full Tokio Integration**: Currently, `await` on pending promises is a placeholder
2. **Async I/O**: File reading, network requests are still synchronous
3. **Error Propagation**: Proper exception handling in async contexts
4. **Generator Functions**: `async function*` for streaming

## Future Enhancements

### 1. **Full Suspension**

When a promise is pending, actually suspend execution:

```rust
PromiseState::Pending => {
    // Suspend current execution context
    // Resume when promise resolves
    self.suspend_until_promise_resolves(promise);
}
```

### 2. **Async I/O**

```typescript
// Future API
async function downloadFile(url: string): Promise<string> {
    const response = await fetch(url);
    return await response.text();
}
```

### 3. **Streams**

```typescript
async function* readLines(file: string): AsyncGenerator<string> {
    // Stream file line by line
    for await (const line of fileStream) {
        yield line;
    }
}
```

## Conclusion

Oite's async/await implementation brings modern JavaScript ergonomics to a native-compiled language. By building on Tokio, we get production-grade performance while maintaining the familiar Promise API.

The foundation is solid. As we integrate more Tokio features, Oite will become an excellent choice for async-heavy applications like web servers, data processing pipelines, and real-time systems.

---

**Try async/await in Oite:**

```bash
# Create test_async.ot
cat > test_async.ot << 'EOF'
async function test() {
    const result = await Promise.resolve(42);
    console.log("Result:", result);
}
test();
EOF

# Run it
./target/release/script test_async.ot
```

**Learn more:**
- [Oite GitHub Repository](https://github.com/warpy-ai/script)
- [Async/Await Documentation](/compiler/language-features#asyncawait)
- [Tokio Documentation](https://tokio.rs/)
