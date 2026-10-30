---
name: fsl
description: Shared FSL language and native verifier reference for writing, checking, verifying, repairing, explaining, mutating, refining, replaying, generating scenarios/test scaffolds, and interpreting fslc JSON, for files headed fsl 5. Use directly for syntax, kernel specs, errors, and commands. Prefer fsl-business, fsl-requirements, or fsl-design for role-specific authoring and fsl-verify for run selection, result reading, and completion reports.
---

# FSL Core — language, verifier, and repair loop

Read the reference that answers the current question; do not write FSL from
memory. This guide describes files headed `fsl 5`. The header precedes document
annotations and the dialect declaration. Migration is a semantic review, not
a search-and-replace of edition numbers.

## Choose the boundary

| Intent | Primary skill | Deliverable |
|---|---|---|
| Business flows, As-Is/To-Be, controls, KPIs, goals | [fsl-business](../fsl-business/SKILL.md) | business/governance contract |
| PM requirements, acceptance/forbidden behavior, NFRs | [fsl-requirements](../fsl-requirements/SKILL.md) | requirements contract with entities/events/rules/decision tables |
| Internal design, decomposition, mapping to requirements | [fsl-design](../fsl-design/SKILL.md) | kernel spec and refinement seam |
| Design variants, change review, substitutability | [fsl-design-review](../fsl-design-review/SKILL.md) | contract-conformance judgment |
| Existing code → design spec | [fsl-from-code](../fsl-from-code/SKILL.md) | witnessed model and implementation anchor |
| Verify, gate, measure strength, report evidence | [fsl-verify](../fsl-verify/SKILL.md) | command sequence and scoped claims |
| Coordinate actual layers end to end | [fsl-delivery](../fsl-delivery/SKILL.md) | connected delivery workflow |
| Checked requirements → stakeholder document | retained `fsl-requirements-document` skill | deterministic document plus non-normative notes |

Do not turn business discovery into internal design, or a PM request into
implementation work, without permission. Each downward handoff is a refinement
seam, not a plain baton pass: requirements `implements` business; design
refines requirements through a mapping. Freeze the upper contract. A green
model does not establish that real code implements it.

FSL has two useful shapes: one isolated hard state machine, or connected
business ⊒ requirements ⊒ design contracts. Use `fslc chain` for real layers
whose alignment matters; do not manufacture duplicate layers for a single
abstraction. See [when to use](references/when-to-use.md) and
[connected layers](references/layers.md).

## Native CLI and outer contracts

Run `fslc <subcommand> ...`. The native Rust CLI is authoritative; the Python
implementation is a frozen compatibility reference, not the entry point for
new language or CLI behavior. External compilers consume `fslc kernel`, not a
private AST or reparsed expression strings. Check independent runtimes with
`fslc conformance` using the same Kernel major.

Most commands emit JSON; `ledger` and `document generate` emit Markdown,
`html` HTML, and some commands have readable/source/code output modes. Gate on
exit and the deciding fields, not merely parseable stdout:

- 0: command success, **not necessarily a passing verification claim**;
- 1: unsatisfied property or command-specific gate;
- 2: usage/input/specification error;
- 3: internal error.

A failure-class result never exits 0. Which nonzero exit a verdict receives
is command-specific: core check/verify-envelope commands map their listed
verdicts to 1 and an unexpected failure verdict to 3; dialect commands map
non-`error` failures to 1. `error` retains its 2/3 code. Common failures include
`violated`, `reachable_failed`, `unknown_cti`, `unknown_budget`, `nonconformant`,
`refinement_failed`, `impl_violated`, `sweep_failed`, `sweep_inconclusive`,
`observed_mismatch`, `document_drifted`, and chain `indeterminate`.

Inline `implements` is fail-closed during `check`/`verify`/`scenarios`/`testgen`:
only `implements.result == "refines"` passes. Its failing nested result
(`refinement_failed`, `impl_violated`, `unknown_budget`) becomes the top-level
result, exit 1, retaining seam evidence. The correspondence search has a fixed
internal state budget; no CLI flag raises it. Scoped/property/snapshot runs
can skip the seam and report `not_evaluated`; those are not completion runs.

