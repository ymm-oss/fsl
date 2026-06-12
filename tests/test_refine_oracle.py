from __future__ import annotations

import textwrap
import copy
from pathlib import Path
from typing import Any

import pytest

from fslc.cli import run_refine
from fslc.model import domain_range
from fslc.parser import parse_refinement
from fslc.runtime import (
    Monitor,
    _as_bool,
    _empty_phys_state,
    _eval_requires,
    compute_updates,
    eval_concrete,
    phys_to_logical,
)
from oracle import ROOT, normalize


SPECS = ROOT / "specs"
GALLERY = ROOT / "examples" / "gallery"


REFINE_CASES = [
    (SPECS / "cart_impl.fsl", SPECS / "cart_v1.fsl", SPECS / "cart_refines.fsl", 4, "refines", None),
    (SPECS / "seat_booking_impl.fsl", SPECS / "seat_booking.fsl", SPECS / "seat_refines.fsl", 4, "refines", None),
    (SPECS / "bank_impl.fsl", SPECS / "bank.fsl", SPECS / "bank_refines.fsl", 4, "refines", None),
    (ROOT / "examples" / "e2e" / "3_design.fsl", ROOT / "examples" / "e2e" / "2_requirements.fsl", ROOT / "examples" / "e2e" / "3_refines_2.fsl", 3, "refines", None),
    (ROOT / "examples" / "consulting" / "tobe_expense.fsl", ROOT / "examples" / "consulting" / "asis_expense.fsl", ROOT / "examples" / "consulting" / "tobe_refines_asis.fsl", 3, "refines", None),
    (ROOT / "examples" / "layers" / "return_impl.fsl", ROOT / "examples" / "layers" / "return_system.fsl", ROOT / "examples" / "layers" / "return_impl_refines.fsl", 3, "refines", None),
    (GALLERY / "errors" / "refinement_failed_impl.fsl", GALLERY / "errors" / "refinement_failed_abs.fsl", GALLERY / "errors" / "refinement_failed_map.fsl", 3, "refinement_failed", "abs_requires_failed"),
    (GALLERY / "adversarial" / "refine_mapping_boundary_impl.fsl", GALLERY / "adversarial" / "refine_mapping_boundary_abs.fsl", GALLERY / "adversarial" / "refine_mapping_boundary_map.fsl", 2, "refinement_failed", "abs_state_mismatch"),
]


class RefineOracleFailure(Exception):
    def __init__(self, kind: str, detail: dict[str, Any]):
        self.kind = kind
        self.detail = detail
        super().__init__(kind)


def _mapping(path: Path) -> dict[str, Any]:
    _, _, items = parse_refinement(path.read_text(encoding="utf-8"))
    maps = {}
    actions = {}
    for item in items:
        if item[0] == "map":
            _, logical, binder, expr, _ = item
            maps[logical] = {"binder": binder, "expr": expr}
        elif item[0] == "action_map":
            _, impl_action, params, target, _ = item
            actions[impl_action] = {"params": list(params), "target": target}
    return {"maps": maps, "actions": actions}


def _merged_spec(impl_spec, abs_spec):
    return {**impl_spec, "types": {**impl_spec["types"], **abs_spec["types"]}}


def _reduce_ite(expr, state, binds, spec):
    if not isinstance(expr, tuple):
        return expr
    if expr[0] == "ite":
        cond = _as_bool(_eval_map_expr(expr[1], state, binds, spec))
        return _reduce_ite(expr[2] if cond else expr[3], state, binds, spec)
    out = []
    for child in expr:
        if isinstance(child, tuple):
            out.append(_reduce_ite(child, state, binds, spec))
        elif isinstance(child, list):
            out.append([_reduce_ite(v, state, binds, spec) for v in child])
        elif isinstance(child, dict):
            out.append({k: _reduce_ite(v, state, binds, spec) for k, v in child.items()})
        else:
            out.append(child)
    return tuple(out)


def _eval_map_expr(expr, impl_phys, binds, spec):
    return eval_concrete(_reduce_ite(expr, impl_phys, binds, spec), impl_phys, binds, spec)


def _domain(ty, spec):
    lo, hi = domain_range(ty, spec["types"])
    return range(lo, hi + 1)


def _assign_scalar(out, logical, ty, value, spec, prefix=None):
    name = logical if prefix is None else prefix
    if ty[0] in {"int", "domain", "enum", "bool"}:
        out[name] = value
    elif ty[0] == "option":
        if value == ("none",):
            out[f"{name}__present"] = False
            out[f"{name}__value"] = 0
        else:
            assert value[0] == "option_val"
            out[f"{name}__present"] = value[1]
            out[f"{name}__value"] = value[2]
    elif ty[0] == "struct":
        assert value[0] == "struct_val"
        for field, fty in spec["types"][ty[1]]["fields"].items():
            _assign_scalar(out, logical, fty, value[2][field], spec, prefix=f"{name}__{field}")
    elif ty[0] == "set":
        out[name] = value[1] if isinstance(value, tuple) and value[0] == "set_val" else value
    elif ty[0] == "seq":
        assert value[0] == "seq_val"
        out[f"{name}__data"] = value[1]
        out[f"{name}__len"] = value[2]
    else:
        raise AssertionError(f"unsupported alpha scalar type: {ty}")


