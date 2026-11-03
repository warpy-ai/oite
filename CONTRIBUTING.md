# Contributing to Oite

Contributions are welcome, by people and by coding agents. Oite is a new
language, so neither can lean on prior knowledge of how it behaves: these rules
are the shared ground. They exist because each one was broken at least once and
the result shipped as a silently wrong program.

Setup, building and project layout are in
[docs/docs/contributing.md](docs/docs/contributing.md) and
[docs/docs/getting-started.md](docs/docs/getting-started.md).

## The rules

### 1. Tests decide what is correct

A change to the language is not done until a golden test shows it.

- Golden tests live in `tests/lang/**`. Each `name.ot` program sits next to
  `name.out` (exact expected stdout, exit 0) or `name.err` (must exit non-zero;
  every line must appear in stderr, in order).
- Run them with `cargo test --release --test golden`. Narrow with
  `OITE_GOLDEN_FILTER=operators`.
- Write the expected output **by hand** from what the program should do. Do not
  copy whatever a backend prints today. `OITE_BLESS=1` rewrites `.out` files from
  current output; use it only after checking the diff line by line.
- Every bug fix starts as a golden test that fails, using the reproduction from
  the issue.

### 2. Native code is the target; a backend matches the expectation or refuses

Oite compiles to native code: LLVM AOT (`oitec build`) for production and the
Cranelift JIT (`oitec jit`) for development. The VM (`oitec file.ot`) is the
debugging interpreter. A feature is done when it runs natively; working only
in the VM is a step on the way, not the goal.

The `.out` / `.err` files define correct behaviour. No backend is the
reference, including the VM. A backend that cannot run a construct correctly
must fail with an error. It must never approximate.

- In `src/ir/lower.rs`, an opcode the native backends do not implement returns
  `LowerError::NotSupportedNatively`. Do not lower it to a no-op or to
  `undefined`.
- The `// backends: vm jit aot` directive at the top of a test runs it on
  each listed backend against the same expectation. Any test the native
  backends can run lists `jit aot`; the goal is every test on `aot`. Add them
  to a test as soon as it passes there. A test for a construct the native
  backends refuse can list `jit aot` with a `.err` file to pin the refusal.
- Language semantics for native code live in the runtime
  (`src/runtime/stubs.rs`), which both native backends call. Put behaviour
  there rather than in one backend's code generator.

### 3. Nothing is dropped silently

- No `_ => {}` arm in the code generator (`src/compiler/mod.rs`). A construct
  that cannot be compiled calls `self.unsupported("…")`, which fails
  compilation and reports a line and column.
- No `eprintln!("Warning: …")` followed by carrying on. If the result would be
  wrong, it is an error.
- Parser errors that the parser recovered from are still errors.

### 4. Every IR op is handled everywhere, or rejected everywhere

Adding an `IrOp` (`src/ir/mod.rs`) means handling it in Cranelift (`src/backend/cranelift.rs`) and in LLVM (`src/backend/llvm/codegen.rs`),
or rejecting it in lowering until both can. Lowering must also not
assume a value on the operand stack survives a jump: `spill_stack` and
`reload_stack` carry stack values across block edges.

### 5. Keep moves and behaviour changes in separate commits

Renaming, moving and reformatting go in their own commits, with no change in
behaviour, so the golden tests prove they changed nothing. Bug fixes and
features go in other commits.

### 6. Docs describe what runs today

Do not mark a feature complete unless golden tests cover it on the backends
the claim names. [docs/docs/development-status.md](docs/docs/development-status.md)
holds the current matrix; update it in the same PR as the change.

### 7. No debug output in committed code

No `DEBUG` prints, no leftover `println!` tracing. Keep diagnostics behind a
flag or remove them.

## Before you open a PR

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --release            # includes the golden tests
bash scripts/bootstrap_verify.sh
```

Attach the issue's reproduction and its output before and after the change.

## Language decisions that are still open

Some behaviour is not decided yet ([#135](https://github.com/warpy-ai/oite/issues/135)). Do not settle these as a side effect
of a fix. Open or comment on an issue instead:

- the string model: what `length` and indexing count ([#133](https://github.com/warpy-ai/oite/issues/133))
- which operations throw, and the error hierarchy ([#121](https://github.com/warpy-ai/oite/issues/121))
- `let b = a` for objects: currently a **move**; one doc page still says copy
- type checking of annotations ([#124](https://github.com/warpy-ai/oite/issues/124))
- memory reclamation ([#123](https://github.com/warpy-ai/oite/issues/123))

## License

By contributing, you agree that your contributions are licensed under the
Apache License 2.0.
