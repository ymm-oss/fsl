Fixed (#1628): Non-Seq induction bases skip the redundant concrete boundary
search only after symbolic BMC verifies every assumed bound. Seq evidence and
non-success priority are preserved; the depth-20 collection deadline is unchanged.
