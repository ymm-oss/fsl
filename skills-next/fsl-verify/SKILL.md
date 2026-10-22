---
name: fsl-verify
description: Run, read, and report fslc verification for an FSL spec at any layer (business, requirements, design, or the whole chain). Use when asked to verify, check, prove, or gate a spec ("verify this spec", "is it proved", "is it ready to hand over", "can development start from this", "how strong is the spec", "did this change break anything"), to choose which fslc commands to run and in what order, to tell a real pass from a bounded or undecided one, to fix a spec from verifier output without weakening it, to measure spec strength with mutate, or to write the completion report. Use the layer skills (fsl-business, fsl-requirements, fsl-design) to author the spec and fsl for syntax.
---

# FSL Verify — choose the run, read the result, report the claim

This skill owns verification: which `fslc` commands a spec needs, how to read
their JSON without over-reading it, how to repair from the output, and how to
report what was and was not shown. The layer skills keep only their minimal
check loop and point here. For syntax read the layer skill (the requirements
forms of files headed `fsl 5` are in `../fsl-requirements/references/syntax.md`)
and `../fsl/SKILL.md`; the full command catalog is
`../fsl/references/commands.md` and the repair table per result kind is
`../fsl/references/errors.md`. The diagnostics that only files headed `fsl 5`
produce are in §8, and the origin self-check that only they need is §9.

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
| `requirements` | new | `check --strict-tags --requirements ids.txt` → `lint` → `verify --depth 8` (read `implements.result`) → `verify --engine induction` → `scenarios` → the origin self-check (§9) | strength: `mutate --by-requirement` (§6); NFR deadline: the variant without its `urgent` event must fail; glossary: `lint --strict-names`, and §9 on both spellings |
| design (kernel `spec`) | new | `check` → `lint` → `verify --depth 8` → `verify --engine induction` → `refine design.fsl req.fsl map.fsl` | `unknown_cti`: `--lemma` or `--engine explicit` (§3); strength: `mutate`; handoff: `testplan` / `testgen` |
| any spec | edit of an existing spec | the column for its layer, then `diff old.fsl new.fsl --forbid behavior_added,behavior_removed,invariant_weakened,forbidden_relaxed,unknown` (or `diff --git BASE..HEAD --forbid ...`) | `refine` again against the **unchanged** upper layer; `mutate` kill rate against the previous baseline |
| several layers | any | `chain fsl-project.toml` | deepen only the failing layer with its own column |
| implementation | write or fix code | `testgen` or `testplan` → run the implementation's tests → `replay` | never lower `--depth` to make generated tests pass |
| failure that depends on size | any | `sweep --depth 1..8 --instances E=1..3` | `counterexample export` for the minimal case |
| a file without the `fsl 5` header | move it to the edition | `migrate --edition 5 file.fsl` (plan) → `migrate --edition 5 --write file.fsl` → the column for its layer | a requirements `process` it cannot rewrite, or whose rewrite would change the kernel, stops with `migrate_manual_required` and its reason, and nothing is written. Re-run the whole column on the migrated file |

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
- A requirements spec headed `fsl 5` is verified through its lowered kernel:
  every command above reads it as it reads any spec, and the results name the
  lowered state (`return_stage`) and the event names. Its rules and decision
  tables are `requires` lines and properties of that kernel; §9 shows which.

## 2. Results that look green but are not

