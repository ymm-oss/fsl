---
name: fsl-from-code
description: Reverse-engineer a witnessed FSL design-layer kernel spec from existing code, using the fsl 5 kernel form. Harvest state/actions/guards/effects, confirm cross-action invariants and negative intent with a human, verify and mutation-test, then wire and execute the real-code conformance harness. Use when code is input and a design spec is the deliverable, not requirements/business discovery or design from an upper contract.
---

# FSL From Code — implementation → witnessed design model

The seam points downward: code → source-witnessed kernel spec → real-code
conformance tests. There is no inferred upper requirements contract. If one is
later agreed, building the upward refinement seam belongs to
[design](../fsl-design/SKILL.md), not automatic reverse-engineering.

Before syntax read [core](../fsl/SKILL.md), its relevant references and
[verify](../fsl-verify/SKILL.md). Produce:

- A kernel `spec` headed `fsl 5`, with `// SRC: file:line` witnesses for
  state/actions/guards/effects and any source-grounded invariants.
- Human-confirmed cross-action safety/history/progress intent, carried in
  `// ASSUME-n:` comments and typed requirement/model/assumption tags.
- A testgen Adapter wired to real code, plus executed random-walk/replay evidence.

Do not invent states/guards/invariants to reach green, create a business or
requirements layer, or claim conformance from model verification alone. A kernel
spec cannot acquire requirements-only scenario/forbidden blocks. Express confirmed
negative intent as supported invariant/trans/history/progress properties and
real implementation rejection tests; route separately to requirements authoring
only when the user asks for that contract.

## Three zones

| Zone | Evidence and boundary |
|---|---|
| A: harvest | Read member state/types, externally callable actions, leading guards and assignments from code; attach exact source witnesses. |
| B: discover intent | Cross-action conservation, ordering, forbidden interaction sequences and responses are questions, not fabricated facts. A rule absent from any single line needs human confirmation. |
| C: truth-check | Model mutation asks whether properties have teeth; executed conformance asks whether modeled behavior matches real code. Neither proves source intent by itself. |

A code mirror can pass conformance while checking no independent rule. Do not
mistake that hollow quadrant for completion, but do not invent rules to escape it.

## Workflow

0. **Fit.** Apply core's state-machine/documentation filter. Interaction order,
   flags, permissions, async/retry may justify verification; an otherwise-needed
   design document can justify a thin model. Static display/glue with no state
   usually needs ordinary tests. Do not reject every CRUD flow categorically or
   fabricate a forbidden state to make one qualify.
1. **Harvest.** Scope one stateful subsystem. Identify state variables, finite
   domains, externally callable entry points, actual guards and effects. Put a
   `// SRC: file:line` on each candidate. Private helpers/internal calls are not
   automatically actions. Preserve actual transaction and concurrency boundaries.
2. **Memo in chat.** Use the question set below. Zone B is questions; domain bounds
   are labeled choices with lost coverage stated. Get human confirmation before
   writing behavior-changing assumptions; do not make another memo deliverable.
3. **Write.** Kernel form only, carrying every source/assumption witness. Use confirmed
   invariants/trans/leadsTo/ghost state as appropriate. No source comment is a
   substitute for a checked declaration. Prefer entity/number with verify sizes
   when those bounds are modeling rather than product truth.
4. **Verify.** Check → strict traceability/lint → verify depth 8 → induction using
   the core repair protocol. Decide whether a failure is a code misread, wrong
   rule, abstraction mismatch or real code defect; ask before choosing.
5. **Anti-hollow.** Mutate plus positive reachable witnesses. Triage surviving
   mutants/empty_formalization into specific Zone-B questions, accounting for
   equivalent/dead/beyond-depth mutations rather than inventing an invariant.
6. **Conformance.** Testgen → wire reset/step/observe to actual code → run the
   harness/random walk/replay. Classify spec/Adapter/code divergence, fix the
   correct owner or explicitly record a deliberate abstraction. Generated tests
   do not explore inside-action crashes/interleavings; add real-boundary tests
   when atomicity matters.

## Formalization memo — ask explicitly

1. **Conservation:** for state written by several actions, what aggregate must
   always agree (counter vs records, consumed vs delivered, etc.)?
2. **Forbidden sequences:** which individually legal operations must not combine
   into a bad state or prohibited call? What state/history property expresses
   it, and which real-code test should attempt it?
3. **Relational/ordering intent:** what ordering/relation must hold regardless of
   the last action? Which source or owner confirms it?
4. **Action/helper boundary:** what is independently callable by external users,
   workers, queues or cron? Which internal calls must stay inside one modeled step?
5. **Concurrency:** locks/transactions/async imply which interleavings? A sequential
   model does not cover races. Confirm the intended atomic operations rather than
   silently flattening multiple transactions.
6. **Domain size:** what bugs require more than N instances? An artificial bound on
   arbitrary lists/integers changes findable bugs, so label the loss in ASSUME-n.
7. **Multi-entity shape:** single struct, Map of cases, multiple actors/components?
   Do not force code into a Map-shaped example when its actual boundaries differ.

Keep only confirmed assumptions in source. Unconfirmed invariants/response goals
remain questions; do not claim them as extracted facts with misleading SRC tags.

## Two-axis anti-hollow evidence

| | Model mutations killed | No model mutations killed |
|---|---|---|
| Conformance passes | properties constrain some modeled changes and observed code agrees | hollow-mirror warning; investigate confirmed intent |
| Conformance fails | constrained model, wrong machine/projection or code defect | both faithfulness and strength unresolved |

Inspect individual survivors and by-requirement attribution, not just one absolute
kill-rate. A covers ID with no guard-kill credit is not by itself a missing rule;
read the actual mutants. `empty_formalization` is observed sensitivity at this
depth/operator set, not proof of absent intent.

When runnable code exists, mutate **code** (remove an update/flip a branch) and
confirm the anchor rejects the expected divergence. This is stronger evidence of
real-bug detection than model mutation alone. When the implementation cannot run,
model mutation is still useful, but implementation conformance stays unestablished.
Independent blind intent review is optional for high risk, not a new mandatory
agent workflow or permission to transmit source externally.

## Guardrails and handoff

- Trace every action and sourced invariant; use requirement/model tags as
  [readable documentation](../fsl/references/layers.md#authoring-specs-as-readable-documentation).
- Keep source fidelity and confirmed intent distinct. Weak sensitivity is a
  question queue, not permission to weaken the model or invent policy.
- Unwired tests fail by default; --allow-unwired skips where supported, so check
  actual executed count. A green empty/all-skipped suite is not an anchor.
- Report model safety/progress proof, bounded scope, mutation evidence and
  implementation conformance separately, with exact commands and source state.
- Without runnable code/logs, say “model checked; implementation not anchored”.
  Do not call a faithful-looking source extraction a proof that the code follows it.
