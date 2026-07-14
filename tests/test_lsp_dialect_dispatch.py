# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

from fslc.lsp.dialect_dispatch import (
    SUPPORTED_DIALECT_KEYWORDS,
    classify_lsp_dialect,
)
import json
import subprocess
from pathlib import Path

import pytest
from lark.exceptions import UnexpectedInput

from fslc.lsp.index import build_index
from fslc.lsp.server import check_source


ROOT = Path(__file__).resolve().parents[1]
RUST_FSLC = ROOT / "rust" / "target" / "debug" / "fslc"


def test_lsp_registry_adapter_has_native_key_order_and_significant_token_rules():
    assert SUPPORTED_DIALECT_KEYWORDS == (
        "spec",
        "refinement",
        "compose",
        "business",
        "governance",
        "requirements",
        "domain",
        "dbsystem",
        "ai_component",
        "agent",
    )
    source = '\ufeff// leading\n@routing(spec, domain, "ai_component") business Demo {}'
    dispatch = classify_lsp_dialect(source)
    assert dispatch is not None
    assert dispatch.keyword == "business"
    assert source[dispatch.declaration_offset:].startswith("business")


def test_lsp_indexes_an_annotated_document_at_original_locations():
    source = '\ufeff// leading\n@owner(team.platform)\nspec Demo {}'
    index = build_index(source, "annotated.fsl")
    demo = next(symbol for symbol in index.symbols if symbol.name == "Demo")
    assert demo.selection_range.start.line == 2
    assert demo.selection_range.start.character == 5


def test_lsp_diagnostics_use_dispatch_for_annotations_comments_and_agents():
    annotated = check_source(
        '@requirement("REQ")\nspec Demo { state { value: Int } }', None
    )
    assert annotated["result"] == "ok"
    assert annotated["spec"] == "Demo"

    domain = check_source(
        "// lead\ndomain Demo { enum Status { Ready } "
        "aggregate Item { id ItemId state { status: Status = Ready; } } }",
        None,
    )
    assert domain["result"] == "ok"
    assert domain["spec"] == "Demo"

    agent = check_source('// lead\nagent Worker {}', None)
    assert agent["result"] == "ok"
    assert agent["spec"] == "Worker"
    assert agent["dialect"] == "fsl-ai-agent.v0"


@pytest.mark.parametrize("source", ["@foo. spec Demo {}", "@foo..bar() spec Demo {}"])
def test_lsp_rejects_malformed_annotation_paths(source):
    assert classify_lsp_dialect(source) is None
    with pytest.raises(UnexpectedInput):
        build_index(source)


def test_lsp_registry_keys_match_the_native_diagnostic_contract(tmp_path):
    if not RUST_FSLC.exists():
        subprocess.run(
            ["cargo", "build", "--quiet", "--locked", "-p", "fslc-rust"],
            cwd=ROOT / "rust",
            check=True,
        )
    source = tmp_path / "unknown.fsl"
    source.write_text("unknown Demo {}", encoding="utf-8")
    completed = subprocess.run(
        [str(RUST_FSLC), "check", str(source)],
        check=False,
        capture_output=True,
        text=True,
    )
    assert completed.returncode == 2
    output = json.loads(completed.stdout)
    assert tuple(output["supported_dialects"]) == SUPPORTED_DIALECT_KEYWORDS
