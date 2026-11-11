---
sidebar_position: 10
title: Contributing to Oite Language
description: Guide for contributing to Oite language development. Learn how to set up the development environment, submit PRs, and follow coding standards.
keywords: [contributing, open source, pull request, development, community]
---

# Contributing

Contributions are welcome! This guide will help you get started.

## Getting Started

1. **Fork the repository**
2. **Clone your fork**:
   ```bash
   git clone https://github.com/warpy-ai/script.git
   cd script
   ```
3. **Create a branch**:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Building

See the [Getting Started](/getting-started) guide for build instructions.

## Testing

Run the test suite:

```bash
# Run all tests (unit tests + golden language tests)
cargo test --release

# Golden language tests only (tests/lang/**/*.ot with .out/.err expectations)
cargo test --release --test golden

# Specific IR tests
cargo test --release ir::
```

Read the [contribution rules](https://github.com/warpy-ai/oite/blob/master/CONTRIBUTING.md)
before changing the compiler, the IR or a backend.

## Code Style

- Follow Rust conventions
- Use `rustfmt` for formatting
- Write tests for new features
- Update documentation

### File size

Keep source files small: **at most about 800 lines**, one topic per file.

- New code goes in a new, focused module rather than onto the end of a
  large file.
- A change that would push a file past the limit splits it first.
- A split is its own pull request with no behaviour change, so the tests
  show nothing moved by accident.

These files are over the limit today and are being split, the ones a
change touches first:

| File | Lines |
| ---- | ----: |
| `src/vm/mod.rs` | 3,455 |
| `src/ir/lower.rs` | 2,049 |
| `src/backend/llvm/codegen.rs` | 1,855 |
| `src/ir/mod.rs` | 1,696 |
| `src/backend/cranelift.rs` | 1,582 |
| `src/stdlib/mod.rs` | 1,062 |
| `src/main.rs` | 1,050 |
| `src/compiler/borrow_ck.rs` | 1,025 |
| `src/types/checker.rs` | 1,005 |
| `src/compiler/scope.rs` | 951 |
| `src/ir/opt.rs` | 923 |
| `src/types/inference.rs` | 917 |
| `src/types/mod.rs` | 813 |

## Project Structure

```
script/
├── src/
│   ├── main.rs              # Entry point
│   ├── compiler/            # Rust compiler (SWC-based)
│   ├── ir/                  # SSA IR system
│   ├── runtime/             # Native runtime kernel
│   ├── vm/                  # Stack-based VM (debug)
│   ├── backend/             # Native backends (Cranelift, LLVM)
│   └── stdlib/              # Standard library
├── bootstrap/               # Self-hosting compiler
├── std/                     # Standard prelude
└── examples/                # Example programs
```

## Areas for Contribution

- **Language features**: ES modules, async/await, more stdlib
- **Performance**: Optimizations, profiling, benchmarks
- **Documentation**: Examples, guides, API docs
- **Tooling**: Formatter, linter, LSP
- **Testing**: More test cases, edge cases

## Submitting Changes

1. **Write tests** for your changes
2. **Update documentation** if needed
3. **Run the test suite** to ensure everything passes
4. **Submit a pull request** with a clear description

## License

By contributing, you agree that your contributions will be licensed under the Apache License 2.0.
