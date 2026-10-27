Fixed (#1506): Reject overflowing causal analysis maximum-time sums with the
contributing claim's location instead of silently saturating. Timeline SCC-internal
sums only include components reachable from the intervention being evaluated.
This restriction does not apply to declared feedback cycle sums in graph or review analyses.
Representable results, output schemas, and source-check acceptance are unchanged.
