# Requirements stage access

Status: accepted

## Contract

Business and requirements expressions use one structural source node:
`Expr::Stage { process: Option<SymbolPath>, entity, ... }`. The node must be
eliminated before checked Kernel construction. Concrete evaluation, symbolic
evaluation, and public Kernel export reject an unlowered node.

The resolver tracks typed parameters and binders. `stage(c)` selects every
process whose final entity type is `c`'s type. One candidate lowers to the
generated process stage-map index. Zero candidates and non-entity/unbound
arguments are type errors. Several candidates are ambiguous and list their
paths. `<path>.stage(c)` filters by the exact structural `SymbolPath`.

Process declarations accept the same path representation. An unqualified
`process Claim` retains the existing generated names. Qualified declarations,
such as `process claims.Claim`, share the entity type `Claim` while generating
path-distinct enum and state symbols.

The resolved map value supplies the expected stage enum, so ordinary Kernel
type checking validates the compared member. Requirements `terminal` remains
explicit; process sinks are not promoted to terminal states.

## Origin and presentation

Lowering binds the generated stage map to its process declaration and binds
each source stage access to its containing Kernel target. Public Kernel v2
therefore contains the lowered state symbol plus a `resolve_stage_access`
origin step and source span. Explain and violation formatting structurally
recognize generated stage-map origins and render `stage(c)` without changing
the checked expression.

Acceptance expressions use the same resolver before Monitor replay. No
dialect-specific runtime implementation or generated-name source fallback is
permitted.

## Acceptance expectation typing (#1439)

Requirements `expect <expr>` and the FSL 5 scenario `then <expr>` spelling
share the acceptance contract. After stage resolution, the existing recursive
expression checker validates the predicate with expected `Bool` against the
lowered model. This checks operands and both conditional branches, not just the
outer comparison's inferred Bool type. For `c: Color`, both `c != 1` and `c == 1`
are rejected before replay with `kind: "semantics"`, exit 2 and the existing
`expression of type Int is not assignable to Named("Color")` diagnostic.
The origin is the authored `expect`/`then` keyword, not the trace declaration.

The lowering check uses a temporary checked model; an invalid model is left to
the ordinary model-building diagnostic path, as with finite-domain checks.
No expectation is inserted into Kernel properties or the public schema.
Expectations remain post-state predicates: `c == Blue` can pass after painting
an initially Red state, and a well-typed false predicate retains the replay
`kind: "acceptance"` failure. Stage-form expectations and forbidden's literal
`expect rejected` grammar are unchanged. Enum/Int compatibility and concrete
equality semantics are not broadened.

### Measured compatibility

The following measurement is for the expectation-only fix; the trace-step
extension has its own measurement below.

BASE was materialized with `git archive` at
`5a539bbe27dc933d7fe8df95e6189cb75cec6cc9`. HEAD is the commit introducing
this note. The measured production patch has SHA-256
`a1452e647639f8ce471470df759c48915e779ec17b9c3e4dcef2c778a0d82686`.
This identifies the production change in `rust/fsl-core/src/dialect.rs`;
test-only revisions do not change that measured implementation.
The BASE binary SHA-256 was
`d4f8ada816cc6367d318cd35732d6a08a27b617f11ade6c803555ced0a333124`;
the measured HEAD binary SHA-256 was
`9a29d76a277275a7a2376808bee9e525fc1f0fc14b16fba2eccdf499eb408c53`.

Inventory command: `rg --files specs examples rust/fslc/tests/fixtures -g '*.fsl'`.
In each tree's own directory its binary ran `check FORM` on all 579 forms,
in both published and gated FSL 5 modes, with two complete repeats under one
shared heavy-work lock: 4,632 checks. Full parsed JSON, stderr and process exits
were identical between BASE/HEAD and repeats; no fields were excluded.
The measured ok-to-error transition set is **empty**, and there are no other
changes. This paragraph covers the expectation-only fix and its round 1
revisions only, not the trace-step extension or later review revisions. No
required tests were renamed, removed or stopped in that phase. Its test changes
add detectors and make the Worker test oracle honor its request's parser
options. The empty set is a corpus measurement, not a claim about every inline
source of those rewritten tests; those sources are measured separately in the
extension round 1 supplement below.

