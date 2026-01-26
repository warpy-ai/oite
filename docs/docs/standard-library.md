---
sidebar_position: 7
title: Standard Library Reference
description: Complete reference for Script's minimal standard library including console, ByteStream, and basic file I/O. Extended functionality is provided by the Rolls ecosystem.
keywords:
  [standard library, stdlib, console, ByteStream, file io, api reference]
---

# Standard Library

Script Core provides a **minimal** standard library — only essential primitives needed to run code. Extended functionality (HTTP, TLS, crypto, etc.) is provided by the **Rolls** ecosystem.

## Philosophy

Script Core is like "C without libc" — minimal and self-contained:

| In Core       | Why                               |
| ------------- | --------------------------------- |
| `console.log` | Essential for output              |
| `ByteStream`  | Needed for bootstrap compiler     |
| Basic `fs`    | Needed for file-based compilation |

| In Rolls (Optional)  | Why                   |
| -------------------- | --------------------- |
| HTTP, TLS, WebSocket | External dependencies |
| Database drivers     | Database-specific     |
| Math functions       | Standard library      |
| Crypto operations    | External libraries    |

## Console

```javascript
console.log("Hello", 42, true);
console.error("Something went wrong!");
```

Outputs values to stdout/stderr with automatic formatting.

## String

```javascript
let char = String.fromCharCode(65); // "A"
```

## ByteStream (Binary Data)

Low-level binary data manipulation for working with bytes. Used internally by the bootstrap compiler.

```javascript
let stream = ByteStream.create();
ByteStream.writeU8(stream, 0xff);
ByteStream.writeU32(stream, 12345);
ByteStream.writeF64(stream, 3.14159);
ByteStream.writeString(stream, "hello");
let bytes = ByteStream.toArray(stream);
```

## File System (Minimal)

Basic file operations for compilation:

```javascript
let fs = require("fs");
let content = fs.readFileSync("file.txt");
fs.writeFileSync("out.txt", "Hello!");
fs.writeBinaryFile("data.bin", bytes);
```

## Module Loading

Script supports both ES modules and require-style loading:

```javascript
// ES Modules (recommended)
import { something } from "./my-module";
export function myFunc() {
  /* ... */
}

// CommonJS-style (also supported)
let fs = require("fs");
let myModule = require("./my-module");
```

## Future: Rolls Ecosystem

Extended functionality will be provided by the Rolls ecosystem:

| Roll            | Purpose                     |
| --------------- | --------------------------- |
| `@rolls/http`   | HTTP/1.1, HTTP/2 server     |
| `@rolls/tls`    | TLS encryption              |
| `@rolls/fs`     | Rich file system operations |
| `@rolls/json`   | JSON parse/stringify        |
| `@rolls/math`   | Math functions              |
| `@rolls/crypto` | Cryptographic operations    |
| `@rolls/db`     | Database drivers            |

See the [Architecture](/docs/architecture) for more details on the Rolls ecosystem.
