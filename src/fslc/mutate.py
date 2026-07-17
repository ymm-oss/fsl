# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Specification mutation testing for FSL kernel ASTs."""
from __future__ import annotations

import json
from copy import deepcopy
from pathlib import Path

from lark.exceptions import UnexpectedInput, VisitError

from .acceptance import validate_acceptance, validate_forbidden
from .bmc import verify
from .model import FslError, build_spec, display_label
from .parser import parse_src
from .refine import build_refinement, refine


DEFAULT_MAX_MUTANTS = 200


class Mutant:
    def __init__(self, op, path, replacement=None, remove=False, loc=None,
                 target=None, requirement=None, action=None):
        self.op = op
        self.path = tuple(path)
        self.replacement = replacement
        self.remove = remove
        self.loc = loc
        self.target = target
        self.requirement = requirement
        self.action = action


def _requirement(meta):
    if not meta:
        return None
    return {"id": meta["id"], "text": meta.get("text")}


def _action_meta(action):
    return action[6] if len(action) > 6 else None


def _action_fair(action):
    return bool(action[5]) if len(action) > 5 else False


def _item_loc(item):
    if not isinstance(item, tuple):
        return None
    tag = item[0]
    if tag == "action":
        return item[4]
    if tag in ("requires", "ensures", "let"):
        return item[-1]
    if tag in ("assign", "if", "forall_stmt"):
        return item[-1]
    return None


def _get(root, path):
    cur = root
    for part in path:
        cur = cur[part]
    return cur


def _replace(root, path, value):
    if not path:
        return value
    head, *tail = path
    if isinstance(root, tuple):
        items = list(root)
        items[head] = _replace(items[head], tail, value)
        return tuple(items)
    if isinstance(root, list):
        out = list(root)
        out[head] = _replace(out[head], tail, value)
        return out
    if isinstance(root, dict):
        out = dict(root)
        out[head] = _replace(out[head], tail, value)
        return out
    raise TypeError(f"cannot replace inside {type(root).__name__}")


def _remove(root, path):
    parent = _get(root, path[:-1])
    idx = path[-1]
    if not isinstance(parent, list):
        raise TypeError("mutation removal target is not in a list")
    new_parent = list(parent)
    del new_parent[idx]
    return _replace(root, path[:-1], new_parent)


def _apply(items, mutant):
    copied = deepcopy(items)
    if mutant.remove:
        return _remove(copied, mutant.path)
    return _replace(copied, mutant.path, deepcopy(mutant.replacement))


def _display_action(name, display_names):
    fake = {"display_names": display_names or {}}
    return display_label(name, fake)


def _enum_siblings(items):
    siblings = {}
    for item in items:
        if isinstance(item, tuple) and item and item[0] == "enum":
            members = item[2]
            for member in members:
                siblings[member] = [m for m in members if m != member]
    return siblings


def _loc_dict(loc):
    return loc if loc is not None else None


def _expr_mutants(expr, path, *, enums, loc, target, requirement, action):
    if not isinstance(expr, tuple) or not expr:
        return
    tag = expr[0]
    if tag == "num":
        n = expr[1]
        for delta, suffix in ((-1, "minus1"), (1, "plus1")):
            yield Mutant(
                f"integer_literal_{suffix}", path, ("num", n + delta),
                loc=loc, target=target, requirement=requirement, action=action,
            )
        return
    if tag == "var" and expr[1] in enums:
        for sibling in enums[expr[1]]:
            yield Mutant(
                "enum_constant_swap", path, ("var", sibling),
                loc=loc, target=f"{target} {expr[1]}->{sibling}",
                requirement=requirement, action=action,
            )
        return
    for child_path in _expr_child_paths(expr):
        yield from _expr_mutants(
            _get(expr, child_path), path + child_path,
            enums=enums, loc=loc, target=target, requirement=requirement, action=action,
        )


