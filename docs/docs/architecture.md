---
sidebar_position: 4
title: Script Compiler Architecture
description: Learn about Script's compiler architecture, including the lexer, parser, type checker, SSA IR, and native code generation via Cranelift and LLVM backends.
keywords: [compiler architecture, lexer, parser, type checker, ssa, ir, cranelift, llvm, code generation]
---

# Script Architecture

This document describes the Script ecosystem architecture, the philosophy behind each layer, and the transition path to full self-hosting.

---

## Philosophy

**Script Core is like C without libc** — a minimal, self-contained language that can run without any external dependencies. Everything else is optional layers that add convenience.

| Layer | Required? | Analogy |
|-------|-----------|---------|
| Script Core | Always | C language + basic allocator |
| Rolls | Optional | libc, POSIX, system libraries |
| NPM (via .nroll) | Optional | Third-party C libraries |
| Unroll | Optional | make, cargo, package manager |

---

## Ecosystem Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                             │
│                           USER APPLICATION                                  │
│                           ─────────────────                                 │
│                           myapp.script                                      │
│                                                                             │
│   import { serve } from "@rolls/http";                                      │
│   import { connect } from "@rolls/db";                                      │
│   import lodash from "lodash";  // npm converted to .nroll                  │
│                                                                             │
└───────────────────────────────────┬─────────────────────────────────────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    │                               │
                    ▼                               ▼
┌───────────────────────────────┐   ┌─────────────────────────────────────────┐
│                               │   │                                         │
│       SCRIPT CORE             │   │              ROLLS                      │
│       ───────────             │   │              ─────                      │
│                               │   │                                         │
│  ┌─────────────────────────┐  │   │  Official System Libraries              │
│  │  Compiler (scriptc)     │  │   │                                         │
│  │  ├── Lexer              │  │   │  ┌─────────────────────────────────┐    │
│  │  ├── Parser             │  │   │  │ @rolls/async   Event loop,      │    │
│  │  ├── Type Checker       │  │   │  │                io_uring         │    │
│  │  ├── IR Generator       │  │   │  ├─────────────────────────────────┤    │
│  │  ├── Optimizer          │  │   │  │ @rolls/http    HTTP/1.1, HTTP/2 │    │
│  │  └── Native Codegen     │  │   │  ├─────────────────────────────────┤    │
│  └─────────────────────────┘  │   │  │ @rolls/tls     TLS via rustls   │    │
│                               │   │  ├─────────────────────────────────┤    │
│  ┌─────────────────────────┐  │   │  │ @rolls/db      Database drivers │    │
│  │  Runtime                │  │   │  ├─────────────────────────────────┤    │
│  │  ├── NaN-boxed Values   │  │   │  │ @rolls/fs      File system      │    │
│  │  ├── Heap Allocator     │  │   │  ├─────────────────────────────────┤    │
│  │  └── FFI Stubs          │  │   │  │ @rolls/json    JSON parse/str   │    │
│  │                         │  │   │  ├─────────────────────────────────┤    │
│  └─────────────────────────┘  │   │  │ @rolls/crypto  Hashing, etc.    │    │
│                               │   │  └─────────────────────────────────┘    │
│  ┌─────────────────────────┐  │   │                                         │
│  │  Primitives             │  │   │  Not required for core execution     │
│  │  ├── number, string     │  │   │  Batteries included for real apps   │
│  │  ├── boolean, null      │  │   │  Statically linked into binary      │
│  │  ├── object, array      │  │   │                                         │
│  │  ├── function           │  │   └─────────────────────────────────────────┘
│  │  └── console.log        │  │
│  └─────────────────────────┘  │               ┌─────────────────────────────┐
│                               │               │                             │
│  RUNNABLE WITHOUT ROLLS    │               │      NPM ECOSYSTEM          │
│  RUNNABLE WITHOUT UNROLL   │               │      ─────────────          │
│  SINGLE BINARY OUTPUT      │               │                             │
│                               │               │  Converted to .nroll format │
│  Like C without libc:         │               │  ├── lodash.nroll           │
│  - Can allocate memory        │               │  ├── uuid.nroll             │
│  - Can do math                │               │  ├── zod.nroll              │
│  - Can print output           │               │  └── ...                    │
│  - Can call functions         │               │                             │
│  - Cannot do HTTP (no Rolls)  │               │  Precompiled, no runtime │
│  - Cannot read files (no Rolls)│              │  No node_modules folder  │
│                               │               │  Statically linked       │
└───────────────────────────────┘               │                             │
                                                └──────────────┬──────────────┘
                                                               │
                    ┌──────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                             │
