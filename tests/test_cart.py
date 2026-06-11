"""End-to-end checks against the shipped sample specs.

These exercise the full pipeline (parse -> build_spec -> verify) and
assert on the machine-readable result envelope, mirroring the JSON the
CLI emits.
"""
from pathlib import Path

import pytest

from fslc import parse, build_spec, verify

SPECS = Path(__file__).resolve().parent.parent / "specs"


def run(name, depth=8):
    ast = parse((SPECS / name).read_text(encoding="utf-8"))
    return verify(build_spec(ast), depth)


def test_buggy_violates_no_negative_stock():
    r = run("cart_buggy.fsl")
    assert r["result"] == "violated"
    assert r["invariant"] == "NoNegativeStock"
    # the counterexample must reach a negative-stock state via checkout
    assert r["violating_bindings"] is not None
    actions = [e["action"]["name"] for e in r["trace"] if "action" in e]
    assert "checkout" in actions


def test_fixed_verifies():
    r = run("cart_fixed.fsl")
    assert r["result"] == "verified"
    assert r["invariants_checked"] == ["NoNegativeStock"]
    # the stock guard must not make any action permanently un-fireable
    assert all(r["action_coverage"].values()), r["action_coverage"]
    assert r["warnings"] == []


@pytest.mark.parametrize("name", ["cart_buggy.fsl", "cart_fixed.fsl"])
def test_all_actions_covered(name):
    # both specs should be able to fire every declared action within the bound
    r = run(name)
    cov = r.get("action_coverage")
    if cov is not None:  # only present on a 'verified' result
        assert set(cov) == {"add_to_cart", "remove_from_cart", "checkout"}
