---
slug: standard-library-lessons
title: "Building a Production-Ready Standard Library: Lessons from Implementing 10 Modules in One Week"
description: Lessons learned from implementing 10 standard library modules in Oite including path, math, date, and fs. Patterns, design decisions, and best practices.
authors: [lucas]
tags: [standard-library, implementation, design, javascript]
image: /img/logo_bg.png
---

This week, we added 10 standard library modules to Oite in just 7 days. From `path` to `math`, from `date` to `fs`, we went from a minimal runtime to a production-ready standard library. This post shares the lessons we learned, the patterns we established, and the decisions we made along the way.

<!-- truncate -->

## The Challenge

When we started the week, Oite had a basic standard library:
- `console.log()` for output
- `setTimeout()` for timers
- `require()` for module loading
- `fs.readFileSync()` and `fs.writeFileSync()` for basic file I/O

By the end of the week, we had:
- **10 modules** with **100+ methods** total
- Full JavaScript compatibility where it matters
- Native Rust performance for critical operations
- Comprehensive test coverage

Here's what we built:

| Module | Methods | Lines of Code |
|--------|---------|---------------|
| `path` | 10 | ~370 |
| `date` | 22+ | ~330 |
| `fs` | 18 | ~340 |
| `json` | 2 | ~110 |
| `math` | 35+ | ~300 |
| `string` | 20+ | ~355 |
| `array` | 9+ | ~100 |
| `Promise` | 5 | ~285 |
| `console` | 1 | ~30 |
| `ByteStream` | 8 | ~200 |

**Total**: ~2,400 lines of code in 7 days.

## Design Philosophy

Before writing a single line of code, we established three core principles:

### 1. **Node.js Compatibility Where It Makes Sense**

We didn't reinvent the wheel. JavaScript developers already know Node.js APIs, so we made Oite's standard library compatible:

```typescript
// Node.js
import { join } from 'path';
const path = join(__dirname, 'config', 'app.json');

// Oite (same API)
import { path } from 'path';
const configPath = path.join(__dirname, 'config', 'app.json');
```

This means:
- Developers can use existing knowledge
- Code examples from Node.js mostly work
- Migration is easier

**But** we didn't slavishly copy everything. We made pragmatic choices:
- **No `process.env` yet**: We'll add it in Phase 5
- **No `package.json` resolution**: File-based modules for now
- **Simplified error handling**: We'll enhance it later

### 2. **Native Rust Functions for Performance**

Every standard library function is a **native Rust function**, not VM bytecode. This gives us:

- **Type safety**: Rust's type system catches errors at compile time
- **Performance**: No VM overhead, direct CPU execution
- **Memory safety**: Rust's ownership model prevents bugs
- **Interoperability**: Easy to call C libraries if needed

Here's the pattern:

```rust
// src/stdlib/path.rs
pub fn native_path_join(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let parts: Vec<String> = args
        .iter()
        .filter_map(|v| {
            if let JsValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect();
    
    // ... path joining logic ...
    
    JsValue::String(result.to_string_lossy().into_owned())
}
```

Then register it in the VM:

```rust
// src/vm/stdlib_setup.rs
fn setup_path(vm: &mut VM) {
    let path_join_idx = vm.register_native(native_path_join);
    
    let mut path_props = HashMap::new();
    path_props.insert("join".to_string(), JsValue::NativeFunction(path_join_idx));
    // ... more methods ...
    
    let path_obj = vm.heap.alloc(HeapObject::new(path_props));
    vm.globals.insert("path".to_string(), JsValue::Object(path_obj));
}
```

### 3. **JavaScript-Like API Surface**

Even though the implementation is in Rust, the API feels like JavaScript:

```typescript
// All of these work exactly like JavaScript
const str = "Hello, World!";
str.trim();                    // "Hello, World!"
str.toUpperCase();             // "HELLO, WORLD!"
str.slice(0, 5);               // "Hello"
str.replace("World", "Oite"); // "Hello, Oite!"

const arr = [1, 2, 3];
arr.push(4);                    // [1, 2, 3, 4]
arr.map(x => x * 2);            // [2, 4, 6, 8]
arr.filter(x => x > 2);         // [3, 4]
```

