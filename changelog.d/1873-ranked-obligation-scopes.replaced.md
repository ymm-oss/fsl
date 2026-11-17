Replaced (#1873): ranked `leadsTo` helpful-fairness, sticky-enabledness and
plain/helpful progress obligations use the scoped-query owner, preserving their
Unknown errors and pop-failure precedence on completed checks. The owner also
attempts pop after assertion, check, or model-read errors; a failing pop takes
precedence over those errors. Non-negativity and no-deadlock remain
inline because they interleave term construction with assertions after push;
prebuilding those terms would change the solver query history and cost.
