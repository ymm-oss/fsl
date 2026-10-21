Fixed (#1243): the commands that report a result about a spec's acceptance /
forbidden cases run the requirement-trace gate and fail closed on its
refusal, reporting the gate's error (the envelope `check` reports; a
`semantics` error when a trace cannot be replayed, e.g. under an `init` that
leaves a state variable unassigned) with exit 2: `fslc document generate` / `document claims` / `document check`
(which rendered, projected, or conformance-checked the refused cases as
claims, exit 0), `fslc ledger` (which rendered a ledger listing the error,
exit 2), `fslc approval create --kind requirements_document` and
`--kind ledger` (which recorded an approval, exit 0), and `fslc approval
check` of either kind (a `ledger` record reported `drifted`, exit 0). The
language server now publishes the refusal as an error diagnostic with code
`FSL-REQUIREMENT-TRACE` at the location `check` reports, where it published
nothing. A spec whose traces pass the gate, or that declares none, is
unaffected. Migration: fix the acceptance / forbidden case the error names
(the same error `fslc check` reports) before generating or approving a
document or ledger; an approval recorded for such a spec can no longer be
checked until the spec passes `check`.
