# 3-layer chain (consulting → requirements → design) — complete version with dialects

The final form of DESIGN-layers.md. The return domain is written in three dialects and chained by refinement.

| File | Layer / dialect | Result |
|---|---|---|
| `return_policy.fsl` | Consulting / `business` (process, policy, kpi, goal) | proved |
| `return_system.fsl` | Requirements / `requirements` (requirement, branches, acceptance, implements) | verified + **implements: refines** + proved |
| `return_impl.fsl` | Design / kernel fsl (two-phase payment + notification queue) | proved |
| `return_impl_refines.fsl` | design → requirements mapping | refines |

```bash
fslc verify examples/layers/return_policy.fsl --engine induction --deadlock ignore
fslc verify examples/layers/return_system.fsl --deadlock ignore       # implements is checked at the same time
fslc refine examples/layers/return_impl.fsl examples/layers/return_system.fsl \
            examples/layers/return_impl_refines.fsl --depth 5
fslc scenarios examples/layers/return_system.fsl --deadlock ignore    # acceptance_AC-1 appears
```

Highlights:

- **Requirement ID transparency**: when you break a requirements-layer guard, the
  counterexample JSON carries both
  `requirement: {id: REQ-3, text: payment only after approval, ledger consistent}`
  and `implements: {result: violated}` against the upper layer — a single
  diagnostic tells you which requirement broke and what it ripples into in the
  business layer.
- **branches**: a data-dependent correspondence such as "business-level approval
  or hold depending on amount" is declared with `when ... maps ...` and split
  automatically (coverage shows `submit[a <= AUTO_LIMIT]`). Downstream refinement
  references it by the internal name `submit__b1` (a current limitation).
- **acceptance**: the requirements-layer acceptance criteria are replay-verified
  with a concrete Monitor at check time, and flow through scenarios → testgen into
  the implementation's conformance test.
