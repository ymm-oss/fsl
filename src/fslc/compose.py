# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Compose spec expansion: merge component specs into a single spec AST."""
from __future__ import annotations

from copy import deepcopy
from pathlib import Path

from .grammar import PARSER, Ast
from .model import FslError
from .literate import extract_literate_source, is_literate_source


def _compose_err(message, kind="type", loc=None):
    raise FslError(message, kind=kind, loc=loc)


def _prefix(name, alias):
    return f"{alias}__{name}"


def _parse_file(path):
    src = Path(path).read_text(encoding="utf-8")
    if is_literate_source(src):
        src = extract_literate_source(src)
    return Ast().transform(PARSER.parse(src))


def _collect_component_names(items):
    consts, types, state, actions, props = set(), set(), set(), set(), set()
    for it in items:
        tag = it[0]
        if tag == "const":
            consts.add(it[1])
        elif tag in ("type", "enum", "struct"):
            types.add(it[1])
        elif tag == "state":
            for _, n, _ in it[1]:
                state.add(n)
        elif tag == "action":
            actions.add(it[1])
        elif tag in ("invariant", "reachable", "leadsto"):
            props.add(it[1])
    return consts, types, state, actions, props


def _resolve_qname(qname, aliases, loc=None):
    if isinstance(qname, tuple) and qname[0] == "qname":
        alias, local = qname[1], qname[2]
        if alias not in aliases:
            _compose_err(f"unknown alias '{alias}'", loc=loc)
        return _prefix(local, alias)
    return qname


def _resolve_type_ref(name_or_qname, aliases, component_types=None, loc=None):
    if isinstance(name_or_qname, tuple) and name_or_qname[0] == "qname":
        return _resolve_qname(name_or_qname, aliases, loc)
    if component_types and name_or_qname in component_types and len(aliases) == 1:
        return _prefix(name_or_qname, next(iter(aliases)))
    return name_or_qname


def _rewrite_type(ty_ast, aliases, component_types=None, component_consts=None):
    component_types = component_types or set()
    component_consts = component_consts or set()
    if ty_ast[0] == "name":
        n = ty_ast[1]
        if isinstance(n, tuple) and n[0] == "qname":
            return ("name", _resolve_qname(n, aliases))
        if n in component_types and len(aliases) == 1:
            return ("name", _prefix(n, next(iter(aliases))))
        return ty_ast
    if ty_ast[0] == "map":
        return (
            "map",
            _rewrite_type(ty_ast[1], aliases, component_types, component_consts),
            _rewrite_type(ty_ast[2], aliases, component_types, component_consts),
        )
    if ty_ast[0] == "set":
        return ("set", _rewrite_type(ty_ast[1], aliases, component_types, component_consts))
    if ty_ast[0] == "seq":
        cap = ty_ast[2]
        if cap[0] == "var" and cap[1] in component_consts and len(aliases) == 1:
            cap = ("var", _prefix(cap[1], next(iter(aliases))))
        return (
            "seq",
            _rewrite_type(ty_ast[1], aliases, component_types, component_consts),
            cap,
        )
    if ty_ast[0] == "option":
        return ("option", _rewrite_type(ty_ast[1], aliases, component_types, component_consts))
    return ty_ast


def _rewrite_binder(binder, aliases, component_types=None, component_consts=None):
    component_consts = component_consts or set()
    if binder[0] == "binder_range":
        _, v, lo, hi = binder
        lo = _rewrite_expr(lo, aliases, set(), component_types, component_consts)
        hi = _rewrite_expr(hi, aliases, set(), component_types, component_consts)
        return ("binder_range", v, lo, hi)
    _, v, ty_name, where = binder
    resolved = _resolve_type_ref(ty_name, aliases, component_types)
    if where is not None:
        where = _rewrite_expr(where, aliases, set(), component_types, component_consts)
    return ("binder_typed", v, resolved, where)


def _rewrite_params(params, aliases, component_types=None, component_consts=None):
    component_consts = component_consts or set()
    out = []
    for p in params:
        if p[0] == "param_range":
            _, n, lo, hi = p
            lo = _rewrite_expr(lo, aliases, set(), component_types, component_consts)
            hi = _rewrite_expr(hi, aliases, set(), component_types, component_consts)
            out.append(("param_range", n, lo, hi))
        else:
            _, n, ty_name = p
            out.append(("param_typed", n, _resolve_type_ref(ty_name, aliases, component_types)))
    return out


