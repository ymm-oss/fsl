# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Dialect AST expansion for requirement-oriented FSL frontends."""
from __future__ import annotations

from copy import deepcopy
from pathlib import Path

from .compose import expand_compose
from .grammar import Ast, PARSER
from .model import FslError, eval_const


def _err(message, kind="type", loc=None, hint=None):
    raise FslError(message, kind=kind, loc=loc, hint=hint)


def _parse_file(path):
    src = Path(path).read_text(encoding="utf-8")
    ast = Ast().transform(PARSER.parse(src))
    if ast[0] == "compose":
        return expand_compose(ast, str(Path(path).parent))
    if ast[0] == "requirements":
        return _expand_requirements_with_display(ast, str(Path(path).parent))
    if ast[0] == "business":
        return expand_business(ast), {}
    if ast[0] == "governance":
        return expand_governance_with_display(ast, str(Path(path).parent))
    if ast[0] == "spec":
        return expand_spec_domains(ast), {}
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


def _meta_with_controls(req_id, text, control_ids, control_by_id):
    meta = _meta(req_id, text)
    if control_ids:
        meta["controls"] = [
            {"id": cid, "text": control_by_id[cid]["text"]}
            for cid in control_ids
        ]
    return meta


def _collect_verify_bounds(items):
    instances = {}
    values = {}
    locs = {"instances": {}, "values": {}, "block": None}
    seen = False
    for item in items:
        if item[0] != "verify_bounds":
            continue
        if seen:
            _err("verify block may appear at most once", loc=item[2])
        seen = True
        locs["block"] = item[2]
        for bound in item[1]:
            tag = bound[0]
            if tag == "verify_instances":
                _, name, n, loc = bound
                if name in instances:
                    _err(f"duplicate instances bound for '{name}'", loc=loc)
                instances[name] = n
                locs["instances"][name] = loc
            elif tag == "verify_values":
                _, name, lo, hi, loc = bound
                if name in values:
                    _err(f"duplicate values bound for '{name}'", loc=loc)
                values[name] = (lo, hi)
                locs["values"][name] = loc
    return instances, values, locs


def _collect_entity_number_locs(items):
    """Scan `entity`/`number` declarations into name->loc maps, with duplicate and
    entity/number-conflict checks. Shared by the requirements dialect and `spec`."""
    entity_locs = {}
    number_locs = {}
    for item in items:
        tag = item[0]
        if tag == "entity":
            _, entity_name, loc = item
            if entity_name in entity_locs:
                _err(f"duplicate entity '{entity_name}'", loc=loc)
            if entity_name in number_locs:
                _err(f"'{entity_name}' cannot be both entity and number", loc=loc)
            entity_locs[entity_name] = loc
        elif tag == "number":
            _, number_name, loc = item
            if number_name in number_locs:
                _err(f"duplicate number '{number_name}'", loc=loc)
            if number_name in entity_locs:
                _err(f"'{number_name}' cannot be both entity and number", loc=loc)
            number_locs[number_name] = loc
    return entity_locs, number_locs


def _entity_number_to_types(entity_locs, number_locs, instances, values, bound_locs):
    """Lower `entity`/`number` declarations to kernel `type X = lo..hi` decls using
    the verify-block bounds. Shared by the requirements dialect and kernel `spec`."""
    out = []
    for entity_name, loc in entity_locs.items():
        if entity_name not in instances:
            _err(f"entity '{entity_name}' has no 'instances' bound in verify block", loc=loc)
        n = instances[entity_name]
        if n < 1:
            _err(f"entity '{entity_name}' instances bound must be >= 1", loc=bound_locs["instances"][entity_name])
        out.append(("type", entity_name, ("num", 0), ("num", n - 1)))
    for number_name, loc in number_locs.items():
        if number_name not in values:
            _err(f"number '{number_name}' has no 'values' bound in verify block", loc=loc)
        lo, hi = values[number_name]
        out.append(("type", number_name, lo, hi))
    for entity_name in instances:
        if entity_name not in entity_locs:
            _err(
                f"verify instances for undeclared entity '{entity_name}'",
                loc=bound_locs["instances"][entity_name],
            )
    for number_name in values:
        if number_name not in number_locs:
            _err(
                f"verify values for undeclared number '{number_name}'",
                loc=bound_locs["values"][number_name],
            )
    return out


