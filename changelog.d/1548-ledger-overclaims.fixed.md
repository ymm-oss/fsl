Fixed (#1548): nonconformant ledger implementation logs now add red conformance
findings. Aggregate assurance stays bounded when induction leaves a bounded leadsTo.
BMC early safety violations mark tagged reachables `not_run`.
Unranked leadsTo/reachable rows stay bounded under refinement failure over induction.
Migration: ledger/HTML rendering digests change for induction with an unranked leadsTo,
BMC early violations with tagged reachables, a nonconformant `--impl-log`, and
refinement failure (or an undecided implements seam) over induction. Existing approvals
report `drifted` (exit 0); review and re-approve affected artifacts.