def _rewrite_alias_access(expr, aliases):
    if not isinstance(expr, tuple):
        return expr
    tag = expr[0]
    if tag == "field" and isinstance(expr[1], tuple) and expr[1][0] == "var" and expr[1][1] in aliases:
        return ("var", _prefix(expr[2], expr[1][1]))
    if tag == "field":
        return ("field", _rewrite_alias_access(expr[1], aliases), expr[2])
    if tag == "index":
        return ("index", _rewrite_alias_access(expr[1], aliases), expr[2])
    return expr


def _rewrite_expr(expr, aliases, comp_state, comp_types, comp_consts):
    if not isinstance(expr, tuple):
        return expr
    expr = _rewrite_alias_access(expr, aliases)
    tag = expr[0]
    if tag == "var":
        n = expr[1]
        if n in comp_state or n in comp_types or n in comp_consts:
            if len(aliases) != 1:
                _compose_err(f"ambiguous reference '{n}' across components")
            return ("var", _prefix(n, next(iter(aliases))))
        return expr
    if tag == "field":
        return ("field", _rewrite_expr(expr[1], aliases, comp_state, comp_types, comp_consts), expr[2])
    if tag == "index":
        return ("index",
                _rewrite_expr(expr[1], aliases, comp_state, comp_types, comp_consts),
                _rewrite_expr(expr[2], aliases, comp_state, comp_types, comp_consts))
    if tag == "method":
        return ("method",
                _rewrite_expr(expr[1], aliases, comp_state, comp_types, comp_consts),
                expr[2],
                [_rewrite_expr(a, aliases, comp_state, comp_types, comp_consts) for a in expr[3]])
    if tag in ("neg", "not", "abs", "old"):
        return (tag, _rewrite_expr(expr[1], aliases, comp_state, comp_types, comp_consts))
    if tag == "bin":
        return ("bin", expr[1],
                _rewrite_expr(expr[2], aliases, comp_state, comp_types, comp_consts),
                _rewrite_expr(expr[3], aliases, comp_state, comp_types, comp_consts))
    if tag == "is":
        return ("is",
                _rewrite_expr(expr[1], aliases, comp_state, comp_types, comp_consts),
                expr[2])
    if tag in ("forall", "exists"):
        return (tag, _rewrite_binder(expr[1], aliases, comp_types, comp_consts),
                _rewrite_expr(expr[2], aliases, comp_state, comp_types, comp_consts))
    if tag == "some":
        return ("some", _rewrite_expr(expr[1], aliases, comp_state, comp_types, comp_consts))
    if tag == "struct_lit":
        sname, fields = expr[1], expr[2]
        if sname in comp_types:
            sname = _prefix(sname, next(iter(aliases)))
        return ("struct_lit", sname,
                {k: _rewrite_expr(v, aliases, comp_state, comp_types, comp_consts) for k, v in fields.items()})
    if tag in ("set_lit", "seq_lit"):
        return (tag, [_rewrite_expr(e, aliases, comp_state, comp_types, comp_consts) for e in expr[1]])
    if tag == "count":
        _, v, ty, cond = expr
        ty = _resolve_type_ref(ty, aliases, comp_types)
        return ("count", v, ty, _rewrite_expr(cond, aliases, comp_state, comp_types, comp_consts))
    if tag == "sum":
        _, v, ty, body, cond = expr
        ty = _resolve_type_ref(ty, aliases, comp_types)
        return ("sum", v, ty,
                _rewrite_expr(body, aliases, comp_state, comp_types, comp_consts),
                _rewrite_expr(cond, aliases, comp_state, comp_types, comp_consts) if cond else None)
    if tag in ("min", "max"):
        return (tag,
                _rewrite_expr(expr[1], aliases, comp_state, comp_types, comp_consts),
                _rewrite_expr(expr[2], aliases, comp_state, comp_types, comp_consts))
    if tag in ("num", "bool", "none", "pat_none", "pat_some"):
        return expr
    return expr


