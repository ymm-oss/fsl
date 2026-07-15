# Production Log Replay through Refinement Mappings

Status: implemented for issue #174.

## 1. Goal and boundary

`fslc replay SPEC --from-log events.jsonl --mapping log_mapping.fsl` checks
production JSONL records with the Z3-free `Monitor`. The mapping file is parsed
by `parse_refinement`; no log-specific FSL grammar is added. It maps external
action names/parameters to spec actions and maps the observed post-action state
to the spec's logical state.

This is finite runtime evidence, not a proof. It does not check `leadsTo`, infer
missing state, query production systems, or replace the existing fsl-db,
fsl-ai, and fsl-domain evidence commands.

Normalized generated-code traces whose names already match Public Kernel use
the versioned complete-state contract in
[`DESIGN-replay-trace.md`](DESIGN-replay-trace.md). This mapping path remains
for production records with external names and schemas. Its mapping target
`-> stutter` is separate syntax; public replay traces encode an observation
stutter as JSON `action:null` and never reserve the action name `stutter`.

## 2. Record and mapping contracts

Every non-empty JSONL line is one object:

```json
{"action":"cart_item_added","params":{"user":0,"item":1},"state":{"inventory":{"0":2,"1":1},"active_cart":{"0":1}}}
```

- `action`: external action name.
- `params`: object whose keys exactly match the mapping's external formal
  parameters.
- `state`: the complete external post-action state needed by every `map`
  expression.

The mapping is ordinary refinement syntax:

```fsl
refinement ProductionLogToShoppingCart {
  impl ProductionLog
  abs ShoppingCart

  map stock[i: ItemId] = inventory[i]
  map cart[u: UserId] = active_cart[u]

  action cart_item_added(user, item) -> add_to_cart(user, item)
  action order_checked_out(user) -> checkout(user)
  action metrics_flushed() -> stutter
}
```

`impl` labels the external log schema; there is no second FSL spec on the CLI.
`abs` must equal `SPEC`'s name. Every target state variable needs a `map`.
`maps auto` supplies same-name state mappings and same-name action mappings on
demand. `preserve progress` is rejected because finite logs cannot establish
liveness.

Mapping expressions use the refinement expression AST and concrete FSL
operators. JSON objects support field access (`snapshot.count`) and indexed
access (`inventory[i]`); numeric JSON object keys are matched to bounded-domain
indices, and enum member strings are converted through the target spec's enum
metadata.

## 3. Execution algorithm

For each record, in order:

1. Select the external action mapping and evaluate its target arguments.
2. Execute the mapped action with `Monitor.step`, or preserve the current state
   for `stutter`.
3. Evaluate every state mapping against the record's `state`.
4. Convert the result to the target spec's logical JSON representation and
   compare it with `Monitor.state`.
5. Stop on the first rejected action, mapping failure, or state mismatch.

The monitor always begins at the spec's deterministic `init`. The observed
state is therefore a post-action assertion, not a replacement initial state.

## 4. Result contract

Success returns `result:"conformant"`, `source:"jsonl_mapping"`, the mapping
name, `steps_checked`, and `final_state`.

Failure returns `result:"nonconformant"` and preserves the existing
`failed_at_event` field while adding:

- `failed_at_record`: zero-based non-empty JSONL record index;
- `log_line`: one-based physical line number;
- `violation.kind`: the Monitor violation kind, `log_mapping`, or
  `state_mismatch`;
- for state mismatch, `expected_state`, `observed_state`, and leaf
  `mismatches[]` paths.

Malformed JSONL or invalid mapping files are command errors (exit 2). A
well-formed record that cannot be mapped or that leaves the spec world is
nonconformant (exit 1).

## 5. Partial observation and external dialects

The first version deliberately requires complete mapped state. A missing
field/key is `log_mapping`, not an unconstrained value: treating absence as a
free variable would let incomplete telemetry appear conformant. A future
partial-observation mode needs an explicit three-valued/constraint contract and
must remain visibly weaker in the JSON faithfulness metadata.

The existing `db observe`, `ai replay`, and `domain replay` commands carry
domain-specific correlation and finding schemas. They are not rewritten here.
They could reuse this core only after their event shapes can preserve those
diagnostics; sharing a generic mapper must not erase domain evidence.

## 6. Coupled-change and tests

The implementation is in `src/fslc/log_replay.py`; CLI dispatch remains in
`src/fslc/cli.py`. `tests/test_log_replay.py` covers conformant replay, first
action/state divergence, incomplete observation, parser/AST identity with
refinement, indexed Map + enum conversion, object-field access, malformed
JSONL line reporting, and the public CLI.
