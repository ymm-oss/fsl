Added (#727): `fslc mutate` now accepts `domain` documents. It renders the
domain document through the same textual kernel path `fslc domain expand`
uses (`fsl_tools::domain_kernel_source`) and mutates the re-parsed kernel
spec instead of the direct-lowering path `check`/`verify` use for domain.
Direct lowering does propagate real spans into the domain source file
(measured: 409 spans, 16 null, same on both paths), but resolves to only 23
distinct non-null source positions against 90 for the rendered path, so it
collapses many distinct mutants onto the same witness location; the
rendered path is chosen for that ~4x finer discrimination, at the cost that
its `loc` points into rendered kernel text rather than a domain source
line, which is why that text is embedded in the output envelope as
`kernel_source`, matching `domain expand`/`domain check`. A saga whose
compensation actions are structurally dead at baseline now reports every
compensation-targeting mutant surviving with the existing "action dead at
baseline" note, giving domain specs the same kill-rate self-check evidence
channel other dialects already have. Unlowerable domain constructs are
still rejected by the shared lowering guard before any mutant runs; other
non-spec-like dialects (`governance`, `db`, …) are unaffected. As
out-of-scope defensive hardening in the same dialect-lowering match this
change adds an arm to, one call site (`requirements_trace_contract`'s error
handler; unreachable for domain, whose only parse there always succeeds)
and a second in the pre-existing `Business|Requirements|Compose` branch
(never reached by `domain`, a separate match arm) stopped collapsing a
typed `CoreError` into an untyped `semantics` message and now render
through `core_error_output`, preserving `loc` and diagnostic
classification (the #780 defect class).
