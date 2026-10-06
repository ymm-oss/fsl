Changed (#1213, breaking): a `forbidden` case is satisfied only when its last
step is rejected, that is, not enabled (`requires_failed` or `bad_call`). A
last step that is enabled and then stops with a runtime violation
(`invariant` / `trans` / `ensures` / `type_bound` / `partial_op`) used to count
as satisfied with `rejected_by` set to the violation kind, so a forbidden passed
while the guard it states was missing. It is now a `kind: "forbidden"` error
(exit 2) with `failed_step`, `step_results: []`, `violation` (`{kind, name,
action, params}`), `accepted_trace` (the setup steps), the `state` before the
last step, a `loc` at that step, and a hint. `fslc diff` follows the same
rule: a NEW final step that is enabled and then violates is
`forbidden_relaxed` (its witness carries `violation`, and its `trace` and
`state` stop before the final step); an OLD final step that violates, or a NEW
setup step that violates, is `unknown` / `forbidden_replay_failed`. `fslc
ledger` summarizes the error as a violation reached by an enabled step, not as
an accepted one.

Migration, per command (exit status and output measured on small specs;
`docs/design/DESIGN-forbidden.md` §2.1): the forbidden replay is a gate that
runs before BMC, induction, and scenario generation. `check` used to exit 0
and now exits 2. `verify` used to report whatever its engine reached —
`violated` / exit 1 when `--depth` reached the violation, `verified` / exit 0
when it needed more steps, and its own verdict under `--engine induction` —
and now exits 2 at any depth and engine, with no BMC or induction trace; so do
`sweep`, `chain`, `mutate`, `html` and `ledger` (`html` and `ledger` still
write their report, with the verification `not_run`: `html` with the
forbidden hint, `ledger` with the violation under `FB-1`). `scenarios` and
`testgen` exit 2 with no scenario and no test file; `scenarios` used to exit
1 when its depth reached the violation and otherwise exit 0 with the
forbidden scenario as `rejected_by: "invariant"`, and `testgen` exited 1
(`violated`) on every measured spec, even at `--depth 1`, because its own
check reached the violation. `counterexample export` exits 2 with no file,
where it wrote a reproducer with exit 1 when its depth reached the violation
and otherwise exited 2 with nothing to export. `approval create` with `--kind
scenarios` or `html` exits 2 without a record (it used to create one when its
depth did not reach the violation), and `approval check` of such a record
exits 2. Three commands do not exit 2. `explain` still exits 0, but where
`scenarios` used to succeed its `witnesses`, and the witnesses section of
`html`, are now empty (a known gap: #1242). `approval create --kind ledger`
exits 0 with a record of a ledger that lists the error: exit 2 → 0 when its
depth reaches the violation, where it now creates a record (the earlier exit
2 was an accident: that ledger embeds wall-clock `elapsed_s`, so it never
matched a fresh rendering), and exit 0 → 0 otherwise. Whether a ledger whose
forbidden gate fails may be approved is tracked by #1243. `approval check` of
a ledger record created before reports `drifted` (exit 0). In `mutate`, a
mutant the bounded oracle leaves clean but whose forbidden last step now
violates is killed with `killed_by: "forbidden"` instead of surviving. In `fslc diff`, a NEW final violation now
fails `--forbid forbidden_relaxed`, and an OLD final or NEW setup violation
fails `--forbid unknown`, where that forbidden used to be preserved and failed
neither. Fix an affected spec by adding the `requires` that rejects the call
(the guard the forbidden states) and keeping the invariant as its own safety
property. There is no opt-out: a violation is evidence the guard is missing.
The strict error reports that violation, but the other properties' verdicts,
the shortest counterexample, and the reproducer come back only once the
forbidden or the guard is fixed. No `.fsl` under `specs/`, `examples/`,
`rust/fslc/tests/fixtures/` or `tests/fixtures/` changes its `check`,
`scenarios` or self-`diff` verdict (`docs/design/DESIGN-forbidden.md` §6).
