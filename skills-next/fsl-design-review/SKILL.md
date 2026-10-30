---
name: fsl-design-review
description: Review FSL design proposals, variants, extensions, changes, and substitutability against a frozen abstract contract. Use fslc refine plus independent model verification, enabledness review and implementation anchoring where required, reporting scoped evidence in SOLID/LSP/OCP/DbC vocabulary. Use fsl-design for authoring a design from requirements; this skill owns the review procedure, not language syntax.
---

# FSL Design Exploration — contract-conformance review

Translate “is this design good?” into explicit contract questions. Design principles
are judgment lenses, not properties that the verifier magically proves. Read
[core](../fsl/SKILL.md) and its needed references for syntax/repair, and
[verify](../fsl-verify/SKILL.md) for runs/claims. For a new design-layer file use
[design](../fsl-design/SKILL.md), then return here. Kernel proposals are headed
`fsl 5`; requirements surface grammar remains owned by its layer skill and maps
through canonical lowered names.

## Step 1 — identify the open and closed parts

Whether comparing proposals, adding a variant, reviewing a change or judging
substitutability, agree in chat on:

- **Open:** what may change, variants, extension points and changed sites.
- **Closed:** observable state, guards, invariants and responses that must hold.
- Action/state correspondence, intended stutters, and human-owned judgments.

OCP lens: choose the stable boundary. DIP lens: identify the shared abstraction.
Do not infer that boundary, variant roster, action mapping or extension point
from design taste. Ask where source material does not settle it. No separate
deliverable memo is needed; confirmation in chat suffices.

## Step 2 — freeze and verify the abstract contract

Write only observable contract state; do not import implementation mechanics.
Check → lint/traceability → bounded verify → induction using the verification
skill. If the abstract spec violates itself, resolve that contract first; a
contradictory abstraction cannot support proposal review.

DbC lens: requires/ensures/invariant are actual contracts; implicit type bounds
and partial-operation checks should not be duplicated as user invariants.
Do not enumerate concrete variants in the abstract enum just to support each
extension. Put their types in detail and project them with conditional maps.
An owner can explicitly revise an abstraction, but that is a contract change,
not repair of a supposedly frozen one.

## Step 3 — model each proposal and its seam

Each proposal is a detail spec plus explicit mapping file. Preserve the frozen
abstract file. A detail-only action maps to stutter only if it changes no
mapped abstract state. Ask about unclear correspondence. A module-addition
extension uses supported compose/synchronization and a cross-cutting invariant;
it is not automatically refinement or a new invented composite syntax.

LSP lens: refine checks trace inclusion. Detail steps must correspond to an
enabled allowed abstract step (or genuine stutter), with equal mapped effects
and preserved checked safety. Verify the abstract spec independently; checking
corresponded steps is not independent verification of every abstract behavior.

**Guard strengthening is permitted by refinement.** A detail withdrawal allowed
only against cleared funds can refuse an abstractly allowed call and still refine.
That is not full LSP (“no stronger preconditions”). Check enabledness separately:

- never_enabled_action catches only an action with no enabled instance through
  depth K; bounded warning/exit 0, made failing by --vacuity error.
- With matching before/after state/actions, diff reports behavior_removed;
  --forbid behavior_removed,unknown gates lost/undecided enabledness.
