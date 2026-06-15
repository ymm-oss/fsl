# DOGFOOD-9: Running the Validation Workflow (2026-06-12)

We ran from start to finish — on a new domain (order payment / cancellation / refund flow, with inventory) — the
workflow added to skills/fsl in issue #2 (validation roadmap for AI formalization): **formalization memo → NL→syntax
mapping → spec → positive-example pair → repair**. The artifact is `examples/validation/order_refund.fsl`.

The verifier (fslc) is unchanged since v1.0.3. What this round verifies is **the workflow, not code** — whether the
new discipline can catch "a spec that passes internal consistency but drifts from intent", before and after it is
written.

## Original Natural-Language Requirements (assumed PM input)

1. An order can be cancelled after payment, and cancellation refunds the full amount
2. It cannot be cancelled after shipping
3. Refunds only for paid orders. Double refunds are forbidden
4. On payment, reserve 1 from inventory; on cancellation, return it to inventory
5. Refunds only within a certain period from payment

## Formalization Memo (put in chat. not made into a file)

The moment we normalized the requirements into trigger / constraint / exception / **boundary implication**, two mines
became visible:

- **R2's boundary**: the "after" in "cannot cancel after shipping" **includes** Shipped.
  → `cancel requires order[o] == Paid` (excludes Shipped). Stated in ASSUME-2.
- **R5 is undefined**: the value, origin, and boundary (within = inclusive?) of "a certain period" are all absent
  from the original text. After filing it as a question for the human, we noted the suspicion "if this is a
  discrete-time SLA, it's a time+deadline matter in the requirements layer, not the design layer".

We held the assumptions as ASSUME-1 through 4 and decided to fold them into the spec (see below).

## Run Log

| Version | Operation | Result |
|---|---|---|
| v1 | naively modeled R5 with a `window_open: Map<OrderId,Bool>` flag | `check` ok |
| v1 | `verify --depth 8` | **reachable_failed**. `FullyRefunded` unreachable, `action_coverage.refund = false` |
| v1 | refund coverage diagnostic | hint: "these requires are unsatisfiable at any step up to depth 8. **Add an action that makes them hold.**" |
| → repair | removed the window flag; refund is just `requires order[o] == Cancelled`. R5 delegated to an upper layer as ASSUME-5 | |
| v2 | `verify --depth 8` | **verified**. coverage all true (refund too). `FullyRefunded` witnessed at step 3: `pay(0) → cancel(0) → refund(0)` |
| v2 | `verify --engine induction` | **proved (k=1)**, 0 CTI rounds. `_bounds_stock` is also inductive under `StockConserved` |

## Insights

- **F13: the positive-example pair (P4) made a "silently verified" visible (the focus of this round).**
  v1's safety invariants (StockConserved / RefundLedger) **both hold** — even with the refund path entirely dead,
  they cannot be broken if you look at safety alone. By having attached a single `reachable FullyRefunded`, `verify`
  returned reachable_failed instead of verified, and coverage named `refund`. With an invariant-only spec, this
  "the refund feature doesn't work" would have passed both CI and review. Separate from the decision to keep P4 a
  recommendation (don't mandate heavy procedures), we confirmed in practice that **attaching one is highly valuable
  for actions involving a boundary**.

- **F14: the formalization memo's "boundary implication" column flagged R5 as a mine before writing it.**
  R5's ambiguity (the period's value, origin, implication) came up as a human question at the memo stage. But
  naively bringing it into the design layer "with a window flag for now" forgot to write a way to open it, making
  refunds impossible. The 3 points **suspect in the memo, demonstrate with the positive-example pair, settle with
  ASSUME** meshed. The boundary "don't casually expand an ambiguous NFR into design-layer state" (DESIGN-nfr's SLA
  is the requirements layer) was reproduced on the workflow as well.

- **F15: the repair weakened the spec, but the ASSUME tag kept the "why".**
  The repair removed one of refund's guards (weakening). The distinction between hollowing and a legitimate repair
  is borne by ASSUME-5 ("the period check is left to an upper layer" + the history). The discipline of appending the
  repair log to the assumptions ledger (SKILL.md repair protocol) was exactly what worked for this weakening.

- **F16: writing a conservation-law invariant made the automatic bound inductive in one shot.**
  `_bounds_stock` (stock ≤ CAP) is non-inductive on its own (a ghost state with stock=CAP and a Paid order present
  could be a CTI), but the moment we wrote the domain truth `StockConserved` (stock + held count == CAP), it became
  proved at k=1. 0 CTI rounds. Re-confirms "auxiliary invariants are themselves domain truths" (DOGFOOD-2).

## Workflow Assessment

- **Before writing (the memo)**: put R2/R5's boundary implications in a form a human could confirm in plain language,
  before dropping them into logical formulas. The aim of not imposing logical-formula review on the human holds.
- **After writing (the positive-example pair)**: caught **over-constraint / a dead path**, not under-constraint.
  This is a kind of error that verify (safety) alone cannot see in principle, and it became the shortest example
  demonstrating P4's reason for existing.
- **Limit**: this round, a positive-example pair written by the formalizer themselves caught the formalizer's own
  judgment error ("bring R5 into the design layer"). If the positive-example pair had also been written under the
  same misunderstanding (that refunds are essentially unnecessary), it would not have been caught. An independent
  channel (a separate agent writing positive/negative traces from the NL = issue #3 forbidden / D4) is the next
  defensive layer.

## Connection to Remaining Work

- v1's "safety passes but the path dies" is the kind of thing issue #4 (vacuity checking) should emit as a warning
  at the verify stage (`always_true_requires` / unreachable). This round the positive-example pair substituted, but
  a detector is needed for specs where one forgets to write the pair.
- The existence of ASSUME tags is the remit of issue #5 (`--strict-tags`); their semantic binding force is the remit
  of issue #6 (`fslc mutate`).

## Addendum (2026-06-13): Mechanical Verification of ASSUME-5 — Running a Design Review

Using the fsl-design-review skill's procedure, we inspected this round's deferral decision itself. ASSUME-5's premise
was "the period restriction can be added later without breaking the frozen design contract":

| Check | Result |
|---|---|
| Windowed variant (`order_refund_windowed.fsl`: age map + tick + a time guard on refund) | **proved** standalone. FullyRefunded@3 (refund within the window is possible) + WindowExpired@4 (expiry actually occurs too) |
| Windowed variant ⊑ contract (`fslc refine`, tick → stutter) | **refines** — the period restriction goes in without editing a single line of the abstract contract. **ASSUME-5 is sound** |
| Negative probe "instant refund" (skip cancel, Paid → Refunded) | standalone **verified** (conservation law and ledger both intact), but refine is **abs_requires_failed**: the shortest 2-step `pay(0) → instant_refund(0)` bypasses "refund only from Cancelled" |

- **F17: a variant that "passes standalone verify but breaks the contract" is turned into a shortest counterexample
  by refinement.** As a counterpart to the main F13 (the positive-example pair broke the silence of reachability),
  refine breaks the silence of **design deviation**. The picture is complete in which, as validation tooling,
  the three layers verify / reachable / refine each handle a different kind of "silently verified".
- The naive formulation `type Age = 0..WINDOW` + `requires age[o] <= WINDOW` becomes a **tautological dead guard**
  because of the type bound (this variant adopts `< WINDOW`). We note this is the kind of error issue #4's
  `always_true_requires` mechanically detects.
- The artifacts are `examples/validation/order_refund_{windowed,instant}*.fsl`.