The post-fix `c != 1` consumer control was also run twice: `check`,
`analyze --profile ai-review`, and `document generate` all return
`result: "error"`, `kind: "semantics"`, exit 2. Analyze retains the same
message/location as check but omits its versions; document reports the location
in its message only, without check's structured `loc`, file label or versions.
These existing consumer-envelope differences are observed, not repaired here.

## Trace step argument typing (#1439 extension)

The same shared requirements lowering path checks every argument expression in
acceptance steps and forbidden setup/final steps. FSL 5 scenario and forbidden
calls, phrase `with` inputs, particle inputs, hybrid marked/`with` inputs and
relation-role values all lower to these argument fields. Stage resolution
precedes the existing recursive expression checker. Conditions, nested operands
and both conditional branches are checked, including an unselected branch.

Unlike a Bool expectation, an argument has **no outer expected parameter type**
in this check. Its internal expression must be well-typed, but final-value kind,
declared-domain membership and values-scope replay classification remain
unchanged. For example, `paint(if c != 1 then Red else Blue)` with `c: Color`
is a check-time `semantics` error / exit 2 even if a guard would reject Red.
The existing assignability message is anchored to the **first authored token of
the step**: the action name for calls, the actor for actor-first phrases, or
the first token of a reordered particle phrase (for example `claim1` in
`claim1 を Employee が ... で submits`). This is not a successful forbidden
rejection. In contrast, internally well-typed `paint(1)` and
`paint(if true then 1 else 2)` retain the forbidden `wrong_kind_argument`
diagnostic. Well-typed arguments excluded only by a values override retain their
replay skips. No runtime equality, compatibility rule, Kernel schema or replay
evidence format changes.

### Extension compatibility measurement and breaking removals

BASE is `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9` in its materialized archive.
HEAD is the commit introducing this extension, measured before commit on that
same base plus the complete dialect.rs production patch SHA-256
`aab6e87cae34da2a9a11e2161698164ee5fda4caa8ce4a5abd8c9702eb28273e`.
The BASE binary remains the coordinator-built binary identified above; HEAD
binary SHA-256 is
`6d57532041b90403815f2e6f40f34fcb0dd85900853221cb56dbfaf889e8a26d`.

Inventory: `rg --files specs examples rust/fslc/tests/fixtures -g '*.fsl'` in
each tree. Each binary ran `check FORM` from its own tree with relative paths,
both with `FSLC_UNSTABLE` removed and set to `fsl5`, twice under one heavy lock.
The 579 corpus forms per tree have an **empty ok-to-error transition set** and
no other full-output differences. A lint-only delimiter rewrite of
`issue_1439_trace_step_types.rs` also requires checking all its inline fixtures:
21 per tree (20 distinct source texts; the values-override test reuses the legal
FSL 5 source). The helper templates and argument strings were mechanically
expanded without changing their FSL content. Total: 600 forms per tree, 4,800
checks. Full stdout bytes/JSON, stderr and exits were compared; no exclusions
or repeat differences.

The original inline fixtures expose **16 breaking removals**, T01–T16 below.
T17–T21 belong to the separately measured round 1 supplement. Each original form
formerly returns `ok` / exit 0 and now returns `error` / `semantics` / exit 2
with the internal assignability mismatch and authored step location. Published
forms transition in both modes; gated forms transition only in FSL 5 mode:
21 mode/form transitions, no additional changes. This is a measured removal of
ill-typed trace expressions, not a removal of a valid parameter value. Repair
the comparison to use a member of the same enum, including in unselected
branches; do not substitute numeric ordinals or weaken the forbidden guard.

