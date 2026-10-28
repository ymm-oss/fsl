Fixed (#1548): nonconformant ledger implementation logs now add red conformance
findings. Aggregate assurance stays bounded when induction leaves a bounded leadsTo.
Early safety violations and explicit deadlock mark tagged reachables `not_run` unless
scenario-confirmed. Unranked leadsTo/reachable rows stay bounded under refinement failure over induction.
