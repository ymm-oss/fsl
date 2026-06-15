# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Dialect AST expansion for requirement-oriented FSL frontends."""
from __future__ import annotations

from copy import deepcopy
from pathlib import Path

from .compose import expand_compose
from .grammar import Ast, PARSER
from .model import FslError, eval_const


def _err(message, kind="type", loc=None):
    raise FslError(message, kind=kind, loc=loc)


def _parse_file(path):
    src = Path(path).read_text(encoding="utf-8")
    ast = Ast().transform(PARSER.parse(src))
    if ast[0] == "compose":
        return expand_compose(ast, str(Path(path).parent))
    if ast[0] == "requirements":
        return _expand_requirements_with_display(ast, str(Path(path).parent))
    if ast[0] == "business":
        return expand_business(ast), {}
    return ast, {}


def _expr_to_str(expr):
    if not isinstance(expr, tuple):
        return str(expr)
    tag = expr[0]
    if tag == "var":
        return expr[1]
    if tag == "num":
        return str(expr[1])
    if tag == "bool":
        return "true" if expr[1] else "false"
    if tag == "none":
        return "none"
    if tag == "bin":
        return f"{_expr_to_str(expr[2])} {expr[1]} {_expr_to_str(expr[3])}"
    if tag == "not":
        return f"not {_expr_to_str(expr[1])}"
    if tag == "neg":
        return f"-{_expr_to_str(expr[1])}"
    if tag == "index":
        return f"{_expr_to_str(expr[1])}[{_expr_to_str(expr[2])}]"
    if tag == "field":
        return f"{_expr_to_str(expr[1])}.{expr[2]}"
    if tag == "method":
        args = ", ".join(_expr_to_str(a) for a in expr[3])
        return f"{_expr_to_str(expr[1])}.{expr[2]}({args})"
    if tag == "some":
        return f"some({_expr_to_str(expr[1])})"
    if tag == "is":
        pat = expr[2]
        if pat[0] == "pat_none":
            return f"{_expr_to_str(expr[1])} is none"
        return f"{_expr_to_str(expr[1])} is some({pat[1]})"
    if tag == "ite":
        return (
            f"if {_expr_to_str(expr[1])} then {_expr_to_str(expr[2])} "
            f"else {_expr_to_str(expr[3])}"
        )
    return tag


def _meta(req_id, text):
    return {"id": req_id, "text": text}


def _with_meta(item, meta):
    tag = item[0]
    if tag == "req_action":
        return item[:6] + (meta, item[7])
    if tag == "action":
        return item[:6] + (meta,)
    if tag in ("invariant", "reachable"):
        return item[:4] + (meta,)
    if tag == "leadsto":
        return item[:6] + (meta,)
    return item


def _kernel_action_from_req(item, meta=None):
    _, name, params, body, loc, fair, own_meta, maps = item
    out_meta = meta if meta is not None else own_meta
    return ("action", name, params, body, loc, fair, out_meta), maps


def _param_names(params):
    return [p[1] for p in params]


def _and_all(exprs):
    if not exprs:
        return ("bool", True)
    out = exprs[0]
    for expr in exprs[1:]:
        out = ("bin", "and", out, expr)
    return out


def _or_all(exprs):
    if not exprs:
        return ("bool", False)
    out = exprs[0]
    for expr in exprs[1:]:
        out = ("bin", "or", out, expr)
    return out


