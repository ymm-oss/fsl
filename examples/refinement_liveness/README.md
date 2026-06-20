# refinement propagates safety, not liveness

An example that shows at a glance **what `fslc refine` (the fidelity check of
detailed spec ⊒ abstract spec) guarantees and what it does not**. The conclusion:
because refinement is forward simulation, **safety** (invariants, control guards,
inclusion of observable behavior) propagates to lower layers, but **liveness**
(`leadsTo`/`responds`) does not — because refinement allows stuttering (internal
steps in which the lower layer does not change the upper-layer state).

For details, see the note in `docs/DESIGN-layers.md` §6 and `docs/LANGUAGE.md` §10.

## Cast

| File | Role |
|---|---|
| `policy.fsl` | The upper-layer contract. Safety (payment only after approval) + liveness (`leadsTo EveryClaimDecided`). Liveness is secured by `fair` adjudication |
| `design_drops_liveness.fsl` | A design that refines faithfully but drops `fair` on adjudication and has an internal stuttering loop |
| `design_keeps_liveness.fsl` | A design differing from the above only in the `fair` on adjudication (recovers liveness) |
| `design_bypasses_control.fsl` | A design that pays while skipping approval (safety violation) |
| `*_refines.fsl` | Mapping of each design ⊒ policy |
| `*_progress_refines.fsl` | Mapping plus `preserve progress`, which checks the upper `leadsTo` on the design execution |

## Run and expected results

```bash
E=examples/refinement_liveness

# The contract is sound on its own (liveness leadsTo holds, payment is also reachable)
fslc verify $E/policy.fsl --engine induction --deadlock ignore        # proved

# ① Liveness does not propagate: refine passes, yet verifying the same policy at the design layer breaks
fslc refine $E/design_drops_liveness.fsl $E/policy.fsl \
            $E/design_drops_liveness_refines.fsl --depth 8            # refines
fslc verify $E/design_drops_liveness.fsl --depth 8 --deadlock ignore  # violated / leadsTo (lasso)

# ② Resolution: add fair to the progress action and re-verify at each layer, and liveness holds too
fslc refine $E/design_keeps_liveness.fsl $E/policy.fsl \
            $E/design_keeps_liveness_refines.fsl --depth 8            # refines
fslc verify $E/design_keeps_liveness.fsl --depth 8 --deadlock ignore  # verified

# ②b Opt in to liveness-preserving refinement: the dropped-liveness design now fails at refine time
fslc refine $E/design_drops_liveness.fsl $E/policy.fsl \
            $E/design_drops_liveness_progress_refines.fsl --depth 8    # refinement_failed / progress_lost
fslc refine $E/design_keeps_liveness.fsl $E/policy.fsl \
            $E/design_keeps_liveness_progress_refines.fsl --depth 8    # refines + progress

# ③ Safety propagates: a design that skips approval is caught by refine
fslc refine $E/design_bypasses_control.fsl $E/policy.fsl \
            $E/design_bypasses_control_refines.fsl --depth 8          # refinement_failed / abs_requires_failed
```

## Highlights

- **① is the key**: `design_drops_liveness` returns `refines` (safety OK), yet the
  upper-layer liveness policy `EveryClaimDecided` is `violated` at the design layer.
  Since `fair` does not appear in `refine`'s mapping, even if the lower layer drops
  the progress that `fair` secured in the upper layer, it remains a faithful
  refinement. **A `leadsTo`/`responds` policy proved at the business layer is not
  automatically inherited just because `refine` passes.**
- **②**: `design_keeps_liveness` differs from `design_drops_liveness` only in the
  `fair` annotation on adjudication. `refine` cannot distinguish the two designs
  (the mapping is identical). To preserve liveness, re-verify `leadsTo` at each
  layer and annotate the progress action with `fair`.
- **②b**: `preserve progress { respond EveryClaimDecided by approve, reject }`
  keeps ordinary safety refinement semantics unchanged, but additionally checks
  the upper response property after mapping it into the design state. This catches
  the heartbeat loop as `progress_lost`.
- **③**: a safety deviation (`abs_requires_failed`) is reliably detected by refine.
  `fast_pay` jumps to the terminal state `DPaid` but is detected at all depths
  (this also serves as the regression example for the fix to the soundness bug
  that used to miss violations reaching a terminal).

Checked by: `tests/test_refinement_liveness_example.py`.
