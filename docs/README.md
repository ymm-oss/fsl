# docs/ Map

## Start here

| Document | Contents |
|---|---|
| [`INTRO-formal-methods-and-fsl.md`](INTRO-formal-methods-and-fsl.md) | **Introduction to formal methods and FSL**. Background for non-specialists, the role of FSL in AI-driven development, and considerations for an introductory PoC |
| [`LANGUAGE.md`](LANGUAGE.md) | **Language reference** (full syntax, semantics, CLI, idioms, the three-layer dialects, and NFRs). Read this if you are writing specifications |
| [`DESIGN-v1.md`](DESIGN-v1.md) | Language design document (design principles G1-G5, type-system design decisions, the repair protocol, and the roadmap) |

## Implementation design by architecture and feature (DESIGN-*)

| Document | Subject |
|---|---|
| [`DESIGN-layers.md`](DESIGN-layers.md) | **Shared kernel + three dialects** (consulting / requirements / design): overall concept and validation |
| [`DESIGN-dialects.md`](DESIGN-dialects.md) | Implementation spec for the dialects (declaration tags, fsl-req, fsl-biz) |
| [`DESIGN-nfr.md`](DESIGN-nfr.md) | Non-functional requirements (mapping table, discrete-time SLA: time/urgent/age/deadline) |
| [`DESIGN-induction.md`](DESIGN-induction.md) | The k-induction engine (proved / unknown_cti / CTI) |
| [`DESIGN-trans.md`](DESIGN-trans.md) | `trans` (transition invariant / two-state safety) |
| [`DESIGN-temporal.md`](DESIGN-temporal.md) | leadsTo, weak fairness (lasso counterexamples), and respond scenarios |
| [`DESIGN-refinement.md`](DESIGN-refinement.md) | Refinement checking (mapping files, conditional expressions) |
| [`DESIGN-compose.md`](DESIGN-compose.md) | Spec composition (namespaces, synchronized actions, internal) |
| [`DESIGN-bridge.md`](DESIGN-bridge.md) | Implementation bridge (runtime Monitor / replay / testgen) |
| [`DESIGN-scenarios.md`](DESIGN-scenarios.md) | scenarios and the unsat-core diagnostics for coverage |
| [`DESIGN-seq.md`](DESIGN-seq.md) | Seq<T,N> (partial_op, type whitelist) |
| [`DESIGN-option-struct.md`](DESIGN-option-struct.md) | Option fields in structs |
| [`DESIGN-divmod.md`](DESIGN-divmod.md) | Integer division `/` and remainder `%` (total definition of division by zero, partial_op, Euclidean) |
| [`DESIGN-forbidden.md`](DESIGN-forbidden.md) | `forbidden` (negative acceptance criteria / must-forbid) — detecting under-constraint |
| [`DESIGN-vacuity.md`](DESIGN-vacuity.md) | Vacuity checking (invariants whose antecedent is unreachable, leadsTo whose trigger is unreachable, always-true requires) |
| [`DESIGN-strict-tags.md`](DESIGN-strict-tags.md) | The `--strict-tags` lint (matching untagged declarations and unreferenced requirements) |
| [`DESIGN-mutate.md`](DESIGN-mutate.md) | `fslc mutate` (spec mutation, requirement stress report) |
| [`DESIGN-explain.md`](DESIGN-explain.md) | `fslc explain` (skeleton enumeration, counterfactuals, witness narration) |
| [`DESIGN-typestate.md`](DESIGN-typestate.md) | `fslc typestate` (applicability check for state machine → typestate + TS scaffold) |
| [`DESIGN-ui.md`](DESIGN-ui.md) | fsl-ui (screen-transition dialect): spike findings, proposed expansion rules, go/no-go (#9) |

## Dogfooding records (DOGFOOD-*)

Findings, bugs, and discoveries from putting each feature into real use. These form the basis of the design decisions.

1. [`DOGFOOD-1.md`](DOGFOOD-1.md) — v1.0 field evaluation (found BUG11-14, PERF1)
2. [`DOGFOOD-2.md`](DOGFOOD-2.md) — proved as standard practice, Seq (discovery of the aggregation idiom)
3. [`DOGFOOD-3.md`](DOGFOOD-3.md) — full workflow (abstract → refine → compose → implementation)
4. [`DOGFOOD-4.md`](DOGFOOD-4.md) — penetration of the three-layer dialects (cross-layer diagnostics by requirement ID)
5. [`DOGFOOD-5.md`](DOGFOOD-5.md) — NFR / discrete-time SLA
6. [`DOGFOOD-6.md`](DOGFOOD-6.md) — bug hunt in the example gallery (two refine misses)
7. [`DOGFOOD-7.md`](DOGFOOD-7.md) — golden-oracle test suite (Monitor BFS, trace soundness, BUG-020)
8. [`DOGFOOD-8.md`](DOGFOOD-8.md) — blind expressibility test (external validation of G1)
9. [`DOGFOOD-9.md`](DOGFOOD-9.md) — real run of the validation workflow (memo → positive-example pair → repair)
10. [`DOGFOOD-10.md`](DOGFOOD-10.md) — fault-injection benchmark (measuring detector capture rate by category × mechanism)

Worked examples are in [`../specs/`](../specs/) (standalone specs) and [`../examples/`](../examples/)
(bank: implementation conformance / layers: three-layer chain / nfr: SLA).
The AI-agent skills are under [`../skills/`](../skills/), split into shared FSL
syntax plus business, requirements, design, and design-review entry points.
