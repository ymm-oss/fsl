import json
import subprocess
import sys
from pathlib import Path

from fslc.cli import exit_code, run_html


ROOT = Path(__file__).resolve().parents[1]
SPECS = ROOT / "specs"


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
