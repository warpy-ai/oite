---
sidebar_position: 3
title: Type System
description: Oite's structural type system with TypeScript-style annotations, type inference, and compile-time type checking.
keywords: [type system, structural typing, type inference, type annotations, interfaces, generics]
---

# Type System

Oite features a **structural type system** inspired by TypeScript. Types are checked at compile time and erased at runtime, providing safety without performance overhead.

## Structural Typing

Oite uses structural typing (duck typing), where type compatibility is determined by the structure of types rather than their names. If two types have compatible shapes, they are considered compatible.

```javascript
interface Point {
    x: number;
    y: number;
}

interface Coordinate {
    x: number;
    y: number;
}

// These are compatible because they have the same structure
let p: Point = { x: 10, y: 20 };
let c: Coordinate = p;  // OK - same structure
```

This is different from **nominal typing** (used in Java/C#) where types must be explicitly declared as related.

## Type Annotations

### Basic Types

```javascript
let count: number = 42;
let name: string = "Oite";
let active: boolean = true;
let nothing: null = null;
let missing: undefined = undefined;
```

### Any and Unknown

```javascript
// 'any' opts out of type checking - use sparingly
let flexible: any = 42;
flexible = "now a string";  // OK

// 'unknown' is safer - requires type checking before use
let data: unknown = fetchData();
// data.property;  // Error: must narrow type first
```

### Arrays

```javascript
let numbers: number[] = [1, 2, 3];
let strings: Array<string> = ["a", "b", "c"];
```

### Functions

```javascript
function add(a: number, b: number): number {
    return a + b;
}

// Arrow functions
let multiply: (x: number, y: number) => number = (x, y) => x * y;

// Optional parameters
function greet(name: string, greeting?: string): string {
    return (greeting || "Hello") + ", " + name;
}
```

## Interfaces

Interfaces define the shape of objects:

```javascript
interface User {
    id: number;
    name: string;
    email?: string;  // Optional property
}

let user: User = {
    id: 1,
    name: "Alice"
    // email is optional, can be omitted
};
```

### Interface Extension

```javascript
interface Animal {
    name: string;
}

interface Dog extends Animal {
    breed: string;
    bark(): void;
}

let dog: Dog = {
    name: "Buddy",
    breed: "Golden Retriever",
    bark: () => console.log("Woof!")
};
```

## Type Aliases

Create custom type names:

```javascript
type ID = number;
type Point = { x: number, y: number };
type StringOrNumber = string | number;

let userId: ID = 123;
let position: Point = { x: 10, y: 20 };
```

## Union Types

A value can be one of several types:

```javascript
let value: string | number = "hello";
value = 42;  // OK

function format(input: string | number): string {
    if (typeof input === "string") {
        return input.toUpperCase();
    }
    return input.toString();
}
```

## Type Inference

Oite infers types when annotations are omitted:

```javascript
let x = 42;           // Inferred as number
let s = "hello";      // Inferred as string
let arr = [1, 2, 3];  // Inferred as number[]

function double(n: number) {
    return n * 2;     // Return type inferred as number
}
```

## Generic Types

Write reusable code that works with multiple types:

```javascript
function identity<T>(value: T): T {
    return value;
}

let num = identity<number>(42);
let str = identity<string>("hello");

// Type argument often inferred
let inferred = identity(42);  // T inferred as number
```

### Generic Interfaces

```javascript
interface Container<T> {
    value: T;
    getValue(): T;
}

let numberContainer: Container<number> = {
    value: 42,
    getValue: function() { return this.value; }
};
```

## Type Checking Errors

The compiler reports type mismatches at compile time:

```javascript
let x: number = "hello";  
// Error: Type 'string' is not assignable to type 'number'

function greet(name: string): string {
    return 42;  
    // Error: Type 'number' is not assignable to return type 'string'
}

let user: { name: string } = { age: 25 };
// Error: Property 'name' is missing
```

## Self-Hosted Type Checker

The Oite compiler is self-hosted, meaning the type checker itself is written in Oite (`compiler/passes/types.ot`). This provides several benefits:

- **Dogfooding**: The type system is validated by being used to build the compiler itself
- **Single language**: No context switching between Rust and Oite
- **Fast iteration**: Changes to type checking don't require Rust recompilation

The Rust VM is used during development for running the self-hosted compiler, while production builds target native code via the LLVM backend.

## Comparison with TypeScript

| Feature | Oite | TypeScript |
|---------|--------|------------|
| Structural typing | Yes | Yes |
| Type inference | Yes | Yes |
| Generics | Yes | Yes |
| Union types | Yes | Yes |
| Intersection types | Planned | Yes |
| Conditional types | No | Yes |
| Runtime type info | No (erased) | No (erased) |
| Compilation target | Native (LLVM) | JavaScript |

Oite's type system is intentionally simpler than TypeScript's, focusing on the most commonly used features while enabling native code compilation.
