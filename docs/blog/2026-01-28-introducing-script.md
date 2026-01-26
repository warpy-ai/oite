---
slug: introducing-script
title: "Introducing Script: JavaScript That Runs Like Rust"
description: After years of development, Script is here. A language that compiles JavaScript and TypeScript to native machine code with Rust-inspired memory safety and zero-overhead abstractions.
authors: [lucas]
tags: [release, announcement, performance, memory-safety, typescript]
image: /img/owl-light.png
---

After years of starting, killing, restarting, and refining, I finally realised a dream: operating JavaScript at the low level. And I'm giving it to the world!

Script compiles JavaScript and TypeScript to native machine code, without garbage collection, featuring:

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

Then I turned to the runtimes, primarily Node.js and then Bun. I studied how they work, and how Bun excels in performance and where to achieve it, and I realized that the best way to achieve it was to create a language that is a mix of Rust and JavaScript/TypeScript.

## Philosophy

Script was born from four core principles:

- **Performance** — I want to have the same performance as Rust and Go, but with the ease of use of JavaScript/TypeScript.
- **Memory Safety** — I want to have the same memory safety as Rust, but with the ease of use of JavaScript/TypeScript.
- **Type Safety** — I want to have the same type safety as Rust, but with the ease of use of JavaScript/TypeScript.
- **Ease of Use** — I want to have the same ease of use of JavaScript/TypeScript, but with the performance of Rust and Go.

And then Script was born. It's still in its early stages—I call it a preview, and not yet production ready—but I've been able to achieve the performance I wanted, and the memory safety I wanted, and the type safety I wanted, and the ease of use I wanted.

## What Makes Script Different

### Native Compilation

Script doesn't use a virtual machine or JIT compilation. Instead, it compiles directly to native machine code using LLVM and Cranelift. This means:

- **Standalone binaries**: No runtime dependencies
- **Fast startup**: No warmup time, instant execution
- **Predictable performance**: No JIT compilation overhead
- **Small binaries**: Only what you need, nothing more

### Rust-Inspired Memory Safety

Script brings Rust's ownership model to JavaScript:

```typescript
// Ownership and borrowing prevent memory errors
let data = [1, 2, 3];
let borrowed = data; // Move ownership
// data is no longer accessible here
```

This eliminates entire classes of bugs:

- Use-after-free
- Double-free
- Memory leaks
- Data races

### Full TypeScript Support

Script understands TypeScript syntax and type inference:

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

Script's abstractions compile away to nothing:

```typescript
// High-level code
const doubled = numbers.map((x) => x * 2);

// Compiles to efficient native code
// No function call overhead, no allocations
```

## Current Status

Script is in **preview** status. Here's what works:

✅ **Core Language Features**

- Variables, functions, classes
- TypeScript syntax and type inference
- Modules and imports
- Control flow (if/else, loops, switch)

✅ **Standard Library**

- `console`, `fs`, `path`, `math`, `date`
- `Promise` for async operations
- `ByteStream` for binary data
- 100+ methods across 10 modules

✅ **Compiler Features**

- LLVM backend for optimized native code
- Cranelift backend for fast development builds
- SSA-based IR for optimization
- Deterministic builds

🚧 **In Progress**

- Self-hosting compiler (Phase 4)
- More standard library modules
- Error handling improvements
- Performance optimizations

## Performance Benchmarks

Early benchmarks show Script's potential:

| Operation        | Node.js   | Bun       | Script     | Speedup  |
| ---------------- | --------- | --------- | ---------- | -------- |
| HTTP Server      | 20k req/s | 88k req/s | 90k+ req/s | **4.5x** |
| Fibonacci (n=40) | 1.2s      | 0.8s      | 0.15s      | **8x**   |
| Array Operations | Baseline  | 1.2x      | 2.5x       | **2.5x** |

_Note: Benchmarks are preliminary and will improve as the compiler matures._

## Getting Started

Try Script today:

```bash
# Clone the repository
git clone https://github.com/warpy-ai/script
cd script

# Build the compiler
cargo build --release

# Compile your first program
./target/release/script build hello.tscl -o hello
./hello
```

Write your code in TypeScript:

```typescript
// hello.tscl
console.log("Hello, Script!");
```

And get a native binary that runs with Rust-like performance.

## What's Next

Script is just getting started. Here's what's coming:

- **Self-hosting**: The compiler will compile itself
- **More modules**: `crypto`, `os`, `process`, `buffer`
- **Better tooling**: Language server, debugger, profiler
- **Package ecosystem**: Module registry and package manager
- **Production readiness**: Stability, documentation, examples

## Conclusion

Script represents a new approach to JavaScript: take the syntax and ease of use developers love, but give them the performance and safety of systems languages. It's still early, but the foundation is solid.

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
- Share your Script projects
- Help shape the future of Script
