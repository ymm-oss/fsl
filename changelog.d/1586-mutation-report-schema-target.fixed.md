Fixed (#1586): the semantic mutation gate's final report schema check reuses
the initial manifest build instead of rebuilding cargo-mutants' differently
flagged dependencies. Report validation, the zero-mutant path, and fresh
per-run mutation build directories are preserved.
