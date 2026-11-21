Changed (#1892, performance): `mutate` no longer runs the main oracle's
bounds-only BMC call when the explicit engine certifies the bounds-only model
clean through the depth. The certificate is the full-model explicit run when
it completed with only a reachable unwitnessed, or one explicit attempt on the
bounds-only model, made only after a full-model run that stayed within its
budget and a boundary probe of at most 3,000 states. Explicit still certifies
clean only, so results are unchanged: on the 115 corpus specs `mutate`
accepts, `--by-requirement` stdout is byte-identical (14 of them also with
`--oracle-attribution`), and the corpus runs 35% faster
(`specs/inventory_reservation.fsl` 186 s to 16 s) with no spec measurably
slower.
