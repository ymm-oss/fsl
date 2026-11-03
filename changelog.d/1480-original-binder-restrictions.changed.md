Reject two previously check-admitted forms before engine dispatch: init statement
`forall` integer-range bounds with free state reads, and expression/action typed
Named Struct binder domains that engines already refuse as non-scalar. Diagnostics
retain
owning source locations. Supported init RHS reads, action/property conditional
ranges, Bool/Enum/Range binders and ordinary/collection Struct values are unchanged.

Checked-source range admission runs after existing init ownership/read checks,
preserving their earlier diagnostics and component provenance. Bare Core model
construction and raw Runtime constructors keep their separate contracts.
Already-refused init-statement Typed-Struct domains retain their complete
unlocated finite-scalar error; they are not new check-admission removals.

This is a breaking check-admission removal. The required named-base/head sweep,
actual form count and complete `ok`→`error` list remain pending; no empty removal
set is claimed. See `DESIGN-check-executable-forms.md` and `DESIGN-spec-domains.md`.
