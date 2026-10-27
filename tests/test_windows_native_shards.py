# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita
"""Acceptance controls for the Windows native-Z3 shard split (#1532)."""
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
NAMES = "tools/windows-native-long-poles.txt"
TESTS = "--test fsl5_glossary --test issue_1442_compose_binding_capture_matrix"
A = "k_glossary_sources_lower_to_the_canonical_kernel"
B = "all_compose_and_sync_binding_sources_preserve_alpha_equivalence"
LIST = (f"     Running tests/fsl5_glossary.rs (fsl5_glossary-1234.exe)\n{A}: test\n"
        f"\n1 test, 0 benchmarks\n     Running tests/issue_1442_compose_binding_capture_matrix.rs "
        f"(issue_1442_compose_binding_capture_matrix-5678.exe)\n{B}: test\n\n1 test, 0 benchmarks\n")
CONDITION = ("(github.event_name != 'pull_request' && github.event_name != 'merge_group') || "
             "github.base_ref == 'production' || vars.FSL_OPTIMISTIC_CI != 'enabled'")


def _check(names, code, diagnostic=""):
    tool = "tools/check_windows_long_poles.py"
    assert (ROOT / tool).is_file(), f"missing {tool}"
    with tempfile.TemporaryDirectory() as directory:
        listing, manifest = Path(directory) / "list.txt", Path(directory) / "names.txt"
        listing.write_text(LIST, encoding="utf-8")
        manifest.write_text(names, encoding="utf-8")
        result = subprocess.run([sys.executable, tool, "--list-output", str(listing), "--names",
                                 str(manifest)], cwd=ROOT, text=True, capture_output=True)
    output = result.stdout + result.stderr
    assert result.returncode == code, (result.returncode, code, output)
    assert diagnostic in output, (diagnostic, output)


def test_checker_exact_match():
    _check(f"# long-pole names\n\n{A}\n{B}\n", 0)
    _check(f"  # long-pole names\n\n  {A}  \n\n{B}", 0)


def test_checker_emit_skip_args():
    names = [line for line in (ROOT / NAMES).read_text(encoding="utf-8").splitlines() if line and not line.startswith("#")]
    with tempfile.TemporaryDirectory() as directory:
        manifest = Path(directory) / "names.txt"
        manifest.write_text("  # names\n\n" + "\n\n".join(f"  {name}  " for name in names).rstrip(), encoding="utf-8")
        result = subprocess.run([sys.executable, "tools/check_windows_long_poles.py", "--names", str(manifest),
                                 "--emit-skip-args"], cwd=ROOT, text=True, capture_output=True)
    assert result.returncode == 0, result.stderr
    assert result.stdout == "".join(f"--skip\n{name}\n" for name in names), result.stdout


def test_checker_stale_name():
    _check(f"{A}\n{B}\nremoved_long_pole_test\n", 1, "removed_long_pole_test")


def test_checker_outside_name():
    _check(f"{A}\n{B}\ntrace_verdicts_do_not_depend_on_the_values_scope\n", 1, "trace_verdicts_do_not_depend_on_the_values_scope")


def test_checker_missing_name():
    _check(f"{A}\n", 1, B)


def test_checker_duplicate_name():
    _check(f"{A}\n{B}\n{A}\n", 1, A)


def test_aggregator_results():
    tool = "tools/require-shard-success.sh"
    assert (ROOT / tool).is_file(), f"missing {tool}"
    for value in ("success", "failure", "cancelled", "skipped", "", "unknown", "SUCCESS", "success\n"):
        result = subprocess.run(["bash", tool, value], cwd=ROOT, text=True, capture_output=True)
        assert result.returncode == (0 if value == "success" else 1), (value, result)
        if value != "success":
            assert result.stdout + result.stderr and value in result.stdout + result.stderr, (value, result)


def _job(text, name):
    match = re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:|\Z)", text, re.M | re.S)
    assert match, f"missing job {name}"
    return match[1]


def _step(job, name):
    return re.search(rf"^      - name: {re.escape(name)}\n(.*?)(?=^      - |\Z)", job, re.M | re.S)[1]


