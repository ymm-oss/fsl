Changed (#1528, breaking removal): `leadsTo` declaration binders now participate
in def free-name capture checks for before/after expressions, `helpful`
arguments, `decreases`, and `within`, in direct specs, compose components and root
properties, and dialect properties lowered to `leadsTo` (such as requirements).
Previously accepted capturing calls reject with exit 2, `kind: "semantics"`,
and `would capture variable` at the call. Rename the declaration binder; no
automatic rename or bypass is added. Binder ranges and binder-where expansion
are unchanged, as are constant/enum `helpful` arguments from #1580.

The state-reading `within` control already failed closed: exit 2 / semantics
is unchanged, but for the acceptance source `unknown constant 'c'` becomes
`predicate 'Bound' call would capture variable 'c'; rename the binder at the call site at 9:41`.
The old check loc 4:23 is replaced by the call position embedded in the message;
the fixed check/BMC/induction error envelopes omit structured loc and agree in full.
This is an error-to-error diagnostic change, not a false-green baseline.
When a `leadsTo` also contains an already-invalid pattern-scope escape (#1547),
`would capture` can likewise precede that rejection; exit 2 / semantics is unchanged.

Separately built `git archive` trees for base
`08d9656c5548a7ae5cbd2156880fdf52f9d0b8fc` and implementation
`c1a7d126071d8ac0600ffcd13d3ea9b3f0b7f286` checked all 601 `.fsl` corpus files
under specs, examples, and CLI fixtures, plus the one migrated inline source
(602 forms). All corpus stdout/stderr bytes and exits agree: zero ok-to-error
transitions and zero error-to-error diagnostic changes. The inline source is
the only ok-to-error transition, a declaration binder capturing a def-body free
state name in a decreases call. No unrelated acceptance is removed.
