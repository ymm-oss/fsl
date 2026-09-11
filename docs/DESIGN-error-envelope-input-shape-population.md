# Error-envelope input-shape population

Issue #801 separates the semantic input-shape population from the
error-envelope matrix's per-fixture classification. The matrix owns only the
cell-or-reasoned-`NotApplicable` classification. `INPUT_SHAPE_CATALOG` and
`INPUT_SHAPE_POPULATIONS` in `rust/fslc/tests/error_envelope_parity.rs` are
the single owner of the matrix population.

The `InputShape` declaration and exhaustive inventory are generated from one
list. The catalog contains every listed shape exactly once, and each native
frontend is assigned exactly once; a new native dialect therefore fails the
matrix until its envelope shape is classified. The parity registry and the
owner command table are exact sets with no duplicate command rows. A new CLI
leaf has no fallback population: it fails until it receives an explicit command
row and input-shape profile.

`tests/dialect_registry.py` remains a separate corpus/compatibility projection.
It is neither imported nor duplicated by this owner: its `DIALECTS` and
`EVIDENCE_CONSTRUCTS` classify corpus evaluation, while this catalog classifies
CLI error-envelope frontends. Both derive their language-level coverage from
the Rust frontend registry, avoiding two owners for the same CLI input-shape
population.

The bidirectional owner test enforces three properties:

- Every executable cell and concrete `NotApplicable` tuple belongs to its
  command/class owner population.
- Every owner tuple has exactly one executable cell or reasoned
  `NotApplicable` classification.
- Every `NotApplicable` carries a nonempty reason.

The closed set of production AI component/project dispatch commands is named
explicitly, rather than inferred from an `ai ` prefix. The independent
dispatch-population test asserts its Component/Project coverage separately
from the catalog and is the detector for the mutation that deletes a cell and
pin while narrowing the AI population.