def _assign_map_value(out, logical, key, value_ty, value, spec):
    if value_ty[0] in {"int", "domain", "enum", "bool"}:
        out[logical][key] = value
    elif value_ty[0] == "option":
        if value == ("none",):
            out[f"{logical}__present"][key] = False
        else:
            assert value[0] == "option_val"
            out[f"{logical}__present"][key] = value[1]
            out[f"{logical}__value"][key] = value[2]
    elif value_ty[0] == "struct":
        assert value[0] == "struct_val"
        for field, fty in spec["types"][value_ty[1]]["fields"].items():
            if fty[0] == "option":
                fv = value[2][field]
                if fv == ("none",):
                    out[f"{logical}__{field}__present"][key] = False
                else:
                    assert fv[0] == "option_val"
                    out[f"{logical}__{field}__present"][key] = fv[1]
                    out[f"{logical}__{field}__value"][key] = fv[2]
            else:
                out[f"{logical}__{field}"][key] = value[2][field]
    else:
        raise AssertionError(f"unsupported alpha map value type: {value_ty}")


def _alpha_phys(impl_mon: Monitor, abs_spec, mapping):
    impl_spec = impl_mon.spec
    merged = _merged_spec(impl_spec, abs_spec)
    out = _empty_phys_state(abs_spec)
    for logical, ty in abs_spec["state"].items():
        entry = mapping["maps"][logical]
        if entry["binder"] is None:
            value = _eval_map_expr(entry["expr"], impl_mon._phys, {}, merged)  # noqa: SLF001
            _assign_scalar(out, logical, ty, value, abs_spec)
            continue
        assert ty[0] == "map"
        _, binder_name, _, _ = entry["binder"]
        for key in _domain(ty[1], abs_spec):
            value = _eval_map_expr(entry["expr"], impl_mon._phys, {binder_name: key}, merged)  # noqa: SLF001
            _assign_map_value(out, logical, key, ty[2], value, abs_spec)
    return out


def _check_abs_bounds(abs_spec, alpha_phys, step):
    for inv in abs_spec["invariants"]:
        if inv.get("implicit") and not _as_bool(eval_concrete(inv["expr"], alpha_phys, {}, abs_spec)):
            raise RefineOracleFailure("map_out_of_bounds", {"step": step, "invariant": inv["name"]})


def _abs_action(abs_spec, name):
    for act in abs_spec["actions"]:
        if act["name"] == name:
            return act
    raise AssertionError(f"unknown abstract action {name}")


def _abs_binds(target, impl_inst, impl_phys, impl_binds, impl_spec, abs_spec):
    _, abs_name, arg_exprs = target
    act = _abs_action(abs_spec, abs_name)
    merged = _merged_spec(impl_spec, abs_spec)
    binds = {}
    for param, expr in zip(act["params"], arg_exprs):
        binds[param[0]] = _eval_map_expr(expr, impl_phys, impl_binds, merged)
    return act, binds


def refine_oracle(
    impl_path: Path,
    abs_path: Path,
    map_path: Path,
    depth: int,
    max_states: int = 500,
) -> dict[str, Any]:
    impl0 = Monitor(impl_path)
    impl0.reset()
    abs_spec = Monitor(abs_path).spec
    mapping = _mapping(map_path)

    alpha0 = _alpha_phys(impl0, abs_spec, mapping)
    _check_abs_bounds(abs_spec, alpha0, 0)

    queue = [(impl0, 0)]
    visited = {normalize(impl0.state)}
    while queue:
        if len(visited) > max_states:
            raise AssertionError(f"refinement oracle state cap exceeded: {len(visited)} > {max_states}")
        impl_mon, step = queue.pop(0)
        before = _alpha_phys(impl_mon, abs_spec, mapping)
        _check_abs_bounds(abs_spec, before, step)
        if step >= depth:
            continue
        for enabled in impl_mon.enabled():
            child = copy.deepcopy(impl_mon)
            impl_inst, bad = child._find_action(enabled["action"], enabled.get("params", {}))  # noqa: SLF001
            assert bad is None
            result = child.step(enabled["action"], enabled.get("params", {}))
            assert result.get("ok"), result

            after = _alpha_phys(child, abs_spec, mapping)
            action_map = mapping["actions"][impl_inst["action"]]
            if action_map["target"][0] == "stutter":
                if normalize(phys_to_logical(before, abs_spec)) != normalize(phys_to_logical(after, abs_spec)):
                    raise RefineOracleFailure("stutter_changed_abs", {"step": step + 1, "action": enabled})
            else:
                abs_act, binds = _abs_binds(
                    action_map["target"], impl_inst, impl_mon._phys, impl_inst["binds"], impl_mon.spec, abs_spec  # noqa: SLF001
                )
                guards_ok, binds, _ = _eval_requires(abs_act["requires"], abs_act["lets"], before, binds, abs_spec)
                if guards_ok is not True:
                    raise RefineOracleFailure("abs_requires_failed", {"step": step + 1, "action": enabled})
                expected = dict(before)
                expected.update(compute_updates(abs_act["stmts"], before, binds, abs_spec))
                if normalize(phys_to_logical(expected, abs_spec)) != normalize(phys_to_logical(after, abs_spec)):
                    raise RefineOracleFailure("abs_state_mismatch", {"step": step + 1, "action": enabled})

            _check_abs_bounds(abs_spec, after, step + 1)
            key = normalize(child.state)
            if key not in visited:
                visited.add(key)
                queue.append((child, step + 1))
    return {"result": "refines"}


