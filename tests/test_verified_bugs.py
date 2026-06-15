from pathlib import Path

from fslc import Monitor, build_spec, parse, verify
from fslc.cli import exit_code, run_check, run_scenarios, run_testgen, run_verify
from fslc.parser import parse_src
from fslc.explain import explain_file
from fslc.model import eval_const
from fslc.mutate import mutate_file
from fslc.typestate import analyze


ROOT = Path(__file__).resolve().parent.parent


def _verify_fixture(tmp_path, name, src, depth=2):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return verify(build_spec(parse(path.read_text(encoding="utf-8"))), depth)


def _typestate_fixture(src):
    return analyze(build_spec(parse(src)))


def _only_entity_action(report, action_name):
    actions = [
        action
        for entity in report["entities"]
        for action in entity["actions"]
        if action["action"] == action_name
    ]
    assert len(actions) == 1, report
    return actions[0]


def test_stage_helper_is_rejected_outside_business_dialect(tmp_path):
    src = """
spec StageOutsideBusiness {
  type Case = 0..1
  state { c: Case }
  init { c = 0 }
  action noop() { }
  invariant Bad { stage(c) == 0 }
}
"""
    path = tmp_path / "stage_outside_business.fsl"
    path.write_text(src, encoding="utf-8")

    result = run_check(str(path))

    assert result["result"] == "error", result
    assert result["kind"] == "type"
    assert result["message"] == "stage(...) は business 方言でのみ使用可"


def test_stage_helper_remains_allowed_in_business_dialect(tmp_path):
    src = """
business StageBusinessOk {
  actor Manager
  case Return = 0..1
  process Return {
    stages Requested, Approved
    initial Requested
    transition approve Requested -> Approved by Manager
  }
  policy P-1 "stage helper is business-only" invariant {
    forall c: Return { stage(c) == Requested or stage(c) == Approved }
  }
}
"""
    path = tmp_path / "stage_business_ok.fsl"
    path.write_text(src, encoding="utf-8")

    result = run_check(str(path))

    assert result["result"] == "ok", result


def test_check_allows_actionless_spec_but_verify_reports_semantics(tmp_path):
    src = """
spec NoActions {
  state { x: Int }
  init { x = 0 }
  invariant Tautology { true }
}
"""
    path = tmp_path / "no_actions.fsl"
    path.write_text(src, encoding="utf-8")

    checked = run_check(str(path))
    verified = run_verify(str(path), depth=1, deadlock_mode="warn")

    assert checked["result"] == "ok", checked
    assert verified["result"] == "error", verified
    assert verified["kind"] == "semantics"
    assert verified["message"] == "spec has no actions"


def test_bool_set_and_map_keys_verify_and_display(tmp_path):
    src = """
spec BoolContainers {
  type Qty = 0..2
  state { seen: Set<Bool>, counts: Map<Bool, Qty> }
  init {
    seen = Set { true }
    counts[false] = 0
    counts[true] = 1
  }
  action mark_false() {
    seen = seen.add(false)
    counts[false] = 2
  }
  invariant BoolKeys { seen.contains(true) and counts[true] <= 2 and seen.size() <= 2 }
  reachable HasFalse { seen.contains(false) and counts[false] == 2 }
}
"""
    result = _verify_fixture(tmp_path, "bool_containers.fsl", src, depth=1)

    assert result["result"] == "verified", result
    assert result["reachables"]["HasFalse"]["witness"][-1]["state"]["counts"] == {
        "false": 2,
        "true": 1,
    }

    monitor_src = """
spec BoolContainersMonitor {
  type Qty = 0..2
  state { seen: Set<Bool>, counts: Map<Bool, Qty> }
  init {
    seen = Set { true }
    counts[true] = 1
  }
  action mark_false() {
    seen = seen.add(false)
    counts[false] = 2
  }
  invariant BoolKeys { true }
}
"""
    monitor = Monitor(build_spec(parse(monitor_src)))
    assert monitor.state == {
        "seen": [True],
        "counts": {"false": 0, "true": 1},
    }
    step = monitor.step("mark_false", {})
    assert step["ok"] is True, step
    assert monitor.state == {
        "seen": [False, True],
        "counts": {"false": 2, "true": 1},
    }


