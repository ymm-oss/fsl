---
name: fsl-verify
description: Run, read, and report fslc verification for an FSL spec at any layer (business, requirements, design, or the whole chain). Use when asked to verify, check, prove, or gate a spec ("verify this spec", "is it proved", "is it ready to hand over", "can development start from this", "how strong is the spec", "did this change break anything"), to choose which fslc commands to run and in what order, to tell a real pass from a bounded or undecided one, to fix a spec from verifier output without weakening it, to measure spec strength with mutate, or to write the completion report. Use the layer skills (fsl-business, fsl-requirements, fsl-design) to author the spec and fsl for syntax.
---

# FSL Verify — choose the run, read the result, report the claim

This skill owns verification: which `fslc` commands a spec needs, how to read
their JSON without over-reading it, how to repair from the output, and how to
report what was and was not shown. The layer skills keep only their minimal
check loop and point here. For syntax read `../fsl/SKILL.md`; the full command
catalog is `../fsl/references/commands.md` and the repair table per result
kind is `../fsl/references/errors.md`.

Three facts drive everything below:

- **Exit 0 is not "passed".** Several outputs that matter (`deadlock` and
  `unreferenced_requirement` warnings, `diff` findings, `mutate` survivors,
  `not_evaluated` seams) arrive with exit 0. Read the fields named in the
  tables, not only the exit code.
- **Every claim has a scope.** `verified` is bounded by `--depth`. `proved`
  holds for the `verify { instances / values }` sizes it ran at, and the
  envelope does not repeat those sizes — you must.
- **Undecided is not a pass.** `unknown_cti`, `unknown_budget`,
  `not_evaluated`, `not_run`, and `closure: false` mean the question is still
  open.

## 1. Decision table — which commands, in which order

Run the "required" column in order and stop at the first non-pass; repair (§4)
and re-run from the top of the column. "When relevant" items are part of the
job when the condition holds, not optional extras.

| Input | Change | Required, in order | When relevant |
|---|---|---|---|
| `business` | new | `check` → `lint` → `verify --depth 8` → `verify --engine induction` | As-Is/To-Be: `refine tobe.fsl asis.fsl map.fsl`; strength: `mutate --depth 4 --by-requirement` |
| `requirements` | new | `check --strict-tags --requirements ids.txt` → `lint` → `verify --depth 8` (read `implements.result`) → `verify --engine induction` → `scenarios` | strength: `mutate --by-requirement`; NFR deadline: a tighter deadline must fail |
| design (kernel `spec`) | new | `check` → `lint` → `verify --depth 8` → `verify --engine induction` → `refine design.fsl req.fsl map.fsl` | `unknown_cti`: `--lemma` or `--engine explicit` (§3); strength: `mutate`; handoff: `testplan` / `testgen` |
| any spec | edit of an existing spec | the column for its layer, then `diff old.fsl new.fsl --forbid behavior_added,behavior_removed,invariant_weakened,forbidden_relaxed,unknown` (or `diff --git BASE..HEAD --forbid ...`) | `refine` again against the **unchanged** upper layer; `mutate` kill rate against the previous baseline |
| several layers | any | `chain fsl-project.toml` | deepen only the failing layer with its own column |
| implementation | write or fix code | `testgen` or `testplan` → run the implementation's tests → `replay` | never lower `--depth` to make generated tests pass |
| failure that depends on size | any | `sweep --depth 1..8 --instances E=1..3` | `counterexample export` for the minimal case |

Notes on the columns:

- `ids.txt` is a file with one requirement ID per line (the IDs from your
  formalization memo), not a comma list.
- `--depth K` includes step K. Depth 8 is the working default for `verify`;
  `mutate` at depth 8 can take minutes, so depth 4 is the usual strength run —
  say which depth you used.
- In `diff --forbid`, list every kind your change policy rejects. A kind you
  do not list never fails the gate: `--forbid behavior_removed` alone exits 0
  on a change that only added behavior. Include `unknown`.
- `refine` against the upper layer passes a change that only makes the lower
  layer refuse more (a strengthened guard); only `diff` reports it, as
  `behavior_removed`. Decide whether the refusal is intended.

## 2. Results that look green but are not

Each row was reproduced with `fslc 4.11.0`; the field is what you must read.

