# FSL — vacuity check implementation design

Motivation: issue #4 (category 5 of roadmap #1). Each of the following specs becomes verified
yet checks nothing: an implication invariant whose antecedent is unreachable (`P => Q` where P
never happens), a leadsTo whose trigger is unreachable (`P ~> Q` where P never happens), a
requires clause that is always true in reachable states (a dead ornament), a frozen ghost
invariant, and a requirements `deadline` whose generated `tick` action is dead because urgency
freezes time. Vacuous verified, on par with under-constraint, is the biggest source of missed
bugs. Conventionally `kind:"vacuous"` referred only to init unsatisfiability.

## 1. CLI

`fslc verify <f> [--vacuity warn|error|ignore]` (default warn, the same shape as deadlock).
- `warn`: list in warnings (the result stays verified / proved)
- `error`: on detection `{"result":"error","kind":<detected kind>,"findings":[…]}` → **exit 2**
  (no counterexample trace, so not violated/exit 1, in the same family as init-unsatisfiable
  `vacuous`)
- `ignore`: skip the check

## 2. Vacuity lanes (only on the verified / proved path)

The `--vacuity warn|error|ignore` selection contract applies only to `verify`
and `sweep` results on their verified/proved path. `scenarios` has no
`--vacuity` mode and does not promote a vacuity finding to exit 2; it
independently preserves the typed `never_enabled_action` diagnostic from its
bounded action-coverage result so generated integration scaffolds do not
misdescribe a blocked action as covered.

1. **`never_enabled_action`**: bounded action-coverage evidence that no instance of an
   action was enabled through depth K. The warning names K and is deliberately not a
   proof that the action is permanently dead: it can disappear at K+1 when a delayed
   enabling path is reached. The existing structured `action_coverage` projection and
   this `--vacuity`-selectable warning remain distinct; neither changes assurance. Only
   actions with an authored primary origin or a non-zero source span become public
   findings; generated-only zero-span lowering sentinels remain visible in coverage but
   do not produce a fabricated location or fail `--vacuity error`.
2. **`vacuous_implication`**: the antecedent of a **user invariant** with a single `=>`
   directly under `forall*` does not become sat within depth K. The existential closure of the
   antecedent is fed to the existing `eval_expr` by wrapping the AST with
   `("exists", binder, A)` (no new evaluator). Implicit `_bounds_*` are out of scope (Seq
   live-prefix is in implication form, and warnings on auto-generated items would be noise).
3. **`vacuous_leadsto`**: check the leadsTo trigger P with the same existential closure.
4. **`always_true_requires`**: for each requires clause j, warn if **with the context of the
   preceding clauses** `sat(clause 1..j-1 ∧ ¬clause j)` is unsat over all reachable states ×
   all instances. The reasons for using context are (a) consistency with Monitor short-circuit
   (BUG-020), (b) detecting redundant clauses too (`st!=Cancelled` after `st==Paid`), and
   (c) spurious sat from the whole-domain Z3 encoding of a let-internal partial op works only in
   the "do not emit a warning" direction and is safe. **Coverage-false actions** (already warned
   never-enabled) and **compose-synchronized actions** are out of scope.
5. **`tautology_over_frozen`**: a user invariant that depends only on state variables no action
   ever assigns to, and is dynamically always true over those frozen values, is a hollow
   invariant. This is a static pre-filter plus Z3 check; it remains pending until final warning
   emission.
6. **`urgency_freeze`**: for requirements `time`/`deadline`, warn only when the generated
   deadline invariants exist, the generated `tick` action has the structural guard
   `requires not(urgent_enabled)`, the deadline age variables are not assigned by non-`tick`
   actions, and Z3 proves `urgent_enabled` holds in every initial state and is preserved by
   every action. This is depth-independent and intentionally incomplete: if the initial or
   inductive proof fails, no warning is emitted.
7. **`vacuous_deadline`**: for each generated deadline, derive the predicate
   that all entries of its age state equal zero. Warn when Z3 proves that
   predicate for every initial state and proves it is preserved by every
   transition. Unlike `urgency_freeze`, this does not claim that `tick` is
   globally dead: `tick` may run while the age condition is false, yet no
   execution consumes deadline slack. The proof permits non-`tick` actions to
   reset age to zero and catches state-changing urgent handlers that disable
   their own guards. A `tick` transition that advances age is the rejecting
   control and suppresses the finding.
