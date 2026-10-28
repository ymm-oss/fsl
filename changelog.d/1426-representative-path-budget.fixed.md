Fixed (#1426): Causal representative-path projection now prunes dead ends and stops
after 100,000 DFS expansions. Budget-limited output marks `paths_truncated_exact` as
`false` and reports the omitted paths counted so far as a lower bound; completed
walks retain the existing exact counts and output shape. The schema constrains
the optional exactness marker to `false`; absence remains the exact-count signal.