def _expr_child_paths(expr):
    tag = expr[0]
    if tag in ("not", "neg", "some", "old", "abs"):
        return [(1,)]
    if tag == "bin":
        return [(2,), (3,)]
    if tag == "index":
        return [(1,), (2,)]
    if tag == "field":
        return [(1,)]
    if tag == "method":
        return [(1,)] + [(3, i) for i in range(len(expr[3]))]
    if tag == "is":
        return [(1,)]
    if tag in ("forall", "exists"):
        paths = [(2,)]
        binder = expr[1]
        if binder[0] == "binder_range":
            paths.extend([(1, 2), (1, 3)])
        elif binder[0] == "binder_typed" and binder[3] is not None:
            paths.append((1, 3))
        return paths
    if tag in ("set_lit", "seq_lit"):
        return [(1, i) for i in range(len(expr[1]))]
    if tag == "struct_lit":
        return [(2, k) for k in sorted(expr[2])]
    if tag == "ite":
        return [(1,), (2,), (3,)]
    if tag == "count":
        return [(3,)]
    if tag == "sum":
        paths = [(3,)]
        if expr[4] is not None:
            paths.append((4,))
        return paths
    if tag in ("min", "max"):
        return [(1,), (2,)]
    return []


def _lvalue_expr_paths(lvalue):
    if not isinstance(lvalue, tuple):
        return []
    if lvalue[0] == "index":
        return [(2,)]
    if lvalue[0] == "field_lv":
        return [(1,)] if isinstance(lvalue[1], tuple) and lvalue[1][0] == "index" else []
    return []


def _stmt_mutants(stmts, path, *, enums, action_name, action_label, requirement):
    for idx, stmt in enumerate(stmts):
        spath = path + (idx,)
        if not isinstance(stmt, tuple):
            continue
        loc = _item_loc(stmt)
        if stmt[0] == "assign":
            yield Mutant(
                "assignment_remove", spath, remove=True, loc=loc,
                target=f"{action_label} assignment", requirement=requirement,
                action=action_label,
            )
            for lpath in _lvalue_expr_paths(stmt[1]):
                yield from _expr_mutants(
                    _get(stmt[1], lpath), spath + (1,) + lpath,
                    enums=enums, loc=loc, target=f"{action_label} assignment target",
                    requirement=requirement, action=action_label,
                )
            yield from _expr_mutants(
                stmt[2], spath + (2,), enums=enums, loc=loc,
                target=f"{action_label} assignment", requirement=requirement,
                action=action_label,
            )
        elif stmt[0] == "if":
            _, cond, then_stmts, else_stmts, if_loc = stmt
            if then_stmts and else_stmts:
                yield Mutant(
                    "then_else_swap", spath,
                    ("if", cond, deepcopy(else_stmts), deepcopy(then_stmts), if_loc),
                    loc=if_loc, target=f"{action_label} if",
                    requirement=requirement, action=action_label,
                )
            yield from _expr_mutants(
                cond, spath + (1,), enums=enums, loc=if_loc,
                target=f"{action_label} if condition", requirement=requirement,
                action=action_label,
            )
            yield from _stmt_mutants(
                then_stmts, spath + (2,), enums=enums, action_name=action_name,
                action_label=action_label, requirement=requirement,
            )
            yield from _stmt_mutants(
                else_stmts, spath + (3,), enums=enums, action_name=action_name,
                action_label=action_label, requirement=requirement,
            )
        elif stmt[0] == "forall_stmt":
            binder = stmt[1]
            if binder[0] == "binder_range":
                yield from _expr_mutants(
                    binder[2], spath + (1, 2), enums=enums, loc=loc,
                    target=f"{action_label} forall lower bound",
                    requirement=requirement, action=action_label,
                )
                yield from _expr_mutants(
                    binder[3], spath + (1, 3), enums=enums, loc=loc,
                    target=f"{action_label} forall upper bound",
                    requirement=requirement, action=action_label,
                )
            elif binder[0] == "binder_typed" and binder[3] is not None:
                yield from _expr_mutants(
                    binder[3], spath + (1, 3), enums=enums, loc=loc,
                    target=f"{action_label} forall where",
                    requirement=requirement, action=action_label,
                )
            yield from _stmt_mutants(
                stmt[2], spath + (2,), enums=enums, action_name=action_name,
                action_label=action_label, requirement=requirement,
            )