│                              UNROLL                                         │
│                              ──────                                         │
│                                                                             │
│  Build System & Package Manager                                             │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  unroll.toml (Project Manifest)                                     │    │
│  │  ──────────────────────────────                                     │    │
│  │  [package]                                                          │    │
│  │  name = "myapp"                                                     │    │
│  │  version = "1.0.0"                                                  │    │
│  │                                                                     │    │
│  │  [dependencies]                                                     │    │
│  │  "@rolls/http" = "^1.0"                                             │    │
│  │  "@rolls/db" = "^2.0"                                               │    │
│  │  "lodash" = { npm = "^4.17", features = ["collection"] }            │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  ┌─────────────┐   │
│  │ unroll build  │  │ unroll run    │  │ unroll add    │  │ unroll fmt  │   │
│  │               │  │               │  │               │  │             │   │
│  │ Compile all   │  │ Build + Run   │  │ Add dependency│  │ Format code │   │
│  │ Link static   │  │ Dev mode      │  │ Update lock   │  │             │   │
│  └───────────────┘  └───────────────┘  └───────────────┘  └─────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  unroll.lock (Lockfile)                                             │    │
│  │  ──────────────────────                                             │    │
│  │  Deterministic, reproducible builds                                 │    │
│  │  SHA256 verification of all dependencies                            │    │
│  │  Exact versions pinned                                              │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
└───────────────────────────────────┬─────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                             │
│                         FINAL APPLICATION BINARY                            │
│                         ────────────────────────                            │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                                                                     │    │
│  │   ./myapp                                     Single executable     │    │
│  │                                                                     │    │
│  │   ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐   │    │
│  │   │ User Code   │ │ Script Core │ │   Rolls     │ │ NPM (.nroll)│   │    │
│  │   │             │ │  Runtime    │ │  Libraries  │ │  Libraries  │   │    │
│  │   └─────────────┘ └─────────────┘ └─────────────┘ └─────────────┘   │    │
│  │                                                                     │    │
│  │   No runtime installation required                               │    │
│  │   No node_modules                                                │    │
│  │   No dynamic linking (optional)                                  │    │
│  │   Deploy anywhere: copy single file                              │    │
│  │                                                                     │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## What Each Layer Provides

### Script Core (Always Available)

```javascript
// These work without Rolls or Unroll

// Variables and types
let x = 42;
let name = "hello";
let obj = { a: 1, b: 2 };
let arr = [1, 2, 3];

// Functions
function add(a, b) {
    return a + b;
}

// Classes
class Point {
    constructor(x, y) {
        this.x = x;
        this.y = y;
    }
}

// Control flow
if (x > 0) { /* ... */ }
for (let i = 0; i < 10; i++) { /* ... */ }
while (condition) { /* ... */ }

// Console output
console.log("Hello, World!");

// Memory allocation (internal)
let bigArray = new Array(1000000);
```

### Rolls (Optional, Recommended)

```javascript
// These require Rolls

import { serve } from "@rolls/http";
import { readFile } from "@rolls/fs";
import { connect } from "@rolls/db";
import { hash } from "@rolls/crypto";

// HTTP Server
serve({ port: 3000 }, (req) => {
    return new Response("Hello!");
});

// File System
let content = await readFile("config.json");

// Database
let db = await connect("postgres://...");
```

### NPM via .nroll (Optional)

```javascript
// NPM packages converted to .nroll format
import _ from "lodash";          // lodash.nroll
import { v4 as uuid } from "uuid"; // uuid.nroll
import { z } from "zod";          // zod.nroll

let id = uuid();
let sorted = _.sortBy(items, 'name');
```