def _subst_expr(expr, env, bound=None):
    bound = set(bound or ())
    if not isinstance(expr, tuple):
        return expr
    tag = expr[0]
    if tag == "var":
        name = expr[1]
        if name in env and name not in bound:
            return deepcopy(env[name])
        return expr
    if tag in ("num", "bool", "none", "pat_none", "pat_some"):
        return expr
    if tag in ("neg", "not", "abs", "old", "some"):
        return (tag, _subst_expr(expr[1], env, bound))
    if tag == "bin":
        return ("bin", expr[1], _subst_expr(expr[2], env, bound), _subst_expr(expr[3], env, bound))
    if tag == "index":
        return ("index", _subst_expr(expr[1], env, bound), _subst_expr(expr[2], env, bound))
    if tag == "field":
        return ("field", _subst_expr(expr[1], env, bound), expr[2])
    if tag == "method":
        return ("method", _subst_expr(expr[1], env, bound), expr[2],
                [_subst_expr(a, env, bound) for a in expr[3]])
    if tag == "is":
        return ("is", _subst_expr(expr[1], env, bound), expr[2])
    if tag == "ite":
        return (
            "ite",
            _subst_expr(expr[1], env, bound),
            _subst_expr(expr[2], env, bound),
            _subst_expr(expr[3], env, bound),
        )
    if tag in ("forall", "exists"):
        binder = expr[1]
        bname = binder[1]
        bbound = set(bound)
        bbound.add(bname)
        if binder[0] == "binder_typed":
            where = _subst_expr(binder[3], env, bbound) if binder[3] is not None else None
            binder = ("binder_typed", binder[1], binder[2], where)
        elif binder[0] == "binder_range":
            binder = (
                "binder_range",
                binder[1],
                _subst_expr(binder[2], env, bound),
                _subst_expr(binder[3], env, bound),
            )
        return (tag, binder, _subst_expr(expr[2], env, bbound))
    if tag in ("set_lit", "seq_lit"):
        return (tag, [_subst_expr(e, env, bound) for e in expr[1]])
    if tag == "struct_lit":
        return (tag, expr[1], {k: _subst_expr(v, env, bound) for k, v in expr[2].items()})
    if tag in ("count", "sum"):
        bbound = set(bound)
        bbound.add(expr[1])
        if tag == "count":
            return ("count", expr[1], expr[2], _subst_expr(expr[3], env, bbound))
        return (
            "sum",
            expr[1],
            expr[2],
            _subst_expr(expr[3], env, bbound),
            _subst_expr(expr[4], env, bbound) if expr[4] is not None else None,
        )
    if tag in ("min", "max"):
        return (tag, _subst_expr(expr[1], env, bound), _subst_expr(expr[2], env, bound))
    return expr


def _target_from_maps(maps, loc):
    if maps is None:
        _err("action in requirements implements block needs a maps clause", loc=loc)
    return maps[1]


def _action_map(name, params, maps, loc):
    return ("action_map", name, _param_names(params), _target_from_maps(maps, loc), loc)


def _split_branch_action(item, req_meta, display_names, action_aliases):
    _, name, params, body, loc, fair, own_meta, action_maps = item
    meta = req_meta if req_meta is not None else own_meta
    branches = [b for b in body if b[0] == "branches"]
    if not branches:
        action, maps = _kernel_action_from_req(item, meta)
        action_aliases.setdefault(name, []).append(name)
        return [action], [_action_map(name, params, maps or action_maps, loc)] if (maps or action_maps) else []
    if len(branches) > 1:
        _err(f"action '{name}' has multiple branches blocks", loc=loc)
    if action_maps is not None:
        _err(f"branched action '{name}' must put maps on each when branch", loc=loc)

    base_items = [deepcopy(b) for b in body if b[0] != "branches"]
    out_actions = []
    out_maps = []
    action_aliases.setdefault(name, [])
    for idx, branch in enumerate(branches[0][1], start=1):
        _, cond, stmts, maps, bloc = branch
        bname = f"{name}__b{idx}"
        bbody = deepcopy(base_items)
        bbody.append(("requires", cond, bloc))
        bbody.extend(deepcopy(stmts))
        out_actions.append(("action", bname, deepcopy(params), bbody, loc, fair, meta))
        out_maps.append(_action_map(bname, params, maps, bloc))
        display_names[bname] = f"{name}[{_expr_to_str(cond)}]"
        action_aliases[name].append(bname)
    return out_actions, out_maps


