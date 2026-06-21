# Consultant-oriented sample — As-Is / To-Be control checking for business transformation

An example of **writing business processes and internal policies as "documents a
machine can check."** The subject is a reform proposal for an expense-reimbursement
workflow:

> The current process (As-Is) routes every case through manager approval and is
> backlogged.
> The reform proposal (To-Be) wants to speed things up by auto-approving a
> low-amount lane.
> **However, the controls (no payment without approval, etc.) must not be weakened
> in any way.**

| File | Contents |
|---|---|
| [`asis_expense.fsl`](asis_expense.fsl) | Current operations (interview results): process + control policies CTRL-1/2 + KPI |
| [`tobe_expense.fsl`](tobe_expense.fsl) | Reform proposal: adds an auto-approval lane, keeps the controls |
| [`tobe_refines_asis.fsl`](tobe_refines_asis.fsl) | **Control check of the reform**: To-Be's business mapping table (auto-approval = approval act) |
| [`governance_controls.fsl`](governance_controls.fsl) | Optional governance catalog: Finance owns CTRL-1/2, both business specs delegate to them, and the To-Be preservation refinement is checked |

## Why this is valuable (3 points)

1. **Contradictions in the interview results are found by machine, before the
   proposal is made.** Writing the policies verbatim lets you exhaustively check
   "is there any ordering in which a request gets stuck?" and "do any policies
   conflict with each other?" (both As-Is and To-Be have all policies proved at
   unbounded depth).
2. **You can write "the controls hold even after the reform" into the proposal,
   with evidence.** The single command line below checks that "every business
   flow allowed by To-Be corresponds to a flow that was also allowed under As-Is
   (i.e., it introduces no new loopholes)."
3. **The proposal flows straight downstream.** This business layer can be chained
   by refinement to requirements definition (`requirements`, see `examples/pm/`)
   → design → implementation tests, so the consulting deliverable does not become
   a "dead document."

## How to run

```bash
# Prove the policies of the current and reform proposals respectively
fslc verify examples/consulting/asis_expense.fsl --engine induction --deadlock ignore
fslc verify examples/consulting/tobe_expense.fsl --engine induction --deadlock ignore

# Control check of the reform (To-Be ⊒ As-Is)
fslc refine examples/consulting/tobe_expense.fsl \
            examples/consulting/asis_expense.fsl \
            examples/consulting/tobe_refines_asis.fsl --depth 6
# → {"result": "refines", ...} = the controls are preserved

# Optional governance catalog check: delegation + preservation in one place
fslc check examples/consulting/governance_controls.fsl
# → {"governance": {"delegates": ..., "preservations": [{"result": "refines"}]}}
```

## A worked example of "what it looks like when a control is broken"

Suppose an "immediate-payment lane that skips approval" (`quick_pay: Submitted -> Paid`)
slips into the reform proposal. Checking it gives:

```json
{
  "result": "refinement_failed",
  "kind": "abs_requires_failed",
  "impl_action": { "name": "quick_pay" },
  "violated_at_step": 2,
  "abs_before": { "claim_stage": { "0": "Submitted", ... }, "paid_claims": 0 },
  "impl_trace": [ "(init)", "submit", "quick_pay" ]
}
```

How to read it: **which business activity (quick_pay)** breaks **which control**
(As-Is payments presuppose prior approval = `abs_requires_failed`), and **the
shortest reproduction steps (submit → immediate-pay, 2 moves)**. The machine
points out "this reform proposal contains a deviation from the current controls"
before the review meeting.

## Key points for writing

- Arrows in the process diagram = `transition`, policies = `policy ID "verbatim text"`,
  business metrics = `kpi`, business goals = `goal`.
- Write As-Is and To-Be in separate files and connect them with a mapping table
  (`refinement`). **Make the interpretation itself explicit in the mapping table** —
  for example, "To-Be's auto-approval corresponds to As-Is's manager approval
  (it is still the control act of approving)." This is the judgment that gets
  questioned in an audit, and the machine guarantees consistency under that
  interpretation.
- The world you verify can be small (3 claims). Bugs reproduce even in a small world.