---

## Compilation Flow

```
                    ┌─────────────────────┐
                    │   Source Files      │
                    │   *.script          │
                    └──────────┬──────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           SCRIPT CORE COMPILER                              │
│                                                                             │
│   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────────┐   │
│   │  Lexer  │──▶│ Parser  │──▶│  Type   │──▶│   IR    │──▶│   Native    │   │
│   │         │   │   AST   │   │  Check  │   │  + Opt  │   │   Codegen   │   │
│   └─────────┘   └─────────┘   └─────────┘   └─────────┘   └──────┬──────┘   │
│                                                                  │          │
└──────────────────────────────────────────────────────────────────┼──────────┘
                                                                   │
                    ┌──────────────────────────────────────────────┘
                    │
                    ▼
          ┌─────────────────┐
          │  Object Files   │
          │  *.o            │
          └────────┬────────┘
                   │
                   │  + Rolls (if imported)
                   │  + NPM .nroll (if imported)
                   │  + Runtime stubs
                   │
                   ▼
          ┌─────────────────┐
          │     Linker      │
          │  (LLD / system) │
          └────────┬────────┘
                   │
                   ▼
          ┌─────────────────┐
          │  Executable     │
          │  ./myapp        │
          └─────────────────┘
```

---

## Transition Path: Rust → Self-Hosted

This diagram shows the evolution from the Rust-based compiler to the fully self-hosted Script compiler.

