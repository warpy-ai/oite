---
sidebar_position: 1
title: Welcome to Script
description: Script is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler. Get started with Script today.
keywords:
  [
    script language,
    programming language,
    javascript alternative,
    native code,
    high performance,
  ]
---

# Welcome to Script

**Script** is a high-performance JavaScript-like scripting language with native code execution, featuring a self-hosting compiler and Rust-inspired memory safety.

```javascript
function fib(n) {
  if (n < 2) return n;
  return fib(n - 1) + fib(n - 2);
}

console.log(fib(35)); // Compiled to native code!
```

## Key Features

- **Native Execution** — SSA-based IR compiled to native code via Cranelift/LLVM
- **Link-Time Optimization** — ThinLTO and Full LTO for maximum performance
- **Standalone Binaries** — Self-contained executables with runtime stubs in LLVM IR
- **Memory Safety** — Ownership model with compile-time borrow checking
- **Self-Hosting Complete** — Compiler written in Script, generates LLVM IR → native binaries
- **Type Inference** — Hindley-Milner type analysis with generics
- **TypeScript Syntax** — Familiar syntax with full ES6+ and TypeScript features
- **Classes & Inheritance** — ES6 classes with extends, super(), and private fields
- **Error Handling** — try/catch/finally with exception propagation
- **Async/Await** — Native async functions with Promise support

## Motive

I've been amazed by the performance of languages like Rust and Go, which I've been working on the backend for a while. And looking a Javascript/Typescript I've always wanted to have the same performance, but I've never been able to achieve it.

First I've tried to create a hybrid server frameworkd , I was studying on building a javascript library: a hybrid Rust+JavaScript web framework that combines Hyper's HTTP performance with JavaScript's flexibility.

So I’ve developed a solution , and the initial benchmark were … honest : 31,136 requests/second.

Not bad! I was able to beat Express.js by 55%. But I wanted more. I wanted to hit 88,000 req/sec , where fast javascript libraries , like Fastify, usually sits. But was never able to achieve it.

Then I turn into the runtimes, primarly nodejs and then bun, I've studied how they works, and how bun excels in performance and were to achieve it, and I've realized that the best way to achieve it, was to create a language that is a mix of Rust and Javascript/Typescript.

## Philosophy

- **Performance** — I want to have the same performance as Rust and Go, but with the ease of use of Javascript/Typescript.
- **Memory Safety** — I want to have the same memory safety as Rust, but with the ease of use of Javascript/Typescript.
- **Type Safety** — I want to have the same type safety as Rust, but with the ease of use of Javascript/Typescript.
- **Ease of Use** — I want to have the same ease of use as Javascript/Typescript, but with the performance of Rust and Go.

And then Script was born. It's still on it's early stages, I call it a preview , and not yet production ready, but I've been able to achieve the performance I wanted, and the memory safety I wanted, and the type safety I wanted, and the ease of use I wanted.

So give it a try, and let me know what you think.

## Quick Start

```bash
# Build
cargo build --release

# Run a script
./target/release/script myprogram.tscl

# Build to native binary (requires LLVM)
./target/release/script build myprogram.tscl --release -o myprogram

# Run the compiled binary
./myprogram
```

## What's Next?

- Read the [Getting Started](/docs/getting-started) guide
- Explore [Language Features](/docs/language-features)
- Learn about the [Architecture](/docs/architecture)
- Check the [Development Status](/docs/development-status)