Governance `preservation` refinements run at depth 8. The first non-`refines`
sets the outer failure/exit 1 while each preservation retains its own result.
On seam/preservation failure, testgen returns the verification owner's
envelope and writes no harness. CTIs are unestablished proof findings, not
reachable counterexamples. Ledger/HTML verification errors retain full JSON
and exit 2/3 without an artifact; other failed verdicts can still render.
Rendering-digest changes require review and re-approval, not hand-patched artifacts.

## Before formalizing: source fidelity

FSL specifies requirements; it does not invent them. Before deriving a model
from prose or code, post a **formalization memo in chat**, not a separate
deliverable file:

- Glossary/ledger: candidate domains, state, actors, actions, enabledness.
- Normalize each requirement: trigger, constraint, exception, boundary
  implications (inclusive/exclusive; before/after; at-least/greater-than).
- Coverage: each source ID → checked declaration or explicit unresolved item.
- Assumptions: separate representation choices from behavior-changing choices.
- Questions: missing priorities, stages, timing, retries, ownership, design
  boundaries, action correspondences, and stutters.

Get owner confirmation before encoding a choice affecting schema, enabledness,
effects, invariants, progress, deadlines, or mappings. If asked for a draft,
write confirmed material only and leave questions open. Small finite domains
may be modeling choices, but record their limits; they do not prove arbitrary
population sizes. Have humans adjudicate concrete counterexamples and intent,
not silently accept invented formulas.

Carry confirmed assumptions in the source (`// ASSUME-1: ...`), and tag the
declaration they justify with `@requirement("ASSUME-SCOPE-001", "intent")`.
Use `MODEL-` for explicit modeling choices rather than pretend source IDs.

When the owner intentionally defers a choice, use `@undecided("reason")` on
the declaration. It is metadata, not a weakened verification obligation.
Ledger/HTML show affected requirement IDs; `analyze --profile ai-review`
keeps findings with `acknowledged:true` and `acknowledged_by` visible. No
acknowledgement means unresolved. Never use the annotation to conceal agent
uncertainty or a failed guess. See [annotations](references/syntax.md).

```fsl-fragment
@requirement("REQ-ROUTE-001", "every request is routed")
@undecided("routing policy awaits owner approval")
action route() {
  // confirmed behavior only
}
```

## Standard check and repair loop

1. `fslc check file.fsl`. Fix `loc`/`expected`/`hint` before verification.
   For traceability use `--strict-tags --requirements ids.txt`, one ID per
   line; zero `untagged` and `unreferenced_requirement` warnings is the target.
