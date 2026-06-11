# NFR Discrete-Time SLA Example

This directory contains the SLA worker spike in two forms:

- `sla_worker_kernel.fsl`: hand-written kernel specification with explicit `Age`,
  `age`, `tick`, urgency guards, and the SLA invariant.
- `sla_worker.fsl`: requirements-dialect fixture using `time`, `urgent`, `age`,
  and `deadline`; `fslc` expands the discrete-time bookkeeping from §3 of
  `docs/DESIGN-nfr.md`.

The modeled SLA is: once a request is submitted, it is finished within 4 discrete
ticks. `urgent start, finish` means `tick` is disabled whenever work can start or
finish, so the model includes the scheduling assumption needed to avoid starvation.
The dialect's generated `tick` only updates age counters by design; use the
kernel fixture when modeling additional tick-side progress such as service time.

Useful checks:

```bash
fslc verify examples/nfr/sla_worker.fsl --depth 10 --deadlock ignore
fslc verify examples/nfr/sla_worker.fsl --depth 10 --deadlock ignore --engine induction
```

Removing `urgent start, finish` leaves `tick` unconstrained by runnable work and
should produce a deadline violation with repeated `tick` steps.
