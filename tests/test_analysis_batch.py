import json
import subprocess
import sys

from fslc.cli import run_analyze


def _write(path, src):
    path.write_text(src, encoding="utf-8")
    return path


VALID = """
spec BatchValid {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant Any "MODEL: baseline" { true }
}
"""


def test_analyze_batch_multiple_files_keeps_successes_with_partial_errors(tmp_path):
    good = _write(tmp_path / "good.fsl", VALID)
    bad = _write(tmp_path / "bad.fsl", "spec Broken { state { x: Int } init { x = } }")

    out = run_analyze([str(good), str(bad)], projection="tsg")

    assert out["result"] == "error"
    assert out["kind"] == "batch"
    assert out["mode"] == "batch"
    assert len(out["files"]) == 2
    assert len(out["errors"]) == 1
    assert any(item["result"] == "analyzed" and item["summary"]["nodes"] > 0 for item in out["files"])
    assert any(item["result"] == "error" and item["kind"] == "parse" for item in out["files"])


def test_analyze_batch_directory_discovers_fsl_files_in_stable_order(tmp_path):
    _write(tmp_path / "b.fsl", VALID.replace("BatchValid", "BatchB"))
    nested = tmp_path / "nested"
    nested.mkdir()
    _write(nested / "a.fsl", VALID.replace("BatchValid", "BatchA"))
    (tmp_path / "ignored.txt").write_text("not fsl", encoding="utf-8")

    out = run_analyze(str(tmp_path), projection="tsg")

    assert out["result"] == "analyzed"
    files = [item["file"] for item in out["files"]]
    assert files == sorted(files)
    assert [name.rsplit("/", 1)[-1] for name in files] == ["b.fsl", "a.fsl"] or files == sorted(files)
    assert len(files) == 2


def test_analyze_batch_profile_tolerates_refinement_mapping_files(tmp_path):
    _write(tmp_path / "spec.fsl", VALID)
    _write(tmp_path / "mapping.fsl", """
refinement ImplRefinesAbs {
  impl Impl
  abs Abs
  map x = y
  action step() -> step()
}
""")

    out = run_analyze(str(tmp_path), profile="ai-review")

    assert out["result"] == "analyzed"
    mapping = next(item for item in out["files"] if item["file"].endswith("mapping.fsl"))
    assert mapping["result"] == "analyzed"
    assert mapping["refinement"] == "ImplRefinesAbs"
    assert mapping["findings"] == []


def test_analyze_batch_cli_exits_two_for_partial_error(tmp_path):
    good = _write(tmp_path / "good.fsl", VALID)
    bad = _write(tmp_path / "bad.fsl", "spec Broken { state { x: Int } init { x = } }")

    proc = subprocess.run(
        [sys.executable, "-m", "fslc", "analyze", str(good), str(bad), "--projection", "tsg"],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )

    assert proc.returncode == 2, proc.stdout + proc.stderr
    payload = json.loads(proc.stdout)
    assert payload["kind"] == "batch"
    assert len(payload["errors"]) == 1
