# NFR Discrete-Time SLA Example

This directory contains the SLA worker spike plus a worked cross-layer
refinement.

- `sla_worker.fsl`: requirements-dialect fixture using `time`, `urgent`, `age`,
  and `deadline`; `fslc` expands the discrete-time bookkeeping from §3 of
  `docs/DESIGN-nfr.md`. Its generated `tick` updates age counters only.
- `sla_worker_kernel.fsl`: hand-written kernel spike with explicit `Age`, `age`,
  `tick`, urgency guards, and the SLA invariant — and a **finer clock**: its
  `tick` also consumes a `busy` service-time counter (service takes 2 ticks).
- `sla_worker_design.fsl` + `sla_worker_refines.fsl`: a design layer that
  **refines** `sla_worker.fsl`, plus the mapping.
- `support_sla.fsl`: a second requirements-dialect SLA fixture (the non-vacuous
  deadline-urgency pattern from DOGFOOD-8).

The modeled SLA is: once a request is submitted, it is finished within 4 discrete
ticks. `urgent start, finish` means `tick` is disabled whenever work can start or
finish, so the model includes the scheduling assumption needed to avoid starvation.

Useful checks:

```bash
fslc verify examples/nfr/sla_worker.fsl --depth 10 --deadlock ignore
fslc verify examples/nfr/sla_worker.fsl --depth 10 --deadlock ignore --engine induction
fslc refine examples/nfr/sla_worker_design.fsl examples/nfr/sla_worker.fsl \
            examples/nfr/sla_worker_refines.fsl --depth 6        # => refines
```

Removing `urgent start, finish` leaves `tick` unconstrained by runnable work and
should produce a deadline violation with repeated `tick` steps.

## Cross-layer SLA: the clock must be shared (issue #56)

`sla_worker_kernel.fsl` is **not** a second encoding of `sla_worker.fsl` — it is a
*different machine* with a finer clock. Because its `tick` advances `age` while
serving (consuming `busy`) but the requirements `tick` is urgency-disabled there,
it **cannot refine** `sla_worker.fsl`:

```bash
# this FAILS with abs_requires_failed on the service tick:
fslc refine examples/nfr/sla_worker_kernel.fsl examples/nfr/sla_worker.fsl <mapping> --depth 6
```

A discrete-time SLA is a safety property of the clock that declares it, and a
refinement preserves it only across a **shared clock** — the same reason
liveness does not propagate across refinement (`docs/DESIGN-layers.md` §6). Two
working patterns:

1. **Clock at the upper layer, design shares it** — `sla_worker_design.fsl`
   mirrors the generated requirements `tick` exactly, so `tick → tick` is valid
   and the deadline is preserved. The design may still add detail the
   requirements abstracts away (here an `audit` step that maps to `stutter`), as
   long as it introduces **no finer time steps**.
2. **Clock at the design layer, upper layer time-less** — put the SLA `time`/
   `tick` in the lower kernel `spec` and verify the deadline there, keeping the
   abstract contract time-agnostic and erasing `tick` with `tick → stutter`.
   See `examples/validation/order_refund_windowed.fsl`.

Use the kernel fixture (a finer clock) when you need to model tick-side progress
such as service time; verify its SLA at the design layer rather than trying to
refine a coarser timed requirements from it.
