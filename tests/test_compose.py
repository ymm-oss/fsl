"""FSL v2.0 spec compose tests (DESIGN-compose.md §5)."""
import json
import subprocess
from pathlib import Path

import pytest

from fslc import Monitor, build_spec, prove, scenarios, verify
from fslc.cli import run_check, exit_code
from fslc.parser import parse_src

ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"
PY = ROOT / ".venv" / "bin" / "python"


def _load_order_system():
    src = (SPECS / "order_system.fsl").read_text(encoding="utf-8")
    ast, dn = parse_src(src, str(SPECS))
    return build_spec(ast, dn), src.splitlines()


def _verify_order(depth=8):
    spec, lines = _load_order_system()
    return verify(spec, depth, source_lines=lines)


def _induction_order(depth=8, k_ind=1):
    spec, _ = _load_order_system()
    return prove(spec, k_ind, depth)


def _scenarios_order(depth=8):
    spec, lines = _load_order_system()
    return scenarios(spec, depth, source_lines=lines)


def _write_compose(body, name="_compose_test.fsl", base_dir=None):
    """Write a compose spec under specs/ (or base_dir) so use paths resolve."""
    root = Path(base_dir) if base_dir else SPECS
    path = root / name
    path.write_text(body, encoding="utf-8")
    return path


def test_order_system_verified_and_induction_proved():
    """§5.1: order_system verified + induction proved."""
    vr = _verify_order()
    assert vr["result"] == "verified"
    assert vr["spec"] == "OrderSystem"
    assert "PaidOrder" in vr["reachables"]
    assert vr["reachables"]["PaidOrder"]["witnessed_at_step"] >= 0

    pr = _induction_order()
    assert pr["result"] == "proved"
    assert pr["engine"] == "induction"
    assert "PaidOrder" in pr["reachables"]


def test_sync_witness_same_step_cart_and_pay():
    """§5.2: checkout_and_pay changes cart stock and pay capture in one step."""
    vr = _verify_order()
    witness = vr["reachables"]["PaidOrder"]["witness"]
    sync_steps = [
        e for e in witness
        if e.get("action", {}).get("name") == "checkout_and_pay"
    ]
    assert sync_steps, "expected checkout_and_pay in PaidOrder witness"
    changes = sync_steps[0]["changes"]
    assert any(k.startswith("cart.stock") for k in changes)
    assert any(k.startswith("pay.payments") and k.endswith("[st]") for k in changes)
    assert any(k == "pay.ledger" for k in changes)


def test_internal_actions_not_in_coverage():
    """§5.3: internal cart.checkout / pay.capture absent from action_coverage."""
    vr = _verify_order()
    cov = vr["action_coverage"]
    assert "cart.checkout" not in cov
    assert "pay.capture" not in cov
    assert "cart__checkout" not in cov
    assert "pay__capture" not in cov
    assert cov.get("checkout_and_pay") is True


def test_internal_removed_allows_standalone_checkout():
    """§5.3b: without internal, cart.checkout can appear in coverage."""
    body = """
compose OrderOpen {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"
}
"""
    path = _write_compose(body, "_compose_open.fsl")
    try:
        ast, dn = parse_src(path.read_text(encoding="utf-8"), str(SPECS))
        spec = build_spec(ast, dn)
        vr = verify(spec, 8)
        assert vr["result"] == "verified"
        assert "cart.checkout" in vr["action_coverage"]
    finally:
        path.unlink(missing_ok=True)


def test_cross_invariant_violation_uses_dotted_keys():
    """§5.4: glue action breaks invariant → violated with cart.stock keys."""
    body = """
compose BrokenOrder {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as pay  from "payment.fsl"

  state { orders_linked: Int }
  init  { orders_linked = 0 }

  action break_linked() {
    orders_linked = orders_linked - 1
  }

  invariant LinkedNonNeg { orders_linked >= 0 }
}
"""
    path = _write_compose(body, "_compose_broken.fsl")
    try:
        ast, dn = parse_src(path.read_text(encoding="utf-8"), str(SPECS))
        spec = build_spec(ast, dn)
        vr = verify(spec, 4)
        assert vr["result"] == "violated"
        assert vr["violation_kind"] == "invariant"
        state_keys = vr["trace"][-1]["state"]
        assert "cart.stock" in state_keys
        assert "__" not in json.dumps(state_keys)
    finally:
        path.unlink(missing_ok=True)


@pytest.mark.parametrize("src,needle", [
    ("""
compose Dup {
  use ShoppingCart as cart from "cart_v1.fsl"
  use Payment      as cart from "payment.fsl"
}
""", "duplicate alias"),
    ("""
compose BadAlias {
  use ShoppingCart as cart from "cart_v1.fsl"
  internal shop.checkout
}
""", "unknown alias"),
    ("""
compose BadName {
  use WrongName as cart from "cart_v1.fsl"
}
""", "spec name mismatch"),
    ("""
compose SameSync {
  use ShoppingCart as cart from "cart_v1.fsl"
  action both(u: cart.UserId) = cart.checkout(u) || cart.remove_from_cart(u) { }
}
""", "same component"),
])
def test_static_compose_errors_kind_type(src, needle):
    """§5.5: static compose errors → kind type."""
    path = _write_compose(src, f"_compose_err_{needle[:8]}.fsl")
    try:
        out = run_check(str(path))
        assert out["result"] == "error"
        assert out["kind"] == "type"
        assert needle in out["message"]
    finally:
        path.unlink(missing_ok=True)


def test_static_compose_missing_file_kind_io():
    """§5.5: missing use file → kind io."""
    body = """
compose Missing {
  use ShoppingCart as cart from "no_such_cart.fsl"
}
"""
    path = _write_compose(body, "_compose_missing.fsl")
    try:
        out = run_check(str(path))
        assert out["result"] == "error"
        assert out["kind"] == "io"
        assert "no_such_cart.fsl" in out["message"]
    finally:
        path.unlink(missing_ok=True)


