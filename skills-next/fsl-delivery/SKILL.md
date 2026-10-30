---
name: fsl-delivery
description: Orchestrate FSL-centered delivery from business planning through PM requirements, engineering design, verification, refinement, scenarios, testgen/replay, and implementation conformance for files headed fsl 5. Use for planning-to-implementation workflows, connected business/requirements/design contracts, or coordination of the FSL layer and review skills.
---

# FSL Delivery

Coordinate the lifecycle; do not replace the layer owners or mix model proof,
refinement, and implementation conformance into a single green claim. FSL is the
checkable contract spine, not the whole product process. A readable spec can
replace a prose document even when verification risk is modest; discovery,
UI/API design, coding and ordinary testing still use their normal tools.
Reverify the living corpus and its seams after changes.

## Routing

Before authoring, read only the relevant owner:

- [Business](../fsl-business/SKILL.md): As-Is/To-Be, actors, stages, controls,
  policies, KPIs/goals, governance preservation.
- [Requirements](../fsl-requirements/SKILL.md): externally observable entity/
  lifecycle/event/rule/decide contracts, scenarios/forbidden traces, NFRs and
  optional business implements seam.
- [Design](../fsl-design/SKILL.md): internal kernel state/actions,
  compose/refinement and implementation handoff.
- [Design review](../fsl-design-review/SKILL.md): proposal/change, frozen
  contract, substitutability and principle-based judgment.
- [From code](../fsl-from-code/SKILL.md): existing implementation → witnessed
  design model, owner-confirmed invariants and downward conformance anchor.
- [Verify](../fsl-verify/SKILL.md): run sequence, results, origin self-check,
  proof limits and final evidence table.
- [Core](../fsl/SKILL.md): source fidelity, syntax, commands and repair.
- Retained `fsl-requirements-document` skill from the base bundle: checked
  requirements/spec → deterministic stakeholder document; edits only the
  non-normative slot. It is intentionally not duplicated in this staging tree.

DB compatibility, Functional DDD/effects, AI tools/agents and external evidence
are cross-cutting [dialects](../fsl/references/advanced.md), not extra mandatory
stages. Keep their structural/statistical/runtime claims distinct from kernel
proof. Do not drop these routes or invent a unified composite dialect.

## Workflow

1. **Entry point and scope.** No stable material → discovery. Agreed business
   contract → requirements. Agreed requirements → design. Existing code/logs →
   extraction or conformance. A safe-change question → design review. Do not
   fabricate an upper layer just to fill a delivery table.
2. **Delivery memo in chat before new FSL:** target flow/risk or documentation
   payoff, source artifacts, layers and filenames, upper contract per lower
   layer, positive/negative examples, implementation anchor if known,
   behavior-changing assumptions and open human decisions. Obtain confirmation
   for choices the source does not settle.
3. **Author one layer at a time.** Business speaks stages/controls, requirements
   external behavior, design internal mechanics. Use `fsl 5` for kernel/business/
   requirements examples as their owners prescribe; requirements process forms
   are not the new edition. Do not apply layer syntax to unrelated dialects.
4. **Verify before depending on a layer:** check → strict traceability → lint →
   verify depth 8 → induction, then its scenarios/seams/origin audit. Fix the first
   non-pass with the owner; exit 0 alone can carry warnings, skipped seams,
   diff findings or surviving mutants.
5. **Connect downward as verified seams:**
   - Requirements implements business; require nested `refines`, not merely
     an ordinary-looking top-level result. Preserve canonical names, actor
     correspondence, stage order and no-bypass history maps.
   - Design refines the **lowered** requirements kernel. Read canonical
     explain/lowering origins before mapping; no glossary aliases in maps.
   - Implementation conforms at the Adapter's abstraction granularity. Reuse
     upper forbidden negatives only, not upper positive scenarios that a sound
     refinement may realize with additional internal steps.
   - Run chain when a manifest exists. A test-running [impl] needs a fresh JUnit
     report with at least one executed test, no failures/errors. All-skipped/
     empty/unwired is indeterminate, not conformance. Exit-code-only evidence
     is for non-test commands, flagged explicitly.
6. **Report categories separately.** Model consistency, bounded/proved scope,
   refinement, and real implementation anchoring each need their own evidence.
   Use [verify's completion table](../fsl-verify/SKILL.md#7-completion-report).

## Stage gates

| Stage | Deliverable | Required evidence |
|---|---|---|
| Business | business/governance contract | check, lint, bounded verify, induction; preservation when declared |
| Requirements | requirements with source IDs, rules/decision tables, scenarios and forbidden cases | strict check, lint, verify, induction, scenarios, origin self-check; implements only on refines |
| Design | kernel spec and explicit upper mapping | check/lint/verify/induction, refine; progress separately or explicitly preserved |
| Implementation | real Adapter/log | actually executed conformance tests or replay; plan/scaffold alone is not an anchor |
| Review/change | proposal or before/after | verify each, refine frozen contract; diff with explicit rejected kinds including unknown where comparable; strength controls by risk |

For every authored layer run strict traceability using ids.txt (one source ID
per line), lint with no findings, and explain --readable as the human source view.
New requirements additionally needs its source→origin rule/row audit and both
spellings checked when Japanese is delivered. For high-risk contracts add
verify --vacuity error and mutate --by-requirement; read survivors, not just exit.
After changes, use the verify skill's full diff policy and rerun unchanged-upper
refinement; --forbid behavior_removed alone misses added behavior and unknowns.

## Claim discipline and human gates

- Green business/requirements/design means consistency of the model under the
  confirmed interpretation and reported bounds, not stakeholder intent truth.
- Only refines passes a seam; unknown_budget/not_evaluated are open. Refinement
  preserves checked safety, permits stronger guards, and is not full LSP.
- Liveness does not automatically propagate. Verify the response-owning layer
  or use supported preserve-progress mapping; list sizes/depth/proof scope.
- An Adapter/log anchors only what executed at its granularity. Generated tests
  do not establish inside-action interleaving/crash atomicity.
- Stop for missing guard, exception, deadline, retry, ownership, lifecycle or
  abstraction/mapping decisions. Confirm stutters and implementation projection.
- If a counterexample could be “fixed” by weakening the upper contract, ask its
  owner rather than silently choosing. On conformance divergence, classify
  spec-intent mismatch, Adapter error, or code defect before editing code.
- Representation-only finite sizes may be chosen and labeled, never silently
  promoted into product requirements or generalized beyond the checked world.

## Output

| Layer | File | Result | Scope / seam / anchor |
|---|---|---|---|
| Business | actual file or not used | proved / bounded / unknown | sizes and controls |
| Requirements | actual file | result + implements | source coverage and origins |
| Design | actual file + map | result + refine | depth/sizes/progress limits |
| Implementation | Adapter/log | conformant / not anchored | executed tests / observed records |

Then give exact commands/exits, open decisions, unshown behavior, and one next
concrete command/file. Do not claim completion before the required evidence.

Typical requests: take cancellation from business to implementation; select real
layers for a feature; connect requirements/design/conformance; review whether an
implementation-oriented design still preserves PM requirements.
