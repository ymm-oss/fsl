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
   (SKILL.md rules + reference.md), DOGFOOD-5.md (record of this spike).
