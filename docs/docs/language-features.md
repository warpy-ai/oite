---
sidebar_position: 3
---

# Language Features

Script combines JavaScript-like syntax with Rust-inspired memory safety and native code performance.

## Variables & Types

```javascript
let x = 42;              // Number
let name = "Script";     // String
let active = true;       // Boolean
let data = { key: 1 };   // Object
let items = [1, 2, 3];   // Array
```

## Functions & Closures

```javascript
// Function declaration
function greet(name) {
    return "Hello, " + name + "!";
}

// Arrow functions
let double = x => x * 2;
let add = (a, b) => a + b;

// Closures
function counter() {
    let count = 0;
    return () => {
        count = count + 1;
        return count;
    };
}
```

## Control Flow

```javascript
if (condition) {
    // ...
} else {
    // ...
}

for (let i = 0; i < 10; i++) {
    // ...
    if (done) break;
    if (skip) continue;
}

while (condition) {
    // ...
}

do {
    // ...
} while (condition);
```

## Objects & Arrays

```javascript
let obj = { x: 10, y: 20 };
obj.z = 30;
console.log(obj["x"]);

let arr = [1, 2, 3];
arr.push(4);
let first = arr[0];
```

## Classes & Inheritance

```javascript
class Animal {
    name: string;
    
    constructor(name: string) {
        this.name = name;
    }
    
    speak() {
        console.log(this.name + " makes a sound");
    }
}

class Dog extends Animal {
    breed: string;
    
    constructor(name: string, breed: string) {
        super(name);
        this.breed = breed;
    }
    
    speak() {
        console.log(this.name + " barks!");
    }
}

let dog = new Dog("Buddy", "Golden Retriever");
dog.speak();  // "Buddy barks!"
```

## Private Fields

Script supports JavaScript-style private fields using the `#` prefix:

```javascript
class Counter {
    #count = 0;           // Private field (only accessible within class)
    
    increment() {
        this.#count++;
    }
    
    getCount() {
        return this.#count;  // Can access private field from methods
    }
}

let c = new Counter();
c.increment();
console.log(c.getCount());  // 1

// c.#count;       // ERROR: Private field not accessible outside class
// c["#count"];    // Returns undefined (encapsulation works)
```

## Error Handling

```javascript
try {
    riskyOperation();
} catch (e) {
    console.log("Error: " + e);
} finally {
    cleanup();
}
```

## Template Literals

```javascript
const name = "World";
const greeting = `Hello, ${name}!`;  // "Hello, World!"
```

## Decorators

Script supports TypeScript-style decorators:

```javascript
function logged(target: any) {
    console.log(`Decorating class: ${target.name}`);
    return target;
}

@logged
class MyClass {
    // ...
}
```

## Type Annotations

Script supports TypeScript-style type annotations with inference:

```javascript
let x: number = 42;
function add(a: number, b: number): number {
    return a + b;
}
let arr: string[] = ["a", "b"];
```
