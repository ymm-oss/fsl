Changed (#1168): removed the unreachable `run_mutate_legacy` and its three
mutant-count helpers from `fslc` (all behind `#[allow(dead_code)]`, called
from no command dispatch). `fslc mutate` is unchanged.