def _action_mutants(item, path, *, enums, display_names):
    name = item[1]
    label = _display_action(name, display_names)
    requirement = _requirement(_action_meta(item))
    body = item[3]
    req_no = 0
    for idx, part in enumerate(body):
        if not isinstance(part, tuple):
            continue
        ppath = path + (3, idx)
        loc = _item_loc(part)
        if part[0] == "requires":
            req_no += 1
            target = f"{label} requires #{req_no}"
            yield Mutant(
                "requires_remove", ppath, remove=True, loc=loc,
                target=target, requirement=requirement, action=label,
            )
            yield Mutant(
                "requires_negate", ppath, ("requires", ("not", part[1]), loc),
                loc=loc, target=target, requirement=requirement, action=label,
            )
            yield from _expr_mutants(
                part[1], ppath + (1,), enums=enums, loc=loc, target=target,
                requirement=requirement, action=label,
            )
        elif part[0] == "let":
            yield from _expr_mutants(
                part[2], ppath + (2,), enums=enums, loc=loc,
                target=f"{label} let {part[1]}", requirement=requirement, action=label,
            )
    yield from _stmt_mutants(
        body, path + (3,), enums=enums, action_name=name,
        action_label=label, requirement=requirement,
    )
    if _action_fair(item):
        replacement = item[:5] + (False,) + item[6:]
        yield Mutant(
            "fair_remove", path, replacement, loc=item[4],
            target=f"{label} fair", requirement=requirement, action=label,
        )


def _type_mutants(item, path):
    _, name, lo, hi = item[:4]
    for bound_idx, bound_name, expr in ((2, "lo", lo), (3, "hi", hi)):
        if not (isinstance(expr, tuple) and expr[0] == "num"):
            continue
        n = expr[1]
        for delta, suffix in ((-1, "minus1"), (1, "plus1")):
            yield Mutant(
                f"type_bound_{bound_name}_{suffix}",
                path + (bound_idx,), ("num", n + delta),
                loc=None, target=f"type {name} {bound_name}", requirement=None,
                action=None,
            )


def enumerate_mutants(ast, display_names=None):
    _, _name, items = ast
    enums = _enum_siblings(items)
    out = []
    for idx, item in enumerate(items):
        if not isinstance(item, tuple) or not item:
            continue
        path = (idx,)
        if item[0] == "type":
            out.extend(_type_mutants(item, path))
        elif item[0] == "const":
            out.extend(_expr_mutants(
                item[2], path + (2,), enums=enums, loc=None,
                target=f"const {item[1]}", requirement=None, action=None,
            ))
        elif item[0] == "init":
            out.extend(_stmt_mutants(
                item[1], path + (1,), enums=enums, action_name="init",
                action_label="init", requirement=None,
            ))
        elif item[0] == "action":
            out.extend(_action_mutants(item, path, enums=enums, display_names=display_names))
    return out


def _acceptance_result(spec):
    checked = validate_acceptance(spec)
    if checked.get("ok"):
        return None
    out = dict(checked)
    out.pop("ok", None)
    return {"result": "error", **out}


def _forbidden_result(spec):
    checked = validate_forbidden(spec)
    if checked.get("ok"):
        return None
    out = dict(checked)
    out.pop("ok", None)
    return {"result": "error", **out}


def _implements_result(spec, depth):
    impl = spec.get("implements")
    if not impl:
        return None
    abs_spec = build_spec(impl["abs_ast"], impl.get("abs_display_names"))
    mapping = build_refinement(impl["mapping_ast"], spec, abs_spec)
    result = refine(spec, abs_spec, mapping, depth)
    if result.get("result") == "refines":
        return {"abs": abs_spec["name"], "result": "refines"}
    return result


def _killer_from_verify(result):
    if result.get("result") == "violated":
        return result.get("invariant") or result.get("violation_kind") or "invariant"
    if result.get("result") == "reachable_failed":
        unreached = result.get("unreached") or []
        if unreached:
            return unreached[0].get("name") or "reachable"
        return "reachable"
    if result.get("result") == "error":
        return result.get("kind") or "error"
    return result.get("result")


def _killer_requirement(kind, result):
    req = result.get("requirement")
    if req:
        return req
    if kind == "refinement":
        action = result.get("impl_action") or {}
        return action.get("requirement")
    return None