def _rewrite_lvalue(lv, aliases, comp_state):
    lv = _rewrite_alias_access(lv, aliases)
    if lv[0] == "var" and lv[1] in comp_state:
        return ("var", _prefix(lv[1], next(iter(aliases))))
    if lv[0] == "index" and isinstance(lv[1], str) and lv[1] in comp_state:
        return ("index", _prefix(lv[1], next(iter(aliases))), lv[2])
    if lv[0] == "field_lv":
        base = lv[1]
        if base[0] == "index" and isinstance(base[1], str) and base[1] in comp_state:
            base = ("index", _prefix(base[1], next(iter(aliases))), base[2])
        else:
            base = _rewrite_alias_access(base, aliases)
            if isinstance(base, tuple) and base[0] == "var":
                return ("field_lv", base, lv[2])
            base = _rewrite_expr(base, aliases, comp_state, set(), set())
        return ("field_lv", base, lv[2])
    return lv


def _rewrite_stmt(stmt, aliases, comp_state, comp_types, comp_consts):
    tag = stmt[0]
    if tag == "assign":
        return ("assign",
                _rewrite_lvalue(stmt[1], aliases, comp_state),
                _rewrite_expr(stmt[2], aliases, comp_state, comp_types, comp_consts),
                stmt[3] if len(stmt) > 3 else None)
    if tag == "if":
        return ("if",
                _rewrite_expr(stmt[1], aliases, comp_state, comp_types, comp_consts),
                [_rewrite_stmt(s, aliases, comp_state, comp_types, comp_consts) for s in stmt[2]],
                [_rewrite_stmt(s, aliases, comp_state, comp_types, comp_consts) for s in stmt[3]],
                stmt[4] if len(stmt) > 4 else None)
    if tag == "forall_stmt":
        return ("forall_stmt", _rewrite_binder(stmt[1], aliases, comp_types, comp_consts),
                [_rewrite_stmt(s, aliases, comp_state, comp_types, comp_consts) for s in stmt[2]],
                stmt[3] if len(stmt) > 3 else None)
    return stmt


def _subst_expr(expr, param_map):
    if not isinstance(expr, tuple):
        return expr
    tag = expr[0]
    if tag == "var" and expr[1] in param_map:
        return deepcopy(param_map[expr[1]])
    if tag in ("num", "bool", "none", "pat_none", "pat_some", "var"):
        return expr
    if tag in ("neg", "not", "abs", "old"):
        return (tag, _subst_expr(expr[1], param_map))
    if tag == "bin":
        return ("bin", expr[1], _subst_expr(expr[2], param_map), _subst_expr(expr[3], param_map))
    if tag == "field":
        return ("field", _subst_expr(expr[1], param_map), expr[2])
    if tag == "index":
        return ("index", _subst_expr(expr[1], param_map), _subst_expr(expr[2], param_map))
    if tag == "method":
        return ("method", _subst_expr(expr[1], param_map), expr[2],
                [_subst_expr(a, param_map) for a in expr[3]])
    if tag == "is":
        return ("is", _subst_expr(expr[1], param_map), expr[2])
    if tag in ("forall", "exists"):
        return (tag, expr[1], _subst_expr(expr[2], param_map))
    if tag == "some":
        return ("some", _subst_expr(expr[1], param_map))
    if tag == "struct_lit":
        return ("struct_lit", expr[1], {k: _subst_expr(v, param_map) for k, v in expr[2].items()})
    if tag in ("set_lit", "seq_lit"):
        return (tag, [_subst_expr(e, param_map) for e in expr[1]])
    if tag == "count":
        return ("count", expr[1], expr[2], _subst_expr(expr[3], param_map))
    if tag == "sum":
        return ("sum", expr[1], expr[2], _subst_expr(expr[3], param_map),
                _subst_expr(expr[4], param_map) if expr[4] else None)
    if tag in ("min", "max"):
        return (tag, _subst_expr(expr[1], param_map), _subst_expr(expr[2], param_map))
    return expr


def _subst_stmt(stmt, param_map):
    tag = stmt[0]
    if tag == "assign":
        return ("assign", stmt[1], _subst_expr(stmt[2], param_map), stmt[3] if len(stmt) > 3 else None)
    if tag == "if":
        return ("if", _subst_expr(stmt[1], param_map),
                [_subst_stmt(s, param_map) for s in stmt[2]],
                [_subst_stmt(s, param_map) for s in stmt[3]],
                stmt[4] if len(stmt) > 4 else None)
    if tag == "forall_stmt":
        return ("forall_stmt", stmt[1], [_subst_stmt(s, param_map) for s in stmt[2]],
                stmt[3] if len(stmt) > 3 else None)
    return stmt


