# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Calibration controls for the parser-backed rust-cache key-generation audit."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = REPO_ROOT / ".github" / "scripts" / "validate_rust_cache_key.py"
SPEC = importlib.util.spec_from_file_location("validate_rust_cache_key", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validator
SPEC.loader.exec_module(validator)

HASH_STEP = """      - name: Hash Cargo profiles for the rust-cache key
        if: steps.scope.outputs.run == 'true'
        run: node .github/scripts/cargo-profile-hash.mjs rust/Cargo.toml
"""
CACHE_STEP = """      - uses: Swatinem/rust-cache@v2
        if: steps.scope.outputs.run == 'true'
        with:
          save-if: false
          workspaces: rust -> target
          shared-key: rust-workspace
          prefix-key: v1-rust
          env-vars: FSL_CARGO_PROFILE_HASH
"""


def workflow(steps: str) -> str:
    return f"""name: ci
on: [push]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
{steps}"""


def write_repository(tmp_path: Path, workflows: dict[str, str], *, prefix: str = "v1-rust") -> Path:
    root = tmp_path / "repository"
    scripts = root / ".github" / "scripts"
    scripts.mkdir(parents=True)
    (scripts / "audit-cache-budget.mjs").write_text(
        f'export const CURRENT_KEY_PREFIX = "{prefix}";\n', encoding="utf-8"
    )
    (scripts / "cargo-profile-hash.mjs").write_text(
        'export const PROFILE_HASH_ENV = "FSL_CARGO_PROFILE_HASH";\n', encoding="utf-8"
    )
    directory = root / ".github" / "workflows"
    directory.mkdir(parents=True)
    for name, text in workflows.items():
        (directory / name).write_text(text, encoding="utf-8")
    return root


def audit(tmp_path: Path, steps: str, **kwargs: object) -> list[str]:
    root = write_repository(tmp_path, {"ci.yml": workflow(steps)}, **kwargs)
    _audited, errors = validator.audit_repository(root, minimum_audited_steps=1)
    return errors


def test_live_repository_passes() -> None:
    audited, errors = validator.audit_repository(REPO_ROOT)
    assert errors == []
    assert audited >= validator.MINIMUM_AUDITED_STEPS


def test_accepting_minimal_step_pair(tmp_path: Path) -> None:
    assert audit(tmp_path, HASH_STEP + CACHE_STEP) == []


def test_accepting_quoted_and_unconditional_values(tmp_path: Path) -> None:
    hash_step = HASH_STEP.replace("        if: steps.scope.outputs.run == 'true'\n", "")
    cache_step = (
        CACHE_STEP.replace("        if: steps.scope.outputs.run == 'true'\n", "")
        .replace("prefix-key: v1-rust", 'prefix-key: "v1-rust"')
        .replace("env-vars: FSL_CARGO_PROFILE_HASH", "env-vars: 'FSL_CARGO_PROFILE_HASH'")
    )
    assert audit(tmp_path, hash_step + cache_step) == []


@pytest.mark.parametrize(
    ("old", "new", "expected"),
    [
        ("          prefix-key: v1-rust\n", "", "prefix-key"),
        ("          prefix-key: v1-rust\n", "          prefix-key: v0-rust\n", "prefix-key"),
        # A commented-out input is absent to GitHub Actions; a line scanner that
        # matched the text would accept it.
        ("          prefix-key: v1-rust\n", "          # prefix-key: v1-rust\n", "prefix-key"),
        ("          env-vars: FSL_CARGO_PROFILE_HASH\n", "", "env-vars"),
        ("          env-vars: FSL_CARGO_PROFILE_HASH\n", "          env-vars: CARGO\n", "env-vars"),
    ],
)
def test_rejecting_cache_step_inputs(tmp_path: Path, old: str, new: str, expected: str) -> None:
    assert old in CACHE_STEP
    errors = audit(tmp_path, HASH_STEP + CACHE_STEP.replace(old, new))
    assert len(errors) == 1, errors
    assert expected in errors[0]


def test_rejecting_audit_constant_drift(tmp_path: Path) -> None:
    errors = audit(tmp_path, HASH_STEP + CACHE_STEP, prefix="v2-rust")
    assert len(errors) == 1, errors
    assert "'v2-rust'" in errors[0]


def test_rejecting_missing_hash_step(tmp_path: Path) -> None:
    errors = audit(tmp_path, CACHE_STEP)
    assert len(errors) == 1, errors
    assert "immediately before" in errors[0]


def test_rejecting_hash_step_not_immediately_before(tmp_path: Path) -> None:
    between = "      - uses: dtolnay/rust-toolchain@1.98.0\n"
    errors = audit(tmp_path, HASH_STEP + between + CACHE_STEP)
    assert len(errors) == 1, errors
    assert "immediately before" in errors[0]


def test_rejecting_mismatched_if(tmp_path: Path) -> None:
    hash_step = HASH_STEP.replace("        if: steps.scope.outputs.run == 'true'\n", "")
    errors = audit(tmp_path, hash_step + CACHE_STEP)
    assert len(errors) == 1, errors
    assert "`if:`" in errors[0]


@pytest.mark.parametrize("override", ["working-directory: rust", "shell: bash", "env: {A: b}"])
def test_rejecting_hash_step_overrides(tmp_path: Path, override: str) -> None:
    hash_step = HASH_STEP + f"        {override}\n"
    errors = audit(tmp_path, hash_step + CACHE_STEP)
    assert len(errors) == 1, errors
    assert override.split(":")[0] in errors[0]


def test_rejecting_duplicate_prefix_key(tmp_path: Path) -> None:
    cache_step = CACHE_STEP + "          prefix-key: v0-rust\n"
    errors = audit(tmp_path, HASH_STEP + cache_step)
    # The unreadable file also leaves the step floor unmet: both fail closed.
    assert len(errors) == 2, errors
    assert "duplicate key 'prefix-key'" in errors[0]
    assert "fewer than the 1" in errors[1]


def test_rejecting_fewer_steps_than_the_floor(tmp_path: Path) -> None:
    root = write_repository(tmp_path, {"ci.yml": workflow(HASH_STEP + CACHE_STEP)})
    audited, errors = validator.audit_repository(root, minimum_audited_steps=2)
    assert audited == 1
    assert len(errors) == 1 and "fewer than the 2" in errors[0]


def test_rejecting_missing_constant(tmp_path: Path) -> None:
    root = write_repository(tmp_path, {"ci.yml": workflow(HASH_STEP + CACHE_STEP)})
    (root / ".github" / "scripts" / "audit-cache-budget.mjs").write_text("// none\n", encoding="utf-8")
    with pytest.raises(ValueError, match="CURRENT_KEY_PREFIX"):
        validator.audit_repository(root, minimum_audited_steps=1)


def test_main_reports_failure(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    root = write_repository(tmp_path, {"ci.yml": workflow(CACHE_STEP)})
    assert validator.main(["--repository", str(root)]) == 1
    assert "rust-cache key audit: FAIL" in capsys.readouterr().err
