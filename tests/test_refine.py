"""Refinement checking (fslc refine) — DESIGN-refinement §5."""
import textwrap
from pathlib import Path

import pytest
from lark.exceptions import UnexpectedInput

from fslc import parse, build_spec, FslError
from fslc.cli import run_refine, exit_code
from fslc.parser import parse_refinement
from fslc.refine import build_refinement, refine

SPECS = Path(__file__).resolve().parent.parent / "specs"


_AUTO_ABS = """
spec AutoAbs {
  type K = 0..1
  state { same: K, logical: K }
  init { same = 0  logical = 0 }
  action bump_same(k: K) { requires same == 0  same = k }
  action bump_logical(k: K) { requires logical == 0  logical = k }
}
"""


_AUTO_IMPL = """
spec AutoImpl {
  type K = 0..1
  state { same: K, detail: K }
  init { same = 0  detail = 0 }
  action bump_same(k: K) { requires same == 0  same = k }
  action bump_logical(k: K) { requires detail == 0  detail = k }
}
"""


_AUTO_FULL_MAP = """
refinement AutoImplRefinesAbs {
  impl AutoImpl
  abs AutoAbs
  map same = same
  map logical = detail
  action bump_same(k) -> bump_same(k)
  action bump_logical(k) -> bump_logical(k)
}
"""


_AUTO_SHORTHAND_MAP = """
refinement AutoImplRefinesAbs {
  impl AutoImpl
  abs AutoAbs
  maps auto
  map logical = detail
}
"""


def _load_cart():
    impl = build_spec(parse((SPECS / "cart_impl.fsl").read_text(encoding="utf-8")))
    abs_spec = build_spec(parse((SPECS / "cart_v1.fsl").read_text(encoding="utf-8")))
    mapping = build_refinement(
        parse_refinement((SPECS / "cart_refines.fsl").read_text(encoding="utf-8")),
        impl, abs_spec,
    )
    return impl, abs_spec, mapping


def test_maps_auto_synthesizes_identity_state_and_actions(tmp_path):
    abs_path = tmp_path / "auto_abs.fsl"
    impl_path = tmp_path / "auto_impl.fsl"
    full_path = tmp_path / "full.fsl"
    auto_path = tmp_path / "auto.fsl"
    abs_path.write_text(textwrap.dedent(_AUTO_ABS), encoding="utf-8")
    impl_path.write_text(textwrap.dedent(_AUTO_IMPL), encoding="utf-8")
    full_path.write_text(textwrap.dedent(_AUTO_FULL_MAP), encoding="utf-8")
    auto_path.write_text(textwrap.dedent(_AUTO_SHORTHAND_MAP), encoding="utf-8")

    full = run_refine(str(impl_path), str(abs_path), str(full_path), depth=3)
    auto = run_refine(str(impl_path), str(abs_path), str(auto_path), depth=3)

    assert full["result"] == "refines"
    assert auto["result"] == full["result"]
    assert auto["action_map"] == full["action_map"]
    assert exit_code(auto) == 0


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


def test_refinement_action_params_accept_matching_type_annotations(tmp_path):
    mapping_src = (SPECS / "cart_refines.fsl").read_text(encoding="utf-8")
    mapping_src = mapping_src.replace(
        "action add_to_cart(u, i)     -> add_to_cart(u, i)",
        "action add_to_cart(u: UserId, i: ItemId)     -> add_to_cart(u, i)",
    ).replace(
        "action remove_from_cart(u)   -> remove_from_cart(u)",
        "action remove_from_cart(u: UserId)   -> remove_from_cart(u)",
    ).replace(
        "action impl_checkout(u)      -> checkout(u)",
        "action impl_checkout(u: UserId)      -> checkout(u)",
    ).replace(
        "action reserve(i)            -> stutter",
        "action reserve(i: ItemId)            -> stutter",
    )
    map_file = tmp_path / "cart_refines_annotated.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")

    r = run_refine(
        str(SPECS / "cart_impl.fsl"),
        str(SPECS / "cart_v1.fsl"),
        str(map_file),
        depth=6,
    )

    assert r["result"] == "refines"
    assert r["action_map"]["reserve"] == "stutter"
    assert exit_code(r) == 0


