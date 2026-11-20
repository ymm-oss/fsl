# FSL — `fslc explain --readable` (human-readable rendering and counterfactuals) implementation design

Motivation: issue #7 (category 6 of roadmap #1). Having PMs and consultants directly
review the logical formulas written by an AI is not realistic, whereas humans are good at
judging the correctness of concrete traces. There was no mechanism to let the machine assist
the spec→human direction of translation. This shifts human review from "reading logical
formulas" to "adjudicating concrete examples."

## 1. CLI / output

`fslc explain <f> [--depth K]` → `result:"explained"`, exit 0, JSON output.
`fslc explain <f> [--depth K] --readable` emits the deterministic text review
view. **No LLM used** (deterministic formatting only; prose generation is left
to the agent-side skill). The verification engine is unmodified; #6 mutate and
verify are reused. The text prints no witness or counterfactual, so
`--readable` does not compute them (#1839); only its `Implements:` section
runs a refinement check. Its second line is still
`Counterfactual scope: invariant/reachable (leadsTo liveness skipped; use verify)`
(written by `run_explain_from_source` in `rust/fslc/src/main.rs`). That line states the scope of the JSON
form's counterfactual analysis; the readable text computed none.

```json
{"result":"explained",
 "skeleton":{"state":…, "actions":[{"name","actor?","requires_text","writes","ensures_text","requirement"}],
             "properties":[{"kind","name","body_text","requirement"}], "auto_checks":…},
 "counterfactuals":[{"invariant","weakening":{op,loc,target},"trace","requirement","violation"}],
 "witnesses":[…]}
```

`counterfactuals[].violation.cost.elapsed_s` is the wall time of the whole
analysis of the weakening that produced the entry (its screening runs and
every per-invariant run, §4) plus building the entry's explanation; it is not
the time of that invariant's own run. Entries from the same weakening
therefore carry nearly the same value, and a `fair_remove` weakening whose
outcomes were reused from an earlier fair removal reports little more than
the explanation time.

## 2. The three artifacts

1. **Skeleton enumeration** — outputs the state schema, verification bounds, KPI
   projections, branch lowering, synthesized refinement mapping, each action's
   "who / when (requires) / what it changes," and each property's "what it
   forbids / guarantees," together with requirement tags and implicit checks
   (type bounds, partial_op).
2. **Counterfactual narratives** — for each user invariant, presents the shortest counterexample
   under the minimal model weakening that makes the invariant violated, as "without this rule,
   the following sequence breaks REQ-3." This automates the manual demo in `cart_v1_buggy.fsl`.
3. **Witness narration** — formats reachable / scenarios traces with display names plus the
   original requirement text.

## 3. Building the skeleton WITHOUT an AST pretty-printer (important)

**FSL has no AST→string formatter.** What `_requires_blocking_entry` (bmc.py) does is
`source_lines[line-1].strip()` — **slicing out a source line by its loc**, not rendering the
AST. Therefore:
- The state schema, action params, and **"what is changed" are computed by structurally
  scanning the left-hand sides of assign statements**.
- The **text of requires / property bodies is shown verbatim via loc slicing** (in a
  dialect-expanded spec the loc points at the original business-layer text, so seeing things
  like "by Manager" is actually desirable).
- **compose**: locs originating from a component point at a file that has not been loaded, so
  source slicing is impossible → fall back to names/structure (do not crash).

## 4. Counterfactuals sit thinly on top of #6 mutate (using all operators)

In essence this is "the #6 mutate kills re-sorted per user invariant and narrated." No new
verification logic is written. Key points:
- The counterfactual search draws on **all of mutate's operators (guard removal + assignment
  removal + fair removal)**. **Requires removal alone is not enough**: what breaks
  `ShippedWasPaid` in order_workflow is not a `requires` removal but **removal of the state
  assignment `orders[o].status = Shipped` in ship** (leaving only `shipped.add(o)` violates
  it via "in the set yet not Shipped").
