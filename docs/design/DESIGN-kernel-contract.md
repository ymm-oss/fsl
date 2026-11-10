# Normalized public Kernel contract and conformance corpus

Status: implemented by issue #208. The native Rust implementation is the
authority; the frozen Python reference is intentionally unchanged.

## Goal and boundary

External compilers, generators, and analyzers consume a versioned JSON contract
after FSL parsing, dialect lowering, name resolution, and semantic checking. They
must not parse FSL source, depend on Rust syntax-tree internals, or consume the
historical Python-shaped AST JSON.

The boundary is:

```text
FSL source / dialect
  -> Rust parse and lowering
  -> checked KernelModel
  -> public Kernel JSON v1
  -> external compiler or generator
```

`fsl_core::public_kernel_contract` is the Rust API. `fslc kernel SPEC` is the
language-neutral process API. The output's `$schema` resolves to
`schemas/fslc/kernel/kernel.v1.schema.json`; `schema_version` is `1.0.0`.

## Contract shape

The top-level contract contains:

| Field | Contract |
|---|---|
| `spec` | Logical name plus source file and original dialect. |
| `semantics` | Simultaneous updates, pre-state reads, disabled requires, rollback failures, `old`, Euclidean integer division, terminal handling, and weak fairness. |
| `constants`, `types`, `state` | Fully normalized declarations with structural types and lowering origin. |
| `init` | Typed normalized statements plus requirement/origin metadata. Kernel state inline initializers appear only as equivalent root assignments. |
| `actions` | Finite typed parameters, ordered `guards` (requires/let binding scope), compatibility projections in `requires`/`lets`, simultaneous updates, ensures, partial-operation conditions, requirement IDs, origin, and span. |
| `properties` | Invariants, transition properties, reachability, progress, and terminal expression. |

Every expression has a `kind`, structural `type`, and source `span`; consumers do
not reparse expressions or infer types. Statement and declaration origins state
the source dialect, originating declaration or requirement ID, whether lowering
occurred, and whether the node was generated. Action/property/statement spans
include start and end positions. The internal surface tree retains inline state
field and initializer spans for diagnostics and migration, but Public Kernel v1
does not publish the sugar as a second declaration form. Direct scalar/type/state
declaration spans in the v1 projection remain explicitly `null`; normalized
initializer statements carry their source span. This is an intentional v1
distinction, not a missing field.

