<div align="center">
  <h1>Script</h1>
  <p>A custom JavaScript-like scripting language with a stack-based VM implemented in <strong>Rust</strong></p>
  <p>Featuring a self-hosting bootstrap compiler written in the language itself</p>
</div>

This is the main source code repository for **Script**. It contains the compiler,
virtual machine, standard library, and bootstrap compiler modules.

For detailed development progress and implementation details, see [PROGRESS.md](PROGRESS.md).

## Why Script?

- **Self-Hosting:** The bootstrap compiler (lexer, parser, emitter) is written entirely in Script itself, demonstrating the language's capabilities.

- **Performance:** Stack-based VM architecture optimized for efficient execution with heap allocation for complex data structures.

- **Memory Safety:** Ownership model with scoped lifetimes ensures variables are automatically freed when their scope ends, preventing memory leaks.

- **JavaScript-like Syntax:** Familiar syntax for developers coming from JavaScript/TypeScript, with support for functions, objects, arrays, closures, and more.

## Architecture

```
┌─────────────┐    ┌─────────────────┐    ┌────────────────┐
│   Parsing   │───▶│  Borrow Checker │───▶│  Stack-based   │
│  (SWC AST)  │    │   (Middle-end)  │    │   VM (Back-end)│
└─────────────┘    └─────────────────┘    └────────────────┘

        │                                         │
        │         Bootstrap Compiler              │
        │    ┌─────────────────────────┐         │
        └───▶│  Lexer → Parser → Emitter │◀───────┘
             │     (Written in Script)     │
             └─────────────────────────┘
```

| Stage                  | Description                                                                 |
| ---------------------- | --------------------------------------------------------------------------- |
| **Rust Compiler**      | SWC-based parser with borrow checking and bytecode generation              |
| **Bootstrap Compiler** | Self-hosting compiler written in Script (lexer, parser, emitter)             |
| **VM**                 | Stack-based VM with heap allocation, closures, and event loop              |

## Quick Start

```bash
# Build the project
cargo build --release

# Run a script
cargo run -- path/to/script.tscl

# Run bootstrap compiler tests
cargo run -- bootstrap/test_emitter.tscl
```

## Features

### Self-Hosting Bootstrap Compiler

The project includes a bootstrap compiler written entirely in Script:

- **Lexer** (`bootstrap/lexer.tscl`) - Tokenizes source code into tokens
- **Parser** (`bootstrap/parser.tscl`) - Recursive descent parser producing AST
- **Emitter** (`bootstrap/emitter.tscl`) - Generates bytecode from AST

### Memory Management

- **Ownership Model** - Variables own their data; assigning objects moves ownership
- **Let vs Store** - `Let` creates new bindings (shadowing), `Store` updates existing ones
- **Scoped Lifetimes** - Variables freed when their containing scope ends
- **Stack vs Heap** - Primitives on stack (copy), Objects/Arrays on heap (move)

### Virtual Machine

- **Stack-based Architecture** - LIFO stack for expressions and operations
- **Call Stack & Frames** - Nested function calls with isolated local scopes
- **Closures** - Functions capture variables from enclosing scopes
- **Event Loop** - Task queue with `setTimeout` support
- **Bytecode Rebasing** - Appended bytecode has addresses automatically adjusted

### Language Support

- **Functions** - Declarations, expressions, arrow functions, closures
- **Objects & Arrays** - Literals, property access, computed access, methods
- **Control Flow** - `if`/`else`, `while`, `break`, `continue`
- **Operators** - Arithmetic, comparison, logical, unary
- **Constructors** - `new` expressions with `this` binding
- **String Methods** - `slice`, `charCodeAt`, `charAt`, `includes`, `trim`
- **Array Methods** - `push`, `pop`, `shift`, `unshift`, `splice`, `indexOf`, `includes`, `join`

## Bytecode Instruction Set

| OpCode             | Description                                           |
| ------------------ | ----------------------------------------------------- |
| `Push(Value)`      | Push constant onto stack                              |
| `Let(Name)`        | Create new variable binding in current scope          |
| `Store(Name)`      | Update existing variable (searches all scopes)        |
| `Load(Name)`       | Push variable's value onto stack                      |
| `NewObject`        | Allocate empty object on heap                         |
| `NewArray(Size)`   | Allocate array of given size                          |
| `SetProp(Key)`     | Set property on heap object                           |
| `GetProp(Key)`     | Get property from heap object                         |
| `Call(ArgCount)`   | Execute function with N arguments                     |
| `CallMethod(N,A)`  | Call method on object                                 |
| `Return`           | Return from function                                  |
| `Jump(Addr)`       | Unconditional jump                                    |
| `JumpIfFalse(Addr)`| Conditional branch                                    |
| `MakeClosure(Addr)`| Create closure with captured environment              |
| `Construct(Args)`  | Construct new object instance                         |
| `Drop(Name)`       | Free variable and its heap data                       |
| `Halt`             | Stop execution                                        |

## Standard Library

### Built-in Objects

- `console.log(...)` - Print values to stdout
- `setTimeout(fn, ms)` - Schedule function execution
- `require(module)` - Load module (currently supports "fs")

### File System (`fs`)

```javascript
let fs = require("fs");
fs.readFileSync(path)           // Read file as string
fs.writeFileSync(path, content) // Write string to file
fs.writeBinaryFile(path, bytes) // Write binary data
```

### ByteStream (Binary Data)

```javascript
let stream = ByteStream.create();
ByteStream.writeU8(stream, byte);
ByteStream.writeU32(stream, value);
ByteStream.writeF64(stream, value);
ByteStream.writeString(stream, str);
ByteStream.writeVarint(stream, value);
ByteStream.patchU32(stream, offset, value);
ByteStream.length(stream);
ByteStream.toArray(stream);
```

## Example

**Source Code:**

```javascript
function greet(name) {
    return "Hello, " + name + "!";
}

let message = greet("World");
console.log(message);
```

**Bootstrap Compiler Usage:**

```javascript
// Compile source to bytecode
let bytecode = compile("1 + 2 * 3");

// Write bytecode to file
compileToFile("let x = 42;", "output.bc");
```

## Project Structure

```
script/
├── src/
│   ├── main.rs          # Entry point with two-stage loading
│   ├── compiler/        # Rust compiler (SWC-based)
│   ├── vm/
│   │   ├── mod.rs       # VM implementation
│   │   ├── opcodes.rs   # Bytecode opcodes
│   │   └── value.rs     # Runtime value types
│   └── stdlib/          # Native function implementations
├── std/
│   └── prelude.tscl     # Standard prelude (OpCodes, Types, utilities)
└── bootstrap/
    ├── lexer.tscl       # Self-hosting lexer
    ├── parser.tscl      # Self-hosting parser
    ├── emitter.tscl     # Self-hosting bytecode emitter
    └── test_emitter.tscl # Emitter test suite
```

## Development Progress

For detailed information about completed features, recent fixes, and upcoming work, see [PROGRESS.md](PROGRESS.md).

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

Script is distributed under the terms of the Apache License (Version 2.0).

See [LICENSE](LICENSE) for details.
