# ui_spike — fsl-ui (screen-transition dialect) spike assets

A spike for issue #9 (write screen flows in plain fsl, and confirm verification and
refinement to the requirements layer). Findings, an expansion-rule proposal, and
go/no-go are in [`../../docs/DESIGN-ui.md`](../../docs/DESIGN-ui.md).

| File | Contents |
|---|---|
| [`return_ui.fsl`](return_ui.fsl) | Screen flow for a return request (plain fsl). verified + proved. screen=enum / navigate=action / no dead end=leadsTo / double-submit prevention=invariant / all screens reachable=reachable |
| [`return_req_min.fsl`](return_req_min.fsl) | The essence of the requirements layer (payment only after approval, ledger consistent). The abs of refine |
| [`ui_refines_req.fsl`](ui_refines_req.fsl) | Mapping of UI flow → requirements. **refines** (UI-only step=stutter, commit=requirements action) |
| [`navstack.fsl`](navstack.fsl) | The back stack as `Map<Depth,Screen> + depth` (LIFO). Seq is FIFO and unsuitable |

```bash
fslc verify examples/ui_spike/return_ui.fsl --engine induction         # proved
fslc refine examples/ui_spike/return_ui.fsl examples/ui_spike/return_req_min.fsl \
            examples/ui_spike/ui_refines_req.fsl --depth 8             # refines
fslc verify examples/ui_spike/navstack.fsl --deadlock ignore           # verified
```

Conclusion of the spike: screen flows can be expressed without changing the
kernel's semantics, and they refine to the requirements layer. Turning it into a
dialect (`expand_ui`) looks feasible as AST sugar (see the go/no-go in
DESIGN-ui.md).