def _expand_item(item, req_meta, display_names, action_aliases):
    tag = item[0]
    if tag == "req_action":
        return _split_branch_action(item, req_meta, display_names, action_aliases)
    if tag in ("action", "invariant", "reachable", "leadsto"):
        return [_with_meta(item, req_meta) if req_meta is not None else item], []
    return [item], []


def _collect_consts(items):
    consts = {}
    for item in items:
        if item[0] == "const":
            consts[item[1]] = eval_const(item[2], consts, {})
    return consts


def _generated_age_type_name(age_name, existing):
    base = "_Age" + "".join(part[:1].upper() + part[1:] for part in age_name.split("_") if part)
    if not base or base == "_Age":
        base = "_AgeCounter"
    name = base
    i = 2
    while name in existing:
        name = f"{base}{i}"
        i += 1
    existing.add(name)
    return name


def _require_simple_type_name(ty_name, loc):
    if not isinstance(ty_name, str):
        _err("indexed age expects a simple domain type name", loc=loc)
    return ty_name


def _param_to_binder(param):
    if param[0] == "param_typed":
        return ("binder_typed", param[1], param[2], None)
    return ("binder_range", param[1], param[2], param[3])


def _action_enabled_expr(action):
    _, _name, params, body, _loc, _fair, *_rest = action
    env = {}
    requires = []
    for item in body:
        if item[0] == "let":
            env[item[1]] = _subst_expr(item[2], env)
        elif item[0] == "requires":
            requires.append(_subst_expr(item[1], env))
    expr = _and_all(requires)
    for param in reversed(params):
        expr = ("exists", _param_to_binder(param), expr)
    return expr


def _merge_init(out, extra_stmts):
    if not extra_stmts:
        return
    last_init_idx = None
    for i, item in enumerate(out):
        if item[0] == "init":
            last_init_idx = i
    if last_init_idx is None:
        out.append(("init", extra_stmts))
        return
    init = out[last_init_idx]
    out[last_init_idx] = ("init", list(init[1]) + extra_stmts)


def _age_ref(age):
    if age["binder"] is None:
        return ("var", age["name"])
    return ("index", ("var", age["name"]), ("var", age["binder"][1]))


def _age_lvalue(age):
    if age["binder"] is None:
        return ("var", age["name"])
    return ("index", age["name"], ("var", age["binder"][1]))


def _build_age_tick_stmt(age):
    ref = _age_ref(age)
    lv = _age_lvalue(age)
    inc = ("assign", deepcopy(lv), ("bin", "+", deepcopy(ref), ("num", 1)), age["loc"])
    reset = ("assign", deepcopy(lv), ("num", 0), age["loc"])
    cap_guard = ("bin", "<", deepcopy(ref), ("num", age["cap"]))
    bump = ("if", cap_guard, [inc], [], age["loc"])
    body = [("if", age["cond"], [bump], [reset], age["loc"])]
    if age["binder"] is None:
        return body[0]
    return ("forall_stmt", age["binder"], body, age["loc"])


def _deadline_invariant_name(req_id, age_name, index):
    safe_req = "".join(ch if ch.isalnum() or ch == "_" else "_" for ch in req_id)
    return f"_deadline_{safe_req}_{age_name}_{index}"


def _deadline_expr(age, bound):
    ref = _age_ref(age)
    expr = ("bin", "<=", ref, bound)
    if age["binder"] is None:
        return expr
    return ("forall", age["binder"], expr)


def _parse_time_block(time_block):
    age_decls = {}
    urgent = []
    if time_block is None:
        return age_decls, urgent
    for item in time_block[1]:
        if item[0] == "time_urgent":
            urgent.extend(item[1])
        elif item[0] == "time_age":
            _, name, binder, cond, loc = item
            if name in age_decls:
                _err(f"duplicate age '{name}'", loc=loc)
            if binder is not None:
                if binder[0] != "binder_typed":
                    _err("indexed age expects syntax `age m[x: T] while ...`", loc=loc)
                _require_simple_type_name(binder[2], loc)
            age_decls[name] = {
                "name": name,
                "binder": binder,
                "cond": cond,
                "loc": loc,
            }
    return age_decls, urgent


