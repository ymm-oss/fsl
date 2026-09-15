Fixed (#997): `fslc diff` no longer silently drops a `verify { values X = lo..hi }`
bound from `scope.old`/`scope.new`/`scope.applied_to_old` when `lo`/`hi` is a
declared const or a compound expression (e.g. `-1..HI`) instead of a bare
integer literal. The bound is now resolved against the same evaluated
domain `check` reports (`TypeDef::Domain`), so a const-only bound change is
recorded as `scope_changed`, applied to OLD, and rejected by
`--forbid scope_changed` the same way a literal bound change already was.
