# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Regression tests for the raw-tree LSP index."""
from pathlib import Path

from fslc.lsp.index import build_index, default_load_index


ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"


def _slice(source, rng):
    lines = source.splitlines()
    if rng.start.line == rng.end.line:
        return lines[rng.start.line][rng.start.character:rng.end.character]
    parts = [lines[rng.start.line][rng.start.character:]]
    for line in range(rng.start.line + 1, rng.end.line):
        parts.append(lines[line])
    parts.append(lines[rng.end.line][:rng.end.character])
    return "\n".join(parts)


def _symbol(index, name, role):
    matches = [sym for sym in index.symbols if sym.name == name and sym.role == role]
    assert matches, f"missing symbol {role}:{name}"
    return matches[0]


def _reference(index, name, role, qualifier=None):
    matches = [
        ref for ref in index.references
        if ref.name == name and ref.role == role and ref.qualifier == qualifier
    ]
    assert matches, f"missing reference {role}:{qualifier or ''}.{name}"
    return matches[0]


def test_raw_tree_index_extracts_spec_definitions_references_and_ranges():
    path = SPECS / "cart_v1.fsl"
    source = path.read_text(encoding="utf-8")
    index = build_index(source, str(path))

    assert _symbol(index, "ShoppingCart", "spec").detail == "spec"
    stock = _symbol(index, "stock", "state_var")
    checkout = _symbol(index, "checkout", "action")
    sold_out = _symbol(index, "SoldOut", "property")

    assert _slice(source, stock.selection_range) == "stock"
    assert _slice(source, checkout.selection_range) == "checkout"
    assert _slice(source, sold_out.selection_range) == "SoldOut"

    stock_ref = _reference(index, "stock", "value")
    stock_loc = index.resolve_reference(stock_ref)
    assert stock_loc is not None
    assert stock_loc.path == str(path.resolve())
    assert stock_loc.range == stock.selection_range

    item_type = _symbol(index, "ItemId", "type")
    item_ref = _reference(index, "ItemId", "type")
    item_loc = index.resolve_reference(item_ref)
    assert item_loc is not None
    assert item_loc.range == item_type.selection_range

    init_i = next(
        sym for sym in index.symbols
        if sym.name == "i" and sym.role == "binder" and sym.scope_range is not None
    )
    init_i_ref = next(
        ref for ref in index.references
        if ref.name == "i"
        and ref.role == "value"
        and init_i.scope_range is not None
        and init_i.scope_range.contains_range_start(ref.range)
    )
    init_i_loc = index.resolve_reference(init_i_ref)
    assert init_i_loc is not None
    assert init_i_loc.range == init_i.selection_range


def test_compose_definition_resolution_crosses_use_imports():
    path = SPECS / "order_system.fsl"
    source = path.read_text(encoding="utf-8")
    index = build_index(source, str(path))

    assert {binding.alias: binding.path for binding in index.imports} == {
        "cart": "cart_v1.fsl",
        "pay": "payment.fsl",
    }

    checkout_ref = _reference(index, "checkout", "action", "cart")
    assert _slice(source, checkout_ref.range) == "checkout"
    checkout_loc = index.resolve_reference(checkout_ref, default_load_index)
    assert checkout_loc is not None
    assert checkout_loc.path == str((SPECS / "cart_v1.fsl").resolve())
    assert _slice((SPECS / "cart_v1.fsl").read_text(encoding="utf-8"), checkout_loc.range) == "checkout"

    pay_id_ref = _reference(index, "PayId", "type", "pay")
    pay_id_loc = index.resolve_reference(pay_id_ref, default_load_index)
    assert pay_id_loc is not None
    assert pay_id_loc.path == str((SPECS / "payment.fsl").resolve())
    assert _slice((SPECS / "payment.fsl").read_text(encoding="utf-8"), pay_id_loc.range) == "PayId"

    payments_ref = _reference(index, "payments", "value", "pay")
    payments_loc = index.resolve_reference(payments_ref, default_load_index)
    assert payments_loc is not None
    assert payments_loc.path == str((SPECS / "payment.fsl").resolve())
    assert _slice((SPECS / "payment.fsl").read_text(encoding="utf-8"), payments_loc.range) == "payments"