@pytest.mark.parametrize(
    "impl,abs_spec,mapping,depth,expected,kind",
    REFINE_CASES,
    ids=[case[2].relative_to(ROOT).as_posix() for case in REFINE_CASES],
)
def test_refine_matches_independent_runtime_oracle(impl, abs_spec, mapping, depth, expected, kind):
    cli_result = run_refine(str(impl), str(abs_spec), str(mapping), depth)
    try:
        oracle_result = refine_oracle(impl, abs_spec, mapping, depth)
    except RefineOracleFailure as exc:
        oracle_result = {"result": "refinement_failed", "kind": exc.kind, **exc.detail}

    assert cli_result["result"] == expected
    assert oracle_result["result"] == expected
    if kind is not None:
        assert cli_result["kind"] == kind
        assert oracle_result["kind"] == kind


DEADLOCK_BUG_FIXTURES = {
    "mismatch": (
        """
        spec AbsDeadlockMismatch {
          type K = 0..0
          state { x: Bool }
          init { x = false }
          action set(k: K) { requires x == false  x = true }
        }
        """,
        """
        spec ImplDeadlockMismatch {
          type K = 0..0
          state { y: Bool }
          init { y = false }
          action bad(k: K) { requires y == false  y = true }
        }
        """,
        """
        refinement DeadlockMismatch {
          impl ImplDeadlockMismatch
          abs AbsDeadlockMismatch
          map x = false
          action bad(k) -> set(k)
        }
        """,
        "abs_state_mismatch",
    ),
    "requires": (
        """
        spec AbsDeadlockRequires {
          type K = 0..0
          state { x: Bool }
          init { x = false }
          action need_true(k: K) { requires x == true  x = false }
        }
        """,
        """
        spec ImplDeadlockRequires {
          type K = 0..0
          state { y: Bool }
          init { y = false }
          action bad(k: K) { requires y == false  y = true }
        }
        """,
        """
        refinement DeadlockRequires {
          impl ImplDeadlockRequires
          abs AbsDeadlockRequires
          map x = false
          action bad(k) -> need_true(k)
        }
        """,
        "abs_requires_failed",
    ),
    "stutter": (
        """
        spec AbsDeadlockStutter {
          type K = 0..0
          state { x: Bool }
          init { x = false }
          action impossible(k: K) { requires x == true  x = false }
        }
        """,
        """
        spec ImplDeadlockStutter {
          type K = 0..0
          state { y: Bool }
          init { y = false }
          action internal(k: K) { requires y == false  y = true }
        }
        """,
        """
        refinement DeadlockStutter {
          impl ImplDeadlockStutter
          abs AbsDeadlockStutter
          map x = y
          action internal(k) -> stutter
        }
        """,
        "stutter_changed_abs",
    ),
}


@pytest.mark.parametrize("name", sorted(DEADLOCK_BUG_FIXTURES))
def test_depth_short_of_deadlock_regressions_are_not_vacuous(tmp_path, name):
    abs_src, impl_src, map_src, kind = DEADLOCK_BUG_FIXTURES[name]
    abs_path = tmp_path / f"{name}_abs.fsl"
    impl_path = tmp_path / f"{name}_impl.fsl"
    map_path = tmp_path / f"{name}_map.fsl"
    abs_path.write_text(textwrap.dedent(abs_src), encoding="utf-8")
    impl_path.write_text(textwrap.dedent(impl_src), encoding="utf-8")
    map_path.write_text(textwrap.dedent(map_src), encoding="utf-8")

    cli_result = run_refine(str(impl_path), str(abs_path), str(map_path), depth=4)
    with pytest.raises(RefineOracleFailure) as excinfo:
        refine_oracle(impl_path, abs_path, map_path, depth=4)

    assert cli_result["result"] == "refinement_failed", cli_result
    assert cli_result["kind"] == kind
    assert excinfo.value.kind == kind
