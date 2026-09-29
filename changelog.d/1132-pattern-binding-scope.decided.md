Decided (#1132): `x is some(v)` is a binding, never a match against an
already-bound `v`, and the binding is lexically confined — a collision with
an action parameter does not survive into the action body. The Python
implementation already does this; the Rust port does not, and the two return
opposite verdicts on the same specification. `docs/DESIGN-pattern-binding-scope.md`
records the decision, the measurements, and the port fixes it implies
(including a Rust-only `trace state mismatch` internal error on a failed
shadowing match).
