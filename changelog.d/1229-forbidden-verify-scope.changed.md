Changed (#1229, breaking): a `forbidden` last step whose argument lies outside
the `verify { instances / values }` scope of an `entity` / `number` parameter
(a requirements or business `process` counts as an `entity`) no longer
satisfies the forbidden. It evaluated no guard, and an implementation, which
has no such bound, may accept the call (for example `respond(7)` under
`instances Case = 3`, or `add(9)` under `values Qty = 0..3` when the guard
admits 9). It used to count as satisfied, with `rejected_by:
"requires_failed"`, so the generated test asserted a rejection no guard made. It is now a `kind: "forbidden"` error
(exit 2) with `failed_step`, `step`, `step_results: []`, `message`,
`out_of_scope_argument` (`{parameter, value, type, scope}`), a `loc` at that
step, and a hint. An argument outside a declared range or enum type stays a
satisfied `bad_call` (#1212), and a guard refusal inside the scope stays
`requires_failed`. Under `--instances` / `--values`, a forbidden whose
final-step argument only the override removed stays `forbidden_skipped` /
`not_evaluated` (#1218); one outside the declared scope is this error unless
the override widens the scope to include it, in which case the guard is
evaluated. The error's `out_of_scope_argument.scope` and the `lo..hi` in its
hint are the scope of the run, so under a narrowing override they show the
overridden range (`scope: [0, 1]` for `respond(7)` with `--instances Case=2`),
not the declared range that makes the step an error, while the hint's "widen
the scope in verify { instances / values }" refers to the declared block.
`fslc ledger` summarizes the error as a step outside the verify
scope.

Migration, per command (exit status and output measured on small specs;
`docs/design/DESIGN-forbidden.md` §2.1): the forbidden replay is a gate that
runs before BMC, induction, and scenario generation, and such a step used to
pass it. `check` used to exit 0 and now exits 2. `verify` (every engine and
depth), `sweep`, the `[requirements]` layer of `chain`, the `mutate` baseline,
`html` and `ledger` used to report whatever their engine reached on the rest
of the spec (exit 0 when it verified, exit 1 when it did not) and now exit 2
with no verdict, counterexample or mutants. `verify`, `sweep`, and `mutate`
print the error (`error` / `kind: "forbidden"`); `chain` prints `kind:
"chain"` with `failed: ["requirements"]` and the error as that layer's
`detail`; `html` and `ledger` print the generated result (`result:
"generated"`) and still write their report, with the error inside and the
verification `not_run`.
`scenarios` and `testgen` now exit 2 with no scenario and no test file; when
the rest of the spec verified they used to exit 0, emitting the forbidden
scenario as `rejected_by: "requires_failed"` and a test asserting it (otherwise
`violated`, exit 1). `counterexample export` returns this error and writes no
file, where it exported the rest of the spec's counterexample, or exited 2
with nothing to export. `explain` still exits 0, with empty `witnesses` (where the rest of the spec
verified, it listed the forbidden's witness), and the witnesses section of
`html` is empty too (known gap #1242).
`approval create` with `--kind scenarios` or `html` exits 2 without a record (it used to create one
when the rest of the spec verified); with `--kind scenarios` it prints the
error, with `--kind html` the `html` result, and `approval check` of such a record
created before exits 2. `--kind ledger` exits 0 with a record of a ledger
that lists the error (where the rest of the spec verified, it created one
before too; where it was violated, it exited 2 only because that ledger embeds
wall-clock `elapsed_s`; whether such a ledger may be approved is tracked by
#1243), and a ledger record created before reports `drifted` (exit 0).
In `mutate`, a mutant that narrows an `entity` / `number` scope so that a
forbidden final step falls outside it (`type_bound_hi_minus1` /
`type_bound_lo_plus1` on that type) is killed with `killed_by: "forbidden"`;
on the measured specs it survived before. `fslc diff` classifies each side with its own `entity` /
`number` types: an OLD final step outside the compared verify scope is
`unknown` / `forbidden_replay_failed` instead of `unknown` /
`forbidden_step_unrelatable` (no `--forbid` exit changes, as it was already
`unknown`), and an OLD `bad_call` is preserved only when NEW also rejects it
as `bad_call` outside a declared type. A compose NEW, or a NEW of another
dialect whose entity types are not in its source, never preserves an OLD
`bad_call`, even when it rejects the step as `bad_call`, since its component
may bound that type by a verify scope: such a forbidden is `unknown` /
`forbidden_step_unrelatable` and fails `--forbid unknown`. `diff --git` and `approval diff` report the same findings.
Fix an affected spec by widening the scope so it includes the value, or by
changing the step to an in-scope value the guard rejects (the error's
`out_of_scope_argument` and hint name the parameter, value, and scope). If the
call must be rejected as a nonexistent value, state that bound as a range type
instead, which keeps it a `bad_call`. There is no opt-out. No `.fsl` under
`specs/`, `examples/`, `rust/fslc/tests/fixtures/` or `tests/fixtures/`
changes its `check`, `scenarios` or self-`diff` verdict
(`docs/design/DESIGN-forbidden.md` §6).
