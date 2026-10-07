Changed (#1234), **breaking for stored `fslc analyze` node IDs of compose
specs**: a compose component action now has one public name, `alias.action`,
built from the `use` alias and the action name, so `use X as a__b` with action
`c` is `a__b.c` (it was `a.b__c`, and some outputs printed the internal
`a__b__c`), matching the frozen Python reference. Every command that names a
component action uses it: verify on every engine (traces, `action_coverage`,
`action_profile`, `cost.properties[].name`, vacuity warnings, induction CTIs,
and the BMC `semantics` error `action 'bank.settle' body evaluation has a
non-partial failure`, likewise for `guard` and `ensures`; it printed `bank__settle`),
sweep, scenarios, explain, testgen, conformance, mutate, refine, diff, html,
ledger, analyze, the `untagged` warnings of `check --strict-tags`, and
`fslc replay`, including names built from the action such
as `_requires_failed_bank.settle`, `_partial_op_bank.settle`, and
`_partial_bank.settle`. Lists keyed by these names (`reachables`,
`action_coverage`, `action_profile`, induction `k_used`, `cost.properties`,
scenarios `reach_*` and `respond_*`, the leadsTo warnings of `fslc scenarios`,
refine `action_map` and `progress`, and the ai-review
conservation findings) are ordered by the name they print, so `acct.go` sorts
before `acct2.go`, and a glue action `z__a`, printed `z.a`, sorts before the
component action `z.b`. `fslc refine` success output now prints `action_map`
keys and values as `a__b.go` (the keys were `a.b__go`) and `progress.*.actions`
as `acct.back` (it was the internal `acct__back`). A v1 replay trace can now write
`bank.settle`, the name testgen emits, so a generated test's actions replay
`conformant` instead of failing with `bad_call`. v1 replay still accepts the
older `bank__settle` for the same action, looked up in the model's table of
component actions rather than rewritten; sync actions and non-compose actions
such as `foo__bar` keep exact-name matching. `fslc replay --from-log` with a
`maps auto` mapping matches a log action written `a__b.c` as well as the
`a.b__c` it matched before. Replay's `state_mismatch.action`
and `bad_call.action` report `bank.settle` where they used to echo
`bank__settle`. The breaking part: in `fslc analyze` output (the default graph,
`--profile ai-review`, and `--export tag-review`), the node IDs of a component
action and of its requires, effect, and ensures clauses move from
`action:bank__settle`, `guard:bank__settle:0`, `effect:bank__settle:0`, and
`ensures:bank__settle:0` to `action:bank.settle`, `guard:bank.settle:0`,
`effect:bank.settle:0`, and `ensures:bank.settle:0`, and edge IDs and endpoints
follow. The `name` field of those nodes moves the same way (`bank__settle` to
`bank.settle`, `bank__settle:0` to `bank.settle:0`), and with alias `a__b` their
`label` spells the action `a__b.c` instead of `a.b__c` (`a__b.c requires 0`).
`--export tag-review` prints a component
action's `name` as `bank.settle`, and an `undecided:` record of a component
action, shown by `html`, `ledger`, and `--profile ai-review`, has
`declaration` `action bank.settle` and `node` `action:bank.settle`. In the
project `traceability_graph` projection of `fslc analyze <manifest>`, a
refinement's `action_map:` and `stutter_map:` node IDs and labels and its
`maps_action` endpoints use the same name (`design:action:bank.settle`), so
every edge ends at a node the graph declares; before, a compose layer left
edges ending at undeclared `action:bank__settle` nodes, dropped the
`lower_anchor` edges from requirements covering an abstract component action,
and reported a spurious `traceability_gap`. All of these IDs now differ from
the frozen Python reference, which keeps `action:bank__settle`; this is
intended. To migrate, rewrite stored node IDs and `--focus` arguments from
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
before. Non-compose specs change in two corner cases. A keyed list now
orders a name containing `__` by the `.` form it prints, so `step__a`
(printed `step.a`) sorts before `step2`; it sorted after. `fslc testgen` now
accepts a state whose name has two `__` separators, such as `a__b__c`
(printed `a.b__c`), and generates every target; it failed with "testgen
concrete vector.initial has unknown state fields: a.b__c" (exit 2). Every other non-compose output is unchanged.