def test_map_int_warning_without_domain_types(tmp_path):
    src = """
spec MapIntWarningStandalone {
  state { m: Map<Int, Int> }
  init { m[0] = 0 }
  action noop() { }
  invariant Tautology { true }
}
"""
    path = tmp_path / "map_int_warning_standalone.fsl"
    path.write_text(src, encoding="utf-8")

    result = run_check(str(path))

    assert result["result"] == "ok", result
    assert any("Map<Int" in warning.get("message", "") for warning in result["warnings"])


def test_typestate_conjunctive_guard_extracts_from_state():
    src = """
spec TsConj {
  enum St { A, B }
  type Qty = 0..2
  struct E { st: St }
  state { e: E }
  init { e = E { st: A } }
  action go(q: Qty) {
    requires e.st == A and q > 0
    e.st = B
  }
}
"""
    go = _only_entity_action(_typestate_fixture(src), "go")

    assert go["verdict"] == "derivable"
    assert go["transitions"][0]["from"] == ["A"]


def test_typestate_if_conditions_extract_branch_from_states():
    path = ROOT / "examples" / "gallery" / "valid" / "tiny_traffic_light.fsl"
    report = analyze(build_spec(parse(path.read_text(encoding="utf-8"))))
    tick = _only_entity_action(report, "tick")

    assert tick["verdict"] != "relational"
    assert all(transition["from"] for transition in tick["transitions"])


def test_leadsto_binder_where_skips_values_outside_filter(tmp_path):
    src = """
spec LeadstoWhere {
  type P = 0..1
  state { x: Int }
  init { x = 0 }
  action noop() { }
  invariant T { true }
  leadsTo PositiveOnly {
    forall p: P where p > 0 {
      p == 0 ~> x == 1
    }
  }
}
"""
    result = _verify_fixture(tmp_path, "leadsto_where.fsl", src)

    assert result["result"] == "verified", result


def test_nested_init_forall_where_does_not_overconstrain(tmp_path):
    src = """
spec InitNestedWhere {
  type T = 0..1
  state { flag: Map<T, Bool> }
  init {
    forall p: T where p > 0 {
      forall q: T where q > 0 {
        flag[q] = true
      }
    }
    flag[0] = false
  }
  action noop() { }
  invariant Tautology { true }
}
"""
    result = _verify_fixture(tmp_path, "init_nested_where.fsl", src)

    assert result["result"] == "verified", result


def test_missing_spec_path_is_io_error_for_check_and_scenarios(tmp_path):
    missing = tmp_path / "missing.fsl"

    checked = run_check(str(missing))
    assert checked["result"] == "error"
    assert checked["kind"] == "io"
    assert exit_code(checked) == 2

    scenario_result = run_scenarios(str(missing), depth=1)
    assert scenario_result["result"] == "error"
    assert scenario_result["kind"] == "io"
    assert exit_code(scenario_result) == 2


def test_compile_time_division_in_range_bound_is_checked_and_evaluated(tmp_path):
    src = """
spec ConstDivisionRange {
  const MAX = 10
  type K = 0..(MAX / 2)
  state { x: K }
  init { x = 0 }
  action noop() { }
  invariant Bound { x <= 5 }
}
"""
    path = tmp_path / "const_division_range.fsl"
    path.write_text(src, encoding="utf-8")

    checked = run_check(str(path))
    assert checked["result"] == "ok", checked

    spec = build_spec(parse(src))
    assert spec["types"]["K"]["lo"] == 0
    assert spec["types"]["K"]["hi"] == 5


def test_monitor_full_seq_push_is_type_bound_not_partial_op():
    # DESIGN-seq: an over-capacity push is a type_bound violation of the implicit
    # _bounds_* length invariant (matching BMC), not a partial_op.
    src = """
spec PushFull {
  type N = 0..2
  state { q: Seq<N, 1> }
  init { q = Seq {} }
  action add() { q = q.push(0) }
  invariant T { true }
}
"""
    monitor = Monitor(src)
    monitor.reset()

    first = monitor.step("add", {})
    assert first["ok"] is True, first

    second = monitor.step("add", {})
    assert second["ok"] is False, second
    assert second["kind"] == "type_bound", second
    assert second["name"] == "_bounds_q", second


