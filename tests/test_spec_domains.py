# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Kernel `spec` accepts `entity`/`number`, desugared to `type` via verify bounds.

This separates domain identity (an entity/number declaration) from the
verification world size (the `verify` block), so a design-layer spec reads as
documentation without the `type X = 0..N` "domain lie". The desugar reuses the
requirements-dialect path, so a spec written either way must behave identically.
"""
from fslc.cli import run_check, run_verify

DOMAIN_SRC = '''spec DomainKernel {
  entity Item
  number Qty
  state { stock: Map<Item, Qty> }
  init { forall i: Item { stock[i] = 0 } }
  fair action restock(i: Item, q: Qty) {
    requires q > 0
    stock[i] = q
  }
  invariant NonNeg { forall i: Item { stock[i] >= 0 } }
}
verify {
  instances Item = 2
  values Qty = 0..3
}
'''

# Identical model, but with the world size inlined in the types (the old style).
EXPLICIT_SRC = '''spec DomainKernel {
  type Item = 0..1
  type Qty = 0..3
  state { stock: Map<Item, Qty> }
  init { forall i: Item { stock[i] = 0 } }
  fair action restock(i: Item, q: Qty) {
    requires q > 0
    stock[i] = q
  }
  invariant NonNeg { forall i: Item { stock[i] >= 0 } }
}
'''


def _write(tmp_path, name, src):
    p = tmp_path / name
    p.write_text(src, encoding="utf-8")
    return str(p)


def test_entity_number_in_kernel_spec_matches_explicit_type(tmp_path):
    dom = _write(tmp_path, "dom.fsl", DOMAIN_SRC)
    exp = _write(tmp_path, "exp.fsl", EXPLICIT_SRC)
    assert run_check(dom)["result"] == "ok"
    # entity/number form must verify exactly like the explicit `type` form.
    assert run_verify(dom, 4, "warn")["result"] == "verified"
    assert run_verify(dom, 4, "warn")["result"] == run_verify(exp, 4, "warn")["result"]


def test_entity_without_instances_bound_is_spec_error(tmp_path):
    src = '''spec NoBound {
  entity Item
  state { seen: Map<Item, Bool> }
  init { forall i: Item { seen[i] = false } }
  fair action mark(i: Item) { seen[i] = true }
  invariant T { forall i: Item { seen[i] == true or seen[i] == false } }
}
'''
    res = run_check(_write(tmp_path, "nobound.fsl", src))
    assert res["result"] == "error"
    assert res["kind"] == "type"
    assert "has no 'instances' bound" in res["message"]


def test_number_without_values_bound_is_spec_error(tmp_path):
    src = '''spec NoVals {
  entity Item
  number Qty
  state { stock: Map<Item, Qty> }
  init { forall i: Item { stock[i] = 0 } }
  fair action restock(i: Item, q: Qty) {
    requires q > 0
    stock[i] = q
  }
  invariant NonNeg { forall i: Item { stock[i] >= 0 } }
}
verify {
  instances Item = 2
}
'''
    res = run_check(_write(tmp_path, "novals.fsl", src))
    assert res["result"] == "error"
    assert "has no 'values' bound" in res["message"]
