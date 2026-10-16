Fixed (#1241, breaking): a `forbidden` last step whose argument is not of its
parameter's kind — an enum member or a Bool for an `entity`, `number`, or range
parameter (`respond(Waiting)` or `respond(true)` with `c: Case`,
`withdraw(Red)` with `amount: Amount`), or an integer, a Bool, or another
enum's member for an enum parameter — no longer satisfies the forbidden as
`bad_call`. It is a spec typo that no guard and no implementation boundary
decided, so it is now a `kind: "forbidden"` error (exit 2) with `failed_step`,
`step`, `step_results: []`, `message`, `wrong_kind_argument`
(`{parameter, value, type}`), a `loc` at that step, and a hint. A value of the
right kind outside a declared range stays `bad_call` (#1212), and one outside a
verify scope stays the #1229 error. One classifier serves `check`, `verify`
(every engine), `sweep`, `chain`, `scenarios`, `testgen`, `mutate`,
`counterexample export`, `html`, `ledger`, `approval`, `explain`, `fslc diff`,
and the Worker's `check` / `verify`; `fslc ledger` names the malformed
argument. Migration: such a spec used to pass `check`, and `scenarios` /
`testgen` emitted a test asserting `bad_call`; every command that runs the
forbidden gate now exits 2, except `explain` (exit 0, no witnesses; #1242),
`approval create --kind ledger` (exit 0, records the ledger listing the
error; #1243), and `approval check` of a ledger record created before
(`drifted`, exit 0); `approval check` of a `scenarios` or `html` record created
before exits 2. In `fslc diff` such an OLD final step is `unknown` /
`forbidden_replay_failed` where it was preserved, and an OLD `bad_call` whose
argument NEW's parameter type makes a wrong kind is `unknown` /
`forbidden_step_unrelatable`. Fix the step to a value of the parameter's type.
Corpus sweep: no corpus forbidden changes (`docs/design/DESIGN-forbidden.md`
§2.1 and §6).
