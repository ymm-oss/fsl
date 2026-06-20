# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Deterministic spec-to-human explanation helpers."""
from __future__ import annotations

import re
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
_TRANSITION_ACTOR_RE = re.compile(
    r"\bby\s+(.+?)(?:\s+with\b|\s+when\b|\s+set\b|\s+covers\b|$)"
)


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
        "trans": ("trans ", "unless ", "until "),
        "reachable": ("reachable ", "goal "),
        "leadsTo": ("leadsTo ", "policy ", "until "),
    }.get(kind, ())
    if text.startswith(prefixes) and (
        local in text or kind == "leadsTo" or text.startswith(("unless ", "until "))
    ):
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
    out = {
        "kind": kind,
        "name": label,
        "body_text": (
            _property_source(source_lines, item.get("loc"), kind, label) or _SOURCE_UNAVAILABLE
        ),
        "requirement": _requirement(item),
    }
    if kind == "leadsTo" and item.get("within") is not None:
        out["within"] = item["within"]
    return out


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
    properties.extend(_property_skeleton("trans", tr, spec, source_lines)
                      for tr in spec.get("transitions", []))
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


def _display_from(display_names, name):
    label = (display_names or {}).get(name, name)
    if isinstance(label, str) and "__" in label:
        return label.replace("__", ".")
    return label


def _type_ref_to_text(ty):
    if isinstance(ty, list):
        ty = tuple(ty)
    if not isinstance(ty, tuple) or not ty:
        return str(ty)
    tag = ty[0]
    if tag == "named":
        return ty[1].replace("__", ".")
    if tag == "name":
        return ty[1].replace("__", ".")
    if tag == "int":
        return "Int"
    if tag == "bool":
        return "Bool"
    if tag == "domain":
        return f"{ty[1]}..{ty[2]}"
    if tag == "enum":
        return ty[1].replace("__", ".")
    if tag == "map":
        return f"Map<{_type_ref_to_text(ty[1])}, {_type_ref_to_text(ty[2])}>"
    if tag == "option":
        return f"Option<{_type_ref_to_text(ty[1])}>"
    if tag == "set":
        return f"Set<{_type_ref_to_text(ty[1])}>"
    if tag == "seq":
        return f"Seq<{_type_ref_to_text(ty[1])}, {ty[2]}>"
    if tag == "struct":
        return ty[1].replace("__", ".")
    return str(ty)


def _state_type_text(name, ty, spec):
    refs = spec.get("state_type_refs") or {}
    if name in refs:
        return _type_ref_to_text(refs[name])
    return _type_ref_to_text(_public_type(ty))


def _map_key_domain_names(ty):
    if isinstance(ty, list):
        ty = tuple(ty)
    if not isinstance(ty, tuple) or not ty:
        return set()
    tag = ty[0]
    if tag == "map":
        out = set()
        key = ty[1]
        if isinstance(key, list):
            key = tuple(key)
        if isinstance(key, tuple) and key and key[0] in ("named", "name"):
            out.add(key[1])
        out.update(_map_key_domain_names(ty[2]))
        return out
    if tag in ("option", "set", "seq"):
        return _map_key_domain_names(ty[1])
    return set()


def _entity_domain_names(spec):
    out = set()
    for ty in (spec.get("state_type_refs") or {}).values():
        out.update(_map_key_domain_names(ty))
    return out


def _verification_world_lines(spec):
    entities = _entity_domain_names(spec)
    lines = []
    for name, info in sorted((spec.get("types") or {}).items()):
        if info.get("kind") != "domain":
            continue
        lo = info.get("lo")
        hi = info.get("hi")
        if lo is None or hi is None:
            continue
        count = hi - lo + 1
        public = name.replace("__", ".")
        if name in entities:
            lines.append(f"{public}: {count} instances ({lo}..{hi})")
        else:
            lines.append(f"{public}: values {lo}..{hi} ({count} values)")
    return lines


def _params_text(params, include_types=True):
    rendered = []
    for param in params or []:
        if isinstance(param, dict):
            name = param.get("name")
            ty = param.get("type")
        elif isinstance(param, tuple) and param and param[0] == "refinement_param":
            name = param[1]
            ty = _type_ref_to_text(param[2]) if param[2] is not None else None
        else:
            name = str(param)
            ty = None
        if include_types and ty:
            rendered.append(f"{name}: {ty}")
        else:
            rendered.append(str(name))
    return ", ".join(rendered)