def test_eval_const_compile_time_division_uses_euclidean_negative_cases():
    assert eval_const(
        ("bin", "/", ("var", "NEG_THREE"), ("num", 2)),
        {"NEG_THREE": -3},
    ) == -2
    assert eval_const(
        ("bin", "/", ("num", 3), ("var", "NEG_TWO")),
        {"NEG_TWO": -2},
    ) == -1


def test_set_bounded_add_out_of_range_is_violated(tmp_path):
    src = """
spec SetBoundedBad {
  type Id = 0..3
  state { s: Set<Id> }
  init { s = Set {} }
  action addbad() { s = s.add(99) }
}
"""
    result = _verify_fixture(tmp_path, "set_bounded_bad.fsl", src, depth=1)

    assert result["result"] == "violated", result
    assert result["violation_kind"] == "type_bound"
    assert result["invariant"] == "_bounds_s"


def test_map_int_value_out_of_range_is_violated(tmp_path):
    src = """
spec MapIntValueBad {
  type Qty = 0..5
  state { m: Map<Int, Qty> }
  init { m[0] = 0 }
  action setbad() { m[0] = 99 }
}
"""
    result = _verify_fixture(tmp_path, "map_int_value_bad.fsl", src, depth=1)

    assert result["result"] == "violated", result
    assert result["violation_kind"] == "type_bound"
    assert result["invariant"] == "_bounds_m"


def test_explain_max_mutants_counts_processed_not_global_index(tmp_path):
    src = """
spec ExplainMaxMutants {
  state { x: Int }
  init { x = 0 }
  action bad() {
    requires x == 1
    x = 2
  }
  invariant Small { x <= 1 }
}
"""
    path = tmp_path / "explain_max_mutants.fsl"
    path.write_text(src, encoding="utf-8")

    result = explain_file(str(path), depth=1, max_mutants=1)
    small = next(item for item in result["counterfactuals"] if item["invariant"] == "Small")

    assert small["weakening"] is not None, result
    assert small["weakening"]["op"] == "assignment-removal"
    assert small["weakening"]["origin"] == "init"


def test_init_weakening_counterfactual():
    result = explain_file(str(ROOT / "specs" / "audit_log.fsl"), depth=4)
    balance = next(
        item for item in result["counterfactuals"]
        if item["invariant"] == "BalanceNonNegative"
    )

    assert balance["weakening"] is not None, result
    assert balance["weakening"]["op"] == "assignment-removal"
    assert balance["weakening"]["origin"] == "init"
    assert balance["weakening"]["label"] == "init weakening"
    assert balance["weakening"]["source_text"] == "balance = 0"


def test_acceptance_forbidden_requirement_not_none(tmp_path):
    src = """
requirements ExplainScenarioRequirements {
  type Count = 0..1
  state { x: Count }
  init { x = 0 }
  action inc() {
    requires x == 0
    x = 1
  }
  acceptance AC-1 "one increment works" {
    inc()
    expect x == 1
  }
  forbidden FB-1 "second increment rejected" {
    inc()
    inc()
    expect rejected
  }
}
"""
    path = tmp_path / "scenario_requirements.fsl"
    path.write_text(src, encoding="utf-8")

    result = explain_file(str(path), depth=2)
    witnesses = {w["name"]: w for w in result["witnesses"]}

    assert witnesses["acceptance_AC-1"]["requirement"] == {
        "id": "AC-1",
        "text": "one increment works",
    }
    assert witnesses["forbidden_FB-1"]["requirement"] == {
        "id": "FB-1",
        "text": "second increment rejected",
    }


def test_step_partial_op_in_invariant_returns_result_dict():
    src = """
spec InvariantPartialOp {
  type JobId = 0..1
  state { q: Seq<JobId, 2> }
  init { q = Seq {} }
  action noop() { }
  invariant HeadDefined { q.head() == 0 }
}
"""
    monitor = Monitor(src)
    monitor.reset()

    result = monitor.step("noop", {})

    assert result["ok"] is False
    assert result["kind"] == "partial_op"
    assert result["name"] == "_partial_noop"