- Otherwise compare every detail guard to its abstract counterpart explicitly;
  general enabledness preservation remains a separate obligation (#1232).

SRP/ISP concern-splitting and visibility/internal discipline may guide compose,
but fslc does not prove that the software follows these design principles.

## Step 4 — run checks and adjudicate the findings

Run `fslc refine impl.fsl abs.fsl map.fsl`, and verify each model independently.
Include strict-tags/requirements traceability, lint, and explain --readable.
For new requirements contracts, audit rule/decision origins against intended
events before using the lowered contract as an abstraction.

Before changing anything ask: bad proposal, wrong mapping, or excessive/misread
contract? The last can be a legitimate review outcome, but needs owner approval.
Present the counterexample instead of silently changing frozen guards or mappings.

| Evidence | Design meaning |
|---|---|
| refines | Checked detail behavior is allowed by the contract at reported depth; safety half only, not full substitutability. Report guard review separately. |
| abs_requires_failed | Detail bypasses an upper guard/control. Read impl_trace and the requirements origin if present. |
| abs_state_mismatch | Init/effect/safety disagrees after mapping; inspect mismatch path before blaming design. |
| stutter_changed_abs | Claimed internal step is externally visible; false encapsulation claim. |
| map_partial_op | Undefined correspondence expression, not a substitutability judgment. |
| abs_partial_op | Abstract body has no defined outcome at mapped enabled state. |
| abs_step_violation | Abstract successor breaks its own contract; read abs_violation/abs_after_attempted. Even unchanged mapped state does not make a forbidden step valid. |
| impl self-violation | Detail invalid on its own; resolve before correspondence judgment. |
| unknown_budget | No decision within correspondence budget; never call refines. |
| never_enabled_action / uncovered | Dead within checked depth only; fully disabled extension point or over-strong guard candidate. |
| unknown_cti | Induction undecided; CTI start may be unreachable, not established phantom or true contract. Explore/prove candidates independently. |
| progress_lost | Explicitly pulled upper response failed; inspect lower fairness and lasso/stall. |

### Non-vacuity audit

Green verify/refine can be hollow, especially with ghost-carried contract facts.
For important contracts inspect mutation sensitivity and vacuity:

- Very low kill rate is a reason to inspect dynamic constraints and equivalent/
  dead/beyond-depth mutants, not a universal numerical correctness threshold.
- tautology_over_frozen catches frozen-ghost tautologies; it does not cover all
  over-constraint or equivalent-mutant cases.
- For a specific invariant, a targeted --property run plus one known bad guard
  mutation should produce the expected violated property. Do not present a
  preservation test that stays green as a detector.
- Pair tightening repairs with positive reachable witnesses. A killed negative
  alone does not show intended behavior remains possible.
- Liveness mutants must actually allow a pending stall; deleting fair is no
  detector if every execution already terminates structurally.

## Step 5 — separate proof from design judgment

Report:

1. **Checked evidence:** exact commands/exits, domain sizes, depths, bounded/proved
   properties, seam result, and enabledness review. Never call refine alone full
   substitutability or OCP/LSP compliance.
2. **Counterexamples:** translate the trace into operations/control bypasses;
   reconcile requirement ID/text with confirmed intent.
3. **Human judgment:** SRP/ISP/DRY/KISS/Demeter, interface quality, naming and
   whether the chosen stable boundary is the right one.
4. **Implementation boundary:** consistency of models is not real-code conformance.
   If required, anchor at the implementation's layer with wired testgen tests or
   replay logs. Actually execute tests; zero executed/all-skipped is not evidence.

Do not write “proved SOLID compliance”. Faithfulness to stakeholder intent also
requires the confirmed memo; the verifier does not decide that intent for humans.

## Principle correspondence and optional risk checks

| Principle | Mechanism / limit |
|---|---|
| LSP | trace inclusion safety; stronger detail guards still pass, precondition side needs review |
| OCP | frozen abstraction plus extension/refine; same enabledness limitation |
| DbC | native requires/ensures/invariant |
| DIP | detail→abstract contracts embody dependency direction |
| SRP/ISP | decomposition/compose/internal discipline, not a proved principle |
| DRY/KISS/Demeter | software structure judgment outside FSL |

For AI-authored/heavily patched specs, analyze --profile ai-review can reveal
disconnected requirements, unanchored properties, cycles, unread state and
conservation candidates. Keep formal_status:not_a_violation and ask the owner;
do not weaken a contract to silence review signals. For multiple high-risk variants
check every variant × frozen contract and guards. For before/after review use
old as abs/new as impl plus diff with explicit forbidden kinds including unknown;
refine alone allows additional refusals. Prefer all-depth proof for stable safety
contracts while stating fixed sizes and progress/overflow limits.
