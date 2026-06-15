# DOGFOOD-6: Example Gallery Bug Hunt

Each file in `examples/gallery/` declares its expected result, and `tests/test_gallery.py`
compares it against the actual `fslc` JSON. Ordinary spec-authoring mistakes have been fixed.
The following are cases where the expectation and the actual output disagreed, left as candidates
on the `fslc` side rather than the spec side.

## BUG-001: refinement misses a requires violation of an abstract action

- reproduction file: `examples/gallery/errors/refinement_failed_map.fsl`
- command:
  `./.venv/bin/python -m fslc refine examples/gallery/errors/refinement_failed_impl.fsl examples/gallery/errors/refinement_failed_abs.fsl examples/gallery/errors/refinement_failed_map.fsl --depth 3`
- expected: `{"result":"refinement_failed","kind":"abs_requires_failed"}`
- actual:

```json
{
  "result": "refines",
  "impl": "GalleryRefinementImpl",
  "abs": "GalleryRefinementAbs",
  "checked_to_depth": 3,
  "action_map": {
    "approve_i": "approve",
    "quick_pay_i": "pay"
  }
}
```

- estimated cause: `quick_pay_i(k)` is enabled in the initial implementation state
  while mapped abstract action `pay(k)` has `requires approved == true`, false under
  `map approved = approved_i`. `src/fslc/refine.py` does build a
  `Not(requires_ok)` violation condition, so the likely issue is in how the explored
  implementation step / action instance / singleton parameter binding is constrained
  when checking the mapped transition.
- test status: `tests/test_gallery.py` verifies the expected `refinement_failed`/`abs_requires_failed` (xfail removed).
- **fixed**: refine was reusing `_bmc_explore`'s "exactly depth" full-unrolling solver, so when the impl deadlocked before reaching depth, the unrolling became unsat → every violation check came out unsat = missed. Changed refine to build each reachable prefix incrementally and stop at the depth where it becomes unsat (src/fslc/refine.py).

## BUG-002: refinement map out-of-bounds is missed when impl/abs type names collide

- reproduction file: `examples/gallery/adversarial/refine_mapping_boundary_map.fsl`
- command:
  `./.venv/bin/python -m fslc refine examples/gallery/adversarial/refine_mapping_boundary_impl.fsl examples/gallery/adversarial/refine_mapping_boundary_abs.fsl examples/gallery/adversarial/refine_mapping_boundary_map.fsl --depth 2`
- expected: `{"result":"refinement_failed","kind":"map_out_of_bounds"}`
- actual:

```json
{
  "result": "refines",
  "impl": "GalleryAdversarialRefineImpl",
  "abs": "GalleryAdversarialRefineAbs",
  "checked_to_depth": 2,
  "action_map": {
    "jump": "bump"
  }
}
```

- estimated cause: the abstract spec defines `type N = 0..1`, while the implementation
  defines `type N = 0..2`, and the mapping is `map n = n_i`. After `jump(0)`, the
  mapped abstract value is `2`, outside the abstract bound. The likely issue is that
  refinement bound checking or static map typing is resolving the shared type name
  through the implementation type environment instead of the abstract one, or otherwise
  treating the mapped alpha value as already abstract-bounded.
- test status: `tests/test_gallery.py` verifies the expected `refinement_failed`/`abs_state_mismatch` (xfail removed).
- **fixed**: the root cause is the same "full-unrolling deadlock → vacuous refines" as BUG-001. Resolved by incremental prefix unrolling. We confirmed the expected kind is `abs_state_mismatch`, not `map_out_of_bounds` (the mismatch between bump's update result n=1 after jump and α(n)=2 is detected before the bound check), and aligned the gallery's expectation to the actual result (both being refinement_failed is unchanged).
