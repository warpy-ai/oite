# 🚀 Scoped-JS VM

A custom-built JavaScript interpreter and Virtual Machine implemented in **Rust**. Unlike standard engines that rely entirely on Garbage Collection, this engine utilizes a **Borrow Checker** and **Explicit Ownership** model to manage memory safety at compile-time.

## 🏗️ Architecture

The engine is divided into three primary stages:

```
┌─────────────┐    ┌─────────────────┐    ┌────────────────┐
│   Parsing   │───▶│  Borrow Checker │───▶│  Stack-based   │
│  (SWC AST)  │    │   (Middle-end)  │    │   VM (Back-end)│
└─────────────┘    └─────────────────┘    └────────────────┘
```

| Stage              | Description                                                                                                                |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------- |
| **Parsing**        | Powered by `swc_ecma_parser` to transform JS/TS code into an Abstract Syntax Tree (AST)                                    |
| **Borrow Checker** | Custom static analysis pass enforcing ownership rules — _Move_ semantics for Heap objects, _Copy_ semantics for Primitives |
| **VM**             | Stack-based Virtual Machine executing custom bytecode with a managed Heap and Call Stack                                   |

## ✨ Features

### Memory Management (The "Rust" Secret)

- **Ownership Model** — Variables "own" their data. Assigning an object to a new variable _moves_ ownership, invalidating the original
- **Lifetime Tracking** — Prevents moving an object if active borrows (references) still point to it
- **Scoped Lifetimes** — Variables are automatically dropped (freed) when their containing Frame or Block ends
- **Stack vs. Heap** — Primitives (`Number`, `Boolean`) live on the Stack (Copy); Objects and Arrays live on the Heap (Move)

### Virtual Machine

- **Stack-based Architecture** — Uses a LIFO stack for expressions and operations
- **Call Stack & Frames** — Supports nested function calls with isolated local scopes
- **Heap Allocation** — Dedicated storage area for dynamic data like Objects and Arrays
- **Native Bridge** — Inject Rust functions (e.g., `console.log`) directly into the JavaScript environment

### Language Support

- **Functions** — Arguments passing, return values, and local variable scoping
- **Objects & Arrays** — Object literals `{a: 1}`, array literals `[1, 2]`, property access `obj.a`, and indexed access `arr[0]`
- **Control Flow** — `if`/`else` branching and `while` loops using backpatched jumps
- **Comparisons** — Full support for `>`, `<`, and `===`
- **Explicit Borrowing** — `void` operator hijacked for explicit reference creation

## 📜 Bytecode Instruction Set

| OpCode           | Description                                      |
| ---------------- | ------------------------------------------------ |
| `Push(Value)`    | Pushes a constant onto the stack                 |
| `Store(Name)`    | Moves a value from the stack to a local variable |
| `Load(Name)`     | Pushes a variable's value onto the stack         |
| `NewObject`      | Allocates a new empty object on the Heap         |
| `SetProp(Key)`   | Sets a property on a heap object                 |
| `Call`           | Executes a Bytecode or Native function           |
| `JumpIfFalse(N)` | Branches execution if the condition is falsy     |
| `Drop(Name)`     | Manually frees a variable and its heap data      |

## 🛠️ Example

**Source Code:**

```javascript
let count = 3;
while (count > 0) {
  console.log(count);
  count = count - 1;
}
```

**Compiler Logic:**

1. **Borrow Check** — Ensures `count` is a primitive and can be used in the comparison and subtraction without moving
2. **Loop Label** — Marks the start of the condition
3. **Jump Logic** — If `count > 0` is false, jumps to the `Halt` instruction at the end
4. **Native Call** — Loads the `console` object and calls the native Rust `log` function

## 🚀 Roadmap

- [ ] **Event Loop** — Implementing a Task Queue to support `setTimeout` and asynchronous I/O
- [ ] **Standard Library** — Adding `fs` (File System) and `net` (Networking) native bindings for Node.js compatibility
- [ ] **Garbage-Free Refinement** — Implementing Reference Counting (RC) for shared ownership of heap objects

## 📦 Getting Started

```bash
# Build the project
cargo build --release

# Run the interpreter
cargo run
```

## 📄 License

See [LICENSE](./LICENSE) for details.
