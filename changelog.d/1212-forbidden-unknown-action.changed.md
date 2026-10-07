Changed (#1212, breaking): a `forbidden` last step that names no action, or no
variant of that arity, is now a `kind: "forbidden"` error with a `message`
(`unknown action '<name>' in forbidden` / `arity mismatch for action '<name>'
in forbidden`), located at that step, instead of a vacuously satisfied
rejection (frozen-Python parity). Migration, per command: `fslc check` used
to pass and now exits 2. `verify` (every engine and depth), `sweep`, the
`[requirements]` layer of `chain`, the `mutate` baseline, `html` and `ledger`
used to report whatever their engine reached on the rest of the spec (exit 0
when it verified, exit 1 when it did not) and now exit 2 with no verdict,
counterexample or mutants: `verify`, `sweep` and `mutate` print this error,
`chain` carries it as the `[requirements]` layer's `detail`, and `html` and
`ledger` print the generated result (`result: "generated"`) and still write
their report, with the verification `not_run` and this error inside. `scenarios`
and `testgen` now exit 2 with no scenario and no test file; for an arity
mismatch they used to emit the forbidden scenario as `rejected_by:
"requires_failed"` and a test asserting it, and for an unknown action they
stopped with exit 2 (`kind: "semantics"`, `unknown forbidden action
'<name>'`), in both cases only when the rest of the spec verified (otherwise
`violated`, exit 1). `counterexample export` returns this error and writes no
file, where it exported the rest of the spec's counterexample, or exited 2
with nothing to export. `explain` still exits 0, with empty `witnesses`, and
the witnesses section of `html` is empty too (a known gap: #1242).
`approval create` with `--kind scenarios` or `html` exits 2 without a record
(for `html`, stdout is the generated `html` result),
and `approval check` of such a record created before exits 2. `approval
create --kind ledger` now exits 0 and creates a record of a ledger that lists
the error: exit 0 → 0 on a spec whose rest verifies, and exit 2 → 0 on a spec
whose rest has a violation, where it now creates a record. The earlier exit 2
was an accident: that spec's ledger embeds wall-clock `elapsed_s`, so it never
matched a fresh rendering. Whether a ledger whose forbidden gate fails may be
approved is tracked by #1243. `approval check` of a ledger record created
before reports `drifted` (exit 0). Fix the step's action name or arguments. `fslc diff`
changes two forbidden verdicts: a last step that OLD and NEW both reject as
`bad_call` outside a range or enum parameter type is now preserved instead
of `unknown` / `forbidden_step_unrelatable`, so that forbidden no longer
fails `--forbid unknown`; an OLD last step that names no action, or no variant
of that arity, is now `unknown` / `forbidden_replay_failed` instead of
preserved by a NEW guard (in the measured cases `--forbid unknown` already
failed, because the action NEW adds, or the arity it changes, is itself an
`unknown` finding). A `bad_call` decided by an `entity` / `number` verify
scope stays `unknown`. `diff --git` and `approval diff` report the same
findings. Each command's measured exit status and output before and after,
and the corpus sweep (no `.fsl` under `specs/`, `examples/`,
`rust/fslc/tests/fixtures/` or `tests/fixtures/` changes its `check`,
`scenarios` or self-`diff` verdict), are in `docs/design/DESIGN-forbidden.md`
§2.1 and §6.