| Removal | Regression test / measured source | Position / malformed expression |
|---|---|---|
| T01 | `acceptance_step_rejects_ill_typed_color_condition` | published acceptance Color argument, `c != 1` condition |
| T02 | `forbidden_setup_step_rejects_ill_typed_color_condition` | published forbidden setup Color argument, `c != 1` |
| T03 | `forbidden_final_step_rejects_ill_typed_color_condition_s1` | published forbidden final S1, `paint(if c != 1 then Red else Blue)` |
| T04 | `fsl5_scenario_call_rejects_ill_typed_stage_condition` | FSL 5 scenario call, `claim_stage[0] != 1` |
| T05 | `fsl5_scenario_input_rejects_ill_typed_stage_condition` | FSL 5 scenario with input, same malformed stage comparison |
| T06 | `fsl5_scenario_marked_input_rejects_ill_typed_stage_condition` | FSL 5 scenario particle input, same comparison |
| T07 | `fsl5_forbidden_call_setup_rejects_ill_typed_stage_condition` | FSL 5 forbidden call setup, same comparison |
| T08 | `fsl5_forbidden_call_final_rejects_ill_typed_stage_condition` | FSL 5 forbidden call final, same comparison |
| T09 | `fsl5_forbidden_input_setup_rejects_ill_typed_stage_condition` | FSL 5 forbidden with setup input, same comparison |
| T10 | `fsl5_forbidden_input_final_rejects_ill_typed_stage_condition` | FSL 5 forbidden with final input, same comparison |
| T11 | `fsl5_forbidden_marked_setup_rejects_ill_typed_stage_condition` | FSL 5 forbidden particle setup input, same comparison |
| T12 | `fsl5_forbidden_marked_final_rejects_ill_typed_stage_condition` | FSL 5 forbidden particle final input, same comparison |
| T13 | `acceptance_numeric_step_rejects_ill_typed_color_condition` | published numeric-result argument, `setn(if c != 1 then 2 else 0)` |
| T14 | `fsl5_marked_with_input_rejects_ill_typed_stage_condition` | FSL 5 marked/with hybrid input, malformed ClaimStage comparison |
| T15 | `fsl5_relation_role_input_rejects_ill_typed_stage_condition` | FSL 5 relation-role value, `ticket_stage[0] != 1` |
| T16 | `acceptance_step_checks_unselected_conditional_branch` | published unselected nested branch, `paint(if true then Blue else (if c != 1 then Red else Blue))` |
| T17 | `cli_and_lsp_source_diagnostics_share_identity_without_changing_cli_envelopes` / acceptance-expect-type | published `expect c != 1` diagnostic row |
| T18 | `check_error_envelopes_match_native_across_parse_guard_and_name` / acceptance expectation type | the same `expect c != 1` source in the rewritten Worker test |
| T19 | `trace_argument_rejects_pattern_binding_outside_its_logical_continuation` | `paint(if o is some(z) then z else Red)`, with `o = none`; `z` is outside its scope |
| T20 | supplemental expectation scope control | `expect (if o is some(z) then z else Red) == Red`, with `o = none`; the same scope escape |
| T21 | `reordered_particle_argument_error_starts_at_the_instance_token` | `claim1 を Employee が amount (if claim_stage[0] != 1 then 1 else 2) で submits`; first-token location is `claim1` |

### Extension round 1 supplemental inline measurement

No production changes accompany this supplement. Against BASE
`5a539bbe27dc933d7fe8df95e6189cb75cec6cc9`, the unchanged HEAD implementation
has the production-patch identity above. The explicitly rebuilt HEAD binary
has SHA-256
`5f1f55ea8e30eee1abb0666a5ebe38c66c96f0244e933fc0e899763440df1733`;
the BASE binary identity is unchanged. Each binary ran `check FORM` in its own
tree, both with the FSL 5 gate removed and enabled, twice under one lock.

