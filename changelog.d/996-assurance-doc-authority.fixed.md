Fixed (#996): `docs/DESIGN-assurance-classes.md` now documents the native Rust
assurance classifier (`ledger::assurance_token`, `formal_assurance`,
`evidence_verdict`, and related APIs) as the authoritative contract; the frozen
Python `src/fslc/assurance.py` surface is retained as a reference-only parity
map. Rust/Python `formal_assurance` vs `classify_element` evaluation-order
divergence on `result:"error"` with `completeness` set remains explicitly
unadjudicated (issue #995).
