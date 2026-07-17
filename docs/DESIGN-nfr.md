# FSL v3.1 — design for handling non-functional requirements (NFR)

Conclusion: **the majority of NFRs can be handled by the existing kernel, and time
(SLA/timeout) can be handled by adding discrete-time syntax. Probability, percentiles,
and real time are out of scope** (an honest boundary). The kernel semantics are not
changed (the time syntax is also a dialect expansion).

## 1. NFR category → FSL mapping table (overview of this design)

| NFR category | Handling | Mechanism |
|---|---|---|
| Security/authorization ("only admins can X") | **possible today** | role state + requires + invariant (idiomatized) |
| Audit/compliance ("all operations are recorded") | **possible today** | bank_system's audit pattern (cross-cutting invariant) |
| Capacity/limit ("queue up to N", "M concurrent") | **possible today** | bounded type / Seq capacity / count invariant |
| Reliability behavior (failover, degradation, recovery) | **possible today** | fault-injection action + mode state + recovery leadsTo (idiomatized) |
| Performance/SLA ("complete within K ticks") / timeout | **added in this design** | `time` block (discrete time) + `deadline` |
| Throughput rate / 99.9% / percentile / real time (ms) | **out of scope** | requires probabilistic/quantitative semantics (the domain of PRISM etc.). Document it |
| Usability / maintainability | out of scope | not a target for formalization |

## 2. Demonstration spike (2026-06-12, unmodified kernel)

"1 worker, processing 2 ticks, 2 requests, SLA: complete within 4 ticks of acceptance"
was constructed by hand in the kernel (tick action + age counter + urgency discipline):

- **BMC**: verified (the SLA holds within the depth)
- **Counterexample** (remove urgency = tick is always possible) → `violated` /
  the **starvation trace** of `submit → tick×5` + `requirement: NFR-1 (original text)`
