"""Tests for `fslc typestate` — judging where phantom-typed typestate is soundly
derivable from a design spec, and emitting a TypeScript skeleton for that slice.
"""
from pathlib import Path

import pytest

from fslc import parse, build_spec
from fslc.typestate import analyze
from fslc.cli import run_typestate, exit_code

ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"
EXAMPLES = ROOT / "examples"


def analyze_spec(path):
    ast = parse(Path(path).read_text(encoding="utf-8"))
    return analyze(build_spec(ast))


def entity(report, key):
    for e in report["entities"]:
        if e["entity"] == key:
            return e
    raise AssertionError(f"entity {key!r} not in {[e['entity'] for e in report['entities']]}")


def action(ent, name):
    for a in ent["actions"]:
        if a["action"] == name:
            return a
    raise AssertionError(f"action {name!r} not found")


# --- enum struct field (the canonical typestate case) ----------------------

def test_order_workflow_full_lifecycle():
    rep = analyze_spec(SPECS / "order_workflow.fsl")
    ent = entity(rep, "Order.status")
    assert ent["applicability"] == "full"
    assert action(ent, "pay")["transitions"] == [
        {"entity": "orders[o]", "from": ["Placed"], "to": "Paid", "conditional": False}
    ]
    # disjunctive guard merges into a union from-state
    cancel = action(ent, "cancel")
    assert sorted(cancel["transitions"][0]["from"]) == ["Paid", "Placed"]


def test_struct_literal_assignment_is_detected():
    # inventory transitions via whole-struct-literal `res[r] = Res { st: Held, ... }`.
    # Missing this would silently under-report the lifecycle while still claiming "full".
    rep = analyze_spec(SPECS / "inventory_reservation.fsl")
    ent = entity(rep, "Res.st")
    assert ent["applicability"] == "full"
    moves = {a["action"]: (a["transitions"][0]["from"], a["transitions"][0]["to"])
             for a in ent["actions"]}
    assert moves == {
        "hold": (["Free"], "Held"),
        "confirm": (["Held"], "Confirmed"),
        "release": (["Held"], "Free"),
    }


def test_value_precondition_kept_out_of_type():
    rep = analyze_spec(SPECS / "order_workflow.fsl")
    place = action(entity(rep, "Order.status"), "place")
    assert any("q > 0" in v for v in place["value_preconditions"])


# --- Option (none/some) state machine --------------------------------------

def test_cart_option_machine_full():
    rep = analyze_spec(SPECS / "cart_v1.fsl")
    ent = entity(rep, "cart (cart)") if False else None
    # entity key for an Option var is "<var> (<EnumOrVar>)"; locate by kind
    opt = [e for e in rep["entities"] if e["kind"] == "option"]
    assert opt, "expected an Option state machine"
    cart = opt[0]
    assert cart["applicability"] == "full"
    assert set(cart["states"]) == {"Empty", "Filled"}
    moves = {a["action"]: (a["transitions"][0]["from"], a["transitions"][0]["to"])
             for a in cart["actions"]}
    assert moves["add_to_cart"] == (["Empty"], "Filled")
    assert moves["remove_from_cart"] == (["Filled"], "Empty")
    assert moves["checkout"] == (["Filled"], "Empty")


# --- relational precondition: must be refused, not degraded ----------------

def test_job_pipeline_relational_is_refused():
    rep = analyze_spec(SPECS / "job_pipeline.fsl")
    ent = entity(rep, "Job.st")
    assert ent["applicability"] == "partial"
    start = action(ent, "start")
    assert start["verdict"] == "relational"
    assert start["transitions"][0]["from"] == []  # no local from-state
    assert start.get("diagnostics"), "relational verdict must carry a diagnostic"
    # the degraded transition must NOT leak into the emitted TypeScript
    assert "function start" not in ent["typescript"]
    assert "function submit" in ent["typescript"]  # the derivable one stays


# --- requirement traceability (business `process` layer) -------------------

def test_business_process_carries_requirement_ids():
    rep = analyze(build_spec(*_parse_layer(EXAMPLES / "layers" / "return_policy.fsl")))
    ent = entity(rep, "return_stage (ReturnStage)")
    assert ent["applicability"] == "full"
    approve = action(ent, "approve")
    assert approve["requirement"]["id"] == "approve"
    assert approve["transitions"][0]["from"] == ["Requested"]
    assert approve["transitions"][0]["to"] == "Approved"


def _parse_layer(path):
    from fslc.parser import parse_src
    ast, dn = parse_src(Path(path).read_text(encoding="utf-8"), str(Path(path).parent))
    return ast, dn


# --- no state machine ------------------------------------------------------

def test_spec_without_state_machine_reports_nothing():
    rep = analyze_spec(SPECS / "rate_limiter.fsl")
    assert rep["summary"]["entities"] == 0
    assert "note" in rep


# --- CLI envelope + exit code ----------------------------------------------

def test_cli_envelope_and_exit_code():
    res = run_typestate(str(SPECS / "order_workflow.fsl"))
    assert res["fsl"]
    assert res["result"] == "typestate"
    assert exit_code(res) == 0


def test_cli_missing_file_is_io_error():
    res = run_typestate(str(SPECS / "does_not_exist.fsl"))
    assert res["result"] == "error" and res["kind"] == "io"
    assert exit_code(res) == 2


# --- whole-corpus robustness: never crash, never silently degrade ----------

@pytest.mark.parametrize("path", sorted(SPECS.glob("*.fsl")))
def test_no_crash_on_any_spec(path):
    res = run_typestate(str(path))
    assert res["result"] in ("typestate", "error")
    if res["result"] == "typestate":
        for e in res["entities"]:
            # applicability is consistent with the per-action verdicts
            rel = [a for a in e["actions"] if a["verdict"] == "relational"]
            if e["applicability"] == "full":
                assert not rel
            # a refused transition is never emitted as a typed function
            for a in rel:
                assert f"function {a['action']}(" not in e["typescript"]