def test_testgen_no_output_flag_no_nameerror(tmp_path):
    src = """
spec TestgenDefaultName {
  state { x: Int }
  init { x = 0 }
  action noop() { }
  invariant TrueInvariant { true }
}
"""
    path = tmp_path / "testgen_default_name.fsl"
    path.write_text(src, encoding="utf-8")

    result = run_testgen(str(path), depth=1, output=None, write_file=False)

    assert result["result"] == "generated", result
    assert result["output"] == "test_testgenDefaultName.py"


def test_forbidden_testgen_rejection_assertion(tmp_path):
    src = """
requirements ForbiddenTestgen {
  type OrderId = 0..1
  enum OSt { Cart, Paid, Shipped, Cancelled }
  state { order: Map<OrderId, OSt> }
  init { forall o: OrderId { order[o] = Cart } }
  action pay(o: OrderId) { requires order[o] == Cart order[o] = Paid }
  action ship(o: OrderId) { requires order[o] == Paid order[o] = Shipped }
  action cancel(o: OrderId) { requires order[o] == Paid order[o] = Cancelled }
  forbidden FB-1 "shipped order cannot be cancelled" {
    pay(0)
    ship(0)
    cancel(0)
    expect rejected
  }
}
"""
    path = tmp_path / "forbidden_testgen.fsl"
    path.write_text(src, encoding="utf-8")

    result = run_testgen(str(path), depth=4, output=None, write_file=False)
    content = result["content"]

    assert result["result"] == "generated", result
    assert "_assert_rejected(result, 'requires_failed')" in content
    assert "result = adapter.step('cancel', {'o': 0})" in content
    compile(content, str(tmp_path / "test_forbidden_testgen.py"), "exec")


def test_acceptance_action_argument_resolves_const(tmp_path):
    src = """
requirements ConstArgAcceptance {
  const FIRST = 0
  type Sub = 0..1
  enum Screen { Browsing, CancelForm }
  state { scr: Map<Sub, Screen> }
  init { forall c: Sub { scr[c] = Browsing } }
  requirement REQ-1 "cancel opens form" {
    action tap_cancel(c: Sub) {
      requires scr[c] == Browsing
      scr[c] = CancelForm
    }
  }
  acceptance AC-1 "const arg reaches form" {
    tap_cancel(FIRST)
    expect scr[FIRST] == CancelForm
  }
}
"""
    path = tmp_path / "const_arg_acceptance.fsl"
    path.write_text(src, encoding="utf-8")

    assert run_check(str(path))["result"] == "ok"


def test_acceptance_non_bool_expect_returns_error_envelope(tmp_path):
    src = """
requirements NonBoolExpect {
  type Count = 0..1
  state { x: Count }
  init { x = 0 }
  requirement REQ-1 "noop" {
    action noop() { }
  }
  acceptance AC-1 "expect must be bool" {
    noop()
    expect x
  }
}
"""
    path = tmp_path / "non_bool_expect.fsl"
    path.write_text(src, encoding="utf-8")

    result = run_check(str(path))

    assert result["result"] == "error"
    assert result["kind"] == "acceptance"
    assert result["id"] == "AC-1"
    assert result["failed_step"] == 1
    assert "expected bool" in result["message"]


def test_forbidden_param_errors_return_failed_step_with_forbidden_kind(tmp_path):
    setup_unknown = """
requirements ForbiddenUnknownSetup {
  type OrderId = 0..1
  enum OSt { Cart, Paid }
  state { order: Map<OrderId, OSt> }
  init { forall o: OrderId { order[o] = Cart } }
  requirement REQ-1 "pay" {
    action pay(o: OrderId) {
      requires order[o] == Cart
      order[o] = Paid
    }
  }
  forbidden FB-1 "bad setup action is structured" {
    missing(0)
    pay(0)
    expect rejected
  }
}
"""
    final_arity = setup_unknown.replace(
        "requirements ForbiddenUnknownSetup",
        "requirements ForbiddenFinalArity",
    ).replace(
        "    missing(0)\n    pay(0)",
        "    pay(0)\n    pay()",
    )
    setup_path = tmp_path / "forbidden_unknown_setup.fsl"
    final_path = tmp_path / "forbidden_final_arity.fsl"
    setup_path.write_text(setup_unknown, encoding="utf-8")
    final_path.write_text(final_arity, encoding="utf-8")

    setup_result = run_check(str(setup_path))
    assert setup_result["result"] == "error"
    assert setup_result["kind"] == "forbidden_setup"
    assert setup_result["id"] == "FB-1"
    assert setup_result["failed_step"] == 0
    assert setup_result["step"]["action"] == "missing"

    final_result = run_check(str(final_path))
    assert final_result["result"] == "error"
    assert final_result["kind"] == "forbidden"
    assert final_result["id"] == "FB-1"
    assert final_result["failed_step"] == 1
    assert final_result["step"]["action"] == "pay"
    assert "arity mismatch" in final_result["message"]


