Added (#1052): an insurance-claims example corpus at three scales under
`examples/claims/`. S is a single `requirements` layer, M adds a design layer with
inline `implements` and a `dbsystem` schema window, and L is a four-layer chain with
two external refinement mappings and a `domain` saga. `examples/claims/README.md`
carries the per-tier scale table, the verification commands, and the measured mutation
evidence for every spec, including the survivor breakdown and which property class
kills which mutant group. `rust/fslc/tests/refine_corpus_parity.rs` registers the L
tier's two mappings, with its projection golden regenerated to match. No `fslc`
behaviour changes.
