"""FSL v1.0 core: verification, CLI, and JSON schema checks."""
import json
import subprocess
import sys
from pathlib import Path

import pytest

from fslc import parse, build_spec, verify, FslError
from fslc.cli import run_check, run_verify, exit_code

SPECS = Path(__file__).resolve().parent.parent / "specs"
ROOT = Path(__file__).resolve().parent.parent
PY = ROOT / ".venv" / "bin" / "python"


def run(name, depth=8, **kwargs):
    ast = parse((SPECS / name).read_text(encoding="utf-8"))
    return verify(build_spec(ast), depth, **kwargs)


def cli_verify(name, depth=8):
    proc = subprocess.run(
        [str(PY), "-m", "fslc", "verify", str(SPECS / name), "--depth", str(depth)],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    return json.loads(proc.stdout), proc.returncode


def test_cart_v1_verified_with_soldout_witness():
    r = run("cart_v1.fsl")
    assert r["result"] == "verified"
    sold = r["reachables"]["SoldOut"]
    assert sold["witnessed_at_step"] == 4
    assert len(sold["witness"]) == 5
    assert sold["witness"][-1]["state"]["stock"] == {"0": 0, "1": 0}


def test_cart_v1_buggy_type_bound():
    r = run("cart_v1_buggy.fsl")
    assert r["result"] == "violated"
    assert r["violation_kind"] == "type_bound"
    assert r["invariant"] == "_bounds_stock"
    assert r["last_action"]["name"] == "checkout"
    assert r["violated_at_step"] == 4


def test_order_workflow_verified():
    r = run("order_workflow.fsl")
    assert r["result"] == "verified"
    assert "FullLifecycle" in r["reachables"]
    assert r["reachables"]["FullLifecycle"]["witnessed_at_step"] == 3


def test_trace_changes_format():
    r = run("cart_v1.fsl")
    steps_with_changes = [e for e in r["reachables"]["SoldOut"]["witness"] if "changes" in e]
    assert steps_with_changes
    for entry in steps_with_changes:
        for path, diff in entry["changes"].items():
            assert "from" in diff and "to" in diff
            assert "[" in path or path in ("revenue", "shipped")


def test_json_no_internal_encoding():
    r = run("order_workflow.fsl")
    blob = json.dumps(r)
    assert "__present" not in blob
    assert "__value" not in blob
    assert "__status" not in blob
    assert "__k" not in blob
    step = r["reachables"]["FullLifecycle"]["witnessed_at_step"]
    witness = r["reachables"]["FullLifecycle"]["witness"][step]
    shipped = [k for k, v in witness["state"]["orders"].items() if v["status"] == "Shipped"]
    assert shipped
    status = witness["state"]["orders"][shipped[0]]["status"]
    assert isinstance(status, str)
    assert status == "Shipped"

    buggy = run("cart_v1_buggy.fsl")
    assert buggy["result"] == "violated"
    for b in buggy.get("violating_bindings") or []:
        assert "__k" not in b
        if "key" in b:
            assert isinstance(b["key"], int)


def test_ensures_violation_detected():
    src = """
spec EnsuresBug {
  type Id = 0..0
  state { x: Int }
  init { x = 0 }
  action inc() {
    x = x + 1
    ensures x == old(x) + 1
  }
  action bad() {
    x = x + 2
    ensures x == old(x) + 1
  }
}
"""
    r = verify(build_spec(parse(src)), 4)
    assert r["result"] == "violated"
    assert r["violation_kind"] == "ensures"
    assert r["last_action"]["name"] == "bad"


def test_double_assign_semantics_error():
    src = """
spec DoubleAssign {
  state { x: Int }
  init { x = 0 }
  action go() {
    x = 1
    x = 2
  }
}
"""
    with pytest.raises(FslError, match="double assignment") as exc:
        verify(build_spec(parse(src)), 2)
    assert exc.value.loc is not None
    assert exc.value.loc["line"] > 0


def test_fslc_check_ok():
    r = run_check(str(SPECS / "cart_v1.fsl"))
    assert r["result"] == "ok"
    assert r["fsl"] == "1.0"
    assert r["spec"] == "ShoppingCart"
    assert "warnings" in r


def test_fslc_check_parse_error():
    r = run_check("/nonexistent/missing.fsl")
    assert r["result"] == "error"
    assert r["kind"] == "io"


def test_map_int_deprecation_warning():
    src = """
spec WarnMapInt {
  type ItemId = 0..1
  state { stock: Map<Int, Int> }
  init { stock[0] = 1 }
  action noop() { }
}
"""
    spec = build_spec(parse(src))
    assert any(
        "Map<Int" in (w.get("message", "") if isinstance(w, dict) else str(w))
        for w in spec["warnings"]
    )


def test_cli_envelope_and_exit_codes():
    out, code = cli_verify("cart_v1.fsl")
    assert out["fsl"] == "1.0"
    assert code == 0

    out, code = cli_verify("cart_v1_buggy.fsl")
    assert out["result"] == "violated"
    assert code == 1

    proc = subprocess.run(
        [str(PY), "-m", "fslc", "check", str(SPECS / "cart_v1.fsl")],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    assert proc.returncode == 0
    assert json.loads(proc.stdout)["result"] == "ok"


def test_exit_code_mapping():
    assert exit_code({"result": "verified"}) == 0
    assert exit_code({"result": "violated"}) == 1
    assert exit_code({"result": "reachable_failed"}) == 1
    assert exit_code({"result": "error", "kind": "type"}) == 2
    assert exit_code({"result": "error", "kind": "internal"}) == 3


def test_if_pend_merge_preserves_prior_writes():
    src = """
spec T {
  type K = 0..1
  state { m: Map<K, Int>, flag: Bool }
  init {
    forall k: K { m[k] = 0 }
    flag = false
  }
  action a() {
    m[0] = 1
    if flag { m[1] = 2 }
  }
  reachable R { m[0] == 1 }
}
"""
    r = verify(build_spec(parse(src)), 4)
    assert r["result"] == "verified"
    assert r["reachables"]["R"]["witnessed_at_step"] == 1


def test_struct_equality_enables_guarded_action():
    src = """
spec StructGuard {
  type Kid = 0..0
  enum ST { A, B }
  struct O { st: ST, q: Int }
  state { os: Map<Kid, O> }
  init { os[0] = O { st: A, q: 0 } }
  action touch(k: Kid) {
    requires os[k] == O { st: A, q: 0 }
    os[k].st = B
  }
}
"""
    r = verify(build_spec(parse(src)), 1)
    assert r["result"] == "verified"
    assert r["action_coverage"]["touch"] is True


def test_scalar_option_state():
    src = """
spec OptScalar {
  type K = 0..0
  state { c: Option<K> }
  init { c = none }
  action set() {
    requires c == none
    c = some(0)
  }
  action clear() {
    requires c is some(x)
    c = none
  }
  reachable HasValue { c != none }
}
"""
    r = verify(build_spec(parse(src)), 4)
    assert r["result"] == "verified"
    w = r["reachables"]["HasValue"]["witness"][-1]["state"]["c"]
    assert w == 0


def test_scalar_struct_state():
    src = """
spec StructScalar {
  enum ST { A, B }
  struct O { st: ST, q: Int }
  state { o: O }
  init { o = O { st: A, q: 0 } }
  action flip() {
    requires o.st == A
    o.st = B
  }
  reachable Flipped { o.st == B }
}
"""
    r = verify(build_spec(parse(src)), 4)
    assert r["result"] == "verified"
    assert r["reachables"]["Flipped"]["witness"][-1]["state"]["o"]["st"] == "B"


def test_set_size_uses_receiver_element_type():
    src = """
spec TwoSets {
  type AId = 0..2
  type BId = 0..0
  state { sa: Set<AId>, sb: Set<BId> }
  init {
    sa = Set {}
    sb = Set { 0 }
  }
  invariant SizeUsesA {
    sa.add(1).size() == 1
  }
  action noop() { }
}
"""
    r = verify(build_spec(parse(src)), 2)
    assert r["result"] == "verified"


def test_warnings_format_and_deadlock_trace():
    src = """
spec DeadEnd {
  state { x: Int }
  init { x = 0 }
  action bump() { requires x == 0  x = 1 }
}
"""
    r = verify(build_spec(parse(src)), 4, deadlock_mode="warn")
    assert r["result"] == "verified"
    assert all(isinstance(w, dict) and "message" in w for w in r["warnings"])
    dl = [w for w in r["warnings"] if "deadlock" in w["message"]]
    assert len(dl) == 1
    assert r["deadlock"]["found"] is True
    assert "at_step" in r["deadlock"]
    assert "trace" in r["deadlock"]
    assert isinstance(r["deadlock"]["trace"], list)


def test_action_coverage_enabled_before_deadlock():
    src = """
spec S {
  type K = 0..1
  enum St { A, B }
  struct O { st: St, q: Int }
  state { os: Map<K, O> }
  init {
    forall k: K { os[k] = O { st: A, q: 0 } }
  }
  action go(k: K) {
    requires os[k] == O { st: A, q: 0 }
    os[k].st = B
  }
  invariant I { true }
}
"""
    r = verify(build_spec(parse(src)), 8)
    assert r["result"] == "verified"
    assert r["action_coverage"]["go"] is True


def test_parse_error_unexpected_characters_expected():
    broken = ROOT / "specs" / "_parse_char_break.fsl"
    broken.write_text("spec X { init { x = } }", encoding="utf-8")
    try:
        out = run_check(str(broken))
        assert out["result"] == "error"
        assert out["kind"] == "parse"
        assert out.get("expected") is not None
        assert "one of:" in out["expected"]
    finally:
        broken.unlink(missing_ok=True)


def test_parse_error_includes_expected_tokens():
    broken = ROOT / "specs" / "_parse_break.fsl"
    broken.write_text("spec X { state {", encoding="utf-8")
    try:
        out = run_check(str(broken))
        assert out["result"] == "error"
        assert out["kind"] == "parse"
        assert out.get("loc")
        assert out.get("expected")
        assert "one of:" in out["expected"]
    finally:
        broken.unlink(missing_ok=True)


def test_option_some_equality_is_type_error():
    src = """
spec P {
  type K = 0..1
  state { c: Option<K> }
  init { c = some(0) }
  action go() {
    requires c == some(0)
    c = none
  }
  invariant I { true }
}
"""
    with pytest.raises(FslError) as exc:
        verify(build_spec(parse(src)), 8)
    assert exc.value.kind == "type"
    assert exc.value.hint is not None
    assert "is some(v)" in exc.value.hint