- `killed_by` includes non-invariants too (ensures = action name, reachable name, bounds) →
  counterfactuals are **limited to user invariants**. Reachable kills are a different kind
  ("without this, X cannot be reached").
- **"No counterfactual" is legitimate** when no eligible single weakening produces
  a violation of the target invariant within the requested depth. Report the
  depth-limit note rather than an error; a deeper search may find a candidate.
  Do not use order_workflow's `NonNegativeRevenue` as a native no-candidate
  example: removing `revenue = 0` from init leaves its `Int` value unconstrained,
  so native explain can select an initial-state violation (#1350).
- **Native per-invariant isolation (#1350):** for each weakening, search each
  user invariant separately, retaining that target invariant but excluding the
  other user-invariant checks. Type bounds and the remaining action/transition
  checks stay in the model, with the selected engine's first-violation order:
  BMC can report the target invariant before a `trans` or `ensures` failure on
  the same terminal edge. Concrete canonicalization may reject that edge; if
  it finds no acceptable replacement trace, the solver-target path retains
  the BMC trace, just as normal `verify` reports that first target failure.
  By contrast, the non-target-preemption
  fallback below accepts only concrete paths with a clean initial state and
  checks the remaining obligations even on the terminal edge. Consequently,
  a target's counterfactual trace may pass through a state that violates another
  invariant; it is not a promise that the complete model admits that trace.
  Ordinary `verify` and
  `replay` retain all invariants and stop at the first violation, so they may
  stop before this target's narrated failure. Attribution is per invariant,
  not just the bounded verifier's first reported failure for the mutant.
- **Non-user-check preemption (#1350):** if the target-isolated verifier first
  reports `type_bound`, `trans`, `partial_op` or `ensures`, try the existing
  solver-independent concrete search for the fewest steps through the
  requested depth. It skips guard-partial action instances and errored/violating children
  while exploring other instances and queued paths; a failed initialization or
  an initially violated/unevaluable state cannot supply a concrete trace.
  A target witness competes under the same ranking tuple. If none is found,
  the entry's existing `note` says `preempted by KIND NAME at step k` rather
  than claiming no counterfactual exists within depth. Retain the first observed
  preemption for that target; any found candidate takes precedence over notes.
- If concrete startup fails for a preempted weakening, append the reason to
  that existing note. Missing/partially initialized roots reported by the
  Monitor use `concrete search not run: nondeterministic initialization of state
  NAME[, NAME] is unsupported`, with public state names. Other initialization
  or initial-state check errors use `concrete search not run:
  initialization/initial-state checks failed: REASON`, preserving the reported
  cause without inventing free roots. Distinct reasons are retained per target;
  they do not replace the existing preemption or bounded-init skip clauses.
  Constructor failure ends the search because no concrete root exists. This does not enumerate nondeterministic initial states. Entries
  whose concrete search did run keep their existing note unchanged, and selected
  candidates still take precedence. A declined optional canonicalization of a
  solver-target witness keeps its BMC evidence as described above.
- Bounded init-assignment removals remain skipped: the concrete search cannot
  enumerate nondeterministic initial valuations. If an invariant has no selected
  candidate and such a skip occurred, append `init-assignment removal of bounded
  state NAME was not analyzed` and the skip reason to its existing depth or
  preemption note, naming each skipped state root. This qualifies the existing
  depth note rather than introducing a JSON field or silently analyzing a
  different initial-state model. HTML already renders the same `note` evidence;
  readable mode remains the existing scope/skeleton view.
- Counterfactual weakening is scoped to user invariants and reachability. The
  per-candidate verifier model removes `leadsTo` properties because their lasso
  results are not consumed by either counterfactual artifact; normal `verify`
  remains the authority for liveness. JSON reports this as
  `counterfactual_scope.liveness:"skipped"`, and `--readable` states the same
  scope. This prevents quantified liveness from multiplying every safety
  weakening without silently implying that liveness was analyzed (#633).

**Amendment (#1838): cost.** The counterfactual phases dominated `explain` and
`html`. They now skip work whose result is already known, and their output is
unchanged byte for byte apart from `cost.elapsed_s`; no budget truncates them.

- **Screening.** Before the per-invariant runs, one bounded run of the
  weakened model retains all its invariants. A violated invariant is dropped
  and the run repeated; a run that ends without a violation proves the same
  for each remaining invariant's own run, which is then skipped. A run asserts
  the init and transition constraints, which do not depend on the invariants,
  and assumes a check only after proving it, so a run that retains one of the
  invariants asks only questions the screening run answered `unsat`. Any other
  outcome screens nothing.
- **Fair removals.** The verifier reads `fair` only in the `leadsTo` lasso
  search and the Monitor never reads it. Counterfactual models clear `leadsTo`,
  so fair removals whose models are equal once `fair` is ignored share one
  analysis.
- **One concrete search.** A preempted target's concrete search runs once,
  breadth-first, to the requested depth. The Monitor reports a violation at
  the step it occurs and every level is generated before the next, so its
  first match is the one the former exact search per step count found first.
- **Reachable phase.** A weakening of a spec with no reachable is not run. A
  weakened model that the invariant phase already ran with all its invariants
  reuses that run's verdicts: whether it found a violation and which
  reachables it left unreached.

- **Solver-chosen values.** Verdicts do not depend on which runs came before,
  but the model a run returns can: solvers on one thread share a Z3 context.
  An outcome carries a solver-chosen value when concrete search does not
  replace its trace (a removed init assignment, an edge the Monitor rejects)
  or when its preemption note names a check whose public name depends on the
  run's last action. The screening and shared analysis therefore run on a
  fresh context (`fsl_solver_z3::with_fresh_context`). Which outcome an entry
  prints is known from verdicts alone: the rank is trace length, operator and
  source line, and a note is printed only when no weakening violates the
  invariant. If a printed outcome carries a solver-chosen value, the
  per-invariant runs are then made in order on the thread's own context, as
  before #1838, up to the last such outcome, and the solver-chosen outcomes
  are taken from them.
- **Re-solved traces (#1940).** A run whose traces can be printed re-solves
  each model it projects a trace from on a fresh Z3 context
  (`DESIGN-rust-port.md` §4), so a printed solver-chosen value no longer
  depends on the runs before it. The screening runs, the reachable phase's
  runs and the invariant phase's analysis runs read only verdicts and make
  their solver `verdicts_only`: they spend no re-solve. In the replay above,
  only the runs whose outcome carries a solver-chosen value re-solve; the
  other replayed runs, the fresh context and the in-order replay are kept as
  they were, though the printed output no longer needs them (removing them
  is a separate change). With `unshared_counterfactual_runs` every
  per-invariant run re-solves, since its outcomes are printed directly.
- **No descriptive probes (#1868).** The counterfactual runs (screening,
  per-invariant and replayed runs, and the reachable phase's runs; all through
  `counterfactual_bounded_run`) call
  `fsl_verifier::verify_bounded_without_coverage`: they ask no
  `action_coverage` probe and no vacuity lane. The counterfactual code reads a
  run's violation (kind, name, step, last action and, in a run that re-solves,
  its trace), its reachables and its `leadsTo` verdict, never its
  `action_coverage`, `vacuity` or deadlock trace, and those probes were most of
  the phase's solver time (on `examples/agentic_rag/agentic_rag_design.fsl`,
  `html` at depth 8 spent about two thirds of its wall time in the
  `action_coverage` probes of counterfactual runs). The output is unchanged:
  both probe families are scoped push/check/pop queries that assert nothing,
  so every verdict is the same, and a printed trace is re-solved from the
  assertions alone (#1940), so the queries the run no longer asks cannot move
  it. Before #1940 they did: skipping them changed a printed trace of
  `examples/self/refinement_algebra.fsl`. A skipped probe that would have
  answered `unknown` failed its run before; no spec in the corpus has one. The
  witness run is not a counterfactual run and keeps both probes. Test:
  `counterfactual_runs_ask_no_coverage_or_vacuity_probe` (`main.rs`) records the
  query kinds the witness and counterfactual runs ask on a small spec.
- **Tests.** `shared_counterfactual_runs_print_the_unshared_output`
  (a unit test in `rust/fslc/src/main.rs`) compares the JSON of `explain` at depth 4
  on `examples/nfr/support_sla.fsl` and `examples/self/refinement_algebra.fsl`,
  without `cost.elapsed_s`, with a reference whose invariant phase makes each
  per-invariant run in order on one context: no screening, no fair-removal
  sharing, no fresh context and no replay (`unshared_counterfactual_runs`,
  read in `invariant_outcomes` and `invariant_counterfactuals_from_source`).
  The reachable phase has no such switch: both sides skip weakenings without
  a reachable and share fair-removal runs (in
  `reachable_counterfactuals_from_source`), so the test does not check those
  shortcuts. `explain_counterfactual_runs_are_pinned` pins, on one
  small spec, the property and weakening operator of each entry of both
  phases and the number of bounded runs and concrete searches. `screening_keeps_counterfactuals_behind_a_preempting_bound`
  checks that a screening run stopped by a type bound
  screens nothing: the counterfactual and the preemption note remain.

## 5. Implementation / tests

`explain` is implemented in native Rust in `rust/fslc/src/main.rs`; the
frozen Python `src/fslc/explain.py` is a compatibility reference only.

- **Entry.** `explain` parses with `mutate` and `typestate`
  (`MutationFamilyOptions`) and calls `run_explain` (from `command`).
  `run_explain_from_source_with_names` runs
  `run_explain_lowering_from_source` for `--lowering` (its
  output is `result`, `spec` and `lowering` only) and `run_explain_from_source`
  otherwise, then respells the output with
  `name_display::respell`, glossary aliases unless `--names canonical`.
- **JSON.** `run_explain_from_source` returns the trace gate's error for a
  spec whose acceptance/forbidden traces the gate refuses
  (`require_trace_gate`, #1242), then adds `counterfactual_scope`, the
  `skeleton` from `model_skeleton` and, for an `fsl 5`
  requirements document, `readings` (`explain_readings::of_source`). Without
  `--readable` it adds `witnesses` (`witness_scenarios`, `explain_witnesses`),
  `counterfactuals` (`invariant_counterfactuals_from_source`) and, when
  non-empty, `reachable_counterfactuals`
  (`reachable_counterfactuals_from_source`). Both phases read their document through
  `counterfactual_document` and try the same weakenings,
  `counterfactual_weakenings`: mutate's built-in
  `assignment_remove`/`requires_remove`/`fair_remove` mutants in
  `fsl_tools::enumerate_builtin_mutants` order (#1169). `weakening_json`
  prints each as its `weakening` object; its `init`
  argument is the one difference between the phases: the reachable phase
  marks a mutant of the `init` block (`BuiltinMutant::edits_init`), the
  invariant phase every mutant whose action is named `init`, which includes
  the mutants of an action named `init`.
- **Readable text.** The `readable` branch of `run_explain_from_source` writes the header, `Verification
  world:`, `State:`, `Lifecycles:`, `Implements:`, `Actions:`, `Properties:`
  and `Automatic checks:` (type bounds only) into `readable`. `Implements:`
  comes from `readable_implements_text_from_source`, which
  runs `implements_result_from_source` bounded by `--depth`.
- **Display names.** Names in that branch are printed through `model.names()`
  public spellings, and branch actions recover their authored name from
  origin metadata (§6).
- **Tests.** `rust/fslc/tests/explain_cli.rs` (skeleton, branch display
  names, `spec_kind`, `auto_checks`, generated-declaration provenance,
  readable origins and counterfactual controls); the #1350 files
  `issue_1350_explain_lowering.rs` (named guards in both counterfactual
  phases), `issue_1350_preemption.rs` and
  `issue_1350_preemption_fallback_edges.rs` (non-target preemption and the
  concrete fallback search) and `issue_1350_shared_guard.rs` (one weakening
  credited to every affected invariant or reachable); `fsl5_implicit_readings.rs`
  (`readings`); and in `main.rs`
  `readable_explain_runs_no_bounded_verification` (#1839)
  plus the #1838 tests in §4. The frozen Python `tests/test_explain.py` covers
  the Python reference (cart_v1 skeleton, `ShippedWasPaid` via assignment
  removal, cancel_flow requirement text, compose, no raw
  `internal_invariant`). It also expects no counterfactual for
  `NonNegativeRevenue`; that historical first-kill behavior is not the native
  contract after #1350.

## 6. Native skeleton restoration (issues #528, #530)

The native Rust `model_skeleton`/`--readable` projection independently walks
`KernelModel` rather than reusing Python's raw `source_lines[line-1].strip()`
slicing (§3), and had drifted from the documented contract on three fronts,
fixed together because they share the same root cause — a synthesized
declaration with no registered `OriginChain`:

- **`spec_kind`** was hard-coded `null`. It is now `source_dialect(source)`
  (`"kernel"`/`"requirements"`/`"business"`/`"domain"`/`"db"`/… — the same
  classification `fslc lint`/`fslc chain` already use), threaded through
  `run_explain` from the raw source text alongside the built model.
- **`auto_checks`** enumerated `type_bound` only. It now also enumerates one
  `partial_op` entry per partial-operation site in each action's
  `requires`/`lets`/`statements`/`ensures`, deliberately without the Public
  Kernel walk's per-branch failure-condition computation, which a static
  enumeration does not need.

  **Amendment (#1166).** The listing first shipped with its own walk, which
  counted no `Seq` index read and skipped quantifier/aggregate binders and
  assignment targets, while `verify` reported `_partial_<action>` for all of
  them. The set of partial operations is now owned once by `fsl-core`
  (`fsl_core::PartialOperation`, six kinds: `head`/`pop`/`at`/`index`/
  `divide`/`remainder`), and `explain` lists `fsl_core::action_partial_operations`:
  every occurrence in evaluation scope, binder range/collection/`where` parts
  and indexed assignment targets included, with an index read counted only when
  its collection is a `Seq` (a `Map` index is total). The verifier's candidate
  check, the Public Kernel's `partial_operations` classification and the
  runtime's `RuntimeError::partial_operation` (which replaced matching the
  error message) use the same enum. A `ModelError` converted into a
  `RuntimeError` is never tagged: its `division by zero`/`remainder by zero`
  come from constant folding at model build, which `check` rejects. A statement-level `forall` binder's site is
  reported with the binder's text as `text`. `typed_agreement`'s
  `partial_inventory_sweep_agrees_across_explain_kernel_verifier_and_runtime`
  checks every kind in every placement against all four consumers, comparing
  the Kernel's entries as an exact multiset. The Kernel expands a quantifier or
  aggregate into one term per finite candidate, so one authored site becomes
  several entries, and a `Seq` collection binder adds one synthesized
  `collection.at(i)` read per candidate whose failure condition is guarded by
  that candidate's own `i < collection.size()` membership and so cannot fire;
  the test counts both explicitly and proves the guard structurally. A
  statement-level `forall` is expanded the same way (#1190): one entry per
  candidate for its binder's `where` and for every partial operation in its
  body, with the binder replaced by the candidate and the failure condition
  guarded by the candidate's membership and `where`. Before #1190 the Kernel
  skipped the binder and walked the body with the binder out of scope, so a
  body operation that read the binder failed `fslc kernel` with "cannot type
  identifier"; the test's exclusion row for that placement is gone.
- **Branch lowering and generated declarations carried no provenance.**
  `branches { when P { … } maps Q }` in a `requirements` action lowers to one
  physical Kernel action per branch, named `name__bN` (issue #528); the SLA
  `time { deadline … }` block synthesizes a `tick` action and one
  `_deadline_*` invariant per deadline with no authored name at all (issue
  #530). Neither ever bound an `OriginChain`, so `model_skeleton`'s existing
  `origin_display_name` fallback had nothing to recover the authored
  identity from and printed the lowered internal name instead. Both
  synthesis sites in `dialect.rs::lower_requirements` now bind one: a branch
  gets `generated: true` with a real `primary` span (the authored action's
  own declaration) and a `lowering_steps` entry of `kind: "branch"` naming
  its guard and `maps` correspondence; `tick`/`_deadline_*` use
  `OriginChain::generated_only` (no authored span exists for them at all).
  `--readable` recovers the authored action name from this origin the same
  way `model_skeleton`'s JSON path already did, prints each branch's guard
  under a `branch:` line, and — the "synthesized refinement mapping" bullet
  in §2 — an `Implements:` section from the existing
  `requirements_implements_output` when the source declares
  `implements X from "Y"`.

## 7. Implicit readings of FSL 5 requirements (#1817)

Several FSL 5 requirements forms take their meaning from a name match, a word
count or line order: `with qty` stores the input in the field `qty`; the first
`lifecycle` line's first stage is initial and the stages are ordered as they
first appear; `Shop Order` (two words) makes the event name the verb; every
event but a `fault event` is fair; `allow … only when` adds a guard rather than
granting permission. Each was documented, but no output said which reading was
taken, so an edit could change meaning without a message. Decision (user,
2026-11-11): output only, in `explain` and `lint`; no language change (an
explicit `initial` declaration was suggested and is out of scope).

- **Where the readings come from.** Only the desugaring knows why an input is
  stored, whether a verb was written and how many lifecycle lines there were.
  It records `EventReading` / `LifecycleReading` on
  `NextEditionRequirements::event_readings` / `lifecycle_readings`, which the
  lowering never reads. Kernel origins are not extended: they reach the Kernel
  and the existing `skeleton.actions[].origin` of `explain`, so every
  document's `check`, `kernel` and existing `explain` output stays as it was.
  Fairness and the guards are the Kernel action's, found through the event's
  origin (its `event` lowering step and declaration span), so `explain` shows
  what `verify` checks.
- **`explain`.** JSON gains one key, `readings`, only for a requirements
  document headed `fsl 5` (LANGUAGE.md §13.2 lists its fields). Each guard is
  marked `conjunctive: true` with the origins `explain --lowering` gives it.
  `--readable` adds a `Lifecycles:` section and `verb:` / `fair:` / `input …:`
  / `guards:` lines; no existing line changes. `explain --lowering` is
  unchanged. `fslc html` embeds the `explain` JSON in its raw-data section, so
  its report of such a document carries `readings` too and an `html` approval
  record of it reports `rendering_changed`; the template is unchanged, so
  `HTML_RENDERER_VERSION` stays (`DESIGN-approval.md`, "Renderer version"). The output is respelled like the rest of `explain`'s, so enum-like
  values (`same_name`, `set`, `relation`) avoid words a glossary would name.
- **`lint`.** The three readings an edit can silently change are info notes
  (`input_stored_by_name`, `verb_from_event_name`,
  `initial_stage_from_line_order`; the last only with more than one lifecycle
  line), taxonomy `ambiguous_intent`, in `files[].notes[]` with a top-level
  `note_count`, both present only when there is a note, so `lint` of every
  other document prints the bytes it printed before (the full-stdout goldens
  of `issue_1434_publication.rs` and `specialized_preflight_preservation.rs`
  hold). They are kept out of `findings` because `finding_count` is the
  lint outcome (`outcome.rs`, "done at `finding_count: 0`") and `lint` exits 1
  whenever it reports a finding: as findings they would turn every such
  document's `lint` from exit 0 to 1. Fairness and conjunctive guards are not
  notes: they cannot change without an edit to the `fault` keyword or the
  rules, which already says what it does.
- **Evidence.** `rust/fslc/tests/fsl5_implicit_readings.rs` (English and
  Japanese spellings, each reading made explicit as a negative control, notes
  beside a finding). Base `9bcff3eb` and head `check`, `kernel` and `lint`
  exit codes over `specs/`, `examples/` and `rust/fslc/tests/fixtures/` are
  identical, and head `explain` JSON equals base apart from `readings`.
