# FSL — `fslc mutate` (spec mutation) implementation design

Motivation: issue #6 (category 4/7 of roadmap #1). A spec whose invariants are too weak or
missing stays silently verified (under-constraint). There was no mechanism to measure "how much
the set of properties constrains the model's behavior." Even when tags are present, "whether
that formalization actually constrains anything" (semantic traceability) is invisible to #5's
existence check. This productizes mutation as a repeatable non-triviality check.

## 1. CLI

`fslc mutate <f> [--depth K=8] [--by-requirement] [--oracle-attribution]
[--max-mutants N=200]
[--from mutants.jsonl] [--fail-on-survivors] [--min-kill-rate R]`. Output
`result:"mutated"`, exit 0 (a generator in the same family as scenarios/testgen;
survivors are review data, not failures, unless an explicit gate is requested —
see "Opt-in gate" below).
**That is the code for its own result, not an unconditional 0.** `mutate` verifies the
spec first and re-emits that baseline envelope unchanged when it is not `verified`, so a
spec that fails its own verification exits with the baseline's code and produces no
mutants at all. Since #1002 a failed inline `implements` seam is such a baseline
(`refinement_failed` / `impl_violated`, exit 1).

`--from` adds every external JSONL record after the selected built-in catalog.
`--max-mutants` caps only built-in enumeration; `--max-mutants 0 --from ...`
is the external-only adjudication form.

## 2. Mutate the **dialect-expanded kernel AST**, not the spec dict

It mutates the kernel AST `("spec", name, items)` returned by `parse_src` (with
compose/requirements/business/domain already expanded), and **re-runs `build_spec` for each
mutant** before checking. Reasons:
1. **The type-bound ±1 mutation requires regenerating the `_bounds_*` invariants that
   `build_spec` produces** — directly mutating the spec dict leaves them stale and the mutation
   has no effect.
2. Derived consistency such as `phys_vars` can be left to build_spec.
3. Dialects are handled uniformly without mutation-specific grammar or verification semantics.

### `domain` (#727): the rendered kernel path, not direct lowering

