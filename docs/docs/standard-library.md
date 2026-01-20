---
sidebar_position: 7
---

# Standard Library

Script provides a growing standard library with essential functionality.

## Console

```javascript
console.log("Hello", 42, true);
```

Outputs values to stdout with automatic formatting.

## Timers

```javascript
setTimeout(() => {
    console.log("Delayed!");
}, 1000);
```

Schedules a callback to run after a delay (in milliseconds).

## File System

```javascript
let fs = require("fs");
let content = fs.readFileSync("file.txt");
fs.writeFileSync("out.txt", "Hello!");
fs.writeBinaryFile("data.bin", bytes);
```

File system operations for reading and writing files.

## ByteStream (Binary Data)

```javascript
let stream = ByteStream.create();
ByteStream.writeU8(stream, 0xFF);
ByteStream.writeU32(stream, 12345);
ByteStream.writeF64(stream, 3.14159);
ByteStream.writeString(stream, "hello");
let bytes = ByteStream.toArray(stream);
```

Low-level binary data manipulation for working with bytes.

## Planned Modules

The standard library is actively being expanded. Planned modules include:

- **fs** - Rich file system operations
- **net** - Network programming
- **http** - HTTP client and server
- **crypto** - Cryptographic operations
- **process** - Process management
- **os** - Operating system interface

## Module Loading

Currently, Script supports `require`-style module loading:

```javascript
let fs = require("fs");
let myModule = require("./my-module");
```

ES modules (`import`/`export`) are planned for future releases.