def test_rewrite_component_range_bounds_use_prefixed_const():
    """Regression: type, binder_range, and param_range bounds rewrite component consts."""
    comp_path = SPECS / "_compose_rewrite_ranges_comp.fsl"
    comp = """
spec RewriteRanges {
  const MAX = 2
  type T = 0..MAX
  state { v: T }
  init { v = 0 }
  action set(n in 0..MAX) { v = n }
  invariant Bound { forall i in 0..MAX { v <= MAX } }
}
"""
    body = """
compose RewriteRangesSystem {
  use RewriteRanges as r from "_compose_rewrite_ranges_comp.fsl"
}
"""
    comp_path.write_text(comp, encoding="utf-8")
    path = _write_compose(body, "_compose_rewrite_ranges.fsl")
    try:
        ast, dn = parse_src(path.read_text(encoding="utf-8"), str(SPECS))
        items = ast[2]
        prefixed_max = ("var", "r__MAX")

        assert ("type", "r__T", ("num", 0), prefixed_max) in items

        action = next(it for it in items if it[0] == "action" and it[1] == "r__set")
        assert action[2] == [("param_range", "n", ("num", 0), prefixed_max)]

        invariant = next(it for it in items if it[0] == "invariant" and it[1] == "r__Bound")
        assert invariant[2][1] == ("binder_range", "i", ("num", 0), prefixed_max)

        spec = build_spec(ast, dn)
        assert spec["types"]["r__T"]["lo"] == 0
        assert spec["types"]["r__T"]["hi"] == 2
    finally:
        path.unlink(missing_ok=True)
        comp_path.unlink(missing_ok=True)


def test_rewrite_sync_action_field_argument_in_param_map():
    """Regression: sync argument substitution rewrites alias.field references."""
    source_path = SPECS / "_compose_sync_source.fsl"
    sink_path = SPECS / "_compose_sync_sink.fsl"
    source = """
spec SyncSource {
  type Qty = 0..3
  state { x: Qty }
  init { x = 1 }
  action noop() { }
}
"""
    sink = """
spec SyncSink {
  type Qty = 0..3
  state { seen: Qty }
  init { seen = 0 }
  action consume(q: Qty) { seen = q }
}
"""
    body = """
compose SyncFieldArg {
  use SyncSource as src from "_compose_sync_source.fsl"
  use SyncSink as sink from "_compose_sync_sink.fsl"
  action mirror() = sink.consume(src.x) { }
}
"""
    source_path.write_text(source, encoding="utf-8")
    sink_path.write_text(sink, encoding="utf-8")
    path = _write_compose(body, "_compose_sync_field_arg.fsl")
    try:
        ast, _ = parse_src(path.read_text(encoding="utf-8"), str(SPECS))
        action = next(it for it in ast[2] if it[0] == "action" and it[1] == "mirror")
        assign = action[3][0]

        assert assign[0] == "assign"
        assert assign[1] == ("var", "sink__seen")
        assert assign[2] == ("var", "src__x")
    finally:
        path.unlink(missing_ok=True)
        source_path.unlink(missing_ok=True)
        sink_path.unlink(missing_ok=True)


def test_order_system_scenarios_cover_all_non_internal_actions():
    """Regression: compose cover_* scenarios use display names for all covered actions."""
    sc = _scenarios_order()
    assert sc["result"] == "scenarios"
    covers = [s for s in sc["scenarios"] if s["kind"] == "action_coverage"]
    reaches = [s for s in sc["scenarios"] if s["kind"] == "reachable"]
    assert len(covers) == 6
    assert len(reaches) == 3
    expected = {
        "cover_cart.add_to_cart",
        "cover_cart.remove_from_cart",
        "cover_pay.authorize",
        "cover_pay.refund",
        "cover_pay.void",
        "cover_checkout_and_pay",
    }
    assert {s["name"] for s in covers} == expected
    assert all(s["name"].startswith("cover_") and "__" not in s["name"] for s in covers)
    assert "__" not in json.dumps(sc)


def test_json_outputs_contain_no_double_underscore():
    """§5.6: verify and scenarios JSON have no '__' substring."""
    vr = _verify_order()
    sc = _scenarios_order()
    assert "__" not in json.dumps(vr)
    assert "__" not in json.dumps(sc)
    assert "cart.stock" in json.dumps(vr)


def test_monitor_order_system_checkout_and_pay():
    """§5.7: Monitor on compose file with dotted state keys."""
    mon = Monitor(str(SPECS / "order_system.fsl"))
    state = mon.reset()
    assert "cart.stock" in state
    assert "pay.payments" in state
    assert "__" not in json.dumps(state)

    mon.step("pay.authorize", {"p": 0, "a": 1})
    mon.step("cart.add_to_cart", {"u": 0, "i": 1})
    r = mon.step("checkout_and_pay", {"u": 0, "p": 0})
    assert r["ok"] is True
    assert "cart.stock" in r["state"]
    assert r["state"]["pay.payments"]["0"]["st"] == "Captured"
    assert "__" not in json.dumps(r)


def test_non_compose_specs_regression():
    """§5.8: non-compose specs unchanged."""
    src = (SPECS / "cart_v1.fsl").read_text(encoding="utf-8")
    ast, dn = parse_src(src)
    assert dn == {}
    spec = build_spec(ast, dn)
    vr = verify(spec, 8, source_lines=src.splitlines())
    assert vr["result"] == "verified"
    assert "SoldOut" in vr["reachables"]
    assert "stock" in vr["reachables"]["SoldOut"]["witness"][-1]["state"]