def test_refinement_action_param_type_annotation_mismatch_is_type_error(tmp_path):
    mapping_src = (SPECS / "cart_refines.fsl").read_text(encoding="utf-8")
    mapping_src = mapping_src.replace(
        "action add_to_cart(u, i)     -> add_to_cart(u, i)",
        "action add_to_cart(u: ItemId, i: ItemId)     -> add_to_cart(u, i)",
    )
    map_file = tmp_path / "cart_refines_bad_annotation.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")

    r = run_refine(
        str(SPECS / "cart_impl.fsl"),
        str(SPECS / "cart_v1.fsl"),
        str(map_file),
        depth=2,
    )

    assert r["result"] == "error"
    assert r["kind"] == "type"
    assert "type annotation mismatch" in r["message"]
    assert exit_code(r) == 2


_CONFLICTING_ENUM_ABS = """
spec ConflEnumAbs {
  type Id = 0..0
  enum Status { Open, Closed }
  state { st: Map<Id, Status> }
  init { forall c: Id { st[c] = Open } }
  fair action close(c: Id) { requires st[c] == Open  st[c] = Closed }
}
"""


_CONFLICTING_ENUM_IMPL = """
spec ConflEnumImpl {
  type Id = 0..0
  enum Status { Open, Stuck, Closed }
  state { st: Map<Id, Status> }
  init { forall c: Id { st[c] = Open } }
  action close(c: Id) { requires st[c] == Open  st[c] = Stuck }
}
"""


_CONFLICTING_ENUM_MAP = """
refinement ConflEnumRefines {
  impl ConflEnumImpl
  abs ConflEnumAbs
  map st[c: Id] = st[c]
  action close(c) -> close(c)
}
"""


def test_same_named_enum_with_different_members_is_rejected_not_merged(tmp_path):
    # Regression: impl's `Stuck` (index 1) used to get silently reinterpreted
    # as abs's `Closed` (also index 1) because _merge_types_meta merged same-
    # named enums by name only. impl never truly reaches Closed here, so a
    # "refines" verdict would be a false positive; it must be rejected instead.
    abs_file = tmp_path / "abs.fsl"
    impl_file = tmp_path / "impl.fsl"
    map_file = tmp_path / "map.fsl"
    abs_file.write_text(_CONFLICTING_ENUM_ABS, encoding="utf-8")
    impl_file.write_text(_CONFLICTING_ENUM_IMPL, encoding="utf-8")
    map_file.write_text(_CONFLICTING_ENUM_MAP, encoding="utf-8")

    r = run_refine(str(impl_file), str(abs_file), str(map_file), depth=6)

    assert r["result"] == "error"
    assert r["kind"] == "type"
    assert "Status" in r["message"]
    assert exit_code(r) == 2


def test_seat_conditional_map_refines_booking():
    r = run_refine(
        str(SPECS / "seat_booking_impl.fsl"),
        str(SPECS / "seat_booking.fsl"),
        str(SPECS / "seat_refines.fsl"),
        depth=6,
    )
    assert r["result"] == "refines"
    assert r["impl"] == "SeatBookingImpl"
    assert r["abs"] == "SeatBooking"
    assert r["action_map"]["confirm"] == "book"
    assert exit_code(r) == 0


def test_seat_wrong_action_mapping_not_refines(tmp_path):
    mapping_src = (SPECS / "seat_refines.fsl").read_text(encoding="utf-8")
    mapping_src = mapping_src.replace(
        "action confirm(s, u)  -> book(s, u)",
        "action confirm(s, u)  -> cancel(s)",
    )
    map_file = tmp_path / "seat_refines_bad_action.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")
    r = run_refine(
        str(SPECS / "seat_booking_impl.fsl"),
        str(SPECS / "seat_booking.fsl"),
        str(map_file),
        depth=6,
    )
    assert r["result"] == "refinement_failed"
    assert exit_code(r) == 1


