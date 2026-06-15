# DOGFOOD-8: Blind Writability Test (External Validation of G1 — Round 1)

## Goal
To measure this project's one unverified core proposition, **G1 "can anyone other than the author write FSL?"**.
Concretely: "relying on **the skill docs alone (SKILL.md + reference.md)**, can a separate agent with none of the
author's context turn natural-language requirements for **a new domain not in the existing examples** into a proved
spec, without syntax hand-holding?"

## Design (constraints for fairness)
- Subject: a separate agent with none of this session's context (general-purpose, same model family).
- References are the **2 documents only**: `skills/fsl/SKILL.md` and `skills/fsl/reference.md`.
  Reading `specs/`, `examples/`, `docs/`, and `src/` is forbidden (to exclude copying examples verbatim and to
  measure "whether the skill alone is a sufficient teacher").
- Subject matter: meeting-room booking (3 rooms × 4 slots × 3 people, no double booking, cancellation frees a slot,
  at most 2 per person, reaching full / holding 2). A new domain not in the existing specs.
- Hollowing out the invariant to make it green is forbidden. The process is logged.
- The result is independently re-verified by the observer (me) + audited with a semantic gallery (does it capture
  the requirements).

## Result: success
- The subject reached **proved** (induction k=1, no auxiliary invariant needed) with **3 fslc runs (check / verify /
  induction) and 0 fixes in the verification phase**.
- Independent re-verification (reproduced in a separate directory): check ok / verify verified (both coverage true,
  reachable witnessed by RoomFull@4 and SomeoneHoldsTwo@2) / induction proved.
- **Confirmed non-hollowing**: removing the guard of `AtMostTwoPerUser` gives violated@3 → it is proved with a
  substantive safety property.
- All 6 requirements were faithfully expressed. No double booking is prevented **structurally** by
  `Map<Cell, Option<UserId>>` (1 cell ≤ 1 holder = two holders are unrepresentable as a type) + reserve's
  empty-slot guard — stronger than an explicit invariant, and the subject honestly reported "there is no explicit
  line" as a reservation (the judgment not to add a hollow invariant was appropriate).

## Improvement Points Surfaced (the main product of this test)
- **F-A (a documentation gap in the skill)**: there is no standard recipe for 2-dimensional data. The naive
  `Map<RoomId, Map<SlotId, …>>` violates the state whitelist (nested Map not allowed). The subject figured this out
  on its own from reference.md §2 and worked around it by flattening (room, slot) into a single domain type `Cell`,
  but a one-liner in SKILL.md saying **"when you want to nest, flatten to a single key or use a struct value"** would
  reduce the snag. A cheap improvement.
- **F-B (an expressiveness gap in the language)**: there is no division or modulo (`+ - *` only). When you flatten
  Cell, you cannot "recover the room from Cell" (`c / SLOTS`), so we had to hard-code "room 0 is full" as the
  literal range `c <= 3`. A spot where the SKILL.md advice "don't hard-code boundaries" clashes with the feature
  set. Candidates: adding `/` and `%` (boundable, so expandable), or presenting a recipe for specs that require
  flattening.

## Limits (so as not to overstate)
- **n=1**: 1 domain, 1 trial. A positive signal, not a proof.
- The subject is an AI of the same model family, not a human PM. What was measured is "whether the skill alone can
  support the **AI authoring** that the README touts as the main path", which matches the real main use case. Human
  writability needs a separate subject.
- The domain is relatively straightforward (easy to cast as a state machine). A follow-up is needed on subject
  matter that requires more tangled requirements, larger boundaries (hitting PERF), or history/responsiveness
  properties.

## Next Moves (in priority order)
1. Add F-A to the skill (cheap, immediately effective).
2. A few more blind follow-up tests with varied domains/difficulty (increase n). Especially "history"-type matter
   that needs leadsTo/ghost variables, and time-based matter that needs an SLA.
3. F-B (division/modulo) is a language decision. After a second real need surfaces.
4. A blind test with a human PM as the subject (not executable by me; on the operations side).

---

# Round 2 (2026-06-12): Harder Follow-up Tests ×2 (to n=3)

With the skill updated to reflect F-A (2D recipe) and F-B (`/` `%`), we ran 2 new domains requiring harder
properties under the same conditions (the 2 skill docs only, reading existing examples forbidden).

## Results Summary

| Subject | Domain (required features) | Result | Fix rounds |
|---|---|---|---|
| ②a | incident ticketing (history ghost + leadsTo/fair) | **proved** (first draft, one try) | 0 |
| ②b | support first-response SLA (time/urgent/age/deadline) | **proved** | 2 (+4 self-initiated experiments) |

Both specs were independently reproduced by the observer and **audited as non-vacuous**: ②a goes violated when a
reopen action is added (with requirement tag REQ-4), and ②b goes violated when deadline is lowered to `<= 2` and
violated when urgent is removed (the boundary is exactly effective).

## Main Product ②b: Discovery of the "Vacuous SLA Trap" (found by the subject, independently confirmed)

Making an action that can always be enabled (the response itself) `urgent` causes **time to freeze, and even
`deadline <= 0` comes out verified** (vacuous). The subject detected this through a self-initiated vacuity
experiment (sweeping deadline over 0/2) and re-invented the **deadline-urgency pattern** (make only a guarded action
of the `respond_due` type, which becomes enabled only when the deadline is reached, urgent). On the observer's
check, this trap is explicitly stated nowhere in the existing docs or examples/nfr — it was the **biggest semantic
gap in the skill alone**. → We documented the trap and the pattern in reference.md / LANGUAGE.md and made the
subject's spec an official example as `examples/nfr/support_sla.fsl` (proved).

## Documentation Gaps Reflected (pointed out by both subjects)

- Placement rules for time/deadline (time directly under requirements, deadline inside a requirement), the
  semantics of age (+1 on tick, 0 when the while is false, readable from a guard), urgent = time freeze.
- leadsTo stays bounded even when proved → for an acyclic system, a `--depth` longer than the longest execution
  covers all executions, a guideline. `--depth K` includes step K.
- A constant-expression type upper bound (`0..ROOMS*SLOTS-1`) is valid (the observer confirmed) — unified the
  literal examples in reference into constant expressions to resolve the inconsistency between the 2 docs.
- Recorded as unaddressed: there is no means to express conditional fairness (only instances under a specific
  condition are fair) (②a; the current workaround is to split into a separate guarded action). Documenting the
  relationship between deadlock and leadsTo stagnation checking also has room for improvement.

## Observer-Side Learnings

In the ②a non-vacuity audit, the probe "removing fair should produce a starvation violated" missed (it stayed
verified) — in a monotone, acyclic system there is no lasso, so every maximal execution ends with everything
resolved, and thus **leadsTo holds structurally even without fairness**. A lesson that designing the audit probe
itself also requires understanding the domain structure.

## Overall Assessment After Update (n=3)

In all 3 domains (booking, history/response, SLA), a blind subject with the 2 skill docs only reached proved.
The snags were consistently **a lack of semantics documentation, not syntax**, and in all 3 the fslc diagnostics
(the expected list, the counterexample trace, the requirement linkage) supported self-recovery. Confidence in G1
strengthened, but it is still AI subjects only — human PM unverified.
