---
sidebar_position: 9
title: Object Model
description: Phase 2 of the performance plan - how native objects, property access, method calls and closure environments change to close the gap with Bun.
keywords: [performance, objects, shapes, hidden classes, inline caches, closures]
---

# Object model (performance phase 2)

> **Status:** design for review. Nothing here is implemented yet. It is
> phase 2 of [Performance](performance.md).

## Why

Native Oite is ahead of Bun on numeric code and start-up, and far behind
wherever a program touches objects (native `--release`, best of 3):

| Benchmark | Oite | Bun | Gap |
| --------- | ---: | --: | --: |
| `props` (object properties) | 421 ms | 28 ms | 15× |
| `methods` (method calls on a class) | 353 ms | 17 ms | 21× |
| `nbody` (floating point on objects) | 3,366 ms | 116 ms | 29× |
| `closures` (callbacks, captured variables) | 152 ms | 53 ms | 2.9× |
| `alloc` (small short-lived objects) | 1,187 ms | 22 ms | 54× |

**Goal of phase 2:** `props`, `methods`, `nbody` and `closures` within
1.5 times Bun's time. `alloc` and `binary_trees` also need memory
reclamation (phase 4); phase 2 should cut their time several-fold, not
close the gap.

## How objects work today

Every object operation in native code is a call into the runtime; no
backend reads an object's fields itself. In the runtime:

- **An object is a header and a pointer to a separate `Vec<(String,
  value)>`.** Each property name is its own heap `String`. A two-property
  literal costs about four allocations.
- **Every access is a linear search comparing strings.** A miss also
  searches for `"__proto__"` to follow the prototype chain.
- **Internal state lives in the same list, under special names:** the
  prototype (`__proto__`), accessors (`getter:x`, `setter:x`), classes
  (`__class__`), promises (`__promise__`), errors (`__error__`), closure
  environments (`%parent`, `%this`, `%super`), private names (`#x@N`).
  Hiding them is done by name prefix, which also hides any user key that
  starts with `__`.
- **One accessor anywhere slows every access.** The first getter or setter
  in a program sets a global flag; from then on every property read builds
  a `"getter:" + name` string and looks it up first.
- **Closure environments are ordinary objects.** Reading a captured
  variable is one property lookup per scope level plus one for the name;
  a loop that captures its variable allocates an environment per
  iteration.
- **Property names are passed as a pointer and a length.** The Cranelift
  backend leaks a new copy of the name for every operation it compiles.
- **Functions have no properties, arrays have no named properties.**
  `f.prototype`, `f.name` and `arr.foo = 1` do not exist.

Phase 2 replaces this with the techniques fast JavaScript engines use:
interned names, shapes and inline caches, plus a representation of
closure environments that the compiler lays out.

## The design

### 1. Atoms: property names as integers

Every property name is interned once into an **atom**, a `u32`, in a
runtime table. Comparing two names is comparing two integers.

- Names the compiler knows (`o.x`, `C.prototype`, method names) are
  resolved to atoms the first time each site runs (see inline caches) and
  never again.
- Computed keys (`o[k]`) are interned when used. Integer-like keys on
  plain objects are atoms too; arrays keep their elements separately.

### 2. Shapes (hidden classes)

A **shape** describes the layout of objects that got the same properties
in the same order: which atom is in which slot, with which flags.

```text
Object                          Shape
┌──────────────┐               ┌───────────────────────────────┐
│ header       │──────────────▶│ parent shape, last atom, slot │
│ shape ●──────┼───────────────│ flags (data/accessor, hidden, │
│ proto        │               │   enumerable)                 │
│ overflow ●   │──▶ [slots…]   │ transitions: atom → shape     │
│ slot 0..N-1  │               │ property count                │
└──────────────┘               └───────────────────────────────┘
```

- **Objects** become: header, shape pointer, prototype, a pointer to
  overflow slots, and `N` inline slots. `N` comes from the allocation site:
  an object literal knows its keys, and a class knows its fields (section
  5). Properties beyond `N` go to the overflow array.
- **Adding a property** moves the object to the next shape through a
  transition. Objects built the same way share every shape on the way, so
  a million `{ x, y }` objects have three shapes between them.
- **Shapes are immutable and never freed.** There are as many as there are
  distinct ways a program builds objects.
- **The prototype is a field of the object**, not a `__proto__` property.
  `o.__proto__ = p` in user code sets the field.
- **Accessors are a flag on the property.** The slot holds a pair (getter,
  setter). The global "accessors exist" flag and the `getter:x` keys go
  away; only properties that are accessors pay for being accessors.
- **Internal state leaves the property list.** Class objects, promises
  and errors get a flag in the object header, or a field of their own,
  instead of a magic key. A user key such as `__id` is an ordinary,
  visible property again.
- **Private names stay atoms** (`#x@N`, unique per class) with a
  *private* flag: not enumerable, not reachable by `o["#x"]`.
- **Key order is insertion order**, as today: the order of the shape
  chain.

**Dictionary mode.** An object used as a hash table should not create a
shape per key. An object switches to a dictionary (an insertion-ordered
hash map from atom to value and flags) when it gets more than 64
properties, or when a property other than the last one is deleted.
Dictionary objects always take the slow path; that is the right trade for
them.

### 3. Inline caches

Every property read, write and method call with a name the compiler knows
gets a **cache cell** in the binary's data (like global slots, #157):

```text
cell: name (pointer, length) │ atom │ shape │ slot │ kind │ holder │ epoch
```

The generated code for `o.x`:

1. If `o` is an object, load its shape and compare it with the cell's.
2. On a hit, load slot `slot`, inline or overflow. That is two loads and
   a compare, no call.