8. **`vacuity_probe_truncated`** (issue #729): lanes 2–3's shared reachability
   probe stopped at its state budget before deciding a candidate either way.
   See the subsection immediately below.

### Lanes 2–3: budgeted reachability probe and `vacuity_probe_truncated` (issue #729)

`fsl-runtime::verification_warnings` used to run one **unbudgeted** concrete BFS
(`expression_reachable`) per implication antecedent and per leadsTo trigger: a
property count multiplier on top of a per-candidate search with no ceiling, so
a spec whose state space defeats BFS dedup (e.g. an order-sensitive history
`Seq`, the reproducer in `rust/fslc/tests/issue_697_all_properties_memory.rs`'s
`LabelCoreRepro`) could consume gigabytes verifying a single property in
isolation, and `--vacuity ignore` did not help: `apply_vacuity_mode`
(`rust/fslc/src/verification.rs`) only filtered the rendered `warnings` array
after the (already paid) computation.

The fix follows the in-repo precedent `docs/DESIGN-explicit-engine.md`
established for the same shape of problem (`--engine explicit`'s
`unknown_budget` verdict): give the search a budget, and make "the search was
cut off before it could decide" its own reportable outcome rather than
silently reusing the closed-search verdict.

- **One shared, budgeted BFS.** `fsl_runtime::expression_reachability` takes
  every implication-antecedent and leadsTo-trigger candidate for a model at
  once (built by `vacuous_implication_candidates`/`vacuous_leadsto_candidates`)
  and walks the concrete state space once, checking every still-pending
  candidate against each popped state and dropping it from `pending` the
  instant it is found true. This removes the per-property multiplier: N
  candidates no longer pay N full BFS traversals. It reuses `find_boundary_
  violation`'s established scratch-`Monitor`/no-per-node-clone pattern
  (issue #730/#776) rather than inventing a new search mechanism.
- **Budget.** Reuses `CONCRETE_PROBE_BUDGET` (50,000 states) — the same
  constant and calibration `find_boundary_violation` uses, protected by the
  same style of corpus-conservation test
  (`rust/fslc/tests/issue_729_vacuity_probe_corpus_budget.rs`, mirroring
  `issue_697_corpus_probe_budget.rs`).
- **Tri-state result, fail-closed.** `Reachability::{Reachable, Unreachable,
  Exhausted}`. `Unreachable` means the same thing it always did — full
  enumeration within `--depth` completed and the candidate never became
  true — and keeps producing `vacuous_implication`/`vacuous_leadsto` exactly
  as before; depth-bounded non-closure was already reported this way and is
  unchanged. `Exhausted` is the new state: no verdict was ever reached for
  this candidate, either because the shared budget was hit while it was
  still pending or because evaluating its expression in some visited state
  returned an error. Folding `Exhausted` into `Unreachable` would be
  fail-open (a false "confirmed vacuous" claim); silently dropping it would
  let `--vacuity error` pass a spec whose vacuity was never actually
  established. Both are unacceptable weakenings of `--vacuity error`'s
  contract ("vacuity evidence is clean"), so `Exhausted` gets its own kind,
  `vacuity_probe_truncated`, added to `fsl_core::VACUITY_KINDS` — selected by
  `--vacuity` exactly like the other seven kinds (`warn` shows it, `error`
  fails closed on it, `ignore` discards it). An informational, non-selected
  kind was considered and rejected for the same reason: it would make
  `--vacuity error` strictly weaker than before #729, letting a
  never-decided spec through silently.
- **A per-candidate evaluation error also resolves `Exhausted`, not
  `Reachable`.** The pre-#729 `expression_reachable` silently treated an
  evaluation `Result::Err` for one property's antecedent as "no warning" for
  that property alone (`matches!(expression_reachable(...), Ok(false))` is
  `false` on `Err` too) — a pre-existing, out-of-scope-for-#729 fail-open on
  an already-rare path. Batching forced a choice: fold that per-candidate
  `Err` into `Reachable` (preserving the old silent-drop byte-for-byte) or
  into `Exhausted` (fail-closed, consistent with the new budget-truncation
  path). Chosen: `Exhausted`. Rationale — this issue's whole point is "never
  silently pass a probe that could not decide," and a caller downstream
  (`--vacuity error`) cannot tell *why* a candidate has no verdict, so the
  two causes (budget cutoff, evaluation error) should not be allowed to
  produce different fail-open/fail-closed outcomes; letting a candidate's
  evaluation fail must not become a way to defeat `--vacuity error` that a
  budget cutoff cannot. The `message`/`hint`/`recommended_action` text for
  `vacuity_probe_truncated` is written cause-neutral ("the probe either
  exhausted its internal state budget or failed to evaluate the candidate")
  rather than claiming budget truncation specifically, since `Reachability`
  does not (and need not) distinguish the two internally.
- **Known gap: a whole-walk `RuntimeError` still loses every candidate's
  finding for that run**, exactly as the pre-#729 per-property calls did for
  their one property each (`verification_warnings`'s
  `.unwrap_or_default()`). Unlike the per-candidate case above, this is not
  attributable to one candidate — it means an action's `enabled`/`step`
  itself could not be evaluated, the same condition that already fails the
  surrounding BMC/explicit run before vacuity warnings are ever rendered, so
  this path is not reachable on any spec that reaches `verification_warnings`
  in the first place. Accepted as a narrow, practically-unreachable gap
  rather than threading partial results out of an `Err` path (see the doc
  comment on `expression_reachability`'s `unwrap_or_default` call site).
- **`--vacuity ignore` skips the computation, not just the output.** Consumer
  audit (issue #729) found exactly one product call site that ever applies a
  vacuity mode (`rust/fslc/src/verification.rs`'s `execute_cli_verification`,
  reached by both the `verify` and `sweep` CLI commands — `sweep` funnels
  through the same `run_verify_cli`); `ledger`/`html`/`mutate` share a
  mode-less baseline (`rust/fslc/src/main.rs`'s `run_verify`, whose signature
  has no vacuity parameter at all) and the wasm Worker request surface has no
  `--vacuity` option either. So `skip_vacuity_probe`
  (`BmcOutputOptions`/`BmcRequest`/`InductionRequest`/`ExplicitRequest` in
  `rust/fslc/src/verification*.rs`) is an explicit argument threaded only
  from that one derivation point (`options.vacuity == "ignore"`, plus its
  `--lemma`-path twin in `run_induction_with_lemmas`); every other caller
  passes `false`. Skipping is proved observationally equivalent to
  computing-then-filtering by
  `rust/fslc/tests/issue_729_vacuity_ignore_skip.rs`, which asserts the
  `--vacuity ignore` envelope equals the `--vacuity warn` envelope with every
  `is_vacuity_kind` warning removed (cost/timing fields excluded, since
  skipping legitimately does less work).
- **Ledger wording.** `rust/fsl-tools/src/ledger.rs`'s summary prefix
  distinguishes the two: an ordinary vacuity finding reads "空虚性の疑い"
  (suspected hollow — something was proven), while
  `kind == "vacuity_probe_truncated"` reads "空虚性未確立（到達性 probe が
  判定に至らず）" (vacuity not established — the probe never reached a
  verdict; deliberately cause-neutral, matching `Reachability::Exhausted`
  not distinguishing budget cutoff from an evaluation error). `html.rs`
  renders `kind`/`message` generically and needed no change.
  `assurance_token` never reads `warnings`, so a truncated-probe finding
  cannot move a requirement's assurance class
  (`rust/fsl-tools/tests/issue_729_vacuity_probe_truncated_ledger.rs`).
- **End-to-end truncation coverage.**
  `rust/fslc/tests/issue_729_vacuity_probe_truncated_e2e.rs` drives the
  `vacuity_probe_truncated` emission arm through the real CLI end to end
  (not just at the `expression_reachability`/`render_ledger` unit level):
  a `count = count * 10 + x` digit-growth model whose reachable state count
  is unbounded and grows `10^level` per BFS level genuinely exhausts
  `CONCRETE_PROBE_BUDGET` within a few seconds, so `--vacuity error` exits 2
  with `kind:"vacuity_probe_truncated"` — proving the whole path from a real
  budget exhaustion through `is_vacuity_kind` selection to the CLI exit code
  actually fires, not just that its helper functions are individually
  correct.

### Native lanes 4–7: "over all reachable states" is decided over the type space

The frozen Python reference discharges lane 4 from the states an unrolling actually witnesses:
a clause survives to a warning when nothing within depth K falsified it. That reports a real
guard as dead merely because the bound was too small. `examples/causal/funnel.fsl` declares
`state { visits: 0..100 }` and `requires visits < 100`; at depth 8 `visits` reaches only 8, so
Python warns — and the warning disappears when `--depth` rises. A judgment that moves with the
bound is not evidence of a hollow spec (issue #465).

The native implementation (`fsl-verifier::vacuity`) therefore decides all three solver-dependent
lanes over the **declared type space**, which is a superset of the reachable states: for clause
`j`, `type bounds ∧ clauses[..j] ∧ ¬clauses[j]` unsat means the clause cannot be false in any
reachable state at any depth. `visits == 100` is inside `0..100`, so `funnel.fsl` is correctly
silent. Every lane is a bounded number of one-shot queries over freshly named states, and each
verdict is therefore independent of `--depth`.

This is sound and deliberately incomplete in the same direction as lane 6: a clause that is dead
for a reason the type space cannot see goes unreported, and an `unknown` backend verdict yields
no finding. The exploration contributes exactly one input, action coverage, and only to suppress
lane 4 for actions that were never enabled.

Two exclusions keep the lanes pointed at authored text. Generated declarations are skipped, and
so is any declaration without a source location: some dialect lowerings synthesize an entire
catalog kernel (the governance catalog's `_governance_catalog_ok` over a frozen generated Bool)
with a zero span rather than a `generated_only` origin, and warning about that scaffolding is
noise in every governance spec.

### Reason for excluding compose-synchronized actions (important)

`deposit_audited = bank.submit_deposit || audit.deposit` inherits `requires a > 0` from both
bank and audit. This is duplication exactly as designed ("each component defends its own
contract"), not a removable redundancy (it is naturally required for audit_log alone). Excluded
not by name guessing but by the sync marker that compose expansion sets (the action dict's
`sync`). Each clause is checked in the right context by the verify of the component spec alone,
so there is zero detection loss.

## 3. Ripple (the verification engine core is unmodified; only a piggyback onto `_bmc_explore`)

- bmc.py: `pending_vacuity` contains dynamic candidates (implication antecedents + leadsTo
  triggers), isomorphic to the `pending_reachables` loop, plus static candidates
  (`tautology_over_frozen`, `urgency_freeze`, `vacuous_deadline`) that are proven before exploration and carried to
  finalization. requires tautology adds the sat of "preceding clauses ∧ ¬clause" to the coverage
  loop. Context-bearing candidates are pre-filtered to only "clauses logically implied by the
  preceding clauses over the type space" (`_requires_clause_locally_implied`), excluding bounded
  false positives from capacity-guard families.
- Output: `{kind, name(display name), loc, requirement, message, hint}` in warnings. prove()
  passes warnings through transparently from the base verify. Native `scenarios`
  does not select a vacuity mode; it emits its own typed `never_enabled_action`
  coverage diagnostic and keeps its existing exit behavior.
- Successful BMC output remains explicitly bounded via `completeness:"bounded"`
  and `checked_to_depth`. When normal exploration first witnesses a
  reachable/vacuity/coverage fact at the final depth K, `verified` includes a
  saturation hint; this is separate from vacuity findings and only says the bound
  has not obviously reached a fixpoint.
- The hint avoids misdirecting the repair: a tautological requires says not "delete the clause"
  but "judge whether the model is lacking or redundant" (it may take effect at greater depth or
  under induction).

## 4. Tests (tests/test_vacuity.py)

The warning kinds (display name, loc, requirement) / forall wrapping / context-bearing
redundant clause / two suppressions (coverage false, sync) / frozen-ghost tautology /
urgency-freeze positive / state-changing deadline-zero positive and
deadline-urgency-pattern negative / not shown on the violated path /
induction pass-through / error (exit 2) and ignore / **corpus zero-false-positive gatekeeper**
(specs/ + examples/ + gallery/valid in one batch). Gallery
`vacuous_implication_warning.fsl` (`--vacuity error`).

## 5. Related

Complementary to #3 forbidden and #6 mutate in detecting under-constraint and empty
formalization. The reachability-based lanes make "within depth K" explicit; `urgency_freeze`
is reported only after an initial + inductive proof of the generated urgent condition. Roadmap #1.