Each row was reproduced with `fslc 4.11.0`, and each row that starts with
`fsl 5` on a file with that header; the field is what you must read.

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
| `fsl 5` requirements: `verified`, exit 0, `deadlock` warning in a state where every instance is in a final stage | this edition derives no `terminal { }` for requirements (a business spec derives one from its sink stages) | if the stop is intended, declare it: `terminal { forall r: Return { stage(r) == Refunded or stage(r) == Rejected } }`; otherwise it is a bug |
| `fsl 5` `check`: `ok`, exit 0, `warnings[].code: "never_rule_without_forbidden"` | a rule rejects calls (`allow` / `never`) but no `forbidden` trace ends in an event it guards; nothing shows the rejection works | add the `forbidden … then rejected` the source implies; never delete the rule to silence it |
| `fsl 5` `check`: `ok`, `warnings[].code: "decide_row_emptied"` | a decision row holds for no value of the fields: its outcome can never be chosen | the row's concepts are wrong or the source is; fix or ask |
| `fsl 5` `check`: `ok`, `warnings[].code: "lifecycle_edge_without_event"` | a lifecycle edge no event moves along: a stage may be unreachable | add the event the source states, or remove the edge |
| `fsl 5` `check`: `warnings[]` with `severity: "info"`, `code: "implied_by_lifecycle"` | an `only after` / `enters … only from` rule the lifecycle already gives | not a failure; keep the rule if the source states it |
| `fsl 5` `mutate --by-requirement`: a decision row's or an event's `covers` ID with `kills: 0`, `warning: "empty_formalization"` | a rule's or decision table's ID counts every killed mutant of the guards it adds (`allow` / `never` / `only after` / the table's guard) besides the kills of the properties it names; a `covers` ID counts only the kills of what carries it (a property, the refinement of its event's action), not those of the guards | read the mutants of the event's guards in `mutants[]` (`target: "request requires #2"`): killed ones show `killed_by` (a property name, `forbidden`, `acceptance`). Only a surviving guard mutant is a weak check |

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
| `error` with `kind: "unsupported_in_edition"` (`fsl 5`) | `findings[].code`, `loc`, `canonical_replacement`, `machine_applicable` | apply the replacement, or `migrate --edition 5 --write` |
| `error` with `kind: "acceptance"` / `"forbidden"` / `"forbidden_setup"` | `id`, `failed_step`, `step` | a scenario step was rejected; a forbidden trace's last step was accepted; a forbidden trace's setup step was already rejected (the trace never reached the call it tests) |

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
- Adding a decision row, an `otherwise`, or an `assuming` the source does not
  state to make `decide_not_exhaustive` / `decide_overlap` go away; deleting
  a `forbidden` trace or a rule to silence `never_rule_without_forbidden` or
  `rule_conflict_unsatisfiable_guard`.
- Reporting a respelled file (`fmt --lang` / `--names`, a hand translation)
  as verified because its other spelling was: run the column on the file you
  hand over.

## 6. Spec strength (mutate)

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
  stays `{}` on an untagged spec. An ID with few or no kills has a weak check
  — except, in a file headed `fsl 5`, a decision row's or an event's `covers`
  ID (§2): read the mutants of its event's guards instead. A rule's or
  decision table's ID counts the killed mutants of its guards.
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

## 8. Diagnostics of files headed `fsl 5`

All are `check` results unless marked. Errors exit 2 with `result: "error"` and the code in
`diagnostic_code`; warnings and infos are `warnings[]` entries of an exit-0
result, with `code` and `severity`. Messages name the alias with the
canonical name in parentheses (`'申請（Claim）'`).

**Edition**

| Code | Means | Next move |
|---|---|---|
| `unsupported_in_edition` (kind, with `findings[]`) | a form the edition makes an error (`legacy_string_metadata`, …) | each finding's `canonical_replacement`, or `migrate --edition 5 --write` |
| `edition_conflict` | `--edition` contradicts the header | drop the flag; the header decides |
| `missing_edition_header` (`lint --edition 5`: info; `migrate`: a planned edit) | the file has no header | `migrate --edition 5 --write` inserts it |
| `mixed_requirements_forms` | `process` / `transition` in a requirements spec of this edition | rewrite as `entity` / `event` / `rule` (`migrate --edition 5` does it when it can) |

**Entities, events and scenarios**

