Fixed (#1515): prevent symbolic and explicit engines from producing opposite
verdicts when a range bound reads a name shared by a local binding and a global
constant. The frontend rejects the collision before verification, preserving
identical complete error envelopes across `check`, BMC, induction and explicit,
and checking component names before prefixing and `def` parameters before
inlining. Engine range-bound evaluators are unchanged.
