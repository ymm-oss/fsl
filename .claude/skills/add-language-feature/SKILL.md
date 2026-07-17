---
name: add-language-feature
description: Scaffold and verify a new native Rust FSL language feature while keeping syntax, lowering, symbolic/concrete semantics, docs, skill reference, tests, and changelog coupled.
---

# Add a native FSL language feature

The Rust workspace is authoritative. Before editing, classify the feature as surface-only lowering,
typed Kernel behavior, evaluator semantics, CLI/report behavior, or public Kernel contract.

## Coupled checklist

- `rust/fsl-syntax`: tokens, parsing, AST, and locations when syntax changes.
- `rust/fsl-core`: validation, types, name resolution, dialect lowering, and Kernel projection.
- `rust/fsl-runtime`: concrete Monitor/BFS behavior for evaluable semantics.
- `rust/fsl-verifier`: symbolic BMC/induction/refinement/liveness behavior.
- `rust/fsl-tools` or `rust/fslc`: report/command/envelope behavior when applicable.
- `rust/fsl-wasm`: only when Worker-visible behavior or shared envelopes require it.
- Focused positive, negative, and boundary regression evidence.
- `docs/LANGUAGE.md`, `skills/fsl/reference.md`, relevant `docs/DESIGN-*.md`, docs map, and
  `CHANGELOG.md` `[Unreleased]`.
- Schemas/conformance vectors when the public Kernel contract changes.

Do not update `src/fslc` by default. Record the explicit compatibility or LSP contract before changing
the frozen Python reference.

## Verification

1. Iterate with the native CLI:
   `cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- check <case>` and the smallest relevant
   `verify` command.
2. Run focused syntax/core/runtime/verifier tests for the changed seam.
3. Check symbolic/concrete/BFS agreement and a false-negative case.
4. Run induction, mutation, vacuity, refinement, replay, or browser evidence when the contract calls for
   it.
5. Run formatting and Clippy, then wider workspace and compatibility gates in proportion to impact.
6. Invoke the soundness and coupled-change reviewers before completion.

Never widen an allowlist, hollow a spec, or hand-edit a generated artifact to make the gate pass.
