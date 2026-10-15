Changed (#1282): `fslc mutate --oracle-attribution` reuses the main oracle's
result (its type-bound killer and its verdict) instead of recomputing the
boundary probe, the bounds-only check, and the whole main oracle for every
killed mutant; only the per-property isolated models are checked again. The
`killers` and `by_obligation` output is unchanged, except that a `build_spec`
killer on a type-bound kill can appear or disappear, as it already could
when earlier Z3 calls in the same process changed.
