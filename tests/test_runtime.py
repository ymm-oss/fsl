"""FSL v2.0 runtime monitor, replay, and testgen tests (DESIGN-bridge §6)."""
import sys
import ast
import copy
import json
import subprocess
import tempfile
from pathlib import Path

import pytest

from fslc import parse, build_spec, verify, scenarios, FslError, Monitor
from fslc.cli import run_replay, run_testgen, exit_code
from fslc.testgen import generate_test_file

ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"
PY = sys.executable

SAMPLE_SPECS = [
    "cart_v1.fsl",
    "order_workflow.fsl",
    "auth_lockout.fsl",
    "inventory_reservation.fsl",
    "payment.fsl",
    "rate_limiter.fsl",
    "mutex_queue.fsl",
    "job_pipeline.fsl",
    "audit_log.fsl",
    "cart_fixed.fsl",
]


def _run_verify(name, depth=8):
    src = (SPECS / name).read_text(encoding="utf-8")
    return verify(build_spec(parse(src)), depth, source_lines=src.splitlines())


def _run_scenarios(name, depth=8):
    src = (SPECS / name).read_text(encoding="utf-8")
    return scenarios(build_spec(parse(src)), depth, source_lines=src.splitlines())


def _replay_trace(mon, trace):
    """Replay witness or scenario trace entries; return list of mismatches."""
    mismatches = []
    mon.reset()
    for i, entry in enumerate(trace):
        if i == 0:
            expected = entry["state"]
            if mon.state != expected:
                mismatches.append((i, "initial", mon.state, expected))
            continue
        if "action" not in entry:
            expected = entry["state"]
            if mon.state != expected:
                mismatches.append((i, "no-action", mon.state, expected))
            continue
        act = entry["action"]
        result = mon.step(act["name"], act["params"])
        if not result["ok"]:
            mismatches.append((i, "step-failed", result, entry))
            continue
        expected = entry["state"]
        if mon.state != expected:
            mismatches.append((i, act["name"], mon.state, expected))
    return mismatches


def _replay_scenario_steps(mon, steps, expected_states):
    mismatches = []
    mon.reset()
    for i, (step, expected) in enumerate(zip(steps, expected_states)):
        result = mon.step(step["action"], step["params"])
        if not result["ok"]:
            mismatches.append((i, "step-failed", result))
            continue
        if mon.state != expected:
            mismatches.append((i, step["action"], mon.state, expected))
    return mismatches


@pytest.mark.parametrize("spec_name", SAMPLE_SPECS)
def test_differential_witness_and_scenarios_match_monitor(spec_name):
    """§6.1: Z3 witness traces and scenario steps match concrete Monitor replay."""
    vr = _run_verify(spec_name)
    assert vr["result"] == "verified", f"{spec_name}: {vr.get('result')}"

    mon = Monitor(str(SPECS / spec_name))
    all_mismatches = []

    for rname, rdata in vr.get("reachables", {}).items():
        witness = rdata["witness"]
        mm = _replay_trace(mon, witness)
        if mm:
            all_mismatches.append(("witness", rname, mm))

    sc = _run_scenarios(spec_name)
    assert sc["result"] == "scenarios"
    for scen in sc["scenarios"]:
        mm = _replay_scenario_steps(mon, scen["steps"], scen["expected_states"])
        if mm:
            all_mismatches.append(("scenario", scen["name"], mm))

    assert not all_mismatches, (
        f"{spec_name}: Monitor/BMC semantic mismatch — likely runtime bug if witness "
        f"was correct: {all_mismatches[:3]}"
    )


def test_requires_failed_guard_unchanged_state():
    mon = Monitor(str(SPECS / "cart_v1.fsl"))
    mon.reset()
    mon.step("add_to_cart", {"u": 0, "i": 0})
    before = copy.deepcopy(mon.state)
    r = mon.step("add_to_cart", {"u": 0, "i": 1})
    assert r["ok"] is False
    assert r["kind"] == "requires_failed"
    assert mon.state == before


