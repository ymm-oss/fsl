# Sample for PMs / PdMs — cancellation flow (with a retention offer)

An example of **writing product requirements as "documents a machine can check."**
No code appears. The subject is a cancellation flow everyone knows: cancellation
request → present retention offer → accept (continue) or decline (cancel).

| File | What it writes | Reader |
|---|---|---|
| [`cancel_flow.fsl`](cancel_flow.fsl) | **Business flow + business rules** (process diagram, policies, KPI, goal) | PM / business side |
| [`cancel_system.fsl`](cancel_system.fsl) | **System requirements** (requirement IDs + verbatim text, screen transitions, acceptance criteria) | PdM / interface with development |

## Why this is valuable (3 points)

1. **Contradictions in the policies and requirements are found by machine before
   implementation.** Writing rules verbatim, such as "do not leave a request
   unattended" or "offer only once," lets the verifier confirm they hold for every
   operation ordering (the single command line below).
2. **A violation comes with "the ID and verbatim text of the broken requirement"
   plus "reproduction steps."** The material to discuss in a review meeting comes
   out as-is (the worked example below).
3. **Acceptance criteria become tests as-is.** The steps written in `acceptance`
   are replayed automatically at verification time and emitted as integration-test
   templates for the development team. The requirements document and the tests
   never drift apart.

## How to run

```bash
# Business layer: prove all rules (proved = holds under any operation ordering)
fslc verify examples/pm/cancel_flow.fsl --engine induction --deadlock ignore

# Requirements layer: requirement checking + consistency check against the business flow (implements) in one command, simultaneously
fslc verify examples/pm/cancel_system.fsl --deadlock ignore

# Emit the acceptance criteria and representative scenarios as test templates for development
fslc scenarios examples/pm/cancel_system.fsl --deadlock ignore
```

Currently all rules hold in both (the business layer is proved at unbounded depth).

## A worked example of "what it looks like when something is violated"

Suppose development adds a "complete cancellation directly from the cancellation
form" shortcut (skipping the offer), and we modify the requirements layer to match:

```json
{
  "result": "violated",
  "requirement": { "id": "REQ-4", "text": "an offer is not re-presented to a contract that already received one" },
  "last_action": { "name": "quick_churn" },
  "trace": [
    { "step": 0, "state": { "scr[0]": { "st": "Browsing",    "offered": false } } },
    { "step": 1, "state": { "scr[0]": { "st": "CancelForm",  "offered": false } },
      "action": "tap_cancel" },
    { "step": 2, "state": { "scr[0]": { "st": "GoodbyePage", "offered": false } },
      "action": "quick_churn" }
  ],
  "implements": { "abs": "CancelFlow", "result": "violated" }
}
```

How to read it: **which requirement broke (REQ-4 and its verbatim text)**, **what
broke it (quick_churn)**, **the shortest reproduction steps (3 steps)**, and on top
of that, **a deviation from the business flow (CancelFlow)** is detected at the
same time. In other words, "a cancellation that bypasses the offer violates both
the system requirements and the business policy" is shown in a single output.

## Key points for writing (also noted in the comments of the two files)

- Write business rules as `policy POL-1 "verbatim text" ...` and requirements as
  `requirement REQ-1 "verbatim text" { ... }`, **putting the ID and verbatim text
  as-is**. They are displayed verbatim on a violation.
- "Will eventually do ~" (do-not-leave-unattended kinds) is `responds`, "always
  is ~" is `invariant`, and "should be reachable ~" is `goal`.
- The world you verify can be small (3 contracts). Bugs reproduce even in a small
  world.
- To go further: you can also write an SLA ("present within K steps of the
  request") → `docs/DESIGN-nfr.md`; the full picture of the 3-layer structure →
  `examples/layers/`.