| You see | It means | Next move / how to report it |
|---|---|---|
| `result: "verified"`, `completeness: "bounded"`, `note: "bounded verification: no violation within depth 8"` | no violation within K steps | run `--engine induction`; if no `proved`, report "bounded to depth K" as the claim |
| `result: "proved"` (induction), `completeness: "unbounded"` | invariants hold at every depth **for the `instances` / `values` sizes of this run**; a `proved` at `instances Item = 2` was `violated` at step 3 with `--instances Item=3` | report the sizes next to `proved`; never write "for any number of" items or values. `sweep --instances Item=1..3` shows the smallest size that fails (`minimal_counterexample`) |
| `proved` with `warnings[].kind: "no_user_invariants"` | only implicit type bounds were checked | report it as "type bounds only"; the spec states no safety property |
| `verified` / `proved`, exit 0, `warnings[].kind: "deadlock"` (`deadlock.found: true`, `at_step`) | a reachable state where nothing is enabled; the default `--deadlock warn` keeps exit 0 | decide if that stop is intended: if so declare it in `terminal { }`; if not, it is a bug. Report it either way |
| `unknown_cti` (exit 1), `invariant`, `cti`, `hint` | induction did not decide; the CTI start state may be unreachable | §4 step 5; the BMC `verified` of the same spec does not settle it |
| `unknown_budget` (exit 1), `states_explored: 50000` — from `refine`, or as the top-level result and `implements.result` of a requirements `verify` | the correspondence search hit its fixed budget; nothing was decided | narrow the `verify {}` domains of both layers, or verify each layer separately; report as open |
| `--engine explicit`: `verified`, `closure: false`, `hint: "state space not saturated..."` | bounded, like BMC | raise `--depth` until `closure: true` (then `result: "proved"`), or use induction |
| `check --strict-tags --requirements ids.txt`: `result: "ok"`, exit 0, `warnings[].kind: "unreferenced_requirement"` | an ID in `ids.txt` is carried by nothing | not done until there are zero `unreferenced_requirement` and zero `untagged` warnings |
| `verify --property X`: top-level `verified`, exit 0, `implements.result: "not_evaluated"`, `reason: "property_selection"` | the upper-layer seam was skipped | never use a `--property` run as the completion run; the seam passes only on `implements.result == "refines"` |
| `testplan`: `formal_result: "not_run"`, `assurance_effect: "none"` | a plan; nothing was verified | run the tests or `replay` against the implementation |
| `diff` without `--forbid`: `result: "semantic_diff"`, exit 0, `summary: ["behavior_added"]`, `gate.passed: true` | findings are reported, not gated | add `--forbid` (§1); gate on `gate.passed` / `gate.violations` |
| `diff` finding `unknown` (for example `forbidden_replay_failed`) | the comparison could not decide that case | list `unknown` in `--forbid`; never read it as "no change". In fslc 4.12.0 and later a NEW guard that disables a forbidden's **setup** step is reported this way, so `--forbid forbidden_relaxed` alone misses it |
| `mutate`: `result: "mutated"`, exit 0, `summary.survived: 18`, `summary.kill_rate: 0.3077` | survivors never change the exit code | §6; for a gate add `--min-kill-rate R` or `--fail-on-survivors` |
| `chain`: `[impl]` layer passed with `evidence = "exit_code"` (`warnings` has `impl_exit_code_only`) | the test command exited 0; no evidence a test ran | give `[impl]` a JUnit `report` so an empty run is `indeterminate` |
| fslc 4.12.0 and later: `proved` with `not_covered: ["int_overflow"]` | the induction step does not check integer overflow; only the base case (to `--depth`) does | run `--engine bmc` or `--engine explicit` at a depth that reaches the large values, or bound the value; report the gap |

## 3. Reading the deciding fields per result

| Result | Read | Then |
|---|---|---|
| `error` (exit 2) | `kind`, `message`, `loc`, `diagnostic_code`, `hint` | fix the syntax or type at `loc`; `../fsl/references/errors.md` |
| `violated` | `violation_kind`, `invariant`, `violated_at_step`, `violating_bindings`, `last_action`, `trace[].changes` | decide (§4 step 3) before editing |
| `reachable_failed` | `unreached`, `action_coverage`, `blocking_requires`, `hint` | depth too small, or a guard blocks the goal |
| `unknown_cti` | `cti.states`, `invariant`, `hint` | §4 step 5; `--engine explicit --depth N` for a small space |
| `proved` | `k_used`, `base_depth`, `invariants_checked`, `warnings` (and `not_covered` in 4.12.0+) | report with the scope |
| `refinement_failed` / `impl_violated` | `kind`, `impl_action`, `impl_trace`, `abs_before`, `abs_after_expected` / `abs_after_actual`, `mismatch` | fix the lower layer or the mapping |
| `refines` | `checked_to_depth`, `action_map` | bounded by that depth |
| `semantic_diff` | `summary`, findings, `gate` | gate with `--forbid` |
| `mutated` | `summary.kill_rate`, `summary.survived`, `mutants[]` with `status: "survived"`, `by_requirement` | §6 |
| `lint` | `finding_count`, `files[].findings[].code` | done at `finding_count: 0` |

## 4. Fix loop

1. Branch on `result`, not on the exit code alone (§2, §3).
2. `error`: fix at `loc` using `expected` / `hint`. Re-run `check` before anything else.
3. `violated`, `refinement_failed`, or `impl_violated`: from the trace, decide which of the
   following it is: **a missing requirement, a wrong interpretation, or an
   intended exception.** If the source does not decide it, stop and ask the
   human — do not pick a guard to reach green.
4. `reachable_failed` / `action_coverage: false`: read `blocking_requires`.
   After adding any guard, re-check coverage: an over-tight guard makes a goal
   unreachable while every invariant stays green.
