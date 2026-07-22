---
paths:
  - "rust/**/*.rs"
  - "rust/Cargo.toml"
  - "rust/**/Cargo.toml"
  - "rust/Cargo.lock"
---

# Native Rust architecture rules

- The Rust workspace is authoritative. Before changing it, read
  [`DESIGN-rust-components.md`](../../docs/DESIGN-rust-components.md) and
  [`DESIGN-rust-component-internals.md`](../../docs/DESIGN-rust-component-internals.md). They own
  the accepted crate and in-crate responsibility boundaries; do not copy Python structure.
- Preserve the owner map: `fsl-syntax` owns source fidelity; `fsl-core` owns checked models and
  Public Kernel; `fsl-runtime` owns concrete Monitor/replay/BFS semantics; `fsl-solver` owns the
  backend-neutral boundary; `fsl-solver-z3` and `fsl-solver-z3js` are native and browser adapters;
  `fsl-verifier` owns symbolic engines; `fsl-tools` owns derived artifacts; and `fslc-rust`,
  `fsl-wasm`, and `fsl-lsp` own native delivery, Worker delivery, and editor projection.
- Keep dependency direction explicit: `fsl-runtime -> fsl-core -> fsl-syntax`, while
  `fsl-verifier -> fsl-solver` and `fsl-verifier -> fsl-core`.
- `fsl-runtime` must not depend directly or transitively on solver or Z3 crates.
- A semantic construct implemented symbolically must have matching concrete/explicit-state behavior.
  Add a negative case capable of exposing a false-green result.
- Preserve raw-output modes as raw, JSON envelopes, exit codes, Public Kernel and replay contracts,
  and native/Worker parity. Delivery adapters must not independently redefine semantic validity.
- The accepted logical module tree is an ownership map, not permission for an eager rewrite. Every
  source-changing C2 candidate requires its own issue and scope, an independently revertible change,
  a positive oracle, and a negative control that rejects known contract drift.
- Semantic and derived transforms receive filesystem data through explicit input or a resolver. The
  existing `testgen::relative_spec_path` access is bounded debt owned by #423; do not copy it or add
  another implicit filesystem dependency.
- Preserve checked `i64` arithmetic and SMT-LIB Euclidean division/remainder behavior.
- Run the smallest affected crate tests first, then formatting, Clippy, and wider workspace gates.
- New Rust source files require `// SPDX-License-Identifier: Apache-2.0`.