def _oracle(spec, depth, source_lines=None):
    v = verify(
        spec,
        depth,
        deadlock_mode="warn",
        source_lines=source_lines,
        vacuity_mode="ignore",
    )
    if v.get("result") in ("violated", "reachable_failed", "error"):
        return {
            "clean": False,
            "result": v,
            "killed_by": _killer_from_verify(v),
            "killer_requirement": _killer_requirement("verify", v),
        }

    acc = _acceptance_result(spec)
    if acc:
        return {
            "clean": False,
            "result": acc,
            "killed_by": "acceptance",
            "killer_requirement": _killer_requirement("acceptance", acc),
        }

    forb = _forbidden_result(spec)
    if forb:
        return {
            "clean": False,
            "result": forb,
            "killed_by": "forbidden",
            "killer_requirement": _killer_requirement("forbidden", forb),
        }

    impl = _implements_result(spec, depth)
    if impl and impl.get("result") != "refines":
        return {
            "clean": False,
            "result": impl,
            "killed_by": "refinement",
            "killer_requirement": _killer_requirement("refinement", impl),
        }

    baseline = dict(v)
    if impl:
        baseline["implements"] = impl
    return {"clean": True, "result": baseline, "killed_by": None, "killer_requirement": None}


def _build_error_kill(exc):
    return {
        "clean": False,
        "result": {
            "result": "error",
            "kind": getattr(exc, "kind", "semantics"),
            "message": str(exc),
            **({"loc": exc.loc} if getattr(exc, "loc", None) else {}),
        },
        "killed_by": "build_spec",
        "killer_requirement": None,
    }


def _requirement_index(spec):
    reqs = {}
    for req_id in spec.get("requirement_ids") or []:
        reqs.setdefault(req_id, {"kills": 0})
    for collection in ("actions", "user_invariants", "leadstos", "reachables"):
        for item in spec.get(collection, []):
            meta = item.get("meta")
            if meta and meta.get("id"):
                reqs.setdefault(meta["id"], {"kills": 0})
    return reqs


def _coverage_false_actions(baseline):
    out = set()
    for name, cov in (baseline.get("action_coverage") or {}).items():
        if cov is not True:
            out.add(name)
    return out


def _public_mutant(mutant, status, killed_by=None, dead_actions=None):
    out = {
        "op": mutant.op,
        "loc": _loc_dict(mutant.loc),
        "target": mutant.target,
        "status": status,
        "killed_by": killed_by,
        "requirement": mutant.requirement,
        "source": "builtin",
    }
    if status == "survived" and mutant.action in (dead_actions or set()):
        out["note"] = "action dead at baseline — survival expected"
    return out


def _invalid_detail(kind, message, loc=None):
    out = {"kind": kind, "message": message}
    if loc is not None:
        out["loc"] = loc
    return out


def _external_public(record, status, *, killed_by=None, invalid=None):
    out = {
        "id": record["id"],
        "op": record.get("op", "external"),
        "loc": None,
        "target": record.get("description") or record["id"],
        "status": status,
        "killed_by": killed_by,
        "requirement": record.get("requirement"),
        "source": "external",
        "input_kind": record.get("input_kind"),
        "line": record["line"],
    }
    if invalid is not None:
        out["invalid"] = invalid
    return out


def _replace_instruction(source, instruction):
    if not isinstance(instruction, dict):
        raise ValueError("replace must be an object")
    target = instruction.get("target")
    replacement = instruction.get("replacement")
    occurrence = instruction.get("occurrence")
    if not isinstance(target, str) or not target:
        raise ValueError("replace.target must be a non-empty string")
    if not isinstance(replacement, str):
        raise ValueError("replace.replacement must be a string")
    starts = []
    pos = 0
    while True:
        found = source.find(target, pos)
        if found < 0:
            break
        starts.append(found)
        pos = found + len(target)
    if occurrence is None:
        if len(starts) != 1:
            raise ValueError(
                f"replace.target must match exactly once without occurrence; matched {len(starts)} times"
            )
        selected = 0
    else:
        if isinstance(occurrence, bool) or not isinstance(occurrence, int) or occurrence < 1:
            raise ValueError("replace.occurrence must be a positive 1-based integer")
        if occurrence > len(starts):
            raise ValueError(
                f"replace.occurrence {occurrence} exceeds {len(starts)} match(es)"
            )
        selected = occurrence - 1
    start = starts[selected]
    return source[:start] + replacement + source[start + len(target):]


def _external_source(record, baseline_source):
    full_keys = [key for key in ("mutated_spec", "spec") if key in record]
    has_nested_replace = "replace" in record
    has_flat_replace = "target" in record or "replacement" in record
    modes = len(full_keys) + int(has_nested_replace or has_flat_replace)
    if modes != 1:
        raise ValueError(
            "provide exactly one mutation form: mutated_spec/spec or replace"
        )
    if full_keys:
        source = record[full_keys[0]]
        if not isinstance(source, str):
            raise ValueError(f"{full_keys[0]} must be a string")
        return source, "full_spec"
    instruction = record.get("replace")
    if instruction is None:
        instruction = {
            "target": record.get("target"),
            "replacement": record.get("replacement"),
            "occurrence": record.get("occurrence"),
        }
    return _replace_instruction(baseline_source, instruction), "replacement"