def _expand_time(out, time_block, deadlines, action_aliases, action_maps, implements, consts):
    if deadlines and time_block is None:
        _err("deadline requires a time block", loc=deadlines[0]["loc"])
    if time_block is None:
        return []

    age_decls, urgent_names = _parse_time_block(time_block)
    if "tick" in action_aliases or any(item[0] == "action" and item[1] == "tick" for item in out):
        _err("time block cannot generate tick: action 'tick' already exists", loc=time_block[2])

    deadline_by_age = {}
    for d in deadlines:
        if d["age"] not in age_decls:
            _err(f"deadline references undeclared age '{d['age']}'", loc=d["loc"])
        k = eval_const(d["bound"], consts, {})
        if k < 0:
            _err("deadline bound must be non-negative", loc=d["loc"])
        d["bound_value"] = k
        deadline_by_age.setdefault(d["age"], []).append(d)

    for name, age in age_decls.items():
        refs = deadline_by_age.get(name, [])
        if not refs:
            _err(f"unused age '{name}'", loc=age["loc"])
        age["cap"] = max(d["bound_value"] for d in refs) + 1

    action_by_name = {item[1]: item for item in out if item[0] == "action"}
    urgent_enabled = []
    for name in urgent_names:
        physical = action_aliases.get(name)
        if not physical:
            _err(f"unknown urgent action '{name}'", loc=time_block[2])
        for phys in physical:
            action = action_by_name.get(phys)
            if action is None:
                _err(f"unknown urgent action '{name}'", loc=time_block[2])
            urgent_enabled.append(_action_enabled_expr(action))

    existing_types = {item[1] for item in out if item[0] in ("type", "enum", "struct")}
    state_decls = []
    init_stmts = []
    ages = []
    for age in age_decls.values():
        type_name = _generated_age_type_name(age["name"], existing_types)
        age["type_name"] = type_name
        out.append(("type", type_name, ("num", 0), ("num", age["cap"])))
        if age["binder"] is None:
            state_decls.append(("decl", age["name"], ("name", type_name)))
            init_stmts.append(("assign", ("var", age["name"]), ("num", 0), age["loc"]))
        else:
            key_ty = _require_simple_type_name(age["binder"][2], age["loc"])
            state_decls.append(("decl", age["name"], ("map", ("name", key_ty), ("name", type_name))))
            idx = ("var", age["binder"][1])
            init_stmts.append((
                "forall_stmt",
                age["binder"],
                [("assign", ("index", age["name"], idx), ("num", 0), age["loc"])],
                age["loc"],
            ))
        ages.append(age)

    if state_decls:
        out.append(("state", state_decls))
    _merge_init(out, init_stmts)

    tick_body = []
    if urgent_enabled:
        tick_body.append(("requires", ("not", _or_all(urgent_enabled)), time_block[2]))
    tick_body.extend(_build_age_tick_stmt(age) for age in ages)
    out.append(("action", "tick", [], tick_body, time_block[2], False, None))
    action_aliases.setdefault("tick", []).append("tick")
    generated_names = ["tick"]
    if implements is not None:
        action_maps.append(("action_map", "tick", [], ("stutter",), time_block[2]))

    idx = 1
    for d in deadlines:
        age = age_decls[d["age"]]
        out.append((
            "invariant",
            _deadline_invariant_name(d["meta"]["id"], d["age"], idx),
            _deadline_expr(age, d["bound"]),
            d["loc"],
            d["meta"],
        ))
        idx += 1
    return generated_names