| Code | Means | Next move |
|---|---|---|
| `actor_not_declared` | an event or step names an actor no `actor` declares (the message lists the declared ones) | declare it if the source has it; otherwise fix the name |
| `event_not_on_lifecycle` | an event's move is not an edge of the lifecycle (or its `in S` is not a stage) | fix the move or the lifecycle to what the source says |
| `ambiguous_input_field` | a bare name in an event's expression is both its input and a field | write `input amount` or `Return.amount` |
| field `… needs an initial value` (parse error) | an entity field without `= init` | give the initial value the source states, or ask |
| `unknown_verb` / `unknown_event_phrase` / `ambiguous_step` | a scenario step matches no event, no event of that actor (the message lists the actor's verbs), or several | fix the step to the event meant |
| `scenario_instance_overflow` | a trace names more instances than `verify { instances }` allows | raise the bound or reuse an instance; never drop the step |
| `generated_name_collision` | a generated name (`return_stage`, `order_did_pay`) collides with a declared one | rename the declaration |
| `actor_shadows_member` (warning) | an actor is spelled like an enum member | rename one |
| `lifecycle_edge_without_event` (warning) | an edge no event takes | §2 |

**Rules**

| Code | Means | Next move |
|---|---|---|
| `rule_conflict_unsatisfiable_guard` | the guards on one event can never hold together; the message names the rules | the rules contradict each other or a rule is wrong: ask, do not drop one |
| `never_rule_without_forbidden` (warning) | a rejecting rule with no forbidden trace that tries it | §2 |
| `implied_by_lifecycle` (info) | the lifecycle already gives this ordering or entry rule | §2 |
| `invalid_rule_property_name` | a `Name:` on a statement that takes no name, or an invalid name | names go on `always` / `enters` only |

**Decision tables**

| Code | Means | Next move |
|---|---|---|
| `decide_event_not_listed` | an event leaving the table's stage is in no row | add it to the row the source gives it, or ask |
| `decide_not_exhaustive` | no row holds for the values in the message (`amount = 2, AUTO_LIMIT = 1`) | a row the source states, `otherwise` if the source says "everything else", or `assuming Q` if the rest of the spec already excludes those values (then `verify` must prove the generated `_decide_…_assuming` invariant) |
| `decide_overlap` | two rows hold for the values in the message | the source's cases overlap or a concept is wrong; fix the concepts or ask |
| `decide_row_emptied` (warning) | a row holds for no value | §2 |

A table whose fields' value product is too large to enumerate is decided by the
solver in the native `fslc check` instead; the diagnostics are the same.

**Glossary and names**

| Code | Means | Next move |
|---|---|---|
| `canonical_name_not_ascii` | a non-ASCII name is declared with no glossary entry, or a glossary's left side is not ASCII | declare an ASCII canonical name and alias it |
| `alias_collision` / `duplicate_alias` | one alias for two names / two aliases for one name | one alias per name |
| `alias_shadows_name` / `alias_shadows_keyword` | an alias spelled like a canonical name / like a keyword or particle | a longer alias |
| `alias_in_implements` | an alias inside `implements { }` | canonical names there |
| `misplaced_glossary` | the glossary is not the first item, or there are two | move it |
| `mixed_names` (`lint --strict-names`, warning) | one name written both ways | one spelling |

**Japanese keywords, particles and characters**

| Code | Means | Next move |
|---|---|---|
| `particle_role_mismatch` | a particle gives a word the wrong role (`経理 を …`: an actor as the entity) | the particle of its role: が actor, を entity / instance, で input, は check |
| `particle_needs_space` | a particle written onto its word (`申請を`) | a space before it (`fmt` offers the fix) |
| `mixed_particles` | some words of a phrase have particles and others not | all words but the verb, or none |
| `fullwidth_ascii_identifier` | full-width letters or digits in a name (`Ｃｌｅｒｋ`) | the ASCII spelling the message gives |
| `fullwidth_symbol`, `middle_dot_identifier`, `mixed_script_identifier` (`Сustomer` with a Cyrillic `С`), `non_ascii_requirement_id` | look-alike or non-ASCII characters where they do not belong | the replacement the message gives |
| `non_nfc_identifier`, `fullwidth_space` (warnings) | a name not in NFC (read as its NFC form); an ideographic space U+3000 (read as a space) | normalize the text. The editor reports these; a clean `fslc check` does not show that there are none |

## 9. Origin self-check (files headed `fsl 5`)

A requirements spec of this edition lowers each rule statement and decision
row to a `requires` on the events it names, or to a property. A rule that
names the wrong event, a row that guards the wrong verb, or a concept that
means something else still checks and verifies — it constrains the wrong
action. Before reporting, confirm that every rule and row landed where the
source says:

1. `fslc explain --readable spec.fsl`. With an origin-aware `fslc`, every
   action of an event shows `event: …` and every `requires` its origin:
   `← move Requested -> Approved`, `← decide REQ-RET-002 for Requested Return
   row 1: SmallReturn => System approves`, `← rule REQ-RET-001 allow …`;
   every property shows `← rule REQ-… always …` / `enters …` / `assuming`.
2. `fslc explain --lowering spec.fsl` prints the same as JSON:
   `lowering.actions[]` with `action`, the event in `origins`, and
   `requires[]` (`index`, `expr`, `origins[]` of `{kind, detail, line}` with
   `kind` one of `move`, `allow`, `never`, `after`, `decide`), and
   `lowering.properties[]` (`kind`, `name`, `origins[]` with `always`,
   `enters`, `assuming`).
3. Check, against the memo's coverage map: each `rule` statement and each
   decision row appears as an origin on exactly the events the source
   restricts, and nowhere else; each named `always` / `enters` statement is a
   property; no `requires` that came from a rule or row is missing.
4. With a glossary or a Japanese spelling, compare
   `fslc explain --lowering --names canonical` of the two spellings: the
   origins are recorded in canonical names and English keywords, so the JSON
   is identical apart from `line` values. A difference is a translation that
   changed the spec.
5. Report it as a row of the completion table (`explain --lowering`, exit 0,
   "every rule and row on its events", proof class `—`).

If `fslc explain` rejects `--lowering` (`unknown explain option '--lowering'`,
exit 2) and `--readable` prints no `←` origins, the fslc predates the origin
output: read each event's `requires` in `fslc explain --readable` and match
each to its rule or row by hand (the `requirement:` lines name the rule and
decision IDs on each action), and say in the report that the check was manual.