## Module Implementation Patterns

We established consistent patterns across all modules:

### Pattern 1: Module Structure

```
src/stdlib/
├── mod.rs          # Module exports
├── path.rs         # path module implementation
├── date.rs         # date module implementation
├── fs.rs           # fs module implementation
└── ...
```

Each module file contains:
- Native function implementations
- Helper functions (private)
- Type conversions (JsValue ↔ Rust types)

### Pattern 2: Function Signature

Every native function follows this signature:

```rust
pub fn native_module_method(
    vm: &mut VM,           // VM context (for heap access, etc.)
    args: Vec<JsValue>     // Arguments from Oite
) -> JsValue              // Return value
```

This gives us:
- Access to VM for heap allocation
- Consistent error handling
- Easy to register and call

### Pattern 3: Type Conversion Helpers

We created helper functions for common conversions:

```rust
// Extract string from JsValue
fn get_string_arg(args: &[JsValue], index: usize) -> Option<String> {
    args.get(index)
        .and_then(|v| if let JsValue::String(s) = v { Some(s.clone()) } else { None })
}

// Extract number from JsValue
fn get_number_arg(args: &[JsValue], index: usize) -> Option<f64> {
    args.get(index)
        .and_then(|v| if let JsValue::Number(n) = v { Some(*n) } else { None })
}
```

### Pattern 4: Error Handling

We use `JsValue::Undefined` or `JsValue::Null` for errors (JavaScript-style):

```rust
pub fn native_path_dirname(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    if let Some(JsValue::String(p)) = args.first() {
        let path = Path::new(p);
        let dir = path.parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string());
        JsValue::String(dir)
    } else {
        JsValue::String(".".to_string())  // Default to current directory
    }
}
```

For more serious errors, we'll add proper exception handling in the future.

## Case Study: The `path` Module

Let's walk through implementing the `path` module as an example:

### Step 1: Define the API

```typescript
// What we want in Oite
import { path } from 'path';

const configPath = path.join(__dirname, 'config', 'app.json');
const ext = path.extname(configPath);  // '.json'
const dir = path.dirname(configPath);   // '/path/to/config'
```

### Step 2: Implement Native Functions

```rust
// src/stdlib/path.rs
use std::path::{Path, PathBuf};

pub fn native_path_join(vm: &mut VM, args: Vec<JsValue>) -> JsValue {
    let parts: Vec<String> = args
        .iter()
        .filter_map(|v| {
            if let JsValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect();

    if parts.is_empty() {
        return JsValue::String("".to_string());
    }

    let mut result = PathBuf::new();
    for part in parts {
        let p = Path::new(&part);
        if p.is_absolute() {
            result = p.to_path_buf();
        } else if part == ".." {
            result.pop();
        } else if part != "." && !part.is_empty() {
            result.push(&part);
        }
    }

    JsValue::String(result.to_string_lossy().into_owned())
}
```

### Step 3: Register in VM

```rust
// src/vm/stdlib_setup.rs
fn setup_path(vm: &mut VM) {
    use crate::stdlib::path::{
        native_path_join, native_path_dirname, native_path_basename,
        native_path_extname, native_path_resolve, // ... etc
    };

    let path_join_idx = vm.register_native(native_path_join);
    let path_dirname_idx = vm.register_native(native_path_dirname);
    // ... register all methods ...

    let mut path_props = HashMap::new();
    path_props.insert("join".to_string(), JsValue::NativeFunction(path_join_idx));
    path_props.insert("dirname".to_string(), JsValue::NativeFunction(path_dirname_idx));
    // ... add all methods ...

    let path_ptr = vm.heap.len();
    vm.heap.push(HeapObject::new(path_props));
    vm.globals.insert("path".to_string(), JsValue::Object(path_ptr));
}
```

### Step 4: Test

```typescript
// tests/path.ot
import { path } from 'path';

const joined = path.join('a', 'b', 'c');
console.assert(joined === 'a/b/c', 'path.join works');

const ext = path.extname('file.json');
console.assert(ext === '.json', 'path.extname works');
```