Native `fslc mutate` accepts `domain` documents by rendering them through the same textual
kernel path `fslc domain expand` uses (`fsl_tools::domain_kernel_source`, i.e.
`fsl_core::domain_kernel_source` in `fsl-core/src/domain.rs`) and mutating the **re-parsed**
kernel spec, rather than the direct-lowering path `check`/`verify` use for domain
(`fsl_core::lower_domain`, `fsl-core/src/dialect.rs:2575`, which delegates to
`fsl-core/src/domain_lowering.rs`). Direct lowering does propagate real spans into the domain
source file at many sites, not one — measured across the public kernel contract for
`examples/domain/order_async_effect.fsl`, both paths emit 409 spans with 16 null (an unrelated,
equal-across-both-paths class), but direct lowering resolves to only 23 distinct non-null source
positions against 90 for the rendered path, so it collapses many distinct mutants onto the same
witness location. The rendered path is chosen for that ~4x finer discrimination, at the cost that
its `loc` points into the rendered `kernel_source` text rather than a line of the domain source
file on disk (direct lowering's `loc` does resolve against the real file). That cost is why
`kernel_source` is embedded in the output envelope (the same field `domain expand`/`domain check`
already emit), so a witness is resolvable from the envelope alone without re-deriving the
rendering. The two lowering paths' span behavior — including the shared 16-null-of-409 count and
the 23-vs-90 discrimination gap — is measured, not merely asserted, and kept from silently
drifting by `fsl-core/tests/domain_render_agreement.rs`, which compares both paths' output while
excluding only `span` itself.

Consequences that follow from mutating the rendered text rather than the domain source:

- Witness `loc` and `target` values refer to the rendered kernel; `target` uses the generated
  action names (e.g. `CapturePayment_SuccessSticky`) and does not map back to domain source lines.
- Mutants inside actions that are dead in the verified baseline carry the existing "action dead at
  baseline — survival expected" note (§3); for domain documents this note is a primary hollowness
  signal — for example, a saga whose compensation actions are structurally unreachable reports
  every compensation-targeting mutant surviving with this note, which is the intended negative
  control, not a defect.
- Absolute kill-rates are not comparable across dialects because domain lowering emits few
  properties; read domain mutation evidence differentially against a base tree and through the
  survivor/dead-note profile, not as a raw kill-rate threshold.
- Unlowerable domain constructs (e.g. `on_stale` policies, top-level `await`, `value_object`
  invariants — #710/#711/#712) are rejected by the shared lowering guard
  (`validate_lowerable_constructs`, invoked inside `domain_kernel_source`) with a located
  `kind:"semantics"` diagnostic before any mutant runs. In practice `mutate`'s own baseline gate
  (§3) already rejects these first, since a domain document the guard would reject also fails
  `run_verify`'s baseline check.
- Aggregate invariants phrased over `can()` are expanded against the current guards at lowering
  time; kernel-level mutants therefore never violate them, and their absence from the kill set
  must not be read as hollowness. Their drift coverage is source-level: re-verification after a
  guard edit re-expands `can()` and falsifies the invariant (measured in #771). For the same
  reason, `--by-requirement`'s `DOMAIN-INVARIANT` bucket reports `kills:0` with an
  `empty_formalization` warning for every domain spec with a `can()` invariant (confirmed on
  `order_async_effect.fsl`) — that warning is this same structural blind spot surfacing through a
  machine-readable field rather than prose, and must not be read as evidence the invariant is
  hollow either.
- `--from` external mutants are adjudicated against the rendered kernel text, not the domain
  source: a `replace:{target,...}` instruction whose `target` is domain source text matches
  nothing in `source` (which mutate's domain arm has already replaced with `kernel_source`) and is
  reported `invalid`, fail-closed. External domain mutants must therefore be written against the
  `kernel_source` a prior `mutate`/`domain expand` run on the same file emitted, not the `.fsl`
  domain document.

### Mutation operators (deterministic enumeration, no randomness)

| op | error simulated | AST operation |
|---|---|---|
| requires removal | missing guard | delete `("requires", …)` from body |
| requires negation | mistaken condition | wrap with `("not", e)` |
| assignment removal | missing update | delete `("assign", …)` |
| enum swap | wrong transition target | change `("var", member)` to another member of the same enum |
| integer/bound ±1 | off-by-one | `("num", n)`±1, `("type", n, lo, hi)`'s lo/hi ±1 |
| then/else swap | mistaken branch | swap an `if` whose both branches are non-empty |
| fair removal | missing leadsTo fairness assumption | flip the action's fair True→False |

Integer and numeric type-bound mutations that would overflow are omitted. Empty
numeric domains remain materializable kernel inputs and are adjudicated by the
ordinary oracle, including its zero-action-instance behavior.

### External JSONL contract

Each nonblank line is one JSON object with an optional unique string `id`,
optional `op`/`description`, and exactly one mutation form:

```json
{"id":"m1", "mutated_spec":"spec WholeMutatedSource { ... }"}
{"id":"m2", "replace":{"target":"exact raw source", "replacement":"new text"}}
{"id":"m3", "replace":{"target":"repeated text", "replacement":"new text", "occurrence":2}}
```

`spec` is accepted as an alias of `mutated_spec`; the replacement fields may
also appear at the top level. Replacement is deliberately textual and strict:
without `occurrence`, `target` must match the baseline raw source exactly once;
`occurrence` is positive and 1-based. Missing/ambiguous targets are `invalid`,
not guessed. The resulting source is parsed relative to the baseline file's
directory (so dialect includes retain their normal resolution) and must expand
to the same spec name as the baseline.

Malformed JSON, duplicate/invalid ids, invalid record shapes/instructions,
parse/name/type/semantic build errors, and a different spec name are generation
quality failures with `status:"invalid"`. They never count as kills. Once an
external source builds, it goes through the exact same BMC + acceptance +
forbidden + refinement oracle as a built-in mutant.

## 3. Kill oracle and baseline gate

Each built-in mutant = mutated AST → `build_spec`; each valid external mutant =
mutated source → `parse_src` → `build_spec`. Both then run **`verify` (BMC,
depth K) + acceptance/forbidden
replay + implements refine**. If any of these returns violated/reachable_failed/
refinement_failed, or BMC rejects the mutant with a semantic error (for example an action
body that is undefined in a reachable state, reported as `killed_by:"build_spec"`) →
**killed** (killer recorded). All clean → **SURVIVED**. Induction is not used (`unknown_cti`
makes the kill decision ambiguous and slow).
**Baseline gate**: if the pre-mutation spec is not verified, refuse (in a buggy spec every
mutant is killed trivially, which is meaningless).

Parse/name/type/semantic construction failures are intercepted before the kill
oracle and classified `invalid`, for built-in and external mutants alike
(#1251). A built-in mutant that does not lower/build used to be a `build_spec`
kill on the grounds that the AST catalog is compiler-owned; but a mutant that
never reached the oracle says nothing about the spec's constraint strength, so
counting it as killed inflated `kill_rate`. It now carries
`invalid:{kind:"semantics",message}` like an external one.

**Oracle errors (#1251).** The oracle is three-valued: clean, killed, or
*error* — it could not judge the mutant. An error is: Z3 could not be created
(`error.stage:"solver"`, previously the `internal` kill); BMC failed because the
solver answered `unknown`, the backend failed, or its model could not be read
back — a state value, a trace step's action choice, or a ranking value the
model does not provide (`stage:"bmc"`; the verifier marks these with
`VerifyError::is_solver_failure`, distinguishing them from the semantic errors
above, which stay kills); or the acceptance/forbidden oracle or the implements
oracle returned an error (`stage:"requirements"` / `"implements"`, previously
the error string itself or `refinement` as the killer for built-ins and
`invalid` for externals). Such a mutant is published with `status:"error"`,
`killed_by:null`, and `error:{stage,message}`; it is never killed, never
`invalid`, excluded from both sides of `kill_rate`, counted in
`summary.errored` (and per source; the key appears only when non-zero, see
§5), and fails any requested gate. **Known residual:** a BMC error the verifier
does not mark as a solver failure stays the `build_spec` kill. That includes
"unsupported expression" and "ill-typed value" errors, which are arguably
limits of the verifier rather than findings about the mutant, and the `Seq`
length projection errors ("model sequence length is negative / exceeds
capacity"), which are likely the same type-bound projection gap #1283 closed
for enum ordinals; both are left for follow-ups. The rule is
the same for both sources: a failure before the mutant builds is `invalid`, a
failure after it builds is `error`. The init-assignment `_bounds_<state>`
re-attribution applies only to judged (clean/killed) outcomes, so it cannot
hide an error. Without a gate the run still exits 0, and a note states how many
mutants could not be judged.

**Inconclusive (#1262).** The implements refine shares `check_refinement`'s
fixed correspondence-walk budget (50,000 states, #1041). When a mutant's walk
reaches it before deciding within the depth, the mutant is neither killed nor
survived: it is `status:"inconclusive"` with
`inconclusive:{"reason":"unknown_budget","states_explored":N}` and
`killed_by:null`, for built-in and external mutants alike. Counting it
`survived` would claim the spec missed a mutant it was never fully checked
against; counting it `killed` would inflate `kill_rate` (the fail-open
direction #1251 describes for oracle failures). A later decided oracle wins
over the cutoff (a built-in `_bounds_*` init kill stays `killed`). An implements
oracle that returns an error rather than a verdict is `error` (above), not
`inconclusive`: the two undecided states stay distinct because they call for
different action (fix the tool or input vs. accept or raise the budget).

**Status precedence (#1251, #1262).** One function (`mutant_status` in
`rust/fslc/src/main.rs`) decides the `status` of every mutant that reached an
oracle, for both sources (external records rejected before any oracle are
published as `invalid` earlier):
`invalid` is exclusive (the mutant never reached the oracle); otherwise
`killed` > `error` > `inconclusive` > `survived`. A decided kill beats both
undecided states: the oracles run in order and stop at the first kill, so a
kill is evidence the spec detects the mutant whatever a later oracle would have
said (this includes the `_bounds_*` re-attribution of a clean outcome, which is
why a kill after a cutoff wins). The re-attribution never applies to an
`error`, so an error is never turned into a kill. `error` beats
`inconclusive`: an error means some oracle did not run at all — a tool or
environment fault the user must fix — while a cutoff means the walk ran within
its budget; the error is the more severe and more actionable condition. Both
are outside `kill_rate` and both fail a requested gate, so the choice changes
only which reason is reported (in the current oracle order the two cannot
co-occur on one mutant, but the rule is fixed so it cannot drift).

## 4. `--by-requirement` (requirement stress report) — the reverse definition

"What breaks if you remove an invariant" is **fundamentally a no-op for safety**: deleting an
invariant only reduces what is checked and produces no violation (monotonicity). An invariant
can only demonstrate its work by **catching a behavior mutation**. Hence the correct
mechanization is reversed: the kill oracle records each mutant's killer → aggregate by the
`killed_by` requirement tag. **A requirement that killed no behavior mutation = an empty
formalization**, warned as `empty_formalization`. v1 records the first-killer and explicitly
labels this "lower observation bound". Acceptance and forbidden kills are attributed through explicit requirement annotations on the
failed trace declaration; their AC/FB case IDs remain unique scenario identities, not requirements.

## 4b. `--oracle-attribution` (opt-in full killer projection)

Default `mutate` output is unchanged: `killed_by` remains the first-killer lower
observation bound and `kill_rate` remains mutant union coverage. With
`--oracle-attribution`, each killed mutant additionally carries a `killers` array
listing every oracle display name that rejected it (invariant, reachable, ensures
action name, `_bounds_*`, acceptance, forbidden, refinement, …), evaluated
independently so the set is order-independent. The envelope also adds
`by_obligation` keyed by those same oracle display names with `kills_any`,
`sole_kills`, and `shared_kills` counts, plus `attribution:{mode:"all_killers",
order_independent:true}`. These fields are **observed lower bounds** within the
chosen mutant set and depth — not obligation completeness, not spec correctness,
and not an assurance-class upgrade. The flag is explicit because full attribution
is more expensive than first-killer recording; when the flag is off, nothing is
fabricated (no empty `killers` arrays on the default path).

## 5. Output / ripple

```json
{"result":"mutated","spec":"…","depth":8,"baseline":"verified",
 "summary":{"total":N,"killed":K,"survived":S,"invalid":I,"kill_rate":0.75,
            "by_source":{"builtin":{...},"external":{...}},
            "errored"?:E,"inconclusive"?:C},
 "mutants":[{"op","loc","target","status","killed_by","requirement","source",
             "invalid"?,"error"?,"inconclusive"?}],
 "by_requirement":{"REQ-7":{"kills":0,"warning":"empty_formalization"}},
 "notes":["mutant cap 200 reached: 37 dropped"]}
```

With `--oracle-attribution` only:

```json
{"mutants":[{"status":"killed","killed_by":"Funded","killers":["Funded","deposit"],…}],
 "by_obligation":{"deposit":{"kills_any":1,"sole_kills":0,"shared_kills":1}},
 "attribution":{"mode":"all_killers","order_independent":true}}
```

New `src/fslc/mutate.py`. Deterministic enumeration + `--max-mutants` truncation is made
explicit in `notes` (no silent cap). Survivors of coverage-false actions are annotated as
"dead at baseline," and equivalent mutants go to a review queue (not a hard failure).
The combined and per-source kill rates use `killed / (killed + survived)`;
`invalid` records are excluded from the denominator and retained as external
generation-quality evidence (an `invalid` record says nothing about the spec's
constraint strength — it failed before reaching the kill oracle, so counting it
either way would distort the score). `inconclusive` mutants (§3) are excluded
from the denominator too, and so are `error` mutants. `summary` and each
`by_source` entry carry the `errored` and `inconclusive` counts only when they
are non-zero, so a run in which every mutant was decided keeps its existing
envelope byte for byte; one policy covers both undecided counts (#1251 adopted
#1262's rather than always emitting `errored`). A reader computes
`total = killed + survived + invalid + errored + inconclusive`, reading an
absent count as 0. Built-in entries have `source:"builtin"`;
external entries add `id`, `source:"external"`, `input_kind`, and JSONL `line`.
Mutation uses the ordinary bounded verifier, including its normal termination
after the initial state when a model has no action instances.

### Opt-in gate (#1237)

`--fail-on-survivors` and `--min-kill-rate R` (a number in `[0, 1]`; anything
else is a usage error, exit 2) turn the run into a CI gate. Without either flag
the envelope and exit code are byte-for-byte what they were. With one or both,
`result` stays `"mutated"` and the envelope gains a `gate` object:

```json
{"gate":{"fail_on_survivors":true,"min_kill_rate":0.8,"judged":19,"survived":13,
 "kill_rate":0.3158,"dropped":0,"violations":["survivors","kill_rate_below_min"],
 "passed":false}}
```

`gate.passed` decides the exit code (0 when true, 1 when false), exactly as
`semantic_diff`'s explicit gate does. The rules are fixed so the verdict can be
reproduced from the JSON alone:

- `judged` is `summary.killed + summary.survived`; `invalid` external records
  and `inconclusive` mutants are excluded, as in the kill-rate denominator.
- Any `inconclusive` mutant (§3) adds the violation `inconclusive`, whichever
  flag was given, and `gate.inconclusive` carries the count (the key appears
  only when non-zero): each one could be a survivor the run never decided, so
  the gate fails closed rather than passing on a kill rate that left it out.
- Zero judged mutants fails either flag with the single violation
  `no_judged_mutants` (for example `--max-mutants 0` with no `--from`): a run
  that adjudicated nothing is not evidence that nothing survives.
- Any oracle error (`summary.errored > 0`, #1251) adds `oracle_errors` under
  either flag, whatever the other counts, and `gate.errored` carries the count
  (only when non-zero): the gate fails closed on mutants nobody adjudicated
  instead of passing on them. `oracle_errors` is listed before `inconclusive`.
- `--fail-on-survivors` adds `survivors` when `survived > 0`. Survivors dead at
  baseline count; there is no equivalent-mutant exclusion yet.
- `--min-kill-rate R` adds `kill_rate_below_min` unless the published,
  four-decimal `summary.kill_rate` is `>= R` (so `0.75` passes `R = 0.75` and
  fails `R = 0.7501`).
- Built-in mutants beyond `--max-mutants` are not judged; their count is
  recorded as `gate.dropped` (and in `notes`) but does not fail the gate.
- Because survivors can now fail the run, the first `notes` entry no longer
  says they are "a review queue, not a hard failure"; it says the run
  requested a gate and that `gate.passed` decides the exit code. The ungated
  note is unchanged.

A baseline that does not verify is re-emitted unchanged with its own exit code;
the gate never applies to it and no `gate` key appears.

### What the score means (and does not)

`kill_rate` is **bounded mutant-set sensitivity**: the fraction of a selected
finite mutant set that the existing net of checks rejects, at the selected
`--depth`, under the BMC + acceptance + forbidden + refinement oracle. The
number moves with every one of those choices — operator mix, `--max-mutants`
truncation, depth, and oracle composition — so it is comparable only across
runs that hold them fixed. It is not a production defect-detection rate, not a
probability that the specification is correct, and not a completeness measure.

Survivors are a review queue, not failures and not automatic missing-invariant
findings (the opt-in gate above is the user's explicit choice to treat them as
failures; it does not change what a survivor means). A survivor may be an equivalent mutant (same behavior, no property
can distinguish it), behavior dead at baseline (annotated via coverage), an
effect only observable beyond the depth bound, or genuine under-constraint —
only the last calls for a spec change. Symmetrically, `empty_formalization`
and the per-requirement kill counts are **observed lower bounds** within the
chosen mutant set and depth: "killed nothing here" never proves a requirement
is vacuous, and a high kill count never proves it is fully formalized.

## 6. Tests / related

tests/test_mutate.py: cart_v1 guard removal → `_bounds_stock` kill / type-bound +1 kill
(evidence of AST mutation + rebuild) / thinned-invariant survivor / `empty_formalization` /
baseline refusal / coverage-false annotation / truncation annotation / corpus stability /
exit 0. A semantic-level extension of #5 strict-tags; #7 explain's counterfactuals narrate
these kills per invariant. Roadmap #1.
