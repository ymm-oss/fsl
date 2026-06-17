"""FSL v1.1: scenarios generation and coverage diagnosis."""
import sys
import copy
import json
import subprocess
from pathlib import Path

import pytest

from fslc import parse, build_spec, verify, scenarios

SPECS = Path(__file__).resolve().parent.parent / "specs"
ROOT = Path(__file__).resolve().parent.parent
PY = sys.executable


def run_scenarios(name, depth=8, **kwargs):
    src = (SPECS / name).read_text(encoding="utf-8")
    return scenarios(build_spec(parse(src)), depth, source_lines=src.splitlines(), **kwargs)


def cli_scenarios(name, depth=8):
    proc = subprocess.run(
        [str(PY), "-m", "fslc", "scenarios", str(SPECS / name), "--depth", str(depth)],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    return json.loads(proc.stdout), proc.returncode


def _simulate_cart_step(state, action, params):
    state = copy.deepcopy(state)
    u = str(params["u"])
    if action == "add_to_cart":
        i = str(params["i"])
        assert state["cart"][u] is None
        state["cart"][u] = int(i)
    elif action == "remove_from_cart":
        assert state["cart"][u] is not None
        state["cart"][u] = None
    elif action == "checkout":
        item = state["cart"][u]
        assert item is not None
        i = str(item)
        state["stock"][i] -= 1
        state["cart"][u] = None
    else:
        raise ValueError(f"unknown action {action}")
    return state


def test_cart_reach_soldout_simulation_matches_expected_states():
    r = run_scenarios("cart_v1.fsl")
    assert r["result"] == "scenarios"
    reach = next(s for s in r["scenarios"] if s["name"] == "reach_SoldOut")
    state = copy.deepcopy(reach["initial_state"])
    for step, expected in zip(reach["steps"], reach["expected_states"]):
        state = _simulate_cart_step(state, step["action"], step["params"])
        assert state == expected


def test_cart_all_actions_have_cover_scenarios():
    r = run_scenarios("cart_v1.fsl")
    names = {s["name"] for s in r["scenarios"]}
    assert "cover_add_to_cart" in names
    assert "cover_remove_from_cart" in names
    assert "cover_checkout" in names


def test_blocking_requires_on_impossible_action():
    src = """
spec Blocked {
  state { x: Int }
  init { x = 0 }
  action bad() {
    requires x == 0
    requires x == 1
    x = 2
  }
  action ok() { x = 1 }
  invariant I { true }
}
"""
    lines = src.strip().splitlines()
    r = verify(build_spec(parse(src)), 4, source_lines=lines)
    assert r["result"] == "verified"
    cov = r["action_coverage"]["bad"]
    assert isinstance(cov, dict)
    assert cov["covered"] is False
    assert cov["blocking_requires"]
    locs = [e["loc"]["line"] for e in cov["blocking_requires"] if e.get("loc")]
    assert any(line for line in locs)

    sc = scenarios(build_spec(parse(src)), 4, source_lines=lines)
    assert sc["result"] == "scenarios"
    cover_names = [s["name"] for s in sc["scenarios"] if s["kind"] == "action_coverage"]
    assert "cover_bad" not in cover_names
    assert any("bad" in w["message"] for w in sc["warnings"])


def test_scenarios_reachable_failed_includes_over_constrained_diagnostic():
    src = """
spec ScenarioImpossibleReachable {
  type Small = 0..2
  state { x: Small }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  reachable TooHigh { x == 3 }
}
"""
    out = scenarios(
        build_spec(parse(src)),
        3,
        deadlock_mode="ignore",
        source_lines=src.splitlines(),
    )
    assert out["result"] == "reachable_failed"
    assert out["checked_to_depth"] == 3
    [unreached] = out["unreached"]
    assert unreached["classification"] == "over_constrained"
    assert any(b.get("name") == "_bounds_x" for b in unreached["blocking_requires"])


def test_coverage_true_remains_bool():
    r = verify(build_spec(parse((SPECS / "cart_v1.fsl").read_text())), 8)
    for name, cov in r["action_coverage"].items():
        assert cov is True, name


def test_deadlock_terminal_scenario():
    src = """
spec DeadEnd {
  state { x: Int }
  init { x = 0 }
  action bump() { requires x == 0  x = 1 }
}
"""
    r = scenarios(build_spec(parse(src)), 4)
    assert r["result"] == "scenarios"
    names = [s["name"] for s in r["scenarios"]]
    assert "deadlock_terminal" in names


def test_cli_scenarios_cart_v1():
    out, code = cli_scenarios("cart_v1.fsl")
    assert code == 0
    assert out["result"] == "scenarios"
    assert out["convention"]
    scenario_names = [s["name"] for s in out["scenarios"]]
    assert "reach_SoldOut" in scenario_names
    assert all(n.startswith("cover_") for n in scenario_names if n.startswith("cover_"))


def test_exit_code_scenarios():
    from fslc.cli import exit_code
    assert exit_code({"result": "scenarios"}) == 0
