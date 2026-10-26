Changed (#1442, breaking removal): synchronized reference arguments read only
header parameters, consts, and state, not `let` bindings in the additional items.
The previously accepted `action s() = b.go(t) { let t = true }`, where that let
is the only declaration of `t`, now fails `check` with exit 2 and
`kind: "semantics"`; the measured diagnostic is
`invalid model expression: public Kernel cannot type identifier 't' at 4:24`.
Declare a header parameter and supply its value at the call site, or read
composition-side state instead; review the changed interface or state semantics.
There is no automatic migration, compatibility bypass, or grace period.
The checked repository corpus has zero ok-to-error transitions; this removal
is demonstrated separately by an inline regression with accepting parameter
and state controls.
