Fixed (#1245): `fslc refine` (single and chain), the `fslc chain` refine layer,
and governance preservations (native and browser Worker) no longer report a
refinement correspondence check cut off by its fixed state budget (50,000
states, #1041) as `refines`. They now report `unknown_budget`: `fslc refine`
exits 1 with `states_explored` and a `hint`, a chain stops at that link, a
`fslc chain` refine layer fails, and a preservation's `result` is
`unknown_budget` — the vocabulary and exit the inline `implements` seam
already used. The Worker governance path also reports a self-violating
`after` spec as `violated` instead of `refines`, matching native. Migration:
a refinement whose correspondence check visits more than 50,000 states
previously reported `refines` / exit 0 and now reports `unknown_budget` /
exit 1; lower `--depth` or narrow both layers' `verify {}` domains.
