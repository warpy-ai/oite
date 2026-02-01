---
slug: introducing-oite
title: "Introducing Oite: JavaScript That Runs Like Rust"
description: After years of development, Oite is here. A language that compiles JavaScript and TypeScript to native machine code with Rust-inspired memory safety and zero-overhead abstractions.
authors: [lucas]
tags: [release, announcement, performance, memory-safety, typescript]
image: /img/owl-light.png
---

NOTE: The project is currently in preview and it will change its name. You can give your suggestions about the name change here: https://github.com/warpy-ai/script/discussions/20

NOTE2: Most of the code in this project is written by a human, me. AI is used selectively to speed up repetitive tasks, generate initial drafts, and it wrote most of the documentation ( which will be reviewed and edited by a human more the project advances). Architecture, system design, and implementation decisions were hand-written, even more on wrong implementation/bugs. If you're against "AI Slop", consider move away from this project.

<!-- truncate -->

## Introduction

After years of starting, killing, restarting, and refining, I finally realised a dream: operating JavaScript at the low level. And I'm giving it to the world!

Oite compiles JavaScript and TypeScript to native machine code, without garbage collection, featuring:

- Rust-inspired ownership & borrow checking
- Native compilation via LLVM & Cranelift
- Full TypeScript syntax with type inference
- Zero-overhead abstractions & standalone binaries

It compiles down to SSA-based IR and produces self-contained executables that run with native performance. As a result, we finally have a language that writes like JS but runs like Rust!

<!-- truncate -->

## The Journey

I've been amazed by the performance of languages like Rust and Go, which I've been working on the backend for a while. And looking at JavaScript/TypeScript, I've always wanted to have the same performance, but I've never been able to achieve it.

First, I tried to create a hybrid server framework. I was studying on building a JavaScript library: a hybrid Rust+JavaScript web framework that combines Hyper's HTTP performance with JavaScript's flexibility.

So I developed a solution, and the initial benchmarks were... honest: **31,136 requests/second**.

Not bad! I was able to beat Express.js by 55%. But I wanted more. I wanted to hit **88,000 req/sec**, where fast JavaScript libraries, like Fastify, usually sit. But I was never able to achieve it.

