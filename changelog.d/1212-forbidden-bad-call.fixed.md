Fixed (#1212): `forbidden`: a last step whose argument lies outside its
parameter's declared range or enum type is now reported as `rejected_by: "bad_call"`
(`docs/design/DESIGN-bridge.md` 1.2), and the generated negative test asserts
`bad_call`; only a call inside that domain that a `requires` guard refuses
stays `requires_failed`. On the spec measured in
`docs/design/DESIGN-forbidden.md` §2.1, no command that runs the forbidden gate
changes its exit status, but an `approval check` of a `scenarios` record
created before reports `drifted` (exit 0) for such a forbidden; `fslc diff`
changes its verdict (below).
An argument outside the `verify { instances / values }`
scope of an `entity` / `number` parameter evaluates no guard, and an
implementation may accept the call, so it is not a satisfied `bad_call` but an
error (#1229, listed in the #1229 `changed` entry). `fslc ledger` no
longer summarizes every forbidden error as an accepted final step with
"ガードを追加": a final step naming no action and a broken setup
(`forbidden_setup`) get their own summary, translation, and next action. The
`fslc diff` verdict changes of #1212 are breaking and listed in the #1212
`changed` entry.
