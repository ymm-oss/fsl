Replaced (#251): Inline branch maps in the public return example with explicit
fair actions and local `implements` correspondences. The public refinement
correspondences intentionally change from `submit.b1` / `submit.b2` to
`submit_small` / `submit_large` as a result of canonicalizing the example,
without changing its guards, effects, fairness or typed requirement metadata.
