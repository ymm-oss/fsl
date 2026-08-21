# SPDX-License-Identifier: Apache-2.0

"""Calibration controls for the parser-backed Rust-toolchain pin audit."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest

MODULE_PATH = Path(__file__).resolve().parents[1] / ".github" / "scripts" / "validate_toolchain_pin.py"
SPEC = importlib.util.spec_from_file_location("validate_toolchain_pin", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validator
SPEC.loader.exec_module(validator)

ACTION = validator.ACTION
SHA = "4cda84d5c5c54efe2404f9d843567869ab1699d4"


def write_repository(
    tmp_path: Path,
    workflows: dict[str, str],
    *,
    rust_version: str = "1.88",
    actions: dict[str, str] | None = None,
) -> Path:
    """Build the smallest repository accepted by the audit."""
    root = tmp_path / "repository"
    (root / "rust").mkdir(parents=True)
    (root / "rust" / "Cargo.toml").write_text(
        f'[workspace.package]\nrust-version = "{rust_version}"\n', encoding="utf-8"
    )
    workflows_directory = root / ".github" / "workflows"
    workflows_directory.mkdir(parents=True)
    for name, text in workflows.items():
        (workflows_directory / name).write_text(text, encoding="utf-8")
    for name, text in (actions or {}).items():
        action = root / name
        action.parent.mkdir(parents=True, exist_ok=True)
        action.write_text(text, encoding="utf-8")
    return root


def audit(
    root: Path,
    *,
    minimum: int = 0,
    msrv_paths: dict[str, str] | None = None,
) -> tuple[int, dict[str, int], list[str]]:
    cargo_toml = (root / "rust" / "Cargo.toml").read_text(encoding="utf-8")
    return validator.audit_repository(
        root,
        validator.declared_msrv(cargo_toml),
        minimum_audited_references=minimum,
        msrv_paths={} if msrv_paths is None else msrv_paths,
    )


def workflow(step: str) -> str:
    return f"""name: probe
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
{step}
"""


def release(step: str) -> str:
    return workflow(step)


def assert_rejected(errors: list[str], diagnostic: str) -> None:
    assert errors, "the mutation must not pass silently"
    assert any(diagnostic in error for error in errors), errors


def test_committed_repository_satisfies_the_required_pin_contract() -> None:
    """The required lane's accepting control is the repository as committed."""
    root = MODULE_PATH.parents[2]
    msrv = validator.declared_msrv((root / "rust" / "Cargo.toml").read_text(encoding="utf-8"))
    audited, seen, errors = validator.audit_repository(root, msrv)
    assert errors == []
    assert audited >= validator.MINIMUM_AUDITED_REFERENCES
    assert seen == {".github/workflows/release.yml": 1}


@pytest.mark.parametrize(
    ("label", "step"),
    [
        (
            "four-space with child",
            f"""      - uses: {ACTION}@{SHA}
        with:
            toolchain: 1.88.0""",
        ),
        (
            "with before uses",
            f"""      - with:
          toolchain: 1.88.0
        uses: {ACTION}@{SHA}""",
        ),
    ],
)
def test_valid_msrv_with_spellings_are_accepted(tmp_path: Path, label: str, step: str) -> None:
    # The line-scanning predecessor falsely rejected these valid spellings.
    root = write_repository(tmp_path, {"release.yml": release(step)})
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert errors == [], label


def test_comment_text_cannot_supply_a_false_msrv_input(tmp_path: Path) -> None:
    # The line scanner read 1.88.0 out of this comment; YAML supplies 1.89.0.
    step = f"""      - uses: {ACTION}@{SHA}
        with: {{
          components: rustfmt, # , toolchain: 1.88.0 }}
          toolchain: 1.89.0
        }}"""
    root = write_repository(tmp_path, {"release.yml": release(step)}, rust_version="1.89")
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert errors == []


def test_quoted_brace_in_flow_mapping_does_not_hide_input(tmp_path: Path) -> None:
    # The line scanner's brace counter saw no input when a quoted value contained }.
    step = f'''      - uses: {ACTION}@{SHA}
        with: {{toolchain: 1.88.0, components: "rustfmt}}"}}'''
    root = write_repository(tmp_path, {"release.yml": release(step)})
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert errors == []


def test_composite_action_toolchain_step_is_audited(tmp_path: Path) -> None:
    # A line scan limited to workflows would miss a repository-owned composite action.
    action = f"""name: local toolchain
runs:
  using: composite
  steps:
    - uses: {ACTION}@stable
"""
    root = write_repository(tmp_path, {"probe.yml": workflow("      - run: true\n")}, actions={".github/actions/toolchain/action.yml": action})
    _audited, _seen, errors = audit(root)
    assert_rejected(errors, "floating channel")


def test_local_reusable_workflow_is_audited_as_its_own_workflow(tmp_path: Path) -> None:
    # A job-level reusable-workflow use reaches this local file, which is parsed too.
    caller = """name: caller
on: push
jobs:
  call:
    uses: ./.github/workflows/reusable.yml
"""
    reusable = workflow(f"      - uses: {ACTION}@stable")
    root = write_repository(tmp_path, {"caller.yml": caller, "reusable.yml": reusable})
    _audited, _seen, errors = audit(root)
    assert_rejected(errors, "floating channel")