5. `unknown_cti`: look at `cti.states[0]`. Add an auxiliary invariant only if
   it is **true of the domain** and excludes that start state; test candidates
   with `--lemma "EXPR"` (only `proved` candidates count) and write back only
   what the run recommends. Or close the space with `--engine explicit` until
   `closure: true`.
6. A failed seam (`implements`, `refine`, governance `preservation`): repair
   the lower layer or the mapping. **Never change the upper layer** to make the
   lower one pass; if the upper contract looks wrong, report the
   counterexample and ask its owner.
7. After every repair re-run, in order: `check` → `lint` → `verify` →
   induction → the seam → `mutate` (the kill rate must not fall below the
   last accepted value).

## 5. Prohibited (these make a run look better without making the spec better)

- Lowering `--depth`, `instances`, or `values` to get green. A smaller run is
  a smaller claim: report it as such, next to the full run.
- Deleting or weakening an `invariant`, `forbidden`, `acceptance`, `leadsTo`,
  `policy`, or `goal` to get green.
- Using `--property`, `--exclude-property`, `--from-state`, `--deadlock ignore`,
  or `--vacuity ignore` in the run you report as complete.
- Reporting `unknown_cti`, `unknown_budget`, `not_evaluated`, `not_run`, an
  `unknown` diff finding, or `closure: false` as a pass.
- Editing the upper layer, a mapping, or a generated snapshot only to make a
  lower layer pass.
- Claiming any result you did not run in this session and read the output of.

## 6. Spec strength (mutate)

The baseline uses auto (1,000,000 states), accepting verified or proved; an
unaccepted result reruns BMC on a fresh thread and continues only if verified.
JSON still says `baseline:"verified"`. A boundary-probe finding wins first;
after a completed empty probe, explicit can certify clean (50,000 states, all
reachables witnessed through K); if neither decides, existing BMC chooses the
verdict/killer. Secondary checks remain.
Stderr progress shows baseline and per-mutant time plus the main-oracle route
(`explicit`, `bmc`, `probe`, `none`), not the later check deciding final status.
Stdout is unchanged except for the encoding-only residual in DESIGN-mutate §3:
explicit-clean can survive where BMC alone reported `build_spec`.
Refused baselines and command errors have no progress.
This optimization does not make mutation unbounded or guarantee a fast run.

`mutate` changes the model (a guard, an assignment, a bound) and checks
whether your properties notice. A survivor is a change your spec cannot tell
apart from the original.

- `summary.kill_rate = killed / (killed + survived)` for this mutant set and
  depth. Compare it with the previous accepted run of the same spec; a drop is
  the signal, one absolute number is not.
- Read each `mutants[]` entry with `status: "survived"` (`op`, `target`,
  `loc`). Some survivors are equivalent mutants (for example widening a type
  bound no action reaches); `notes` says to review them by hand.
- `--by-requirement` fills `by_requirement` with kills per requirement ID; it
  stays `{}` on an untagged spec. An ID with few or no kills has a weak check.
- What to add for a real survivor, when the source states the rule:
  - a guard mutant survives → a `forbidden` trace that calls the action where
    the guard must reject it;
  - an assignment or effect mutant survives → an `acceptance` trace with an
    `expect` on the changed value, or an `invariant` / `trans` over it;
  - a goal stays reachable no matter what → a `reachable` or `leadsTo` that
    the mutant breaks.
  If the source states no such rule, list the survivor as a question.

## 7. Completion report

The report is **the last message of the task**. Before writing it, wait for
every background command to finish or stop it; do not start anything after
it. If a result arrives after the report, write the whole report again — never
a postscript that only describes the late result.

Use this table, one row per run you rely on:

| Command | Exit | Result | Depth / scope | Proof class |
|---|---|---|---|---|
| `fslc lint req.fsl` | 0 | `lint`, `finding_count: 0` | — | — |
| `fslc verify req.fsl --depth 8` | 0 | `verified`, `implements.result: refines` | depth 8; `Claim = 3`, `Amount = 0..3` | bounded; refines (depth 8) |
| `fslc verify req.fsl --engine induction` | 0 | `proved` | `Claim = 3`, `Amount = 0..3` | proved (at these sizes) |
| `fslc mutate req.fsl --depth 4 --by-requirement` | 0 | `mutated`, kill rate 0.81 (18 survived) | depth 4 | strength, not proof |

Proof classes: **proved** (with the sizes), **bounded** (with depth and
sizes), **refines** (with depth), **not_run**, **unknown** (with which value:
`unknown_cti`, `unknown_budget`, `not_evaluated`, `unknown` finding).

After the table:

- **Open items**: every unknown, warning (`deadlock`, `no_user_invariants`,
  `unreferenced_requirement`, `not_covered`), survivor you did not resolve, and
  question for a human.
- **Not shown**: what these runs do not cover — larger sizes, depths beyond
  the bound, the implementation (until `testgen` / `replay` ran against it).
- Keep design invariants, the refinement to the upper layer, and
  implementation conformance as separate claims.
