# Prototype: string functions in Oite on byte primitives

Not for merging. Measures whether `@rolls/string` can be written in Oite,
on a few general byte primitives, instead of per-operation intrinsics.

- Primitives (intrinsics): `byte_length`, `byte_at`, `byte_slice`,
  `string_from_bytes` (src/runtime/intrinsics.rs). `byte_at` and
  `byte_length` have a fast path in `ot_intrinsic`.
- `bytestr.ot`: `indexOf`, `slice`, `trim`, `split`, `toUpperCase` in Oite.
  `check.ot` compares them with the built-in methods.
- `core_*.ot` / `byte_*.ot`: the same work with core's Rust methods and
  with `bytestr.ot`, on a 10,000-character string, 200 times.

Native results (ms, three runs, AOT):

| Work | Core (Rust) | Oite on bytes |
| --- | --- | --- |
| slice + split + trim | 45-59 | 73-83 |
| indexOf (full scan) | 19-22 | 30-32 |
| toUpperCase | 13-21 | 69-106 |

The current `@rolls/string` (`charAt` loops) ran out of memory after 59 s
on the first benchmark. On the VM the byte versions are 50-170x slower
than the Rust methods.
