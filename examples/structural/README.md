# Structural Examples

## What this is

Step 1 of issue #35: zero-cost demand validation for Alloy-style structural
discovery in current FSL. There is no language change here.

| File | What it tests |
|---|---|
| `rbac.fsl` | Flattened RBAC relations, cardinality/multiplicity, relational join via nested quantifiers, and bounded existence of a valid assignment. |
| `graph_reach.fsl` | Bounded graph reachability and acyclicity with hand-inlined transitive closure. |

## The idiom

Model relations as flattened-product `Map<Cell,Bool>` state. Recover tuple axes
with `/` and `%` when needed, or by reconstructing indices such as `u*NR + r`.
Use non-deterministic populate-style actions to build structure step by step.
Use `reachable` for the Alloy-run-style witness search.

Multiplicity maps directly to existing predicates: `unique` for at-most-one,
`exactlyOne` for exactly-one, `exists` for some, and `count` when a numeric
cardinality is clearer. Relational join is nested existential quantification over
the shared axis.

Transitive closure is encoded as a hand-inlined n-step union. In
`graph_reach.fsl`, the duplicated closure expression at both use sites is
intentional: it documents the current friction rather than hiding it behind a
local workaround.

## Results

Run from the repository root after:

```bash
source .venv/bin/activate
```

| Command | Result | checked_to_depth | Completeness | elapsed_s | Notes |
|---|---:|---:|---|---:|---|
| `fslc check examples/structural/rbac.fsl` | `ok` | n/a | n/a | n/a | `warnings: []` |
| `fslc verify examples/structural/rbac.fsl --depth 6` | `verified` | 6 | `bounded` | 1.051522 | `WellFormedAssignment` witnessed at step 3 |
| `fslc verify examples/structural/rbac.fsl --depth 6 --vacuity warn` | `verified` | 6 | `bounded` | 1.04276 | `warnings: []`; reachable witnessed at step 3 |
| `fslc check examples/structural/graph_reach.fsl` | `ok` | n/a | n/a | n/a | `warnings: []` |
| `fslc verify examples/structural/graph_reach.fsl --depth 5` | `verified` | 5 | `bounded` | 0.639819 | `ReachAcyclicDAG` witnessed at step 1 |
| `fslc verify examples/structural/graph_reach.fsl --depth 5 --vacuity warn` | `verified` | 5 | `bounded` | 0.859933 | `warnings: []`; reachable witnessed at step 1 |

## Determination

The idiom covers referential integrity, cardinality/multiplicity (`lone`, `one`,
`some`), relational joins, and "does a valid configuration exist" with no
language change and good ergonomics. About 80% of practical structural checks fit
the existing frame, as shown by `rbac.fsl`.

Transitive closure (`^` / `*`) is the single feature with real friction, for
three reasons visible in `graph_reach.fsl`: no closure operator means the n-step
union is hand-unrolled; no spec-level predicate/macro abstraction means the
unrolled closure must be copy-pasted at every use site; and the hop bound is
soundness-critical. Distinct-pair reachability needs K = N-1, while
cycle/self-reachability needs K = N; the uniform K = N union is sound.

Honesty/contract notes for issue risks #1 and #4: every result is
`completeness: "bounded"`, and reachable witnesses are bounded. These
static-structure queries have no transition story, so k-induction adds nothing.
The Monitor/oracle.py dual-evaluator safety net does not apply; this is the exact
trust-model regression the issue warns about. Any future helper must self-label
`completeness: "bounded"`, `induction: "n/a"`, and `cross_check: "none"`.

Recommendation: the existing idiom is sufficient for the bulk of this work, so
full kernel relational algebra stays deferred as issue Step 3 / wontfix. The only
element worth a narrow frontend Step 2 helper, and only if closure-heavy
structural discovery proves recurrent in requirements/design layers, is a bounded
transitive-closure helper such as `reaches(R,a,b)` desugaring to the sound K = N
unrolling, plus optionally a multiplicity shorthand. Until recurring demand
appears in real specs, no language change is warranted.