def _load_external_records(path, baseline_source):
    records = []
    seen_ids = set()
    with Path(path).open(encoding="utf-8") as fh:
        for line_no, raw in enumerate(fh, start=1):
            if not raw.strip():
                continue
            fallback_id = f"external:{line_no}"
            try:
                value = json.loads(raw)
            except json.JSONDecodeError as exc:
                record = {"id": fallback_id, "line": line_no, "input_kind": None}
                records.append((record, None, _invalid_detail("json", str(exc))))
                continue
            if not isinstance(value, dict):
                record = {"id": fallback_id, "line": line_no, "input_kind": None}
                records.append((record, None, _invalid_detail(
                    "shape", "each JSONL line must be an object")))
                continue
            record = dict(value)
            record_id = record.get("id", fallback_id)
            if not isinstance(record_id, str) or not record_id.strip():
                record_id = fallback_id
                invalid = _invalid_detail("shape", "id must be a non-empty string")
                record.update({"id": record_id, "line": line_no, "input_kind": None})
                records.append((record, None, invalid))
                continue
            record.update({"id": record_id, "line": line_no})
            if record_id in seen_ids:
                record["input_kind"] = None
                records.append((record, None, _invalid_detail(
                    "shape", f"duplicate external mutant id '{record_id}'")))
                continue
            seen_ids.add(record_id)
            try:
                source, input_kind = _external_source(record, baseline_source)
            except ValueError as exc:
                record["input_kind"] = None
                records.append((record, None, _invalid_detail("instruction", str(exc))))
                continue
            record["input_kind"] = input_kind
            records.append((record, source, None))
    return records


def _external_spec(source, base_dir, expected_name):
    try:
        ast, display_names = parse_src(source, base_dir)
    except UnexpectedInput as exc:
        loc = None
        if getattr(exc, "line", -1) > 0 and getattr(exc, "column", -1) > 0:
            loc = {"line": exc.line, "column": exc.column}
        return None, None, _invalid_detail(
            "parse",
            str(exc).split("\n")[0],
            loc,
        )
    except VisitError as exc:
        orig = exc.orig_exc
        return None, None, _invalid_detail(
            getattr(orig, "kind", "semantics"),
            str(orig),
            getattr(orig, "loc", None),
        )
    except FslError as exc:
        return None, None, _invalid_detail(
            exc.kind, str(exc), getattr(exc, "loc", None))
    if ast[0] != "spec":
        return None, None, _invalid_detail(
            "semantics", "external mutant must be a spec-like FSL file")
    if ast[1] != expected_name:
        return None, None, _invalid_detail(
            "spec_name",
            f"external mutant spec name '{ast[1]}' does not match baseline '{expected_name}'",
        )
    try:
        return build_spec(ast, display_names), display_names, None
    except FslError as exc:
        return None, None, _invalid_detail(
            exc.kind, str(exc), getattr(exc, "loc", None))


def _kill_rate(killed, survived):
    judged = killed + survived
    return round(killed / judged, 4) if judged else None


def _summary(mutants):
    by_source = {}
    for source in ("builtin", "external"):
        entries = [item for item in mutants if item["source"] == source]
        killed = sum(item["status"] == "killed" for item in entries)
        survived = sum(item["status"] == "survived" for item in entries)
        invalid = sum(item["status"] == "invalid" for item in entries)
        by_source[source] = {
            "total": len(entries),
            "killed": killed,
            "survived": survived,
            "invalid": invalid,
            "kill_rate": _kill_rate(killed, survived),
        }
    killed = sum(item["status"] == "killed" for item in mutants)
    survived = sum(item["status"] == "survived" for item in mutants)
    invalid = sum(item["status"] == "invalid" for item in mutants)
    return {
        "total": len(mutants),
        "killed": killed,
        "survived": survived,
        "invalid": invalid,
        "kill_rate": _kill_rate(killed, survived),
        "by_source": by_source,
    }


