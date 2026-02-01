---
slug: self-hosting-journey
title: "Oite v0.4: From VM to Native - The Self-Hosting Journey Begins"
description: Oite enters Phase 4 with the self-hosting compiler journey. Learn what self-hosting means, why it matters, and how Oite is becoming self-compiling.
authors: [lucas]
tags: [compiler, self-hosting, architecture, phase-4]
image: /img/logo_bg.png
---

This week marks a major milestone for Oite: we've officially begun **Phase 4 - Self-Hosting Compiler**. After completing Phase 3 (Language Completion), we're now taking the next step toward making Oite a truly self-contained language that can compile itself. This post explores what self-hosting means, why it matters, and how we're building toward it.

<!-- truncate -->

## What is Self-Hosting?

A **self-hosting compiler** is a compiler written in the language it compiles. Currently, Oite's compiler is written in Rust. The goal of Phase 4 is to port the compiler to Oite itself, so that Oite can compile Oite.

This might sound like a chicken-and-egg problem, but it's actually a well-established pattern in language development. Here's how it works:

```
tscl₀ (Rust compiler) ──compile──> tscl₁ (Oite-compiled binary)
     │                                 │
     │                                 └──compile──> tscl₂ (tscl₁-compiled)
     │                                               │
     │                                               └──verify──> ✓
     │
     └──validate hash(tscl₁) == hash(tscl₂)
```

**Success condition**: If `tscl₁` and `tscl₂` produce bit-for-bit identical binaries, we've achieved self-hosting.

## Why Self-Hosting Matters

Self-hosting is more than just a technical achievement—it's a proof of maturity. Here's why it matters:

### 1. **Language Completeness**

If a language can compile itself, it's complete enough to build real software. You can't write a compiler in a language that's missing critical features. Self-hosting proves that Oite has:
- Sufficient control flow (loops, conditionals, functions)
- Adequate data structures (objects, arrays, strings)
- Proper error handling
- Module system for code organization
- Performance characteristics suitable for large codebases

### 2. **Removing Runtime Dependencies**

Currently, Oite's VM is written in Rust and linked into every binary. While this works, it means:
- Production binaries include the entire Rust runtime
- We're limited by Rust's compilation model
- We can't optimize the runtime as aggressively as we could if it were in Oite

With self-hosting, the compiler itself runs as native code, and we can eventually remove the VM from production builds entirely.

### 3. **Faster Iteration**

Once self-hosting is achieved, we can iterate on the compiler using the compiler itself. This creates a positive feedback loop:
- Improve the compiler → faster compilation
- Faster compilation → easier to improve the compiler
- Better compiler → better language features

### 4. **Community Confidence**

Self-hosting demonstrates that Oite is serious and production-ready. It shows that the language isn't just a toy project, but something that can be used to build real, complex software.

## The Bootstrap Architecture

Our self-hosting strategy follows a three-step bootstrap process:

### Step 1: Stabilize the Output

Before we can port the compiler, we need **deterministic, reproducible builds**. This means:

- **ABI Versioning**: We've frozen the Application Binary Interface (ABI) at version 1. This is the contract between compiled code and the runtime:

```rust
// src/runtime/abi_version.rs
pub const ABI_VERSION: u32 = 1;
```

The ABI defines:
- Function signatures for runtime stubs (`tscl_add_any`, `tscl_alloc_object`, etc.)
- NaN-boxed value encoding (64-bit words)
- Heap object layouts
- Calling conventions

Once frozen, we can't change these without bumping the version.

- **IR Serialization**: We've created a deterministic Intermediate Representation (IR) format:

```bash
./target/release/script build app.ot --emit-ir -o app
```

This produces a stable, text-based IR that can be:
- Verified (`--verify-ir`)
- Serialized and deserialized
- Used for bootstrap verification

The IR format ensures that:
- Register numbering is deterministic
- Block ordering is stable
- Function ordering is lexicographic
- No random seeds or non-deterministic operations

### Step 2: Port the Compiler

The compiler consists of several modules:

| Module | Lines (est) | Priority |
|--------|-------------|----------|
| `lexer.ot` | ~400 | 1 |
| `parser.ot` | ~1200 | 2 |
| `emitter.ot` | ~800 | 2 |
| `ir.ot` | ~600 | 3 |
| `codegen.ot` | ~1000 | 4 |

We'll port incrementally, keeping Rust as the reference implementation. Each module will be tested independently before moving to the next.

### Step 3: Bootstrap Verification

Once the compiler is ported, we verify self-hosting:

```typescript
// tests/bootstrap/loop.ot
export function testBootstrapLoop(): void {
    // Step 1: Compile compiler with Rust tscl
    const tscl1 = runRustTscl("build compiler.ot --dist -o /tmp/tscl1");
    
    // Step 2: Compile compiler with tscl₁
    const tscl2 = runTscl("/tmp/tscl1", "build compiler.ot --dist -o /tmp/tscl2");
    
    // Step 3: Verify bit-for-bit match
    assert(hash("/tmp/tscl1") === hash("/tmp/tscl2"), "Bootstrap not deterministic");
}
```

If `tscl₁` and `tscl₂` produce identical binaries, we've achieved self-hosting.

## ABI Freezing Strategy

The ABI (Application Binary Interface) is the contract between compiled Oite code and the runtime. Freezing it is critical because:

1. **Stability**: Once frozen, we can't change function signatures without breaking compatibility
2. **Verification**: We can verify that the ABI hasn't changed between bootstrap stages
3. **Documentation**: It serves as a clear specification for what the runtime provides

