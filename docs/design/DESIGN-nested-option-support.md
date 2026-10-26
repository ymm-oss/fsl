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
`Set<B>`, not a universal `Set<Int>`. Collection binders inherit the endpoint
type, as do public Kernel expression and binder annotations (the schema is
unchanged). This restores valid enum projection idioms, including
`not (exists x in domain(r): reachable(r, x, x))`, while closing the numeric
endpoint bypass through enum projection binders. Symbolic and concrete
projection semantics are unchanged; enum/Int confusion is rejected before
evaluation rather than interpreted through ordinals. Named numeric endpoints
remain integer-compatible. The fixed domain/range target has nine detectors
(three restored enum uses and six newly rejected forms) and seven numeric
preservation/rejecting controls. Existing non-relation signature handling is
outside this change (#1459).

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

#### Historical endpoint-only measurement (before projection typing/restack)

BASE and the HEAD commit reference are both
`75ce67832faf063f3ffb850ddb6ef6465b5b62ef`: HEAD here is that commit plus
the uncommitted production patch SHA-256
`0caed026d28c3653628f821ed912b420e740941fe3843c9665b17c298cc7e945`,
not a claimed new commit. BASE was materialized by `git archive` under
`.claude/work/base-tree`; HEAD is the working tree. Each binary checked forms
in its own tree. The supplied BASE binary `.claude/work/fslc-1439` has SHA-256
`ed375b54aea571629ab3cd3f1e7d0e07c4a5b8b6aac034c3d871eff4908eb3fd`;
the final built HEAD `rust/target/debug/fslc` has SHA-256
`b72f3ccf797fecc628e898e3127df13dcb0cfcc6b0353ba7a9f3e53f65a7139a`.

Exact driver commands: `python3 .claude/work/sweep-1458.py prepare`,
`python3 .claude/work/sweep-1458.py base`,
`python3 .claude/work/sweep-1458.py head`,
`python3 .claude/work/sweep-1458.py compare`. Each check invocation acquires
the lane lock followed by the heavy lock. The driver checks every `.fsl` under
`specs/`, `examples/`, `rust/fslc/tests/fixtures/` (**579 forms**), plus every
inline fixture occurrence in the fixed regression file (35), the augmented
native/LSP diagnostic test (8), and the augmented Worker parity test (7):
**629 forms per tree**, twice with `FSLC_UNSTABLE` removed and twice with
`FSLC_UNSTABLE=fsl5`, **5,032 checks**. This includes deliberately rejecting
corpus outcomes such as governance `refinement_failed` / exit 1; those are
compared, not dropped. Repeat stdout bytes, JSON, stderr and exit codes agree
exactly, without any excluded field.

The measured corpus `ok` → `error` transition set is **empty**. The following
17 inline occurrences (15 distinct source texts) each change from `ok` / 0 to
`error` / `semantics` / 2 in **both modes**: **34 mode-form transitions**.
All other full outputs are unchanged. Here `N = 0..2` and `Color = {Red, Blue}`;
all new errors retain invariant origin 7:3 and the endpoint repair hint.

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

This is the final #1458 measurement, including endpoint-typed `domain`/`range`,
not a transfer of the historical endpoint-only observation above:

- BASE: `e8cf78f0162530e264128f588981d49790f2767f`, materialized with
  `git archive e8cf78f0 | tar -x -C .claude/work/base-tree-e8cf`.
- HEAD: restacked implementation commit
  `c11d5ed399807979c3bf89b07e06e4d2cbde9adc`, on the rebased #1439 commit
  `73d7def57dc7da0ca60cdf9480ccf0bda8762159`. The pre-restack implementation
  `17134b6dc6245a036a7487d8c58081ab25fc826b` is retained on
  `v5/1458-pre-restack`; `git range-diff` reports identical patches. There were
  no rebase conflicts. This evidence-only documentation follow-up does not
  change production or test sources.
- BASE binary: `.claude/work/base-tree-e8cf/rust/target/debug/fslc`, SHA-256
  `2208f37aa85ed5535c7132ff008af90e235cbb09face9dc367bdbf3b0f8b9367`.
  HEAD binary: `rust/target/debug/fslc`, SHA-256
  `a3bf584becb4f2ce98e03b5a03be0b3aa32f668627ef5fd6d7e7a017c6e6f1d6`.
  Checker SHA-256 values respectively:
  `a7d9fa098655a325fe184cc6d849fb37d6dbfc32288dc66e9436c026447ebe9b` and
  `61dcd7feb19f120b53f3df6fc3cde8240a9a86f345462075f0a7c32b8e6888b8`.

The first post-restack HEAD build was
`cargo build --manifest-path rust/Cargo.toml --workspace --locked --offline`.
BASE was then built with a **fresh, unpopulated target**, not cloned artifacts:
`cargo build --manifest-path rust/Cargo.toml -p fslc-rust --bin fslc --locked --offline -v`
from the archived tree. Its verbose log confirms `fsl-syntax`, `fsl-core` and
`fslc-rust` compiled from that tree's paths. Both builds exited 0, acquired the
lane lock followed by the heavy lock, and used toolchain 1.98.0 with four build
jobs/test threads and worktree-local TMPDIR. Calibration ran the #1439 S1
`paint(if c != 1 then Red else Blue)` forbidden final-step form and #1458
`not reachable(r, Red, Blue)` numeric-relation form: both are `ok` / 0 on BASE
and `error` / `semantics` / 2 on HEAD, **twice**.

Exact orchestration command: `python3 .claude/work/restack-v4-1458.py` (exit 0).
It builds BASE, mechanically expands the inline fixtures, calibrates the
binaries, then invokes `python3 .claude/work/restack-sweep-1458.py base`,
`python3 .claude/work/restack-sweep-1458.py head`, and
`python3 .claude/work/restack-sweep-1458.py compare` (each exit 0).
`python3 .claude/work/restack-audit-1458.py` also exits 0 and accounts for every
changed output. Each binary runs `check FORM` in its own materialized tree,
acquiring the lane lock followed by the heavy lock **per invocation**. All runs
remove `FSLC_UNSTABLE` for stable mode or set it to `fsl5`, with cache disabled.
Uptime is checked before each invocation, waiting for one-minute load <= 9.

| Forms per tree | Count |
|---|---:|
| Every `.fsl` under `specs/`, `examples/`, `rust/fslc/tests/fixtures/` | 591 |
| All five `issue_1439_*.rs` files: expect/nested/scope/step/unary | 6 + 8 + 2 + 21 + 10 = 47 |
| Fixed #1458 endpoint and domain/range files | 35 + 16 = 51 |
| Native/shared diagnostic step and parity fixtures | 2 + 8 = 10 |
| Worker parity, step and scenario-then fixtures | 7 + 2 + 1 = 10 |
| LSP stdio expectation and step fixtures | 2 + 2 = 4 |
| Total | **713 (591 corpus + 122 inline)** |

All #1458 inline expansions are byte-checked against the actual fixtures written
by the immutable tests. Each mode/tree runs twice: **5,704 checks**. Complete
stdout bytes, parsed JSON, stderr and exits repeat exactly; **no fields or forms
are excluded**. The **591-form corpus `ok` → `error` transition set is empty**,
with no other corpus output drift.

The #1458 breaking removal set is exactly the 15 endpoint source expressions in
the table above, the duplicate native target-only contains / Worker target-only
reachable occurrences, and all six enum projection expressions in the earlier
projection table: **23 occurrences, 21 distinct sources, 46 mode-form transitions**.
All are `ok` / 0 → `error` / `semantics` / 2 in both modes. Endpoint and bypass
errors retain their repair hint; projection membership/comparison errors retain
the existing assignability message and authored origin without inventing a new
hint. The three legitimate enum projection idioms check on both BASE and final
HEAD; only the intermediate endpoint-only patch regressed them.

The larger comparison also observes the inherited #1439 change: **70 additional
mode-form removals**, already governed by `DESIGN-requirements-stage.md`, and
**three error-envelope reclassifications**, not removals or excluded drift.
The latter are the published `expect c == 1` fixture in both modes and the FSL 5
`then c == 1` fixture with the gate enabled: both binaries exit 2, but BASE
reports `kind: "acceptance"` after replay, while HEAD reports `kind: "semantics"`
with the Int-to-Color diagnostic at the authored keyword. In total there are
116 mode-form removals, these three reclassifications, and **1,307 entirely
unchanged mode-form pairs**. No further output changes were observed.

Direct public Kernel controls also run twice for `N -> Color` and `Color -> N`:
`python3 .claude/work/kernel-projection-1458.py` exits 0, requires `rel_domain`
and `rel_range` set-item annotations to retain the correct named endpoint, and
compares complete repeat outputs without exclusions. This checks projection
annotations, not independent semantic triangulation or unbounded verification.

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