def test_ensures_violation_kind():
    src = """
spec EnsuresBug {
  state { x: Int }
  init { x = 0 }
  action bad() {
    x = x + 2
    ensures x == old(x) + 1
  }
}
"""
    vr = verify(build_spec(parse(src)), 4)
    assert vr["result"] == "violated"
    assert vr["violation_kind"] == "ensures"
    trace = vr["trace"]
    last = trace[-1]
    mon = Monitor(build_spec(parse(src)))
    mon.reset()
    for entry in trace[1:-1]:
        mon.step(entry["action"]["name"], entry["action"]["params"])
    r = mon.step(last["action"]["name"], last["action"]["params"])
    assert r["ok"] is False
    assert r["kind"] == "ensures"


def test_type_bound_violation_kind():
    vr = _run_verify("cart_v1_buggy.fsl")
    assert vr["result"] == "violated"
    assert vr["violation_kind"] == "type_bound"
    trace = vr["trace"]
    last = trace[-1]
    mon = Monitor(str(SPECS / "cart_v1_buggy.fsl"))
    mon.reset()
    for entry in trace[1:-1]:
        mon.step(entry["action"]["name"], entry["action"]["params"])
    r = mon.step(last["action"]["name"], last["action"]["params"])
    assert r["ok"] is False
    assert r["kind"] == "type_bound"


def test_invariant_violation_kind():
    src = """
spec InvBug {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant Pos { x >= 0 }
  invariant Max { x <= 0 }
}
"""
    vr = verify(build_spec(parse(src)), 4)
    assert vr["result"] == "violated"
    assert vr["violation_kind"] == "invariant"
    trace = vr["trace"]
    last = trace[-1]
    mon = Monitor(build_spec(parse(src)))
    mon.reset()
    for entry in trace[1:-1]:
        mon.step(entry["action"]["name"], entry["action"]["params"])
    r = mon.step(last["action"]["name"], last["action"]["params"])
    assert r["ok"] is False
    assert r["kind"] == "invariant"


def test_partial_op_violation_kind():
    src = """
spec PartialUnguarded {
  type JobId = 0..1
  state { queue: Seq<JobId, 2> }
  init { queue = Seq {} }
  action bad_pop() {
    queue = queue.pop()
  }
}
"""
    vr = verify(build_spec(parse(src)), 2)
    assert vr["result"] == "violated"
    assert vr["violation_kind"] == "partial_op"
    trace = vr["trace"]
    last = trace[-1]
    mon = Monitor(build_spec(parse(src)))
    mon.reset()
    r = mon.step(last["action"]["name"], last["action"]["params"])
    assert r["ok"] is False
    assert r["kind"] == "partial_op"
    assert r["name"] == "_partial_bad_pop"


def test_enabled_short_circuits_requires_before_guarded_let():
    mon = Monitor(str(SPECS / "job_pipeline.fsl"))
    mon.reset()
    initial_enabled = mon.enabled()
    assert {entry["action"] for entry in initial_enabled} == {"submit"}

    submit = mon.step("submit", {"j": 0})
    assert submit["ok"] is True
    enabled_after_submit = mon.enabled()
    assert "start" in {entry["action"] for entry in enabled_after_submit}


def test_nondeterministic_init_raises_semantics():
    src = """
spec NoInit {
  state { x: Int, y: Int }
  init { x = 0 }
  action noop() { }
  invariant I { true }
}
"""
    with pytest.raises(FslError) as exc:
        Monitor(build_spec(parse(src)))
    assert exc.value.kind == "semantics"
    assert "deterministic init" in (exc.value.hint or "")


def test_monitor_missing_fsl_path_raises_io():
    with pytest.raises(FslError) as exc:
        Monitor("specs/nonexistent.fsl")
    assert exc.value.kind == "io"
    assert str(exc.value) == "file not found: specs/nonexistent.fsl"


def test_monitor_accepts_direct_fsl_source_string():
    src = """
spec DirectSource {
  state { x: Int }
  init { x = 0 }
  action inc() { x = x + 1 }
  invariant NonNegative { x >= 0 }
}
"""
    mon = Monitor(src)
    assert mon.state == {"x": 0}


