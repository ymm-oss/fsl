import subprocess
import sys

from fslc.analysis.export import to_dot, to_mermaid
from fslc.cli import run_analyze


def test_analyze_graph_projection_exports_dot_and_mermaid(tmp_path):
    path = tmp_path / "export.fsl"
    path.write_text("""
spec ExportGraph {
  state { x: Int }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  invariant Any "MODEL: baseline" { true }
}
""", encoding="utf-8")

    dot = run_analyze(str(path), projection="action_state_graph", output_format="dot")
    mermaid = run_analyze(str(path), projection="action_state_graph", output_format="mermaid")

    assert dot["result"] == "analyzed"
    assert dot["format"] == "dot"
    assert dot["content"].startswith("digraph fsl_analysis")
    assert '"action:inc"' in dot["content"]
    assert mermaid["content"].startswith("graph TD")
    assert "action_inc" in mermaid["content"]


def test_analyze_cli_dot_writes_raw_graph(tmp_path):
    path = tmp_path / "export_cli.fsl"
    path.write_text("""
spec ExportCli {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant Any "MODEL: baseline" { true }
}
""", encoding="utf-8")

    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "fslc",
            "analyze",
            str(path),
            "--projection",
            "action_state_graph",
            "--format",
            "dot",
        ],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )

    assert proc.returncode == 0, proc.stdout + proc.stderr
    assert proc.stdout.startswith("digraph fsl_analysis")


def test_graph_export_escapes_labels_and_ids():
    graph = {
        "nodes": [
            {"id": "requirement:REQ-1", "kind": "requirement", "label": "REQ \"quoted\""},
            {"id": "action:submit-order", "kind": "action", "label": "submit order"},
        ],
        "edges": [
            {"id": "e", "kind": "covers|unsafe", "from": "requirement:REQ-1", "to": "action:submit-order"}
        ],
    }

    dot = to_dot(graph)
    mermaid = to_mermaid(graph)

    assert 'REQ \\"quoted\\"' in dot
    assert "requirement_REQ_1" in mermaid
    assert "&quot;quoted&quot;" in mermaid
    assert "covers/unsafe" in mermaid


def test_dot_mermaid_reject_profile_outputs_clearly(tmp_path):
    path = tmp_path / "profile_export.fsl"
    path.write_text("""
spec ProfileExport {
  state { x: Int }
  init { x = 0 }
  action broad() { x = x + 1 }
  invariant Any "MODEL: baseline" { true }
}
""", encoding="utf-8")

    out = run_analyze(str(path), profile="ai-review", output_format="dot")

    assert out["result"] == "error"
    assert out["kind"] == "semantics"
    assert "not profiles" in out["message"]
