Changed (#1234): a compose component action now has one public name,
`alias.action`, across testgen, conformance, the Monitor, verify/scenarios/explain
traces, and `fslc replay`. A v1 replay trace can now write `bank.settle`, the
name testgen emits, so a generated test's actions replay `conformant` instead of
failing with `bad_call`. v1 replay still accepts the older `bank__settle` for
the same action, looked up in the model's table of component actions rather
than rewritten; sync actions and non-compose actions such as `foo__bar` keep
exact-name matching. Replay's `state_mismatch.action` now reports `bank.settle`
where it used to echo `bank__settle`. The name is built from the `use` alias
and the action name, so `use X as a__b` with action `c` is `a__b.c` (it was
`a.b__c`), matching the frozen Python reference. Two other outputs that still
printed internal names now use the public form: conformance outcome names
(`_requires_failed_bank.settle`) and verify's `cost.properties[].name`.