def _requirement_text(req):
    if not req:
        return None
    rid = req.get("id")
    text = req.get("text")
    if rid and text:
        return f"{{{rid}: {text}}}"
    if rid:
        return f"{{{rid}}}"
    if text:
        return f"{{{text}}}"
    return None


def _dedupe(items):
    out = []
    seen = set()
    for item in items or []:
        if item in seen:
            continue
        seen.add(item)
        out.append(item)
    return out


def _actor_from_action(action):
    if action.get("actor"):
        return action["actor"]
    for text in action.get("requires_text") or []:
        match = _TRANSITION_ACTOR_RE.search(text)
        if match:
            return " ".join(match.group(1).split())
    return None


def _expr_to_text(expr, display_names=None):
    if not isinstance(expr, tuple):
        return str(expr)
    tag = expr[0]
    if tag == "var":
        return _display_from(display_names, expr[1])
    if tag == "num":
        return str(expr[1])
    if tag == "bool":
        return "true" if expr[1] else "false"
    if tag == "none":
        return "none"
    if tag == "bin":
        return f"{_expr_to_text(expr[2], display_names)} {expr[1]} {_expr_to_text(expr[3], display_names)}"
    if tag == "not":
        return f"not {_expr_to_text(expr[1], display_names)}"
    if tag == "neg":
        return f"-{_expr_to_text(expr[1], display_names)}"
    if tag == "index":
        return f"{_expr_to_text(expr[1], display_names)}[{_expr_to_text(expr[2], display_names)}]"
    if tag == "field":
        return f"{_expr_to_text(expr[1], display_names)}.{expr[2]}"
    if tag == "method":
        args = ", ".join(_expr_to_text(a, display_names) for a in expr[3])
        return f"{_expr_to_text(expr[1], display_names)}.{expr[2]}({args})"
    if tag == "some":
        return f"some({_expr_to_text(expr[1], display_names)})"
    if tag == "is":
        pat = expr[2]
        if pat[0] == "pat_none":
            return f"{_expr_to_text(expr[1], display_names)} is none"
        return f"{_expr_to_text(expr[1], display_names)} is some({pat[1]})"
    if tag == "ite":
        return (
            f"if {_expr_to_text(expr[1], display_names)} then "
            f"{_expr_to_text(expr[2], display_names)} else "
            f"{_expr_to_text(expr[3], display_names)}"
        )
    if tag in ("set_lit", "seq_lit"):
        head = "Set" if tag == "set_lit" else "Seq"
        return f"{head} {{{', '.join(_expr_to_text(e, display_names) for e in expr[1])}}}"
    if tag == "struct_lit":
        fields = ", ".join(
            f"{k}: {_expr_to_text(v, display_names)}"
            for k, v in sorted(expr[2].items())
        )
        return f"{expr[1]} {{ {fields} }}"
    if tag in ("old", "abs"):
        return f"{tag}({_expr_to_text(expr[1], display_names)})"
    if tag in ("min", "max"):
        return (
            f"{tag}({_expr_to_text(expr[1], display_names)}, "
            f"{_expr_to_text(expr[2], display_names)})"
        )
    if tag == "count":
        return f"count({expr[1]}: {expr[2]} where {_expr_to_text(expr[3], display_names)})"
    if tag == "sum":
        where = f" where {_expr_to_text(expr[4], display_names)}" if expr[4] is not None else ""
        return (
            f"sum({expr[1]}: {expr[2]} of "
            f"{_expr_to_text(expr[3], display_names)}{where})"
        )
    if tag in ("forall", "exists"):
        return f"{tag} {_binder_to_text(expr[1], display_names)}: {_expr_to_text(expr[2], display_names)}"
    return tag


def _binder_to_text(binder, display_names=None):
    if not isinstance(binder, tuple) or len(binder) < 3:
        return str(binder)
    if binder[0] == "binder_typed":
        text = f"{binder[1]}: {_type_ref_to_text(('name', binder[2]))}"
        if len(binder) > 3 and binder[3] is not None:
            text += f" where {_expr_to_text(binder[3], display_names)}"
        return text
    if binder[0] == "binder_range":
        return (
            f"{binder[1]} in {_expr_to_text(binder[2], display_names)}.."
            f"{_expr_to_text(binder[3], display_names)}"
        )
    return str(binder)


def _binder_name(binder):
    if isinstance(binder, tuple) and len(binder) > 1:
        return binder[1]
    return "?"


