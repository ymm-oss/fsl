Fixed (#1189): `verify --engine induction` no longer reports a ranked
`leadsTo ... decreases M` as `proved` / `unbounded` when the obligation can be
pending in a state where nothing that must fire is enabled. The ranking proof
asserted the transition relation on its pre-state, which forced some action
to be enabled there, so the `no_deadlock` obligation of
`docs/design/DESIGN-induction.md` §2.3 never saw such a state: without
`helpful` it was not checked at all, and with `helpful` a pending state in
which no action at all was enabled was skipped. The obligation is now asked
over an unconstrained invariant state and fails as `unknown_cti` /
`violation_kind: "leadsTo_rank"` with `rank_failure: "deadlock"` (or
`helpful_action_not_enabled`), as in the frozen Python reference. It is a
proof obligation over invariant states, so `--deadlock` does not disable it,
and an unreachable deadlocked pending state blocks the proof until an
invariant excludes it.
Visible change: at a depth large enough for the base-case BMC to find the
stall, such a spec now reports `unknown_cti` / `rank_failure: "deadlock"`
instead of `violated` / `leadsTo` (the existing rule prefers the rank failure
when the base case shows a leadsTo violation, as Python does); the exit code
stays 1.