def test_seat_conditional_map_type_mismatch_is_type_error(tmp_path):
    mapping_src = (SPECS / "seat_refines.fsl").read_text(encoding="utf-8")
    mapping_src = mapping_src.replace(
        "if slots[s].st == Sold then slots[s].holder else none",
        "if slots[s].st == Sold then slots[s].st else none",
    )
    map_file = tmp_path / "seat_refines_bad_ite_type.fsl"
    map_file.write_text(mapping_src, encoding="utf-8")
    r = run_refine(
        str(SPECS / "seat_booking_impl.fsl"),
        str(SPECS / "seat_booking.fsl"),
        str(map_file),
        depth=2,
    )
    assert r["result"] == "error"
    assert r["kind"] == "type"
    assert exit_code(r) == 2


def test_ite_syntax_is_not_allowed_in_normal_specs():
    src = """
spec BadIte {
  state { x: Int }
  init { x = if true then 1 else 0 }
}
"""
    with pytest.raises(UnexpectedInput):
        parse(src)


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


# ── Regression: violation into a terminal/deadlock state ─────────────────
# A guard-bypassing impl transition whose post-state is terminal (deadlocks
# within the bound) must still be reported. The refine unrolling formerly
# committed every step's transition into one solver, so a violating prefix that
# could not extend to the full depth was filtered out of every model and the
# violation was silently missed — detected at depth 1 (where the trace IS
# full-length) but lost at depth>=2 (non-monotone). See specs/review/ and the
# project memory `refine-deadlock-masks-violation`.

_TERMINAL_ABS = """
spec UpTerminal {
  enum St { Submitted, Approved, Rejected, Paid }
  state { s: St }
  init { s = Submitted }
  action approve() { requires s == Submitted   s = Approved }
  action reject()  { requires s == Submitted   s = Rejected }
  action pay()     { requires s == Approved     s = Paid }
}
"""

# fast_pay bypasses approval and lands directly in the terminal Paid state.
_TERMINAL_IMPL = """
spec DownTerminal {
  enum DSt { DSubmitted, DApproved, DRejected, DPaid }
  state { d: DSt }
  init { d = DSubmitted }
  action approve()  { requires d == DSubmitted   d = DApproved }
  action reject()   { requires d == DSubmitted   d = DRejected }
  action pay()      { requires d == DApproved     d = DPaid }
  action fast_pay() { requires d == DSubmitted    d = DPaid }
}
"""

_TERMINAL_MAP = """
refinement DownRefinesUp {
  impl DownTerminal
  abs  UpTerminal
  map s = if d == DSubmitted then Submitted
          else if d == DApproved then Approved
          else if d == DRejected then Rejected
          else Paid
  action approve()  -> approve()
  action reject()   -> reject()
  action pay()      -> pay()
  action fast_pay() -> pay()
}
"""


@pytest.mark.parametrize("depth", [1, 2, 3, 8])
def test_violation_into_terminal_state_detected_at_all_depths(tmp_path, depth):
    a = tmp_path / "up.fsl"; a.write_text(_TERMINAL_ABS, encoding="utf-8")
    i = tmp_path / "down.fsl"; i.write_text(_TERMINAL_IMPL, encoding="utf-8")
    mp = tmp_path / "map.fsl"; mp.write_text(_TERMINAL_MAP, encoding="utf-8")
    r = run_refine(str(i), str(a), str(mp), depth=depth)
    assert r["result"] == "refinement_failed", f"violation missed at depth {depth}"
    assert r["kind"] == "abs_requires_failed"
    assert r["impl_action"]["name"] == "fast_pay"
    assert exit_code(r) == 1


def test_terminal_violation_does_not_overflag_legitimate_refinement(tmp_path):
    # Same shape, but fast_pay removed: the impl is a faithful refinement and
    # must still report `refines` (the fix must not introduce false positives).
    impl = _TERMINAL_IMPL.replace(
        "  action fast_pay() { requires d == DSubmitted    d = DPaid }\n", "")
    mp_src = _TERMINAL_MAP.replace("  action fast_pay() -> pay()\n", "")
    a = tmp_path / "up.fsl"; a.write_text(_TERMINAL_ABS, encoding="utf-8")
    i = tmp_path / "down.fsl"; i.write_text(impl, encoding="utf-8")
    mp = tmp_path / "map.fsl"; mp.write_text(mp_src, encoding="utf-8")
    r = run_refine(str(i), str(a), str(mp), depth=8)
    assert r["result"] == "refines"
    assert exit_code(r) == 0
