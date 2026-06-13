"""Deterministic spec-to-human explanation helpers."""
from __future__ import annotations

from pathlib import Path

from .bmc import _collect_partial_op_sites, _requirement, scenarios
from .model import FslError, build_spec, display_label
from .mutate import _apply, _oracle, enumerate_mutants
from .parser import parse_src


WEAKENING_OPS = {
    "requires_remove": "requires-removal",
    "assignment_remove": "assignment-removal",
    "fair_remove": "fair-removal",
}

_SOURCE_UNAVAILABLE = "source unavailable; using name/structure (component-origin or generated element)"


def _public_name(name, spec):
    label = display_label(name, spec)
    if isinstance(label, str) and "__" in label:
        return label.replace("__", ".")
    return label


def _public_type(ty):
    if isinstance(ty, tuple):
        return [_public_type(x) for x in ty]
    if isinstance(ty, list):
        return [_public_type(x) for x in ty]
    if isinstance(ty, dict):
        return {k.replace("__", ".") if isinstance(k, str) else k: _public_type(v)
                for k, v in ty.items() if k != "ty"}
    if isinstance(ty, str):
        return ty.replace("__", ".")
    return ty


def _source_line(source_lines, loc):
    if not loc or not source_lines:
        return None
    line = loc.get("line")
    if not line or line < 1 or line > len(source_lines):
        return None
    text = source_lines[line - 1].strip()
    if not text:
        return None
    return text


def _requires_source(source_lines, loc, action_label):
    text = _source_line(source_lines, loc)
    if not text:
        return None
    local = action_label.split(".")[-1]
    if text.startswith("requires "):
        return text
    if text.startswith("transition ") and local in text:
        return text
    if local in text and (" by " in text or text.startswith("action ")):
        return text
    return None


def _ensures_source(source_lines, loc):
    text = _source_line(source_lines, loc)
    if text and text.startswith("ensures "):
        return text
    return None


def _property_source(source_lines, loc, kind, label):
    text = _source_line(source_lines, loc)
    if not text:
        return None
    local = label.split(".")[-1]
    prefixes = {
        "invariant": ("invariant ",),
        "reachable": ("reachable ", "goal "),
        "leadsTo": ("leadsTo ", "policy "),
    }.get(kind, ())
    if text.startswith(prefixes) and (local in text or kind == "leadsTo"):
        return text
    return None


def _requirement_from_meta(meta):
    if not meta:
        return None
    return {"id": meta["id"], "text": meta.get("text")}


def _actor(action):
    req = _requirement(action)
    text = (req or {}).get("text") or ""
    if text.startswith("by "):
        return text[3:]
    return None


def _lvalue_root(lvalue):
    if not isinstance(lvalue, tuple) or not lvalue:
        return None
    tag = lvalue[0]
    if tag == "var":
        return lvalue[1]
    if tag == "index":
        return lvalue[1]
    if tag == "field_lv":
        return _lvalue_root(lvalue[1])
    return None


def _stmt_writes(stmts, spec):
    writes = set()
    for stmt in stmts:
        if not isinstance(stmt, tuple) or not stmt:
            continue
        if stmt[0] == "assign":
            root = _lvalue_root(stmt[1])
            if root:
                writes.add(_public_name(root, spec))
        elif stmt[0] == "if":
            writes.update(_stmt_writes(stmt[2], spec))
            writes.update(_stmt_writes(stmt[3], spec))
        elif stmt[0] == "forall_stmt":
            writes.update(_stmt_writes(stmt[2], spec))
    return sorted(writes)


def _action_skeleton(action, spec, source_lines):
    label = _public_name(action["name"], spec)
    req = _requirement(action)
    out = {
        "name": label,
        "params": [
            {"name": n, "type": tname.replace("__", "."), "lo": lo, "hi": hi}
            for n, lo, hi, tname in action.get("params", [])
        ],
        "requires_text": [
            _requires_source(source_lines, r.get("loc"), label) or _SOURCE_UNAVAILABLE
            for r in action.get("requires", [])
        ],
        "writes": _stmt_writes(action.get("stmts", []), spec),
        "ensures_text": [
            _ensures_source(source_lines, e.get("loc")) or _SOURCE_UNAVAILABLE
            for e in action.get("ensures", [])
        ],
        "requirement": req,
    }
    actor = _actor(action)
    if actor:
        out["actor"] = actor
    if action.get("fair"):
        out["fair"] = True
    return out


def _property_skeleton(kind, item, spec, source_lines):
    label = _public_name(item["name"], spec)
    return {
        "kind": kind,
        "name": label,
        "body_text": (
            _property_source(source_lines, item.get("loc"), kind, label) or _SOURCE_UNAVAILABLE
        ),
        "requirement": _requirement(item),
    }


def _auto_checks(spec, source_lines):
    checks = []
    for inv in spec.get("invariants", []):
        if not inv.get("implicit"):
            continue
        checks.append({
            "kind": "type_bound",
            "name": _public_name(inv["name"], spec),
            "target": _public_name(inv.get("logical_var"), spec),
            "requirement": None,
        })
    for action in spec.get("actions", []):
        action_label = _public_name(action["name"], spec)
        for site in _collect_partial_op_sites(action):
            checks.append({
                "kind": "partial_op",
                "name": _public_name(f"_partial_{action['name']}", spec),
                "action": action_label,
                "loc": site.get("loc"),
                "text": _source_line(source_lines, site.get("loc")) or _SOURCE_UNAVAILABLE,
                "requirement": _requirement(action),
            })
    return checks