def test_mutate_by_requirement_excludes_acceptance_and_forbidden_ids(tmp_path):
    src = """
requirements LoneAcceptanceStress {
  type Count = 0..2
  state { x: Count }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  acceptance AC-2 "one increment works" {
    inc()
    expect x == 1
  }
  forbidden FB-1 "third increment rejected" {
    inc()
    inc()
    inc()
    expect rejected
  }
}
"""
    path = tmp_path / "lone_acceptance_stress.fsl"
    path.write_text(src, encoding="utf-8")

    result = mutate_file(str(path), depth=3, by_requirement=True, max_mutants=1000)

    assert result["result"] == "mutated", result
    assert "AC-2" not in result["by_requirement"]
    assert "FB-1" not in result["by_requirement"]
    assert result["by_requirement"] == {}


def test_refine_action_argument_type_mismatch_is_rejected(tmp_path):
    from fslc.cli import run_refine

    impl = """
spec Impl488 {
  enum St { A, B }
  type N = 0..1
  state { st: St }
  init { st = A }
  action go(n: N) { st = A }
  invariant T { true }
}
"""
    abs = """
spec Abs488 {
  enum St { A, B }
  state { st: St }
  init { st = A }
  action astep(s: St) { st = s }
  invariant T { true }
}
"""
    mapping = """
refinement R488 {
  impl Impl488
  abs Abs488
  map st = st
  action go(n) -> astep(n)
}
"""
    (tmp_path / "impl488.fsl").write_text(impl, encoding="utf-8")
    (tmp_path / "abs488.fsl").write_text(abs, encoding="utf-8")
    (tmp_path / "map488.fsl").write_text(mapping, encoding="utf-8")

    r = run_refine(
        str(tmp_path / "impl488.fsl"),
        str(tmp_path / "abs488.fsl"),
        str(tmp_path / "map488.fsl"),
        depth=2,
    )
    # domain arg `n` mapped into an enum parameter must be a type error
    assert r["result"] == "error", r
    assert r["kind"] == "type", r
    assert "argument" in r["message"] and "mismatch" in r["message"], r


def test_compose_rewrites_component_const_in_type_binder_and_param(tmp_path):
    # コンポーネントが const をレンジ/binder/param で使うと、展開時に const は
    # alias__ でプレフィクスされるが式中の参照が書き換えられず未解決になっていた。
    comp = """
spec Counter {
  const CAP = 2
  type N = 0..CAP
  state { v: N }
  init { v = 0 }
  action bump(n in 0..CAP) {
    requires v < CAP
    v = v + 1
  }
  invariant Bound { forall k in 0..CAP { v <= CAP } }
}
"""
    compose = """
compose CounterSys {
  use Counter as c from "counter_comp.fsl"
}
"""
    (tmp_path / "counter_comp.fsl").write_text(comp, encoding="utf-8")
    cpath = tmp_path / "counter_sys.fsl"
    cpath.write_text(compose, encoding="utf-8")

    # check が通る(展開後の c__N = 0..c__CAP が解決される)
    checked = run_check(str(cpath))
    assert checked["result"] == "ok", checked

    # verify も通り、境界 invariant が効く
    ast, dn = parse_src(compose, str(tmp_path))
    spec = build_spec(ast, dn)
    vr = verify(spec, 5)
    assert vr["result"] in ("verified", "proved"), vr
