Changed (#1892, performance): `mutate` no longer runs the main oracle's
bounds-only BMC call when the explicit engine certifies the bounds-only model
clean through the depth. The certificate is the full-model explicit run when
it completed with only a reachable unwitnessed, or one explicit attempt of at
most 3,000 states on the bounds-only model. That attempt is made when the
full-model run stopped at an invariant, transition or reachable violation, or
was unsupported because of `leadsTo` (which the bounds-only model drops), and
the boundary probe walked at most 3,000 states; it is not made when the
full-model run did not happen, failed, overran its budget, or stopped at an
obligation the bounds-only model keeps. Explicit still certifies clean only,
and the skip assumes explicit and BMC agree on what is clean: where they do
not (for example #1961, a missed `terminal` partial operation), the killer can
change. Results are unchanged on the corpus: on the 115 specs `mutate`
accepts, `--by-requirement` stdout is byte-identical (14 of them also with
`--oracle-attribution`), and the corpus runs 31% faster
(`specs/inventory_reservation.fsl` 162 s to 12 s).
