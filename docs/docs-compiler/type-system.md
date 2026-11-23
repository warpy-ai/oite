---
sidebar_position: 3
title: Type System
description: Oite's structural type system with TypeScript-style annotations, type inference, and compile-time type checking.
keywords: [type system, structural typing, type inference, type annotations, interfaces, generics]
---

# Type System

Oite has a **structural type system** modelled on TypeScript. The checker in `src/types/check/` runs before code generation. A type error is a warning on `oitec build`, `oitec jit` and a plain run, and an error on `oitec check`. With `--strict-types`, type errors stop the build, and the compiled program checks values where dynamic code meets typed code. [Types](/types) describes the design and what is not checked yet.

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

// 'unknown' must be narrowed before it is stored in a typed place
let data: unknown = fetchData();
// let n: number = data;  // Error: Type 'unknown' is not assignable to type 'number'.
// data.property is not reported yet.
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

## Type inference

The checker infers the type of an expression from the types of its parts, in one direction. It does not infer the type of a declaration that has no annotation:

```javascript
let y: number = 1;
let z: string = y * 2;  // Error: 'y * 2' is a number

let x = 42;             // x is 'any': no annotation
x = "a";                // OK

function double(n: number) {
    return n * 2;
}
let d: string = double(2);  // OK: no return annotation, so the result is 'any'
```

## Generic types

Generic functions, classes and interfaces parse and run, but the checker treats every type parameter as `any`. It ignores type arguments, written or inferred ([#190](https://github.com/warpy-ai/oite/issues/190)):

```javascript
function identity<T>(value: T): T {
    return value;
}

let n: number = identity("hello");          // not reported
let m: number = identity<string>("hello");  // not reported

interface Container<T> {
    value: T;
}
let c: Container<number> = { value: "no" }; // not reported
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

## The self-hosted type checker

`oitec` uses the checker written in Rust, in `src/types/check/`. The self-hosted compiler (`compiler/`) has a separate checker written in Oite, `compiler/passes/types.ot`. `oitec` does not run it, and the two can disagree.

## Comparison with TypeScript

| Feature | Oite | TypeScript |
|---------|--------|------------|
| Structural typing | Yes | Yes |
| Type inference | Expressions only; an unannotated declaration is `any` | Yes |
| Generics | Parsed, not checked ([#190](https://github.com/warpy-ai/oite/issues/190)) | Yes |
| Union types | Yes | Yes |
| Intersection types | Planned | Yes |
| Conditional types | No | Yes |
| Runtime type checks | With `--strict-types` | No (erased) |
| Compilation target | Native (LLVM) | JavaScript |

Oite's type system is intentionally simpler than TypeScript's, focusing on the most commonly used features while enabling native code compilation.