def _expand_requirements_with_display(ast, base_dir):
    _, name, items = ast
    out = []
    display_names = {}
    action_maps = []
    action_aliases = {}
    implements = None
    acceptances = []
    forbiddens = []
    time_block = None
    deadlines = []
    requirement_ids = []
    generated_names = []
    consts = _collect_consts(items)

    for item in items:
        tag = item[0]
        if tag == "implements":
            if implements is not None:
                _err("requirements may declare implements only once", loc=item[4])
            _, abs_name, path, map_items, loc = item
            abs_path = Path(base_dir) / path
            if not abs_path.is_file():
                _err(f"file not found: {path}", kind="io", loc=loc)
            abs_ast, abs_display_names = _parse_file(abs_path)
            if abs_ast[0] != "spec" or abs_ast[1] != abs_name:
                _err(f"spec name mismatch: expected '{abs_name}', got '{abs_ast[1]}'", loc=loc)
            implements = {
                "abs": abs_name,
                "path": str(abs_path),
                "abs_ast": abs_ast,
                "abs_display_names": abs_display_names,
                "maps": map_items,
                "loc": loc,
            }
        elif tag == "time":
            if time_block is not None:
                _err("requirements may declare time block only once", loc=item[2])
            time_block = item
        elif tag == "requirement":
            _, req_id, text, req_items, loc = item
            requirement_ids.append(req_id)
            req_meta = _meta(req_id, text)
            for child in req_items:
                if child[0] == "deadline":
                    deadlines.append({
                        "age": child[1],
                        "bound": child[2],
                        "loc": child[3],
                        "meta": req_meta,
                    })
                    continue
                expanded, maps = _expand_item(child, req_meta, display_names, action_aliases)
                out.extend(expanded)
                action_maps.extend(maps)
        elif tag == "acceptance":
            _, ac_id, text, steps, expect, loc = item
            if expect is None:
                _err(f"acceptance '{ac_id}' missing expect", loc=loc)
            acceptances.append({
                "id": ac_id,
                "text": text,
                "steps": steps,
                "expect": expect[1],
                "loc": loc,
            })
        elif tag == "forbidden":
            _, fb_id, text, steps, loc = item
            forbiddens.append({
                "id": fb_id,
                "text": text,
                "steps": steps,
                "loc": loc,
            })
        else:
            expanded, maps = _expand_item(item, None, display_names, action_aliases)
            out.extend(expanded)
            action_maps.extend(maps)

    generated_names.extend(
        _expand_time(out, time_block, deadlines, action_aliases, action_maps, implements, consts)
    )

    if implements is not None:
        mapping_items = [("impl", name), ("abs", implements["abs"])]
        mapping_items.extend(implements["maps"])
        mapping_items.extend(action_maps)
        implements["mapping_ast"] = ("refinement", f"{name}Implements{implements['abs']}", mapping_items)
        out.append(("__implements", implements))
    if display_names:
        out.append(("__display_names", display_names))
    if action_aliases:
        out.append(("__action_aliases", action_aliases))
    if acceptances:
        out.append(("__acceptance", acceptances))
    if forbiddens:
        out.append(("__forbidden", forbiddens))
    if generated_names:
        out.append(("__generated", generated_names))
    if requirement_ids:
        out.append(("__requirement_ids", requirement_ids))
    return ("spec", name, out), display_names


def expand_requirements(ast, base_dir):
    """Expand requirements AST to a kernel spec AST."""
    expanded, _display_names = _expand_requirements_with_display(ast, base_dir)
    return expanded


def expand_requirements_with_display(ast, base_dir):
    """Expand requirements AST and return display names for parser plumbing."""
    return _expand_requirements_with_display(ast, base_dir)


def _type_name_from_binder(binder):
    if binder[0] == "binder_typed":
        return binder[2]
    return None