3. On a miss, call `ot_get_prop_ic(o, cell)`, which does the full lookup,
   fills the cell and returns the value.

- **Writes** cache both kinds of store: writing an existing slot (same
  shape before and after) and adding a property (shape A to shape B, with
  the slot to fill). The second is what makes constructors fast.
- **Method calls** cache where the method was found. A method is usually on
  the prototype, so the cell also records the *holder* (the prototype
  object) and a **prototype epoch**. Any change to an object that is used
  as a prototype increments one global epoch; a cached prototype hit is
  valid only while the epoch is unchanged. Prototypes are set up once and
  rarely change after, so this check almost always passes, and it is
  always safe.
- **Monomorphic first.** Each cell remembers one shape. If benchmarks show
  call sites that see several shapes, cells grow to four entries.
- **Names are resolved once.** The cell holds the name, so the first miss
  interns it into an atom. This replaces the pointer and length passed
  today and the copies the JIT leaks.

### 4. Arrays

- Element reads and writes with a number index are emitted inline: check
  the object is an array, check the bounds, load or store. Out of bounds,
  holes and growth go to the runtime as today.
- `length` is read inline.
- Named properties on arrays (`arr.foo`) remain unsupported. They can be
  added later with a shape pointer on arrays, not as part of this phase.

### 5. Classes and object literals

- **Instances get their final shape up front.** A class's field
  declarations, and the assignments to `this.x` at the top of its
  constructor, give the field order at compile time. `new C()` allocates
  an object with those slots inline, already in that shape. The
  constructor's stores then hit the "existing slot" cache, with no
  transitions.
- **Object literals** `{ a, b, c }` allocate with three inline slots and
  move through the cached transitions. A literal with only constant keys
  could later be allocated directly in its final shape.
- **The class object and the constructor** keep their current
  representation in this phase. Making functions real objects with shapes
  (`f.prototype`, `f.name`, statics on functions) is a later step.

### 6. Closure environments

Environments stop being objects. The compiler already knows, for every
scope, which variables are captured and in which order (`scope.rs`):

- **An environment becomes an array of slots** plus a pointer to its
  parent: header, parent, slots.
- **New ops `LoadEnvSlot(hops, slot)` and `StoreEnvSlot(hops, slot)`.**
  They replace the `GetProp("%parent")` chains and named lookups. In native
  code each is `hops` pointer loads and one more load: no call.
- **The VM gets the same ops** on a new `Env` heap value, so all three
  backends keep running the same bytecode.
- **`%this`, `%super` and `%fieldinit<N>`** become ordinary environment
  slots.

### 7. Effect on the rest of the system

- **Language behaviour:** unchanged, with two corrections. User keys that
  start with `__` become visible, and an accessor on one object no longer
  slows down every other object. The golden tests define the behaviour;
  phase 2 adds tests for shape transitions, prototype changes after
  caching, dictionary mode, `delete`, accessors, private names and
  environments.
- **The VM:** only environments change (section 6). Its objects stay hash
  maps.
- **Rolls:** no change. Rolls use objects through the language, and no roll
  has native code yet. When FFI comes (#132), its value API is designed
  against this layout.
- **Memory:** objects become smaller and need one allocation instead of
  about four. Nothing is freed yet; that is phase 4, which this layout
  makes easier (an object's references are its proto, overflow and slots).

## Code organisation

New code goes in small modules, and the files phase 2 touches are split
first. Today `src/runtime/stubs.rs` has about 2,700 lines and holds the
whole native runtime.

- **Step 0 splits it without changing behaviour** into modules such as
  `values`, `globals`, `objects`, `calls`, `operators`, `strings`,
  `errors`, `inspect`, `promises` and `builtins_setup`. The tests show
  nothing changed.
- **The new object model** lives in `src/runtime/object/`: `atom.rs`,
  `shape.rs`, `object.rs`, `dictionary.rs`, `ic.rs`, `env.rs`, each a few
  hundred lines.
- **Layout tests** check the real structures: sizes and the field offsets
  that generated code relies on. Today's layout tests check constants
  against themselves.

The other large files (`src/compiler/mod.rs`, `src/vm/mod.rs`,
`src/ir/lower.rs`, the two backends) are split the same way in their own
pull requests, each with no behaviour change.

## Order of work

Each step is one pull request, with tests on vm, jit and aot and the
benchmark table:

1. **Split `stubs.rs`** into modules (no behaviour change), and add real
   layout tests.
2. **Atoms, shapes and the new object layout**, behind the same runtime
   functions: the backends do not change yet. Accessors and internal state
   move out of the property list; dictionary mode.
3. **Inline caches** for property reads, writes and method calls, in both
   backends.
4. **Class instances and object literals** allocated with their slots.
5. **Closure environments** as slot arrays (compiler, VM and both
   backends).
6. **Inline array element access.**

Expected effect, as estimates to be checked by each step's benchmarks:
steps 2 and 3 take `props`, `methods` and `nbody` most of the way;
step 5 is for `closures`; steps 2 and 4 cut the allocations in `alloc`
and `binary_trees`.

## Risks

- **Cache invalidation is where engines have bugs.** The prototype epoch
  is deliberately coarse: any change to any prototype invalidates every
  cached prototype hit. Tests change prototypes after caching on purpose.
- **Shape explosion.** Programs that add properties in many orders create
  many shapes. Dictionary mode bounds the cost per object; a global count
  of shapes is reported in debug builds.
- **One large change in the runtime.** Step 2 keeps every runtime
  function's interface, so the backends and tests do not change at the
  same time as the representation.