2. `fslc lint file.fsl`: no findings. Use typed `@requirement("ID", "intent")`
   links (or the dialect's own ID/`covers` syntax), not legacy string slots.
3. `fslc verify file.fsl --depth 8`: read warnings, coverage, and seams too.
4. `fslc verify file.fsl --engine induction`: `proved` is stronger than bounded
   `verified`, but still only for the declared domain sizes. `--depth K`
   includes step K. An unranked `leadsTo` remains bounded; `decreases` can
   support an unbounded response proof. `unknown_cti` is undecided, not truth.
   Independently adjudicated `--lemma "EXPR"` candidates count only if proved.
5. Run the layer's scenarios, seams, origin self-check, and applicable
   mutation/vacuity/implementation checks from [fsl-verify](../fsl-verify/SKILL.md).
   Read [errors](references/errors.md) before choosing a repair.

`verify --from-state state.json --depth 8` is a separate BMC what-if from a
complete Monitor-state snapshot, replacing init and stamped
`bounded_from_snapshot`. Never report it as verification from the real init.

## Minimal kernel example

One complete design-level compilation unit; no requirements-only declarations:

```fsl check
fsl 5
spec Cart {
  const CAP = 3
  type ItemId = 0..1
  type UserId = 0..1
  type JobId = 0..1
  type Qty = 0..5
  enum St { Open, Closed }
  struct Order { st: St, qty: Qty, buyer: Option<UserId> }
  state {
    stock: Map<ItemId, Qty>,
    cart: Option<ItemId>,
    q: Seq<JobId, CAP>
  }
  init {
    forall i: ItemId { stock[i] = 1 }
    cart = none
    q = Seq {}
  }
  @requirement("REQ-CART-001", "one selected item at a time")
  action add_to_cart(i: ItemId) {
    requires cart == none
    cart = some(i)
  }
  @requirement("REQ-CART-002", "a selection may be abandoned")
  fair action abandon() {
    requires cart != none
    cart = none
  }
  @requirement("REQ-CART-003", "checkout consumes available stock")
  fair action checkout(u: UserId) {
    requires cart is some(i)
    requires stock[i] > 0
    stock[i] = stock[i] - 1
    cart = none
    ensures stock[i] == old(stock[i]) - 1
  }
  @requirement("MODEL-CART-001", "this model leaves the job queue unused")
  invariant QueueStaysEmpty { q.size() == 0 }
  trans StockNeverIncreases { stock[0] <= old(stock[0]) }
  reachable SoldOut { stock[0] == 0 }
  leadsTo Served { cart is some(j) ~> cart == none }
  terminal { stock[0] == 0 }
}
```

Bounds above are model sizes. For readable domains prefer `entity Item` /
`number Qty` and a sibling `verify { instances Item = N values Qty = lo..hi }`
when those sizes are not business facts. Declare every named type you use.

## Structural pitfalls

- No sentinel IDs: use `Option<T>`. Equality compares whole Options; `is some(v)`
  binds a fresh payload only in its true continuation.
- Bounds are implicit checks; do not restate non-negativity as user safety.
- Action assignments read the old state simultaneously. A repeated write on
  the same path is an error; if arms are separate paths. Unproved forall
  distinctness reports `FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED`.
- Set/Seq/relation methods return values: reassign `s = s.add(x)`.
- Guard Seq head/pop/at and zero divisors with short-circuit conditions.
- Prefer `forall x in q { P(x) }` for element-wise properties. Use guarded
  indices from `CAP` only when position/order/no-duplicates carries meaning.
- No nested Maps: flatten product indices and recover axes using `/` and `%`.
- `old()` belongs only in ensures/trans. History needs a confirmed ghost;
  response belongs in `leadsTo`, not a current-state formula alone.
- Declare intended stops with `terminal`, not global deadlock suppression.
  Business derives sink terminals; new requirements does not.

## Reference index

- [syntax](references/syntax.md): kernel grammar, types, expressions, statements,
  annotations, compose/refinement and automatic checks.
- [layers](references/layers.md): connectivity, source-owned layer syntax, tags,
  readable documentation, governance and seam limits.
- [commands](references/commands.md): native commands, JSON, engines, replay,
  diff/chain, analysis, reports, approvals, and cross-dialect command routing.
- [errors](references/errors.md): result-to-repair protocol.
- [advanced](references/advanced.md): DB, Functional DDD/effects, AI/agents,
  external evidence and typestate.
- [impl](references/impl.md): Adapter, test generation, atomicity limits, NFR,
  clocks/deadlines, and review-only causal evidence.
- [nl-to-syntax](references/nl-to-syntax.md): confirmed sentences → constructs.
- [practices](references/practices.md): optional high-risk controls.
- [when-to-use](references/when-to-use.md): verification/documentation payoff.

Requirements syntax is owned by
[fsl-requirements](../fsl-requirements/references/syntax.md), including its
[keyword table](../fsl-requirements/references/keywords.md) and
[glossary](../fsl-requirements/references/glossary.md). Do not revive the
earlier process/transition requirements forms under this edition.
The document skill is retained from the base bundle, not duplicated here;
its sibling `fsl` references resolve to this replacement after overlay.
