Unified (#1870, second step): the refinement sequence (load both sides, parse
the mapping, walk, then the progress stage on a walk that refines) is one
library function, `fslc_rust::refine_run::run_refinement`. `fslc refine` (and
through it native governance preservations) and the Worker's governance
preservations both call it with their own loaders, solver and depths, instead
of the Worker restating the sequence. Command output and exit codes are
unchanged.