def test_replay_conformant_and_nonconformant():
    vr = _run_verify("cart_v1.fsl")
    witness = vr["reachables"]["SoldOut"]["witness"]
    events = [
        {"action": e["action"]["name"], "params": e["action"]["params"]}
        for e in witness[1:]
    ]
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump({"events": events}, f)
        trace_path = f.name
    try:
        good = run_replay(str(SPECS / "cart_v1.fsl"), trace_path)
        assert good["result"] == "conformant"
        assert good["steps_checked"] == len(events)
        assert exit_code(good) == 0

        bad_events = list(events)
        bad_events.append({"action": "checkout", "params": {"u": 0}})
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as bf:
            json.dump(bad_events, bf)
            bad_path = bf.name
        try:
            bad = run_replay(str(SPECS / "cart_v1.fsl"), bad_path)
            assert bad["result"] == "nonconformant"
            assert bad["violation"]["kind"] == "requires_failed"
            assert exit_code(bad) == 1
        finally:
            Path(bad_path).unlink(missing_ok=True)

        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as af:
            json.dump(events, af)
            arr_path = af.name
        try:
            arr = run_replay(str(SPECS / "cart_v1.fsl"), arr_path)
            assert arr["result"] == "conformant"
        finally:
            Path(arr_path).unlink(missing_ok=True)
    finally:
        Path(trace_path).unlink(missing_ok=True)


def test_testgen_import_skips_without_adapter():
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "test_gen.py"
        gen = run_testgen(str(SPECS / "cart_v1.fsl"), output=str(out))
        assert gen["result"] == "generated"
        proc = subprocess.run(
            [str(PY), "-m", "pytest", str(out), "-q"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        assert proc.returncode == 0, proc.stdout + proc.stderr
        assert "skipped" in proc.stdout.lower()


def test_testgen_sanitizes_composed_scenario_function_names():
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "test_bank_system.py"
        gen = run_testgen(str(SPECS / "bank_system.fsl"), output=str(out))
        assert gen["result"] == "generated"

        content = out.read_text(encoding="utf-8")
        compile(content, str(out), "exec")
        module = ast.parse(content, filename=str(out))
        test_names = [
            node.name
            for node in module.body
            if isinstance(node, ast.FunctionDef) and node.name.startswith("test_scenario_")
        ]

        assert test_names
        assert all("." not in name for name in test_names)
        assert "test_scenario_reach_bank_Settled" in test_names
        assert "test_scenario_cover_bank_settle" in test_names

        docstrings = {
            node.name: ast.get_docstring(node)
            for node in module.body
            if isinstance(node, ast.FunctionDef) and node.name.startswith("test_scenario_")
        }
        assert docstrings["test_scenario_reach_bank_Settled"] == "Scenario: reach_bank.Settled"
        assert docstrings["test_scenario_cover_bank_settle"] == "Scenario: cover_bank.settle"


def test_testgen_self_conformance_with_monitor_adapter():
    content = generate_test_file(str(SPECS / "cart_v1.fsl"), depth=8)
    adapter_block = '''
class MonitorAdapter:
    def __init__(self):
        self._mon = Monitor(SPEC_PATH)

    def reset(self):
        self._mon.reset()

    def step(self, action, params):
        r = self._mon.step(action, params)
        assert r.get("ok"), r

    def observe(self):
        return self._mon.state

@pytest.fixture
def adapter():
    return MonitorAdapter()
'''
    content = content.replace(
        "@pytest.fixture\ndef adapter():\n    return Adapter()",
        adapter_block.strip(),
    )
    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "test_self.py"
        out.write_text(content, encoding="utf-8")
        proc = subprocess.run(
            [str(PY), "-m", "pytest", str(out), "-q"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        assert proc.returncode == 0, proc.stdout + proc.stderr
        assert "failed" not in proc.stdout.lower()


def test_enabled_matches_guarded_instances():
    mon = Monitor(str(SPECS / "cart_v1.fsl"))
    mon.reset()
    mon.step("add_to_cart", {"u": 0, "i": 0})
    enabled = mon.enabled()
    names = {(e["action"], tuple(sorted(e["params"].items()))) for e in enabled}
    assert ("remove_from_cart", (("u", 0),)) in names
    assert ("checkout", (("u", 0),)) in names
    assert all(e["action"] != "add_to_cart" or e["params"]["u"] != 0 for e in enabled)
