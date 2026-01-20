---
sidebar_position: 9
---

# Performance

Script is designed for high performance, targeting speeds comparable to C and Rust while maintaining JavaScript-like syntax.

## Benchmarks

| Benchmark | Target |
|-----------|--------|
| `fib(35)` | 20ms |
| Startup | 5ms |
| HTTP hello | 250k rps |

## Performance Targets

| Benchmark | Node.js | Bun | Target Script |
|-----------|--------|-----|---------------|
| HTTP hello world | 100k rps | 200k rps | 250k rps |
| JSON parse | 1x | 1.5x | 2x |
| `fib(35)` | 50 ms | 30 ms | 20 ms |
| Startup | 30 ms | 10 ms | 5 ms |

## JIT vs VM

The Cranelift JIT backend is significantly faster than the VM:

```
=== Summary ===
VM:        2.34 µs/iter
JIT:       0.39 µs/iter
JIT compilation:  980 µs

JIT is 5.98x faster than VM
Break-even point: 503 iterations
```

## Optimization Techniques

### Type Specialization

Dynamic operations are specialized based on type inference:

| Before | After | Speedup |
|--------|-------|---------|
| `add.any v0, v1` | `add.num v0, v1` | ~10x |
| `mul.any v0, v1` | `mul.num v0, v1` | ~10x |

### Link-Time Optimization

LLVM AOT backend supports multiple optimization levels:

- **Dev build**: Basic optimizations
- **Release (`--release`)**: ThinLTO for fast builds with good optimization
- **Dist (`--dist`)**: Full LTO for maximum performance

### Tiered Compilation

The JIT backend uses tiered compilation:
- Baseline compilation for cold code
- Optimized compilation for hot code (after threshold)

## Measuring Performance

### Benchmark Command

```bash
./target/release/script bench examples/bench_arithmetic.tscl
```

### Inspecting IR

```bash
./target/release/script ir myprogram.tscl
```

Shows the IR at various stages:
- Before optimization
- After type inference
- After optimizations

This helps identify optimization opportunities.

## Performance Tips

1. **Use type annotations** - Helps the compiler specialize operations
2. **Prefer numeric operations** - Specialized numeric ops are ~10x faster
3. **Use `--release` or `--dist`** - Enables LTO for maximum performance
4. **Profile your code** - Use the IR dump to see what optimizations are applied