I've even tried to use LLM's to improve the performance, which fails miserably, and you can read more about it -> [here](https://medium.com/@jucasoliveira/llm-tried-to-cheat-benchmarks-it-failed-c0f5b42400db).

Then I turned to the runtimes, primarily Node.js and then Bun. I studied how they work, and how Bun excels in performance and where to achieve it, and I realized that the best way to achieve it was to create a language that is a mix of Rust and JavaScript/TypeScript.

## Philosophy

Oite was born from four core principles:

- **Performance** - I want to have the same performance as Rust and Go, but with the ease of use of JavaScript/TypeScript.
- **Memory Safety** - I want to have the same memory safety as Rust, but with the ease of use of JavaScript/TypeScript.
- **Type Safety** - I want to have the same type safety as Rust, but with the ease of use of JavaScript/TypeScript.
- **Ease of Use** - I want to have the same ease of use of JavaScript/TypeScript, but with the performance of Rust and Go.

And then Oite was born. It's still in its early stages, I call it a preview, and not yet production ready, but I've been able to achieve the performance I wanted, the memory safety I wanted, the type safety I wanted, and the ease of use I wanted.

## What Makes Oite Different

### Native Compilation

Oite doesn't use a virtual machine or JIT compilation. Instead, it compiles directly to native machine code using LLVM and Cranelift. This means:

- **Standalone binaries**: No runtime dependencies
- **Fast startup**: No warmup time, instant execution
- **Predictable performance**: No JIT compilation overhead
- **Small binaries**: Only what you need, nothing more

### Rust-Inspired Memory Safety

Oite brings Rust's ownership model to JavaScript with moves and borrows:

```typescript
// Move: ownership transfers, original becomes invalid
let data = [1, 2, 3];
let newOwner = data;      // Move ownership
// console.log(data);     // ✗ Compile error: use after move
console.log(newOwner);    // ✓ Works fine

// Borrow: reference without taking ownership
let items = [4, 5, 6];
let ref = &items;         // Immutable borrow
console.log(ref[0]);      // ✓ Can read through reference
console.log(items[0]);    // ✓ Original still valid

// Mutable borrow: exclusive write access
let buffer = [0, 0, 0];
let mutRef = &mut buffer;
mutRef[0] = 42;           // ✓ Can mutate through &mut
// let other = &buffer;   // ✗ Compile error: already mutably borrowed
```

No lifetime annotations needed—Oite infers them automatically. This eliminates entire classes of bugs:

- Use-after-free
- Double-free
- Memory leaks
- Data races

### Full TypeScript Support

Oite understands TypeScript syntax and type inference:

```typescript
// Type inference works just like TypeScript
function add(a: number, b: number): number {
  return a + b;
}

// Type checking at compile time
let result = add(5, 10); // ✓ Type safe
// let error = add("5", 10);  // ✗ Compile error
```

### Zero-Overhead Abstractions

Oite's abstractions compile away to nothing:

```typescript
// High-level code
const doubled = numbers.map((x) => x * 2);

// Compiles to efficient native code
// No function call overhead, no allocations
```

## Current Status

Oite has reached a **major milestone**: the compiler is now **fully self-hosting** and can generate native binaries!

**Core Language Complete**

**Phase 0: Runtime Kernel**

- NaN-boxed values for efficient memory representation
- Unified heap allocator
- FFI stubs for native backends

**Phase 1: SSA IR System**

- Register-based SSA intermediate representation
- Flow-sensitive type inference
- Dead code elimination, constant folding, CSE
- Borrow checking for memory safety

**Phase 2: Native Backend**

- Cranelift JIT for fast development builds
- LLVM AOT with ThinLTO and Full LTO
- Multi-function JIT with tiered compilation
- VM interpreter for debugging

**Phase 3: Language Completion**

- Full TypeScript syntax support (types, interfaces, enums)
- ES6 classes with inheritance and private fields
- Async/await with Promise support
- Try/catch/finally error handling
- ES module system (import/export)

**Phase 4: Self-Hosting Compiler**

- Bootstrap compiler written in Oite (~5,000 lines)
- Modular compiler architecture (~3,500 lines)
- Generates LLVM IR from Oite source
- Native binaries ~30x faster than VM
- 113 tests passing

  **What You Can Do Today**

```bash
# Self-hosted compiler generates LLVM IR
./script compiler/main.ot llvm myapp.ot

# Compile to native binary
clang myapp.ot.ll -o myapp

# Run at native speed!
./myapp
```

**Minimal Standard Library**

Oite core is intentionally minimal (like C without libc):

- `console.log`, `console.error`
- `ByteStream` for binary data
- Basic `fs` operations (readFileSync, writeFileSync)
- Everything else comes from the Rolls ecosystem (coming soon)

**Next Phase: Rolls Ecosystem**

- Standard libraries (`@rolls/http`, `@rolls/tls`, `@rolls/fs`)
- Package manager and build system (Unroll)
- Language server and developer tools
- Production-ready performance optimizations

## Performance Benchmarks

Real-world performance with the self-hosted compiler:

| Execution Mode       | Description                      | Performance             |
| -------------------- | -------------------------------- | ----------------------- |
| **VM**               | Bytecode interpreter (debugging) | Baseline                |
| **Cranelift JIT**    | Fast compilation for development | ~6x faster than VM      |
| **Native (LLVM IR)** | Self-hosted compiler output      | **~30x faster than VM** |

**Compilation Performance:**

- Arithmetic operations: 2.34 µs/iter (VM) → 0.39 µs/iter (JIT)
- JIT compilation time: ~980 µs per function
- Break-even point: ~500 iterations

**Native Binary Examples:**

- Fibonacci(25): Matches Rust performance
- Object/array operations: Full native speed
- Function calls and recursion: Zero overhead

_Note: Full benchmarks against Node.js and Bun will come with the Rolls ecosystem (HTTP server, etc.)_

## Getting Started

Try Oite today:

```bash
# Clone the repository
git clone https://github.com/warpy-ai/script
cd script

# Build the compiler
cargo build --release

# Compile your first program
./target/release/script build hello.ot -o hello
./hello
```

Write your code in TypeScript:

```javascript
// hello.ts
const name: string = "Oite";
console.log(`Hello, ${name}!`);
```

And get a native binary that runs with Rust-like performance.

## What's Next

With the core language complete and self-hosting achieved, the focus shifts to the ecosystem:

- **Rolls System Libraries**: `@rolls/http`, `@rolls/tls`, `@rolls/fs`, `@rolls/crypto`, `@rolls/async`
- **Unroll Tooling**: Package manager, build system, and project scaffolding
- **Developer Experience**: Language server (LSP), debugger, profiler
- **Performance Tuning**: Further LLVM optimizations, profile-guided optimization
- **Production Hardening**: Comprehensive test coverage, real-world validation
- **Documentation**: Complete API reference, tutorials, and examples

The self-hosted compiler opens the door to rapid iteration—now we can improve Oite by writing Oite!

## Conclusion

Oite represents a new approach to JavaScript: take the syntax and ease of use developers love, but give them the performance and safety of systems languages. It's still early, but the foundation is solid.

So give it a try, and let me know what you think. This is just the beginning.

---

**Get Started:**

- [GitHub Repository](https://github.com/warpy-ai/script)
- [Getting Started Guide](/docs/getting-started)
- [Language Features](/docs/language-features)
- [Architecture Overview](/docs/architecture)

**Join the Community:**

- Report issues and suggest features
- Contribute to the standard library
- Share your Oite projects
- Help shape the future of Oite
