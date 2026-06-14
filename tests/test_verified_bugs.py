from fslc import build_spec, parse, verify
from fslc.cli import exit_code, run_check, run_scenarios
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