- **Inductive proof**: 6 auxiliary invariants (3 structural: mutual exclusion,
  serving⇒pending, busy⇒serving / **3 time-budget**: `age[serving] + busy <= 4`, the
  waiter's budget, age=0 before service starts) were derived over 4 CTI rounds to reach
  **proved**

Findings: (a) an SLA can be checked as a safety property (an age-upper-bound invariant).
(b) **the urgency discipline is essential** — unless "time does not advance while an
urgent action is enabled" is woven into the tick guard, interleaving always produces a
starvation counterexample.
(c) BMC checking works immediately. The inductive proof requires a ladder of time-budget
invariants and is heavier (4 rounds) than an untimed spec (1-round convergence) — the
default is BMC, and the proof is positioned as opt-in.

## 3. Syntax (added to the `requirements` dialect)

```fsl
requirements OrderProcessingReq {
  ...types, state, init, requirement...

  time {
    urgent start, finish                       // forbid tick while enabled
    age waitAge[r: Req] while pending[r]       // +1 on tick, reset to 0 when condition is false
    age idleAge while queue.size() == 0        // scalar form is also allowed
  }

  requirement NFR-1 "an accepted request completes within 4 ticks" {
    deadline waitAge <= 4
  }
}
```

### 3.1 Expansion rules (all into existing kernel syntax)

The `time` block (at most one inside `requirements`):

1. `age m[x: T] while P` →
   - upper bound `cap = max(K of the deadlines referencing this age) + 1` (a type error
     "unused age" if no deadline references it)
   - a domain equivalent to `type _AgeM = 0..cap` + `state { m: Map<T, _AgeM> }` + init 0
     (the scalar form is `m: _AgeM`)
2. `urgent a, b, ...` → validate the enumerated (post-expansion) action names
   (written with the names before branches splitting: `urgent submit` applies to all
   branches after splitting).
3. Auto-generate the tick action:
   ```
   action tick() {
     requires not (exists <all parameter bindings of each urgent action> { their requires conjunction })
     forall x: T { if P { if m[x] < cap { m[x] = m[x] + 1 } } else { m[x] = 0 } }
     ...similarly for all ages...
   }
   ```
   When an urgent's requires contains an is-binding, it can be embedded directly inside
   the exists (a kernel expression). A type error if a user action named `tick` already
   exists.
4. `deadline m <= K` (inside a requirement) → a meta-tagged invariant
   `forall x: T { m[x] <= K }` (the scalar is `m <= K`).
   A deadline may only reference an age declared in the time block.

### 3.2 Semantics notes (to be stated explicitly in the documentation)

- A tick is one step like any other action. "within K ticks" = "at most K ticks while P
  holds continuously".
- Urgency is a **modeling premise** ("the system does not defer work when idle"). If
  urgent is not specified, most deadlines fail with a starvation counterexample — that
  is the check correctly pointing out that "there is no scheduling premise".
- The inverse pitfall is the vacuous-SLA trap: if the urgent-enabled condition is
  provably true in all reachable states, `tick` is never enabled, time freezes, and
  deadline invariants are hollow. `fslc verify --vacuity` reports
  `kind:"urgency_freeze"` only for the sound case where Z3 proves that condition
  initially and inductively.
- The counterexample trace of a deadline violation lines up ticks (the waiting time is
  visible).
- The inductive proof often needs time-budget auxiliary invariants (of the
  `age + remaining work <= K` form). Derive them from CTIs (place the worked example
  of §2 in examples).
- Relationship with deadlock checking: since tick has a requires, a state where "all
  urgent are disabled and time cannot advance" is detected as a deadlock (correct).

## 4. Idiomatizing NFRs the existing kernel suffices for (documentation only)

Add an "how to write NFRs" section to LANGUAGE.md / skills:

- **Authorization**: `requires role[u] == Admin`, invariant
  `forall x { sensitive_done[x] => done_by_admin[x] }` (ghost)
- **Audit completeness**: the bank_system pattern (`audit.balance == ... + withdrawn`)
- **Capacity**: type bounds + `requires q.size() < CAP` (make the exhaustion behavior
  explicit in an action too)
- **Reliability behavior**: fault injection such as `action crash() { mode = Degraded }` +
  `invariant DegradedRefusesWrites` + `fair action recover` +
  `leadsTo CrashRecovers { mode == Degraded ~> mode == Normal }`

## 5. Implementation plan

1. Add time/deadline/urgent to `expand_requirements` (§3.1).
2. Tests (tests/test_nfr.py): a fixture writing the §2 spike in the dialect, that is
   (a) BMC verified, (b) a variant with urgent removed is violated + starvation trace +
   requirement, (c) a version with auxiliary invariants added is proved, (d) type errors
   for unused age / unknown urgent / tick-name collision / duplicate time block,
   (e) all existing tests unchanged.
3. examples/nfr/: a hand-written kernel version (proved, with auxiliary invariants) and
   the dialect version side by side + README.
4. LANGUAGE.md (time/deadline in §13, a new "how to write NFRs" section), skills/fsl
   (SKILL.md rules + reference.md), with executable cases under `examples/nfr/`.

## 6. Discrete-time SLA across layers (issue #56)

A discrete-time SLA is a **safety property of the clock that declares it** (the
`deadline` invariant ranges over the `age` counter that the `time` block's `tick`
advances). Refinement is forward simulation — every *impl* `tick` must have an
*abstract* image — so a refinement carries the SLA **only across a shared clock**.
This is the same non-propagation result as liveness (`DESIGN-layers.md` §6): a
lower layer is free to introduce internal steps the upper layer does not model,
and time steps are no exception. It is a property of forward simulation, not an
`fslc` defect.

Concretely, `examples/nfr/sla_worker_kernel.fsl` is a **finer clock** than
`examples/nfr/sla_worker.fsl`: its `tick` also consumes a `busy` service-time
counter, so it ticks *while serving*, where the requirements `tick` is
urgency-disabled (`urgent finish`). Mapping that design onto the requirements
spec fails with `abs_requires_failed` on the service tick — the finer tick has no
coarse image. (`sla_worker_kernel.fsl` is therefore a *different machine*, not a
second encoding of `sla_worker.fsl`.)

**Two idioms that do work** (and are exercised in the corpus):

1. **Clock at the upper layer; the design shares it.** A kernel design that
   mirrors the generated `tick` exactly refines the timed requirements; it may add
   detail the requirements abstracts away as long as it introduces no finer time
   steps. Worked example: `examples/nfr/sla_worker_design.fsl` +
   `sla_worker_refines.fsl` → `refines`.
2. **Clock at the design layer; the upper layer is time-less.** Put the SLA
   `time`/`tick` in the lower kernel `spec`, verify the deadline there, and erase
   `tick` with `tick → stutter` against a time-agnostic abstract contract. Worked
   example: `examples/validation/order_refund_windowed.fsl`.

**Options considered and deferred.** Extending the language was evaluated and not
adopted, because none removes the underlying clock-granularity mismatch without
disproportionate cost or a kernel-semantics change the architecture forbids
(`DESIGN-layers.md` §8, "do not add new semantics to the kernel"):

- *Open `time` to the kernel/design layer* — relocates the dialect sugar but the
  generated `tick` is still age-only, so it does not let a design model finer time;
  the mismatch remains.
- *Tick-side effects in the `time` block (`on tick { … }`)* — would let a
  requirements clock match a finer design clock, but it forces both layers to
  duplicate the same timed machine (the refinement becomes near-trivial) and
  pushes a service-time *how* into the requirements *what*. A possible future
  consolidation (it could subsume the kernel fixture), not a present need.
- *A cross-granularity refinement bridge (N impl ticks ⊒ 1 abstract tick)* — the
  general fix, but it changes the refinement core where the dual-evaluator /
  oracle soundness invariant lives, for a narrow benefit.

The guidance above (verify a timed property at the clock-owning layer; share the
clock to carry it across a refinement) is the supported approach.

## 7. Runtime evidence for bounded responses (issue #225)

There are two intentionally different runtime-visible deadline forms:

- requirements `deadline age <= K` lowers to an invariant and is checked as
  safety by Monitor/replay;
- `leadsTo P ~> within K Q` is a bounded response and trace schema 1.2 checks it
  with the solver-free bounded-liveness monitor.

Both count logical observations, not timestamps. For bounded `leadsTo`, the
trigger observation is `p`, the inclusive deadline is `p + K`, and both action
and stutter events advance time. Finite prefixes may end with pending
obligations; unbounded response claims still require verifier evidence. See
`examples/nfr/bounded_response.fsl` and its positive/overdue replay traces.
