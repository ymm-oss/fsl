Fixed (#796): `domain analyze` and `domain expand` now reject unresolved domain
identifiers with the same located semantic diagnostic as `check`, instead of
returning a false-green analysis or an unusable generated Kernel.
