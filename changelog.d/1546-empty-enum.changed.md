Changed (#1546, breaking removal): `check` and every model-loading command,
including fmt, lint, kernel, diff, all verification engines, scenarios and
testgen, reject empty spec/Kernel enums even when unused, with kind type,
`enum '<Name>' has no members` and CLI exit 2. Where the command publishes loc,
it is the declaration start (symmetric when present); a compose child uses the
#567 parent-use loc wrapper
with child path/position in its message. Domain keeps its earlier semantics
error. Tool-less standalone `ai_component` inputs, including empty authority,
are rejected at the kernel boundary by check, ai check, BMC, induction,
scenarios, testgen and Worker checking with exact
`ai_component '<Name>' declares no tools` and component loc; one tool with empty
authority remains valid. Existing bad-rule/unknown-authority errors keep their
priority. This does not change ai compat, ai replay or ai_project paths; their
tool-less validation gap remains #1545. Column-less dbsystem databases (schema0,
no tables or only an empty table) are also breaking removals: check, BMC,
induction, explicit, scenarios, testgen and kernel report exact
`database '<Name>' declares no columns`, type/exit2 and database loc. db check
reports that error only when it reaches lowering; static compatibility findings
still come first (an artifact reading an absent column can yield violated/exit1,
as on base). DB fmt/lint keep their existing semantics/exit2 rendering with a
location suffix in the message and no loc field. Empty `EmptyImpl` / `EmptyAbs`
refinement inputs now
fail before mapping elaboration: kind/exit are unchanged, message changes from
`enum conversion endpoint 'X' has no members` or
`enum abstraction endpoint 'X' has no members` to `enum 'X' has no members`, loc
moves from the mapping file to the impl/abs declaration, and the later mapping
span object is absent. See the
[boundary decision](../docs/design/DESIGN-enum-member-identity.md#nonempty-enum-declarations-1546).