def _prefix_component_items(items, alias, display_names):
    consts, types, state, actions, props = _collect_component_names(items)
    out = []
    for it in items:
        tag = it[0]
        if tag == "const":
            pn = _prefix(it[1], alias)
            display_names[pn] = _display_logical(alias, it[1])
            out.append(("const", pn, _rewrite_expr(it[2], {alias}, state, types, consts)))
        elif tag == "type":
            pn = _prefix(it[1], alias)
            lo = _rewrite_expr(it[2], {alias}, state, types, consts)
            hi = _rewrite_expr(it[3], {alias}, state, types, consts)
            out.append(("type", pn, lo, hi, *it[4:]))
        elif tag == "enum":
            pn = _prefix(it[1], alias)
            out.append(("enum", pn, it[2], *it[3:]))
        elif tag == "struct":
            pn = _prefix(it[1], alias)
            fields = {
                fn: _rewrite_type(ft, {alias}, types, consts) for fn, ft in it[2].items()
            }
            out.append(("struct", pn, fields))
        elif tag == "state":
            decls = []
            for _, n, ty_ast in it[1]:
                pn = _prefix(n, alias)
                display_names[pn] = _display_logical(alias, n)
                decls.append(("decl", pn, _rewrite_type(ty_ast, {alias}, types, consts)))
            out.append(("state", decls))
        elif tag == "init":
            out.append(("init", [_rewrite_stmt(s, {alias}, state, types, consts) for s in it[1]]))
        elif tag == "action":
            aname, params, body, loc = it[1], it[2], it[3], it[4]
            fair = it[5] if len(it) > 5 else False
            meta = it[6] if len(it) > 6 else None
            pn = _prefix(aname, alias)
            display_names[pn] = _display_logical(alias, aname)
            new_body = []
            for bit in body:
                bt = bit[0]
                if bt == "requires":
                    new_body.append(("requires", _rewrite_expr(bit[1], {alias}, state, types, consts), bit[2]))
                elif bt == "ensures":
                    new_body.append(("ensures", _rewrite_expr(bit[1], {alias}, state, types, consts), bit[2]))
                elif bt == "let":
                    new_body.append(("let", bit[1], _rewrite_expr(bit[2], {alias}, state, types, consts), bit[3]))
                else:
                    new_body.append(_rewrite_stmt(bit, {alias}, state, types, consts))
            out.append((
                "action", pn, _rewrite_params(params, {alias}, types, consts), new_body, loc, fair, meta,
            ))
        elif tag == "invariant":
            pn = _prefix(it[1], alias)
            display_names[pn] = _display_logical(alias, it[1])
            out.append(("invariant", pn, _rewrite_expr(it[2], {alias}, state, types, consts), it[3], it[4] if len(it) > 4 else None))
        elif tag == "reachable":
            pn = _prefix(it[1], alias)
            display_names[pn] = _display_logical(alias, it[1])
            out.append(("reachable", pn, _rewrite_expr(it[2], {alias}, state, types, consts), it[3], it[4] if len(it) > 4 else None))
        elif tag == "leadsto":
            binders = [_rewrite_binder(b, {alias}, types, consts) for b in it[2]]
            pn = _prefix(it[1], alias)
            display_names[pn] = _display_logical(alias, it[1])
            measure = (
                _rewrite_expr(it[7], {alias}, state, types, consts)
                if len(it) > 7 and it[7] is not None else None
            )
            out.append(("leadsto", pn, binders,
                        _rewrite_expr(it[3], {alias}, state, types, consts),
                        _rewrite_expr(it[4], {alias}, state, types, consts),
                        it[5], it[6] if len(it) > 6 else None, measure))
    return out


def _display_logical(alias, name):
    return f"{alias}.{name}"


def _action_lookup(actions):
    return {a[1]: a for a in actions if a[0] == "action"}


