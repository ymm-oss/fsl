Unified (#1873): ranked `leadsTo` helpful-fairness, sticky-enabledness and
plain/helpful progress obligations use the scoped-query owner, preserving their
Unknown errors and pop-failure precedence. Non-negativity and no-deadlock remain
inline because they interleave term construction with assertions after push;
prebuilding those terms would change the solver query history and cost.