The supplement includes every source occurrence of both rewritten diagnostic
tests: seven native/shared cases and six Worker cases (including all four
`include_str!` fixtures). It also checks the three existing Worker tests whose
shared native oracle changed: duplicate writes, unproved write distinctness,
and verify-build errors. Two new regression fixtures and the matching expect
and invariant scope controls complete **20 forms per tree / 160 checks**.
Full stdout bytes/JSON, stderr and exits repeat exactly, with no exclusions.

T17–T20 each transition in both modes; T21 transitions only with FSL 5 enabled:
**five fixture occurrences / nine mode-form transitions**, with no other
changes. T17 and T18 contain identical FSL, so this is four distinct newly
recorded source texts. All transition from `ok` / 0 to `error` / `semantics` / 2.
T17/T18 retain the Int-to-Color message at `expect` (10:5); T19/T20 report
`public Kernel cannot type identifier 'z'` at the step (9:5) / `expect` (10:5).
T21 reports the ClaimStage mismatch at `claim1` (8:5), not the actor later in
the line. The 579-form corpus measurement and original T01–T16 measurement
above remain separately identified historical observations, not rerun claims.

T19/T20 are intended breaking removals: an `is some(z)` binding is available
only in its true logical continuation (`=>` / `and`), not a conditional's
`then` arm. Invariants already reject that form on BASE and HEAD. An unselected
branch does not legalize the out-of-scope name. Move the use into a supported
logical continuation or use an expression that does not escape the binding.
This does not introduce conditional-arm binding semantics or change the shared
type checker. The new step and reordered-particle detectors both fail on
`ok` / 0 when only the extension is reverted, and pass after exact restoration.

The five preservation tests remain unchanged in meaning: legal published and
FSL 5 expressions, literal/conditional wrong parameter kind, and values-override
skips. The isolated extension revert produces 16 intended detector failures and
five preservation passes; exact restoration makes all 21 pass.

The S1 consumer control ran twice for `check`, `verify --depth 0`,
`scenarios --depth 0`, `testgen --depth 0`, `analyze --profile ai-review`,
`document generate` and `mutate --depth 0 --max-mutants 1`. All refuse the
malformed baseline with `error` / `semantics` / exit 2 before emitting scenarios,
tests, a document or mutation results. Mutation's trace replay oracle is not
reached for this malformed baseline; no claim about every external mutant is
inferred. Document still carries the position only in its message, as above.

## Unary operand typing (#1439)

