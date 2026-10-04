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
- Success if the **last step is rejected**. Rejection has two forms:
  - (a) **not-enabled** (`requires_failed` / out-of-domain `bad_call`) — the **primary use**.
    A "correct prohibition by a guard" that is invisible as a safety invariant.
  - (b) **violation on execution** (`invariant` / `type_bound` / `partial_op` / `ensures`).
    But a reachable violating state ⇒ means **the spec itself is violated under verify**
    (case b is the signal "forbidden is satisfied but the spec is buggy"). The output's
    `rejected_by` carries this distinction.
  - `rejected_by` names the Monitor outcome: `requires_failed` when every argument lies in
    its parameter domain and a `requires` guard refuses the call, `bad_call` when no action
    of that name and arity has every argument in its parameter domain (DESIGN-bridge.md
    1.2), or the violation kind of (b). The generated negative test asserts that kind, so an
    out-of-domain call is never asserted as a guard refusal (issue #1212). The Monitor's
    `bad_call` in DESIGN-bridge.md 1.2 also covers an unknown action or a missing
    parameter; for a forbidden last step those are not rejections (next bullet), so here
    `bad_call` means only an argument outside the parameter domain.
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
its own materialized tree: main at `458040f3` (before) and this change (after).
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
Monitor enables. The spec is
the Wallet of `rust/fslc/tests/issue_1212_forbidden_bad_call.rs` (`withdraw` guarded by
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

## 3. Ripple (verification engine and Monitor unmodified)

- grammar.py: `forbidden_def` (`expect rejected` inline) + transformer.
- dialects.py: `("__forbidden", …)` collection. model.py: store into `spec["forbidden"]`.
- acceptance.py: `replay_forbidden` / `validate_forbidden`. A copy of `replay_acceptance`,
  differing in "premise all ok / last expected to be ok:False / no `expect` state evaluation."
  Because `Monitor.step()` returns `ok:False` + `kind` for rejection via requires_failed /
  invariant / type_bound / partial_op / ensures, both (a) and (b) are decided from step()'s
  return alone.
- cli.py: `_forbidden_error` wired into both the check and verify paths. bmc.py: emits
  `forbidden_<ID>` (with `rejected_by`) into `scenarios` → for testgen's negative tests.

## 4. Tests (tests/test_forbidden.py)

Case-a satisfied + scenario / accepted → `kind:"forbidden"` + accepted_trace / broken setup →
`forbidden_setup` / case b (rejected_by=type_bound and verify violated) / empty steps / the
verify gate fires before BMC. Gallery positive example (`small_forbidden_guarded_cancel.fsl` →
verified) and incorrect example (`forbidden_op_accepted.fsl` → error/forbidden).

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
