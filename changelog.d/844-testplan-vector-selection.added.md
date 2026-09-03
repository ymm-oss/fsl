Added (#844): `fslc testplan SPEC [--depth N]` selects the bounded
`conformance` vectors — the accepting ones and the `requires_failed` ones no
test-generation path consumed — into a closed `test-plan.v1` JSON document
(`schemas/fslc/kernel/test-plan.v1.schema.json`). Kernel and conformance JSON
are built from one checked model, so a plan cannot pair two snapshots. A plan
is a selection, never a verdict: it carries `formal_result: "not_run"`,
`assurance_effect: "none"`, and a `do_not_assume` list, and it records the
requirement to pass a spec at the implementation's layer granularity.
