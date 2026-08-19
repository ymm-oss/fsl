Documented (#827): the root `README.md` rebuilt around the native Rust
workspace (`rust/`) as the authoritative implementation, replacing the stale
"Lark + Z3" / `src/fslc/` product framing, a 47-line directory tree with no
`rust/` entry, a `pytest` / "Library API" contract that `AGENTS.md` no longer
treats as the product gate, and three overlapping, self-contradicting install
paths. In their place: one consolidated install section with a chooser, a
`rust/` crate layout, the native CLI surface that was missing (`kernel`,
`document`, `ledger`, multi-target `testgen`, `replay --from-log/--mapping`,
`--engine explicit|auto`, `analyze` projections), editor integration, and the
Public Kernel JSON contract as the supported programmatic surface in place of
a Python library import. The `specs/`/`examples/` per-file inventories are
deliberately not restored -- a hand-maintained list is what went stale.