### Current ABI Surface

The ABI consists of ~20 runtime stubs:

```rust
extern "C" {
    // Arithmetic
    fn tscl_add_any(a: u64, b: u64) -> u64;
    fn tscl_sub_any(a: u64, b: u64) -> u64;
    fn tscl_mul_any(a: u64, b: u64) -> u64;
    
    // Allocation
    fn tscl_alloc_object() -> u64;
    fn tscl_alloc_array() -> u64;
    fn tscl_alloc_string(ptr: *const u8, len: usize) -> u64;
    
    // Property access
    fn tscl_get_prop(obj: u64, key: u64) -> u64;
    fn tscl_set_prop(obj: u64, key: u64, val: u64) -> u64;
    
    // Function calls
    fn tscl_call(func: u64, args: u64, arg_count: u32) -> u64;
    
    // Error handling
    fn tscl_abort(msg: *const u8, len: usize) -> !;
}
```

All values are **NaN-boxed** into 64-bit words, allowing us to represent:
- Numbers (as f64)
- Booleans, null, undefined (in NaN space)
- Pointers to heap objects (in NaN space)

### ABI Versioning Rules

1. **No signature changes** without version bump
2. **No NaN-box encoding changes** without version bump
3. **No layout changes** to heap objects without version bump
4. **Version bump required** for any breaking change

We've documented the ABI in `docs/ABI.md` and created compatibility tests to ensure it doesn't drift.

## IR Serialization

The Intermediate Representation (IR) is the bridge between the compiler and code generation. Serializing it allows us to:

1. **Verify determinism**: Same source → same IR (bit-for-bit)
2. **Debug compilation**: Inspect IR at each stage
3. **Bootstrap verification**: Compare IR between bootstrap stages

### IR Format

```text
; ============================================================
; tscl IR Module
; Format version: 1
; ABI version: 1
; ============================================================

fn main() -> any {
    ; Local variables
    local $0: any = console

bb0:
    v0 = const "test"
    v1 = load.local $0
    v2 = call.method v1.log(v0)
    return
}
```

### Determinism Guarantees

1. **Register numbering**: Allocated in definition order, no renumbering
2. **Block ordering**: Entry block first, then by first reference
3. **Function ordering**: Lexicographic by name
4. **No randomness**: Fixed seeds, deterministic hash maps

### CLI Flags

```bash
# Emit IR to file
./target/release/script build app.ot --emit-ir -o app
# → app.ir

# Verify IR validity
./target/release/script build app.ot --verify-ir

# Emit LLVM IR
./target/release/script build app.ot --emit-llvm -o app
# → app.ll

# Emit object file
./target/release/script build app.ot --emit-obj -o app
# → app.o
```

## Performance Implications

Self-hosting has significant performance implications:

### Native Code Generation

Currently, Oite can compile to:
- **Cranelift JIT**: Fast development, ~6x faster than VM
- **LLVM AOT**: Optimized native binaries with LTO

Once self-hosting is complete, the compiler itself will run as native code, providing:
- **Faster compilation**: No VM overhead
- **Better optimization**: LLVM can optimize the compiler itself
- **Smaller binaries**: No VM runtime in production builds

### VM Removal

The VM will remain for:
- **Development mode**: `--dev` flag for debugging
- **REPL**: Interactive console
- **Testing**: Test runner

But production builds (`--release`, `--dist`) will be VM-free, resulting in:
- Smaller binaries
- Faster startup
- Lower memory usage

## Challenges and Risks

Self-hosting is where many languages fail. Here are the risks we're aware of:

### 1. **IR Drift**

**Risk**: The IR format changes during porting, breaking bootstrap.

**Mitigation**: Keep Rust as reference, test incrementally, version the IR format.

### 2. **Non-Determinism**

**Risk**: Builds aren't bit-for-bit identical, breaking verification.

**Mitigation**: Fixed seeds, deterministic ordering, stable linker flags.

### 3. **Bootstrap Loop**

**Risk**: Infinite loop if `tscl₁` can't compile `tscl₂`.

**Mitigation**: Verify at each step, keep Rust compiler as fallback.

### 4. **Performance Regression**

**Risk**: Self-hosted compiler is slower than Rust version.

**Mitigation**: Benchmark suite, performance budgets, incremental optimization.

## What's Next

Phase 4 is a multi-week effort. Here's the roadmap:

- **Week 1-2**: ABI freezing and IR serialization (COMPLETE)
- **Week 3-8**: Port compiler modules (lexer → parser → emitter → IR → codegen)
- **Week 9**: Bootstrap tests and verification
- **Week 10+**: Performance tuning and VM removal

We're taking it step by step, ensuring each stage is solid before moving to the next.

## Conclusion

Self-hosting is a major milestone that proves Oite's maturity and sets the foundation for future growth. By freezing the ABI and creating deterministic IR serialization, we've laid the groundwork for a robust bootstrap process.

The journey from VM to native is challenging, but it's also exciting. Every language that achieves self-hosting joins an elite group of systems that can truly stand on their own.

We're building Oite to be fast, safe, and practical. Self-hosting is the next step in that journey.

---

**Try Oite today:**

```bash
cargo build --release
./target/release/script build hello.ot -o hello
./hello
```

**Learn more:**
- [Oite GitHub Repository](https://github.com/warpy-ai/script)
- [Architecture Documentation](/docs/architecture)
- [Development Status](/docs/development-status)