def mutate_file(
        file, depth=8, by_requirement=False, max_mutants=DEFAULT_MAX_MUTANTS,
        external_mutants=None):
    src = Path(file).read_text(encoding="utf-8")
    ast, display_names = parse_src(src, str(Path(file).parent))
    if ast[0] != "spec":
        raise FslError("mutate expects a spec-like FSL file", kind="semantics")
    _, name, items = ast
    source_lines = src.splitlines()

    baseline_spec = build_spec(ast, display_names)
    baseline_oracle = _oracle(baseline_spec, depth, source_lines=source_lines)
    if not baseline_oracle["clean"]:
        return baseline_oracle["result"]
    baseline = baseline_oracle["result"]

    all_mutants = enumerate_mutants(ast, display_names)
    selected = all_mutants[:max_mutants]
    dropped = max(0, len(all_mutants) - len(selected))
    notes = [
        "possible equivalent mutants should be reviewed manually; survivors are a review queue, not a hard failure",
    ]
    if dropped:
        notes.append(f"mutant cap {max_mutants} reached: {dropped} dropped")

    dead_actions = _coverage_false_actions(baseline)
    public_mutants = []
    by_req = _requirement_index(baseline_spec) if by_requirement else {}

    for mutant in selected:
        mutated_items = _apply(items, mutant)
        mutated_ast = ("spec", name, mutated_items)
        try:
            mutated_spec = build_spec(mutated_ast, display_names)
            oracle = _oracle(mutated_spec, depth, source_lines=source_lines)
        except FslError as exc:
            oracle = _build_error_kill(exc)

        if oracle["clean"]:
            public_mutants.append(_public_mutant(
                mutant, "survived", dead_actions=dead_actions))
            continue

        killed_by = oracle["killed_by"]
        public_mutants.append(_public_mutant(
            mutant, "killed", killed_by=killed_by, dead_actions=dead_actions))
        killer_req = oracle.get("killer_requirement")
        if by_requirement and killer_req and killer_req.get("id") in by_req:
            by_req[killer_req["id"]]["kills"] += 1

    if external_mutants is not None:
        base_dir = str(Path(file).parent)
        for record, mutated_source, invalid in _load_external_records(
                external_mutants, src):
            if invalid is not None:
                public_mutants.append(_external_public(
                    record, "invalid", invalid=invalid))
                continue
            mutated_spec, _mutated_display_names, invalid = _external_spec(
                mutated_source, base_dir, name)
            if invalid is not None:
                public_mutants.append(_external_public(
                    record, "invalid", invalid=invalid))
                continue
            try:
                oracle = _oracle(
                    mutated_spec,
                    depth,
                    source_lines=mutated_source.splitlines(),
                )
            except FslError as exc:
                public_mutants.append(_external_public(
                    record,
                    "invalid",
                    invalid=_invalid_detail(
                        exc.kind, str(exc), getattr(exc, "loc", None)),
                ))
                continue
            oracle_result = oracle.get("result") or {}
            if (
                not oracle["clean"]
                and oracle_result.get("result") == "error"
                and oracle_result.get("kind") not in {"acceptance", "forbidden"}
            ):
                public_mutants.append(_external_public(
                    record,
                    "invalid",
                    invalid=_invalid_detail(
                        oracle_result.get("kind"),
                        oracle_result.get("message", "invalid external mutant"),
                        oracle_result.get("loc"),
                    ),
                ))
                continue
            if oracle["clean"]:
                public_mutants.append(_external_public(record, "survived"))
                continue
            public_mutants.append(_external_public(
                record, "killed", killed_by=oracle["killed_by"]))
            killer_req = oracle.get("killer_requirement")
            if by_requirement and killer_req and killer_req.get("id") in by_req:
                by_req[killer_req["id"]]["kills"] += 1

    if by_requirement:
        for req_id, data in by_req.items():
            if data["kills"] == 0:
                data["warning"] = "empty_formalization"
        notes.append(
            "by_requirement kills are an observed lower bound within this mutant set and depth"
        )

    if external_mutants is not None:
        notes.append(
            "invalid external mutants are generation-quality findings and are excluded from kill-rate denominators"
        )
    return {
        "result": "mutated",
        "spec": name,
        "depth": depth,
        "baseline": baseline.get("result", "verified"),
        "summary": _summary(public_mutants),
        "mutants": public_mutants,
        "by_requirement": by_req,
        "notes": notes,
    }
