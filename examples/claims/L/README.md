# L-tier scaling baseline

`claims_L_requirements.fsl` is the checked L-tier scaling baseline. It carries the inline
business `implements` mapping and uses `Claim = 0..1`.

To reproduce the scaling inputs locally, copy `claims_L_requirements.fsl` beside its
sibling `claims_L_business.fsl` and change only `type Claim` to `0..3` (n4) or
`0..7` (n8). The high-memory copies are not stored in this repository; their
reproduction procedure is maintained in [issue #1041](https://github.com/ymm-oss/fsl/issues/1041).

They cannot live in `specs/`, `examples/`, or `rust/fslc/tests/fixtures/`: each
is a required whole-tree check population, and local capped checks exceeded
8 GiB. This follows the source-string reproduction precedent in
`rust/fslc/tests/issue_697_all_properties_memory.rs`; CI OOM itself has not
been reproduced.

Run only the baseline here:

```bash
fslc check examples/claims/L/claims_L_requirements.fsl
```

(`claims_L_n2.fsl` was a byte-identical duplicate of `claims_L_requirements.fsl` — same
sha256 `188faefafd1a63caf6586e0f0163c128d713d6f67f7f5213e479299480b7c99a`, not merely the same
range bound — and was not referenced by `fsl-project.toml`'s chain. It has been removed; this
file is both the chain's requirements input and the scaling baseline.)

## Deviation from BRIEF §3-2 (approved by controller, 2026-09-15)

This tier does **not** carry the `claims_L_n2.fsl` / `n4.fsl` / `n8.fsl` three-file family that
BRIEF §3-2 originally specified. The controller approved replacing that scaling axis:

- `n4` and `n8` cannot be checked: `fslc check` on the inline-`implements` design file at those
  bounds exceeds 8 GiB RSS (issue [#1041](https://github.com/ymm-oss/fsl/issues/1041)). There is
  no location for such an input — not `specs/`, not `examples/`, not
  `rust/fslc/tests/fixtures/` — because all three are populations that a required whole-tree
  `check` sweep walks (`rust/fslc/tests/corpus_check_sweep.rs`), and placing an 8 GiB+ input in
  any of them risks OOM inside that required job.
- The scaling axis is replaced by issue #1041's `Amount` × `ClaimId` nine-point family (each
  point paired with a no-`implements` control variant, so growth attributable to `implements`
  itself is isolated from growth attributable to the state space). That family, not `n4`/`n8`, is
  the scaling evidence for this tier.
- The generator for that nine-point family is not stored in this repository; its full source is
  quoted in an issue #1041 comment, sha256
  `e14f127ada3ebbf07c5a2fecd627fa0d7d2c4db7af4ccd68af7cb44ca4686787`.

## Refinement checks

`claims_L_requirements_refines_business.fsl` — expected result: `refines`.

```bash
fslc refine examples/claims/L/claims_L_requirements.fsl \
  examples/claims/L/claims_L_business.fsl \
  examples/claims/L/claims_L_requirements_refines_business.fsl --depth 6
```

`claims_L_design_refines_requirements.fsl` — expected result: `refines`.

```bash
fslc refine examples/claims/L/claims_L_design.fsl \
  examples/claims/L/claims_L_requirements.fsl \
  examples/claims/L/claims_L_design_refines_requirements.fsl --depth 6
```

### Fault injection: both mapping files do detect a broken mapping

The refinement checks above only establish that these two mappings currently pass; that is not
the same as showing they would catch a broken one. Each mapping file was fault-injected once,
in an isolated copy, and reverted (real content unchanged; see `git diff` below):

- `claims_L_requirements_refines_business.fsl`: changed `action receive(c, a) -> intake(c)` to
  `-> approve(c)`. `fslc refine` on the mutated copy: `{"result": "refinement_failed",
  "kind": "abs_requires_failed", "violated_at_step": 1, ...}`.
- `claims_L_design_refines_requirements.fsl`: changed `action pay_submit(c) -> pay(c)` to
  `-> withdraw(c)`. `fslc refine` on the mutated copy: `{"result": "refinement_failed",
  "kind": "abs_requires_failed", "violated_at_step": 3, ...}`.

Both mechanisms work correctly for what they actually check — a mapping that sends an
implementation action to the wrong abstract action is caught. What they do **not** and cannot
check is whether the *abstract* side's own guards are hollow (see the business-layer correction
in the main `examples/claims/README.md`) — that is a different question from "is this mapping
wired correctly," and the fault injection above only speaks to the latter.
```