def _expand_sync_action(sync, action_by_name, aliases, loc, warnings):
    _, name, params, sync_refs, body_items, _, fair, sync_meta = sync
    if len({r[1] for r in sync_refs}) != len(sync_refs):
        _compose_err("sync action cannot reference two actions from the same component", loc=loc)
    merged = []
    fair_constituents = []
    for ref in sync_refs:
        _, alias, act_name, arg_exprs = ref
        if alias not in aliases:
            _compose_err(f"unknown alias '{alias}'", loc=loc)
        key = _prefix(act_name, alias)
        comp = action_by_name.get(key)
        if comp is None:
            _compose_err(f"unknown action '{alias}.{act_name}'", loc=loc)
        _, _, cparams, cbody, _, _fair, _meta = comp
        if _fair:
            fair_constituents.append(f"{alias}.{act_name}")
        if len(cparams) != len(arg_exprs):
            _compose_err(f"sync arity mismatch for '{alias}.{act_name}'", loc=loc)
        # Compose checks arity here; argument type mismatches are caught by
        # build_spec's type checker after sync expansion.
        param_map = {}
        for i, p in enumerate(cparams):
            pname = p[1]
            param_map[pname] = _rewrite_expr(arg_exprs[i], aliases, set(), set(), set())
        for bit in cbody:
            bt = bit[0]
            if bt == "requires":
                merged.append(("requires", _subst_expr(bit[1], param_map), bit[2]))
            elif bt == "ensures":
                merged.append(("ensures", _subst_expr(bit[1], param_map), bit[2]))
            elif bt == "let":
                merged.append(("let", bit[1], _subst_expr(bit[2], param_map), bit[3]))
            else:
                merged.append(_subst_stmt(bit, param_map))
    empty_comp = set()
    for bit in body_items:
        bt = bit[0]
        if bt == "requires":
            merged.append((
                "requires",
                _rewrite_expr(bit[1], aliases, empty_comp, set(), set()),
                bit[2],
            ))
        elif bt == "ensures":
            merged.append((
                "ensures",
                _rewrite_expr(bit[1], aliases, empty_comp, set(), set()),
                bit[2],
            ))
        elif bt == "let":
            merged.append((
                "let",
                bit[1],
                _rewrite_expr(bit[2], aliases, empty_comp, set(), set()),
                bit[3],
            ))
        else:
            merged.append(_rewrite_stmt(bit, aliases, empty_comp, set(), set()))
    if not fair and fair_constituents:
        refs = ", ".join(fair_constituents)
        warnings.append({
            "kind": "fair_not_inherited",
            "message": (
                f"synchronized action '{name}' is not fair; fair constituent "
                f"action(s) {refs} will not contribute fairness unless the "
                "composite action is declared fair"
            ),
            "loc": loc,
        })
    # 8th element marks this as a sync action (inherits clauses from multiple
    # components) — used to scope per-action diagnostics like always_true_requires.
    return ("action", name, _rewrite_params(params, aliases), merged, loc, fair, sync_meta, True)


def _resolve_components(uses, base_dir, display_names):
    aliases = {}
    for use in uses:
        _, spec_name, alias, path, loc = use
        if alias in aliases:
            _compose_err(f"duplicate alias '{alias}'", loc=loc)
        aliases[alias] = (spec_name, path, loc)

    merged = []
    all_actions = []

    for use in uses:
        spec_name, alias, path, loc = use[1], use[2], use[3], use[4]
        fpath = Path(base_dir) / path
        if not fpath.is_file():
            _compose_err(f"file not found: {path}", kind="io", loc=loc)
        comp_ast = _parse_file(fpath)
        if comp_ast[0] == "compose":
            _compose_err("nested compose is not supported", loc=loc)
        if comp_ast[0] != "spec" or comp_ast[1] != spec_name:
            _compose_err(f"spec name mismatch: expected '{spec_name}', got '{comp_ast[1]}'", loc=loc)
        prefixed = _prefix_component_items(comp_ast[2], alias, display_names)
        merged.extend(prefixed)
        all_actions.extend([a for a in prefixed if a[0] == "action"])

    return aliases, merged, all_actions


def _merge_internal_actions(internals, aliases, all_actions):
    action_by_name = _action_lookup(all_actions)
    internal_phys = set()
    for internal in internals:
        _, alias, act_name, loc = internal
        if alias not in aliases:
            _compose_err(f"unknown alias '{alias}'", loc=loc)
        key = _prefix(act_name, alias)
        if key not in action_by_name:
            _compose_err(f"unknown action '{alias}.{act_name}'", loc=loc)
        internal_phys.add(key)
    return action_by_name, internal_phys