def _branch_lowering_lines(display_names):
    groups = {}
    for _physical, label in sorted((display_names or {}).items()):
        if not isinstance(label, str) or "[" not in label or not label.endswith("]"):
            continue
        base = label.split("[", 1)[0]
        groups.setdefault(base, []).append(label)
    return [
        f"{base} \u2192 {', '.join(labels)}"
        for base, labels in sorted(groups.items())
    ]


def _mapping_state_line(item, impl_display_names, abs_display_names):
    _, logical, binder, expr, _loc = item
    lhs = _display_from(abs_display_names, logical)
    if binder is not None:
        lhs = f"{lhs}[{_binder_name(binder)}]"
    return f"{lhs} \u21a6 {_expr_to_text(expr, impl_display_names)}"


def _mapping_action_line(item, impl_display_names, abs_display_names):
    _, aname, params, target, _loc = item
    lhs = _display_from(impl_display_names, aname)
    lhs = f"{lhs}({_params_text(params, include_types=False)})"
    if target[0] == "stutter":
        rhs = "stutter"
    else:
        _, abs_name, args = target
        rhs = _display_from(abs_display_names, abs_name)
        rhs = f"{rhs}({', '.join(_expr_to_text(arg, impl_display_names) for arg in args)})"
    return f"{lhs} \u21a6 {rhs}"


def _refinement_mapping_lines(spec, display_names):
    impl = spec.get("implements")
    if not impl or not impl.get("mapping_ast"):
        return []
    impl_display_names = spec.get("display_names") or display_names or {}
    abs_display_names = impl.get("abs_display_names") or {}
    lines = [
        f"  - implements {spec['name']} \u21a6 {impl.get('abs')}",
        "    includes explicit maps and generated-by-name correspondences; review actor/intent matches",
    ]
    state_lines = []
    action_lines = []
    for item in impl["mapping_ast"][2]:
        if item[0] == "map":
            state_lines.append(_mapping_state_line(item, impl_display_names, abs_display_names))
        elif item[0] == "action_map":
            action_lines.append(_mapping_action_line(item, impl_display_names, abs_display_names))
    if state_lines:
        lines.append("    state maps:")
        lines.extend(f"      - {line}" for line in state_lines)
    if action_lines:
        lines.append("    action correspondences:")
        lines.extend(f"      - {line}" for line in action_lines)
    return lines


def render_readable(explained: dict, spec: dict, display_names: dict) -> str:
    """Render an explain result as deterministic human-readable text."""
    skeleton = explained.get("skeleton") or {}
    lines = [f"Spec: {spec['name']} (depth {explained.get('depth')})"]

    def section(title, items):
        if not items:
            return
        lines.append("")
        lines.append(f"{title}:")
        lines.extend(items)

    section("Verification world", [f"  - {line}" for line in _verification_world_lines(spec)])

    state_items = []
    state_order = sorted(
        spec.get("state", {}),
        key=lambda internal: _public_name(internal, spec),
    )
    for internal in state_order:
        name = _public_name(internal, spec)
        state_items.append(f"  - {name}: {_state_type_text(internal, spec['state'][internal], spec)}")
    section("State", state_items)

    action_items = []
    generated_by_name = {
        _public_name(action["name"], spec): action.get("generated")
        for action in spec.get("actions", [])
    }
    for action in skeleton.get("actions") or []:
        params = _params_text(action.get("params"))
        generated = generated_by_name.get(action["name"])
        marker = []
        if action.get("fair"):
            marker.append("fair")
        if generated and generated.get("kind") == "time_tick":
            marker.append("generated by time block")
        elif action["name"] == "tick" and generated is not None:
            marker.append("generated")
        suffix = f" [{' | '.join(marker)}]" if marker else ""
        actor = _actor_from_action(action)
        actor_text = f" actor: {actor}" if actor else ""
        action_items.append(f"  - {action['name']}({params}){suffix}{actor_text}")
        req = _requirement_text(action.get("requirement"))
        if req:
            action_items.append(f"    requirement: {req}")
        requires = _dedupe(action.get("requires_text"))
        if requires:
            action_items.append("    requires:")
            action_items.extend(f"      - {text}" for text in requires)
        writes = action.get("writes") or []
        if writes:
            action_items.append(f"    writes: {', '.join(writes)}")
    section("Actions", action_items)

    kpi_items = [
        (
            f"  - {kpi['name']} = count of {kpi['entity']} in {kpi['stage']} "
            "(derived projection - not stored state)"
        )
        for kpi in sorted(spec.get("kpis") or [], key=lambda item: item["name"])
    ]
    section("Derived metrics (KPIs)", kpi_items)

    prop_items = []
    for prop in skeleton.get("properties") or []:
        req = _requirement_text(prop.get("requirement"))
        req_text = f" {req}" if req else ""
        within = f" [within {prop['within']}]" if prop.get("within") is not None else ""
        prop_items.append(f"  - {prop['kind']} {prop['name']}{within}{req_text}")
        if prop.get("body_text"):
            prop_items.append(f"    body: {prop['body_text']}")
    section("Properties", prop_items)

    check_items = []
    for check in skeleton.get("auto_checks") or []:
        if check.get("kind") == "type_bound":
            check_items.append(
                f"  - type_bound: {check.get('target')} (implicit bounded-domain check)"
            )
        elif check.get("kind") == "partial_op":
            check_items.append(
                f"  - partial_op: {check.get('action')} checks {check.get('text')}"
            )
        else:
            check_items.append(f"  - {check.get('kind')}: {check.get('name')}")
    section("Automatic checks", check_items)

    branch_items = [f"  - {line}" for line in _branch_lowering_lines(spec.get("display_names") or display_names)]
    section("Branch lowering", branch_items)

    section("Refinement mapping (synthesized + explicit)", _refinement_mapping_lines(spec, display_names))

    fair = [action["name"] for action in skeleton.get("actions") or [] if action.get("fair")]
    section("Fairness assumptions", [f"  - fair: {', '.join(fair)}"] if fair else [])

    return "\n".join(lines)