That v1 `origin` object is a frozen, lossy compatibility projection. The Rust
frontend's authoritative internal provenance is the non-serialized origin chain in
[`DESIGN-origin-chain.md`](DESIGN-origin-chain.md), where declaration/source
origin and requirement traceability are separate types. The historical v1
projection may still place a requirement tag in `origin.declaration` to remain
byte-compatible; internal consumers must not infer source identity from it.
The internal chain is absent from this closed schema and will be published only
by Public Kernel v2 (#256).

Unsupported or incompletely lowered structures fail export with a semantic error.
The exporter never silently omits an unknown type, predicate call, expression,
lvalue, or non-scalar parameter domain.

The Rust domain frontend reaches this boundary through typed name/type
resolution and direct AST-to-AST lowering. Generated Kernel source from
`fslc domain expand` is a debug representation only and is never reparsed to
construct the checked contract. Resolver diagnostics therefore retain the
original domain node coordinates.

Compose lowering currently loses the component filename while retaining its line
span. Public Kernel v1 therefore rejects `compose` input explicitly instead of
fabricating a root-file source location. A future schema version may add
multi-source provenance once it is retained by the checked Kernel.

## Transition and failure semantics

Action right-hand sides read the same pre-state and updates commit
simultaneously. `old(expr)` also reads the pre-state. A false `requires` clause
means the action instance is not enabled.

The native Monitor evaluates and classifies one selected action in this order:

1. guards (`let` and `requires`) in source order;
2. body statements, with all right-hand sides reading the pre-state;
3. state-variable type bounds in model order;
4. invariants in declaration order;
5. transition properties in declaration order; and
6. action `ensures` in declaration order.

The first reached failed check or partial expression determines the outcome. A
false guard is `requires_failed`; a reached partial expression is `partial_op`;
and false post-update phases are respectively `type_bound`, `invariant`, `trans`,
or `ensures`. Guard/body failures have no attempted state. Failures after the
body retain the exact uncommitted successor as `attempted_state`.

Partial operations (`head`, `pop`, `at`, sequence indexing, division, and
remainder) are listed with a typed failure condition. The set is defined once as
`fsl_core::PartialOperation` and shared by the Public Kernel, the verifier, the
runtime (`RuntimeError::partial_operation`) and `fslc explain` (#1166); an
indexed assignment target's index expression is listed like any other operand.
A quantifier, an aggregate and a statement-level `forall` are expanded into one
entry per finite candidate, with the binder replaced by the candidate and the
failure condition guarded by the candidate's membership and `where`, so no
failure condition names a bound variable (#1190). An operation in a branch of
an expression or statement-level `if` is guarded by the branch's condition,
negated for `else`, and nested `if`s and `forall`s stack their guards, so a
failure condition holds only on a path that evaluates the operation (#1260).
A statement-level `forall`
with no partial operation in its binder, `where` or body is not expanded, so its
range need not have constant bounds. Every classified failure
above rolls back the whole step: the Monitor returns the input state and leaves
its internal state unchanged. For diagnostics, `attempted_state` is absent for
guard/body failures and is the exact uncommitted candidate for any later
`type_bound`, `partial_op`, `invariant`, `trans`, or `ensures` failure. This
rollback rule is shared by the schema and executable vectors.

A guard (`let` or `requires`) that reaches a partial operation is that action's
`partial_op` (`_partial_<action>`) in every engine, exactly like a body partial
operation (#1191). The solver-free walks — explicit-state `verify`, Monitor
BFS, the concrete boundary pre-pass below, refinement's self-violation walk,
and the concrete cover/response/reachability walks — enumerate instances with
`Monitor::candidates`, which returns such an instance as
`ActionCandidate::GuardPartial` instead of aborting the whole walk with the raw
evaluation error; `Monitor::step_candidate` turns it into the `partial_op`
step (state unchanged, no `attempted_state`), the same evidence the symbolic
engine's witness carries. Walks that only follow successful steps skip it like
any other violating step. Such an instance is not *disabled* — its guard did
not evaluate to `false` — so it keeps its state from being a deadlock and does
not count toward action coverage. `Monitor::enabled` keeps returning the raw
error for callers that need every guard to evaluate.

The `verify` envelope's `loc` for a guard `partial_op` is the span the Public
Kernel's `partial_operations` gives that guard's site — the `requires` clause,
or the action itself for a `let`, which carries no span of its own — in every
engine, also for an action without a body. The renderer finds the failing guard
by re-evaluating the guards on the trace's last pre-state
(`fsl_runtime::guard_partial_operation`). A body `partial_op` keeps its
existing location.

`partial_operations` is an action-context list only. An `init` statement's
partial operations (and its other failure sites, such as a `Map` key outside
the finite key domain or a checked integer overflow) are not published in the
Public Kernel: an init failure is neither a rollback nor a `partial_op`
violation, because there is no prior state to return to and no step to blame.
Init definedness is a verifier obligation instead: every engine (`explicit`,
`auto`, `bmc`, `induction`) reports an init that some initial state cannot evaluate
as a `semantics` error with exit code 2, and all engines locate it at the failing
init statement with the same ` in init at L:C` suffix
(e.g. `division by zero in init at 9:5`; #1258, #1543).
Even with out-of-type init assignments, `bmc` and `induction` prioritize init
undefinedness over assigned-root bounds and retain this same located message (#1597).
If a symbolic init failure is found only beyond a stored `Seq`'s represented
capacity, for example when init stores an over-capacity sequence and reads beyond
its slots, these engines instead refuse without a location as
`model sequence length exceeds capacity`.
The schema is unchanged. Publishing init failure conditions, should a Kernel
consumer need them, would add a field with its own `state_effect_on_failure`
value and so a minor schema version.

For binder expressions in init, symbolic diagnostics follow concrete evaluation
order and name the first reached operation (#1607): source, then each present
candidate's `where`, then its body/summand when selected. `forall` and `exists`
respect their decisive-result short circuits; `count`, `unique`, and `exactlyOne`
continue through all candidates. `sum` reports an undefined summand's operation
before accumulation, and checks every partial sum for `integer overflow in sum`.
These diagnostic sites retain the containing statement's location and remain
verifier-internal; they do not add entries to `partial_operations`.

## Concrete boundary pre-pass budget

Native verification has a solver-free concrete boundary BFS —
`fsl_runtime::find_boundary_violation` — over exactly the `Monitor` semantics
above, looking only for a `partial_op` or `type_bound`
outcome, supplying concrete, replayable `violated`/`type_bound` evidence for
state assignments beyond a sequence's declared capacity. Symbolic `push`
temporaries retain every live element even past `Seq<T, N>`'s capacity; the
state type boundary checks `len <= N`, independently of temporary storage.
Symbolic state frames remain bounded to N slots. When a sequence's implicit
bound is not checked, any reachable frame with `len > N` makes the symbolic
search fail closed with a `semantics` error, "model sequence length exceeds
capacity", before evaluating properties or actions on its incomplete data.
It does not silently assume that bound to discard the overflowing path, nor
use a reconstructed trace to justify a verdict on missing elements.
This guard is deliberately more conservative than the former render-time
failure: it can reject a selected run even when its property does not read the
Seq, or after a reachable target was already witnessed, if a later explored
frame overflows.

The native boundary pre-pass runs when `selected_implicit_bounds` returns
`None`, no initial state is supplied, and init is deterministic. Standalone BMC
`--property` selections skip it. Induction instead materializes the selected user
properties and clears selector metadata before preparing its base: every implicit
bound assumed by its step is checked even under `--property P` or
`--property _bounds_xs` (#1598). These induction runs can enter the pre-pass and
receive the initial step-0 correction. Selected-trans induction additionally
retains all user invariant hypotheses. For an induction base whose typed state
contains no Seq (including nested types), symbolic BMC runs first (#1628).
Only a `verified` base skips the redundant concrete search; every symbolic
non-success or error still runs the same pre-pass with the same evidence
priority. Seq models and standalone BMC keep the eager pre-pass. No implicit
bound check or induction hypothesis is removed.
`--exclude-property <user property>` alone leaves all implicit bounds checked
and retains pre-pass eligibility: its initial-bound evidence also moves from
step 1 / two states to step 0 / one state when the old probe found an idle
successor. It can likewise change a depth-0 Seq projection error to a violation.
Internally, `checked_bounds: Some(...)` can likewise omit bounds; the symbolic
guard applies to each omitted Seq bound.
This is not a public `--exclude-property _bounds_*` route: the current CLI
rejects implicit-bound names in `--exclude-property` as `no such property`.

Verification witness replay also carries this same bound selection (#1531),
for BMC, induction's bounded witnesses, and explicit. The Monitor replay step
checks only the selected implicit bounds: an omitted range bound must not roll
back an intermediate step and turn a genuine selected-property counterexample
into an internal trace mismatch. Explicit now rejects an explored successor
whose omitted root Seq bound exceeds capacity with the same `semantics` error
as BMC, before enqueueing or reporting that successor. The initial frame itself
is not screened by explicit's new guard, but its successors are, including those
that preserve an initial overflow unchanged. The exact selected-run verdict
changes are recorded in
[DESIGN-explicit-engine.md](DESIGN-explicit-engine.md#selected-bounds-and-witness-replay-1531).

The existing `replay_trace`, `replay_trace_from_state`, and
`replay_bmc_witnesses` entry points remain all-bounds wrappers; their
`*_with_bounds` variants carry a verification run's selection (`None` still
means all bounds). Only selected verification orchestration opts into this
policy. Scenarios, mutation verdict replay, refinement, the `fsl-bmc` binary,
and Worker verification continue using the all-bounds entry points. Worker
has no property-selection option; this change adds none.

Before exploring successors, the pre-pass checks only the initial state's type
bounds using the same bounds helper as the `Monitor`; it does not evaluate user
invariants or propagate their evaluation errors at this point. An initial
`type_bound` violation in a model with actions is returned at step 0 with exactly
one trace state (the initial state), no action, `exhausted: false`, and
`states_explored: 1`, even at depth 0 (#1591). It does not wait for an enabled
action to reproduce the invalid value. The native default `bmc` and `induction`
paths therefore agree with explicit on this initial-bound evidence. Selections
that bypass the pre-pass, supplied initial states, and nondeterministic init
retain their existing routes. A valid initial state followed by an overflowing
first action still reports step 1 and two
trace states. With no actions, the probe skips this initial check and the
existing verifier rejection remains exit 2 / `semantics` / `spec has no actions`
under bmc, induction, explicit, and auto. The search budget and exhaustion
fallthrough are unchanged.

The WASM Worker's `verify` shares `find_boundary_violation`, so deterministic
models with actions receive the same step-0/root-only initial-bound correction.
Its caller has no property-selection or supplied-state option. This shared
runtime change does not add either option to the Worker.

This is not a replay change: the CLI's legacy empty action trace `[]` still
does not evaluate initial-state bounds (#1591's third issue item). It is not
evidence that the initial state satisfies its bounds. Versioned replay with an
observed initial state has its existing safety check; the runtime's kernel
state-trace replay rejects a completely empty trace. None of these replay
contracts is changed by the boundary pre-pass fix.

The induction engine also refuses to display an invariant, `trans`, or ranked
`leadsTo` CTI if any state in its SAT model has missing sequence slots, returning
the same `semantics` error rather than reconstructing a trace for a predicate
evaluated on incomplete data. Bound-property CTIs retain supported action-based
reconstruction; sequence length-bound failures already follow from the frame's
length, independently of its missing suffix. This changes neither the induction
premises nor the proof meaning. This restriction also applies to ranking even
when every state bound is checked: a bounded pre-state does not establish
bounds on its one-step successor. The two-state rank SAT arms screen both
projected states before metric extraction and projection. One-state rank
obligations instead rely on the bounded pre-state (or independent deadlock
state); their raw step-0 projection fails closed if an internal bounds subset
leaves that state unrepresentable. A ranking error preserves
an existing BMC `leadsTo` violation rather than replacing it with a phantom CTI;
on the post-induction path it is reported as the semantics error.

This pre-pass is an evidence detour, not a verdict authority: it only ever
returns `partial_op`/`type_bound`, and `probe_bmc_boundary` consumes only the
non-`partial_op` half (`partial_op` belongs to `verify_bounded*` itself, per
issue #651). Nothing about it may change *whether* a spec verifies — only
*how good the evidence looks* when it doesn't. That is what makes it safe to
budget (issue #697): `find_boundary_violation(model: &KernelModel, depth:
usize, budget: usize) -> Result<BoundaryProbe, RuntimeError>` stops after
visiting `budget` distinct concrete states and reports
`BoundaryProbe { finding, exhausted, states_explored }`. Every caller must
treat `exhausted && finding.is_none()` exactly like today's empty result:
fall through to the symbolic engine. Checked action-assignment overflows can
also produce a symbolic `type_bound` witness by reconstructing the actual
attempted value, so this class is no longer representable only by the pre-pass.
An unchecked over-capacity frame is instead refused by the guard before any
property query, not by an attempt to render the selected property's result.
An unrepresentable initial frame has no preceding action to reconstruct and
still fails closed in symbolic projection; the default concrete pre-pass, when
it finds that initial overflow, can supply `type_bound` evidence instead.
The issue #697 default/isolated contrast pins these different paths, not an
inability to render checked symbolic action overflows after budget exhaustion.
Exhaustion must never turn an undecided or fail-closed result into a false green.

For a concrete initial value `xs = Seq { 0, 1 }` with capacity 1 and an idle
action, the engine and path matter (`E` below is exit 2 / `semantics` /
`model sequence length exceeds capacity`; `TB@n` is exit 1 / `type_bound` /
`_bounds_xs` at step n):

| Path | bmc | induction | explicit |
|---|---|---|---|
| Default, depth 0 | TB@0 (concrete initial check) | TB@0 (concrete initial check) | TB@0 |
| Default, depth at least 1 | TB@0 (concrete initial check) | TB@0 (concrete initial check) | TB@0 |
| `--property _bounds_xs` (only BMC skips the pre-pass) | E (symbolic initial projection) | TB@0 (concrete initial check, #1598) | TB@0 |

**Selected-induction verdict transitions (#1598).** These are intentional
selection-contract changes, not changes to standalone BMC or explicit:

| Form under `--engine induction --property P` (or a selected bound) | Before #1598 | After #1598 |
|---|---|---|
| Initial bound omitted at every base depth; a shallow run can also miss P | exit 0 `proved` / `unbounded` (false proof) | exit 1 `violated` / `bounded`, `type_bound`, `_bounds_*` |
| Deterministic initial Seq overflow reaching the pre-pass (including `InitIdle` and the selected `_bounds_xs` row above), or an overflowing Seq successor within the base depth | exit 2 `error`, `kind: "semantics"` | exit 1 `violated` / `bounded`, `type_bound`, `_bounds_xs` (step 0 for init) |
| Bound fails at or before the selected property's failing step | exit 1 `unknown_cti`, or exit 1 `violated P` at the same or a later step | exit 1 `violated` / `bounded`, `type_bound`, `_bounds_*` at the first failing base step |
| The same base through `sweep --engine induction --property P`, or a `verify --lemma` target | exit 0 `sweep_passed`, or exit 0 `proved` for the lemma-assisted target | exit 1 `sweep_failed`, or exit 1 `violated` / `type_bound` for the target |

Same-step examples under selected induction: `n: 0..1`, init 0, increment,
`P { n <= 1 }` at depth 2 changes `violated P`@2 to `type_bound _bounds_n`@2.
For a Set initial copy with `K = 0..1`, `init { x = 2 t = Set { x } s = t }`,
`P { not s.contains(2) }` changes `violated P`@0 to `type_bound _bounds_s`@0.
In both cases the exit stays 1; attribution changes at the same step.

The `proved` envelope's `invariants_checked` includes all implicit bounds;
`violated` envelopes omit that field. They identify the failed bound using
`violation_kind` and `invariant`, with the corresponding base-state evidence.

Each default TB@0 above has the one-state trace `[{step: 0, state: {xs: [0, 1]}}]`
and `last_action: null`. This replaces the former depth-at-least-1 default
TB@1/two-state idle trace, and changes the two symbolic engines' depth-0 default
Seq result from exit 2 / `semantics` to exit 1 / `violated` / `type_bound`.
The same step-0 rule applies to an initial range overflow. The regression matrix
in `rust/fslc/tests/issue_1591_init_bound_step0.rs` covers both forms at depths
0/1/2 under all three engines (18 cells): eight symbolic depth-1/2 cells change
step/trace length, two symbolic depth-0 Seq cells change error to violation,
and the other eight core cells retain step 0. The JSON schema is unchanged;
`violated_at_step` and `checked_to_depth` now identify 0 on the corrected path.

That matrix counts verdict/step/trace changes, not complete JSON-value equality.
An initial violation formerly reported by the symbolic engine (for example a
range overflow at depth 0, or a value repaired by the first action) is now
rendered by the boundary renderer: `violating_bindings` is `[{}]` rather than
`null`, and `cost.properties` is empty. The probe performs no solver checks
(`cost.solver.checks: 0`, with solver memory unavailable/null), unlike the former
symbolic query. Explicit retains its own `violating_bindings: null`; agreement
above refers to the initial violation and trace, not all engine metadata.

The exit-2-to-exit-1 change is not limited to Seq at depth 0. If init already
violates a bound and the first action errors, the probe now returns the initial
`type_bound` instead of exploring that action. For example, `n: 0..5`,
`init { n = 9223372036854775807 }`, `action inc() { n = n + 1 }` changes bmc and
induction at depths 1/2 (four example cells) from exit 2 / `semantics` /
`integer overflow in addition` to exit 1 / `type_bound` / step 0 / one state.
The no-actions case is deliberately not an exit-code change.

Selecting a supported user invariant leaves the Seq bound unchecked in standalone
BMC, which rejects the initial frame via the guard. Induction checks that bound
in its base and, for this deterministic initial overflow, reports TB@0 instead
of the former exit-2 `semantics` error (#1598). Explicit retains
the concrete elements and evaluates the selected predicate at depth 0. If that
predicate holds and an idle successor preserves the initial overflow, depth at
least 1 now returns the same capacity `semantics` error, before closure can
report `proved` (#1531); explicit's initial frame alone remains unguarded.
A reachable-only induction selection is still a usage error. Before #1519,
explicit rejected the same overflowing literal earlier with `sequence literal
exceeds capacity` at every depth; it now reaches the concrete initial bound
check. #1591 resolves the default pre-pass's separate step-1 versus explicit
step-0 discrepancy; #1519 did not change that label.

Before the budget, this pre-pass cloned its `Monitor` (the whole `KernelModel`
by value) and its trace per visited BFS node; a spec whose history-recording
`Seq` state defeats the BFS's `visited` dedup made the frontier grow like
branching^depth, driving the *default*, all-properties path (which alone runs
this pre-pass) past 11.4 GB RSS while every `--property`-narrowed run of the
same spec verified in seconds. `find_boundary_violation` now takes the model
by reference and reuses one scratch `Monitor` across the whole search, and
reconstructs a found violation's trace from a parent-link map only once
(rather than cloning `Vec<TraceStep>` per node) — the same pattern
`rust/fsl-runtime/src/explicit.rs`'s `verify_explicit_selected` already used,
lifted into `rust/fsl-runtime/src/trace.rs` so both share it instead of
duplicating it.

`CONCRETE_PROBE_BUDGET` (`rust/fsl-runtime/src/lib.rs`) is the default budget
every production caller passes — native (`rust/fslc/src/verification.rs`,
`rust/fslc/src/main.rs`'s weakening-counterfactual and
`mutation_oracle_for_model` call sites) and the browser Worker
(`rust/fsl-wasm/src/lib.rs`, sharing the same constant so native/Worker
strategy parity holds by construction). It is calibrated against a full
sweep of `specs/` + `examples/` at their default `--depth 8`: the observed
maximum `states_explored` was 23,409, every file reached ordinary BFS
closure well short of a 500,000-state sweep ceiling, and the constant keeps
a >=2.1x margin over that maximum (>=3x over every other corpus file) —
generous enough that no corpus spec's pre-pass is expected to exhaust, which
`rust/fslc/tests/corpus_check_sweep.rs` continues to guard by requiring every
non-error-declaring corpus file to check clean. `rust/fsl-runtime/tests/
boundary_probe_budget.rs` proves the cutoff is exact (a model whose bounded
reachable set is provably far larger than the budget still reports
`states_explored` equal to the budget, not more or less) independent of any
timing or resource-limit assumption; `rust/fslc/tests/
issue_697_all_properties_memory.rs` anchors the end-to-end contract this
section describes — joint-vs-isolated verdict/attribution agreement, the
fail-closed preservation above, a genuinely broken invariant still reporting
`violated`, and (Linux only, since macOS does not enforce `RLIMIT_AS`) a
generous address-space ceiling derived from this fix's own measured
baseline.

See `docs/design/DESIGN-explicit-engine.md` for `--explicit-budget`, the
in-repo precedent for bounding a solver-free BFS by state count rather than
by depth or wall time; that budget still governs only
`verify_explicit_selected` and is unrelated to this pre-pass beyond sharing
the same idea of an explicit `usize` cutoff surfaced in the result.

### Retained Set members and relation pairs (#1541)

An accepted `add` retains its computed member or pair even when it lies outside
the receiver's declared finite carrier. Symbolic values use the valid-state grid
plus guarded, distinct scalar candidates; membership, cardinality, extensional
equality, removal, domain/range and relation graph consumers see the actual live
values. This does not relax static element/endpoint typing or admit unbounded or
composite collection forms.

Literal member values, including enumerated action parameters, update existing
carrier slots without appending an entry; nominal bounded types alone are not
proof when state bounds are unselected. Conditional merges align known literal
keys by value. Symbolic search may equate a complete root with a fresh frame
whose finite support contains every live member/pair. For selected, already
asserted root bounds, the declared carrier supplies that proof without another
query. For unchecked roots, the candidate support includes the carrier and known
literal keys outside it; only an UNSAT query for a live key escaping that support
licenses re-framing. The query does not assume the omitted bound. SAT preserves
the complete open value, and solver unknown/backend errors are not treated as
proof. Re-framing applies only to top-level Set/relation state roots; other roots
remain unchanged. Invalid successors are checked/projected before re-framing.
Finite reachable support can therefore be compacted even under property selection;
genuinely growing support still grows rather than losing outside values.

Retention is paired with successor construction and evidence: complete pending
collection expressions enter successor frames instead of being equated to an
incapable closed grid, and projection preserves outside members. Relational init
still requires repeated assignments to agree. Its shared planner propagates only
fresh collection shape through the checked dependency order, including copies,
copy chains, conditions and relation pairs; it does not execute assignments or
propagate scalar values. Existing grid keys need no additional initial slots,
but computed/outside candidates and copied support must remain representable.
Concrete snapshots likewise must not discard their members. Static `check` and
concrete init's read-before-assignment policy are unchanged: a free scalar read
can remain an explicit/Monitor init error while symbolic engines expose an
initial bound violation. Bound checks retain their existing state order: restored
collection support can expose its bound before the free scalar's own bound.
For an accepted `add` on
the bounded receiver, storing its result back exposes `_bounds_<root>` rather
than removing the transition. Whole-collection assignment from a wider Set or
relation carrier remains unsupported by BMC/induction (`cannot coerce symbolic
value`, exit 2), as on base; this repair does not add cross-carrier coercion.
Bounded BMC/explicit success before that transition is legitimate; induction must
not promote it to an unbounded proof of a false bound or invariant.

Relations contribute implicit bounds consistently to the core obligation catalog,
induction, property selection and reports. Thus `--property _bounds_r` is available
for relation state `r`, `invariants_checked` includes `_bounds_r`, and a successful
induction proof records it in `k_used`. HTML's embedded verification inventory
changes accordingly. The existing `--exclude-property _bounds_*` rejection above
is unchanged. Regression and rejecting controls live in
`rust/fslc/tests/issue_1541_out_of_domain_members.rs`.

## Conformance corpus

`fslc conformance SPEC --depth N` enumerates every finite action instance at
each bounded reachable state and emits:

- the stable logical states (`s0`, `s1`, ...);
- successful successor states and field-level changes;
- disabled instances as `requires_failed`; and
- runtime/property failures with `state_changed:false` and the unchanged state.

The schema is `schemas/fslc/kernel/conformance.v1.schema.json`. The checked-in
input/golden pair
`rust/fslc/tests/fixtures/conformance_failures.{fsl,v1.json}` fixes v1 failure
semantics. `kernel_contract.fsl` covers domains, enums, structs, maps, sets,
sequences, options, quantification, simultaneous updates, requirements,
properties, partial operations, and terminal states. All ordering is
deterministic.

Conformance values preserve nested option identity with tagged
`{"kind":"none"}` and `{"kind":"some","value":...}` objects. This keeps
`none` distinct from `some(none)` during reachable-state deduplication. Other
scalar and collection values retain their ordinary JSON representation.

### Deterministic testgen trace

Target harness generation has a different operational contract from exhaustive
bounded exploration: it must retain the historical fixed-seed, at-most-100-step
walk without expanding the reachable state space or changing generated bytes.
`fslc_rust::testgen_trace_vectors` publishes that single path as
`schemas/fslc/kernel/testgen-trace.v1.schema.json` when the walk is
violation-free. It returns `TestgenWalk::Violated` instead of a trace document
when the walk reaches a violation (issue #843): `testgen-trace.v1` cannot
express a violating step, and the Monitor's rollback would otherwise be
published as that step's `expected`. No non-conforming document is ever
emitted, so the schema itself is unchanged. The envelope records its
schema version, the Public Kernel version, and spec identity. State values retain
ordinary Monitor JSON because they are embedded directly into generated
harnesses; this contract is therefore separate from the exhaustive corpus's
tagged Option representation.

The shared testgen adapter consumes Public Kernel v1 and this versioned trace,
then cross-checks full state fields, action names, and exact parameter sets.
Scenario expected states may be partial but cannot name fields outside the
Kernel contract. An incompatible or malformed trace fails before target
rendering. This resolves the Public Kernel/conformance-vector boundary required
by issue #214 without weakening the exhaustive corpus or changing legacy output.

## Conformance coverage matrix

Status: implemented by issue #223. The fixed fixture manifest
(`kernel_contract.fsl` at depth 2, `conformance_failures.fsl` at depth 1) is
comprehensive by construction, not by inspection: an external implementer
reading only the golden vectors has no mechanical way to tell which
documented kernel semantic, outcome kind, value encoding, or structural
feature the corpus actually demonstrates versus merely declares. The
coverage matrix answers that question directly.

`fslc_rust::coverage::coverage_matrix()` parses both fixtures, builds their
public Kernel contracts and conformance vectors in memory, and structurally
detects, for a fixed list of feature rows, whether each is:

- **exercised** — a concrete vector or state snapshot in the generated
  corpus demonstrates the feature (e.g. an `outcome.kind == "trans"` vector,
  a state snapshot encoding a `Map` value, a literal negative-operand
  division whose recorded result matches Euclidean, not truncating,
  division); or
- **declared** — the contract states the feature exists (a
  `partial_operations` entry, `fair: true`, a `terminal` expression) but no
  vector in the fixed fixture manifest can fire it. This level is reserved
  for features that are structurally unobservable in a single bounded
  transition vector — `terminal_deadlock` and `fairness_weak` are liveness/
  whole-trace properties with no finite-vector witness — and for the six
  `partial_op_*` rows, where a `partial_operations` declaration is the
  required bar and a firing vector is additional, non-required evidence.

No row is ever hardcoded to a fixed level: every detector inspects the
generated JSON (expression trees, `outcome.kind`, state snapshots) directly.

Collection/range aggregates are source and private checked-IR features, not a
new Public Kernel v1/v2 representation. Before export, finite `count`/`sum`
over Set, Seq, or range binders is normalized to existing `ite`, arithmetic,
membership, `.size()`, and `.at()` expression nodes. Seq expansion includes the
structural `index < size` guard for every capacity slot, so inactive slots are
not counted and partial-operation evidence remains guarded. Existing typed
aggregate JSON keeps its exact `{binding, domain, condition}` shape (`sum`
without a filter retains `condition: null`). Adding `{binder: ...}` to the
published `count`/`sum` kinds would be a representation change and therefore
requires a future Public Kernel major version.

For Set and relation-projection aggregates, normalization includes actual `add`
candidates, not just declared carrier values. Public Kernel v1/v2 export uses
existing scalar `let`/`var` nodes to capture candidates of collection let aliases,
including chained aliases and the previous binding when a name is shadowed.
Candidate scopes follow those captured values through guards, updates, ensures
and partial-operation enumeration; later scalar shadowing or state updates must
not recompute an earlier collection's candidates. Conditional candidates preserve
branch evaluation, and membership/distinctness guards count each live value once.
Consumers must process generated guards/lets in order. `min`/`max` around a sum
use the same repaired sum candidates and remain binary arithmetic, not new
collection aggregate kinds. No Public Kernel field or node shape is added.

`coverage_matrix()` is itself the enforcement gate: if any feature row falls
short of its required level, it returns `Err` naming every shortfall instead
of producing a matrix that silently under-reports coverage. This is how
issue #223's "warn about newly uncovered features" requirement is
implemented — as a loud test failure, matching this repository's existing
snapshot-test discipline, rather than a warning that can be ignored.

The feature rows cover: the eight `semantics` keys in
`kernel.v1.schema.json`; the seven `outcome.kind` values
`fsl_runtime::Monitor` can emit; eight value-encoding rows (int range, bool,
enum, struct, option, map, set, seq — a nested `Option<Option<_>>` tagged
`some(none)` is additional, non-required evidence for the `value_option` row,
since the fixed fixture manifest does not carry one; that shape is instead
covered by the dedicated `conformance_distinguishes_nested_options_and_guard_partials`
unit test); the six partial operations (`head`, `pop`, `at`, `index`, `divide`,
`remainder`); and `quantification` (`forall`/`exists`), `param_finite_domains`,
and `requirement_traceability`.

Outputs:

- `schemas/fslc/kernel/conformance-coverage.v1.schema.json` — the JSON Schema
  for the matrix, following the same draft 2020-12 / closed-object
  conventions as the Kernel and conformance schemas.
- `rust/fslc/tests/fixtures/conformance_coverage.v1.json` — the golden
  matrix.
- `rust/fslc/tests/fixtures/conformance_coverage.v1.md` — the same data
  rendered as a Markdown feature-by-fixture table
  (`fslc_rust::coverage::coverage_matrix_markdown`), for humans skimming
  which fixture backs which feature.

Regenerate both after an intended change:

```bash
FSLC_COVERAGE_UPDATE=1 cargo test -p fslc-rust --test conformance_coverage
```

`rust/fslc/tests/conformance_coverage.rs` enforces the coupled-change
discipline this matrix depends on:

- every `kernel.v1.schema.json` `semantics` key has a matching feature row
  (`semantics_schema_keys_are_all_registered_as_feature_rows`);
- every `outcome.kind` the corpus emits — independently regenerated from the
  fixtures, not read back from the matrix — is registered and exercised
  (`every_outcome_kind_the_corpus_emits_is_registered_and_exercised`);
- every feature row meets its required level
  (`every_feature_row_meets_its_required_coverage_level`, via
  `coverage_matrix()`'s own `Err`);
- the golden JSON/Markdown match (`coverage_matrix_matches_the_v1_golden_json_and_markdown`);
- the schema's `$id` matches `fslc_rust::coverage::COVERAGE_SCHEMA_ID`
  (`published_coverage_schema_id_matches_the_rust_api_constant`).

**Coupled-change rule**: adding a new `semantics` key, a new
`fsl_runtime::Monitor` violation kind, a new partial operation, or a new
value kind to the kernel/conformance contract requires adding a matching
feature row (with a real, non-hardcoded detector) to
`rust/fslc/src/coverage.rs`, adding fixture evidence for it if the required
level is `exercised`, and regenerating the golden matrix above — in the same
change, the same way a kernel-contract shape change requires regenerating
`kernel_contract.v1.json`.

Concrete/symbolic agreement is a separate mechanical gate:
`fsl_verifier::transition_matches_step` pins a Monitor pre-state, action
parameters, and successor into the symbolic transition relation. Tests require
every checked successful Monitor transition to be satisfiable and a deliberately
altered successor to be rejected. `transition_outcome_matches_step` additionally
checks that disabled actions have an actually false guard and distinguishes
representable successful, type-bound, invariant, transition, and ensures
outcomes in native evaluation order. It rejects malformed calls and state
shapes, corrupted attempted/committed states, and substituted outcome kinds.
Because the symbolic value evaluator totalizes partial operators and uses
unbounded integer terms, agreement uses a separate typed definedness predicate
that follows native short-circuit, statement-branch, guard, post-property, and
checked-i64 evaluation order. A non-partial claim is rejected when its reached
path is undefined; an untaken partial expression does not make a valid outcome
unsupported. The same backend-neutral traversal returns both full definedness
and the first reached partial operation for ordinary BMC. Ordered guards,
reached body expressions, and `ensures` classify only the six LANGUAGE.md
partial operations, while an earlier checked integer overflow or invalid finite
Map lookup remains a fail-closed evaluation error instead of being relabeled by
a later partial operation.
This keeps action-context `partial_op` classification inside the public
`verify_bounded*` boundary without adding a production dependency on
`fsl-runtime`. Reached reads of an empty zero-capacity sequence are classified
as `partial_op`; when no physical slots exist, ordinary BMC and induction
construct fresh, unobservable read terms, not the agreement-only fallback
values used by broader conformance predicates. Ranked `leadsTo` decreases
measures are the narrow exception: before initial, successor, or deadlock
measure evaluation, a path-sensitive preflight refuses only reached zero-slot
reads whose fresh values would otherwise enter the rank evaluation. It retains
the base semantics error (`head() on zero-capacity sequence` or
`cannot select from an empty finite domain`). Unreached conditional or
short-circuited reads can now prove by ranking where the base reported a
semantics error. That narrow preflight precedes the general reached
partial-operation measure probe (#1542), which also covers capacity-zero
push/pop and conditional-merge temporaries with physical slots. A possible
partial read in the rank-use region is refused as a semantics error rather
than used as rank evidence; see
[Seq partial operations](DESIGN-seq-partial-operations.md#symbolic-contract).
The BMC ranking fast path discharges nothing on either refusal, as for
other ranking errors.
Nonempty expression temporaries retain their live elements and normal read
semantics, independently of declared state capacity. Concrete conformance
vectors remain authoritative for exact rollback evidence that a
bounded symbolic representation cannot retain, such as an over-capacity
sequence suffix. Property-context totalization is unchanged.
The solver-free `fsl-runtime` dependency boundary remains intact.

## CLI and API

```bash
fslc kernel spec.fsl > kernel.json
fslc conformance spec.fsl --depth 4 > vectors.json
```

Both commands return one JSON document and use the normal native exit contract:
0 success, 2 usage (invalid argv)/parse/type/semantic/I/O error. `kernel` accepts source dialects that
reach the shared lowering gate and retain truthful source provenance; `compose`
is the explicit v1 exception described above. The output records the original
dialect and requirement origin.

DB lowering validates the closed compatibility-rule vocabulary before building
the catalog. An unknown rule now returns an authored `semantics` error (exit 2),
with its rule location, rather than exporting a catalog that silently omits that
obligation. The same boundary covers every consumer of DB lowering. This is not
full DB compatibility validation: DB-only findings and unreachable schema-window
errors remain with the DB tools and do not newly block `kernel`. The existing
unlocated specialized-validator envelopes of `check` and the Worker are unchanged;
other commands retain their established lowering-error rendering (including
message-only locations for `fmt`/`lint`/`migrate`).

Rust callers can use:

- `fsl_core::public_kernel_contract(&KernelSpec, &KernelModel, path, dialect)`;
- `fsl_core::public_kernel_contract_for_version(..., PublicKernelVersion)` for
  explicit v1/v2 negotiation (the legacy entrypoint remains v1);
- `fslc_rust::conformance_vectors(&KernelModel, depth)`;
- `fslc_rust::testgen_trace_vectors(&KernelModel) -> Result<TestgenWalk, String>`
  for the versioned, fixed-seed single-path harness contract; `TestgenWalk`
  separates a violation-free trace from a walk that reached a violation, so no
  caller can publish a rolled-back step as an expectation (issue #843);
- `fslc_rust::coverage::coverage_matrix()` and `coverage_matrix_markdown(&Value)`
  for the fixture corpus's feature coverage matrix (no CLI subcommand in v1;
  see "Conformance coverage matrix" above); and
- `fsl_verifier::transition_matches_step(...)` and
  `transition_outcome_matches_step(...)` for semantic agreement.

## Versioning and compatibility

The public Kernel schema follows SemVer independently of the FSL language and
CLI versions:

- patch: documentation/schema clarification that accepts exactly the same JSON;
- minor: additive optional fields only at schema locations explicitly declared
  extensible, or new explicitly extensible `kind` values;
- major: removal/rename, required-field addition, representation change, or any
  semantic change to evaluation, updates, failures, fairness, or properties.

Consumers must inspect `schema_version`, accept supported `1.x` schemas, and
reject an unsupported major. Objects marked `additionalProperties:false` are
closed in v1; adding fields there is a major change. Existing fields keep
their meaning for the whole major line. Deprecations remain emitted for at least
two minor releases and are documented before removal in the next major.
Conformance vectors, testgen traces, and public replay traces have their own
versions and record the Kernel schema version they exercise. Replay-trace v1 is
shared by Kernel v1/v2 because v2 adds provenance without changing execution
values; its complete tick/state contract is defined by
[`DESIGN-replay-trace.md`](DESIGN-replay-trace.md). A schema change requires coupled updates to the schemas,
golden vectors, Rust tests, this document, `docs/manual/LANGUAGE.md`, the shared skill,
and a `changelog.d/<id>-<slug>.<category>.md` fragment (see `changelog.d/README.md`) --
not a direct `CHANGELOG.md` edit, which is aggregated from fragments only at release time.

Replay observation actions execute through `Monitor::attempt`. Solver-free BFS
also uses Monitor transitions, while
`fsl-verifier/src/agreement.rs::{transition_matches_step,transition_outcome_matches_step}`
checks Monitor successors and failures against symbolic transition semantics.
Stutter performs no transition and requires full equality with the current
Monitor state, so it introduces no second runtime or solver semantics.
Replay schema 1.2 additionally runs the solver-free `BoundedLivenessMonitor` on
the same observations. Its inclusive `leadsTo ... within` deadline is
differentially checked against the native BMC deadline probe; requirements
`deadline` remains an ordinary lowered safety invariant.

Public Kernel v2 is the separately negotiated provenance publication contract
defined by [`DESIGN-kernel-origin-v2.md`](DESIGN-kernel-origin-v2.md). Use
`fslc kernel SPEC --kernel-version 2`; omission remains v1. Its companion
conformance and coverage schemas also report `2.0.0`. A v2 schema does not imply
complete provenance: `provenance.completeness` and assurance counts expose gaps.
Both majors continue to reject compose until component file identities are
truthfully retained.

## Distribution and downstream migration

Native releases attach independently checksummed `fsl-kernel-contract-v1` and
`fsl-kernel-contract-v2` bundles. The v1 bundle contains only v1 artifacts and
continues to contain all six v1 schemas (including the shared replay trace and domain scaffold
metadata bridge), the comprehensive input fixture and
its golden public Kernel JSON, the golden failure vectors, the golden
conformance coverage matrix
(JSON and Markdown), the replay positive/negative fixtures, and this guide. The
v2 bundle carries the same replay schema and fixtures. Target-specific generators remain
supported. General pytest, Vitest, Swift, Kotlin, Dart, and PHPUnit generators
completed their Public Kernel v1 input migration in
[#214](https://github.com/ymm-oss/fsl/issues/214); their shared adapter also
consumes scenario JSON and the versioned fixed-seed testgen trace. Compose retains an
explicit names/order-only producer because both public Kernel majors reject
incomplete multi-file provenance. All target emitters still consume the same
normalized adapter, and that producer is retired when truthful compose export
exists. Domain TypeScript, Python, Kotlin, Swift, and Rust scaffolds completed
their Public Kernel v1 input migration in
[#213](https://github.com/ymm-oss/fsl/issues/213). Because the closed Kernel
contract intentionally omits unused domain declarations, source expressions,
and saga/effect source topology, their shared adapter also consumes the
versioned `domain-scaffold-metadata.v1` public compatibility companion. The
adapter rejects incompatible versions and missing lowered member counterparts;
the companion is authoritative for topology that v1 cannot represent. It is supported for at
least two minor releases and may be removed only in a following major after an
equivalent external generator or negotiated public topology contract exists.
No target emitter receives `DomainSpec`, and `domain testgen` reuses the same
adapter/effect output. The native Rust typestate
analysis and TypeScript output migration completed in
[#215](https://github.com/ymm-oss/fsl/issues/215): the CLI now feeds public
Kernel JSON v1 to the generator, rejects incompatible schema versions, and no
longer uses the private-model adapter. The frozen Python reference is unchanged.
