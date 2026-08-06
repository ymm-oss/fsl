# SPDX-License-Identifier: Apache-2.0
"""Contract tests for the opt-in Referance domain probe manifests."""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFESTS = (
    ROOT / "tools/referance/domain-generate-probe.json",
    ROOT / "tools/referance/domain-generate-mismatch-control.json",
)
REQUIRED_SOURCE_ARTIFACTS = {
    "docs/DESIGN-domain.md",
    "examples/domain/order_functional_ddd.fsl",
    "src/fslc/cli.py",
    "src/fslc/domain_check.py",
    "src/fslc/domain_expand.py",
    "src/fslc/domain_codegen/simple.py",
    "src/fslc/domain_ir.py",
    "src/fslc/domain_parser.py",
    "tests/test_rust_cli_semantics.py",
    "tools/referance/domain-probe-adapter.py",
}
REQUIRED_CANDIDATE_ARTIFACTS = {
    "docs/DESIGN-domain.md",
    "docs/DESIGN-kernel-contract.md",
    "rust/Cargo.lock",
    "rust/Cargo.toml",
    "rust/fsl-core/Cargo.toml",
    "rust/fsl-core/src/domain.rs",
    "rust/fsl-core/src/domain_lowering.rs",
    "rust/fsl-core/src/public_kernel.rs",
    "rust/fsl-syntax/src/domain.rs",
    "rust/fsl-tools/src/domain.rs",
    "rust/fsl-tools/src/domain_codegen.rs",
    "rust/fsl-tools/src/domain_naming.rs",
    "rust/fslc/Cargo.toml",
    "rust/fslc/build.rs",
    "rust/fslc/src/main.rs",
    "tools/referance/domain-probe-adapter.py",
}


def _load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def _missing_artifacts(manifest: dict) -> dict[str, set[str]]:
    return {
        "source": REQUIRED_SOURCE_ARTIFACTS - set(manifest["source"]["artifacts"]),
        "candidate": REQUIRED_CANDIDATE_ARTIFACTS - set(manifest["candidate"]["artifacts"]),
    }


def test_domain_probe_manifests_bind_behavior_owners_and_contracts() -> None:
    for path in MANIFESTS:
        missing = _missing_artifacts(_load(path))
        assert missing == {"source": set(), "candidate": set()}, (path, missing)


def test_artifact_completeness_control_rejects_an_omitted_behavior_owner() -> None:
    manifest = copy.deepcopy(_load(MANIFESTS[0]))
    manifest["candidate"]["artifacts"].remove("rust/fsl-tools/src/domain.rs")
    assert _missing_artifacts(manifest)["candidate"] == {"rust/fsl-tools/src/domain.rs"}


def test_adapter_retains_unknown_envelope_fields_without_projection() -> None:
    path = ROOT / "tools/referance/domain-probe-adapter.py"
    spec = importlib.util.spec_from_file_location("domain_probe_adapter", path)
    assert spec is not None and spec.loader is not None
    adapter = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(adapter)
    envelope = {"result": "generated", "future_contract_field": {"nested": [1, 2, 3]}}
    encoded = adapter._encode_stdout(json.dumps(envelope))
    assert encoded == {"format": "json", "value": envelope}
