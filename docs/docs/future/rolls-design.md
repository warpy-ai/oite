---
title: Rolls - Oite System Libraries Design
description: Design document for Rolls, the official system library ecosystem for Oite. Analogous to Rust's standard library crates.
keywords: [rolls, system libraries, stdlib, packages, future]
---

# Rolls - Official System Libraries for Oite

> **Status**: Future Implementation - Archived Design Document
>
> This document describes the planned architecture for Rolls, the official
> system library ecosystem for the Oite language. Code has been removed
> from Oite core to maintain a clean language/library separation.

## Overview

**Rolls** are the official system libraries for Oite, analogous to Rust's
standard library crates. Each Roll provides specific functionality that builds
on Oite core's primitives.

```
User App Code
     │
     ▼
┌────────────────────────────────┐
│  Rolls (official system libs)  │  ← THIS DOCUMENT
│  @rolls/http, @rolls/tls, etc  │
└────────────────────────────────┘
     │
     ▼
Oite Core (compiler, ABI, basic async)
```

## Roll Catalog

### Core System Rolls

| Roll | Purpose | Dependencies | Estimated LOC |
|------|---------|--------------|---------------|
| `@rolls/async` | Work-stealing executor, io_uring | oite-core | ~800 |
| `@rolls/tls` | TLS encryption via rustls | oite-core | ~600 |
| `@rolls/http` | HTTP/1.1, HTTP/2 server | @rolls/tls, @rolls/async | ~1800 |
| `@rolls/websocket` | WebSocket protocol | @rolls/http | ~800 |

### Standard Library Rolls

| Roll | Purpose | Dependencies | Estimated LOC |
|------|---------|--------------|---------------|
| `@rolls/fs` | File system operations | oite-core | ~400 |
| `@rolls/path` | Path utilities | oite-core | ~200 |
| `@rolls/json` | JSON parse/stringify | oite-core | ~300 |
| `@rolls/math` | Math functions | oite-core | ~200 |
| `@rolls/date` | Date/time handling | oite-core | ~300 |
| `@rolls/string` | String methods | oite-core | ~400 |
| `@rolls/array` | Array methods | oite-core | ~400 |
| `@rolls/promise` | Promise implementation | oite-core | ~300 |

## File Mapping Reference

Code removed from Oite core that maps to future Rolls:

### @rolls/async

```
Removed Files:
- src/runtime/async/work_stealing.rs (301 lines)
- src/runtime/async/worker.rs (138 lines)
- src/runtime/async/io_uring.rs (360 lines)

Key Types/Functions:
- WorkStealingExecutor
- Worker
- IoUringReactor
- crossbeam-deque work queues
```

### @rolls/tls

```
Removed Files:
- src/runtime/async/tls.rs (607 lines)

Key Types/Functions:
- TlsStream<T>
- TlsClientConfig
- TlsServerConfig
- TlsAcceptor
- TlsConnector

Dependencies:
- rustls 0.23 (with aws-lc-rs backend)
- webpki-roots 0.26
- rustls-pemfile 2.1
```

### @rolls/http

```
Removed Files:
- src/runtime/http/mod.rs (706 lines)
- src/runtime/http/server.rs (964 lines)
- src/runtime/http/h2.rs (205 lines)
- src/runtime/http/h2_adapter.rs (85 lines)
- src/runtime/http/protocol.rs (99 lines)

Key Types/Functions:
- HttpRequest / HttpResponse
- HttpServer
- HTTP/1.1 parsing (httparse)
- HTTP/2 support (h2 crate)
- ALPN protocol detection

Dependencies:
- httparse 1.8
- h2 0.4
- http 1.0 (HTTP types)
- bytes 1.5
```

### @rolls/websocket

```
Removed Files:
- src/runtime/http/websocket.rs (479 lines)
- src/runtime/http/ws_connection.rs (330 lines)

Key Types/Functions:
- WebSocketFrame
- WebSocketConnection
- Frame encoding/decoding
- SHA-1 handshake

Dependencies:
- sha1 0.10
- base64 0.22
```

### @rolls/fs

