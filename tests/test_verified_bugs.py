from fslc import Monitor, build_spec, parse, verify
from fslc.cli import exit_code, run_check, run_scenarios, run_testgen
from fslc.explain import explain_file
from fslc.model import eval_const


def _verify_fixture(tmp_path, name, src, depth=2):
    path = tmp_path / name
    path.write_text(src, encoding="utf-8")
    return verify(build_spec(parse(path.read_text(encoding="utf-8"))), depth)


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
    assert small["weakening"]["op"] == "requires-removal"


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
