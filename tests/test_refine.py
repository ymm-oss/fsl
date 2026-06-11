"""Refinement checking (fslc refine) — DESIGN-refinement §5."""
from pathlib import Path

import pytest

from fslc import parse, build_spec, FslError
from fslc.cli import run_refine, exit_code
from fslc.parser import parse_refinement
from fslc.refine import build_refinement, refine

SPECS = Path(__file__).resolve().parent.parent / "specs"


def _load_cart():
    impl = build_spec(parse((SPECS / "cart_impl.fsl").read_text(encoding="utf-8")))
    abs_spec = build_spec(parse((SPECS / "cart_v1.fsl").read_text(encoding="utf-8")))
    mapping = build_refinement(
        parse_refinement((SPECS / "cart_refines.fsl").read_text(encoding="utf-8")),
        impl, abs_spec,
    )
    return impl, abs_spec, mapping


def test_cart_impl_refines_shopping_cart():
    r = run_refine(
        str(SPECS / "cart_impl.fsl"),
        str(SPECS / "cart_v1.fsl"),
        str(SPECS / "cart_refines.fsl"),
        depth=6,
    )
    assert r["result"] == "refines"
    assert r["impl"] == "CartImpl"
    assert r["abs"] == "ShoppingCart"
    assert r["action_map"]["reserve"] == "stutter"
    assert exit_code(r) == 0


def test_stutter_violation_when_reserve_changes_abs_stock(tmp_path):
    impl_src = (SPECS / "cart_impl.fsl").read_text(encoding="utf-8")
    impl_src = impl_src.replace(
        "impl_stock[i] = impl_stock[i] + 1\n    reserved[i] = reserved[i] + 1",
        "reserved[i] = reserved[i] + 1",
    )
    impl_file = tmp_path / "cart_impl_bad_stutter.fsl"
    impl_file.write_text(impl_src, encoding="utf-8")
    r = run_refine(str(impl_file), str(SPECS / "cart_v1.fsl"),
                   str(SPECS / "cart_refines.fsl"), depth=6)
    assert r["result"] == "refinement_failed"
    assert r["kind"] == "stutter_changed_abs"
    assert r["mismatch"]
    assert exit_code(r) == 1


def test_abs_requires_failed_when_impl_guard_loosened(tmp_path):
    impl_src = (SPECS / "cart_impl.fsl").read_text(encoding="utf-8")
    impl_src = impl_src.replace(
        "requires impl_stock[i] > reserved[i]",
        "requires impl_stock[i] > 0",
    )
    impl_file = tmp_path / "cart_impl_loose.fsl"
    impl_file.write_text(impl_src, encoding="utf-8")
    r = run_refine(str(impl_file), str(SPECS / "cart_v1.fsl"),
                   str(SPECS / "cart_refines.fsl"), depth=6)
    assert r["result"] == "refinement_failed"
    assert r["kind"] == "abs_requires_failed"
    assert exit_code(r) == 1


def test_abs_state_mismatch_on_map_sign_bug(tmp_path):
    mapping_src = (SPECS / "cart_refines.fsl").read_text(encoding="utf-8")
    mapping_src = mapping_src.replace(
        "impl_stock[i] - reserved[i]",
        "impl_stock[i] - reserved[i] - 1",
    )
    map_file = tmp_path / "cart_refines_bad_map.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")
    r = run_refine(str(SPECS / "cart_impl.fsl"), str(SPECS / "cart_v1.fsl"),
                   str(map_file), depth=6)
    assert r["result"] == "refinement_failed"
    assert r["kind"] == "abs_state_mismatch"
    assert exit_code(r) == 1


def test_init_mismatch(tmp_path):
    impl_src = (SPECS / "cart_impl.fsl").read_text(encoding="utf-8")
    impl_src = impl_src.replace("impl_stock[i] = 1", "impl_stock[i] = 0")
    impl_file = tmp_path / "cart_impl_bad_init.fsl"
    impl_file.write_text(impl_src, encoding="utf-8")
    r = run_refine(str(impl_file), str(SPECS / "cart_v1.fsl"),
                   str(SPECS / "cart_refines.fsl"), depth=4)
    assert r["result"] == "refinement_failed"
    assert r["at"] == "init"
    assert exit_code(r) == 1


def test_static_missing_map(tmp_path):
    mapping_src = (SPECS / "cart_refines.fsl").read_text(encoding="utf-8")
    mapping_src = "\n".join(
        line for line in mapping_src.splitlines()
        if not line.strip().startswith("map cart")
    )
    map_file = tmp_path / "cart_refines_missing_map.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")
    r = run_refine(str(SPECS / "cart_impl.fsl"), str(SPECS / "cart_v1.fsl"),
                   str(map_file), depth=4)
    assert r["result"] == "error"
    assert r["kind"] == "type"
    assert exit_code(r) == 2


def test_static_unknown_action(tmp_path):
    mapping_src = (SPECS / "cart_refines.fsl").read_text(encoding="utf-8")
    mapping_src = mapping_src.replace(
        "  action reserve(i)            -> stutter\n}",
        "  action reserve(i)            -> stutter\n  action ghost(i) -> stutter\n}",
    )
    map_file = tmp_path / "cart_refines_unknown.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")
    r = run_refine(str(SPECS / "cart_impl.fsl"), str(SPECS / "cart_v1.fsl"),
                   str(map_file), depth=4)
    assert r["result"] == "error"
    assert r["kind"] == "type"
    assert exit_code(r) == 2


def test_static_missing_action_correspondence(tmp_path):
    mapping_src = (SPECS / "cart_refines.fsl").read_text(encoding="utf-8")
    mapping_src = "\n".join(
        line for line in mapping_src.splitlines()
        if not line.strip().startswith("action reserve")
    )
    map_file = tmp_path / "cart_refines_missing_action.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")
    r = run_refine(str(SPECS / "cart_impl.fsl"), str(SPECS / "cart_v1.fsl"),
                   str(map_file), depth=4)
    assert r["result"] == "error"
    assert r["kind"] == "type"
    assert "reserve" in r["message"]
    assert exit_code(r) == 2


def test_map_out_of_bounds(tmp_path):
    mapping_src = (SPECS / "cart_refines.fsl").read_text(encoding="utf-8")
    mapping_src = mapping_src.replace(
        "impl_stock[i] - reserved[i]",
        "impl_stock[i] - reserved[i] + 4",
    )
    map_file = tmp_path / "cart_refines_oob.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")
    r = run_refine(str(SPECS / "cart_impl.fsl"), str(SPECS / "cart_v1.fsl"),
                   str(map_file), depth=4)
    assert r["result"] == "refinement_failed"
    assert r["kind"] == "map_out_of_bounds"
    assert exit_code(r) == 1


def test_regression_existing_verify_still_works():
    from fslc import verify
    spec = build_spec(parse((SPECS / "cart_v1.fsl").read_text(encoding="utf-8")))
    r = verify(spec, depth=4)
    assert r["result"] == "verified"
