# Dogfooding Round 5 — Non-Functional Requirements (Discrete-Time SLA) (2026-06-12)

Verification record for the implementation answering "can FSL handle non-functional requirements?" (DESIGN-nfr.md).

## Results

| Item | Result |
|---|---|
| Hand-written kernel version (examples/nfr/sla_worker_kernel.fsl) | BMC verified + **induction proved** (6 auxiliary invariants, 4 CTI rounds) |
| Dialect version (examples/nfr/sla_worker.fsl, `time` + `deadline`) | BMC verified (the automatic tick appears in coverage) |
| Variant with urgent removed | **violated** — a starvation trace `submit → tick×5` + `requirement: NFR-1 (original text)` |
| Static checks | unused age / unknown urgent / tick name collision / duplicate time / undeclared deadline → type error |

## Insights

- **An SLA can be checked as a safety property**: "within K ticks" = an upper-bound invariant on the age counter.
  This lets you write a stronger "with a deadline" property than leadsTo (eventually).
- **Urgency discipline is essential**: "while an urgent action is enabled, time does not advance" is woven into
  tick's guard. A spec that forgets this gets a starvation trace back from the verifier — a correct, mechanical
  detection that "the scheduling assumption is not written", and it becomes a finding as-is in an NFR review.
- **Proof cost is higher than for untimed specs**: a ladder of time-budget invariants
  (`age[serving] + busy <= 4`, the waiters' budget, age=0 before service starts) is needed, with 4 CTI rounds
  (the prior track record was 1 round). The default workflow is the BMC check; proof being opt-in is the correct
  positioning.
- The boundary of which NFRs are handled (DESIGN-nfr §1) is unchanged after implementation:
  authorization, audit, capacity, reliability behavior (from today) / SLA, timeout (this feature) /
  probability, percentiles, real-time ms (out of scope — to the docs).