@pytest.mark.parametrize(
    ("label", "step"),
    [
        # The line scanner skipped double-quoted action references.
        ("double quoted uses", f'      - uses: "{ACTION}@stable"'),
        # The line scanner skipped single-quoted action references too.
        ("single quoted uses", f"      - uses: '{ACTION}@stable'"),
        # The line scanner required the list dash on the uses: line itself.
        (
            "uses sibling of name",
            f"""      - name: install
        uses: {ACTION}@stable""",
        ),
        # The line scanner required no whitespace before the YAML key colon.
        ("space before uses colon", f"      - uses : {ACTION}@stable"),
        # The line scanner handled the comment as text rather than YAML syntax.
        ("trailing comment", f"      - uses: {ACTION}@stable # predecessor ignored suffixes"),
    ],
)
def test_historical_uses_line_scanner_holes_reject_floating_pin(
    tmp_path: Path, label: str, step: str
) -> None:
    # The line-scanning predecessor got every spelling in this table wrong.
    root = write_repository(tmp_path, {"probe.yml": workflow(step)})
    _audited, _seen, errors = audit(root)
    assert_rejected(errors, "floating channel")


def test_expression_ref_is_rejected_not_assumed_pinned(tmp_path: Path) -> None:
    # The line scanner could collect this but could not prove the matrix value is safe.
    root = write_repository(
        tmp_path, {"probe.yml": workflow(f"      - uses: {ACTION}@${{{{ matrix.toolchain }}}}")}
    )
    _audited, _seen, errors = audit(root)
    assert_rejected(errors, "run-time ref cannot be shown pinned")


def test_flow_mapping_spanning_lines_is_read_structurally(tmp_path: Path) -> None:
    # The line scanner counted braces and mishandled multi-line flow mappings.
    step = f"""      - uses: {ACTION}@{SHA}
        with: {{
          toolchain: 1.88.0,
          components: rustfmt
        }}"""
    root = write_repository(tmp_path, {"release.yml": release(step)})
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert errors == []


def test_folded_msrv_scalar_with_blank_line_is_read_as_yaml(tmp_path: Path) -> None:
    # The line scanner lost track of blank lines inside a block scalar.
    step = f"""      - uses: {ACTION}@{SHA}
        with:
          toolchain: >-
            1.88.0

            """
    root = write_repository(tmp_path, {"release.yml": release(step)})
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert errors == []


def test_env_toolchain_does_not_satisfy_action_input_contract(tmp_path: Path) -> None:
    # The line scanner accepted env: as though it were the action's with: input.
    step = f"""      - uses: {ACTION}@{SHA}
        env:
          toolchain: 1.88.0"""
    root = write_repository(tmp_path, {"release.yml": release(step)})
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert_rejected(errors, "direct `with: toolchain:`")


def test_nested_with_toolchain_does_not_satisfy_action_input_contract(tmp_path: Path) -> None:
    # The line scanner mistook with.extra.toolchain for the action input.
    step = f"""      - uses: {ACTION}@{SHA}
        with:
          extra:
            toolchain: 1.88.0"""
    root = write_repository(tmp_path, {"release.yml": release(step)})
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert_rejected(errors, "direct `with: toolchain:`")


def test_duplicate_uses_key_fails_closed_with_its_source_line(tmp_path: Path) -> None:
    # The line scanner chose one of two uses: keys; YAML must reject the ambiguity.
    step = f"""      - uses: {ACTION}@1.98.0
        uses: {ACTION}@stable"""
    root = write_repository(tmp_path, {"probe.yml": workflow(step)})
    _audited, _seen, errors = audit(root)
    assert_rejected(errors, "invalid YAML")
    assert any("duplicate key 'uses'" in error and ":8:" in error for error in errors)


def test_duplicate_with_key_fails_closed(tmp_path: Path) -> None:
    # The line scanner could inspect the first with: while Actions used the last.
    step = f"""      - uses: {ACTION}@{SHA}
        with: {{toolchain: 1.88.0}}
        with: {{toolchain: 1.88.0}}"""
    root = write_repository(tmp_path, {"release.yml": release(step)})
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert_rejected(errors, "duplicate key 'with'")


def test_invalid_yaml_cannot_hide_a_toolchain_reference(tmp_path: Path) -> None:
    # The line scanner could skip malformed YAML and report a falsely green audit.
    bad = workflow(f"      - uses: {ACTION}@stable\n        with: [")
    root = write_repository(tmp_path, {"probe.yml": bad})
    _audited, _seen, errors = audit(root)
    assert_rejected(errors, "invalid YAML")


def test_declared_msrv_path_without_step_is_rejected(tmp_path: Path) -> None:
    # The predecessor's aggregate count let a declared MSRV path disappear.
    root = write_repository(tmp_path, {"release.yml": workflow("      - run: true")})
    _audited, _seen, errors = audit(
        root, msrv_paths={".github/workflows/release.yml": "release MSRV"}
    )
    assert_rejected(errors, "contains no dtolnay/rust-toolchain step")


def test_floor_rejects_an_audit_that_lost_its_inputs(tmp_path: Path) -> None:
    # The predecessor could report success after collecting no references at all.
    root = write_repository(tmp_path, {"probe.yml": workflow("      - run: true")})
    _audited, _seen, errors = audit(root, minimum=11)
    assert_rejected(errors, "audited only 0 development toolchain reference")
