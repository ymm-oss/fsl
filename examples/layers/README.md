# 3-layer chain (consulting → requirements → design) — complete version with dialects

The final form of DESIGN-layers.md. The return domain is written in three dialects and chained by refinement.

| File | Layer / dialect | Result |
|---|---|---|
| `return_policy.fsl` | Consulting / `business` (process, policy, kpi, goal) | proved |
| `return_system.fsl` | Requirements / `requirements` (requirement, explicit actions, acceptance, implements) | verified + **implements: refines** + proved |
| `return_impl.fsl` | Design / kernel fsl (two-phase payment + notification queue) | proved |
| `return_impl_refines.fsl` | design → requirements mapping | refines |

```bash
fslc verify examples/layers/return_policy.fsl --engine induction
fslc verify examples/layers/return_system.fsl       # implements is checked at the same time
fslc refine examples/layers/return_impl.fsl examples/layers/return_system.fsl \
            examples/layers/return_impl_refines.fsl --depth 5
fslc scenarios examples/layers/return_system.fsl    # acceptance_AC-1 appears
```

Highlights:

- **Requirement ID transparency**: when you break a requirements-layer guard, the
  counterexample JSON carries both
  `requirement: {id: REQ-3, text: payment only after approval, ledger consistent}`
  and `implements: {result: violated}` against the upper layer — a single
  diagnostic tells you which requirement broke and what it ripples into in the
  business layer.
- **Amount-dependent correspondence**: the explicit fair actions `submit_small`
  (`a > 0` and `a <= AUTO_LIMIT`) and `submit_large` (`a > AUTO_LIMIT`) preserve
  auto-approval versus the manager queue. Their correspondences live in
  `implements`: the small action maps to `approve`, the large one to `stutter`.
  Downstream refinement uses these declared names, not generated branch names.
- **acceptance**: the requirements-layer acceptance criteria are replay-verified
  with a concrete Monitor at check time, and flow through scenarios → testgen into
  the implementation's conformance test.
