# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Calibration controls for the VS Code release-asset documentation audit."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / ".github" / "scripts" / "validate_vscode_vsix_name.py"
SPEC = importlib.util.spec_from_file_location("validate_vscode_vsix_name", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validator
SPEC.loader.exec_module(validator)

GOOD_DOCS = {
    "editors/vscode/README.md": "code --install-extension fsl-vscode.vsix",
    "README.md": "fsl-vscode.vsix",
}


def test_committed_repository_documents_the_published_name() -> None:
    assert validator.validate(
        validator.RELEASE.read_text(encoding="utf-8"),
        {path.relative_to(validator.ROOT).as_posix(): path.read_text(encoding="utf-8")
         for path in validator.DOCUMENTS},
    ) == []


def test_renamed_release_output_is_rejected() -> None:
    errors = validator.validate(
        "      - name: Build .vsix\n        run: |\n          npm exec -- vsce package --out renamed.vsix\n",
        GOOD_DOCS,
    )
    assert errors == [
        "editors/vscode/README.md: missing published VSIX name 'renamed.vsix'",
        "README.md: missing published VSIX name 'renamed.vsix'",
    ]


def test_versioned_documentation_is_rejected() -> None:
    errors = validator.validate(
        "      - name: Build .vsix\n        run: |\n          vsce package --out fsl-vscode.vsix\n",
        {**GOOD_DOCS, "editors/vscode/README.md": "fsl-vscode-<version>.vsix"},
    )
    assert "editors/vscode/README.md: contains a versioned VSIX name" in errors
