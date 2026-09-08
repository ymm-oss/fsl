Inline `implements` refinement failures on `fslc check` and `fslc verify` now
fold into the top-level `result` (`refinement_failed` or `impl_violated`) and
exit 1 instead of reporting success with the failure nested under `implements`
only.
