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
base-to-fixed check sweep of all 579 repository forms has zero ok-to-error
transitions; inline standalone-refusal controls demonstrate this removal even
though those invalid source forms do not occur in the checked corpus.