def expand_spec_domains(ast):
    """Desugar `entity`/`number` declarations in a kernel `spec` into `type` decls
    using the `verify` block bounds. A spec without `entity`/`number` is returned
    unchanged, so existing kernel specs are unaffected."""
    _, name, items = ast
    if not any(it[0] in ("entity", "number") for it in items):
        return ast
    instances, values, bound_locs = _collect_verify_bounds(items)
    entity_locs, number_locs = _collect_entity_number_locs(items)
    type_items = _entity_number_to_types(entity_locs, number_locs, instances, values, bound_locs)
    rest = [it for it in items if it[0] not in ("entity", "number", "verify_bounds")]
    return ("spec", name, type_items + rest)


def _with_meta(item, meta):
    tag = item[0]
    if tag == "req_action":
        return item[:6] + (meta, item[7])
    if tag == "action":
        return item[:6] + (meta,)
    if tag in ("invariant", "reachable"):
        return item[:4] + (meta,)
    if tag == "leadsto":
        return item[:6] + (meta,) + item[7:]
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


def _process_field_state_var(process_name, field_name):
    return f"{process_name.lower()}_{field_name}"


def _param_name(param):
    return param[1]


def _collect_requirements_processes(items, entity_locs, number_locs):
    process_items = [item for item in items if item[0] == "biz_process"]
    processes = []
    process_by_name = {}
    transition_names = set()
    for item in process_items:
        proc = _collect_process(
            item,
            set(),
            entity_locs,
            allow_data=True,
            validate_actors=False,
        )
        if proc["name"] in process_by_name:
            _err(f"duplicate process '{proc['name']}'", loc=proc["loc"])
        field_names = set()
        fields = []
        for field in proc["fields"]:
            _, fname, ty = field
            if fname in field_names:
                _err(f"duplicate carried process field '{fname}'", loc=proc["loc"])
            field_names.add(fname)
            if not isinstance(ty, str) or ty not in number_locs:
                _err("carried process field must be a number type", loc=proc["loc"])
            fields.append({
                "name": fname,
                "type": ty,
                "state_var": _process_field_state_var(proc["name"], fname),
            })
        proc["fields"] = fields

        for tr in proc["transitions"]:
            if tr["name"] in transition_names:
                _err(f"duplicate transition label '{tr['name']}'", loc=tr["loc"])
            transition_names.add(tr["name"])
            extras = tr.get("extras") or {}
            inputs = list(extras.get("inputs", []))
            input_names = set()
            for param in inputs:
                pname = _param_name(param)
                if pname == "c":
                    _err("transition input conflicts with generated entity binder 'c'", loc=tr["loc"])
                if pname in field_names:
                    _err(
                        f"transition input '{pname}' conflicts with carried process field",
                        loc=tr["loc"],
                    )
                if pname in input_names:
                    _err(f"duplicate transition input '{pname}'", loc=tr["loc"])
                input_names.add(pname)
            sets = list(extras.get("sets", []))
            for assign in sets:
                if assign[1] not in field_names:
                    _err(
                        f"transition set references unknown carried process field '{assign[1]}'",
                        loc=tr["loc"],
                    )
            tr["inputs"] = inputs
            tr["guard"] = extras.get("guard")
            tr["sets"] = sets
            tr["covers"] = extras.get("covers")
        processes.append(proc)
        process_by_name[proc["name"]] = proc
    return processes, process_by_name


def _resolve_process_expr(expr, proc):
    fields = {
        field["name"]: ("index", ("var", field["state_var"]), ("var", "c"))
        for field in proc["fields"]
    }
    return _subst_expr(expr, fields)