def test_workflow_structure():
    text = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    shards = _job(text, "rust-native-z3-windows-shards")
    gate = _job(text, "rust-native-z3-windows")
    assert "    name: native Z3 4.16 (windows-latest)\n" in gate
    assert "    needs: rust-native-z3-windows-shards\n" in gate
    for job, expected in ((shards, CONDITION), (gate, f"always() && ({CONDITION})")):
        guard = re.search(r"^    if: (.*(?:\n      \S.*)*)", job, re.M)[1].removeprefix(">-")
        assert " ".join(guard.split()) == expected
    assert "    timeout-minutes: 10\n" in gate
    aggregate = re.search(r"^      - name: .+\n(.*?)(?=^      - |\Z)", gate, re.M | re.S)[1]
    assert '        run: bash tools/require-shard-success.sh "$SHARDS_RESULT"\n' in aggregate
    assert "        env:\n          SHARDS_RESULT: ${{ needs.rust-native-z3-windows-shards.result }}\n" in aggregate
    assert "    runs-on: ubuntu-latest\n" in gate
    for required in ("fail-fast: false", "part: [long-poles, rest]", "timeout-minutes: 90",
                     "name: native Z3 4.16 (windows-latest, ${{ matrix.part }})",
                     "cargo test --locked --no-fail-fast -p fsl-solver-z3 -p fsl-verifier -p fslc-rust", TESTS):
        assert required in shards, required
    assert shards.index(f"{TESTS} -- --list") < shards.index("tools/check_windows_long_poles.py")
    assert TESTS in shards[shards.index("tools/check_windows_long_poles.py"):]
    assert "--list-output" in shards and "--names" in shards
    poles = _step(shards, "Check live long-pole names and run both binaries")
    rest = _step(shards, "Test pinned native solver and BMC crates except long poles")
    for step, part in ((poles, "long-poles"), (rest, "rest")):
        assert f"        if: matrix.part == '{part}'\n" in step
        assert "        shell: bash\n        working-directory: rust\n" in step
    assert 'cargo test --locked --no-fail-fast -p fsl-solver-z3 -p fsl-verifier -p fslc-rust -- --exact "${skips[@]}"\n' in rest
    assert f"mapfile -t skips < <(python ../tools/check_windows_long_poles.py --names ../{NAMES} --emit-skip-args)\n" in rest
    assert "      - rust-native-z3-windows\n" in _job(text, "product-gate")
    try:
        import yaml
    except ImportError:
        return
    jobs = yaml.safe_load(text)["jobs"]
    assert jobs["rust-native-z3-windows-shards"]["strategy"] == {
        "fail-fast": False, "matrix": {"part": ["long-poles", "rest"]}}


def test_real_names():
    names = [line.strip() for line in (ROOT / NAMES).read_text(encoding="utf-8").splitlines()
             if line.strip() and not line.lstrip().startswith("#")]
    assert len(names) == len(set(names)) == 12, names
    source = "\n".join((ROOT / f"rust/fslc/tests/{binary}.rs").read_text(encoding="utf-8")
                       for binary in ("fsl5_glossary", "issue_1442_compose_binding_capture_matrix"))
    assert set(names) <= set(re.findall(r"^fn (\w+)\(", source, re.M)), names
    long_poles = {ROOT / f"rust/fslc/tests/{binary}.rs" for binary in ("fsl5_glossary", "issue_1442_compose_binding_capture_matrix")}
    for crate in ("fsl-solver-z3", "fsl-verifier", "fslc"):
        for path in (ROOT / "rust" / crate).rglob("*.rs"):
            if path not in long_poles:
                assert not re.search(r"\bfn\s+(?:" + "|".join(map(re.escape, names)) + r")\s*\(", path.read_text(encoding="utf-8")), path


if __name__ == "__main__":
    failures = 0
    for name, test in sorted(list(globals().items())):
        if name.startswith("test_") and callable(test):
            try:
                test()
                print(f"PASS {name}")
            except Exception as error:
                failures += 1
                print(f"FAIL {name}: {type(error).__name__}: {error}")
    sys.exit(bool(failures))
