---
name: fsl-from-code
description: Reverse-engineer an FSL design-layer spec from existing source code. Scope the stateful subsystem, harvest state/actions/guards/effects with source-line witnesses, surface invariants and forbidden flows as confirmation questions (never fabricated), then verify, mutation-test for hollowness, and prove conformance by replaying the generated harness against the real code. Use when the input is code and the deliverable is a spec. Do not use when starting from a requirements contract (use fsl-design), nor for business/PM requirements discovery.
---

# FSL From Code — extracting a design spec from an implementation

Use this skill when the input is **existing code** and the deliverable is an FSL
**design-layer** spec. The direction is the reverse of `fsl-design`: there is no
upper requirements contract to refine to, so the spec is anchored **downward** —
its faithfulness is proved by replaying the generated conformance harness against
the very code it was read from. If the user later wants an upper requirements
contract, that is `fsl-design`'s job, not this skill's.

Before writing syntax, read `../fsl/SKILL.md` and `../fsl/reference.md` for the
shared language rules, verifier workflow, and repair protocol. Inside this
repository, study the canonical triple **in reverse**: `examples/e2e/impl/expense.py`
(plain code) → `examples/e2e/3_design.fsl` (the spec) → `examples/e2e/impl/test_conformance.py`
(the generated Adapter + random-walk Monitor). That triple is exactly the artifact
this skill produces, read backwards.

## Boundary

Produce only:

- A design-layer kernel `spec` whose state/actions/guards/effects are read from the
  code, each tagged with a `// SRC: file:line` witness
- Invariants and forbidden flows that the human **confirmed**, tagged with
  `// ASSUME-n:` ledger comments
- The `fslc testgen` conformance harness, with the Adapter wired to the real code

Do not:

- Invent invariants, guards, states, or transition targets to make the spec look
  complete or to make it verify. A missing rule is a question, not a default.
- Produce a requirements or business layer, or refine upward.
- Claim implementation conformance unless the harness actually ran green against the
  code (a spec that only `verify`s is internally consistent, not faithful to code).

## Why this is not a transpiler — three zones

Extraction is not uniform. It splits into three zones at different confidence
levels, and conflating them is what produces a worthless spec:

- **Zone A — mechanical, high-recall, traceable.** `state` (member variables and
  their types, narrowed to bounded domain types), `action` (a method), `requires`
  (its leading guard clauses), effects (its assignments). Read directly off the
  code.
- **Zone B — NOT present in the code; the actual value, and the trap.** Cross-action
  invariants (conservation laws), forbidden interaction sequences, and `leadsTo`
  responses. `3_design.fsl`'s `CountsMatchSubmittedOrPaid` appears on no line of
  `expense.py` — it is a rule a human recognizes. These must be **discovered and
  confirmed**, never read off and never fabricated.
- **Zone C — mechanical truth-check.** `fslc mutate` (do the invariants have teeth?)
  and the conformance harness (does the modeled behavior match the code?).

A spec mirrored from code alone is **hollow**: it restates what the code does, so it
passes conformance trivially while asserting nothing a mutation can kill. The whole
job of this skill is to force Zone B and gate the hollow result.

## Workflow

```
0. Gate        Apply fsl's state-machine self-check. Reject CRUD / display / glue —
               FSL only pays off when interaction (order/flags/permissions/async/
               retry) can reach a forbidden state. Recommend ordinary tests instead.
1. Harvest     Read the code into a Zone-A skeleton: state vars + types, the methods
               that are actions, their guard clauses, their effects. Record a
               // SRC: file:line for each.
2. Memo        Post the formalization memo (below) in chat. Zone-B is QUESTIONS, not
               assertions; finite bounds are labeled modeling assumptions. Get human
               confirmation before writing.  <─────────────────────────────┐
3. Write       Write the .fsl after confirmation. Fold confirmed Zone-B rules in     │
               as invariant/forbidden/leadsTo; carry every // SRC: and // ASSUME-n:.  │
4. Verify      fslc check → verify --depth 8 → --engine induction. Repair via the     │
               fsl repair protocol (decide: code misread, or rule wrong?).            │
5. Anti-hollow fslc mutate (kill-rate) + reachable witnesses. A survivor or an        │
               `empty_formalization` requirement is a missing rule → back to Memo ────┘
               with a SPECIFIC question for that survivor (not a vague redo).
6. Conformance fslc testgen → wire the Adapter's observe()/step() to the real code →
               run the random-walk + replay. Drift means the modeled behavior left
               the code; fix the spec (or record a deliberate abstraction).
```

