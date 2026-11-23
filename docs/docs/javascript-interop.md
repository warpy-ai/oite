---
sidebar_position: 5
title: JavaScript & TypeScript Interoperability
description: How Oite compiles .js, .ts and .ot files, which rules are the same for all of them, and what is not implemented yet.
keywords:
  [
    javascript,
    typescript,
    interop,
    compatibility,
    node,
    npm,
    ownership,
    memory model,
  ]
---

# JavaScript & TypeScript interoperability

Oite compiles `.ot`, `.ts` and `.js` files with the same rules. The file
extension selects only the parser.

## The extension selects the parser

| Extension | Parsed as |
| --------- | --------- |
| `.js`, `.jsx` | JavaScript |
| `.ts` | TypeScript, with decorators |
| `.tsx` | TypeScript, with decorators and JSX |
| `.ot` and every other extension | TypeScript, with decorators |

After parsing, every file goes through the same move check, type checker
and code generation:

- **Type annotations** are a syntax error in a `.js` file. In the other
  files the checker checks them ([Types](types.md)).
- **Memory.** Every file follows the same rules: objects are shared by
  reference, and at the top level of a file `let b = a` moves the value
  out of `a`. There is no copy-by-default mode for `.js` files.
  [Memory model](/rolls/memory-model) has the rules.

```javascript
// main.js
let a = { value: 1 };
let b = a;
b.value = 2;
console.log(a.value);
```

```
$ oitec main.js
main.js:4:13: borrow error: Cannot borrow moved variable 'a'
```

The same program is rejected with the same error as `main.ot`.

## Imports

An import names a relative path or a roll. For a relative path such as
`./util`, Oite tries these files in order:

1. `util`
2. `util.ot`
3. `util.ts`
4. `util.js`
5. `util/mod.ot`
6. `util/index.ot`

A module can import a module written with any of these extensions, with
no change in behavior at the boundary.

An import of `@rolls/<name>` loads the roll's `src/mod.ot`. Pass `--roll
<name>=DIR` or set `OITE_ROLLS_PATH` to say where rolls are.

## Not implemented

- **npm packages.** `import _ from "lodash"` is an error: a module is a
  relative path or a roll. The planned package manager is described in
  [Unroll](/unroll/intro).
- **JSX.** A `.tsx` file parses JSX, and the compiler then reports
  `unsupported: JSX`.
- **Borrow syntax and lifetimes.** `&a`, `&mut a` and `<'a>` are syntax
  errors. [Lifetimes](/lifetimes) is a design, not a feature.

## See also

- [Memory model](/rolls/memory-model): the move check rules
- [ECMAScript conformance](/ecmascript-conformance): where Oite differs
  from JavaScript
- [Types](types.md): what the type checker checks
