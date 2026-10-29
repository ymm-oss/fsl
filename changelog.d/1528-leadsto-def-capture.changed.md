Changed (#1528, breaking removal): `leadsTo` declaration binders now participate
in def free-name capture checks for before/after expressions, `helpful`
arguments, `decreases`, and `within`, in direct specs and compose components.
Previously accepted capturing calls reject with exit 2, `kind: "semantics"`,
and `would capture variable` at the call. Rename the declaration binder; no
automatic rename or bypass is added. Binder ranges and binder-where expansion
are unchanged, as are constant/enum `helpful` arguments from #1580.

The state-reading `within` control already failed closed: exit 2 / semantics
is unchanged, but `unknown constant 'c'` becomes
`predicate 'Bound' call would capture variable 'c'; rename the binder at the call site at 9:41`.
The old check loc 4:23 is replaced by the call position embedded in the message;
the fixed check/BMC/induction error envelopes omit structured loc and agree in full.
This is an error-to-error diagnostic change, not a false-green baseline.

Separately built `git archive` trees for base
`f5f3f1942220ad6c1e1802fe9bc523fabe2155ae` and implementation
`2c955b9756b5fd8f5d0650ffef650ba9952571bc` checked all 598 `.fsl` corpus files
under specs, examples, and CLI fixtures, plus the one migrated inline source
(599 forms). All corpus stdout/stderr bytes and exits agree: zero ok-to-error
transitions and zero error-to-error diagnostic changes. The inline source is
the only ok-to-error transition, a declaration binder capturing a def-body free
state name in a decreases call. No unrelated acceptance is removed.