```
═══════════════════════════════════════════════════════════════════════════════
                           COMPILER EVOLUTION
═══════════════════════════════════════════════════════════════════════════════

RUST FOUNDATION Complete
───────────────────────────

    ┌─────────────────────────────────────────────────────────────────────┐
    │                                                                     │
    │   src/compiler/ (Rust)              Production compiler             │
    │   ════════════════════                                              │
    │                                                                     │
    │   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────────────┐     │
    │   │   SWC   │──▶│Bytecode │──▶│  SSA IR │──▶│ LLVM / Cranelift│     │
    │   │ Parser  │   │  Gen    │   │  + Opts │   │    Backend      │     │
    │   └─────────┘   └─────────┘   └─────────┘   └────────┬────────┘     │
    │                                                      │              │
    │                                                      ▼              │
    │                                              ┌───────────────┐      │
    │                                              │ Native Binary │      │
    │                                              └───────────────┘      │
    │                                                                     │
    └─────────────────────────────────────────────────────────────────────┘

    ┌─────────────────────────────────────────────────────────────────────┐
    │                                                                     │
    │   bootstrap/*.tscl                  Reference implementation        │
    │   ════════════════                                                  │
    │                                                                     │
    │   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────────────┐     │
    │   │  Lexer  │──▶│ Parser  │──▶│   IR    │──▶│    Bytecode     │     │
    │   │ (.tscl) │   │ (.tscl) │   │ (.tscl) │   │     Output      │     │
    │   └─────────┘   └─────────┘   └─────────┘   └────────┬────────┘     │
    │                                                      │              │
    │                                                      ▼              │
    │                                              ┌───────────────┐      │
    │                                              │   Rust VM     │      │
    │                                              │  (executes)   │      │
    │                                              └───────────────┘      │
    │                                                                     │
    └─────────────────────────────────────────────────────────────────────┘


FEATURE PARITY Complete
──────────────────────────

    ┌─────────────────────────────────────────────────────────────────────┐
    │                                                                     │
    │   compiler/*.tscl                   Modular, production-ready       │
    │   ═══════════════                                                   │
    │                                                                     │
    │   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐             │
    │   │  Lexer  │──▶│ Parser  │──▶│  Type   │──▶│   IR    │             │
    │   │         │   │         │   │  Check  │   │ + Opts  │             │
    │   └─────────┘   └─────────┘   └─────────┘   └────┬────┘             │
    │                                                  │                  │
    │                 ADDED:                            │                  │
    │                 ══════                           │                  │
    │                 Type inference                │                  │
    │                 Optimizations (DCE, const fold)                  │
    │                 Borrow checker                ▼                  │
    │                                          ┌─────────────┐            │
    │                                          │  Bytecode   │            │
    │                                          └──────┬──────┘            │
    │                                                 │                   │
    │                                                 ▼                   │
    │                                          ┌─────────────┐            │
    │                                          │  Rust VM    │            │
    │                                          └─────────────┘            │
    │                                                                     │
    └─────────────────────────────────────────────────────────────────────┘


NATIVE CODEGEN Complete (via LLVM IR)
────────────────────────────────────────

    ┌─────────────────────────────────────────────────────────────────────┐
    │                                                                     │
    │   compiler/*.tscl (scriptc)         Full native compiler            │
    │   ═════════════════════════                                         │
    │                                                                     │
    │   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐             │
    │   │  Lexer  │──▶│ Parser  │──▶│  Type   │──▶│   IR    │             │
    │   │         │   │         │   │  Check  │   │ + Opts  │             │
    │   └─────────┘   └─────────┘   └─────────┘   └────┬────┘             │
    │                                                  │                  │
    │                                    ┌─────────────┴─────────────┐    │
    │                                    │                           │    │
    │                                    ▼                           ▼    │
    │                             ┌─────────────┐             ┌──────────┐│
    │                             │   x86-64    │             │  ARM64   ││
    │                             │   Backend   │             │  Backend ││
    │                             └──────┬──────┘             └────┬─────┘│
    │                                    │                         │      │
    │                                    └───────────┬─────────────┘      │
    │                                                │                    │
    │                 ADDED:                      ▼                    │
    │                 ══════                  ┌─────────────┐             │
    │                 x86-64 codegen       │   Native    │             │
    │                 ARM64 codegen        │   Binary    │             │
    │                 ELF/Mach-O writer    └─────────────┘             │
    │                 Register allocator                               │
    │                 No Rust needed!                                  │
    │                                                                     │
    └─────────────────────────────────────────────────────────────────────┘


BOOTSTRAP VERIFICATION In Progress
──────────────────────────────────────

    ┌─────────────────────────────────────────────────────────────────────┐
    │                                                                     │
    │   BOOTSTRAP VERIFICATION                                            │
    │   ══════════════════════                                            │
    │                                                                     │
    │                                                                     │
    │   ┌───────────────────┐                                             │
    │   │                   │                                             │
    │   │  tscl₀ = Rust     │  Original Rust compiler                     │
    │   │  compiler         │                                             │
    │   │                   │                                             │
    │   └─────────┬─────────┘                                             │
    │             │                                                       │
    │             │ compiles compiler/*.tscl                              │
    │             ▼                                                       │
    │   ┌───────────────────┐                                             │
    │   │                   │                                             │
    │   │  tscl₁ = scriptc  │  First native scriptc                       │
    │   │  (built by Rust)  │                                             │
    │   │                   │                                             │
    │   └─────────┬─────────┘                                             │
    │             │                                                       │
    │             │ compiles compiler/*.tscl                              │
    │             ▼                                                       │
    │   ┌───────────────────┐                                             │
    │   │                   │                                             │
    │   │  tscl₂ = scriptc  │  Self-compiled scriptc                      │
    │   │  (built by tscl₁) │                                             │
    │   │                   │                                             │
    │   └─────────┬─────────┘                                             │
    │             │                                                       │
    │             │ compiles compiler/*.tscl                              │
    │             ▼                                                       │
    │   ┌───────────────────┐                                             │
    │   │                   │                                             │
    │   │  tscl₃ = scriptc  │  Should be identical to tscl₂               │
    │   │  (built by tscl₂) │                                             │
    │   │                   │                                             │
    │   └───────────────────┘                                             │
    │                                                                     │
    │                                                                     │
    │   ┌─────────────────────────────────────────────────────────────┐   │
    │   │                                                             │   │
    │   │   VERIFICATION:  sha256(tscl₂) == sha256(tscl₃)             │   │
    │   │                                                             │   │
    │   │   If hashes match → Deterministic, reproducible compiler    │   │
    │   │   If hashes differ → Bug in codegen, must fix               │   │
    │   │                                                             │   │
    │   └─────────────────────────────────────────────────────────────┘   │
    │                                                                     │
    │                                                                     │
    │   FINAL STATE:                                                      │
    │   ════════════                                                      │
    │                                                                     │
    │   scriptc compiles itself                                        │
    │   No Rust compiler needed for development                        │
    │   Deterministic builds verified                                  │
    │   src/compiler/ (Rust) kept for reference/testing only           │
    │                                                                     │
    └─────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════
                              PROGRESS
═══════════════════════════════════════════════════════════════════════════════

    Foundation       Feature Parity   Native Codegen   Bootstrap Verify
    ══════════       ══════════════   ══════════════   ════════════════

    [Rust+Bootstrap] [Type+Borrow]    [LLVM IR Gen]    [Self-Compile]
         │                │                │                │
         │   ~5,400       │   ~10,500      │    +1,348      │  In Progress
         │   lines        │   lines        │    lines       │
         │                │                │                │
    ─────┴────────────────┴────────────────┴────────────────┴─────────────▶

       [x]               [x]              [x]              [ ]
    COMPLETE          COMPLETE        COMPLETE        IN PROGRESS


═══════════════════════════════════════════════════════════════════════════════
```

