Fixed (#1255): `fslc conformance` names a compose component invariant broken
while another action stepped by its public name, `acct.Inv` (was `acct__Inv`),
the name `verify` and `replay` give it. `fslc mutate` compares the dead
actions of the baseline (`action_coverage` keys, `acct.thaw`) with the mutant's
action in the same spelling, so a never-enabled component action's surviving
mutants carry `note: "action dead at baseline — survival expected"` in a
compose as they do in a single spec. Migration: a consumer that matched
`alias__Name` in a conformance `outcome.name` must match `alias.Name`.
