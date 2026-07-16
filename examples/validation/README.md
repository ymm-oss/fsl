# validation — a worked run of the validation workflow

A worked deliverable showing how to catch "a spec that passes internal consistency
(verify) but drifts from the original intent" through discipline both before and
after writing. The reusable workflow is defined in
[`../../skills/fsl/SKILL.md`](../../skills/fsl/SKILL.md).

| File | Contents |
|---|---|
| [`order_refund.fsl`](order_refund.fsl) | Design-layer spec of an order payment/cancel/refund flow (with stock) = **the frozen contract**. proved |
| [`order_refund_windowed.fsl`](order_refund_windowed.fsl) | A design variant: **with a refund-period window** (a design-layer implementation proposal for R5, deferred in ASSUME-5). proved |
| [`order_refund_windowed_refines.fsl`](order_refund_windowed_refines.fsl) | The mapping for the above (tick is a stutter). **refines** — a time limit can be added without breaking the contract (OCP/LSP) |
| [`order_refund_instant.fsl`](order_refund_instant.fsl) | **A negative probe**: an "instant refund" that skips cancel. verified on its own |
| [`order_refund_instant_refines.fsl`](order_refund_instant_refines.fsl) | The mapping for the above. **refinement_failed / abs_requires_failed** — a contract bypass appears in the 2 moves `pay → instant_refund` |

## What this sample demonstrates

- **A formalization note** surfaces the "boundary implications" of a requirement
  before writing it (e.g., "no cancel *after* shipping" = includes Shipped). The
  note goes in chat, not into a file.
- **Assumptions are folded into the `.fsl`** with ASSUME tags/comments (not into a
  separate note file).
- **A positive-example pair (`reachable FullyRefunded`) makes "silently verified"
  visible**: the first version that naively brought the refund period into the
  design layer had all safety invariants holding, yet the entire refund path was
  dead. The positive-example pair detected it with `reachable_failed`, and coverage
  named `refund` (invariants alone would have passed it silently).
- **Design review can be translated into a contract-conformance check** (a worked
  run of the fsl-design-review skill): the windowed variant refines without editing
  a single line of the abstract contract (machine verification of the deferral
  decision ASSUME-5). Conversely, the "instant refund" **breaks nothing under
  verify alone**, yet refine shows a contract bypass in the shortest 2 moves — a
  worked example where refinement catches a design deviation invisible to verify.

```bash
fslc verify examples/validation/order_refund.fsl --engine induction            # proved (contract)
fslc verify examples/validation/order_refund_windowed.fsl --engine induction   # proved (variant)
fslc refine examples/validation/order_refund_windowed.fsl \
            examples/validation/order_refund.fsl \
            examples/validation/order_refund_windowed_refines.fsl --depth 8    # refines
fslc refine examples/validation/order_refund_instant.fsl \
            examples/validation/order_refund.fsl \
            examples/validation/order_refund_instant_refines.fsl --depth 8     # refinement_failed
```