def _rewrite_stage_expr(expr, env, process_by_case):
    if not isinstance(expr, tuple):
        return expr
    tag = expr[0]
    if tag == "stage":
        arg = expr[1]
        if not (isinstance(arg, tuple) and arg[0] == "var"):
            _err("stage(...) expects a bound case variable", loc=expr[2] if len(expr) > 2 else None)
        var_name = arg[1]
        case_ty = env.get(var_name)
        if case_ty is None:
            _err(f"stage({var_name}) cannot be resolved; '{var_name}' is not a typed case binder",
                 loc=expr[2] if len(expr) > 2 else None)
        processes = process_by_case.get(case_ty, [])
        if not processes:
            _err(f"stage({var_name}) refers to type '{case_ty}', which has no process",
                 loc=expr[2] if len(expr) > 2 else None)
        if len(processes) > 1:
            _err(f"stage({var_name}) is ambiguous for type '{case_ty}'", loc=expr[2] if len(expr) > 2 else None)
        return ("index", ("var", processes[0]["state_var"]), arg)
    if tag in ("num", "bool", "none", "var", "pat_none", "pat_some"):
        return expr
    if tag in ("neg", "not", "abs", "old", "some"):
        return (tag, _rewrite_stage_expr(expr[1], env, process_by_case))
    if tag == "bin":
        return ("bin", expr[1],
                _rewrite_stage_expr(expr[2], env, process_by_case),
                _rewrite_stage_expr(expr[3], env, process_by_case))
    if tag == "index":
        return ("index",
                _rewrite_stage_expr(expr[1], env, process_by_case),
                _rewrite_stage_expr(expr[2], env, process_by_case))
    if tag == "field":
        return ("field", _rewrite_stage_expr(expr[1], env, process_by_case), expr[2])
    if tag == "method":
        return ("method",
                _rewrite_stage_expr(expr[1], env, process_by_case),
                expr[2],
                [_rewrite_stage_expr(a, env, process_by_case) for a in expr[3]])
    if tag == "is":
        return ("is", _rewrite_stage_expr(expr[1], env, process_by_case), expr[2])
    if tag in ("forall", "exists"):
        binder = expr[1]
        next_env = dict(env)
        ty_name = _type_name_from_binder(binder)
        if ty_name is not None:
            next_env[binder[1]] = ty_name
            if binder[3] is not None:
                binder = ("binder_typed", binder[1], binder[2],
                          _rewrite_stage_expr(binder[3], next_env, process_by_case))
        return (tag, binder, _rewrite_stage_expr(expr[2], next_env, process_by_case))
    if tag == "struct_lit":
        return ("struct_lit", expr[1],
                {k: _rewrite_stage_expr(v, env, process_by_case) for k, v in expr[2].items()})
    if tag in ("set_lit", "seq_lit"):
        return (tag, [_rewrite_stage_expr(e, env, process_by_case) for e in expr[1]])
    if tag == "count":
        _, v, ty, cond = expr
        next_env = dict(env)
        next_env[v] = ty
        return ("count", v, ty, _rewrite_stage_expr(cond, next_env, process_by_case))
    if tag == "sum":
        _, v, ty, body, cond = expr
        next_env = dict(env)
        next_env[v] = ty
        return ("sum", v, ty,
                _rewrite_stage_expr(body, next_env, process_by_case),
                _rewrite_stage_expr(cond, next_env, process_by_case) if cond else None)
    if tag in ("min", "max"):
        return (tag,
                _rewrite_stage_expr(expr[1], env, process_by_case),
                _rewrite_stage_expr(expr[2], env, process_by_case))
    if tag == "ite":
        return ("ite",
                _rewrite_stage_expr(expr[1], env, process_by_case),
                _rewrite_stage_expr(expr[2], env, process_by_case),
                _rewrite_stage_expr(expr[3], env, process_by_case))
    return expr


def _rewrite_stage_binders(binders, p, q, process_by_case):
    env = {}
    out_binders = []
    for binder in binders:
        ty_name = _type_name_from_binder(binder)
        if ty_name is not None:
            env[binder[1]] = ty_name
            if binder[3] is not None:
                binder = ("binder_typed", binder[1], binder[2],
                          _rewrite_stage_expr(binder[3], env, process_by_case))
        out_binders.append(binder)
    return (
        out_binders,
        _rewrite_stage_expr(p, env, process_by_case),
        _rewrite_stage_expr(q, env, process_by_case),
    )


