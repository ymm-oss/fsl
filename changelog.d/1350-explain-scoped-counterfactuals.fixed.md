Fixed (#1350): `fslc explain` now produces invariant and reachable
counterfactual candidates for kernel specs using `entity` / `number` domains
bounded by a sibling `verify` block, just as for directly bounded `type`
declarations. Invariants without a candidate within the requested depth retain
their depth-limit note instead of silently disappearing. Kernel specs using
`def` (named predicates) also regain counterfactual candidates, with authored
source locations and text preserved. `fslc html` shows the
same restored candidates without changing the explanation JSON format. A
weakening that violates several invariants or prevents several reachable goals
now contributes a candidate to each affected property, retaining the existing
per-invariant ranking. Counterfactual traces record the actual violating
successor and its state changes rather than the Monitor rollback state.
Each invariant is searched in isolation, so its counterfactual trace may pass
through states that violate another invariant. Normal `verify` and `replay`
retain all invariants and stop at the first violation instead.
When a non-user check preempts a target's invariant search, explain now tries
bounded solver-independent concrete search for a valid target witness. If none
is found, the existing note identifies the preempting kind, name and step rather
than claiming absence within depth. Bounded init-assignment removals remain
skipped; no-candidate depth/preemption notes now append the named state root
and the reason that nondeterministic initialization was not analyzed. This is
an intentional note-wording change, with no new JSON field.
Concrete search also continues past partial guards and erroring children,
and refuses witnesses starting from an already-violated initial state.
Preempted no-candidate notes also explain when concrete search could not start,
naming reported free state roots publicly or preserving another startup error's
cause. This is diagnostic only; nondeterministic initialization is not searched.
