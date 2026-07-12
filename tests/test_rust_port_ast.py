# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Phase-0 contracts for the Rust port's Python AST oracle."""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

from fslc.parser import parse_expr

from tools.check_rust_ast_parity import run as run_rust_ast_parity
from tools.export_ast import canonical, corpus_paths, export_corpus, export_expression, export_file
from tools.check_rust_kernel_parity import direct_spec_cases
from tools.inventory_rust_port import inventory
from tools.check_rust_surface_parity import (
    first_difference,
    spec_cases,
    specialized_cases,
    surface_cases,
)


ROOT = Path(__file__).resolve().parents[1]


def test_canonical_preserves_ast_order_but_sorts_mappings():
    value = ("node", {"z": (2,), "a": (1,)}, [3, 4])
    assert canonical(value) == ["node", {"a": [1], "z": [2]}, [3, 4]]


def test_expression_export_is_the_parse_expr_oracle():
    source = "not q.contains(1) => some(2) is some(v)"
    exported = export_expression(source)
    assert exported["schema"] == "fsl-python-ast.v1"
    assert exported["ast"] == canonical(parse_expr(source))


def test_file_export_is_source_relative_and_deterministic():
    path = ROOT / "examples" / "e2e" / "2_requirements.fsl"
    first = export_file(path, root=ROOT)
    second = export_file(path, root=ROOT)
    assert first == second
    assert first["path"] == "examples/e2e/2_requirements.fsl"
    assert first["status"] == "ok"
    assert first["ast"][0] == "spec"


def test_surface_export_stops_before_requirements_lowering():
    path = ROOT / "examples" / "e2e" / "2_requirements.fsl"
    exported = export_file(path, root=ROOT, stage="surface")
    assert exported["status"] == "ok"
    assert exported["stage"] == "surface"
    assert exported["frontend"] == "shared"
    assert exported["ast"][0] == "requirements"


def test_specialized_surface_ir_is_typed_and_explicit():
    path = next((ROOT / "examples" / "db").glob("*.fsl"))
    exported = export_file(path, root=ROOT, stage="surface")
    assert exported["status"] == "ok"
    assert exported["frontend"] == "db"
    assert exported["ast"]["$type"] == "DbSystem"


def test_corpus_export_never_silently_drops_a_fsl_file():
    paths = corpus_paths(ROOT)
    exported = export_corpus(ROOT, paths)
    assert len(exported["files"]) == len(paths)
    assert sum(exported["counts"].values()) == len(paths)
    assert {entry["path"] for entry in exported["files"]} == {
        path.relative_to(ROOT).as_posix() for path in paths
    }
    assert set(exported["counts"]) <= {"ok", "error", "evidence_only"}


def test_ast_export_cli_emits_one_json_document():
    proc = subprocess.run(
        [sys.executable, "tools/export_ast.py", "--expr", "1 + 2 * 3", "--compact"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    assert proc.returncode == 0, proc.stderr
    result = json.loads(proc.stdout)
    assert result["ast"] == ["bin", "+", ["num", 1], ["bin", "*", ["num", 2], ["num", 3]]]


def test_rust_expression_parity_runner_rejects_no_reference_cases(tmp_path):
    fake = tmp_path / "fake-rust-parser.py"
    fake.write_text(
        f"#!{sys.executable}\n"
        "import json, sys\n"
        "from fslc.parser import parse_expr\n"
        "from tools.export_ast import canonical\n"
        "print(json.dumps(canonical(parse_expr(sys.argv[1]))))\n",
        encoding="utf-8",
    )
    cases = tmp_path / "cases.json"
    cases.write_text(json.dumps(["1 + 2", "not false"]), encoding="utf-8")
    result = run_rust_ast_parity(cases, [sys.executable, str(fake)])
    assert result == {
        "schema": "fsl-rust-ast-parity.v1",
        "scope": "expression",
        "cases": 2,
        "matched": 2,
        "failures": [],
    }


def test_rust_port_inventory_accounts_for_every_corpus_file():
    result = inventory(ROOT)
    assert sum(result["statuses"].values()) == result["total_files"]
    assert (
        sum(result["frontends"].values()) + result["statuses"].get("error", 0)
        == result["total_files"]
    )
    assert result["shared_top_levels"]["spec"] >= 1
    assert result["surface_node_tags"]["bin"] >= 1


def test_spec_parity_cases_equal_the_measured_kernel_spec_surface():
    result = inventory(ROOT)
    assert len(spec_cases(ROOT)) == result["shared_top_levels"]["spec"]


def test_direct_kernel_cases_equal_the_measured_kernel_spec_surface():
    result = inventory(ROOT)
    assert len(direct_spec_cases(ROOT)) == result["shared_top_levels"]["spec"]


def test_supported_surface_cases_equal_all_shared_top_level_inventory():
    result = inventory(ROOT)
    assert len(surface_cases(ROOT)) == sum(result["shared_top_levels"].values())


def test_supported_specialized_cases_equal_parseable_specialized_inventory():
    result = inventory(ROOT)
    assert len(specialized_cases(ROOT)) == sum(
        result["frontends"][name] for name in ("ai-component", "db", "domain")
    )


def test_spec_parity_reports_the_first_structural_difference():
    assert first_difference(["spec", "A", []], ["spec", "B", []]) == {
        "json_path": "$[1]",
        "python": "A",
        "rust": "B",
    }