def _rewrite_compose_items(compose_rest, action_by_name, compose_aliases, merged, all_actions, warnings):
    empty_comp = set()

    for it in compose_rest:
        tag = it[0]
        if tag == "sync_action":
            expanded = _expand_sync_action(it, action_by_name, compose_aliases, it[5], warnings)
            merged.append(expanded)
            all_actions.append(expanded)
        elif tag == "action":
            aname, params, body, loc = it[1], it[2], it[3], it[4]
            fair = it[5] if len(it) > 5 else False
            meta = it[6] if len(it) > 6 else None
            new_body = []
            for bit in body:
                bt = bit[0]
                if bt == "requires":
                    new_body.append(("requires", _rewrite_expr(bit[1], compose_aliases, empty_comp, set(), set()), bit[2]))
                elif bt == "ensures":
                    new_body.append(("ensures", _rewrite_expr(bit[1], compose_aliases, empty_comp, set(), set()), bit[2]))
                elif bt == "let":
                    new_body.append(("let", bit[1], _rewrite_expr(bit[2], compose_aliases, empty_comp, set(), set()), bit[3]))
                else:
                    new_body.append(_rewrite_stmt(bit, compose_aliases, empty_comp, set(), set()))
            merged.append(("action", aname, _rewrite_params(params, compose_aliases), new_body, loc, fair, meta))
        elif tag == "state":
            decls = []
            for _, n, ty_ast in it[1]:
                decls.append(("decl", n, _rewrite_type(ty_ast, compose_aliases)))
            merged.append(("state", decls))
        elif tag == "init":
            merged.append(("init", [_rewrite_stmt(s, compose_aliases, empty_comp, set(), set()) for s in it[1]]))
        elif tag == "invariant":
            merged.append(("invariant", it[1], _rewrite_expr(it[2], compose_aliases, empty_comp, set(), set()), it[3], it[4] if len(it) > 4 else None))
        elif tag == "reachable":
            merged.append(("reachable", it[1], _rewrite_expr(it[2], compose_aliases, empty_comp, set(), set()), it[3], it[4] if len(it) > 4 else None))
        elif tag == "leadsto":
            binders = [_rewrite_binder(b, compose_aliases) for b in it[2]]
            measure = (
                _rewrite_expr(it[7], compose_aliases, empty_comp, set(), set())
                if len(it) > 7 and it[7] is not None else None
            )
            merged.append(("leadsto", it[1], binders,
                           _rewrite_expr(it[3], compose_aliases, empty_comp, set(), set()),
                           _rewrite_expr(it[4], compose_aliases, empty_comp, set(), set()),
                           it[5], it[6] if len(it) > 6 else None, measure))


def _finalize_compose_merged(merged, internal_phys):
    final_actions = [a for a in merged if a[0] == "action" and a[1] not in internal_phys]
    non_actions = [a for a in merged if a[0] not in ("action", "init")]
    init_stmts = []
    for a in merged:
        if a[0] == "init":
            init_stmts.extend(a[1])
    merged = non_actions
    if init_stmts:
        merged.append(("init", init_stmts))
    merged.extend(final_actions)
    return merged


def expand_compose(ast, base_dir):
    """Expand compose AST to a single spec AST. Returns (spec_ast, display_names)."""
    _, compose_name, items = ast
    uses, internals, compose_rest = [], [], []
    for it in items:
        if it[0] == "use":
            uses.append(it)
        elif it[0] == "internal":
            internals.append(it)
        else:
            compose_rest.append(it)

    display_names = {}
    aliases, merged, all_actions = _resolve_components(uses, base_dir, display_names)
    action_by_name, internal_phys = _merge_internal_actions(internals, aliases, all_actions)
    compose_aliases = set(aliases.keys())
    warnings = []
    _rewrite_compose_items(compose_rest, action_by_name, compose_aliases, merged, all_actions, warnings)
    merged = _finalize_compose_merged(merged, internal_phys)
    if warnings:
        merged.append(("__warnings", warnings))

    return ("spec", compose_name, merged), display_names
