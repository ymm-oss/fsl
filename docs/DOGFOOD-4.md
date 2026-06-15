# Dogfooding Round 4 — Penetrating the 3-Dialect Stack (2026-06-11)

After implementing the kernel + 3 dialects (DESIGN-layers.md / DESIGN-dialects.md), we built up a returns domain
across four files — **business → requirements → design fsl → mapping** — and verified every stage
(`examples/layers/`).

## Results

| Stage | Result |
|---|---|
| business (3 process transitions + KPI + 2 policies + goal) | proved (the automatic `_kpi_refunded` consistency invariant enters the inductive hypothesis) |
| requirements (branches, acceptance, implements) | verified + implements: **refines** (upper-layer check included in a single verify command) + proved |
| design layer (two-stage payment + notification queue) | proved + **refines** to the requirements layer |
| variant that breaks a requirement | the counterexample carries `requirement: {REQ-3, original text}` and `implements: violated` **simultaneously** |
| acceptance AC-1 | verified by a Monitor replay at check time, and flows into scenarios |

## Discoveries

- **BUG18 (fixed)**: an identifier with a keyword prefix (`notify` → `not` + `ify`) was tokenized incorrectly.
  Found during the layer spike, fixed in stage 2.
- **F11: the downstream reference to a branches-split action is the internal name.** The design-layer mapping has to
  reference the requirements-layer split action as `submit__b1` (it cannot be written with the display name
  `submit[a <= AUTO_LIMIT]`). It works but is ugly as UX — reference by display name / original name + when-condition
  is filed as future work.
- **F12: cross-layer diagnostics line up in a single JSON.** The requirement (with original text) and implements
  (propagation to the upper layer) ride on the same counterexample — an agent can read "which requirement broke,
  and what business-level thing it violates" in one round trip. The design's aim (transparent composition) holds up
  on the diagnostics side.
- The dialect expander is stable even as compose's third example (BMC/induction/scenarios/Monitor/refine all worked
  on the dialect specs unmodified). The great principle of the unchanged kernel was upheld across all four stages.