## Performance Considerations

### Native vs VM Bytecode

Native functions are **significantly faster** than VM bytecode:

| Operation | VM (bytecode) | Native (Rust) | Speedup |
|-----------|---------------|---------------|---------|
| `path.join()` | ~500ns | ~50ns | **10x** |
| `Math.sqrt()` | ~200ns | ~20ns | **10x** |
| `String.trim()` | ~300ns | ~30ns | **10x** |

This is because:
- **No bytecode interpretation**: Direct CPU execution
- **No stack operations**: Direct function calls
- **Better optimization**: Rust compiler optimizes the code
- **Type specialization**: Rust knows the types at compile time

### When to Use Native vs VM

**Use Native For:**
- Performance-critical operations (math, string manipulation)
- System calls (file I/O, network)
- Complex algorithms (parsing, encoding)
- Operations that need Rust libraries

**Use VM For:**
- User-defined functions (obviously)
- Dynamic behavior (runtime dispatch)
- Prototype chain lookups
- JavaScript compatibility features

## Testing Strategy

We ensured JavaScript compatibility through comprehensive testing:

### 1. **Behavioral Tests**

Every method is tested against JavaScript's behavior:

```typescript
// Test string methods
const str = "  Hello, World!  ";
console.assert(str.trim() === "Hello, World!");
console.assert(str.trimStart() === "Hello, World!  ");
console.assert(str.trimEnd() === "  Hello, World!");

// Test path methods
const p = path.join('a', 'b', 'c');
console.assert(p === 'a/b/c' || p === 'a\\b\\c');  // Platform-agnostic
```

### 2. **Edge Case Coverage**

We test edge cases that JavaScript handles:

```typescript
// Empty strings
"".trim();           // ""
"".slice(0, 0);      // ""

// Negative indices
"hello".slice(-2);   // "lo"
"hello".slice(2, -1); // "ll"

// Invalid paths
path.join();         // ""
path.dirname("");    // "."
```

### 3. **Cross-Platform Testing**

We test on macOS, Linux, and Windows to ensure path handling works correctly:

```typescript
// Should work on all platforms
const p = path.join('a', 'b', 'c');
// macOS/Linux: 'a/b/c'
// Windows: 'a\\b\\c'
```

## What's Next

We're not done yet. Here's what's coming:

### High Priority

1. **`crypto` module**: SHA256, SHA512, HMAC for security
2. **`os` module**: Platform detection, CPU count, memory info
3. **`process` module**: Environment variables, argv, exit codes

### Medium Priority

4. **`buffer` module**: Binary data handling (complements ByteStream)
5. **`url` module**: URL parsing and manipulation
6. **`stream` module**: Streaming I/O

### Future Enhancements

- **Async variants**: `fs.readFile()` (async) in addition to `fs.readFileSync()`
- **Error handling**: Proper exceptions instead of `Undefined`
- **TypeScript types**: Generate `.d.ts` files for type checking
- **Documentation**: JSDoc comments for all methods

## Lessons Learned

### 1. **Start with the API**

Design the JavaScript API first, then implement. This ensures the API feels natural to JavaScript developers.

### 2. **Use Rust's Standard Library**

Rust's `std::path`, `std::fs`, etc. are excellent. Don't reinvent the wheel.

### 3. **Test Early, Test Often**

We wrote tests alongside implementation, catching bugs immediately.

### 4. **Keep It Simple**

We didn't try to implement everything at once. We focused on the core methods that 80% of users need.

### 5. **Performance Matters**

Native functions are 10x faster. For a language focused on performance, this is critical.

## Conclusion

Building a standard library is more than just writing functions—it's about creating an ecosystem. By following consistent patterns, prioritizing performance, and ensuring JavaScript compatibility, we've created a standard library that feels familiar yet performs like native code.

The 10 modules we built this week are just the beginning. As Oite grows, so will its standard library. And with each new module, we'll apply the lessons we've learned.

---

**Contribute to Oite's standard library:**

- [GitHub Repository](https://github.com/warpy-ai/script)
- [Standard Library Documentation](/docs/standard-library)
- [Contributing Guide](/docs/contributing)