---

## Repository Structure (Final State)

After bootstrap verification is complete:

```
script/
├── compiler/                     # THE compiler (scriptc) - MAIN
│   ├── main.tscl                 # Entry point
│   ├── lexer/                    # Tokenization
│   ├── parser/                   # AST generation
│   ├── ast/                      # AST types
│   ├── passes/                   # Compiler passes
│   │   ├── typecheck.tscl        # Type inference
│   │   ├── opt.tscl              # Optimizations
│   │   └── borrow_ck.tscl        # Ownership checking
│   ├── ir/                       # SSA IR
│   ├── backend/                  # Native codegen
│   │   ├── x86_64/               # x86-64 backend
│   │   ├── arm64/                # ARM64 backend
│   │   └── object/               # ELF/Mach-O writers
│   ├── codegen/                  # Bytecode gen (for VM)
│   └── stdlib/                   # Built-in declarations
│
├── src/                          # Rust support code
│   ├── runtime/                  # KEEP: ABI, heap, stubs
│   │   ├── abi.rs                # NaN-boxed values
│   │   ├── heap.rs               # Memory allocation
│   │   └── stubs.rs              # FFI bridge
│   └── vm/                       # KEEP: For debugging
│
├── bootstrap/                    # ARCHIVE: Reference only
│
└── docs/
    ├── ARCHITECTURE.md           # This file
    ├── SELF_HOSTING.md           # Detailed roadmap
    └── future/
        ├── rolls-design.md       # Rolls architecture
        └── unroll-design.md      # Unroll architecture
```

---

## Key Principles

### 1. Minimal Core
Script Core contains only what's necessary to run code:
- Compiler
- Basic types
- Memory allocation
- Console output

### 2. Optional Batteries
Rolls provides system functionality but is never required:
- HTTP, TLS, Database
- File system, Crypto
- All statically linked

### 3. No Runtime Dependencies
Final binaries have no external dependencies:
- No node_modules
- No dynamic linking (optional)
- No runtime installation

### 4. Reproducible Builds
Everything is deterministic:
- Lockfiles pin exact versions
- SHA256 verification
- Bit-for-bit reproducible with `--dist`

### 5. Progressive Enhancement
Use only what you need:
```
Script Core only     → Minimal binary, ~100KB
+ Rolls              → Full-featured, ~1-5MB
+ NPM libraries      → Ecosystem access, varies
```

---

## Comparison with Other Ecosystems

| Aspect | Script | Node.js | Go | Rust |
|--------|--------|---------|-----|------|
| Core without stdlib | Yes | No | No | Yes (#![no_std]) |
| Static linking | Default | No | Default | Default |
| Single binary | Yes | No | Yes | Yes |
| Package manager | Unroll | npm | go mod | Cargo |
| Self-hosted compiler | Yes | No (C++) | Yes | Yes |