The shared recursive model-expression checker validates `not` with expected
operand type `Bool` and unary minus with expected operand type `Int`, accepting
the existing numeric-range compatibility. This is not limited to requirements
traces: every model-expression position using this checker inherits the rule,
including invariants, guards and effects. Both conditional branches and both
operands of `and` / `or` are checked regardless of concrete evaluation.
There is no truthiness conversion or enum-to-integer conversion. Result inference
is unchanged (`Bool` for `not`, `Int` for unary minus): inference chooses a result
type, while recursive validation enforces operand types and existing origins.
No new diagnostic category, parameter-kind coercion, scope rule, runtime or
Kernel schema is introduced. Existing post-state, wrong-kind and scope behavior
above is preserved. Option pattern shapes, `abs`, relation signatures/endpoints,
method receivers and arity are outside this fix (follow-ups #1458 and #1459).

### Unary compatibility measurement and breaking removals

BASE is `5a539bbe27dc933d7fe8df95e6189cb75cec6cc9`, materialized under
`.claude/work/base-tree`. HEAD is that same commit plus the uncommitted final
implementation measured for this note, not a claimed new commit or remote main.
The combined production diff for `dialect.rs` and `typecheck.rs` has SHA-256
`9b04a4ca9e9ea9ba4dfe8c880d81b9ff4a028020c96be921779a3e6416e07d43`;
the isolated unary patch has SHA-256
`8e6d6c9e64d9368964786cba8af4b7447269e0d4d92a9582ae1b9db18614064c`.
The approved BASE executable is the coordinator-built binary identified above,
SHA-256 `d4f8ada816cc6367d318cd35732d6a08a27b617f11ade6c803555ced0a333124`.
The final workspace build produced HEAD `rust/target/debug/fslc`, SHA-256
`ed375b54aea571629ab3cd3f1e7d0e07c4a5b8b6aac034c3d871eff4908eb3fd`.

Inventory in each tree:
`rg --files specs examples rust/fslc/tests/fixtures -g '*.fsl'`.
The fresh sweep command is
`./scripts/cx-log d1439-tc-v4-sweep -- python3 .claude/work/typecheck-sweep-1439.py`.
For every form, each binary runs from its own materialized tree using
`lockf -k HEAVY_LOCK BINARY check FORM` (the configured advisory heavy-work lock);
the lock covers **one invocation**, not the sweep. `FSLC_UNSTABLE` is explicitly
removed and then set to `fsl5`, with two complete repeats in each mode/tree.
The task-local lock path is an execution detail, not a language requirement.

Both independent inventories contain **579 corpus forms**. The measured corpus
ok-to-error transition set is **empty**, with no other corpus output changes.
The sweep also mechanically expands all ten fixtures of
`issue_1439_unary_operand_types.rs`, the 21 fixtures of the lint-rewritten step
test and the 20 inherited rewritten-diagnostic/oracle/scope fixture occurrences:
**51 inline occurrences per tree, 630 total forms per tree, 5,040 invocations**.
Every stdout byte, parsed JSON value, stderr and exit is compared in full;
no fields are excluded and all repeat pairs agree.

T01–T21 above remain the inherited removals (30 mode/form transitions).
The following seven additional forms are **breaking removals**, each `ok` / 0
on BASE to `error` / `semantics` / 2 on HEAD in both modes: **14 new mode/form
transitions, 44 total**, with no other full-output differences. U01–U07 identify
the exact inline sources of `rust/fslc/tests/issue_1439_unary_operand_types.rs`:

| Removal | Regression test | Malformed form |
|---|---|---|
| U01 | `step_argument_rejects_not_integer_in_unselected_branch` | forbidden `paint(if true then Red else (if not 1 then Red else Blue))` |
| U02 | `step_argument_rejects_enum_minus_in_unselected_branch` | forbidden `paint(if true then Red else (if -Red > 0 then Red else Blue))` |
| U03 | `step_argument_rejects_not_integer_behind_short_circuited_or` | forbidden `paint(if c == Red or not 1 then Red else Blue)` |
| U04 | `expectation_rejects_not_integer_behind_short_circuited_or` | acceptance `expect c == Blue or not 1` after `paint(Blue)` |
| U05 | `invariant_rejects_not_integer` | invariant `c == Red or c == Blue or not 1` |
| U06 | `invariant_rejects_enum_minus` | invariant `c == Red or c == Blue or -Red > 0` |
| U07 | `expectation_rejects_not_integer_behind_short_circuited_and` | acceptance `expect not (false and not 1)` after `paint(Blue)` |

The `not` diagnostic contains `expression of type Int is not assignable to Bool`;
the minus diagnostic contains
`expression of type Named("Color") is not assignable to Int`. The fixed tests
pin authored locations: step 12:5, expectation 13:5, invariant 10:3. Replace
the invalid operand with a Bool or numeric expression as appropriate, even in
skipped expressions. Well-typed `not (c == Red)`, `-(-1)` and numeric `- n`
remain accepted in both binaries/modes. Isolating only the unary patch's revert
produces exactly seven original false-green detector failures and three passing
preservation controls; exact restore makes all ten tests pass again. No required
test is rewritten, renamed, removed or stopped in this unary phase.
