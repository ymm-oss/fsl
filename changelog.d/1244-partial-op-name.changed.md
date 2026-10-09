Changed (#1244), **breaking for stored `fslc conformance` and `fslc replay`
output**: a partial operation reached in an action's guard (`requires` or
`let`, such as `requires 2 / x >= 0` with `x == 0`) is now named
`_partial_<action>` by `conformance` (`vectors[].outcome.name`) and `replay`
(`violation.name`), the name `verify` already gave it on every engine and the
name a body partial operation already had everywhere. It was
`_partial_op_<action>`, so the same failure could not be matched between a
conformance vector and the verify result for one spec. For a compose
component action the name is `_partial_alias.action` (it was
`_partial_op_alias.action`). The `kind` stays `partial_op`. To migrate,
rewrite stored conformance vectors, replay results, or test expectations
from `_partial_op_<action>` to `_partial_<action>`.