```
Removed Files:
- src/stdlib/fs.rs (~400 lines)

Key Functions:
- readFileSync / readFile
- writeFileSync / writeFile
- existsSync / exists
- mkdirSync / mkdir
- readdirSync / readdir
- statSync
- unlink, rmdir, rename
- copyFileSync, appendFileSync
```

### @rolls/path

```
Removed Files:
- src/stdlib/path.rs (~200 lines)

Key Functions:
- path.join()
- path.resolve()
- path.dirname()
- path.basename()
- path.extname()
- path.parse() / path.format()
- path.isAbsolute()
- path.relative()
```

### @rolls/json

```
Removed Files:
- src/stdlib/json.rs (~300 lines)

Key Functions:
- JSON.parse()
- JSON.stringify()

Dependencies:
- serde_json 1.0
```

### @rolls/math

```
Removed Files:
- src/stdlib/math.rs (~200 lines)

Key Functions:
- All Math.* methods (abs, floor, ceil, round, etc.)
- Trigonometric functions (sin, cos, tan, etc.)
- Math constants (PI, E, etc.)

Dependencies:
- fastrand 2.0 (for Math.random)
```

### @rolls/date

```
Removed Files:
- src/stdlib/date.rs (~300 lines)

Key Functions:
- Date constructor
- Date.now(), Date.parse(), Date.UTC()
- getTime, getFullYear, getMonth, etc.
- toISOString, toString, toJSON

Dependencies:
- chrono 0.4
```

### @rolls/string

```
Removed Files:
- src/stdlib/string.rs (~400 lines)

Key Functions:
- String.fromCharCode()
- String prototype methods (when implemented)
```

### @rolls/array

```
Removed Files:
- src/stdlib/array.rs (~400 lines)

Key Functions:
- Array prototype methods (map, filter, reduce, etc.)
```

### @rolls/promise

```
Removed Files:
- Promise implementation in src/stdlib/mod.rs

Key Functions:
- Promise constructor
- Promise.resolve() / Promise.reject()
- Promise.all()
- .then() / .catch()
```

## Implementation Architecture

### Roll Package Structure

```
@rolls/http/
├── roll.toml           # Roll manifest
├── src/
│   ├── lib.ot        # Public API
│   ├── request.ot    # HttpRequest type
│   ├── response.ot   # HttpResponse type
│   ├── server.ot     # HttpServer implementation
│   └── internal/       # Private implementation
│       ├── parser.ot
│       └── h2.ot
└── tests/
    └── server_test.ot
```

### Roll Manifest (roll.toml)

```toml
[roll]
name = "http"
version = "0.1.0"
license = "Apache-2.0"
repository = "https://github.com/warpy-ai/script"

[dependencies]
tls = { version = "0.1", optional = true }
async = "0.1"

[features]
default = []
http2 = ["dep:h2-native"]
tls = ["dep:tls"]

[native]
# Native Rust dependencies compiled as FFI
httparse = "1.8"
h2 = { version = "0.4", optional = true }
```

## Usage Example

```javascript
// Import from Rolls
import { HttpServer, HttpRequest, HttpResponse } from "@rolls/http";
import { TlsConfig } from "@rolls/tls";

// Create HTTPS server
let server = new HttpServer({
    port: 443,
    tls: TlsConfig.fromFiles("cert.pem", "key.pem")
});

server.on("request", (req: HttpRequest): HttpResponse => {
    return new HttpResponse(200, { "Content-Type": "text/plain" }, "Hello!");
});

await server.listen();
```

## Performance Targets

| Metric | Target |
|--------|--------|
| HTTP hello-world | 250k req/s |
| TLS handshake | &lt;1ms |
| WebSocket frame | 500k msg/s |
| Work-stealing overhead | &lt;5% vs single-thread |

## Future Considerations

1. **Native FFI**: Rolls can include native Rust code compiled as FFI
2. **Tree-shaking**: Unroll should tree-shake unused Roll code
3. **Version Resolution**: Semantic versioning with lockfile support
4. **Security Audits**: Critical Rolls (@rolls/tls, @rolls/http) need audits