def _expand_requirements_process(proc, values, consts):
    out = []
    action_maps = []
    state_maps = []

    out.append(("enum", proc["enum"], proc["stages"]))
    state_decls = [
        ("decl", proc["state_var"], ("map", ("name", proc["name"]), ("name", proc["enum"]))),
    ]
    for field in proc["fields"]:
        state_decls.append((
            "decl",
            field["state_var"],
            ("map", ("name", proc["name"]), ("name", field["type"])),
        ))
    out.append(("state", state_decls))

    init_body = [
        (
            "assign",
            ("index", proc["state_var"], ("var", "c")),
            ("var", proc["initial"]),
            proc["loc"],
        )
    ]
    for field in proc["fields"]:
        lo_expr, _hi_expr = values[field["type"]]
        init_body.append((
            "assign",
            ("index", field["state_var"], ("var", "c")),
            ("num", eval_const(lo_expr, consts, {})),
            proc["loc"],
        ))
    _merge_init(out, [(
        "forall_stmt",
        ("binder_typed", "c", proc["name"], None),
        init_body,
        proc["loc"],
    )])

    state_maps.append((
        "map",
        proc["state_var"],
        ("binder_typed", "c", proc["name"], None),
        ("index", ("var", proc["state_var"]), ("var", "c")),
        proc["loc"],
    ))

    for tr in proc["transitions"]:
        loc = tr["loc"]
        body = [
            (
                "requires",
                (
                    "bin",
                    "==",
                    ("index", ("var", proc["state_var"]), ("var", "c")),
                    ("var", tr["src"]),
                ),
                loc,
            )
        ]
        if tr["guard"] is not None:
            body.append(("requires", _resolve_process_expr(tr["guard"], proc), loc))
        body.append((
            "assign",
            ("index", proc["state_var"], ("var", "c")),
            ("var", tr["dst"]),
            loc,
        ))
        for assign in tr["sets"]:
            _, fname, expr = assign
            field = next(f for f in proc["fields"] if f["name"] == fname)
            body.append((
                "assign",
                ("index", field["state_var"], ("var", "c")),
                _resolve_process_expr(expr, proc),
                loc,
            ))

        meta = _meta(*tr["covers"]) if tr["covers"] is not None else _meta(tr["name"], f"by {tr['actor']}")
        params = [("param_typed", "c", proc["name"])] + deepcopy(tr["inputs"])
        out.append(("action", tr["name"], params, body, loc, True, meta))
        action_maps.append((
            "action_map",
            tr["name"],
            ["c"] + [_param_name(p) for p in tr["inputs"]],
            ("action", tr["name"], [("var", "c")]),
            loc,
        ))

    return out, state_maps, action_maps


def _business_action_actor_map(abs_ast):
    if abs_ast[0] != "spec":
        return {}
    actors = {}
    for item in abs_ast[2]:
        if item[0] != "action" or len(item) <= 6:
            continue
        meta = item[6]
        if not isinstance(meta, dict):
            continue
        text = meta.get("text")
        if not isinstance(text, str) or not text.startswith("by "):
            continue
        actor = text[3:].strip()
        if actor:
            actors[item[1]] = actor
    return actors


def _check_auto_mapped_process_actors(processes, abs_ast):
    business_actors = _business_action_actor_map(abs_ast)
    if not business_actors:
        return
    for proc in processes:
        for tr in proc["transitions"]:
            name = tr["name"]
            biz_actor = business_actors.get(name)
            if biz_actor is None:
                continue
            req_actor = tr["actor"]
            if req_actor != biz_actor:
                _err(
                    f"transition '{name}' is auto-mapped to business action '{name}' "
                    f"but its actor 'by {req_actor}' does not match the business actor "
                    f"'by {biz_actor}'; rename the action or write an explicit map",
                    loc=tr["loc"],
                )


