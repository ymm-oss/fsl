Rewrote the root `README.md` around the native Rust workspace (`rust/`) as the
authoritative implementation, replacing the stale "Lark + Z3" / `src/fslc/`
product framing, a 47-line directory tree with no `rust/` entry, a
`pytest`/"Library API" contract that AGENTS.md no longer treats as the
product gate, and three overlapping, self-contradicting install paths with
one consolidated install section, a `rust/` crate layout, the full native
CLI surface (`kernel`, `document`, `ledger`, `mutate`, `refine`, `chain`,
multi-target `testgen`, `replay --from-log/--mapping`, `analyze`
projections), and the Public Kernel JSON contract as the supported
programmatic surface in place of a Python library import.
