# FSL — `forbidden` (negative acceptance criteria / must-forbid) implementation design

Motivation: issue #3 (category 4/6 of validation roadmap #1). `acceptance` (must-allow)
re-checks at check time that "this operation sequence passes," but there was no way to express
that "this operation sequence **must be rejected**" (must-forbid). **Under-constraint** such as
a missing guard accepts an "operation that should be forbidden" without breaking a single safety
invariant, so verify stays silent about it. `forbidden` is an independent channel that breaks
that silence (a receiver for cross-validation in which a separate agent writes positive and
negative traces from the NL and action signatures alone).

## 1. Syntax (requirements dialect)

```fsl
forbidden FB-1 "cancellation after shipping is rejected" {
  pay(0)  ship(0)        // premise (setup): all enabled and ok
  cancel(0)              // last step: expected to be rejected
  expect rejected
}
```

A copy of `acceptance_def`. `expect rejected` is an inline marker (unlike `acceptance`'s
`expect <expr>`, it does not evaluate a state predicate). `FB-1` matches the `REQ_ID` token.

## 2. Semantics (concrete Monitor replay, at check time)

- The premise steps `steps[0..n-2]` must all be `ok` (enabled and no violation).
- Success if and only if the **last step is rejected**, that is, **not enabled**:
  - `requires_failed` — every argument lies in its parameter domain and a `requires` guard
    refuses the call. A "correct prohibition by a guard" that is invisible as a safety
    invariant;
  - `bad_call` — no action of that name and arity has every argument in its parameter
    domain (DESIGN-bridge.md 1.2), so no guard is ever evaluated.

  The scenario's `rejected_by` names which one, and the generated negative test asserts that
  kind, so an out-of-domain call is never asserted as a guard refusal (issue #1212). The Monitor's
  `bad_call` in DESIGN-bridge.md 1.2 also covers an unknown action or a missing parameter;
  for a forbidden last step those are not rejections (below), so here `bad_call` means only
  an argument outside the parameter domain.
  - The parameter domain is the **checked value domain**, not always the declared type. For
    a range type (`0..100`, `type Amount = 0..100`) or an enum it is the declared type. For
    an `entity` it is the `verify { instances E = N }` scope, and for a `number` it is the
    `verify { values N = lo..hi }` scope: `accept(7)` under `instances Case = 3`, or
    `add(9)` under `values Qty = 0..3`, is a `bad_call`. Such a last step satisfies the
    forbidden **without evaluating any guard**, although an implementation, which has no
    such bound, may accept that call. Issue #1229 tracks changing this `check` verdict for
    `entity` / `number`; until it lands, a forbidden that is meant to test a guard must use
    an argument inside the checked scope. `fslc diff` already does not treat such a
    `bad_call` as a rejection: it preserves a forbidden that OLD and NEW both reject as
    `bad_call` only when, on both sides, every same-named action of that arity has an
    argument outside a range or enum parameter type; a `bad_call` decided by an `entity` /
    `number` scope (including one that the NEW scope introduces when OLD is replayed
    under it) stays `unknown` / `forbidden_step_unrelatable` (DESIGN-semantic-diff.md).
- **Strict default (issue #1213, breaking).** A last step that is enabled and then
  stops with a runtime **violation** (`invariant` / `trans` / `ensures` / `type_bound` /
  `partial_op`, every kind the Monitor reports for an enabled step) is **not** a rejection:
  the guard admits the call, and only a reachable spec bug stops it.
  It is a `kind: "forbidden"` error carrying `failed_step`, `step_results: []` (as every
  error about one step), `violation` (`{kind, name, action, params}`), `accepted_trace` (the
  setup steps), the `state` before the last step (the Monitor does not commit a violating
  step), and a hint. Before #1213 this case counted as satisfied with `rejected_by` set to
  the violation kind ("forbidden is satisfied but the spec is buggy"), so a forbidden could
  pass while the very guard it states was missing — the opposite of "the strength of a claim
  is its weakest evidence" and of failing closed.
  - Migration — the forbidden replay is a gate that every caller of it listed in §2.1 runs **before**
    BMC, induction, or scenario generation (§3), so not only `check` changes:
    - `verify` on such a spec now stops at the gate with `error` / `kind: "forbidden"`
      (exit 2) whatever its engine or `--depth`, and **no BMC or induction trace is
      produced**; the `violation`, `accepted_trace`, and `state` of the error are the only
      trace. Before, the spec passed the gate and `verify` reported whatever its engine
      reached: with BMC, `violated` / exit 1 when `--depth` reaches the violation, but
      `verified` / exit 0 when the violation needs more steps than `--depth`; with
      `--engine induction`, its own verdict (`violated`, or `unknown_cti` when the
      violation is far away). So the exit status moves from **0 or 1 to 2**, and a CI job
      keyed on exit 0, exit 1, or `result: "violated"` is affected.
    - `sweep`, `chain`, `mutate`, `html`, and `ledger` move from 0 or 1 to 2 the same way,
      but only `sweep` and `mutate` print the same `error` / `kind: "forbidden"`: `chain`
      prints `kind: "chain"` with `failed: ["requirements"]` and that error as the layer's
      `detail`, and `html` and `ledger` print the generated result (`result: "generated"`)
      and still write their report, with the verification `not_run`: `html` with the
      forbidden hint, `ledger` with the violation under `FB-1`.
      `counterexample export` exits 2 with that error and without a reproducer
      (before: exit 1 with one when its depth reached the violation, otherwise exit 2 with
      nothing to export). `approval create --kind scenarios` / `html` exits 2 without a
      record (before: exit 2 on `violated`, otherwise a record); `--kind scenarios` prints
      that error, and `--kind html` the generated `html` result. `approval check` of
      such a record created before exits 2.
      `scenarios` and `testgen` exit 2 with no scenario and no test file. `scenarios` used
      to exit 1 (`violated`) when its depth reached the violation, and otherwise exited 0
      and emitted the forbidden scenario with `rejected_by: "invariant"`; `testgen` exited 1
      (`violated`) without a test file on every spec measured in §2.1, even at `--depth 1`,
      because its own check reached the violation (`checked_to_depth: 21` on the
      eleven-step spec).
    - Three commands do not exit 2. `explain` still exits 0, but where `scenarios` used to
      succeed its `witnesses` (and the `html` witnesses section) become empty (a known gap
      tracked by issue #1242). `approval create --kind ledger` exits 0 with a record of a
      ledger that lists the error: when its depth reached the violation it used to exit 2,
      only because that ledger embeds wall-clock `elapsed_s` and so never matched a fresh
      rendering, and otherwise it exited 0 (not folding the verdict into the status is
      the #592 design; whether a ledger whose forbidden gate fails may be approved is
      tracked by issue #1243). `approval check` of a ledger record created before reports
      `drifted` (exit 0). §2.1 has the measured exit status and output of each command
      before and after.
    - `fslc mutate`: the requirement oracle (`apply_requirement_mutation_oracle`) runs on
      mutants the bounded oracle leaves clean. Such a mutant whose forbidden last step is
      now enabled and violates is killed with `killed_by: "forbidden"` (and the forbidden's
      requirement IDs as `killer_requirements`) where it used to survive, so kill rates
      and the `killed_by` distribution can shift toward `forbidden`.
    - The fix for an affected spec is the `requires` that rejects the call — the guard the
      forbidden states — keeping the invariant as a separate safety property. The corpus
      sweep is recorded in §6.
  - No opt-out (`expect rejected_or_violated` or similar) is provided. A forbidden claims
    that the call is rejected; a violation reached by an enabled last step is evidence that
    the rejecting guard is missing, never that it exists, so the lax reading let `check`
    (which runs no BMC) pass and let testgen assert a violation kind as a rejection. The
    strict error itself reports that violation (kind, name, action, arguments, setup trace,
    state before the last step). It does not report what `verify` used to report after the gate: the
    verdicts of the other properties, the shortest counterexample (which can be shorter
    than the forbidden's trace or reach a different violation), and the `counterexample
    export` reproducer only come back once the forbidden or the guard is fixed. The corpus has no
    forbidden that relies on the lax reading. The change errors at once rather than being
    staged by `--edition` (DESIGN-migration.md): an edition stages a syntax change whose
    canonical rewrite leaves the checked model unchanged (`migrate` compares Public Kernel
    JSON before and after), while the fix here adds a guard and so changes the model; keeping
    the lax reading for a release would keep a fail-open verdict, and §6 finds no corpus
    forbidden that the change affects. Re-evaluate only if a real spec needs a
    must-forbid whose only rejection is a runtime check that cannot be stated as a guard.
  - `fslc diff` follows the same rule: a NEW final step that is enabled and then violates is
    `forbidden_relaxed`, and an OLD final step that violates
    is `unknown` / `forbidden_replay_failed` (OLD never rejected it). A NEW setup step that is
    enabled and then violates is `unknown` / `forbidden_replay_failed` too: NEW never reaches
    the final step, so it neither preserves nor relaxes the OLD rejection. A NEW setup step
    that its guard disables never reaches the final step either, yet `fslc diff` still
    reports it as preserved: a known gap tracked by issue #1239 (DESIGN-semantic-diff.md).
  - The witness of a violation-carrying `forbidden_relaxed` differs from an accepted one:
    the final step was rolled back, so `trace` ends at the last setup step and `state` is the
    state before the final step; `violation` (`{kind, name}`) names what stopped it, and
    `accepted_step` still names the final step's action.
  - The frozen Python reference (`src/fslc/acceptance.py`) keeps the reading from before #1213; it
    is not the product surface (AGENTS.md "Authority and scope").
- A last step whose `requires` cannot be evaluated in the state the setup reaches (a partial
  operation such as a division by zero in the guard) is not a rejection either: no guard
  decided the call. Today the replay stops with `error` / `semantics` (issue #1191). A fix
  for #1191 must keep this case out of `requires_failed`: reading an undefined guard as
  disabled would satisfy the forbidden without a decided guard (fail-open), and would let
  `fslc diff` preserve an OLD rejection that a NEW undefined guard never decided. The
  regression tests `a_final_step_whose_guard_is_undefined_does_not_satisfy_the_forbidden`
  and `diff_does_not_preserve_a_forbidden_whose_new_guard_is_undefined` pin both; they
  require only that `check` is not `ok` (non-zero exit) and that `diff` does not preserve the
  forbidden, so a #1191 fix may change the error's shape.
- A last step that names no action, or no variant of that arity, is not a rejection: it is a
  `kind: "forbidden"` error carrying `failed_step` and `message` (`unknown action '<name>' in
  forbidden` / `arity mismatch for action '<name>' in forbidden`), so a typo cannot satisfy
  `expect rejected` vacuously (issue #1212, frozen-Python parity). `fslc diff` applies the
  same rule to the OLD side: such an OLD last step is `unknown` / `forbidden_replay_failed`,
  never an OLD rejection that a NEW guard could preserve.
  - **Breaking (ok → error):** before this change such a step satisfied the forbidden, so
    `check` passed; what each other command reported depended on the rest of the spec, and
    for `scenarios` and `testgen` also on whether the step names an unknown action or a
    wrong arity (§2.1 has each command's measured exit status and output).
    `check`, `verify`, `sweep`, `chain`, `scenarios`, `testgen`, `mutate`, `counterexample
    export`, `html`, `ledger`, and `approval create --kind scenarios` / `html` now exit 2,
    and none of them reports what the rest of the spec reaches (a verdict, counterexample,
    scenario, test file, mutant, or reproducer), but their stdout differs. `html`,
    `ledger`, and `approval create --kind html` print the generated result (`result:
    "generated"`) and carry the error inside the report (`html` and `ledger` still write
    theirs; `approval create --kind html` creates no record). `chain` prints `kind:
    "chain"` with `failed: ["requirements"]` and the error as that layer's `detail`. The
    others print `error` / `kind: "forbidden"` with that `message`. `approval check` of a
    `scenarios` or `html` record created before exits 2. Three commands do not exit 2:
    `explain` still exits 0, with `witnesses: []` (a known gap tracked by issue #1242);
    `approval create --kind ledger` exits 0 with a record of the ledger, which now lists the
    error — on a spec whose rest has a violation it used to exit 2, only because that
    ledger embeds wall-clock `elapsed_s` and so never matched a fresh rendering (not
    folding the verdict into the status is the #592 design; whether a ledger whose
    forbidden gate fails may be approved is tracked by issue #1243); and `approval check`
    of a ledger record created before reports `drifted` (exit 0). Fix
    the step's action name or arguments. Corpus sweep (#1212): no `.fsl` under `specs/`,
    `examples/`, `rust/fslc/tests/fixtures/` or `tests/fixtures/` has such a step (§6).
- `loc`: an error that carries `failed_step` (`forbidden_setup`, and a `kind: "forbidden"`
  error about one step) is located at that step; the accepted-step error (`accepted_step`)
  is located at the `forbidden` declaration (frozen-Python parity).
- Last step `ok` (= accepted) → `kind: "forbidden"` error + `accepted_trace`.
- A premise step is not `ok` → `kind: "forbidden_setup"` (the trace is malformed; not treated
  as success).
- Zero steps → an error (at least one step is required). Native `fslc` reports
  `kind: "semantics"` with `message` `forbidden '<ID>' must have at least one step` and no
  `loc`; the frozen Python reference reports `kind: "forbidden"` located at the declaration.

### 2.1 Migration by command

The forbidden gate is `validate_requirement_trace_contract`
(`rust/fslc/src/verification_output.rs`), and the `diff` judgment is `forbidden_diff_findings`
(`rust/fslc/src/main.rs`). Their callers, followed through the non-test call graph to the CLI
dispatch, are:

- the gate: `check` (`check_stages.rs`); `verify` on every engine, `sweep`, and the layers of
  `chain` (`prepare_cli_verification_from_source` / `run_verify_from_source`); `scenarios`,
  `testgen`, and `explain` (`run_scenarios_mode_from_source`; `explain` discards that exit
  status and reads only its `scenarios` array, so a gate error leaves it at exit 0 with
  `witnesses: []`, and `html`, which embeds `explain`, with an empty witnesses section — a
  known gap tracked by issue #1242);
  `html` and `ledger`; `approval create` / `check` with `--kind scenarios`, `html`, or
  `ledger`, which render that artifact (the ledger rendering for approval does not fold
  the verification into its status, so `--kind ledger` exits 0 with a record of a ledger
  that lists the error, whether or not the rest of the spec verifies — issue #1243; `--kind
  requirements_document` renders no verification and never runs the gate);
  `counterexample export`; `mutate` (the baseline `verify` and
  `apply_requirement_mutation_oracle`); and the Worker's `check` / `verify`
  (`rust/fsl-wasm/src/lib.rs`). `db check`, `compat check`, `domain check` / `testgen`, and
  `ai check` reach it only with a `dbsystem` / domain / `ai_component` document, and
  `fsl_core::requirements_trace_contract` returns `None` for every document that is not a
  requirements document, so they cannot change;
- the `diff` judgment: `diff`, `diff --git`, and `approval diff` (which runs `diff` with no
  `--forbid`, so its exit stays 0).

Measured with debug `fslc` binaries (`cargo build -p fslc-rust --bin fslc`), each built in
its own materialized tree: main at `458040f3`, #1212 on top of it, and #1213 on top of #1212.
The "before" classification is also the latest release's. The tag `v4.8.1` is not an
ancestor of `458040f3`, so each function was extracted from `git show v4.8.1:<path>` and
`git show 458040f3:<path>` (signature through its matching brace) and compared as text. The
`for case in &contract.forbidden` loop of v4.8.1's `validate_requirement_trace_source` is
identical (77 lines) to the loop in `458040f3`'s `validate_requirement_trace_contract`. The
following are identical as whole functions: `requirement_step_match`,
`requirement_step_match_values`, `requirement_step_json` and `requirement_failure_base`
(`verification_output.rs`); `fsl_core::requirements_trace_contract`; and
`requirement_trace_scenarios_from_source`, `run_scenarios_mode_from_source`,
`forbidden_case_finding`, `forbidden_diff_findings`, `forbidden_unknown` and
`old_forbidden_arguments` (`main.rs`). `fsl-runtime` differs between the two, so this
covers how the gate and `diff` classify a last step's Monitor outcome, not which steps the
Monitor enables.

**#1212** (before: `458040f3`; after: #1212). The spec is the Wallet of
`rust/fslc/tests/issue_1212_forbidden_bad_call.rs` (`withdraw` guarded by
`requires amount <= balance`, balance 50, forbidden `withdraw(10)` then the last step below).
The first two columns verify; the third adds `invariant NotThirty { balance != 30 }`, which
`withdraw(20)` breaks at depth 1 away from the forbidden trace, so it shows that what a
command did before depended on the rest of the spec. Each command's whole stdout and every
file it writes were kept and compared; outside what the table states, they differ only in
`cache.key` (it hashes the binary), `approved_at`, and wall-clock `elapsed_s` (with the
approval digest of an artifact that embeds it). Default depth 3:

| command | last step `withdrew(60)` (no such action) | last step `withdraw(1, 2)` (arity) | either last step, with `NotThirty` |
|---|---|---|---|
| `check` | exit 0 → 2: `ok` → the error (`message`, `failed_step`, `step`, `loc`) | same | same |
| `verify` at `--depth` 1 and 3 | `verified` (exit 0) → 2 | same | `violated` (exit 1, the `NotThirty` counterexample) → 2, no counterexample |
| `verify --engine induction` | `proved` (exit 0) → 2 | same | `violated` (exit 1) → 2 |
| `sweep --depth 1..3` | `sweep_passed` (exit 0) → 2 | same | `sweep_failed` (exit 1) → 2 |
| `chain` | `verified` (exit 0) → `kind: "chain"`, `failed: ["requirements"]`, the layer's `detail` is the error (exit 2) | same | `violated` (exit 1) → 2, same |
| `scenarios` (depth 1, 3, 8) | exit 2 → 2: `kind: "semantics"` `unknown forbidden action 'withdrew'` → the error; no scenarios before or after | exit 0 with every scenario, the forbidden one as `rejected_by: "requires_failed"` → 2, no scenarios | `violated` (exit 1) → 2 |
| `testgen` (depth 1, 3) | exit 2 → 2; no test file before or after | exit 0 writing a test that asserts `requires_failed` → 2, no file | `violated` (exit 1), no file → 2, no file |
| `mutate` | `mutated` (exit 0) → 2, no mutants | same | `violated` (exit 1) → 2 |
| `html` | exit 0 → 2; the report is still written, but its status goes from `verified` (bounded, depth 3) to `error` / `not_run` and the action-coverage marks are gone; its witnesses section was already empty | exit 0 → 2, same, and the witnesses section loses both witnesses (`cover_withdraw`, `forbidden_FB-1`) | exit 1 → 2; the report is still written, its status and every property `not_run`, and the `NotThirty` counterexample trace is gone |
| `ledger` | exit 0 → 2; the ledger is still written, lists the error under `FB-1` (next action: fix the step's action name or arguments), and every requirement row's assurance becomes `not_run` | same | exit 1 → 2; same, and the `NotThirty` finding is gone |
| `counterexample export` | exit 2 → 2: `no counterexample to export: verification succeeded` → the error; no file before or after | same | exit 1 writing the `NotThirty` reproducer → 2, no file |
| `explain` (depth 1, 3) | exit 0 → 0; `witnesses` was and stays `[]` (the `scenarios` run it reads already failed) | exit 0 → 0, but `witnesses` goes from 2 entries to `[]` | exit 0 → 0; `witnesses` was and stays `[]` |
| `approval create --kind scenarios` | exit 2 → 2: `scenarios`' `kind: "semantics"` error → the error | exit 0 creating a record → 2, no record | exit 2 (`violated`) → 2 |
| `approval create --kind html` | exit 0 creating a record → 2, no record (stdout is the `html` result) | same | exit 2 → 2 |
| `approval create --kind ledger` | exit 0 → 0: a record is still created, of the ledger that now lists the error | same | exit 2 (`reviewed artifact does not match a fresh rendering`: a violated spec's ledger embeds wall-clock `elapsed_s`) → exit 0, a record |
| `approval check` of a record created before | `scenarios`: none could be created; `html`: `approved` → exit 2; `ledger`: `approved` → `drifted` (exit 0) | `scenarios`, `html`: `approved` → exit 2; `ledger`: `approved` → `drifted` | none could be created |
| Worker `check` / `verify` | the native error, pinned by the unit test `worker_reports_an_unknown_forbidden_final_action_like_native` (the Worker has no CLI to run) | same function | same function |

A last step `withdraw(500)` outside `type Amount = 0..100` keeps the exit status of every
command in the table above (`fslc diff` changes; see the next table); its
forbidden scenario moves from `rejected_by: "requires_failed"` to `"bad_call"`, so the
generated test asserts `bad_call`, and `approval check` of a `scenarios` record created before
reports `drifted` (exit 0) instead of `approved`.

For `FB-1`, `diff --depth 0` and `diff --git` report the following, and so does `approval diff`
of a `scenarios` record created before wherever one could be created (the first and third
rows, and the `number` case of the last):

| OLD → NEW | before | after | `--forbid unknown` |
|---|---|---|---|
| last step `withdraw(500)` on both sides | `unknown` / `forbidden_step_unrelatable` | preserved | exit 1 → 0 |
| OLD last step `withdrew(60)`; NEW declares `withdrew` with a refusing guard | preserved | `unknown` / `forbidden_replay_failed` | exit 1 → 1 |
| OLD last step `withdraw(1, 2)`; NEW gives `withdraw` that arity and refuses it | preserved | `unknown` / `forbidden_replay_failed` | exit 1 → 1 |
| last step `withdrew(60)`, spec against itself | `unknown` / `forbidden_step_unrelatable` | `unknown` / `forbidden_replay_failed` | exit 1 → 1 |
| `respond(7)` under `instances Case = 3`, or `add(9)` under `values Qty = 0..3`, against itself; OLD `entity Case` against a kernel NEW with `type Case = 0..2` | `unknown` / `forbidden_step_unrelatable` | unchanged | exit 1 → 1 |

In the two rows where an OLD unknown action or arity stops being preserved, `--forbid unknown`
already failed before: to refuse the step, NEW must declare the action OLD lacks, or give
`withdraw` the other arity, and that difference is itself an `unknown` finding
(`state_or_action_names_differ`, respectively `automatic_mapping_failed`). A requirements spec
cannot declare two actions of one name, so a NEW that adds the arity while keeping OLD's
`withdraw` cannot be written. `--forbid forbidden_relaxed` exits 0 before and after in every
row.

**#1213** (before: #1212, and `458040f3` gives the same exit status and verdict in every row;
after: #1213). The specs are the guardless Wallet of
`rust/fslc/tests/issue_1213_forbidden_strict.rs` (`withdraw` without `requires`,
`invariant NonNegative`, balance 50) with three forbidden traces whose last step breaks
`NonNegative`: `withdraw(10) withdraw(60)` (one step reaches the violation), `withdraw(30)
withdraw(30)` under `type Amount = 0..30` (two steps), and eleven `withdraw(5)` under
`type Amount = 0..5` (eleven steps, beyond every `--depth` used here; `testgen`'s own check
runs to depth 21 and reaches it). Default depth 3 unless stated:

| command | `withdraw(10) withdraw(60)` | `withdraw(30) withdraw(30)` | eleven `withdraw(5)` |
|---|---|---|---|
| `check` | exit 0 → 2 | exit 0 → 2 | exit 0 → 2 |
| `verify --depth 1` | `violated` (exit 1) → 2 | `verified` (exit 0) → 2 | `verified` (exit 0) → 2 |
| `verify --depth 3` | `violated` (exit 1) → 2 | `violated` (exit 1) → 2 | `verified` (exit 0) → 2 |
| `verify --engine induction` | `violated` (exit 1) → 2 | `violated` (exit 1) → 2 | `unknown_cti` (exit 1) → 2 |
| `sweep --depth 1..3` | `sweep_failed` (exit 1) → 2 | `sweep_failed` (exit 1) → 2 | `sweep_passed` (exit 0) → 2 |
| `chain` | `violated` (exit 1) → 2: its `[requirements]` layer fails | same | `verified` (exit 0) → 2, same |
| `scenarios` | `violated` (exit 1) → 2 at depth 1 and 3 | exit 0 with the forbidden scenario as `rejected_by: "invariant"` at depth 1, `violated` (exit 1) at depth 3 → 2 | exit 0 with `rejected_by: "invariant"` at depth 1, 3 and 8 → 2 |
| `testgen` (depth 1 and 3) | `violated` (exit 1) → 2; no test file before or after | same | same |
| `mutate` | `violated` (exit 1) → 2 | `violated` (exit 1) → 2 | exit 0 → 2: the baseline stops, no mutants |
| `html` | exit 1 → 2; the report is still written, its status goes from `violated` to `error` / `not_run` with the forbidden hint, `NonNegative`'s assurance becomes `not_run`, and the counterexample trace is gone; its witnesses section was already empty | same | exit 0 → 2; the report is still written, its status goes from `verified` (bounded, depth 3) to `error` / `not_run`, and the action-coverage marks and both witnesses (`cover_withdraw`, `forbidden_FB-1`) are gone |
| `ledger` | exit 1 → 2; the ledger is still written, the spec-wide `NonNegative` finding is replaced by an `FB-1` row (`forbidden`, the violation), and the requirement row's assurance becomes `not_run` | same | exit 0 → 2; the ledger is still written, gains the `FB-1` row (it had no finding), and the requirement row goes from `確認済（承認可）` to `not_run` |
| `counterexample export` | exit 1 writing a reproducer → 2 with no file | same | exit 2 (`kind: "semantics"`, nothing to export) → the forbidden error, no file |
| `approval create --kind scenarios` | exit 2 (`violated`) → the forbidden error | same | exit 0 creating a record → exit 2, no record |
| `approval create --kind html` | exit 2 → 2, no record | same | exit 0 creating a record → 2, no record (stdout is the `html` result) |
| `approval create --kind ledger` | exit 2 (`reviewed artifact does not match a fresh rendering`: a violated spec's ledger embeds wall-clock `elapsed_s`) → exit 0, a record of the ledger that lists the error | same | exit 0 → 0, a record of the ledger that now lists the error |
| `approval check` of a record created before | none could be created | none could be created | `scenarios`, `html`: `approved` (exit 0) → exit 2; `ledger`: `approved` → `drifted` (exit 0) |
| `explain` (depth 1, 3) | exit 0 → 0; `witnesses` was and stays `[]` | exit 0 → 0; at depth 1 `witnesses` goes from 2 entries (`cover_withdraw`, `forbidden_FB-1`) to `[]`, at depth 3 it was already `[]` | exit 0 → 0; `witnesses` goes from the same 2 entries to `[]` at depth 1 and 3 |
| Worker `check` / `verify` | the native error, pinned by the unit test `worker_reports_a_violating_forbidden_final_step_like_native` | same function | same function |

After #1213, `scenarios` and `testgen` emit no scenario and write no test file, and `ledger`
reports the violation under `FB-1`. `mutate --depth 1` on the two-step Wallet
*with* `requires amount <= balance` exits 0 before and after, but its `requires_remove` mutant
moves from `survived` to `killed` with `killed_by: "forbidden"`.

For `FB-1`, `diff --depth 0` and `diff --git` report the following (`approval diff` of a
`scenarios` record created before reports the same findings with exit 0, in the rows whose
OLD could be recorded: the first, second, and last):

| OLD → NEW | before | after | `--forbid forbidden_relaxed` | `--forbid unknown` |
|---|---|---|---|---|
| guarded Wallet with `NonNegative` → kernel Wallet without the guard (NEW final step violates) | preserved | `forbidden_relaxed` | exit 0 → 1 | exit 0 → 0 |
| guarded Wallet → kernel Wallet with `invariant Floor { balance >= 45 }` (NEW setup step violates) | preserved | `unknown` / `forbidden_replay_failed` | exit 0 → 0 | exit 0 → 1 |
| guardless Wallet with `NonNegative` → guarded Wallet (OLD final step violates) | preserved | `unknown` / `forbidden_replay_failed` | exit 0 → 0 | exit 0 → 1 |
| guardless Wallet with `NonNegative` against itself | preserved | `unknown` / `forbidden_replay_failed` | exit 0 → 0 | exit 0 → 1 |
| guarded Wallet against itself (control) | preserved | preserved | exit 0 → 0 | exit 0 → 0 |

## 3. Ripple (verification engine and Monitor unmodified)

- grammar.py: `forbidden_def` (`expect rejected` inline) + transformer.
- dialects.py: `("__forbidden", …)` collection. model.py: store into `spec["forbidden"]`.
- acceptance.py: `replay_forbidden` / `validate_forbidden`. A copy of `replay_acceptance`,
  differing in "premise all ok / last expected to be ok:False / no `expect` state evaluation."
  Because `Monitor.step()` returns `ok:False` + `kind` for rejection via requires_failed /
  invariant / type_bound / partial_op / ensures, the outcome is decided from step()'s
  return alone. The frozen Python reference still counts the violation kinds as a
  rejection; the native CLI does not since #1213 (§2).
- cli.py: `_forbidden_error` wired into both the check and verify paths. bmc.py: emits
  `forbidden_<ID>` (with `rejected_by`) into `scenarios` → for testgen's negative tests.

## 4. Tests

`tests/test_forbidden.py` (frozen Python reference): case-a satisfied + scenario / accepted →
`kind:"forbidden"` + accepted_trace / broken setup → `forbidden_setup` / case b (a type_bound
violation: still `ok` with `rejected_by: "type_bound"` and `verify` `violated`, the reading
before #1213) / empty steps / the verify gate fires before BMC. Gallery positive example
(`small_forbidden_guarded_cancel.fsl` → verified) and incorrect example
(`forbidden_op_accepted.fsl` → error/forbidden).

Native (`rust/fslc/tests/`): `issue_1212_forbidden_bad_call.rs` (`bad_call` vs
`requires_failed`, unresolved last steps, `loc`, both-side `bad_call` in `diff`) and
`issue_1213_forbidden_strict.rs` (case b is `kind:"forbidden"` for each violation kind;
`verify` at `--depth` 1, 2, and 3 and with `--engine induction`, `sweep`, `chain`,
`counterexample export`, `scenarios`, `testgen`, `html`, and `ledger` stop at the gate, with
`html` and `ledger` still writing their report; `mutate` kills a guard-dropping mutant with
the forbidden; `diff` OLD/NEW violations and their `--forbid` exits; the undefined-guard
regression). The Worker's `check` / `verify` errors are
pinned against native in `rust/fsl-wasm/src/lib.rs`.

## 5. Related

The dual of `acceptance` (DESIGN-bridge / DESIGN-dialects). Detecting
under-constraint is complementary to #4 vacuity (`always_true_requires`) and #6
mutate. It is the validation workflow's independent negative-example channel.

## 6. Corpus sweep

Each sweep runs a debug `fslc` (`cargo build -p fslc-rust --bin fslc`) built in, and run
inside, its own materialized tree — main at `458040f3` (base) and the change under test on
top of it — over every `.fsl` under `specs/` (23), `examples/` (191),
`rust/fslc/tests/fixtures/` (309) and `tests/fixtures/` (29) — 552 files:

- `fslc check <file>` for every file;
- for the 25 files that contain a `forbidden` line (21 files with 39 requirements
  `forbidden` declarations, plus 4 AI-authority `forbidden` rules):
  `fslc scenarios <file> --depth 8 --deadlock ignore` and the self-diff
  `fslc diff <file> <file> --depth 2 --forbid forbidden_relaxed,unknown`, which replays
  every forbidden of the file against itself.

The full JSON output and exit status are compared file by file. The base binary is run
twice; the only fields that differ between those two runs are `cost.elapsed_s`,
`cost.properties[*].elapsed_s` and `cost.solver.check_elapsed_s` of `scenarios`
(wall-clock time), and only those are excluded. A command that exceeds the sweep's
300-second limit is rerun alone with a 3600-second limit in the same tree before comparing
(`scenarios` of `examples/agentic_rag/agentic_rag_requirements.fsl` on a loaded machine, in
every run).

- #1212: `check` verdicts at base are 391 `ok`, 154 `error`, 3 `causal_model_checked`,
  3 `refinement_failed`, 1 `impl_violated`; base emits 31 forbidden scenarios, all
  `requires_failed`. Of the 25 self-diffs, 19 are `no_semantic_change` (exit 0); 5 report
  their forbidden as `unknown` / `forbidden_replay_failed` (exit 1) because OLD itself
  accepts the last step — the 5 forbidden-bearing files whose `check` is `error` /
  `forbidden` (`forbidden_op_accepted`, the three `*__guard_weakening` gallery specs, and
  `forbidden_final_unguarded`); and the AI document
  `examples/ai/support_answer_quality.fsl` is not a spec (`kind: "parse"`, exit 2).
  Base → #1212: verdict changes (exit, `result`, `kind`, and for `diff` its `summary`,
  forbidden findings, and gate violations) 0; forbidden `rejected_by` changes 0; other
  differences 0. The ok → error transition set of §2 (an unknown-action or arity-mismatch
  last step) is empty for the corpus, no emitted forbidden scenario moves from
  `requires_failed` to `bad_call`, and no self-diff verdict moves.
- #1213, on top of #1212, with the same base runs: base → #1213 verdict changes 0,
  forbidden `rejected_by` changes 0, other differences 0, so #1212 → #1213 differs only in
  the excluded wall-clock fields. All 31 emitted forbidden scenarios are `requires_failed`
  on both sides, so no corpus forbidden relied on a violation kind as its rejection, and
  no self-diff finds a violating final or setup step. The 5 forbidden-bearing files whose
  `check` is not `ok` report the same accepted-step `kind: "forbidden"` error, byte for
  byte, at base and with #1213.
