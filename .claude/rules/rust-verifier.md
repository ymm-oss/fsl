---
paths:
  - "rust/**/*.rs"
  - "rust/Cargo.toml"
  - "rust/Cargo.lock"
---

# Native Rust verifier rules

- The Rust workspace is authoritative. Design a feature around the typed Rust boundaries rather than
  porting Python implementation structure mechanically.
- Keep dependency direction explicit: `fsl-runtime -> fsl-core -> fsl-syntax`, while
  `fsl-verifier -> fsl-solver` and `fsl-verifier -> fsl-core`.
- `fsl-runtime` must not depend directly or transitively on solver or Z3 crates.
- A semantic construct implemented symbolically must have matching concrete/explicit-state behavior.
  Add a negative case capable of exposing a false-green result.
- Preserve checked `i64` arithmetic and SMT-LIB Euclidean division/remainder behavior.
- Preserve native/Worker envelope semantics and replay witnesses rather than comparing unstable solver
  model bytes.
- Run the smallest affected crate tests first, then formatting, Clippy, and wider workspace gates.
- New Rust source files require `// SPDX-License-Identifier: Apache-2.0`.
