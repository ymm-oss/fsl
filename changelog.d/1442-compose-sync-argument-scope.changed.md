Changed (#1442, breaking removals): synchronized reference arguments read only
header parameters, consts, and state, not `let` bindings in the additional items.
The previously accepted `action s() = b.go(t) { let t = true }`, where that let
is the only declaration of `t`, now fails `check` with exit 2 and
`kind: "semantics"`; its cause is
`public Kernel cannot type identifier 't'`, retaining the original component
position. As with other component-owned model errors, the diagnostic is wrapped
at the component's `use` declaration with that inner cause and component
file, line and column preserved.
Declare a header parameter and supply its value at the call site, or read
composition-side state instead; review the changed interface or state semantics.
There is no automatic migration, compatibility bypass, or grace period.
The checked repository corpus has zero ok-to-error transitions; this removal
is demonstrated separately by an inline regression with accepting parameter
and state controls.
Composition also no longer rescues originally undeclared component value reads
or update targets through header parameters, composition state, foreign enum
members, or generated state/const names. Such inputs now retain the standalone
component's semantic cause and original component location; late model errors
retain them inside the existing component-use wrapper. Declare or
correct the name in its owning component, or pass a value through an explicit
component parameter; there is no automatic migration or grace period. The
check sweep comparing base `9ad07c7ad8327b637d8d419eee7a313dc726e9e5`
and implementation head `24a807883786e3ad3477c6ecbbdace2d5378441c`
covers all 597 repository forms with zero ok-to-error
transitions; inline standalone-refusal controls demonstrate this removal even
though those invalid source forms do not occur in the checked corpus.

Changed (#1442): `check` now validates each `leadsTo` helpful-action argument
with the ordinary expression typechecker against the corresponding action
parameter type, in the property's binder scope. Previously unchecked argument
expressions with wrong types, unknown names, or forbidden pattern shadowing
now reject before verification. Name and arity checks remain unchanged.
Diagnostics identify the helpful clause; individual argument spans and the
state-independence rule remain follow-ups in #1483.
Composition also preserves original-source pattern refusals after aggregate
normalization: prefixing cannot rescue a nonempty `count` or `sum` whose
pattern shadows an original global. Empty aggregates remain accepted when
their standalone source accepts them. No engine semantics or bypass is added.