def _skeleton(spec, source_lines):
    properties = []
    properties.extend(_property_skeleton("invariant", inv, spec, source_lines)
                      for inv in spec.get("user_invariants", []))
    properties.extend(_property_skeleton("leadsTo", lt, spec, source_lines)
                      for lt in spec.get("leadstos", []))
    properties.extend(_property_skeleton("reachable", reach, spec, source_lines)
                      for reach in spec.get("reachables", []))
    return {
        "state": {
            _public_name(name, spec): _public_type(ty)
            for name, ty in spec.get("state", {}).items()
        },
        "actions": [_action_skeleton(a, spec, source_lines)
                    for a in spec.get("actions", [])],
        "properties": properties,
        "auto_checks": _auto_checks(spec, source_lines),
    }


def _trace_len(result):
    trace = result.get("trace") or []
    return len(trace)


def _weakening(mutant, source_lines):
    return {
        "op": WEAKENING_OPS.get(mutant.op, mutant.op.replace("_", "-")),
        "loc": mutant.loc,
        "target": mutant.target.replace("__", ".") if isinstance(mutant.target, str) else mutant.target,
        "source_text": _source_line(source_lines, mutant.loc),
    }


def _counterfactuals(ast, display_names, spec, source_lines, depth, max_mutants):
    _, name, items = ast
    invariant_labels = {
        _public_name(inv["name"], spec): inv for inv in spec.get("user_invariants", [])
    }
    found = {}
    reachable_kills = []
    op_priority = {"assignment_remove": 0, "requires_remove": 1, "fair_remove": 2}

    for idx, mutant in enumerate(enumerate_mutants(ast, display_names)):
        if mutant.op not in WEAKENING_OPS:
            continue
        if mutant.action == "init":
            continue
        if max_mutants is not None and idx >= max_mutants:
            break
        try:
            mutated_spec = build_spec(("spec", name, _apply(items, mutant)), display_names)
            oracle = _oracle(mutated_spec, depth, source_lines=source_lines)
        except FslError:
            continue
        if oracle.get("clean"):
            continue
        result = oracle.get("result") or {}
        killed_by = oracle.get("killed_by")
        if killed_by in invariant_labels:
            current = found.get(killed_by)
            candidate_key = (
                _trace_len(result),
                op_priority.get(mutant.op, 99),
                (mutant.loc or {}).get("line") or 10**9,
            )
            if current is None or candidate_key < current["_key"]:
                found[killed_by] = {
                    "_key": candidate_key,
                    "invariant": killed_by,
                    "weakening": _weakening(mutant, source_lines),
                    "trace": result.get("trace"),
                    "requirement": _requirement(invariant_labels[killed_by]),
                    "violation": {
                        k: v for k, v in result.items()
                        if k not in ("trace", "spec", "result")
                    },
                }
        elif result.get("result") == "reachable_failed":
            reachable_kills.append({
                "property": killed_by,
                "weakening": _weakening(mutant, source_lines),
                "result": "reachable_failed",
                "requirement": oracle.get("killer_requirement"),
            })

    out = []
    for inv_label, inv in invariant_labels.items():
        if inv_label in found:
            item = dict(found[inv_label])
            item.pop("_key", None)
            out.append(item)
        else:
            out.append({
                "invariant": inv_label,
                "weakening": None,
                "trace": None,
                "requirement": _requirement(inv),
                "note": f"no counterfactual within depth {depth}",
            })
    return out, reachable_kills


def _narrate_steps(steps):
    lines = []
    for idx, step in enumerate(steps or [], start=1):
        action = step.get("action")
        params = step.get("params") or {}
        rendered = ", ".join(f"{k}={v}" for k, v in params.items())
        lines.append(f"{idx}. {action}({rendered})" if rendered else f"{idx}. {action}()")
    return lines


def _witnesses(spec, source_lines, depth):
    result = scenarios(spec, depth, deadlock_mode="warn", source_lines=source_lines)
    if result.get("result") != "scenarios":
        return []
    reqs = {}
    for item in spec.get("reachables", []) + spec.get("leadstos", []):
        reqs[_public_name(item["name"], spec)] = _requirement(item)
    for action in spec.get("actions", []):
        reqs[_public_name(action["name"], spec)] = _requirement(action)
    witnesses = []
    for scenario in result.get("scenarios", []):
        key = scenario.get("property") or scenario.get("action") or scenario.get("final_check")
        witnesses.append({
            "name": scenario.get("name"),
            "kind": scenario.get("kind"),
            "target": key,
            "requirement": reqs.get(key),
            "steps": scenario.get("steps", []),
            "narration": _narrate_steps(scenario.get("steps", [])),
            "initial_state": scenario.get("initial_state"),
            "expected_states": scenario.get("expected_states", []),
        })
    return witnesses


def explain_file(file, depth=8, max_mutants=None):
    src = Path(file).read_text(encoding="utf-8")
    ast, display_names = parse_src(src, str(Path(file).parent))
    if ast[0] != "spec":
        raise FslError("explain expects a spec-like FSL file", kind="semantics")
    source_lines = src.splitlines()
    spec = build_spec(ast, display_names)
    counterfactuals, reachable_kills = _counterfactuals(
        ast, display_names, spec, source_lines, depth, max_mutants
    )
    out = {
        "result": "explained",
        "spec": spec["name"],
        "depth": depth,
        "skeleton": _skeleton(spec, source_lines),
        "counterfactuals": counterfactuals,
        "witnesses": _witnesses(spec, source_lines, depth),
    }
    if reachable_kills:
        out["reachable_counterfactuals"] = reachable_kills
    return out
