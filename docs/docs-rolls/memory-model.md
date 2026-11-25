---
sidebar_position: 3
title: Memory Model
description: What Oite does today when you assign or pass a value, which code the move check covers, and what is not implemented yet.
keywords:
  [memory model, ownership, move, borrow checker, use after move, no gc]
---

# Memory model

This page describes what `oitec` does today. It is not a specification:
the normative description of the language, including the ownership model,
is open in [#135](https://github.com/warpy-ai/oite/issues/135).

At run time, assigning an object shares it, as in JavaScript, and
nothing is copied. A compile-time check, the move check, rejects some
programs that use a variable after its value was assigned to another
variable. Nothing is freed.

## Values at run time

- Every value is a 64-bit NaN-boxed word. A number, a boolean, `null` and
  `undefined` are stored in the word. An object, an array, a string and a
  function are pointers to the heap.
- Assigning a value, or passing it to a function, copies the word. For a
  heap value, both names then point at the same object. No object is
  cloned.
- Memory is never freed
  ([#123](https://github.com/warpy-ai/oite/issues/123)).

So in a function, where the move check does not run, this prints
`2 true`:

```javascript
function f() {
  let p = { value: 1 };
  let q = p;
  q.value = 2;
  console.log(p.value, p === q); // 2 true
}
f();
```

## The move check

The move check is in `src/compiler/borrow_ck.rs`. It runs on every file
before type checking and code generation, for `oitec check`, `oitec
build`, `oitec jit` and a plain run. The first error it finds stops the
build, with the message `borrow error: ...`. For a moved variable, the
error also shows where the value was moved.

### Code that it checks

The move check reads the statements at the top level of a file. It also
reads the blocks, `if`, `while` and `for` statements among them, and the
same statements nested inside those.

It does not read these:

- the body of a function, an arrow function or a method
- `try`, `switch`, `do..while`, `for..of`, `for..in` and labeled
  statements
- `export` declarations

### What moves a value

| Code at the top level | Effect on `a` |
| --------------------- | ------------- |
| `let b = a`, `const b = a`, `var b = a` | Moves the value out of `a`, unless `a` holds a copy value |
| `b = a` | Moves the value out of `a`, unless `a` holds a copy value |
| `f(a)`, `a.x`, `a[i]` | Borrows `a` until the end of the statement. `a` stays usable. |
| An `async` arrow function or `async` function expression that names `a` | Moves the value into the closure |
| A function that is not `async`, and any other use, such as `a + 1`, `[a]` or `{ k: a }` | No effect |

Only a bare name on the right moves a value. `let b = (a)` does not.

A variable holds a **copy value** when its initializer is a number
literal, a boolean literal, or another variable that holds a copy value.
The value of every other variable moves. That includes a variable whose
initializer is a string, `null`, a call or an arithmetic expression such
as `1 + 2`, and a variable with a `number` annotation. The move check does not read type annotations.

### Errors

After `a` was moved:

| Code | Error |
| ---- | ----- |
| `let c = a`, `a + 1` | `Use of moved variable 'a'` |
| `f(a)`, `a.x` | `Cannot borrow moved variable 'a'` |
| `let c = a`, `a + 1`, after an `async` closure captured `a` | `'a' was moved into an async closure! Cannot use after capture.` |
| `f(a)`, `a.x`, after an `async` closure captured `a` | `Cannot borrow 'a' - it was captured by an async closure!` |

Assigning a new value to `a` does not make it usable again.

```javascript
let a = { value: 1 };
let b = a;            // the value moves from 'a' to 'b'
b.value = 2;
console.log(b.value); // 2
// console.log(a.value);
// borrow error: Cannot borrow moved variable 'a'

let x = 10;
let y = x;            // 'x' holds a copy value
console.log(x, y);    // 10 10
```

## File extensions

`.ot`, `.ts` and `.js` files follow the same rules. The extension selects
only the parser: `.js` and `.jsx` files are parsed as JavaScript, and
every other file as TypeScript with decorators. There is no
copy-by-default mode for `.js` files.

## Not implemented

- **Borrow syntax.** `&a` and `&mut a` are syntax errors. `Ref<T>` and
  `MutRef<T>` parse as type names and have no effect.
- **Lifetimes.** `<'a>` is a syntax error.
  [Lifetimes](/lifetimes) is a design, not a feature.
- **Freeing memory**
  ([#123](https://github.com/warpy-ai/oite/issues/123)).
- **The move check inside functions**, and in the statements listed under
  "Code that it checks".
