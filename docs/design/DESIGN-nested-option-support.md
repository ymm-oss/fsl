<!-- SPDX-License-Identifier: Apache-2.0 -->

# Issue #841: nested `Option` full-support implementation design

Status: Accepted. Acceptance baseline: `155203331d95a12f912b39a84e8803fa79f45ed1`
(current `HEAD` on 2026-08-27).

## 1. Decision and scope

The maintainer decision is option B: nested `Option` is a supported language
capability. This document does not reconsider options A or C.

The supported capability is recursive rather than special-cased at depth two:

```text
Scalar          ::= Int | Bool | Range | Domain | Enum
OptionalScalar  ::= Scalar | Option<OptionalScalar>
```

Thus `Option<Option<Bit>>`, `Option<Option<Option<Bit>>>`, and the same
`OptionalScalar` in an allowed struct field or Map value are legal. A fixed
two-level implementation would merely move the current unsound cliff to depth
three and is not acceptable.

`Option<Struct>`, `Option<Set<_>>`, `Option<Map<_, _>>`,
`Option<Seq<_, _>>`, `Option<relation _ -> _>`, nested structs, and collection
fields in structs remain unsupported. Every surface form that resolves to one
of those shapes must fail during `check`, with `kind: "type"`, exit 2, and the
state/struct-field declaration `loc`. No verifier or runtime is allowed to be
the first component to reject such a model.

One-line semantic design:

> Resolve the assignment target type before evaluating its right-hand side;
> recursively encode each Option layer as `(present, payload)` in BMC and as
> `None | Some(value)` concretely, then require BMC, Monitor, and explicit BFS
> to agree on the three distinct states `none`, `some(none)`, and
> `some(some(v))`.

## 2. Evidence and present failure

The following citations were re-read at the acceptance baseline.

| Observation | Current evidence | Consequence |
|---|---|---|
| At the acceptance baseline, the written contract rejected nested Option | `docs/design/DESIGN-option-struct.md:10-14` records the v2.1 rejection and its subsequent supersession | This was an intentional contract change to `OptionalScalar`, not silent drift. |
| Syntax and public AST are already recursive | `rust/fsl-syntax/src/surface.rs:20-29`, `rust/fsl-syntax/src/parser.rs:1593-1631`, and `TypeExpr::kernel_ast_v1` at `surface.rs:732-747` | No grammar production or AST variant is needed. Parser preservation controls are still required. |
| The resolved kernel model is already recursive | `rust/fsl-core/src/model.rs:18-49`; `resolve_type` at `model.rs:1337-1367`; defaults at `model.rs:214-247` | No new `FslValue` or `TypeRef` variant is needed. `Value`'s derived `Eq`/`Ord` already distinguishes the three values. |
| Typechecking already propagates an assignment expectation | `rust/fsl-core/src/typecheck.rs:189-201`, `438-450`, and `952-967` | `x = some(none)` is correctly accepted for `x: Option<Option<T>>`; the verifier later discards that context. |
| The documented state whitelist is not centrally enforced | The state builder at `rust/fsl-core/src/model.rs:603-678` rejects `Map<Int, _>` and resolves arbitrary `TypeExpr`, but has no total state-shape validation; the only recursive struct-field gate is `model.rs:1017-1058` | Add one exhaustive resolved-type gate. Do not infer safety from top-level parser/type counts. |
| The BMC failure is an untyped inner `none` | `rust/fsl-verifier/src/eval.rs:65-76` evaluates `Some` without an expectation and emits `"some() requires a typed value"` at line 71 | The failure is contextual typing, not a missing symbolic Option representation. |
| Comparison happens to work | Equality evaluates its opposite operand with `eval_expected` at `eval.rs:1533-1599`; that helper already recurses through `Some` | Preserve the comparison test, but do not mistake it for assignment coverage. |
| Init and action assignment drop the expectation | `rust/fsl-verifier/src/transition.rs:443-456` and `579-599` call plain `eval`; `assign` only discovers/coerces to the target type later at `transition.rs:658-674` and sibling indexed/field arms | Determine the lvalue type first and call contextual evaluation before `assign`. |
| Whole-struct and conditional expressions have the same sibling hole | `eval_struct_literal` calls plain `eval` before `coerce` at `eval.rs:1219-1244`; conditional branches call plain `eval` at `eval.rs:146-161` | Context propagation must cover constructors and branches, not only root-variable assignment. |
| Symbolic representation and bounds already recurse | `rust/fsl-verifier/src/value.rs:20-62`, `719-742`, and `869-909`; induction bound discovery at `rust/fsl-verifier/src/induction.rs:103-114` | Reuse the representation; do not introduce an alternate nested-Option encoding. |
| Concrete evaluation already preserves nesting | `rust/fsl-runtime/src/lib.rs:96-109` and `value_conforms` at `lib.rs:3707-3765` | Monitor requires controls, not a new value representation. |
| Explicit BFS uses Monitor states and steps | `rust/fsl-runtime/src/explicit.rs:127-175`; the frontier/seen sets contain the concrete `State` | Explicit and Monitor are two execution surfaces but one concrete semantic lineage. |
| Current verifier failure loses source location | `VerifyError` from the verifier is rendered as a generic semantics error in `rust/fslc/src/verification.rs:1122-1141` | Legal nested Option must no longer reach this path; illegal shapes must fail from a spanned `ModelError`. |
| Ordinary state JSON collapses the new distinction | `rust/fsl-core/src/trace_json.rs:78-105` renders both `None` and `Some(None)` as `null`; replay input decodes `null` as the outer `None` at `rust/fslc/src/main.rs:15989-16065` | Full support requires a lossless ordinary trace/replay form while retaining legacy bytes for `Option<scalar>`. |
| Conformance JSON is already lossless | `rust/fslc/src/lib.rs:412-490` recursively emits tagged Option values; `docs/design/DESIGN-kernel-contract.md:202-205` states the distinction | Preserve this separate, fully tagged contract and strengthen its exact assertions. |
| Existing tests acknowledge but do not close the assignment gap | `rust/fsl-verifier/tests/expression_agreement.rs:250-268`, `rust/fslc/tests/kernel_contract.rs:207-247`, and `rust/fslc/src/coverage.rs:691-696,993-995` | Keep all three; add assignment-side and exact-value controls. |

The reported path is therefore:

```text
checked assignment
  -> typecheck knows Option<Option<T>>
  -> transition::collect_init_statement / compute_statement calls eval(rhs)
  -> eval(Some(none)) calls eval(none)
  -> SymbolicValue::None has no ty()
  -> eval.rs:71 VerifyError("some() requires a typed value")
  -> generic CLI semantics error, no loc, exit 2
```

## 3. Normative state-type boundary

### 3.1 Total resolved-type predicates

Implement the boundary in `fsl-core` over resolved `TypeRef` and `TypeDef`,
not by matching source strings and not independently in each engine. The
implementation should use exhaustive `match` expressions without `_` arms.

```text
scalar(T) =
  T is Int, Bool, Range, or Named(Domain|Enum)

bounded_scalar(T) =
  T is Bool, Range, or Named(Domain|Enum)

optional_scalar(T) =
  scalar(T), or T is Option<U> and optional_scalar(U)

struct_value(T) =
  T is Named(Struct fields) and every field type is optional_scalar

legal_state(T) =
  optional_scalar(T)
  or struct_value(T)
  or T is Map<K,V> and bounded_scalar(K)
       and (optional_scalar(V) or struct_value(V))
  or T is Set<K> and bounded_scalar(K)
  or T is Seq<V,N> and scalar(V)
  or T is Relation<A,B> and bounded_scalar(A) and bounded_scalar(B)
```

`Int` remains a scalar but is not a bounded collection key. `Option` is legal
only through `optional_scalar`; it is never a generic wrapper around another
otherwise-legal state shape. A named struct is legal only at the state root or
as a Map value, never through `Option` and never as another struct field.

This inventory covers every current `TypeRef` variant: `Int`, `Bool`, `Range`,
`Named(Domain)`, `Named(Enum)`, `Named(Struct)`, `Option`, `Map`, `Set`, `Seq`,
and `Relation`. Unknown named types continue to fail during resolution before
the shape gate.

### 3.2 Placement matrix

| Placement | Legal examples | Rejected controls |
|---|---|---|
| State root | `Option<Option<Bit>>`, `Option<Option<Option<Bit>>>` | `Option<Box>`, `Option<Set<Bit>>`, `Option<Map<Id,Bit>>`, `Option<Seq<Bit,1>>`, `Option<relation Id -> Id>` |
| Struct field | `Option<Option<Bit>>` | a struct field whose resolved type is struct, Set, Map, Seq, relation, or Option of any of those |
| Map value | `Option<Option<Bit>>`, or a struct with `OptionalScalar` fields | Set, Map, Seq, relation, or Option-wrapped struct/collection |
| Set element / Map key / relation endpoint | existing bounded scalar only | Option at any depth, struct, collection, and unbounded `Int` |
| Seq element | existing scalar only | Option at any depth, struct, and collection |

The same gate runs after type resolution for direct Kernel source and for
dialect-lowered `KernelModel` construction. It must not leave a path by which a
dialect can construct a model that source `check` would reject.