def _trace_len(result):
    trace = result.get("trace") or []
    return len(trace)


def _weakening(mutant, source_lines):
    out = {
        "op": WEAKENING_OPS.get(mutant.op, mutant.op.replace("_", "-")),
        "loc": mutant.loc,
        "target": mutant.target.replace("__", ".") if isinstance(mutant.target, str) else mutant.target,
        "source_text": _source_line(source_lines, mutant.loc),
    }
    if mutant.action == "init":
        out["origin"] = "init"
        out["label"] = "init weakening"
    return out


def _counterfactuals(ast, display_names, spec, source_lines, depth, max_mutants):
    _, name, items = ast
    invariant_labels = {
        _public_name(inv["name"], spec): inv for inv in spec.get("user_invariants", [])
    }
    found = {}
    reachable_kills = []
    op_priority = {"assignment_remove": 0, "requires_remove": 1, "fair_remove": 2}

    processed = 0
    for mutant in enumerate_mutants(ast, display_names):
        if mutant.op not in WEAKENING_OPS:
            continue
        if max_mutants is not None and processed >= max_mutants:
            break
        processed += 1
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
    for item in spec.get("acceptance", []) + spec.get("forbidden", []):
        reqs[item["id"]] = {"id": item["id"], "text": item.get("text")}
    witnesses = []
    for scenario in result.get("scenarios", []):
        keys = [
            scenario.get("property"),
            scenario.get("action"),
            scenario.get("final_check"),
            scenario.get("acceptance"),
            scenario.get("forbidden"),
        ]
        key = next((k for k in keys if k), None)
        requirement = scenario.get("requirement")
        if requirement is None:
            requirement = next((reqs[k] for k in keys if k in reqs), None)
        witnesses.append({
            "name": scenario.get("name"),
            "kind": scenario.get("kind"),
            "target": key,
            "requirement": requirement,
            "steps": scenario.get("steps", []),
            "narration": _narrate_steps(scenario.get("steps", [])),
            "initial_state": scenario.get("initial_state"),
            "expected_states": scenario.get("expected_states", []),
        })
    return witnesses


def explain_file(file, depth=8, max_mutants=None, readable=False):
    src = Path(file).read_text(encoding="utf-8")
    ast, display_names = parse_src(src, str(Path(file).parent))
    if ast[0] != "spec":
        raise FslError("explain expects a spec-like FSL file", kind="semantics")
    source_lines = src.splitlines()
    spec = build_spec(ast, display_names)
    out = {
        "result": "explained",
        "spec": spec["name"],
        "depth": depth,
        "skeleton": _skeleton(spec, source_lines),
    }
    if readable:
        out["readable"] = render_readable(out, spec, display_names)
        return out
    counterfactuals, reachable_kills = _counterfactuals(
        ast, display_names, spec, source_lines, depth, max_mutants
    )
    out["counterfactuals"] = counterfactuals
    out["witnesses"] = _witnesses(spec, source_lines, depth)
    if reachable_kills:
        out["reachable_counterfactuals"] = reachable_kills
    return out
