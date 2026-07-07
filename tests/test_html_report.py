import json
import subprocess
import sys
from pathlib import Path

from fslc.cli import exit_code, run_html
from fslc.html_report import render_html_report


ROOT = Path(__file__).resolve().parents[1]
SPECS = ROOT / "specs"


def test_spec_level_kind_tag_renders_title_badge():
    ui = ROOT / "examples" / "ui_spike" / "return_ui.fsl"
    result = run_html(str(ui), depth=3, write_file=False)

    assert result["result"] == "generated"
    html = result["content"]
    assert '<span class="badge neutral kind"' in html
    assert 'title="return-request screen flow (behavioral slice only)"' in html
    assert ">ui</span>" in html


def test_run_html_generates_self_contained_report():
    result = run_html(str(SPECS / "cart_v1.fsl"), depth=4, write_file=False)

    assert result["result"] == "generated"
    assert result["kind"] == "html_report"
    assert result["spec"] == "ShoppingCart"
    assert exit_code(result) == 0

    html = result["content"]
    assert html.startswith("<!doctype html>")
    assert "<title>ShoppingCart - FSL Specification Report</title>" in html
    assert "State Model" in html
    assert "Actions" in html
    assert "Counterfactuals" in html
    assert "requires cart[u] == none" in html
    assert "Action to state write graph" in html
    assert "<script src=" not in html


def test_html_report_escapes_source_and_payload_text(tmp_path):
    path = tmp_path / "escape.fsl"
    path.write_text(
        r'''// <script>alert("source")</script>
spec Escape {
  type Id = 0..1
  state { x: Id }
  init { x = 0 }
  action inc() {
    requires x < 1
    x = 1
  }
  invariant Safe "<b>not html</b>" { x <= 1 }
}
''',
        encoding="utf-8",
    )

    result = run_html(str(path), depth=1, write_file=False)
    assert result["result"] == "generated"
    html = result["content"]

    assert "<script>alert" not in html
    assert "&lt;script&gt;alert" in html
    assert "<b>not html</b>" not in html
    assert "&lt;b&gt;not html&lt;/b&gt;" in html


def test_html_report_includes_violation_trace_for_buggy_spec():
    result = run_html(str(SPECS / "cart_v1_buggy.fsl"), depth=4, write_file=False)

    assert result["result"] == "generated"
    html = result["content"]
    assert '<span class="badge bad">violated</span>' in html
    assert "Trace Review" in html
    assert "-> -1" in html
    assert "type_bound" in html


def test_html_report_surfaces_temporal_sugar_and_within(tmp_path):
    path = tmp_path / "temporal_sugar.fsl"
    path.write_text(
        r"""
spec TemporalSugar {
  type Step = 0..3
  state { x: Step, held: Bool, released: Bool }
  init { x = 0  held = true  released = false }
  action inc() {
    requires x < 3
    x = x + 1
  }
  action drop() { held = false }
  action release() { released = true  held = false }
  leadsTo TooFast { x == 1 ~> within 1 x == 3 }
  unless HeldUnlessReleased { held unless released }
  until HeldUntilReleased { held until released }
}
""",
        encoding="utf-8",
    )

    result = run_html(str(path), depth=3, write_file=False)

    assert result["result"] == "generated"
    html = result["content"]
    assert "within 1" in html
    assert "deadline=" in html or "Deadline" in html
    assert "trans" in html
    assert "unless HeldUnlessReleased" in html
    assert "until HeldUntilReleased" in html


def test_html_report_renders_relation_graph_from_trace(tmp_path):
    path = tmp_path / "relation_html.fsl"
    path.write_text(
        r'''spec RelationHtml {
  type User = 0..1
  state { delegates: relation User -> User }
  init { delegates = Set {} }
  action delegate(a: User, b: User) {
    requires a != b
    delegates = delegates.add(a, b)
  }
  reachable CanDelegate { delegates.contains(0, 1) }
}
''',
        encoding="utf-8",
    )

    result = run_html(str(path), depth=1, write_file=False)
    assert result["result"] == "generated"
    html = result["content"]
    assert "relation 0..1 -&gt; 0..1" in html
    assert "Relation Graphs" in html
    assert "delegates" in html
    assert "0 -&gt; 1" in html


def test_html_report_renders_refinement_failure_side_by_side():
    html = render_html_report(
        "impl.fsl",
        "spec Impl {}",
        {"result": "explained", "spec": "Impl", "skeleton": {"state": {}, "actions": [], "properties": []}},
        {
            "result": "verified",
            "spec": "Impl",
            "implements": {
                "abs": "Abs",
                "result": "refinement_failed",
                "violation": {
                    "result": "refinement_failed",
                    "impl": "Impl",
                    "abs": "Abs",
                    "kind": "abs_requires_failed",
                    "impl_action": {"name": "fast_pay"},
                    "abs_action": {"name": "pay"},
                    "impl_state": {"paid": True},
                    "alpha_before": {"done": False},
                    "alpha_after_expected": {"done": True},
                    "alpha_after_actual": {"done": False},
                    "mismatch": [{"path": "done"}],
                },
            },
        },
    )

    assert "Refinement Evidence" in html
    assert "Implementation Side" in html
    assert "Abstract Side" in html
    assert "fast_pay" in html
    assert "fast_pay -&gt;" not in html  # rendered inside escaped JSON, not as raw HTML
    assert "<script" not in html


def test_html_cli_stdout_and_output_file(tmp_path):
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "fslc",
            "html",
            str(SPECS / "cart_v1.fsl"),
            "--depth",
            "4",
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.returncode == 0, proc.stderr
    assert proc.stdout.startswith("<!doctype html>")
    assert "ShoppingCart" in proc.stdout

    out = tmp_path / "cart_report.html"
    proc = subprocess.run(
        [
            sys.executable,
            "-m",
            "fslc",
            "html",
            str(SPECS / "cart_v1.fsl"),
            "--depth",
            "4",
            "-o",
            str(out),
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.returncode == 0, proc.stderr
    payload = json.loads(proc.stdout)
    assert payload["result"] == "generated"
    assert payload["kind"] == "html_report"
    assert payload["output"] == str(out)
    assert "content" not in payload
    assert out.read_text(encoding="utf-8").startswith("<!doctype html>")
