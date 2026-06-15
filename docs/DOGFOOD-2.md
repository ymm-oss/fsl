# Dogfooding Round 2 — Findings (2026-06-11)

We put all of v1.1's features (Seq / k-induction / unsat core diagnostics / scenarios) into real use and
evaluated **"a workflow where proved is the standard"** (not stopping at BMC verified, but going all the way
to an unbounded-depth proof via CTI → auxiliary invariants). Three specs: `specs/mutex_queue.fsl`
(FIFO mutex), `specs/job_pipeline.fsl` (job pipeline with retries),
`specs/audit_log.fsl` (append-only audit log).

## Results Summary

| Spec | BMC (depth 8) | induction | CTI rounds |
|---|---|---|---|
| mutex_queue | verified, coverage all true | **proved (k=1)** | 0 (the first draft was already inductive) |
| job_pipeline | verified, coverage all true | **proved (k=1)** | 1 (added NoDupQueue) |
| audit_log | verified | **proved (k=1)** | 0 (even with the strict invariant) |

Including the round 1 specs, **all 10 correct specs in the repository are proved at k=1**.

## Evaluation of the proved Workflow

- **The job_pipeline CTI made its cause obvious on first read**: a ghost state `queue = [0, 0, 0]` (the same job
  entered three times). Since pop removes only the single front element, the state transition over the remaining
  duplicates breaks `QueuedAreQueued`. A single auxiliary invariant `NoDupQueue` (no duplicates in the queue)
  flipped it to proved. Together with round 1's auth_lockout / payment, **the CTI → auxiliary invariant loop
  converged in one round 3/3**. The display quality of the CTI (logical values, enum names, changes) directly
  drives this convergence speed.
- Every auxiliary invariant was "itself a domain truth" (no duplicates in the queue, refunds only from Captured,
  locked when attempts=3) and never became an artifact existing only for the proof. A nice side effect is that
  spec quality goes up.

## New Discoveries

### F5: a Seq aggregation idiom using an index domain type (a pleasant surprise)

At design time we assumed "you can't write aggregation (sum) over a Seq", but in practice:

```fsl
type Idx = 0..3   // a domain type covering up to capacity-1
invariant BalanceMatchesLog {
  balance == sum(i: Idx of log.at(i) where i < log.size())
}
```

The combination of `at()` being total in property contexts (out-of-range is don't-care) plus the `where` guard
lets you **fold over the live prefix**. audit_log's strict invariant (balance = log sum) can be written this way,
and it even came out proved at k=1. This should be documented as a standard idiom in the LANGUAGE doc.

### F6: scenarios' shortest trace correctly solves the chain of preconditions

`cover_finish_fail` generates `submit → start → finish_retry → start → finish_fail`.
finish_fail requires `tries >= 1`, and to get there it correctly assembles the 5-step shortest sequence that
passes through retry first. Practical quality as an integration test skeleton.

### F7: "a handoff happened" cannot be stated by state alone (re-confirming F1)

mutex_queue's `HandoffHappened` was written as `holder == some(1)`, but acquire_free(1) also satisfies it at step 1,
so it cannot pin down "the result of a handoff". Same root as round 1's F1 (properties about the past need a ghost
variable). Added as a motivating example for v2.0's `leadsTo`.

## Bugs

For these three specs + probes, **0 new bugs**.
(In review during the Seq implementation round, BUG15 (false detection of partial_op inside an if guard) and
two check pass-throughs (a capacity-overflow literal, `Map<K, Set<K>>`) were detected and fixed beforehand.
Details in DESIGN-seq.md and commit d8e2ecf.)

## Performance

For all three specs, BMC + induction at depth 8 finished within a few seconds. The post-PERF1-fix encoding is
stable even with Seq's shift ites added.
