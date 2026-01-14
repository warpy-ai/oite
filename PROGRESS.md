🚀 Scoped-JS VM: Low-Level Bytecode Engine
A custom-built JavaScript interpreter and Virtual Machine (VM) implemented in Rust. Unlike standard engines that rely entirely on Garbage Collection, this engine utilizes a Borrow Checker and Explicit Ownership model to manage memory safety at compile-time.

🏗️ Core Architecture
The engine is divided into three primary stages:

Parsing: Powered by swc_ecma_parser to transform JS/TS code into an Abstract Syntax Tree (AST).

Borrow Checker (Middle-end): A custom static analysis pass that enforces ownership rules, distinguishing between "Move" semantics for Heap objects and "Copy" semantics for Primitives.

VM (Back-end): A stack-based Virtual Machine that executes custom bytecode instructions with a managed Heap and Call Stack.

✅ Features Developed

1. Memory Management (The "Rust" Secret)
   Ownership Model: Variables "own" their data. Assigning an object to a new variable moves ownership, invalidating the original variable.

Lifetime Tracking: Prevents moving an object if active borrows (references) are still pointing to it.

Scoped Lifetimes: Variables are automatically dropped (freed) when their containing Frame or Block ends.

Stack vs. Heap: Primitives (Numbers, Booleans) live on the Stack (Copy); Objects and Arrays live on the Heap (Move).

2. Virtual Machine (Execution)
   Stack-based Architecture: Uses a LIFO stack for expressions and operations.

Call Stack & Frames: Supports nested function calls with isolated local scopes.

Heap Allocation: A dedicated storage area for dynamic data like Objects and Arrays.

Native Bridge: Ability to inject Rust functions (e.g., console.log) directly into the JavaScript environment.

3. Language Support
   Functions: Arguments passing, return values, and local variable scoping.

Objects & Arrays: Support for object literals {a: 1}, array literals [1, 2], property access obj.a, and indexed access arr[0].

Control Flow: if/else branching and while loops using backpatched jumps.

Comparisons: Full support for >, <, and ===.

Explicit Borrowing: Implementation of the void operator hijacked for explicit reference creation.

📜 Bytecode Instruction Set
OpCode Description
Push(Value) Pushes a constant onto the stack.
Store(Name) Moves a value from the stack to a local variable.
Load(Name) Pushes a variable's value onto the stack.
NewObject Allocates a new empty object on the Heap.
SetProp(Key) Sets a property on a heap object.
Call Executes a Bytecode or Native function.
JumpIfFalse(N) Branches the execution if the condition is falsy.
Drop(Name) Manually frees a variable and its heap data.
🛠️ Example Trace
Source Code:

JavaScript

let count = 3;
while (count > 0) {
console.log(count);
count = count - 1;
}
Compiler Logic:

Borrow Check: Ensures count is a primitive and can be used in the comparison and the subtraction without moving.

Loop Label: Marks the start of the condition.

Jump Logic: If count > 0 is false, it jumps to the Halt instruction at the end.

Native Call: Loads the console object and calls the native Rust log function.

🚀 Next Steps
Event Loop: Implementing a Task Queue to support setTimeout and asynchronous I/O.

Standard Library: Adding fs (File System) and net (Networking) native bindings for Node.js compatibility.

Garbage-Free Refinement: Implementing Reference Counting (RC) for shared ownership of heap objects.
