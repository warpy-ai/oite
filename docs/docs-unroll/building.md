---
sidebar_position: 9
title: Building Projects
description: How to build Oite projects with unroll, including build profiles, cross-compilation, and optimization.
keywords: [build, compile, profiles, release, optimization, cross-compilation, lto]
---

# Building Projects

Unroll provides a multi-profile build system with support for optimization levels, link-time optimization, and cross-compilation.

## Basic Build

```bash
unroll build
```

Compiles all `.ot` files in `./src/` and produces output in `./target/dev/`.

## Build Profiles

Three built-in profiles control optimization and debugging:

### Debug (Default)

```bash
unroll build
```

| Setting | Value |
|---------|-------|
| Optimization | `-O0` (none) |
| Debug symbols | Yes |
| LTO | None |
| Output | `./target/dev/{name}` |

Best for development: fast compilation, full debug information.

### Release

```bash
unroll build --release
# or
unroll build -r
```

| Setting | Value |
|---------|-------|
| Optimization | `-O3` (maximum) |
| Debug symbols | No |
| LTO | Thin |
| Output | `./target/release/{name}` |

Best for production: optimized code with reasonable build times.

### Distribution

```bash
unroll build --dist
```

| Setting | Value |
|---------|-------|
| Optimization | `-O3` (maximum) |
| Debug symbols | No |
| LTO | Fat (full) |
| Output | `./target/dist/{name}` |

Best for shipping: maximum optimization, smallest binary. Slower build times than release.

## Profile Comparison

| Feature | Dev | Release | Dist |
|---------|-----|---------|------|
| Build speed | Fast | Medium | Slow |
| Binary size | Large | Medium | Small |
| Runtime speed | Slow | Fast | Fastest |
| Debug info | Yes | No | No |
| LTO | None | Thin | Fat |

## Build Flags

```bash
unroll build [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `--release`, `-r` | Release profile |
| `--dist` | Distribution profile |
| `--target <TARGET>` | Cross-compilation target |
| `--jobs <N>`, `-j <N>` | Parallel compilation jobs |
| `--verbose`, `-v` | Show compilation commands |
| `--no-std` | Disable implicit `@rolls/std` |

## Build Process

The build pipeline:

```
1. Load manifest (unroll.toml)
2. Load lockfile (unroll.lock) if present
3. Discover source files (./src/**/*.ot)
4. Compile each source → object file (.o)
5. Compile dependencies (from lockfile)
6. Link all objects → binary
```

### Source Discovery

Unroll recursively discovers all `.ot` files in `./src/`:

```
src/
├── main.ot          → target/dev/main.o
├── server.ot        → target/dev/server.o
├── routes/
│   ├── api.ot       → target/dev/routes_api.o
│   └── web.ot       → target/dev/routes_web.o
└── utils/
    └── helpers.ot   → target/dev/utils_helpers.o
```

Object files are named by flattening the path: `routes/api.ot` becomes `routes_api.o`.

### Verbose Output

```bash
unroll build --verbose
```

Shows each file being compiled:

```
Building my-app (dev)...
Compiling: ./src/main.ot
Compiling: ./src/server.ot
Compiling: ./src/routes/api.ot
Built: ./target/dev/my-app
```

## Running

### Build and Run

```bash
unroll run
```

Finds `src/main.ot` and executes it through the oitec interpreter. This does not go through the full compile-link pipeline; it interprets the source directly.

### Pass Arguments

```bash
unroll run -- --port 8080 --verbose
```

Everything after `--` is passed to your program via `process.argv`.

### Run Release Build

```bash
unroll run --release
```

## Watch Mode

Automatically rebuild when files change:

```bash
unroll watch
```

Options:

```bash
unroll watch --release        # Rebuild in release mode
unroll watch --run            # Rebuild and execute
```

Monitors `./src/` for file changes. Press `Ctrl+C` to stop.

## Cross-Compilation

Build for a different platform:

```bash
unroll build --target aarch64-unknown-linux-gnu
```

### Supported Targets

| Target | Description |
|--------|-------------|
| `native` | Host platform (default) |
| `x86_64-unknown-linux-gnu` | Linux x64 (glibc) |
| `x86_64-unknown-linux-musl` | Linux x64 (static) |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-pc-windows-msvc` | Windows x64 |
| `wasm32-unknown-unknown` | WebAssembly |
| `wasm32-wasi` | WebAssembly + WASI |

## Link-Time Optimization (LTO)

LTO performs optimization across compilation unit boundaries:

| Mode | Description | Build Time | Binary Size |
|------|-------------|------------|-------------|
| None | No cross-unit optimization | Fastest | Largest |
| Thin | Parallel, less aggressive | Medium | Medium |
| Fat | Full optimization | Slowest | Smallest |

Configure in profiles:

```toml
[profile.release]
lto = "thin"

[profile.dist]
lto = "fat"
```

## Cleaning Build Artifacts

```bash
unroll clean
```

Deletes the entire `./target/` directory.

## Output Layout

```
target/
├── dev/
│   ├── my-app           # Debug binary
│   ├── main.o           # Object files
│   └── ...
├── release/
│   └── my-app           # Release binary
├── dist/
│   └── my-app           # Distribution binary
└── doc/
    └── index.html       # Generated documentation
```

## Compiler Detection

Unroll locates the oitec compiler by checking (in order):

1. `OITEC_PATH` environment variable
2. `oitec` in system `PATH`
3. `./oitec` (current directory)
4. `../oite/target/debug/oite` (development)
5. `../oite/target/release/oite` (development)
6. `/usr/local/bin/oitec`
7. `~/.oite/bin/oitec`

If not found, the build fails with: "Could not find oitec compiler."
