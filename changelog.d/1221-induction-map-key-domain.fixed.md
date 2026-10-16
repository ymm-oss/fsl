Fixed (#1221): `fslc verify --engine induction` no longer reports `proved`
for a spec that indexes a finite-key `Map` outside its key domain (for
example `m[cur + 1]` once `cur` reaches the top key) beyond `--depth`. The
step's definedness obligation now also asks, for every invariant, `leadsTo`
trigger/goal, `trans`, guard, body (reads and indexed assignments) and
reached `ensures`, whether a state satisfying the proved invariants reaches
such a miss; a `sat` answer is `unknown_cti` with the new
`violation_kind: "key_domain"` and `invariant: "_key_domain_<action>"` /
`"_key_domain_property_<name>"`. Within depth the base case reports the same
error as BMC and explicit (`map index outside finite key domain` /
`map assignment index outside key domain`), so the three engines agree.
Only sites the obligation catalog cannot place inside the key type are
asked; in-domain reads such as `forall k: K { m[k] >= 0 }` add no query.
Checked i64 overflow is still not part of the step obligation (#1263).
Migration: a spec that was `proved` (exit 0) can now be `unknown_cti`
(exit 1) when an invariant-satisfying state misses the key domain, or
`error` (exit 2) when the miss is reachable within `--depth`; guard the
index (`requires cur < 5`, `cur < 5 => m[cur + 1] > 0`) or, if the CTI start
is unreachable, add an auxiliary invariant that excludes it.
