Changed (#1234), **breaking for stored `fslc analyze` node IDs of compose
specs**: a compose component action now has one public name, `alias.action`,
built from the `use` alias and the action name, so `use X as a__b` with action
`c` is `a__b.c` (it was `a.b__c`, and some outputs printed the internal
`a__b__c`), matching the frozen Python reference. Every command that names a
component action uses it: verify on every engine (traces, `action_coverage`,
`action_profile`, `cost.properties[].name`, vacuity warnings, induction CTIs),
sweep, scenarios, explain, testgen, conformance, mutate, refine, diff, html,
ledger, analyze, and `fslc replay`, including names built from the action such
as `_requires_failed_bank.settle`, `_partial_op_bank.settle`, and
`_partial_bank.settle`. Lists keyed by these names are ordered by the published
name, so `acct.go` sorts before `acct2.go`. A v1 replay trace can now write
`bank.settle`, the name testgen emits, so a generated test's actions replay
`conformant` instead of failing with `bad_call`. v1 replay still accepts the
older `bank__settle` for the same action, looked up in the model's table of
component actions rather than rewritten; sync actions and non-compose actions
such as `foo__bar` keep exact-name matching. Replay's `state_mismatch.action`
and `bad_call.action` report `bank.settle` where they used to echo
`bank__settle`. The breaking part: in `fslc analyze` output (the default graph,
`--profile ai-review`, and `--export tag-review`), the node IDs of a component
action and of its requires, effect, and ensures clauses move from
`action:bank__settle`, `guard:bank__settle:0`, `effect:bank__settle:0`, and
`ensures:bank__settle:0` to `action:bank.settle`, `guard:bank.settle:0`,
`effect:bank.settle:0`, and `ensures:bank.settle:0`, and edge IDs and endpoints
follow. To migrate, rewrite stored node IDs and `--focus` arguments from
`alias__action` to `alias.action`: `--focus action:bank.settle` already
resolved for an alias without `__` and keeps resolving, while
`--focus action:bank__settle` now fails with "unknown analyze focus node"
(exit 2). State and property node IDs are unchanged. `fslc document` claim
IDs and fingerprints are unchanged, because the document projection rejects
compose specs. For a compose spec, `fslc testgen` now lists the keys of each
scenario's expected state in state declaration order (`bank.cleared`,
`bank.pending`, `audit.balance`, `audit.log`, `withdrawn` for
`specs/bank_system.fsl`) in the vitest, swift, kotlin, dart, and phpunit
scaffolds, as pytest already did; they used to list the compose spec's own
state first and component state after it in name order. The values and the
partial-match check are unchanged, so a generated test passes or fails as
before. Non-compose specs print the same output as before.