## Formalization memo (the crux — post in chat, do not make a file)

This is the only stage where insight emerges; stages 4–6 merely *test* what you
already claimed. Read the full memo discipline in `../fsl/SKILL.md`; this skill adds
the code-extraction question set. Ask these explicitly — a free-form "what invariants
exist?" reliably yields a Zone-A-only (hollow) spec.

**Zone-B discovery questions (these find the invariants):**

1. **Conservation laws.** For each state variable written by more than one action:
   is there a global aggregate (`sum` / `count`) that must always hold? (e.g. a
   counter equals the count of records in certain states.)
2. **Forbidden sequences.** Is there a state that each action's guard allows
   individually, but that a *sequence* of otherwise-valid actions must never reach?
   (a `forbidden` flow or a cross-action `invariant`, the kind a single `requires`
   stays silent about.)
3. **Relational / ordering constraints.** Regardless of which action fires, is there
   an ordering or relation between state values that is globally true?

**Scoping questions (get these wrong and the spec is valid but mis-modeled):**

4. **Action vs. helper boundary.** Which methods are entry points callable by
   external callers without depending on a prior call? Private-by-convention helpers,
   cron/queue-triggered steps, and cross-class internal calls all blur this. Only
   entry points become `action`s.
5. **Concurrency.** Does the code use locks / transactions / async? FSL models
   interleaving. Decide explicitly: are we modeling single-threaded correctness, or
   concurrent properties? A sequential model of concurrent code silently loses the
   races — often the whole reason to reach for FSL.
6. **Domain sizing.** Where the real system has no natural capacity limit (an
   unbounded list, arbitrary ints), bounding to `0..N` is **not** purely
   representational: it decides which interaction bugs are findable at depth 8.
   Record in `// ASSUME-n:` what bounding to N gives up — which bugs need more than N
   instances to appear.
7. **Multi-entity shape.** Is this one instance (→ a single `struct` in `state`), a
   collection (→ `Map<Id, Data>` over a bounded key type), or multiple actors (→
   `compose`)? Real code often does not align as cleanly as the `Map`-shaped example.

## Two-axis anti-hollow gate

Faithfulness has two orthogonal axes; they catch different failures, so run both.

| | `mutate` kills | `mutate` kills **nothing** |
|---|---|---|
| **conformance passes** | good | **hollow** — code faithfully mirrored, zero rules captured |
| conformance fails | invariants have teeth, but the model is a different machine | doubly broken |

The dangerous quadrant for code-extraction is *conformance-passes + kills-nothing*:
exactly where a naive transpile lands. The exit from it is `mutate --by-requirement`'s
`empty_formalization` warning driving a specific Zone-B question back into the memo.

When the implementation is **runnable**, prefer the stronger check: mutate the
**code** (remove an update, flip a branch) and confirm the conformance run goes red —
this proves the spec catches what the code gets wrong, which model mutation cannot.
Fall back to `fslc mutate` only when no runnable implementation exists (typical for
legacy reverse-engineering). A blind second agent writing invariants from the spec's
natural-language rendering is an option for high-stakes specs, too costly to mandate.

## Guardrails

- **Gate first.** Most code is not a state machine worth specifying. Recommending
  ordinary tests is a valid, common outcome — do not force a spec.
- **Confirm before encoding.** Every Zone-B rule and every non-representational bound
  is a question for the human first, then an `// ASSUME-n:` tag, never a silent
  default. Treat a low `mutate` kill-rate as evidence Zone B is unfinished, not as a
  reason to weaken the spec.
- **Trace everything.** `// SRC: file:line` on each action and each invariant keeps
  the spec auditable against the code it came from.
- **Conformance honesty.** Report "design invariants proved" and "conforms to code"
  as separate claims; make the second only after the harness ran green against the
  real implementation.
