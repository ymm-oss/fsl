# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Deterministic tag-drift signals and declaration-level review export."""
from __future__ import annotations

import re

from ..render import expr_to_text


TAG_REVIEW_SCHEMA_VERSION = "tag-review.v0"
_BACKTICK_IDENTIFIER = re.compile(r"`([A-Za-z_][A-Za-z0-9_]*)`")
_BARE_IDENTIFIER = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\b")


def _expression_names(node):
    names = set()

    def visit(value):
        if isinstance(value, tuple):
            if value and value[0] == "var":
                names.add(value[1])
                return
            if value and value[0] == "index" and isinstance(value[1], str):
                names.add(value[1])
            for part in value[1:]:
                visit(part)
        elif isinstance(value, list):
            for part in value:
                visit(part)
        elif isinstance(value, dict):
            for part in value.values():
                visit(part)

    visit(node)
    return names


def _statement_effects(stmts, display_names):
    effects = []

    def target_text(lvalue):
        if not isinstance(lvalue, tuple):
            return str(lvalue)
        if lvalue[0] == "var":
            return lvalue[1]
        if lvalue[0] == "index":
            base = lvalue[1] if isinstance(lvalue[1], str) else expr_to_text(lvalue[1], display_names)
            return f"{base}[{expr_to_text(lvalue[2], display_names)}]"
        if lvalue[0] == "field_lv":
            return f"{target_text(lvalue[1])}.{lvalue[2]}"
        return lvalue[0]

    def visit(stmt, conditions):
        if stmt[0] == "assign":
            effects.append({
                "target": target_text(stmt[1]),
                "expression": expr_to_text(stmt[2], display_names),
                "conditions": list(conditions),
            })
        elif stmt[0] == "if":
            condition = expr_to_text(stmt[1], display_names)
            for child in stmt[2]:
                visit(child, [*conditions, condition])
            for child in stmt[3]:
                visit(child, [*conditions, f"not ({condition})"])
        elif stmt[0] == "forall_stmt":
            for child in stmt[2]:
                visit(child, conditions)

    for statement in stmts:
        visit(statement, [])
    return effects


def _catalog(spec):
    states = set(spec.get("state") or {})
    actions = {item["name"] for item in spec.get("actions") or []}
    constants = set(spec.get("consts") or {})
    enums = set()
    types = set(spec.get("types") or {})
    for info in (spec.get("types") or {}).values():
        enums.update(info.get("members") or [])
    return {
        "states": states,
        "actions": actions,
        "constants": constants,
        "types": types,
        "enum_members": enums,
        "all": states | actions | constants | types | enums,
    }


def _property_declaration(kind, item, spec):
    display_names = spec.get("display_names") or {}
    if kind == "leadsTo":
        expressions = [item.get("P"), item.get("Q"), item.get("decreases")]
        formal = {
            "premise": expr_to_text(item.get("P"), display_names),
            "consequence": expr_to_text(item.get("Q"), display_names),
        }
        if item.get("within") is not None:
            formal["within"] = item["within"]
        if item.get("decreases") is not None:
            formal["decreases"] = expr_to_text(item["decreases"], display_names)
    else:
        expressions = [item.get("expr")]
        formal = {"expression": expr_to_text(item.get("expr"), display_names)}
    identifiers = set()
    for expression in expressions:
        identifiers.update(_expression_names(expression))
    return {
        "kind": kind,
        "name": item["name"],
        "node_id": f"{kind}:{item['name']}",
        "tag": item.get("meta"),
        "loc": item.get("loc"),
        "formal_definition": formal,
        "formal_identifiers": sorted(identifiers),
    }


def tagged_declarations(spec):
    """Return tagged user declarations in deterministic source-model order."""
    declarations = []
    display_names = spec.get("display_names") or {}
    for action in spec.get("actions") or []:
        if not action.get("meta") or action.get("generated"):
            continue
        expressions = list(action.get("requires") or []) + list(action.get("ensures") or [])
        identifiers = set(_expression_names(action.get("stmts") or []))
        for expression in expressions:
            identifiers.update(_expression_names(expression))
        identifiers.update(param["name"] for param in action.get("params") or [])
        declarations.append({
            "kind": "action",
            "name": action["name"],
            "node_id": f"action:{action['name']}",
            "tag": action["meta"],
            "loc": action.get("loc"),
            "formal_definition": {
                "parameters": [param["name"] for param in action.get("params") or []],
                "requires": [expr_to_text(expr, display_names) for expr in action.get("requires") or []],
                "ensures": [expr_to_text(expr, display_names) for expr in action.get("ensures") or []],
                "effects": _statement_effects(action.get("stmts") or [], display_names),
            },
            "formal_identifiers": sorted(identifiers),
        })
    for kind, key in (
        ("invariant", "user_invariants"),
        ("trans", "transitions"),
        ("reachable", "reachables"),
        ("leadsTo", "leadstos"),
    ):
        for item in spec.get(key) or []:
            if item.get("meta"):
                declarations.append(_property_declaration(kind, item, spec))
    return sorted(declarations, key=lambda item: (item["kind"], item["name"]))


def _tag_identifiers(text, catalog, local_names):
    explicit = set(_BACKTICK_IDENTIFIER.findall(text))
    bare = set(_BARE_IDENTIFIER.findall(text))
    known = (explicit | bare).intersection(catalog["all"] | local_names)
    code_shaped = {
        token for token in explicit | bare
        if token in explicit or "_" in token or (token.isupper() and len(token) > 2)
    }
    stale = code_shaped - catalog["all"] - local_names
    return known, stale


def tag_drift_candidates(spec):
    catalog = _catalog(spec)
    findings = []
    relevant_formula_names = catalog["states"] | catalog["constants"]
    for declaration in tagged_declarations(spec):
        tag = declaration.get("tag") or {}
        text = tag.get("text") or ""
        local_names = set(declaration["formal_identifiers"]) - catalog["all"]
        mentioned, stale = _tag_identifiers(text, catalog, local_names)
        if stale:
            findings.append({
                "finding_type": "tag_stale_reference",
                "declaration": declaration,
                "identifiers": sorted(stale),
            })
        disjoint = sorted(
            (mentioned & relevant_formula_names)
            - set(declaration["formal_identifiers"])
        )
        if disjoint:
            findings.append({
                "finding_type": "tag_formula_disjoint",
                "declaration": declaration,
                "identifiers": disjoint,
            })
    return findings


def export_tag_review(spec):
    return {
        "analysis": "tag_review",
        "export": "tag-review",
        "schema_version": TAG_REVIEW_SCHEMA_VERSION,
        "review_contract": {
            "unit": "declaration",
            "decision": "compare tag.text with formal_definition",
            "formal_status": "not_a_violation",
            "meaning_judgment": "external_review_required",
        },
        "declarations": tagged_declarations(spec),
    }
