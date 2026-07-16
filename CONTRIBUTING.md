# Contribution Guide

Contributions to FSL (`fslc`) are welcome. Bug reports, feature proposals, documentation improvements,
and code changes should preserve FSL's observable language and evidence contracts.

## Read these first

- [`docs/LANGUAGE.md`](docs/LANGUAGE.md) — complete language contract.
- [`docs/README.md`](docs/README.md) — documentation map.
- [`docs/DESIGN-rust-port.md`](docs/DESIGN-rust-port.md) — authoritative implementation boundary.
- [`docs/RUST-PORTING.md`](docs/RUST-PORTING.md) — differential and replay evidence gates.
- Relevant `docs/DESIGN-*.md` for the surface being changed.

## Implementation boundary

The native Rust workspace under `rust/` is authoritative, including the `fslc-lsp` language server.
The Python package under `src/fslc/` is a frozen compatibility reference. New product and language
behavior belongs in Rust. Change Python only when an accepted compatibility requirement explicitly
calls for it.

The most important structural boundary is that `fsl-runtime` remains independent of `fsl-solver`, Z3,
and JavaScript solver bridges. Symbolic BMC, the concrete Monitor, solver-free BFS, compatibility
fixtures, and cross-implementation replay provide independent evidence; agreement between two paths
that share one solver is insufficient.

## Development environment

Install a stable Rust toolchain with `rustfmt` and `clippy`, Node.js 22, and the pinned
`wasm-bindgen-cli` used by CI. Python 3.9+ is optional unless changing the frozen compatibility
reference or Python-based repository hooks:

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -e ".[dev]"
```

Run the working-tree native CLI without relying on a globally installed `fslc`:

```bash
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- check specs/cart_v1.fsl
cargo run --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc -- verify specs/cart_v1.fsl --depth 8
```

## Testing

Start with the smallest relevant package or integration test. Before submitting a product change,
run the single required CI-equivalent gate:

```bash
./tools/check-native-integration.sh
```

Native solver changes also require focused tests for `fsl-solver-z3`, `fsl-verifier`, and `fslc-rust`.
Public Kernel or CLI changes require the relevant Rust envelope, schema, corpus, and bidirectional
replay cases. See [`docs/DESIGN-rust-integration.md`](docs/DESIGN-rust-integration.md) for the contract
inventory and explicitly optional Python surfaces.

## Guidelines for changes

- **Language or semantics:** update Rust syntax/lowering, typed model, symbolic and concrete evaluation,
  regression cases, `docs/LANGUAGE.md`, `skills/fsl/reference.md`, an accepted design note, and
  `CHANGELOG.md` together.
- **Public Kernel contract:** update schemas, Rust exporter/consumer paths, conformance vectors,
  agreement tests, `docs/DESIGN-kernel-contract.md`, language/reference docs, and changelog.
- **CLI/JSON contract:** preserve field meanings, ordering requirements, raw-output modes, exit codes,
  locations, and replayable evidence. Any parity allowlist entry needs a nondeterminism rationale.
- **Specs and examples:** run native `check`, bounded verification, induction where relevant, mutation,
  and vacuity checks. Do not weaken an invariant to erase a real counterexample.
- **Frozen Python reference:** do not mirror every Rust feature into Python. A Python change needs an
  explicit compatibility reason and focused regression evidence.
- **Generated artifacts:** never hand-edit compatibility snapshots or generated site output. Use the
  owning generator and review the diff.

Use repository-relative paths in committed text and examples. New source files require the Apache-2.0
SPDX header used by neighboring files. Rust must remain formatted, Clippy-clean, and free of unsafe code.

## Commits and pull requests

Use a dedicated branch/worktree for non-trivial work and preserve unrelated local changes. Keep one
topic per commit, use a Conventional Commit-style subject, and add notable changes under
`CHANGELOG.md` `[Unreleased]`.

A pull request should describe the problem, the accepted contract, implementation scope, verification
evidence, linked issue, and any documentation or agent-skill changes. Bug reports and proposals should
include a minimal reproducing `.fsl`, the exact command, and observed versus expected behavior.

Report security issues privately through [`SECURITY.md`](SECURITY.md), not a public issue. All
participation is governed by [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## License

Contributions are provided under Apache-2.0. Add the repository SPDX header to new source files. When
the file type's neighboring files carry a copyright line, follow that convention as well.