def _process_state_var(name):
    return f"{name.lower()}_stage"


def _process_stage_enum(name):
    return f"{name}Stage"


def _collect_process(item, actors, cases):
    _, name, parts, loc = item
    if name not in cases:
        _err(f"process '{name}' has no matching case declaration", loc=loc)
    stages = None
    initial = None
    transitions = []
    for part in parts:
        tag = part[0]
        if tag == "biz_stages":
            if stages is not None:
                _err(f"process '{name}' declares stages more than once", loc=part[2])
            stages = part[1]
        elif tag == "biz_initial":
            if initial is not None:
                _err(f"process '{name}' declares initial more than once", loc=part[2])
            initial = part[1]
        elif tag == "biz_transition":
            transitions.append({
                "name": part[1],
                "src": part[2],
                "dst": part[3],
                "actor": part[4],
                "loc": part[5],
            })
    if not stages:
        _err(f"process '{name}' must declare stages", loc=loc)
    if initial is None:
        _err(f"process '{name}' must declare initial stage", loc=loc)
    if initial not in stages:
        _err(f"process '{name}' initial stage '{initial}' is not in stages", loc=loc)
    for tr in transitions:
        if tr["src"] not in stages:
            _err(f"transition '{tr['name']}' uses unknown source stage '{tr['src']}'", loc=tr["loc"])
        if tr["dst"] not in stages:
            _err(f"transition '{tr['name']}' uses unknown target stage '{tr['dst']}'", loc=tr["loc"])
        if tr["actor"] not in actors:
            _err(f"transition '{tr['name']}' uses undeclared actor '{tr['actor']}'", loc=tr["loc"])
    return {
        "name": name,
        "stages": stages,
        "initial": initial,
        "transitions": transitions,
        "state_var": _process_state_var(name),
        "enum": _process_stage_enum(name),
        "loc": loc,
    }


