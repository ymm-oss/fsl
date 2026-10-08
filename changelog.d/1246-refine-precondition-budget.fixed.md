Fixed (#1246): the impl self-consistency precondition that runs before every
refinement correspondence check (`fslc refine` single and chain, the
`fslc chain` refine layer, an inline `implements` seam in `check`/`verify`,
governance preservations, `fslc diff`, `fslc mutate`'s implements oracle) now
stops at the same 50,000-state budget as the correspondence check (#1041).
It walked the whole reachable set unbounded first, so #1041's original
reproducer still aborted above 6 GB; it now reports `unknown_budget`
(exit 1, `states_explored: 50000`) at about 1.2 GB. A precondition cutoff is
reported exactly as a correspondence cutoff, through the same consumers.
Migration: an impl whose reachable set within the depth reaches 50,000 states
now reports `unknown_budget` even where it used to report a decided result
because the unbounded precondition happened to fit in memory — a
self-violation found beyond the first 50,000 states (`violated` /
`impl_violated`), or a correspondence failure the walk found before its own
budget (`refinement_failed`). Lower the depth or narrow both layers'
`verify {}` domains. `fslc diff` and `fslc mutate` still do not read this
cutoff (#1262): on such an impl `fslc diff` can now report `no_semantic_change`
(exit 0) where it reported `impl_violated`, and `fslc mutate` counts the
mutant as survived.