**Breaking removal recorded (#925).** Before PR #923 native `check` accepted
`Map<bounded scalar, Map<bounded scalar, scalar>>` as a state variable; the
fixture was then map_nested_bounded_key_accepted.fsl and the required test was
native_check_accepts_nested_bounded_map_keys (both removed by PR #923). The
measured reason (release `fslc 4.4.1`, SHA `af56aa8`, whose `rust/` tree
differs from `ca2d5e7d`, the `main` commit PR #923 merged onto, only in the
test file `rust/fslc/tests/error_envelope_parity.rs`, which is not compiled
into the binary (`git diff --stat af56aa8 ca2d5e7d -- rust/`)): `check` returned
`ok`; the grammar has no chained index assignment, so an inner Map element cannot
be written in `init` or an action (`m[i][k] = true` is a parse error
`expected '='`); the explicit engine rejected every expressible `init` (`init does
not assign state variable(s): m`, or `init references state variable 'm' before
it is assigned`). The accepted form therefore had no end-to-end execution path —
a hollow accepted construct. The "Map value" row's `Map` rejection is correct and
PR #923 shipped it as a breaking change: fixture
`rust/fslc/tests/fixtures/map_nested_bounded_value_rejected.fsl`, test
`native_check_rejects_nested_bounded_map_values_with_a_located_state_type_contract`
in `rust/fslc/tests/cli_regression.rs`, fragment
`changelog.d/841-nested-map-state-boundary.changed.md`, and the normative wording
in `docs/manual/LANGUAGE.md`, `docs/manual/LANGUAGE.ja.md`, and
`skills/fsl/references/syntax.md`. This row was discovered by a required-test
failure during PR #923, not by design review: the acceptance evidence of PR
#916 records no measurement of existing accepted forms against the matrix, for
this row or any other; section 11 defines the procedure that closes this gap.

### 3.3 Diagnostics

For each rejected state field, return a `ModelError` at `field.span` before
initializer or verifier execution. For a rejected struct field, retain the
declaration span used by `model.rs:1020-1037`, because `TypeExpr` has no
component span. The public result is:

```json
{
  "result": "error",
  "kind": "type",
  "message": "state variable 'x' has unsupported state type",
  "hint": "state types allow scalars, nested Option around a scalar, structs with those fields, Map<bounded scalar, scalar-or-nested-Option-or-struct>, Set<bounded scalar>, Seq<scalar,N>, and bounded-scalar relations; Option cannot wrap a collection or struct",
  "loc": {"line": 3, "column": 3}
}
```

The wording may be polished once, but tests must fix `kind`, nonempty actionable
hint, and exact `loc`. Add the new message class to
`verification_output::semantic_error_kind`; otherwise the model rejects at the
right time but is mislabeled `semantics`. Update the existing struct-field hint
from `Option<scalar>` to nested Option around scalar. CLI and shared LSP
diagnostics must remain identical.

## 4. Execution semantics

### 4.1 Logical value

For a scalar `T`, `Option<Option<T>>` has these disjoint logical values:

| FSL value | Concrete `FslValue` | Symbolic representation |
|---|---|---|
| `none` | `None` | outer `present = false`; payload is the canonical default of `Option<T>` and is semantically ignored |
| `some(none)` | `Some(None)` | outer `present = true`; inner `present = false`; scalar payload is canonical default and ignored |
| `some(some(v))` | `Some(Some(v))` | both presence bits true; scalar payload is `v` |

Equality is recursive logical equality: presence bits must agree and payloads
are compared only when the corresponding presence bit is true. Bounds likewise
apply only under every enclosing `present`. Existing `logical_equal`, `coerce`,
`bounds`, and `has_bounds` already implement that recursion and remain the one
semantic definition.

### 4.2 Contextual symbolic evaluation

Add `KernelModel::state_lvalue_type(&LValue) -> Result<TypeRef, ModelError>` in
`fsl-core` as the one target-type lookup. It starts only from `model.state` and
`model.types`, covers root, Map/Relation index, root struct field, and Map-value
struct field targets, and does not validate the index expression itself.
Typechecking validates the index separately and translates
this helper's error to its authored statement span; the verifier translates it
to `VerifyError`. The verifier must not rediscover the target type differently
from the checked model.

Expose the existing evaluator helper within `fsl-verifier` as
`pub(crate) eval_expected(expr, expected)` and make it the contextual entry
point:

1. `none` expected as `Option<U>` becomes a typed symbolic Option with
   `present=false` and `default(U)` payload.
2. `some(e)` expected as `Option<U>` becomes a typed symbolic Option with
   `present=true` and `eval_expected(e, U)` payload.
3. A struct constructor evaluates every field with its declared field type.
4. Both branches of a conditional are evaluated with the same expected type
   before `ite_value` joins them.
5. Set/Seq/empty-relation literals retain their existing expected collection
   coercion behavior.
6. All other expressions use ordinary `eval` followed by the existing
   `coerce`.

Both init and action assignment resolve the target expectation first, evaluate
the RHS with it, and then call `assign`. This covers inline initializers because
`model.rs:655-665` normalizes them to init assignments. Apply the same rule to
root, indexed, field, and Map-value field assignments; a fix only in the root
arm is incomplete.

Uncontextualized `none` and `some(none)` remain invalid where the typechecker
has no expected type, such as a type-less local expression. That is not a
runtime/verifier error path: checked-model construction must reject it with the
authored expression span.

### 4.3 Monitor and explicit BFS

Monitor keeps `Expr::Some(e) -> Value::Some(eval(e))` and recursively checks
`value_conforms`. Add no second normalization layer. Its accepting controls
must show the exact sequence `None -> Some(None) -> Some(Some(1)) -> None`.

Explicit BFS continues to store full concrete `State` values in `frontier`,
`seen`, and `parents`, and to obtain successors through `Monitor`. Since
`FslValue` derives `Eq` and `Ord`, the three values remain distinct during
deduplication and trace reconstruction. A dedicated wiring detector must still
prove that explicit exploration inserts the Monitor successor rather than its
parent; sharing the evaluator does not prove correct BFS wiring.

### 4.4 Three-way agreement and the triangulation boundary

Use `rust/fslc/tests/typed_agreement/engines.rs::compare_agreement` as the main
three-surface control. It already compares Monitor BFS, explicit, and BMC
verdicts/reachable steps/action coverage, replays evidence, and sends sampled
Monitor successors through `transition_matches_step`. Add the nested-Option
case and exact concrete state assertions instead of inventing another partial
comparator.

This is an engine-agreement claim, not a `TriangulatedClaim` under
`docs/design/DESIGN-triangulated-assurance.md`. Monitor, Monitor BFS, and explicit all
share `fsl_runtime::eval`; the existing inventory correctly records only two
semantic lineages: concrete and symbolic. Therefore this work must not label
three process surfaces as three independent observers. If a future change
claims triangulation, it must separately preserve a raw observation, declare
two independent decision lineages, execute model-world/oracle-world/model-oracle
edges, and calibrate them as required by that accepted design.

### Relation endpoint expression boundary (#1458)

The bounded-scalar relation declaration gate also requires typed uses of its
endpoints. For `r: relation A -> B`, `r.contains(a,b)`, `r.add(a,b)`,
`r.remove(a,b)` and `reachable(r,a,b)` validate `a` against `A` and `b` against
`B` independently through the shared recursive expression checker. Enum types
remain nominal and existing integer/range assignability is unchanged.

This is a breaking removal of previously accepted ill-typed expressions, not
a runtime coercion or new relation operation. A mismatch retains the existing
assignability diagnostic and authored expression origin, appending
`hint: use endpoints matching the relation's declared source and target types`.
Native check and verify reject it with `kind: "semantics"`, exit 2, before
evaluation. LSP retains `FSL-SEMANTIC`; Worker preserves the native semantic
error envelope with Worker-owned delivery metadata. Public Kernel expression
validation uses the same gate without changing the schema. Native/Worker parity
is shared-checker evidence, not independent semantic triangulation.

The 15 endpoint detectors and 14 preservation/rejecting controls in
`issue_1458_relation_endpoint_types.rs` cover each method's two positions,
both reachable positions, heterogeneous source/target order, nested expressions,
and valid relation operations. A well-typed negated membership control must
still produce a real invariant violation, not a check error or false green.
Receiver/arity validation, relation functions on non-relations and the
self-relation requirement are separate #1459 signature work. No runtime or
verifier endpoint checks are introduced here.

For `r: relation A -> B`, `domain(r)` infers `Set<A>` and `range(r)` infers
`Set<B>`, not a universal `Set<Int>`. Binders over these projections inherit the
endpoint type, as do their public Kernel projection and binder annotations (the
schema is unchanged). Enum-typed projection uses are accepted, including
`not (exists x in domain(r): reachable(r, x, x))`, while closing the numeric
endpoint bypass through enum projection binders. Symbolic and concrete
projection semantics are unchanged; enum/Int confusion is rejected before
evaluation rather than interpreted through ordinals. Named numeric endpoints
remain integer-compatible. The fixed domain/range target has nine detectors
(three enum uses preserved relative to released main and six newly rejected
forms) and seven numeric preservation/rejecting controls. Existing non-relation
signature handling is outside this change (#1459).

The additional breaking removals for `s: relation Color -> Color` and
`r: relation N -> N` are:

| Inline fixture | Removed invariant expression |
|---|---|
| enum-domain-bypass | `forall x in domain(s): not r.contains(x, x)` |
| enum-range-bypass | `forall x in range(s): not r.contains(x, x)` |
| enum-domain-integer-member | `not domain(s).contains(0)` |
| enum-range-integer-member | `not range(s).contains(0)` |
| enum-domain-integer-comparison | `forall x in domain(s): x != 0` |
| enum-range-integer-comparison | `forall x in range(s): x != 0` |

#### Removed endpoint forms

These 17 inline occurrences (15 distinct sources) change from `ok` / 0 to
`error` / `semantics` / 2 in both modes in the final measurement below.
Here `N = 0..2` and `Color = {Red, Blue}`; endpoint errors retain invariant
origin 7:3 and the endpoint repair hint. Projection removals are listed above.

| Inline fixture label | Relation | Removed invariant expression |
|---|---|---|
| fixed-reachable-evaluated-enums | N -> N | `not reachable(r, Red, Blue)` |
| fixed-contains-evaluated-enums | N -> N | `not r.contains(Red, Blue)` |
| fixed-add-contains-evaluated-enums | N -> N | `r.add(Red, Blue).contains(Red, Blue)` |
| fixed-remove-evaluated-enums | N -> N | `r.remove(Red, Blue) == r` |
| fixed-reachable-source | N -> N | `not reachable(r, Red, 1)` |
| fixed-reachable-target | N -> N | `not reachable(r, 1, Blue)` |
| fixed-contains-source | N -> N | `not r.contains(Red, 1)` |
| fixed-contains-target | N -> N | `not r.contains(1, Blue)` |
| fixed-add-source | N -> N | `r.add(Red, 1) != r` |
| fixed-add-target | N -> N | `r.add(1, Blue) != r` |
| fixed-remove-source | N -> N | `r.remove(Red, 1) == r` |
| fixed-remove-target | N -> N | `r.remove(1, Blue) == r` |
| fixed-heterogeneous-contains-target | N -> Color | `not r.contains(1, 2)` |
| fixed-heterogeneous-add-target | N -> Color | `r.add(1, 2) != r` |
| fixed-heterogeneous-remove-target | N -> Color | `r.remove(1, 2) == r` |
| lsp-08 | N -> N | `not r.contains(1, Blue)` (duplicate occurrence) |
| worker-07 | N -> N | `not reachable(r, 1, Blue)` (duplicate occurrence) |

#### Restacked endpoint and projection boundary measurement

This is the historical #1458 measurement on the frozen `8e54e862` integration base:

- BASE: `8e54e862cb809121bcd33de53645795bf94a1df5`, materialized by
  `git archive` and built in that archive's own fresh, unpopulated target.
- Measured HEAD: `870d03e7aa0be837a43c4c5c15f10d32853941bb`; production
  commit `dba6456882a75ab34695767383ed86fccbc5ea38`. Subsequent commits
  change no `rust/*/src` files. The three fixed test files are byte-identical
  to `764a00c37750ef271a8407ca9785a5d27251406e`.
- All five original #1458 patches remain identical under `git range-diff`;
  the rebases had no conflicts. This records-only follow-up changes no source,
  test, corpus or CLI contract JSON.

On BASE `8e54e862`, both the workspace HEAD build
(`cargo build --manifest-path rust/Cargo.toml --workspace --locked --offline`)
and the fresh archived BASE build
(`cargo build -v --manifest-path rust/Cargo.toml --locked --offline -p fslc-rust --bin fslc`)
exit 0. Verbose compilation witnesses identify `fsl-syntax`, `fsl-core` and
`fslc-rust` under the base paths. Toolchain is 1.98.0, with four build jobs/test
threads and worktree-local TMPDIR. Each binary executes `fslc check FORM` in
its own tree. One lane-D-then-heavy lock acquisition encloses the **whole
sweep**, with no per-invocation locks. Each invocation waits for one-minute
load below 9. Stable mode removes `FSLC_UNSTABLE`; the other mode sets it to
`fsl5`; cache is disabled.

Binary-identity controls within the sweep on BASE `8e54e862` run the #1439 S1
`paint(if c != 1 then Red else Blue)` forbidden final-step form and the #1458
`not reachable(r, Red, Blue)` numeric-relation form, twice in both modes:
S1 is `error` / `semantics` / 2 on **both** binaries; the endpoint detector is
`ok` / 0 on BASE and `error` / `semantics` / 2 on HEAD (16 calibration checks).
Because #1439 is already in BASE, its removals and expectation-envelope
reclassifications are not inherited #1458 transitions in this measurement.

| Forms per tree | Count |
|---|---:|
| Every `.fsl` under `specs/`, `examples/`, `rust/fslc/tests/fixtures/` | 597 |
| All five `issue_1439_*.rs` files: expect/nested/scope/step/unary | 6 + 8 + 2 + 21 + 10 = 47 |
| Fixed #1458 endpoint and domain/range files | 35 + 16 = 51 |
| Fixed #1458 projection-acceptance file: four models, two refinement-input triples, two Kernel forms | 12 |
| Native/shared diagnostic step and parity fixtures | 2 + 8 = 10 |
| Worker parity, step and scenario-then fixtures | 7 + 2 + 1 = 10 |
| LSP stdio expectation and step fixtures | 2 + 2 = 4 |
| Total | **731 (597 corpus + 134 inline)** |

Written #1458 inline expansions are byte-checked against the actual fixtures
from the immutable tests; the two in-memory Kernel sources are mechanically
expanded from their test template. Each mode/tree runs twice: **5,848 checks**. Complete
stdout bytes, parsed JSON, stderr and exits repeat exactly; **no fields or forms
are excluded**. The **597-form corpus `ok` → `error` transition set is empty**,
with no other corpus output drift.

Within the original inline subset retained in this measurement, the observed
#1458 breaking removals are exactly the 15 endpoint source expressions in
the table above, the duplicate native target-only contains / Worker target-only
reachable occurrences, and all six enum projection expressions in the earlier
projection table: **23 occurrences, 21 distinct sources, 46 mode-form transitions**,
with exactly the same membership as the earlier inventory. The expanded third
fixed file additionally counts `xs = domain(s)` with `s: relation Color -> Color`
and `xs: Set<N>`: `ok` / 0 → `error` / `semantics` / 2 at assignment origin
8:19 in both modes. This is the already documented q01 family below, not a new
removal caused by the rebase. Totals are **24 occurrences, 22 distinct sources,
48 mode-form removals**.
All are `ok` / 0 → `error` / `semantics` / 2 in both modes. Endpoint and bypass
errors retain their repair hint; projection membership/comparison errors retain
the existing assignability message and authored origin without inventing a new
hint. The three legitimate enum projection idioms check on both BASE and final
HEAD; only the intermediate endpoint-only patch regressed them.

The third file also adds three counted accepting forms: `COUNT`, `SET_EQUALITY`
and `COLOR_ASSIGNMENT`, each `error` / `semantics` / 2 → `ok` / 0 in both modes
(six mode-form additions, not error-envelope reclassifications). Of **1,462
mode-form pairs**, **1,408 are entirely unchanged**. No further output changes
were observed. Standalone checks of refinement input components do not replace
the fixed tests' multi-file `refine` assertions.

The two new corpus fixtures are included without exclusions:
`rust/fslc/tests/fixtures/legacy_return_branches.fsl` is `ok` / 0 with
`implements.result: "refines"`; `legacy_return_impl_refines.fsl`, a standalone
refinement fragment, is `error` / `semantics` / 2, `spec has no state block`.
Both retain identical complete outputs on BASE and HEAD in both modes and
repetitions. All **5,832 shared observations** also match the earlier
`6e99702c05c92be39791ab1b0effe78f8ab0e60d` measurement in full; the 16
observations of the two added corpus forms are retained separately because
those paths did not exist in that base.

On BASE `8e54e862`, `cargo test --manifest-path rust/Cargo.toml --workspace --locked --offline`
exits 0: **3,063 passed, zero failed or ignored**. CLI conformance, the three
fixed targets (52 tests) and formatting also pass on that base. Public Kernel
controls require `rel_domain`/`rel_range` item and binder annotations for both
`N -> Color` and `Color -> N`; these are shared-checker controls, not independent
semantic triangulation or unbounded verification.

The production-src-only revert calibration was **measured on BASE
`6e99702c05c92be39791ab1b0effe78f8ab0e60d`**, producing **28 intended failures
and 24 preservation passes** (endpoint 15/14, domain/range 6/10, acceptance 7/0).
The three legitimate enum-binder/cycle idioms are preserved under this full
revert, while detecting a projection-only revert. Exact source/diff/status
restore was proved clean. Clippy was also **measured on 6e99702c**, exit 101
only for authorized #1444 (`claims_next_edition`, requirement_trace_large_domain:65).
Neither is relabeled or rerun on `8e54e862`: COMMON rev 4/4.1 carries them
across 26 changed main files, with zero production/golden/CLI-contract/lock
overlap and only the two corpus additions, both `A` in `git diff --name-status`.

#### Remeasurement after main's typecheck changes (`9af1fa5f`)

The current measurement uses BASE `9af1fa5f36f91e415b7ba249888f4ada4cf7a0db`
and measured HEAD `3a8a6019a5bed3ae62f960afedf2bdcf6d129341`. The six prior
commits were rebased from `8e54e862cb809121bcd33de53645795bf94a1df5`:
production is now `66727f6ead1da285f7c026de2dc20605cb2c2ae2`, with the five
later patches unchanged. Two conflicts in `typecheck.rs` retain main's
phase-aware recursive validation and the relation endpoint expectations; the
endpoint helper passes the current validation phase through rather than
resetting aggregate normalization or substituted pattern scopes. Main's #1442,
#1463 and #1448 checks remain intact. The only subsequent source change is the
whitespace-only `rel_domain`/`rel_range` arm formatting in the measured HEAD.
The complete file before and after that commit is equal after removing all
whitespace; the exact diff changes only that arm. No test was adapted. The
three fixed files remain byte-identical to `764a00c37750ef271a8407ca9785a5d27251406e`.

BASE was materialized with `git archive` into its own tree and compiled into a
fresh target with `cargo build -v --manifest-path rust/Cargo.toml --locked --offline -p fslc-rust --bin fslc`
(exit 0). Verbose output witnesses `fsl-syntax`, `fsl-core` and `fslc-rust`
compiling from that base tree. Each binary runs `fslc check FORM` in its own
materialized tree, enumerating every `.fsl` under `specs/`, `examples/` and
`rust/fslc/tests/fixtures/`, plus the inline inventory above. The corpus remains
**597 forms**, with ten modified sources since `8e54e862`: the seven injected
`return_system__*` fixtures, `examples/layers/return_impl_refines.fsl`,
`examples/layers/return_system.fsl`, and `examples/pm/cancel_system.fsl` (main's
canonical action-correspondence updates). BASE and HEAD corpus bytes match.
The same **134 inline forms** yield **731 forms per tree / 5,848 checks**, both
modes twice, with **no excluded fields or forms** and exact repeat agreement.
Binary-identity controls again pass all 16 expected accepting/rejecting checks.

The measured **597-form corpus `ok` → `error` transition set is empty** between
these named BASE and HEAD binaries, with no other corpus output drift. The
inline result remains **48 removals** (24 occurrences / 22 distinct sources)
and **six additions** (the three counted accepting forms, in both modes), with
**1,408 unchanged mode-form pairs** out of 1,462. Every removal/addition has
the same membership as the `8e54e862` measurement above, including the original
46-transition subset. No new #1458 breaking family is introduced by the rebase.

Full comparison with the old measurement observes exactly two changed forms
(12 observations, both modes and repetitions), all attributable to main's
#1463 in `28c4eab710f8d83e766bb0e7c7e006d09d7777d8`, not #1458:

| Form / file | Old to current observation | Scope / attribution |
|---|---|---|
| `forall i in s { m[i] = true }`, `rust/fslc/tests/fixtures/explicit_init_state_collection.fsl` | `ok` / 0 → `error` / `semantics` / 2, location 12:5, `init forall over a state collection is not supported; state variable 's' is not allowed` | Both BASE and HEAD; main moves the existing init collection restriction to model construction (8 observations). |
| `count(x in domain(s)) <= 1`, the `COUNT` model in `rust/fslc/tests/issue_1458_projection_acceptance.rs` | BASE remains `error` / `semantics` / 2; `invalid model expression: type is not a finite scalar` gains ` at 8:3` and `loc: {line: 8, column: 3}` | BASE only; main preserves the authored aggregate location (4 observations). HEAD remains accepted. |

No other complete stdout, parsed JSON, stderr or exit changes were observed
across the old and new measurements. In particular, the ten changed corpus
sources do not create output differences. The shared main rejection is not
counted as a #1458 corpus removal, and the located BASE count diagnostic does
not change its error-to-ok addition classification.

On BASE `9af1fa5f`, the production-source-only reverse calibration was freshly
measured at `3a8a6019`: the nonempty reverse patch names exactly
`rust/fsl-core/src/typecheck.rs` and `rust/fsl-wasm/src/lib.rs`, and **both files
equal their BASE blobs** before testing. The three fixed targets with
`--no-fail-fast` produce **28 intended failures / 24 preservation passes**:
endpoint 15/14, domain/range 6/10, acceptance 7/0, identical failing test
membership to the old `6e99702c` calibration. Exact file bytes, full diff and
clean status prove restoration; all 52 tests pass afterward. These are fresh
`9af1fa5f` measurements, unlike the explicitly carried historical calibration.

`cargo test --manifest-path rust/Cargo.toml --locked --offline --workspace --no-fail-fast`
passes **3,468 tests, zero failed or ignored**, measured on
`fc77124b31744f1c1728295db6f09508265b8285`. This result is carried across only
the proved whitespace-only commit to `3a8a6019`; it is not claimed as a rerun
there. On the latter HEAD, workspace build, CLI conformance (11), the three
fixed targets twice (52 each), formatting and
`cargo clippy --manifest-path rust/Cargo.toml --locked --offline --workspace --all-targets -- -D warnings`
all exit 0. Clippy is **warning-free on BASE `9af1fa5f`**: no #1444 exception
is carried forward. Main's changed #1448 tests (10), core tests (55), and
compose/quantifier parent targets (230, including support modules) also pass
on `fc77124b`. All checks use toolchain 1.98.0, four jobs/threads, offline
locked dependencies and worktree-local TMPDIR. Heavy jobs release their locks
between commands; after the scheduling amendment, subsequent heavy jobs yield
to other waiting lanes before reacquisition. One
whole-script acquisition encloses the sweep. These are bounded check/test
observations, not independent triangulation or unbounded verification.

#### Projection categories and final-review regression controls

The measured fixture lists above are **not an exhaustive description of the
language boundary**. Both each element and the whole set produced by `domain`
or `range` take the declared endpoint type. Consequently an enum or Bool
projection element is rejected anywhere `Int` is expected; its whole set is rejected
wherever a numeric Set is expected. The breaking removal includes
comparison, ordering, arithmetic, `min`/`max`, membership, set-literal equality,
Option comparisons and pattern payloads, numeric Set state assignment, and
refinement maps. This is shared type inference, not an allowlist of the six
projection examples or a new runtime conversion.

Newly accepted families, relative to released BASE `e8cf78f0`, include `count`
over finite endpoint-typed domain/range projections; numeric `sum`; enum
set-literal equality, enum-typed `where` filters and conditional projection
membership; `let`-bound projections consumed as matching typed sets; matching
enum Set state assignment; and refinement maps into matching enum Sets. These
forms still evaluate their authored properties: acceptance is not an invariant
truth claim. `unique` over enum domain/range is **preserved accepted behavior**,
not a new addition. Direct measurements override the review brief's initial
classification: its p28 is a `count` probe, and both `unique` supplements check
on BASE and HEAD and produce real step-2 violations on both engines. The three
enum-binder/cycle idioms a02/a03/a05 also check on released BASE and final HEAD;
only the unreleased intermediate endpoint-only patch rejected them.

The broader final-review probes below are **historical measurements on BASE
`e8cf78f0162530e264128f588981d49790f2767f` and production HEAD
`c11d5ed399807979c3bf89b07e06e4d2cbde9adc`**, not measurements on the final
integration base. They were independently run twice: 228 main observations and
76 supplements, both runs exiting 0. The supplement adds test sources but no
production change. Each direct call removes `FSLC_UNSTABLE`, disables cache,
and retains complete source, stdout, JSON, stderr and exit. An accepted check
is followed by
`verify FORM --engine bmc --depth 3 --no-cache` and the analogous explicit call;
maps use `refine IMPL ABS MAP --depth 3`. No field is excluded, and no
cross-engine envelope equality is claimed. The tables report both repetitions'
matching semantic outcomes.

Here E is check `error/semantics/2`, O is check `ok/0`, and Xn is a real
invariant violation at step n, exit 1, on **both** BMC and explicit. All ordinary
HEAD rejection origins below are invariant 8:3 unless stated otherwise.

| Newly rejected probe | Authored form / context | BASE check | HEAD check / origin |
|---|---|---|---|
| q01 | `xs = domain(s)`, `s: relation Color -> Color`, `xs: Set<N>` | O | E, assignment 8:19 |
| p14 | `forall x in domain(s) { x + 1 != 0 }` | O | E |
| p16 | `forall x in domain(s) { min(x, 1) != 0 }` | O | E |
| max supplement | same form with `max(x, 1)` | O | E, origin 8:3 |
| q14 | `not domain(b).contains(1)`, `b: relation Bool -> Color` | O | E, 6:3 |
| p06 | `domain(s) != Set { 0 }` | O | E |
| p07 | `Set { 0 } != domain(s)` | O | E |
| p18 | `forall x in domain(s) { some(0) != some(x) }` | O | E |
| u04 | `forall x in domain(s) { o != some(x) }`, `o: Option<N>` | O | E |
| p17 | `forall x in domain(s) { (some(x) is some(y)) => y != 0 }` | O | E |
| p11 | `forall y in range(h) { y < 1 }`, `h: relation N -> Color` | O | E |
| numeric refinement map | abstract `xs: Set<N>`, `map xs = domain(s)` | type validation accepts; simulation returns `refinement_failed/abs_state_mismatch/1` | `error/type/2` before simulation, existing unknown-span origin 1:1 |

| Newly accepted probe | Authored form / context | BASE | HEAD check / BMC / explicit |
|---|---|---|---|
| p01 | `count(x in domain(s)) <= 1` | E | O / X2 / X2 |
| p02 | `sum(x in domain(r) of x) <= 1`, numeric r | E | O / X1 / X1 |
| range-sum supplement | same sum over `range(r)` | E | O / X1 / X1 |
| p28 | `count(x in range(r) where x > 0) <= 1`, numeric r | E | O / X2 / X2 |
| p05 | `domain(s) == Set {} or domain(s) == Set { Red }` | E | O / X1 / X1 |
| p21 | `not (exists x in domain(s) where x != Red { s.contains(x, Red) })` | E | O / X1 / X1 |
| p23 | `not (if s.contains(Red, Red) then domain(s) else range(s)).contains(Blue)` | E | O / X1 / X1 |
| q07 | `let d = domain(s)`, `requires d.contains(Red)`, then n becomes 1 | E | O / X2 / X2, invariant 12:3 |
| q02 | `xs = domain(s)`, `xs: Set<Color>` | E | O / X2 / X2 |
| enum refinement map | abstract `xs: Set<Color>`, `map xs = domain(s)`, `links(a,b) -> mark(a)` | `error/type/2` | `refines/0`, checked to depth 3, action map links to mark |

All newly accepted ordinary probes above retain invariant origin 8:3 except
q07. Successful refinement has no diagnostic origin; type errors for map
expressions currently use 1:1 rather than the authored map keyword. Neither
origin limitation is changed or disguised as an authored-span improvement.

Public Kernel precision here is limited to the **projection nodes and binders
over those projections**. Numeric endpoint literals in relation `contains`,
`add`, `remove`, and `reachable` arguments still export as `int`, rather than
the named numeric endpoint type: this is a pre-existing annotation limitation
(`public_kernel.rs` method argument projection and ternary argument projection).
Probe k01 remains accepted on BASE and HEAD, with numeric relation arguments
exported as `int`; no `public_kernel.rs` change is part of this fix.

`issue_1458_projection_acceptance.rs` commits seven controls: three require real
violations on both engines with authored invariant origins; one requires
refinement into `Set<Color>`; two reject numeric Set assignment/refinement
before evaluation/simulation; and one asserts exact public Kernel projection
item **and binder** types for both `N -> Color` and `Color -> N`. The Kernel
fixture uses binder self-comparison so its revert failure is an `int` annotation,
not an incidental model-loader rejection. Reversing only the domain/range hunk
of `c11d5ed3`, measured on historical BASE `e8cf78f0`, produces **seven intended
failures / zero passes**; exact checker bytes and diff are restored, then all
three #1458 targets pass twice (52 tests per run). The two fixed files remain
unchanged. In that historical round on BASE `e8cf78f0`, formatting passes and
CLI all-target Clippy retains only authorized #1444.

These historical probes are not retroactively added to the final sweep. The
final measurement above explicitly includes all forms of the third fixed file;
the broader probe tables retain their original base and head identities.

Out-of-scope leftovers are explicitly retained: cross-range numeric Set coercion
(u01/u02/q03) checks on both binaries but BMC reports a type/coercion error while
explicit returns a violation; relation indexing `h[x]` (q17) checks on both but
both engines report errors. Both are routed to the representative as new issue
candidates, not fixed or covered by an unchanged-runtime agreement claim here.
At that checkpoint `abs` on an enum projection element (p15) checked but both
engines errored on both binaries; it and the non-relation `domain` fallback to
`Set<Int>` were deferred to #1459, resolved by the amendment below.

### Static operand and signature boundary (#1459)

This amendment closes six remaining shared-checker gaps. It changes neither
grammar/lowering nor runtime/verifier semantics. Invalid authored expressions
now fail during model loading, with `kind: semantics`, exit 2, and the existing
authored expression origin; generated aggregate terms must not replace that
origin with 1:1. These checks cover expression trees visited by the shared
validator: unselected conditional branches, initializers, action assignments,
guard/let values and ensures, properties, leadsTo predicates/ranking, terminal
predicates, and trace expression internals. Parameter-type checking of helpful
action arguments was outside the historical baseline; main's #1527 now provides
it and is preserved. Outer trace argument kinds retain their existing replay contract.

| Breaking removal category | Previously accepted examples now rejected |
|---|---|
| Non-Option patterns | `n is none`, `n is some(x)` for numeric `n` |
| Non-integer abs operands | `abs(Red)`, `abs(true)`, `abs(x)` for an enum domain/range binder |
| Non-relation functions/projections | `acyclic(n)`, `functional(n)`, `injective(n)`, `domain(n)`, `range(n)`, `reachable(n,0,1)`; no invented `Set<Int>` fallback |
| Invalid path/cycle relation shape | `acyclic(h)` and `reachable(h,a,b)` on mixed enum/numeric endpoints or `N -> M`, even with identical numeric bounds |
| Invalid collection receiver/name/arity | numeric receivers; Set.push, Seq.add, relation.size; missing/extra Set/Seq arguments; relation endpoint methods without exactly two arguments; action assignment `n = q.at()` |
| Erased empty-aggregate operands | `count(i in 1..0 where 1)` / `where not 1`, `sum(i in 1..0 of true)` / `of -Red`, and corresponding zero-capacity Seq forms |

The method gate matches existing runtime dispatch: Set contains/add/remove take
one item, size takes none; Seq contains/push take one item, at takes one integer
index, pop/head/size take none; relation contains/add/remove take exactly two
typed endpoints. Map has no methods. No new runtime operation is introduced.
Original count/sum binders, Bool filters and integer sum values are validated
before normalization; well-typed empty domains keep their zero identities.

Diagnostics reuse domain-lowering Option and method wording, ordinary Int/Bool
assignability, public relation-function names and repair hints. Existing #1458
endpoint mismatches/hints remain unchanged. Heterogeneous non-path relation
functions, valid methods, Option continuation bindings and non-reflexive
reachability retain their meaning. Additive native/shared-LSP and Worker rows
assert located Option errors rather than merely a runtime exit code.

The existing verifier test `acyclic_on_a_non_self_relation_is_rejected` keeps
its name and intent. It now asserts the earlier model-build rejection of its
unchanged `relation User -> Role` source. It also retains the verifier-level
`verify_bounded` rejection by mutating a public checked-model expression from
functional to acyclic, following the existing #1259 unchecked-input test pattern.
No unchecked builder API, product bypass or accepted-source exception is added.

Self-relation means exact declared endpoint `TypeRef` identity for both
`reachable` and `acyclic`, matching symbolic evaluation: `T -> T`, not merely
numeric assignability. Thus `type N = 0..2` and `type M = 0..2` still make
`relation N -> M` invalid for these two functions. Heterogeneous non-path
functions and endpoint methods remain valid. This is an explicit breaking
removal, not a new enum/numeric encoding or runtime conversion.

#### Three-tree boundary audit (#1459)

The released BASE is `e8cf78f0162530e264128f588981d49790f2767f`, the immediate
parent is `764a00c37750ef271a8407ca9785a5d27251406e`, and the implementation
HEAD is `33d7faa7dc034a7c8b484d59c40c0e78a45607ca`. Both baselines were freshly
materialized with `git archive` inside the task worktree, each starting without
a target directory. Each tree built its own CLI with
`cargo build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --locked --offline -v`.
Verbose logs show `fsl-syntax`, `fsl-core` and `fslc-rust` compiling from that
tree's own paths, not a cloned target or another worktree's binary.

| State | CLI binary SHA-256 | Checker source SHA-256 |
|---|---|---|
| BASE `e8cf78f0` | `218301707985a399d4cdfa722b5033950298ce5cb8565f12c01ad541e022c89c` | `a7d9fa098655a325fe184cc6d849fb37d6dbfc32288dc66e9436c026447ebe9b` |
| Parent `764a00c3` | `086b0db01caaee83a0bd102b2519dc9fbda88dd874a7f75f86d8110101b5c098` | `61dcd7feb19f120b53f3df6fc3cde8240a9a86f345462075f0a7c32b8e6888b8` |
| HEAD `33d7faa7` | `649ee0617acebb01716cfc9f940289f3643e07b73d16e6c9942d907553956661` | `08ac6ec27e100b7b6832aabac302137be071b88ca847a523bcbc4e984c66078e` |

Recount command:
`rg --files -0 specs examples rust/fslc/tests/fixtures -g '*.fsl'`.
An independent recursive inventory agrees, and all corpus paths and source
bytes agree across the three trees. The **591 corpus files** and **384 inline
occurrences** give **975 forms per tree**. Inline coverage includes the original
122 occurrences, 12 forms from the seven final #1458 tests, two new consumer
rows, two forms for the adapted verifier test, all 228 current operand fixtures,
17 statement fixtures, and the original combined numeric-policy fixture from
the formerly ignored test. The historical 122 source hashes are unchanged.

Each binary ran `check FORM` in its own tree, serially, in both modes twice:

```text
env -u FSLC_UNSTABLE FSLC_CACHE=off BINARY check FORM
env FSLC_UNSTABLE=fsl5 FSLC_CACHE=off BINARY check FORM
```

One outer lane-D-then-heavy advisory lock covered calibration, all checks,
comparison and audit; per-binary locks were removed before measurement. Uptime
was checked **inside** those locks before every binary, pausing at one-minute
load ≥9. Observed sweep loads were 1.63–2.43. Whole-wrapper exit was **0**,
wall time **754.44 s including lock wait / 272.20 s inside the locks**.
There are **11,700 sweep checks**, plus 12 calibration calls: the Option detector
checks on both bases and rejects on HEAD, while the #1458 endpoint detector
checks on BASE but rejects on parent and HEAD, each twice.

All **5,850 repeat pairs** agree in full stdout, JSON, stderr and exit. There
are no excluded fields or omitted forms. The **corpus ok→error set is empty**
for BASE→HEAD and parent→HEAD; there is also **no other corpus output drift**.
These are measured results for the named commits and commands, not an
assumption from the narrower native corpus gate.

| New #1459 category | Removed source occurrences | Removed mode-forms | Diagnostic reclassifications (mode-forms) |
|---|---:|---:|---:|
| Option operand shape | 9 | 18 | 8 |
| Integer abs operands | 12 | 24 | 16 |
| Relation operands/projections | 15 | 30 | 24 |
| Exact self-relation identity | 20 | 40 | 0 |
| Method receiver/name/arity | 44 | 88 | 12 |
| Original empty count/sum operands | 24 | 48 | 0 |
| **Total** | **124** | **248** | **60** |

The removals comprise **123 distinct source texts** (the new native/Worker
Option rows duplicate one text). Numeric `N -> M` accounts for **nine source
occurrences / 18 mode-form removals**: eight final reachable/acyclic position
fixtures plus the original combined fixture. The adapted verifier's unchanged
`User -> Role` acyclic source contributes two mode-form removals. All 60 new
diagnostic reclassifications remain `error/semantics/2`: formerly unlocated
runtime errors now have the authored static origin and approved class text.

Attribution uses BASE→parent for inherited work and parent→HEAD for #1459:

| Issue in the expanded inventory | Removals | Diagnostic reclassifications | Additions |
|---|---:|---:|---:|
| Inherited #1439 | 86 | 11 | 0 |
| Inherited #1458 | 56 | 0 | 6 |
| New #1459 | 248 | 60 | 0 |

All entries count mode-forms. The original 122 occurrences reproduce exactly
the prior 70 #1439 removals, three #1439 reclassifications and 46 #1458 removals.
The expanded controls add 16 inherited unary-filter removals and eight
head/at trace-location reclassifications; eight endpoint-calibration removals
and two numeric-Set assignment removals remain #1458, not new #1459 behavior.
The six additions are the two-mode count, enum set-literal equality and enum
Set assignment forms added by #1458. Direct BASE→HEAD comparison
has **390 removals, 71 diagnostic reclassifications, six additions and 1,483
fully unchanged pairs**, out of 1,950 mode-form pairs.

Final regression calibration: the fixed operand target passes **28 tests twice**,
with no ignore. Reversing only the production checker change produces the
expected **18 original-reason detector failures and ten control passes**
(14 false-green check tuples, four missing-static-diagnostic failures); exact
checker/diff and fixed-test hashes are restored. The statement target passes
**17 tests twice**; its production-only revert produces exactly **16 intended
check false-green failures and one valid-body control pass**. All six scoped
package suites pass, native LSP/Worker rows run twice, formatting passes, and
final scoped Clippy has only the separately authorized #1444 dead-code warning.
This is source-boundary and routing evidence, not independent semantic
triangulation or an unbounded-verification claim.

#### Final review round 1 (#1459)

Review of `33d7faa7` found that exact endpoint identity was still checked only
on an operand's inferred type. Conditional inference uses the then branch,
and ordinary numeric compatibility permitted a non-self else branch to pass.
The self-relation check now visits both branches, including nesting and
unselected branches, through `old` and relation add/remove receivers. Action
lets retain a separate private identity guarantee computed before each binding,
so two-step aliases cannot discard a non-self selection. This does not alter
ordinary numeric or Set compatibility, public type environments or model IR.

All errors leaving recursive expression validation acquire the current authored
span through the existing insert-if-absent rule; precise inner spans are kept.
Equality inference distinguishes context-needed literal failures structurally,
so an empty literal cannot mask a substantive domain/range operand error.
Range-bound assignability does not manufacture a 1:1 span: the enclosing binder
supplies the authored origin. Terminal validation uses its stored declaration
span. Synthetic aggregate/partial-path spans and legacy APIs without a supplied
source span are not reclassified as authored positions.

The inference call-site audit covers the original 20 typecheck calls, three
post-validation Public Kernel projection calls and two partial-operation calls
that consume inference errors. Recursive type inference, index/method helpers,
binary operand inference, pattern extension and aggregate normalization now
leave validation through the common authored boundary. `expression_type` and
assignability already attach caller spans. The former repeated raw let inference
is span-bound; the three new branch/receiver inferences are covered by the same
boundary or the action-span mapping during let capture. Public projection runs
only after full expression/model validation; its raw calls cannot be the first
diagnostic for invalid authored input. Partial-operation classification consumes
its two errors rather than returning them as diagnostics.

The `unknown_span` inventory identifies one authored-model use in the model
expression validator: terminal, now replaced by `terminal_span`. The two range
bound calls are also fixed. Remaining uses are generated aggregate conditionals,
generated quantified partial paths, source-span-less legacy expression APIs,
and declaration-annotation fallbacks (the no-target case is unused). KPI loading
explicitly restores the projection declaration origin. A source-less programmatic
model without a terminal span retains the legacy fallback; an authored terminal
does not use it.

Two scope clarifications remain explicit. At the historical round-1 baseline,
`helpful` actions checked existence/arity only; main's #1527 now checks parameter
types and is preserved during integration. Relation-valued temporary collection
literals are not generalized by the self-relation provenance fix: a mixed
relation sequence can check while both engines reject it because such sequences
are unsupported. That separate constructor-acceptance gap is likewise routed
upstream. Neither is evidence for claiming all language expression positions
or temporary constructors are supported.

Round-1 implementation HEAD is `8caf5c2ed9e1aa15238dcde1b3a66bb4ccf9c857`.
The primary BASE/parent/HEAD comparison remains `e8cf78f0` / `764a00c3` / this
HEAD. A **fourth own-tree baseline** was freshly materialized with
`git archive 33d7faa7` and built from its own source paths, starting without a
target directory. It was not a copy of the old working-tree executable.

| R1 state | CLI binary SHA-256 | Checker source SHA-256 |
|---|---|---|
| R0 `33d7faa7` own-tree baseline | `365a2474809cbf2bb773ace0c2fb23b25e5b301d08f977feec8d86dbb0ed1cd7` | `08ac6ec27e100b7b6832aabac302137be071b88ca847a523bcbc4e984c66078e` |
| R1 HEAD `8caf5c2e` | `d04df10f36e1140ba18e22d8d7e6b32a5b33987ddb97daf0c2d01456d9533b38` | `5f80538cfaac2b261068ca8cb22037a0d3278ad9fd4c27bae83d7a6c140c4411` |

The earlier two own-tree baselines retain their recorded binary/source hashes.
The census command and cache/feature-mode `check FORM` commands are the same as
above. All **591 corpus files** and the original **384 inline occurrences**
retain their source hashes; 46 review fixtures and 17 supplementary path/origin
fixtures add **63 forms**, yielding **447 inline / 1,038 total forms per tree**.
The primary three-tree measurement has **12,456 checks**, with **4,152 additional
33d checks** for attribution: **16,608 total**, stable/fsl5 twice in all four
trees. Twenty-four calibration calls prove the original Option/endpoint gates
and, separately, the conditional-self detector (accepted by `33d7faa7`, rejected
by R1), each twice per state.

Fresh-baseline/head builds, calibration, all four measurement passes, comparison
and audit ran under **one lane-D-then-heavy acquisition**. Before every Cargo
start/binary, uptime was checked inside the locks and load ≥9 paused the run.
Observed measurement load was 1.46–3.95. Wrapper exit is **0**; wall time is
**613.81 s including lock wait / 613.71 s inside locks including the fresh build**.
All **8,304 repeat pairs** agree in full stdout/JSON/stderr/exit. There are **no
excluded forms or fields**, no corpus ok→error transitions, and no other corpus
output drift in any comparison.

| R1-only change, `33d7faa7` → `8caf5c2e` | Source occurrences | Mode-forms |
|---|---:|---:|
| Non-self conditional selections through supported wrappers/aliases, now rejected | 21 | 42 removals |
| Projection inference origin/primary-diagnostic correction | 8 | 16 reclassifications |
| Range-bound generated 1:1 → authored origin | 3 | 6 reclassifications |
| Terminal generated 1:1 → authored origin | 1 | 2 reclassifications |

Thus R1 adds **42 removals and 24 diagnostic reclassifications**, no additions,
with **2,010 unchanged pairs** out of 2,076. Every reclassification remains
`error/semantics/2`: eight source forms formerly had no location and four had
generated 1:1; all now have the expected authored location. Two of the eight
also replace a context-needed empty-literal error with the substantive
domain/range error. The precise-inner-origin control remains unchanged.

| Full expanded comparison | Removed mode-forms | Diagnostic reclassifications | Additions | Unchanged |
|---|---:|---:|---:|---:|
| BASE → parent | 142 | 11 | 8 | 1,915 |
| Parent → R0 `33d7faa7` | 274 | 64 | 0 | 1,738 |
| R0 → R1 | 42 | 24 | 0 | 2,010 |
| Parent → R1 | 316 | 66 | 0 | 1,694 |
| BASE → R1 | 458 | 77 | 8 | 1,533 |

The larger fixture set does not retroactively change the earlier 975-form audit.
Inherited #1439 still contributes 86 removals/11 reclassifications; inherited
#1458 contributes 56 removals and now eight additions (including the additional
enum-projection equality control). The parent→R0 edge adds 26 removals and four
diagnostic changes on new fixtures to the earlier 248/60 counts. The latter are
the two-mode Boolean/projection aggregate-range-bound forms: the parent's static
range-bound error becomes the R0 specific operand error at 1:1, then R1 attaches
the authored origin. Do not add edge reclassification counts blindly: a form
already removed by R0 can have a further diagnostic correction in R1.

Final-code V1 runs all four immutable targets twice: **28 + 17 + 30 + 13 = 88
tests**, none ignored. Reverting only R1 production to exact `33d7faa7` behavior
produces **33 intended detector failures and 55 passes**, including all 45 earlier
tests and ten new controls. Produced reasons are 21 false-green check tuples,
six missing locations, four generated-1:1 locations and two masked substantive
errors; exact checker/diff and all four hashes are restored. Requested R1 scoped
packages pass **2,414 tests**, formatting passes, and scoped Clippy retains only
the separately authorized #1444. No new allowance or existing-test rewrite is
used. These are bounded source/type and routing checks, not an assurance upgrade.

#### Integration audit carried to main `11f01225` (#1459)

Final base is **`11f01225e244dabde0f8dfd9aaa665bc47dccfd4`**. Implementation
was measured at **`2b7d2e98349505af7dccb7843e41ab659066f021`**, on
`b1557a31b77f549d2c48823c3435217f9df4fc53`, and carried to `11f01225`, whose
tree differs only by `docs/intro/language.en.html` and `language.ja.html`.
#1458 is already on main, not a separate integration parent. Its historical
attribution is still measured by the `9af1fa5f36f91e415b7ba249888f4ada4cf7a0db`
→`b1557a31` split; the latter's code was built at
`3a8a6019a5bed3ae62f960afedf2bdcf6d129341` and is identical to the final base.

Every `.fsl` under `specs/`, `examples/`, and `rust/fslc/tests/fixtures/` is
included: **597 corpus inputs**, plus **601 inline inputs**. The inline inventory
retains the five #1459 fixed files, three #1458 fixed files and rewritten Kernel
fixtures, and adds every source from the **137 shared #1463 rows**, including
their one resolver dependency. Each invocation is `fslc check FORM`, cache off,
with `FSLC_UNSTABLE` unset and set to `fsl5`, each repeated twice in independently
materialized states. **1,198 inputs/state**, **14,376 primary checks**, **24
identity calibrations**, and **7,188 full-output repeat pairs**. All repeat pairs
agree literally. No corpus output changes and no form or output-field exclusions.

| Full expanded comparison | Removals | Diagnostic reclassifications | Additions | Unchanged mode-forms |
|---|---:|---:|---:|---:|
| Historical pre-#1458 `9af1fa5f` → measured baseline carried to `11f01225` | 56 | 0 | 8 | 2,332 |
| Final base `11f01225` → measured #1459 implementation | 294 | 62 | 0 | 2,040 |
| Historical `9af1fa5f` → measured #1459 implementation | 350 | 62 | 8 | 1,976 |

The #1459 removals are **147 source occurrences / 142 distinct source texts**;
the 62 reclassifications are 31 occurrences. The historical #1458 134-entry
subset remains **48 removals / six additions**. Expanded fixtures contribute
eight additional inherited endpoint removals and two inherited projection-equality
additions; do not attribute these to #1459.

The historical 316/66 #1459 comparison above changes because main #1463
(`28c4eab710f8d83e766bb0e7c7e006d09d7777d8`) already validates original
aggregate operands and authored bounds. These 24 occurrences no longer contribute
48 #1459 removals: `empty-count-{0,1}-{A,D,F,I}`,
`empty-sum-{0,1}-{A,D,F,I}`, `empty-seq-count-{A,D,F,I}`, and
`empty-seq-sum-{A,D,F,I}` in the operand fixture inventory. Here A/D/F/I denote
acceptance/dead branch/forbidden/invariant positions. The two `range-bool` and
`range-enum` path occurrences no longer contribute four reclassifications:
base and head already agree on their located semantics errors. All other retained
transition members are unchanged. Added occurrences contribute 16 let-shadowing,
four public-Kernel, four CLI-Kernel, and two shared-row removal mode-forms:
**316−48+26=294**, **66−4=62**. Historical measurements are not relabeled.

The breaking shared-row removal is **`X1/operand-method-types`**:
`invariant Capture { acyclic(done) }` with Bool `done`. Its recorded baseline
stays `[Ok, Error, Error, Error]`; expected outcomes become
`[Error, Error, Error, Error]` in check/BMC/induction/explicit order, with check
semantics rejection at **8:3**. It is the only changed shared row. For the other
136 primary sources and one resolver dependency (**137 inputs**), the same
`fslc check FORM` command at the named base and implementation SHAs yields an
empty transition set and no other output change, in both modes twice.

The full inherited comparison preserves **5,848 raw observations**: **292**
differences are solely the input path embedded in `message` (exit, location,
other JSON fields and stderr unchanged). Another **16** observations concern
two inherited entries, `1458-acceptance-kernel-N-Color` and
`1458-acceptance-kernel-Color-N`. The previous extractor selected scratch labels
`kernel-N-Color` / `kernel-Color-N`, not the spec template at
`issue_1458_projection_acceptance.rs:270`; both sides therefore returned parse
errors. This sweep uses the actual `spec ProjectionKernel` test sources, which
check on both baseline binaries. There is no transition or attribution change.
Raw differences remain recorded; no field is normalized/excluded in the sweep.
The optional canonical-path replay was explicitly **not run**. The #1458-side
extraction defect is reported upstream separately.

Coverage adaptations preserve guards rather than bypass them. The existing
relation_operations verifier test still reaches its low-level rejection through
an edited checked model. The #1464 public-Kernel missing-at-index test is split
into `check_rejects_at_without_an_index`,
`walk_partial_rejects_an_at_call_without_an_index`, and
`public_kernel_rejects_an_at_call_without_an_index_without_panicking`. The latter
two start with accepted `at(0)`, remove the index in the model AST, and separately
exercise the projection guard and public checked-model validation. The renamed
CLI `empty_at_index_is_rejected_by_check_and_kernel` stops at check for both
commands and pins their complete observed envelopes; it no longer claims to
exercise the projection guard. Its unchanged valid-index control still exports
the partial operation. The X1 expectation change follows upstream approval and
the explicit #1459 ownership in DESIGN-check-executable-forms.

All five immutable #1459 files pass 98 tests twice; #1458's three files pass 52;
Kernel passes 24 twice and public Kernel 10 twice. Full workspace rerun passes
**3,568 / 0 failed / 0 ignored**; workspace build, Clippy and fmt pass. Reverting
only typecheck.rs and compose.rs to exact `b1557a31` gives **75 intended failures
/ 60 preservation passes** across 135 tests, including X1 and renamed Kernel
check-step detectors; three original aggregate tests now remain green because
main already closes those holes, so they are preservation controls on this base.
Direct `args[0]` calibration panics in the walk guard test; disabling public
checked-model validation fails with the index-guard message instead of at/0.
Each mutation is restored byte-for-byte with an empty full diff, then the head
is rebuilt. These checks establish bounded typing/routing evidence, not an
assurance upgrade or independent triangulation.

## 5. Lossless public observation and replay

Full language support is incomplete if engines distinguish `none` from
`some(none)` internally but ordinary traces, generated harnesses, or replay
collapse them. Preserve the legacy `null`/value bytes for `Option<scalar>` and
introduce a tag only when the declared payload is itself an Option:

Public Kernel projection (`expr_json`): an accepted empty relation initializer
is exported as `kind="set_lit"` with `type` naming the relation endpoints and
`items` as an empty array. The closed public Kernel schema introduces no new
expression `kind`; there is no `relation_lit` variant.

| Type and value | Canonical ordinary JSON |
|---|---|
| `Option<T> = none` | `null` |
| `Option<T> = some(1)` | `1` |
| `Option<Option<T>> = none` | `null` |
| `Option<Option<T>> = some(none)` | `{"kind":"some","value":null}` |
| `Option<Option<T>> = some(some(1))` | `{"kind":"some","value":1}` |
| `Option<Option<Option<T>>> = some(some(none))` | `{"kind":"some","value":{"kind":"some","value":null}}` |

The canonical typed rule is:

```text
encode(Option<U>, None)       = null
encode(Option<U>, Some(v))    =
  {"kind":"some","value":encode(U,v)}  if U resolves to Option<_>
  encode(U,v)                               otherwise
```

The implementation may retain the current untyped `fsl_value_json` API by
recognizing a concrete `Some` whose payload is `None|Some`, because unsupported
`Option<struct/collection>` is removed by the state gate. Decoding must remain
type-directed. For expected `Option<Option<_>>`, accept only `null` or the exact
closed `{kind:"some", value:...}` object and recurse; reject missing/extra keys
and a noncanonical untagged non-null value. For expected `Option<scalar>`, keep
the legacy `null`/value decoder.

Do not detect an Option tag by object field names while recursively diffing
JSON: a legal struct can itself have `kind` and `value` fields. Compute trace
changes from typed `FslValue`/`TypeRef`, or mark Option values atomically before
generic object descent. The expected change path remains the logical state path
`x`, not `x[kind]` or `x[value]`. `state_summary` must render nested values as
`some(none)` / `some(some(1))`, not `null`.

The existing conformance-vector format remains fully tagged at every Option
layer (`{"kind":"none"}` / `{"kind":"some","value":...}`); it is a separate
versioned contract and must not be changed to the hybrid ordinary form.
Public-Kernel type JSON is already recursively typed and requires no schema
major bump. Replay/testgen state schemas already admit arbitrary state-value
JSON objects, so their schema identifiers remain unchanged; their design docs
must define the newly canonical nested-Option value. Add schema/golden tests to
prove that this is an additive representation for a newly supported type and
that every previously supported `Option<scalar>` byte stays unchanged.

## 6. Coupled-surface inventory (25 surfaces)

“No change” means the surface was inspected and has an executable preservation
control; it does not mean it may be omitted from review.

| # | Surface | Current disposition and required change |
|---:|---|---|
| 1 | Grammar and surface AST (`rust/fsl-syntax`) | Recursive already; no production/variant change. Add depth-3 parse/Kernel-AST preservation. |
| 2 | Type resolution/lowering (`fsl-core::resolve_type`) | Recursive already; no representation change. Verify direct and dialect-lowered models pass the same gate. |
| 3 | Typed model/defaults/state-shape gate (`rust/fsl-core/src/model.rs`) | Add the exhaustive `legal_state` gate and reuse `optional_scalar` for struct fields. Preserve recursive defaults. |
| 4 | Typechecking/pattern binding (`rust/fsl-core/src/typecheck.rs`) | Preserve recursive expected typing and `is some(v)` binding (`v` has the inner Option type). Share target-type lookup with verifier. |
| 5 | Symbolic expression semantics (`rust/fsl-verifier/src/eval.rs`) | Generalize contextual evaluation for Option, struct fields, and conditionals. |
| 6 | Symbolic init/action transitions (`rust/fsl-verifier/src/transition.rs`) | Evaluate every assignment RHS against its target type before assignment/coercion. |
| 7 | Symbolic bounds, induction, refinement, and trace projection | Existing recursion is preserved; add nested bounded-domain and refinement/trace controls so a sibling walker cannot stay hollow. |
| 8 | Concrete Monitor (`rust/fsl-runtime/src/lib.rs`) | No representation change; add exact nested-value, pattern-binding, and assignment controls. |
| 9 | Explicit BFS (`rust/fsl-runtime/src/explicit.rs`) | No alternate semantics; add exact successor/closure/witness control and wiring mutation. |
| 10 | Public Kernel AST/type export and v1/v2 schemas | Recursive `option.item` already exists; preserve golden bytes for prior types and add nested recursive schema validation. |
| 11 | Conformance vectors and coverage (`rust/fslc/src/lib.rs`, `coverage.rs`) | Keep fully tagged form; strengthen current broad `kind == some` assertion to exact `some(none)` and `some(some(1))`. |
| 12 | Ordinary value/state/trace JSON, summaries, and changes (`fsl-core::trace_json`) | Add the canonical hybrid nested tag, type-safe diff behavior, and exact round trips. |
| 13 | Replay, testgen trace, and generated target adapters (`rust/fslc`) | Extend type-directed snapshot decode; ensure generators compare the nested object without flattening. Update replay/kernel contract docs. |
| 14 | Native CLI diagnostics/envelope/exit | Add `type` classification and hint for the centralized gate; legal verification must no longer expose the unlocated verifier error. |
| 15 | Browser Worker (`rust/fsl-wasm`) | Shared verifier/output should inherit behavior; add native/Worker JSON parity and nested trace rendering controls. No native-Z3 dependency. |
| 16 | Regression/agreement/fault-operator infrastructure | Add focused three-way cases, rejecting matrix, exact comparator checks, and registered implementation fault operators. |
| 17 | `docs/manual/LANGUAGE.md` | Replace `Option<scalar>` with recursively defined optional scalar; remove nested Option from the prohibition while retaining all other exclusions. Record the nested Map value removal as a breaking change (done in PR #923). |
| 18 | `docs/manual/LANGUAGE.ja.md` | Make the section-aligned 1:1 Japanese change; retain the same 18 `##` sections in the same order. Record the nested Map value removal as a breaking change (done in PR #923). |
| 19 | Generated site references (`docs/intro/language.{en,ja}.html`) | Regenerate with `tools/build_site_reference.py`; freshness CI remains the authority for generated bytes. |
| 20 | `skills/fsl/references/syntax.md` | Update the type table, struct note, and whitelist to `OptionalScalar`; state the unsupported wrappers explicitly. State that `Map<_, Map<_, _>>` is rejected and why (done in PR #923). |
| 21 | Accepted design notes | Add `docs/design/DESIGN-nested-option-support.md`; mark only the nested-Option rejection sentence in `docs/design/DESIGN-option-struct.md:10-13` as superseded, retaining its v2.1 history. Update replay/kernel design notes for the ordinary/tagged boundary. |
| 22 | `changelog.d/` | Add `changelog.d/841-nested-option-typed-boundary.added.md`, `changelog.d/841-nested-map-state-boundary.changed.md`, `changelog.d/841-nested-option-lossless-json.fixed.md`, `changelog.d/841-nested-option-cross-engine-controls.required.md`, and `changelog.d/841-nested-option-support.documented.md`; do not edit `CHANGELOG.md`. Any matrix row that changes the verdict of an existing accepted form additionally requires a `changed` fragment. |
| 23 | LSP (`rust/fsl-lsp/src/index.rs` and diagnostic tests) | No new declaration, binder, or reference form: `Option` is already a keyword and nested `some` uses the existing pattern binder. No index code change; update CLI/LSP diagnostic identity tests for rejected shapes. |
| 24 | Dialect registry (`tests/dialect_registry.py`) | No new top-level construct, dialect, or `specs/`/`examples/` directory. No registry edit; record this reviewed no-change disposition. |
| 25 | Frozen Python compatibility reference (`src/fslc`) | No product behavior change. Native Rust is authoritative and this feature is not an explicitly requested compatibility change. Any observed difference is documented, not “fixed” in Python. |

## 7. Executable controls and calibration

### 7.1 Accepting controls

Use a checked fixture with a deterministic cycle and two reachability goals:

```fsl check
spec NestedOptionAssignment {
  type Bit = 0..1
  state { x: Option<Option<Bit>> }
  init { x = none }

  action wrap() {
    requires x == none
    x = some(none)
  }
  action fill() {
    requires x == some(none)
    x = some(some(1))
  }
  action clear() {
    requires x == some(some(1))
    x = none
  }

  invariant Shape {
    x == none or x == some(none) or
    x == some(some(0)) or x == some(some(1))
  }
  reachable Wrapped { x == some(none) }
  reachable Filled { x == some(some(1)) }
}
```

At depth 3, require:

1. direct Monitor steps equal `None`, `Some(None)`, `Some(Some(Int(1)))`,
   `None` in order;
2. a test-only Monitor traversal reaches exactly the three logical states;
   explicit reaches closure and witnesses `Wrapped` at step 1 and `Filled` at
   step 2 without exposing its internal state-count as a language contract;
3. BMC produces the same reachability steps and clean verdict;
4. `transition_matches_step` accepts both concrete successors and rejects a
   deliberately substituted `None` for `Some(None)`;
5. a companion invariant-violation fixture after `fill` makes explicit and BMC
   compare the complete trace, and both traces replay through Monitor;
6. ordinary JSON and replay round-trip the three distinct values; conformance
   JSON emits exact fully tagged counterparts.

Add placement cases for inline init, root assignment, Map index assignment,
struct-field assignment, whole-struct construction, Map-value struct field,
and a conditional whose branch is `some(none)`. Add one depth-3 nested Option
case to prove recursion rather than a two-level special case. Add a bounded
payload case to exercise conditional bounds and induction.

The existing comparison-only test, conformance test, and coverage detail are
retained. The comparison test remains a preservation control. The conformance
test is strengthened from “some exists” to exact nested tags, but it is not
presented as BMC coverage. A new assignment control owns that claim.

### 7.2 Rejecting controls

A table-driven `check` test covers every unsupported family in section 3,
including direct state, struct-field, and Map-value positions. For every row it
asserts the full stable result:

```text
process exit = 2
result       = error
kind         = type
message      = exact selected wording
hint         = exact actionable wording
loc          = exact authored declaration line/column
```

The core control calls `build_model` directly and proves rejection before any
verifier API is invoked. CLI and LSP receive the same source and compare
message/kind/span/hint exactly.

### 7.3 Comparison scope

Compare stable semantic observations in full: verdict/result, violation kind
and name, reachable names and witness steps, action coverage, logical state,
action/params, property/action locations, trace changes, replay result,
closure, and comparable depth. Build any exclusion list from actual emitted
objects, and fail if an excluded key is absent on both sides.

Do not compare `cost`, elapsed time, solver statistics, cache provenance, or
`states_explored` for equality across engines. Check such ambient fields only
for their declared type/range/membership. In particular,
`typed_agreement/inventory.v1.json` already records that BMC has no concrete
state-count analogue; this change must not turn `states_explored` into language
semantics. Run every new or changed control twice in the same session.

### 7.4 Detector mutations

Register the seven implementation-seam mutations below as minimal patches
under `rust/fslc/tests/fault_operators/`; keep the comparator corruption as its
existing in-process test-only control. Each mutation is isolated; report the
produced value beside the expected value. After each manual calibration,
revert and prove exact equality to the named baseline with `git diff --exit-code
155203331d95a12f912b39a84e8803fa79f45ed1 -- <paths>` (or the implementation
PR's recorded pre-mutation commit).

| Detector label | Isolated mutation | Expected detector observation |
|---|---|---|
| Symbolic context detector | Change the new assignment call from `eval_expected(rhs,target_ty)` back to plain `eval(rhs)` | Produced `Err("some() requires a typed value")`; expected clean agreement and `Wrapped@1`. The new assignment test fails; the old equality test may keep passing and is explicitly the blind preservation control. |
| Recursive Option detector | In the expected `Some` arm, evaluate the child without its inner expectation | Depth-2/3 construction produces the same untyped error or a type mismatch; expected `Some(None)` / depth-3 state. The depth-3 case prevents a depth-2 patch from masquerading as recursion. |
| Concrete wrapper detector | Mutate concrete `Expr::Some(e)` to return `eval(e)` without `Value::Some` | Produced state after `wrap` is `None`; expected `Some(None)`. Exact Monitor state and successor-admission tests fail. |
| Explicit wiring detector | Insert/requeue the parent state instead of the Monitor successor in explicit exploration | Produced closure lacks `Wrapped@1`/`Filled@2`; expected both witnesses. Monitor and symbolic direct controls remain green, showing the detector owns explicit wiring. |
| State-shape gate detector | Remove the `Option` payload rejection for one collection branch | Produced `check` exit 0 for `Option<Set<Bit>>`; expected exit 2/type/located error. The corresponding table row fails. |
| Diagnostic-location detector | Drop `.at(field.span)` from the new state-shape error | Expected exact `loc` at the state-field declaration (`line:6`, `column:5`). Produced `loc` is not absent: the origin fallback remains and moves it to a different location. The focused CLI detector rejects that move. |
| Ordinary JSON detector | Restore unconditional `Some(v) => fsl_value_json(v)` flattening | Produced `none == null` and `some(none) == null`; expected `null` versus `{"kind":"some","value":null}`. Exact JSON, changes-path, testgen, and replay round-trip controls fail. |
| Comparator detector | Corrupt one normalized BMC reachable step or nested state after observation, before comparison | Produced `Filled` step/state differs; expected exact equality. The comparator's existing negative-control pattern rejects the corruption. |

Each registered operator names a primary failing target and an unrelated blind
target, as required by `operators.txt`. A patch that stops applying is a loud
failure. A preservation control that remains green is reported only as a blind
control, never as the detector.

The earlier wording that this mutation produced an absent `loc` was incorrect.
Dropping `.at(field.span)` leaves the intentional origin fallback intact (the
#831 / PR #935 removal of fabricated 1:1 locations), so the calibration must
detect movement away from the exact state-field declaration rather than the
absence of a location.

## 8. Four-PR implementation sequence

Each PR stays at roughly 300 authored logical lines or less and has one review
topic. Generated HTML and schema/golden bytes are reported separately from the
authored-line budget. Each semantic PR includes a narrow regression test; PR 4
hardens and calibrates the controls rather than leaving earlier behavior
untested.

### PR 1 — typed boundary and three-engine semantics (about 280 lines)

Changes:

- add recursive `optional_scalar` / total `legal_state` validation and located
  type diagnostics;
- share target-type lookup and use contextual RHS evaluation for init/action,
  index, field, constructor, and conditional paths;
- add narrow core/verifier/runtime regressions for root assignment and rejected
  shapes.

Verification, with the new focused tests run twice:

```bash
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo test --manifest-path rust/Cargo.toml --locked -p fsl-core --test state_type_boundary nested_option_state_shape -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fsl-verifier --test transition_agreement nested_option_assignments_are_context_typed -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fsl-runtime --test explicit_engine nested_options_remain_distinct -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fsl-core --test state_type_boundary nested_option_state_shape -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fsl-verifier --test transition_agreement nested_option_assignments_are_context_typed -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fsl-runtime --test explicit_engine nested_options_remain_distinct -- --exact
./tools/check-native-integration.sh fsl-logic pr
```

The test names above are the names to create; they are deliberately stable
contract labels, not existing claims.

### PR 2 — lossless JSON, replay, testgen, and Worker projection (about 280 lines)

Changes:

- implement the canonical ordinary nested-Option encoding, typed decode,
  logical change path, and summary;
- preserve fully tagged conformance encoding and legacy `Option<scalar>` bytes;
- add exact replay/testgen/conformance and native/Worker rendering controls.

Verification, with each focused changed control repeated:

```bash
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test kernel_contract conformance_distinguishes_nested_options_and_guard_partials -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test replay_trace_contract nested_option_state_round_trips_without_flattening -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test testgen_contract nested_option_expected_state_is_lossless -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fsl-wasm tests::nested_option_trace_matches_native_encoding -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test kernel_contract conformance_distinguishes_nested_options_and_guard_partials -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test replay_trace_contract nested_option_state_round_trips_without_flattening -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test testgen_contract nested_option_expected_state_is_lossless -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fsl-wasm tests::nested_option_trace_matches_native_encoding -- --exact
./tools/check-native-integration.sh fsl-logic pr
```

### PR 3 — normative docs and coupled surfaces (about 220 authored lines)

Changes:

- update both language references, skill reference, accepted nested-Option
  design note, replay/kernel design boundary, and old-note supersession;
- regenerate both language site pages;
- add the changelog fragment and update the shared CLI/LSP diagnostic test;
- record no-change dispositions for LSP indexing, dialect registry, and frozen
  Python in the PR description.

Verification:

```bash
python3 tools/build_site_reference.py
python3 -m pytest tests/test_site_reference_snapshot.py -v
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test lsp_diagnostic_contract
python3 -m pytest tests/test_site_reference_snapshot.py -v
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test lsp_diagnostic_contract
./tools/aggregate_changelog.sh check
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
```

The site snapshot test is the required EN/JA section-count/order/freshness
check. The Python package remains frozen; using its docs generator is not a
Python language-semantics change.

### PR 4 — calibrated cross-engine and fault controls (about 290 lines)

Changes:

- add the clean and violating nested fixtures to `typed_agreement`, exact
  state/successor comparison, placement matrix, and unsupported-shape matrix;
- register the minimal source fault operators from section 7.4;
- update the typed-agreement inventory only for actual observed/excluded keys.

Verification:

```bash
cargo fmt --manifest-path rust/Cargo.toml --all -- --check
cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test typed_agreement nested_option_assignment_agrees_across_all_engines -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test cli_regression unsupported_nested_option_payloads_fail_check_with_locations -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test typed_agreement nested_option_assignment_agrees_across_all_engines -- --exact
cargo test --manifest-path rust/Cargo.toml --locked -p fslc-rust --test cli_regression unsupported_nested_option_payloads_fail_check_with_locations -- --exact
./tools/check-native-integration.sh fault-operators
./tools/check-native-integration.sh fsl-logic scheduled
cargo test --manifest-path rust/Cargo.toml --workspace --locked
cargo build --manifest-path rust/Cargo.toml --workspace --locked
./tools/check-native-integration.sh
```

The last command is the complete product gate and remains required for the
merged `main` state and release promotion even if merge-readiness evidence is
already green.

## 9. Risks and mitigations

| Risk | Why it matters | Mitigation / evidence |
|---|---|---|
| **Most important: semantic state is losslessly distinct internally but flattened at the public JSON/replay boundary** | A false agreement can be reported while `none` and `some(none)` replay as the same state; generated adapters can accept a wrong implementation. | Canonical hybrid encoding, exact state/change/replay/testgen controls, and the JSON-flattening fault operator. |
| Context is fixed only for root assignment | Struct literals, conditionals, Map entries, or field updates retain the fail-late error. | One contextual evaluator; placement matrix across every lvalue/constructor path. |
| A depth-2 special case is shipped | Depth 3 recreates the same hollow accepted construct. | Recursive predicates/evaluation and an explicit depth-3 accepting/mutation control. |
| The generic recursive type model admits unsupported containers | Solver/runtime code may later crash or, worse, appear verified without supported semantics. | Total resolved-type whitelist with one rejecting row per `TypeRef` family and located diagnostics. |
| Three processes are mistaken for three independent semantics | Monitor BFS and explicit can share the same wrong evaluator and outvote BMC in rhetoric. | No majority voting; identify two semantic lineages; compare every required edge; do not claim triangulation. |
| Payload defaults constrain absent Options | A solver may accidentally require an absent inner scalar to be in range, causing false negatives. | Preserve `present => bounds(payload)` recursively and mutate/check bounded nested payloads. |
| Trace object tags collide with ordinary struct fields | A generic JSON walker can emit `x[kind]` and hide the logical Option change. | Type/value-aware change computation; exact logical-path assertion. |
| Existing `Option<scalar>` clients see a breaking byte change | Always-tagged output would silently break replay and generated adapters. | Tags only when the payload type is Option; byte-for-byte legacy golden preservation. |
| Ambient counters make agreement flaky or become de facto semantics | Cache, backend, and traversal order can change counts without a language change. | Exact stable projection, membership checks for ambient fields, dead-exclusion checks, and two runs per changed control; never equate `states_explored` across engines. |
| State-space cost grows with Option depth | `Option^n<T>` has additional presence combinations and can increase explicit/BMC cost. | Keep budgets/cost as operational observations, add small depth-3 probes, and use scheduled tier for breadth; do not cap the language at depth 2 without a separate contract decision. |
| A placement-matrix row silently removes an existing accepted form | The matrix is not proven to preserve existing verdicts; the removal surfaces as a required-test failure (PR #923) or, worse, ships undocumented. | Section 11 requires `check` verdict measurement, measured over every `.fsl` under `specs/`, `examples/`, and `rust/fslc/tests/fixtures/` in the materialized tree of each SHA, with both binaries, plus the inline fixtures of every required test the change rewrites, renames, removes, or stops from running; each accepted→rejected transition is recorded as a breaking change with the coupled surfaces in rows 17, 18, 20, and 22. |

## 10. Completion criteria

The implementation is complete only when all of the following are true:

1. every legal placement in section 3 checks and executes without a verifier
   typing error;
2. BMC, Monitor, and explicit produce the exact distinctions and agreement in
   section 7, including replayable traces;
3. every unsupported family fails `check` with the authored declaration `loc`
   and never reaches a solver/runtime;
4. ordinary and conformance JSON each follow their documented, distinct
   canonical forms, and existing `Option<scalar>` bytes do not change;
5. all 25 coupled surfaces have their specified change or reviewed no-change
   control;
6. every named detector has been executed against its isolated mutation, its
   produced/expected values recorded in the PR evidence, and its exact revert
   proved;
7. the focused controls pass twice, the scheduled FSL-logic lane passes, and
   the complete native integration gate passes;
8. every accepted→rejected `check` verdict transition between the named base SHA
   and head, measured over every `.fsl` under `specs/`, `examples/`, and
   `rust/fslc/tests/fixtures/` in the materialized tree of each SHA, with both
   binaries, plus the inline fixtures of every required test the change
   rewrites, renames, removes, or stops from running, is recorded as a
   breaking change (section 11), or PR evidence documents an empty measured
   transition set with the command, both SHAs, and the form count.

There is no remaining product-policy question in this design. The only fixed
choice is full recursive support for nested Option around scalar payloads;
expanding Option to structs or collections would be a separate language
change.

## 11. Amendment (#925): matrix rows that change an existing verdict

The §3.2 "Map value" row changed an existing accepted form; it was found by CI,
not by design review. The root cause is that a type-boundary gate rewrite is
only complete when contract preservation is measured, and a placement matrix
does not establish that existing accepted forms are preserved.

Run the following procedure before merge and record its result in the PR evidence:

1. Name the base SHA and the head SHA; build a `fslc` binary from each (or use
   a release binary built from a SHA whose `rust/` tree — including
   `rust/Cargo.toml`, `rust/Cargo.lock`, and every crate manifest — differs
   from the base SHA only in files that are not compiled into `fslc`, and show
   that `git diff --stat <release-sha> <base-sha> -- rust/` in the PR evidence).
2. Materialize each SHA's full tree (`git worktree add <dir> <sha>`, or `git
   archive <sha> | tar -x -C <dir>`) and, in each materialized tree, run
   `fslc check` in place on every `.fsl` under `specs/`, `examples/`, and
   `rust/fslc/tests/fixtures/` with both binaries, so `use ... from` imports
   resolve against that SHA's own siblings; a form's verdict is the result for
   its file in its own tree. Add every inline fixture in a required test the
   change rewrites, renames, removes, or can stop from running (through a
   shared test helper, a crate manifest test target, or
   `tools/rust-test-shard-groups.txt` it edits). An untouched required test that
   pins an acceptance fails in CI on its own when the verdict changes, provided
   it still runs; a test the change rewrites, renames, removes, or silences is
   where a transition can be laundered, so those are the inline forms that must
   be re-measured from the base SHA.
3. List every form whose `result` changes from `ok` to `error`. Each is a
   breaking removal: record it in this design note (§3.2), `docs/manual/LANGUAGE.md`,
   `docs/manual/LANGUAGE.ja.md` (section-aligned), `skills/fsl/references/syntax.md`,
   and a `changed` fragment in `changelog.d/`, and rename or replace the
   fixture and test that pinned the acceptance.
4. If the list is empty, state the command, both SHAs, and the form count in
   the PR evidence. "No transition" is a measured claim, not a default.

The corresponding rule in `AGENTS.md` (Correctness invariants) mirrors this procedure.