def _lower_acceptance_expect(expect, process_by_name):
    if expect[0] == "acceptance_expect":
        return expect[1]
    _, entity, n, stage, loc = expect
    proc = process_by_name.get(entity)
    if proc is None:
        _err(f"expect references entity '{entity}' with no process", loc=loc)
    if stage not in proc["stages"]:
        _err(f"stage '{stage}' is not declared for process '{entity}'", loc=loc)
    return (
        "bin",
        "==",
        ("index", ("var", proc["state_var"]), ("num", n)),
        ("var", stage),
    )


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
    out.append((
        "action",
        "tick",
        [],
        tick_body,
        time_block[2],
        False,
        None,
        False,
        {"kind": "time_tick", "urgent_actions": tuple(urgent_names)},
    ))
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
    auto_state_maps = []
    action_aliases = {}
    implements = None
    acceptances = []
    forbiddens = []
    time_block = None
    deadlines = []
    requirement_ids = []
    generated_names = []
    consts = _collect_consts(items)
    instances, values, bound_locs = _collect_verify_bounds(items)
    entity_locs, number_locs = _collect_entity_number_locs(items)
    process_entity_locs = {}
    for item in items:
        if item[0] == "biz_process":
            _, entity_name, _fields, _parts, loc = item
            process_entity_locs.setdefault(entity_name, loc)

    for entity_name, loc in process_entity_locs.items():
        if entity_name in number_locs:
            _err(f"'{entity_name}' cannot be both process entity and number", loc=loc)
        entity_locs.setdefault(entity_name, loc)

    out.extend(_entity_number_to_types(entity_locs, number_locs, instances, values, bound_locs))

    processes, process_by_name = _collect_requirements_processes(items, entity_locs, number_locs)
    process_by_ast_name = {proc["name"]: proc for proc in processes}
    kpi_infos = _build_kpi_metadata(
        [item for item in items if item[0] == "biz_kpi"],
        process_by_name,
    )

    for item in items:
        tag = item[0]
        if tag in ("entity", "number", "verify_bounds", "biz_kpi"):
            continue
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
        elif tag == "biz_process":
            expanded, state_maps, maps = _expand_requirements_process(
                process_by_ast_name[item[1]],
                values,
                consts,
            )
            out.extend(expanded)
            auto_state_maps.extend(state_maps)
            action_maps.extend(maps)
            for action in maps:
                action_aliases.setdefault(action[1], []).append(action[1])
        elif tag == "acceptance":
            _, ac_id, text, steps, expect, loc = item
            if expect is None:
                _err(f"acceptance '{ac_id}' missing expect", loc=loc)
            acceptances.append({
                "id": ac_id,
                "text": text,
                "steps": steps,
                "expect": _lower_acceptance_expect(expect, process_by_name),
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
        _check_auto_mapped_process_actors(processes, implements["abs_ast"])
        mapping_items = [("impl", name), ("abs", implements["abs"])]
        mapping_items.extend(implements["maps"])
        mapping_items.extend(auto_state_maps)
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
    kpi_metadata = _project_kpi_metadata(kpi_infos)
    if kpi_metadata:
        out.append(("__kpis", kpi_metadata))
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
            _err("stage(...) expects a bound entity variable", loc=expr[2] if len(expr) > 2 else None)
        var_name = arg[1]
        case_ty = env.get(var_name)
        if case_ty is None:
            _err(f"stage({var_name}) cannot be resolved; '{var_name}' is not a typed entity binder",
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


def _collect_process(item, actors, cases, *, allow_data=False, validate_actors=True):
    _, name, fields_node, parts, loc = item
    if name not in cases:
        _err(f"process '{name}' has no matching entity declaration", loc=loc)
    fields = []
    if fields_node is not None:
        if not allow_data:
            _err(
                "data guards/fields are a requirements-layer feature; business processes are pure stage graphs",
                loc=fields_node[2],
            )
        fields = fields_node[1]
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
            extras = part[5]
            if extras and not allow_data:
                _err(
                    "data guards/fields are a requirements-layer feature; business processes are pure stage graphs",
                    loc=part[6],
                )
            transitions.append({
                "name": part[1],
                "src": part[2],
                "dst": part[3],
                "actor": part[4],
                "extras": extras,
                "loc": part[6],
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
        if validate_actors and tr["actor"] not in actors:
            _err(f"transition '{tr['name']}' uses undeclared actor '{tr['actor']}'", loc=tr["loc"])
    return {
        "name": name,
        "fields": fields,
        "stages": stages,
        "initial": initial,
        "transitions": transitions,
        "state_var": _process_state_var(name),
        "enum": _process_stage_enum(name),
        "loc": loc,
    }


def _collect_business_entities(items):
    actors = set()
    entities = {}
    cases = {}
    process_items = []
    kpis = []
    controls = []
    policies = []
    goals = []
    instances, values, bound_locs = _collect_verify_bounds(items)

    for item in items:
        tag = item[0]
        if tag == "biz_actor":
            for actor in item[1]:
                actors.add(actor)
        elif tag == "entity":
            _, entity_name, loc = item
            if entity_name in entities:
                _err(f"duplicate entity '{entity_name}'", loc=loc)
            entities[entity_name] = loc
        elif tag == "biz_process":
            process_items.append(item)
        elif tag == "biz_kpi":
            kpis.append(item)
        elif tag == "biz_control":
            controls.append(_control_info(item))
        elif tag == "biz_policy":
            policies.append(item)
        elif tag == "biz_goal":
            goals.append(item)

    for entity_name, loc in entities.items():
        if entity_name not in instances:
            _err(f"entity '{entity_name}' has no 'instances' bound in verify block", loc=loc)
        n = instances[entity_name]
        if n < 1:
            _err(f"entity '{entity_name}' instances bound must be >= 1", loc=bound_locs["instances"][entity_name])
        cases[entity_name] = {
            "lo": ("num", 0),
            "hi": ("num", n - 1),
            "loc": loc,
        }
    for entity_name in instances:
        if entity_name not in entities:
            _err(
                f"verify instances for undeclared entity '{entity_name}'",
                loc=bound_locs["instances"][entity_name],
            )
    for number_name in values:
        _err(
            f"verify values for undeclared number '{number_name}'",
            loc=bound_locs["values"][number_name],
        )

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

    _validate_controls_and_satisfaction(controls, policies, goals)

    return actors, cases, processes, kpis, controls, policies, goals, process_by_case, process_by_name


def _control_info(item):
    _, control_id, text, attrs, loc = item
    owner = None
    severity = None
    applies_to = []
    for attr in attrs:
        tag = attr[0]
        if tag == "control_owner":
            if owner is not None:
                _err(f"control '{control_id}' declares owner more than once", loc=loc)
            owner = attr[1]
        elif tag == "control_severity":
            if severity is not None:
                _err(f"control '{control_id}' declares severity more than once", loc=loc)
            severity = attr[1]
        elif tag == "control_applies_to":
            applies_to.append(attr[1])
    return {
        "id": control_id,
        "text": text,
        "owner": owner,
        "severity": severity,
        "applies_to": applies_to,
        "loc": loc,
    }


def _satisfies_refs(item):
    return item[5] if len(item) > 5 and item[5] is not None else []


def _validate_controls_and_satisfaction(controls, policies, goals):
    control_by_id = {}
    for control in controls:
        cid = control["id"]
        if cid in control_by_id:
            _err(f"duplicate control '{cid}'", loc=control["loc"])
        control_by_id[cid] = control

    used = set()
    for item in list(policies) + list(goals):
        for cid in _satisfies_refs(item):
            if cid not in control_by_id:
                _err(
                    f"{item[0].replace('biz_', '')} '{item[1]}' satisfies unknown control '{cid}'",
                    loc=item[4],
                )
            used.add(cid)
    return control_by_id, used


def _build_kpi_metadata(kpis, process_by_name):
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
        kpi_infos.append({
            "name": kname,
            "case": case_name,
            "stage": stage,
            "process": proc,
            "loc": loc,
        })
    return kpi_infos


def _project_kpi_metadata(kpi_infos):
    kpi_metadata = []
    for kpi in kpi_infos:
        state_var = _process_state_var(kpi["case"])
        cond = (
            "bin",
            "==",
            ("index", ("var", state_var), ("var", "c")),
            ("var", kpi["stage"]),
        )
        kpi_metadata.append({
            "name": kpi["name"],
            "entity": kpi["case"],
            "stage": kpi["stage"],
            "expr": ("count", "c", kpi["case"], cond),
        })
    return kpi_metadata


def _process_for_case(case_name, process_by_case, loc):
    processes = process_by_case.get(case_name, [])
    if not processes:
        _err(f"entity '{case_name}' has no process", loc=loc)
    if len(processes) > 1:
        _err(
            f"entity '{case_name}' has multiple processes; natural stage syntax is ambiguous",
            loc=loc,
        )
    return processes[0]


def _stage_is(case_name, var_name, stage, process_by_case, loc):
    proc = _process_for_case(case_name, process_by_case, loc)
    if stage not in proc["stages"]:
        _err(f"stage '{stage}' is not declared for process '{case_name}'", loc=loc)
    return (
        "bin",
        "==",
        ("index", ("var", proc["state_var"]), ("var", var_name)),
        ("var", stage),
    )


def _any_stage(case_name, var_name, stages, process_by_case, loc):
    return _or_all([
        _stage_is(case_name, var_name, stage, process_by_case, loc)
        for stage in stages
    ])


def _generate_business_items(cases, processes, kpi_infos, controls, policies, goals, process_by_case):
    out = []
    control_by_id, used_controls = _validate_controls_and_satisfaction(controls, policies, goals)
    for case_name, data in cases.items():
        out.append(("type", case_name, data["lo"], data["hi"]))
    for proc in processes:
        out.append(("enum", proc["enum"], proc["stages"]))

    state_decls = []
    for proc in processes:
        state_decls.append(("decl", proc["state_var"], ("map", ("name", proc["name"]), ("name", proc["enum"]))))
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
    if init_stmts:
        out.append(("init", init_stmts))

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
            out.append((
                "action",
                tr["name"],
                [("param_typed", "c", proc["name"])],
                body,
                tr["loc"],
                True,
                _meta(tr["name"], f"by {tr['actor']}"),
            ))

    if processes:
        sink_exprs = []
        for proc in processes:
            outgoing = {tr["src"] for tr in proc["transitions"]}
            sinks = [stage for stage in proc["stages"] if stage not in outgoing]
            if not sinks:
                sink_exprs = None
                break
            sink_exprs.append((
                "forall",
                ("binder_typed", "c", proc["name"], None),
                _any_stage(proc["name"], "c", sinks, process_by_case, proc["loc"]),
            ))
        if sink_exprs:
            out.append(("terminal", _and_all(sink_exprs), None))

    kpi_metadata = _project_kpi_metadata(kpi_infos)
    if kpi_metadata:
        out.append(("__kpis", kpi_metadata))

    if controls:
        satisfactions = []
        for item in policies:
            for cid in _satisfies_refs(item):
                satisfactions.append({
                    "element": "policy",
                    "id": item[1],
                    "control": cid,
                    "loc": item[4],
                })
        for item in goals:
            for cid in _satisfies_refs(item):
                satisfactions.append({
                    "element": "goal",
                    "id": item[1],
                    "control": cid,
                    "loc": item[4],
                })
        out.append(("__controls", {
            "controls": [
                {
                    "id": control["id"],
                    "text": control["text"],
                    "owner": control["owner"],
                    "severity": control["severity"],
                    "applies_to": list(control["applies_to"]),
                    "loc": control["loc"],
                }
                for control in controls
            ],
            "satisfies": satisfactions,
        }))
        unused = sorted(set(control_by_id) - used_controls)
        if unused:
            out.append(("__warnings", [
                {
                    "kind": "unused_control",
                    "element": "control",
                    "name": cid,
                    "loc": control_by_id[cid]["loc"],
                    "hint": "no policy or goal declares `satisfies` for this control",
                }
                for cid in unused
            ]))

    for item in policies:
        _, policy_id, text, body, loc = item[:5]
        control_refs = _satisfies_refs(item)
        meta = _meta_with_controls(policy_id, text, control_refs, control_by_id)
        if body[0] == "biz_policy_invariant":
            expr = _rewrite_stage_expr(body[1], {}, process_by_case)
            out.append(("invariant", policy_id, expr, loc, meta))
        elif body[0] == "biz_policy_responds":
            binders, p, q = _rewrite_stage_binders(body[1], body[2], body[3], process_by_case)
            out.append(("leadsto", policy_id, binders, p, q, loc, meta, None, body[4]))
        elif body[0] == "biz_policy_eventually":
            _, case_name, source_stage, target_stages = body
            binder = ("binder_typed", "c", case_name, None)
            p = _stage_is(case_name, "c", source_stage, process_by_case, loc)
            q = _any_stage(case_name, "c", target_stages, process_by_case, loc)
            out.append(("leadsto", policy_id, [binder], p, q, loc, meta))

    for item in goals:
        _, goal_id, text, body, loc = item[:5]
        control_refs = _satisfies_refs(item)
        if body[0] == "biz_goal_expr":
            expr = _rewrite_stage_expr(body[1], {}, process_by_case)
        elif body[0] == "biz_goal_some_stage":
            _, case_name, stage = body
            expr = (
                "exists",
                ("binder_typed", "c", case_name, None),
                _stage_is(case_name, "c", stage, process_by_case, loc),
            )
        elif body[0] == "biz_goal_all_stage":
            _, case_name, stages = body
            expr = (
                "forall",
                ("binder_typed", "c", case_name, None),
                _any_stage(case_name, "c", stages, process_by_case, loc),
            )
        else:
            _err(f"unknown business goal body '{body[0]}'", loc=loc)
        out.append(("reachable", goal_id, expr, loc, _meta_with_controls(goal_id, text, control_refs, control_by_id)))

    return out


def expand_business(ast):
    """Expand business AST to a kernel spec AST."""
    _, name, items = ast
    _, cases, processes, kpis, controls, policies, goals, process_by_case, process_by_name = (
        _collect_business_entities(items)
    )
    kpi_infos = _build_kpi_metadata(kpis, process_by_name)
    out = _generate_business_items(
        cases, processes, kpi_infos, controls, policies, goals, process_by_case,
    )
    return ("spec", name, out)


def _resolve_relative(base_dir, path):
    return Path(base_dir) / path


def _collect_governance_controls(items):
    controls = {}
    authorities = []
    for item in items:
        if item[0] == "biz_control":
            control = _control_info(item)
            cid = control["id"]
            if cid in controls:
                _err(f"duplicate control '{cid}'", loc=control["loc"])
            controls[cid] = control
        elif item[0] == "gov_authority":
            authorities.append({
                "authority": item[1],
                "owns": list(item[2]),
                "loc": item[3],
            })
    for authority in authorities:
        for cid in authority["owns"]:
            if cid not in controls:
                _err(
                    f"authority '{authority['authority']}' owns unknown control '{cid}'",
                    loc=authority["loc"],
                )
    return controls, authorities


def _control_refs_from_meta(meta):
    refs = []
    if not isinstance(meta, dict):
        return refs
    for control in meta.get("controls") or []:
        if isinstance(control, dict) and control.get("id"):
            refs.append(control["id"])
    return refs


def _business_catalog_from_ast(ast):
    if ast[0] != "spec":
        _err("governance delegates must reference a business spec", kind="type")
    policies = {}
    goals = {}
    controls = {}
    satisfies = {}
    for item in ast[2]:
        tag = item[0]
        if tag in ("invariant", "leadsto"):
            name = item[1]
            meta = item[4] if tag == "invariant" and len(item) > 4 else item[6] if tag == "leadsto" and len(item) > 6 else None
            policies[name] = {"id": name, "meta": meta}
            for cid in _control_refs_from_meta(meta):
                satisfies.setdefault(cid, []).append({"kind": "policy", "id": name})
        elif tag == "reachable":
            name = item[1]
            meta = item[4] if len(item) > 4 else None
            goals[name] = {"id": name, "meta": meta}
            for cid in _control_refs_from_meta(meta):
                satisfies.setdefault(cid, []).append({"kind": "goal", "id": name})
        elif tag == "__controls":
            for control in item[1].get("controls", []):
                controls[control["id"]] = control
            for sat in item[1].get("satisfies", []):
                kind = sat.get("element")
                if kind in ("policy", "goal"):
                    satisfies.setdefault(sat["control"], []).append({"kind": kind, "id": sat["id"]})
    return {
        "policies": policies,
        "goals": goals,
        "controls": controls,
        "satisfies": satisfies,
    }


def _dedupe_artifacts(artifacts):
    seen = set()
    out = []
    for artifact in artifacts:
        key = (artifact["kind"], artifact["id"])
        if key in seen:
            continue
        seen.add(key)
        out.append(artifact)
    return out


def _validate_delegate(item, base_dir, controls):
    _, business_name, path, parts, loc = item
    abs_path = _resolve_relative(base_dir, path)
    if not abs_path.is_file():
        _err(f"file not found: {path}", kind="io", loc=loc)
    ast, _display_names = _parse_file(abs_path)
    if ast[0] != "spec" or ast[1] != business_name:
        _err(f"spec name mismatch: expected '{business_name}', got '{ast[1]}'", loc=loc)
    catalog = _business_catalog_from_ast(ast)
    required = []
    explicit = {}
    for part in parts:
        if part[0] == "gov_require":
            cid = part[1]
            if cid not in controls:
                _err(f"delegates '{business_name}' requires unknown control '{cid}'", loc=part[2])
            required.append(cid)
        elif part[0] == "gov_satisfaction":
            cid = part[1]
            if cid not in controls:
                _err(
                    f"delegates '{business_name}' maps unknown control '{cid}'",
                    loc=part[3],
                )
            refs = []
            for ref in part[2]:
                kind, artifact_id, ref_loc = ref
                collection = catalog["policies"] if kind == "policy" else catalog["goals"]
                if artifact_id not in collection:
                    _err(
                        f"delegates '{business_name}' references unknown {kind} '{artifact_id}'",
                        loc=ref_loc,
                    )
                refs.append({"kind": kind, "id": artifact_id})
            explicit.setdefault(cid, []).extend(refs)

    required = list(dict.fromkeys(required))
    satisfied = {cid: list(catalog["satisfies"].get(cid, [])) for cid in required}
    for cid, refs in explicit.items():
        satisfied.setdefault(cid, []).extend(refs)

    missing = [cid for cid in required if not satisfied.get(cid)]
    if missing:
        _err(
            f"delegates '{business_name}' requires unsatisfied control(s): {', '.join(missing)}",
            loc=loc,
            hint="add `policy ... satisfies CTRL` in the business spec or map it with `CTRL is satisfied_by ...`",
        )

    return {
        "business": business_name,
        "path": str(abs_path),
        "required": required,
        "satisfied": {
            cid: _dedupe_artifacts(satisfied.get(cid, []))
            for cid in required
        },
        "loc": loc,
    }


def _validate_preservation(item, base_dir, controls):
    _, name, parts, loc = item
    before = None
    after = None
    preserves = []
    refinement = None
    for part in parts:
        tag = part[0]
        if tag == "preservation_before":
            if before is not None:
                _err(f"preservation '{name}' declares before more than once", loc=part[3])
            before = {"spec": part[1], "path": str(_resolve_relative(base_dir, part[2])), "source": part[2], "loc": part[3]}
        elif tag == "preservation_after":
            if after is not None:
                _err(f"preservation '{name}' declares after more than once", loc=part[3])
            after = {"spec": part[1], "path": str(_resolve_relative(base_dir, part[2])), "source": part[2], "loc": part[3]}
        elif tag == "preservation_preserve":
            if part[1] not in controls:
                _err(f"preservation '{name}' preserves unknown control '{part[1]}'", loc=part[2])
            preserves.append(part[1])
        elif tag == "preservation_refinement":
            if refinement is not None:
                _err(f"preservation '{name}' declares checked_by more than once", loc=part[2])
            refinement = {"path": str(_resolve_relative(base_dir, part[1])), "source": part[1], "loc": part[2]}
    if before is None:
        _err(f"preservation '{name}' missing before spec", loc=loc)
    if after is None:
        _err(f"preservation '{name}' missing after spec", loc=loc)
    if refinement is None:
        _err(f"preservation '{name}' missing checked_by refinement", loc=loc)
    if not preserves:
        _err(f"preservation '{name}' must preserve at least one control", loc=loc)
    for entry in (before, after, refinement):
        if not Path(entry["path"]).is_file():
            _err(f"file not found: {entry['source']}", kind="io", loc=entry["loc"])
    for side in (before, after):
        ast, _display_names = _parse_file(side["path"])
        if ast[0] != "spec" or ast[1] != side["spec"]:
            _err(f"spec name mismatch: expected '{side['spec']}', got '{ast[1]}'", loc=side["loc"])
    return {
        "name": name,
        "before": before,
        "after": after,
        "preserve": list(dict.fromkeys(preserves)),
        "refinement": refinement,
        "loc": loc,
    }


def _governance_kernel_items(name, metadata):
    return [
        ("type", "_GovernanceUnit", ("num", 0), ("num", 0)),
        ("state", [("decl", "_governance_ok", ("bool",))]),
        ("init", [("assign", ("var", "_governance_ok"), ("bool", True), None)]),
        (
            "action",
            "_governance_noop",
            [],
            [("requires", ("bool", False), None)],
            None,
            False,
            _meta("GOV", f"governance catalog {name}"),
        ),
        (
            "invariant",
            "_governance_catalog_ok",
            ("bin", "==", ("var", "_governance_ok"), ("bool", True)),
            None,
            _meta("GOV", f"governance catalog {name}"),
        ),
        ("terminal", ("bool", True), None),
        ("__generated", ["_governance_noop", "_governance_catalog_ok"]),
        ("__governance", metadata),
    ]


def expand_governance_with_display(ast, base_dir):
    """Validate a governance catalog and expand it to a metadata-only kernel spec."""
    _, name, items = ast
    controls, authorities = _collect_governance_controls(items)
    delegates = []
    preservations = []
    for item in items:
        if item[0] == "gov_delegates":
            delegates.append(_validate_delegate(item, base_dir, controls))
        elif item[0] == "gov_preservation":
            preservations.append(_validate_preservation(item, base_dir, controls))
    metadata = {
        "name": name,
        "controls": [
            {
                "id": control["id"],
                "text": control["text"],
                "owner": control["owner"],
                "severity": control["severity"],
                "applies_to": list(control["applies_to"]),
                "loc": control["loc"],
            }
            for control in controls.values()
        ],
        "authorities": authorities,
        "delegates": delegates,
        "preservations": preservations,
    }
    return ("spec", name, _governance_kernel_items(name, metadata)), {}
