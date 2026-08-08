Added (#727): `fslc mutate` now accepts `domain` documents. It renders the
domain document through the same textual kernel path `fslc domain expand`
uses (`fsl_tools::domain_kernel_source`) and mutates the re-parsed kernel
spec, so every mutant witness carries a real `loc` inside the rendered
kernel text instead of the effectively null span the direct-lowering path
produces; the rendered text is embedded in the output envelope as
`kernel_source`, matching `domain expand`/`domain check`. A saga whose
compensation actions are structurally dead at baseline now reports every
compensation-targeting mutant surviving with the existing "action dead at
baseline" note, giving domain specs the same kill-rate self-check evidence
channel other dialects already have. Unlowerable domain constructs are
still rejected by the shared lowering guard before any mutant runs; other
non-spec-like dialects (`governance`, `db`, …) are unaffected. Two call
sites in `run_mutate`'s dialect-lowering error path (already reachable
before this change, and now also reached by the new `domain` arm) stopped
collapsing a typed `CoreError` into an untyped `semantics` message and now
render through `core_error_output`, preserving `loc` and diagnostic
classification (the #780 defect class).
