# Error-envelope input-shape population

Issue #801 separates the semantic input-shape population from the
error-envelope matrix's per-fixture classification. The matrix owns only the
cell-or-reasoned-`NotApplicable` classification. `INPUT_SHAPE_CATALOG` and
`INPUT_SHAPE_POPULATIONS` in `rust/fslc/tests/error_envelope_parity.rs` are
the single owner of the matrix population.

The catalog is checked exactly against `fsl_syntax::DIALECT_KEYWORDS`; a new
native dialect therefore fails the matrix until its envelope shape is
classified. The parity registry is checked exactly against CLI-contract leaves;
a new leaf therefore fails until its command scope is classified, then receives
the Source population by default or the AI dispatch population automatically.

`tests/dialect_registry.py` remains a separate corpus/compatibility projection.
It is neither imported nor duplicated by this owner: its `DIALECTS` and
`EVIDENCE_CONSTRUCTS` classify corpus evaluation, while this catalog classifies
CLI error-envelope frontends. Both derive their language-level coverage from
the Rust frontend registry, avoiding two owners for the same CLI input-shape
population.

The independent dispatch-population test derives AI command membership from
the CLI parity registration and asserts Component/Project coverage separately
from the catalog. It is the detector for the review mutation that deletes a
cell and pin while narrowing the AI population. `NotApplicable` remains the
only permitted omission path and must carry a nonempty reason.
