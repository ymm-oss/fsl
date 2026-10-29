Fixed (#1587): Local expression binders that shadow state names no longer count as state reads
in frozen-state vacuity warnings, explain trace blame, or document undecided
requirement associations. This also prevents a spurious symbolic range-bound
error after explicit verification of nested aggregates with shadowed binders.