def expand_business(ast):
    """Expand business AST to a kernel spec AST."""
    _, name, items = ast
    actors = set()
    cases = {}
    process_items = []
    kpis = []
    policies = []
    goals = []

    for item in items:
        tag = item[0]
        if tag == "biz_actor":
            for actor in item[1]:
                actors.add(actor)
        elif tag == "biz_case":
            _, case_name, lo, hi, loc = item
            if case_name in cases:
                _err(f"duplicate case '{case_name}'", loc=loc)
            cases[case_name] = {"lo": lo, "hi": hi, "loc": loc}
        elif tag == "biz_process":
            process_items.append(item)
        elif tag == "biz_kpi":
            kpis.append(item)
        elif tag == "biz_policy":
            policies.append(item)
        elif tag == "biz_goal":
            goals.append(item)

    processes = []
    transition_names = set()
    for item in process_items:
        proc = _collect_process(item, actors, cases)
        for tr in proc["transitions"]:
            if tr["name"] in transition_names:
                _err(f"duplicate transition label '{tr['name']}'", loc=tr["loc"])
            transition_names.add(tr["name"])
        processes.append(proc)

    process_by_case = {}
    process_by_name = {p["name"]: p for p in processes}
    for proc in processes:
        process_by_case.setdefault(proc["name"], []).append(proc)

    kpi_infos = []
    kpi_names = set()
    for item in kpis:
        _, kname, case_name, stage, loc = item
        if kname in kpi_names:
            _err(f"duplicate kpi '{kname}'", loc=loc)
        kpi_names.add(kname)
        proc = process_by_name.get(case_name)
        if proc is None:
            _err(f"kpi '{kname}' refers to unknown process '{case_name}'", loc=loc)
        if stage not in proc["stages"]:
            _err(f"kpi '{kname}' refers to unknown stage '{stage}'", loc=loc)
        for tr in proc["transitions"]:
            if tr["src"] == stage and tr["dst"] != stage:
                _err(
                    f"kpi '{kname}' counts stage '{stage}', but transition '{tr['name']}' leaves it; "
                    "decrement KPI is not supported in fsl-biz v3",
                    loc=tr["loc"],
                )
        kpi_infos.append({
            "name": kname,
            "case": case_name,
            "stage": stage,
            "process": proc,
            "loc": loc,
        })

    out = []
    generated_names = []
    for case_name, data in cases.items():
        out.append(("type", case_name, data["lo"], data["hi"]))
    for proc in processes:
        out.append(("enum", proc["enum"], proc["stages"]))

    state_decls = []
    for proc in processes:
        state_decls.append(("decl", proc["state_var"], ("map", ("name", proc["name"]), ("name", proc["enum"]))))
    for kpi in kpi_infos:
        state_decls.append(("decl", kpi["name"], ("int",)))
    if state_decls:
        out.append(("state", state_decls))

    init_stmts = []
    for proc in processes:
        init_stmts.append((
            "forall_stmt",
            ("binder_typed", "c", proc["name"], None),
            [("assign", ("index", proc["state_var"], ("var", "c")), ("var", proc["initial"]), proc["loc"])],
            proc["loc"],
        ))
    for kpi in kpi_infos:
        init_stmts.append(("assign", ("var", kpi["name"]), ("num", 0), kpi["loc"]))
    if init_stmts:
        out.append(("init", init_stmts))

    kpis_by_transition = {}
    for kpi in kpi_infos:
        for tr in kpi["process"]["transitions"]:
            if tr["dst"] == kpi["stage"]:
                kpis_by_transition.setdefault(tr["name"], []).append(kpi)

    for proc in processes:
        for tr in proc["transitions"]:
            body = [
                ("requires",
                 ("bin", "==",
                  ("index", ("var", proc["state_var"]), ("var", "c")),
                  ("var", tr["src"])),
                 tr["loc"]),
                ("assign",
                 ("index", proc["state_var"], ("var", "c")),
                 ("var", tr["dst"]),
                 tr["loc"]),
            ]
            for kpi in kpis_by_transition.get(tr["name"], []):
                body.append(("assign",
                             ("var", kpi["name"]),
                             ("bin", "+", ("var", kpi["name"]), ("num", 1)),
                             tr["loc"]))
            out.append((
                "action",
                tr["name"],
                [("param_typed", "c", proc["name"])],
                body,
                tr["loc"],
                True,
                _meta(tr["name"], f"by {tr['actor']}"),
            ))

    for kpi in kpi_infos:
        proc = kpi["process"]
        cond = ("bin", "==",
                ("index", ("var", proc["state_var"]), ("var", "c")),
                ("var", kpi["stage"]))
        expr = ("bin", "==", ("var", kpi["name"]), ("count", "c", kpi["case"], cond))
        inv_name = f"_kpi_{kpi['name']}"
        out.append(("invariant", inv_name, expr, kpi["loc"], None))
        generated_names.append(inv_name)

    for item in policies:
        _, policy_id, text, body, loc = item
        meta = _meta(policy_id, text)
        if body[0] == "biz_policy_invariant":
            expr = _rewrite_stage_expr(body[1], {}, process_by_case)
            out.append(("invariant", policy_id, expr, loc, meta))
        elif body[0] == "biz_policy_responds":
            binders, p, q = _rewrite_stage_binders(body[1], body[2], body[3], process_by_case)
            out.append(("leadsto", policy_id, binders, p, q, loc, meta))

    for item in goals:
        _, goal_id, text, expr, loc = item
        out.append(("reachable", goal_id, _rewrite_stage_expr(expr, {}, process_by_case), loc, _meta(goal_id, text)))

    if generated_names:
        out.append(("__generated", generated_names))
    return ("spec", name, out)
