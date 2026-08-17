Fixed (#808): native `fslc mutate` now binds its baseline verification, Kernel/model load,
requirements-trace contract, and mutant enumeration to one captured root-spec snapshot, so a
concurrent edit of the input file can no longer mix one revision's baseline with another
revision's kill-rate.
