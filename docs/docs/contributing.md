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
# Run all tests
cargo test --release

# Run specific IR tests
cargo test --release ir::

# Output: 94 tests passed
```

## Code Style

- Follow Rust conventions
- Use `rustfmt` for formatting
- Write tests for new features
- Update documentation

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
