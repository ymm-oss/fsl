# CI validator inventory

Accepted decision for issue #962 slice 1: record every `tests/test_*.py`
validator module in a machine-generated inventory and fail closed when a new
module appears without required-gate wiring or an explicit exempt
classification. Extended by issue #761 stage 2 to `tools/check_rust_*.py`
(reserved as future scope by slice 1's own "Scope boundaries" section below).

## Guarantee boundary

This tool establishes that a validator module's tier and exempt reason were
**recorded**, not that the recorded reason is **correct**. `--exempt
path:reason` costs one command and satisfies `generate`'s guard completely;
that is intentional, not a gap -- the property this check protects is "no
validator module accumulates silently, unclassified" (the root problem
#761's 17 unwired `tools/check_rust_*.py` harnesses demonstrated: nobody had
recorded why any of them were unwired). Whether a recorded reason accurately
describes a specific module -- the F1-F7 precondition analysis, native-owner
cross-referencing, and retirement readiness for the parity harnesses -- is
`docs/RUST-PORTING.md`'s job, a human-maintained document this tool does not
read and cannot verify.

## Problem

Standalone pytest modules can remain green while never executing in a required
pre-merge lane. PR #964 added controls to `tests/test_hook_enforcement.py`,
but CI only runs a name-specific allowlist, so those controls never executed in
the required gate.

## Authority

- Inventory: `.github/ci-validator-inventory.json` (generated; do not hand-edit)
- Generator and auditor: `tools/check_ci_validator_inventory.py`
- Calibration pytest: `tests/test_ci_validator_inventory.py`
- Required gate entrypoints scanned for wiring:
  - `tools/check-merge-readiness.sh`
  - `.github/workflows/site-reference-freshness.yml`

Non-required workflows (for example
`.github/workflows/cache-budget-audit-wiring.yml`) are intentionally excluded
from required-gate resolution.

## Tiers

| Tier | Meaning | Required-gate wiring |
|---|---|---|
| `required` | Merge/product contract validator | Must resolve to a scanned required entrypoint |
| `exempt` | Intentionally unwired validator | Must not appear in required-gate wiring |

Declared `exempt_reason` values:

- `frozen-python-compatibility` — frozen Python reference/product tests kept
  outside required gates (`docs/DESIGN-rust-integration.md`). Also covers the
  7 `tools/check_rust_*.py` parity harnesses whose disposition in
  `docs/RUST-PORTING.md` is F1, F2, F3, F4, F5, F6, or F7 (`corpus_cli_parity`,
  `dialect_parity`, `induction_parity`, `leadsto_parity`, `phase2_commands`,
  `refinement_parity`, `replay_parity`) — each still fundamentally compares
  against the frozen Python reference; the precise stage each is at within
  that F1-F7 pipeline (precondition unmet, native owner pending review, or
  explicitly retained) is recorded in `RUST-PORTING.md`'s own table, not
  re-encoded here (see "Guarantee boundary" above).
- `hook-local` — agent hook contract tests with timing or local-environment
  dependencies (`tests/test_hook_enforcement.py`; see
  `docs/DESIGN-hooks-enforcement.md`).
- `manual-developer-run` (#761 stage 2) — a `tools/check_rust_*.py` harness
  whose own docstring declares it "Optional developer-run", run only for an
  intentional change to a named frozen-Python projection
  (`ast_parity`, `cli_snapshot`, `grammar_fuzz`, `kernel_parity`,
  `surface_parity`). Distinct from `frozen-python-compatibility`: the reason
  these are unwired is a declared process boundary, not (only) that the
  property is frozen-reference-scoped -- `surface_parity` in particular does
  not import the frozen Python package at all.
- `parked-pending-unrelated-work` (#761 stage 2) — `phase3_commands`,
  blocked on an unowned native `ai compare` contract while AI work is
  currently parked; unrelated to Python-compatibility retirement.
- `pending-native-migration` (#761 stage 2) — `bfs_parity`, `bmc_parity`,
  `scenarios_parity`, blocked on the not-yet-complete native BFS/BMC
  migration (`docs/DESIGN-bfs-bmc-native-migration.md`), a tracked multi-PR
  sequence rather than a single harness's own precondition.

Unwired validators are not defects by default. Only `required` rows must be
reachable from the scanned entrypoints.

## Controls

1. **Inventory completeness** — every tracked `tests/test_*.py` and
   `tools/check_rust_*.py` module must appear in the committed inventory;
   new modules fail `check` until classified.
2. **Required wiring** — `tier: required` rows must match live wiring
   discovered from the scanned entrypoints.
3. **Exempt anti-reward** — a module wired into a required entrypoint cannot
   remain `tier: exempt`; inventory-only exempt rows do not satisfy required
   wiring.
4. **Generate guard** — `generate` refuses new unwired modules unless they are
   wired (becoming `required`) or passed explicitly via
   `--exempt path:reason`. `--bootstrap` seeds the initial exempt baseline
   only.

No coverage ratio, score, or “N of M wired” gate is part of this contract.

## Verification

```bash
python3 tools/check_ci_validator_inventory.py selftest
python3 tools/check_ci_validator_inventory.py check
python3 -m pytest tests/test_ci_validator_inventory.py -v
./tools/check-merge-readiness.sh automation
```

Calibration evidence:

- Add a new unwired `tests/test_*.py` module → `check` fails with
  `untracked validator module`.
- Wire it into `tools/check-merge-readiness.sh` and run `generate` → the module
  becomes `tier: required` and `check` passes.
- Add a new unwired `tools/check_rust_*.py` module → `check` fails with
  `untracked validator module` (#761 stage 2, same execution path as the
  `tests/` case above). Measured directly against this repository, not only a
  fixture: `check` on the unmodified tree exits 0 (123 validator modules);
  adding an unclassified `tools/check_rust_probe_injected.py` and re-running
  the identical command exits 1, naming that exact path; removing it restores
  exit 0. Classify it via `generate --exempt
  tools/check_rust_probe_injected.py:<reason>` (one of the four declared
  reasons above) or wire it into a required entrypoint to clear the finding.

## Scope boundaries

- Slice 1 does **not** wire the existing 98 unwired frozen-Python
  `tests/test_*.py` modules.
- **#761 stage 2 (this change)**: `tools/check_rust_*.py` discovery and
  classification are now in scope for this inventory, using the 4 declared
  reasons above. The *disposition* of each harness -- whether it should
  eventually be wired, retired, or stay exempt, and why -- remains
  `docs/RUST-PORTING.md`'s authority; this inventory only records that a
  classification exists, per the guarantee boundary above. Orphan binaries
  (`fsl-parse-expr`, `fsl-parse-kernel`, `fsl-parse-surface`, `fsl-bfs`,
  `fsl-bmc`, `fsl-replay-actions`) are #761 stage 3, still not this
  inventory's concern.
- `tests/test_hook_enforcement.py` remains exempt (`hook-local`); its timing-
  dependent cargo-lock serialization test is not promoted to a required gate.

## Related documents

- `docs/DESIGN-ci.md` — required pre-merge contexts and automation contracts
- `docs/DESIGN-hooks-enforcement.md` — hook-local exempt boundary
- `docs/DESIGN-rust-integration.md` — frozen Python compatibility boundary
- `docs/RUST-PORTING.md` — the 16-harness `tools/check_rust_*.py` disposition
  table this inventory's `exempt_reason` values summarize but do not replace
