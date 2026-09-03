# CI validator inventory

Accepted decision for issue #962 slice 1: record every `tests/test_*.py`
validator module in a machine-generated inventory and fail closed when a new
module appears without required-gate wiring or an explicit exempt
classification.

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
  outside required gates (`docs/DESIGN-rust-integration.md`).
- `hook-local` — agent hook contract tests with timing or local-environment
  dependencies (`tests/test_hook_enforcement.py`; see
  `docs/DESIGN-hooks-enforcement.md`).

Unwired validators are not defects by default. Only `required` rows must be
reachable from the scanned entrypoints.

## Controls

1. **Inventory completeness** — every tracked `tests/test_*.py` module must
   appear in the committed inventory; new modules fail `check` until classified.
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

## Scope boundaries

- Slice 1 does **not** wire the existing 98 unwired frozen-Python modules.
- `tools/check_rust_*.py` harness disposition and orphan binaries are issue
  #761 (stages 2–3), not this inventory.
- `tests/test_hook_enforcement.py` remains exempt (`hook-local`); its timing-
  dependent cargo-lock serialization test is not promoted to a required gate.

## Related documents

- `docs/DESIGN-ci.md` — required pre-merge contexts and automation contracts
- `docs/DESIGN-hooks-enforcement.md` — hook-local exempt boundary
- `docs/DESIGN-rust-integration.md` — frozen Python compatibility boundary
