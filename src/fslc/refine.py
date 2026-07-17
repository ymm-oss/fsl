# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Refinement checking: impl spec refines abs spec via a mapping file."""
from __future__ import annotations

import itertools

import z3

from .diagnostics import with_faithfulness
from .bmc import (
    _bmc_explore,
    _action_enabled_exprs,
    _build_trace,
    _deadlock_from_enabled,
    _display_param,
    _display_leadsto_bindings,
    _eval_requires,
    _expr_static_type,
    _fairness_ok,
    _last_action,
    _leadsto_binding_types,
    _logical_eq,
    _logical_eq_var,
    binder_range,
    build_instances,
    compute_updates,
    domain_range,
    eval_expr,
    init_constraints,
    logical_state_values,
    make_state,
    transition,
    z3_sort,
    _eval_cache_scope,
    _map_domain,
    expand_leadsto_bindings,
    _z3_domain_value,
)
from .model import (
    FslError,
    annotate_display_name,
    bounds_invariant_expr,
    domain_range as model_domain_range,
    resolve_type,
)


_REFINE_HINT = (
    "the impl step does not correspond to the mapped abs action; "
    "fix the map expressions, the action correspondence, or guard the impl action"
)

_ENSURES_NOTE = (
    "abs ensures are not checked during refinement; verify/prove the abstract spec separately"
)

_PROGRESS_HINT = (
    "the impl refines the abstract safety contract, but admits an execution where "
    "the pulled-back abstract leadsTo remains pending. Fairness must come from "
    "lower-layer `fair action` declarations for the implementation actions named "
    "by preserve progress; action mappings do not create fairness or prove "
    "implementation conformance by themselves"
)


def _err(message, kind="semantics", loc=None, expected=None, hint=None):
    raise FslError(message, kind=kind, loc=loc, expected=expected, hint=hint)


def _types_compatible(abs_ty, impl_ty):
    """Return True if impl expression type can map to abs variable type."""
    if abs_ty == impl_ty:
        return True
    if abs_ty[0] == impl_ty[0]:
        if abs_ty[0] in ("int", "bool", "domain", "enum"):
            return True
        if abs_ty[0] == "option":
            return _types_compatible(abs_ty[1], impl_ty[1])
        if abs_ty[0] == "struct":
            if abs_ty[1] != impl_ty[1]:
                return False
            abs_fields = impl_ty  # placeholder
            return abs_ty[1] == impl_ty[1]
        if abs_ty[0] == "map":
            return _types_compatible(abs_ty[1], impl_ty[1]) and _types_compatible(abs_ty[2], impl_ty[2])
        if abs_ty[0] == "set":
            return _types_compatible(abs_ty[1], impl_ty[1])
        if abs_ty[0] == "seq":
            return (
                _types_compatible(abs_ty[1], impl_ty[1])
                and abs_ty[2] == impl_ty[2]
            )
        if abs_ty[0] == "relation":
            return (
                _types_compatible(abs_ty[1], impl_ty[1])
                and _types_compatible(abs_ty[2], impl_ty[2])
            )
    if abs_ty[0] == "domain" and impl_ty[0] == "int":
        return True
    if abs_ty[0] == "enum" and impl_ty[0] == "enum" and abs_ty[1] == impl_ty[1]:
        return True
    return False


def _type_defs_conflict(impl_info, abs_info):
    """True if two same-named type declarations are unsafe to merge.

    Domain types with different bounds are deliberately allowed to share a
    name: an impl value outside the abs range is still caught downstream as
    an `abs_state_mismatch` when the mapped value is checked against the abs
    bounds. Enums (and structs) have no such downstream bounds check — an
    impl-only member's ordinal position gets silently reinterpreted as
    whichever abs member sits at that index, so a same name with a
    different member list (or field set) must be rejected here instead,
    otherwise a real refinement violation can come back as a false
    "refines".
    """
    if impl_info["kind"] != abs_info["kind"]:
        return True
    if impl_info["kind"] == "enum":
        return impl_info["members"] != abs_info["members"]
    if impl_info["kind"] == "struct":
        return impl_info["fields"] != abs_info["fields"]
    return False


def _merge_types_meta(impl_spec, abs_spec):
    """Merge type metadata; abs types take precedence on name clash."""
    merged = dict(impl_spec["types"])
    for name, info in abs_spec["types"].items():
        impl_info = impl_spec["types"].get(name)
        if impl_info is not None and _type_defs_conflict(impl_info, info):
            _err(
                f"type '{name}' is declared differently in the impl and abs specs "
                f"(impl: {impl_info}, abs: {info})",
                kind="type",
                hint=(
                    f"refinement merges type metadata by name, so a same-named type "
                    f"with a different definition cannot be resolved safely; give the "
                    f"impl and abs layers distinct type names (e.g. enum ImplStatus vs "
                    f"enum AbsStatus) instead of reusing '{name}' for two different types"
                ),
            )
        merged[name] = info
    return merged


def _eval_map_expr(expr, impl_state, impl_spec, abs_spec, extra_binds=None):
    """Evaluate a mapping expression in impl context."""
    merged_types = _merge_types_meta(impl_spec, abs_spec)
    spec = {**impl_spec, "types": merged_types}
    binds = dict(extra_binds or {})
    with _eval_cache_scope({}, None):
        return eval_expr(expr, impl_state, binds, spec)


def _subst_binder(expr, binder_var, key_val):
    def walk(e):
        tag = e[0]
        if tag == "var" and e[1] == binder_var:
            if isinstance(key_val, bool):
                return ("bool", key_val)
            return ("num", key_val)
        if tag == "num" or tag == "bool" or tag == "none":
            return e
        if tag == "index":
            base = e[1] if isinstance(e[1], str) else walk(e[1])
            return ("index", base, walk(e[2]))
        if tag == "field":
            return ("field", walk(e[1]), e[2])
        if tag == "struct_lit":
            return ("struct_lit", e[1], {k: walk(v) for k, v in e[2].items()})
        if tag == "bin":
            return ("bin", e[1], walk(e[2]), walk(e[3]))
        if tag == "ite":
            return ("ite", walk(e[1]), walk(e[2]), walk(e[3]))
        if tag == "neg":
            return ("neg", walk(e[1]))
        if tag == "not":
            return ("not", walk(e[1]))
        if tag == "some":
            return ("some", walk(e[1]))
        if tag in ("forall", "exists"):
            b = e[1]
            if b[0] == "binder_typed" and b[1] == binder_var:
                return e
            return (tag, b, walk(e[2]))
        if tag == "method":
            return ("method", walk(e[1]), e[2], [walk(a) for a in e[3]])
        if tag in ("rel_reachable",):
            return (tag, walk(e[1]), walk(e[2]), walk(e[3]))
        if tag in ("rel_acyclic", "rel_functional", "rel_injective", "rel_domain", "rel_range"):
            return (tag, walk(e[1]))
        return e

    return walk(expr)


def _default_array_value(ty, types_meta):
    if ty[0] in ("int", "domain", "enum"):
        lo = ty[1] if ty[0] == "domain" else 0
        return z3.IntVal(lo if ty[0] == "domain" else 0)
    if ty[0] == "bool":
        return z3.BoolVal(False)
    _err(f"cannot build default array value for type {ty}", kind="internal")


def _build_map_array(elem_expr_template, binder, key_ty, value_ty, impl_state, impl_spec, abs_spec):
    """Build K(ArraySort)+Store chain for per-key map abstraction."""
    merged_types = _merge_types_meta(impl_spec, abs_spec)
    types_meta = merged_types
    elem_sort = z3_sort(value_ty, types_meta)
    arr = z3.K(z3_sort(key_ty, types_meta), _default_array_value(value_ty, types_meta))
    for k in _map_domain(key_ty, {"types": types_meta, "consts": {}}):
        expr_k = _subst_binder(elem_expr_template, binder[1], k)
        val = _eval_map_expr(expr_k, impl_state, impl_spec, abs_spec, {binder[1]: k})
        if isinstance(val, tuple) and val[0] == "option_val":
            _err("per-key map mapping must produce scalar values, not Option", kind="type")
        arr = z3.Store(arr, _z3_domain_value(key_ty, k), val)
    return arr


def _expand_alpha_scalar(logical, ty, z3_val, out, types_meta):
    """Place a scalar/option/struct/seq/set value into physical alpha dict."""
    if ty[0] in ("int", "bool", "domain", "enum"):
        out[logical] = z3_val
        return
    if ty[0] == "option":
        if isinstance(z3_val, tuple) and z3_val[0] == "option_val":
            out[f"{logical}__present"] = z3_val[1]
            out[f"{logical}__value"] = z3_val[2]
        elif z3_val == ("none",):
            out[f"{logical}__present"] = z3.BoolVal(False)
            out[f"{logical}__value"] = _default_array_value(ty[1], types_meta)
        else:
            _err(f"map for Option '{logical}' must produce none or some(...)", kind="type")
        return
    if ty[0] == "struct":
        if not isinstance(z3_val, tuple) or z3_val[0] != "struct_val":
            _err(f"map for struct '{logical}' must produce a struct literal", kind="type")
        _, sname, fields = z3_val
        for fn, fv in fields.items():
            fty = types_meta[sname]["fields"][fn]
            if fty[0] == "option" and isinstance(fv, tuple) and fv[0] == "option_val":
                out[f"{logical}__{fn}__present"] = fv[1]
                out[f"{logical}__{fn}__value"] = fv[2]
            elif fty[0] == "option" and fv == ("none",):
                out[f"{logical}__{fn}__present"] = z3.BoolVal(False)
                out[f"{logical}__{fn}__value"] = _default_array_value(fty[1], types_meta)
            elif fty[0] == "option":
                _err(f"map for Option field '{logical}.{fn}' must produce none or some(...)", kind="type")
            else:
                out[f"{logical}__{fn}"] = fv
        return
    if ty[0] == "set":
        if isinstance(z3_val, tuple) and z3_val[0] == "set_val":
            out[logical] = z3_val[1]
        else:
            out[logical] = z3_val
        return
    if ty[0] == "seq":
        if isinstance(z3_val, tuple) and z3_val[0] == "seq_val":
            out[f"{logical}__data"] = z3_val[1]
            out[f"{logical}__len"] = z3_val[2]
        else:
            _err(f"map for Seq '{logical}' must produce a Seq expression", kind="type")
        return
    if ty[0] == "relation":
        if isinstance(z3_val, tuple) and z3_val[0] == "relation_val":
            out[logical] = z3_val[1]
        else:
            out[logical] = z3_val
        return
    _err(f"unsupported abstraction type {ty} for '{logical}'", kind="type")


def _expand_alpha_map_option(logical, kty, vty, impl_state, elem_expr, binder,
                             impl_spec, abs_spec, out):
    merged_types = _merge_types_meta(impl_spec, abs_spec)
    pres_arr = z3.K(z3_sort(kty, merged_types), z3.BoolVal(False))
    val_arr = z3.K(z3_sort(kty, merged_types), _default_array_value(vty[1], merged_types))
    for k in _map_domain(kty, {"types": merged_types, "consts": {}}):
        expr_k = _subst_binder(elem_expr, binder[1], k)
        val = _eval_map_expr(expr_k, impl_state, impl_spec, abs_spec, {binder[1]: k})
        zkey = _z3_domain_value(kty, k)
        if isinstance(val, tuple) and val[0] == "option_val":
            pres_arr = z3.Store(pres_arr, zkey, val[1])
            val_arr = z3.Store(val_arr, zkey, val[2])
        elif val == ("none",):
            pres_arr = z3.Store(pres_arr, zkey, z3.BoolVal(False))
        else:
            _err(f"map for Option map '{logical}' must produce none or some(...)", kind="type")
    out[f"{logical}__present"] = pres_arr
    out[f"{logical}__value"] = val_arr


def build_alpha(impl_state, mapping, impl_spec, abs_spec):
    """Build physical-level abs state as Z3 expressions over impl_state."""
    alpha = {}
    merged_types = _merge_types_meta(impl_spec, abs_spec)

    for logical, ty in abs_spec["state"].items():
        if logical not in mapping["maps"]:
            _err(f"no map for abstract state variable '{logical}'", kind="type")
        m = mapping["maps"][logical]
        if m["kind"] == "scalar":
            val = _eval_map_expr(m["expr"], impl_state, impl_spec, abs_spec)
            if ty[0] == "map":
                if not isinstance(val, tuple) or val[0] != "set_val":
                    if ty[2][0] == "option":
                        _err(
                            f"map '{logical}' needs per-key mapping for Map with Option values",
                            kind="type",
                        )
                    alpha[logical] = val
                else:
                    alpha[logical] = val[1]
            else:
                _expand_alpha_scalar(logical, ty, val, alpha, merged_types)
        elif m["kind"] == "indexed":
            binder = m["binder"]
            kty = ty[1] if ty[0] == "map" else None
            if ty[0] == "map":
                vty = ty[2]
                if vty[0] == "option":
                    _expand_alpha_map_option(
                        logical, ty[1], vty, impl_state, m["expr"], binder,
                        impl_spec, abs_spec, alpha,
                    )
                elif vty[0] == "struct":
                    sname = vty[1]
                    key_values = _map_domain(ty[1], {"types": merged_types, "consts": {}})
                    for fn, fty in merged_types[sname]["fields"].items():
                        if fty[0] == "option":
                            pres_arr = z3.K(z3_sort(ty[1], merged_types), z3.BoolVal(False))
                            val_arr = z3.K(
                                z3_sort(ty[1], merged_types),
                                _default_array_value(fty[1], merged_types),
                            )
                            for k in key_values:
                                expr_k = _subst_binder(m["expr"], binder[1], k)
                                sval = _eval_map_expr(
                                    expr_k, impl_state, impl_spec, abs_spec, {binder[1]: k})
                                if not isinstance(sval, tuple) or sval[0] != "struct_val":
                                    _err(f"map for struct map '{logical}' must produce struct values", kind="type")
                                fv = sval[2][fn]
                                zkey = _z3_domain_value(ty[1], k)
                                if isinstance(fv, tuple) and fv[0] == "option_val":
                                    pres_arr = z3.Store(pres_arr, zkey, fv[1])
                                    val_arr = z3.Store(val_arr, zkey, fv[2])
                                elif fv == ("none",):
                                    pres_arr = z3.Store(pres_arr, zkey, z3.BoolVal(False))
                                else:
                                    _err(
                                        f"map for Option field '{logical}[].{fn}' must produce none or some(...)",
                                        kind="type",
                                    )
                            alpha[f"{logical}__{fn}__present"] = pres_arr
                            alpha[f"{logical}__{fn}__value"] = val_arr
                        else:
                            arr = z3.K(
                                z3_sort(ty[1], merged_types),
                                _default_array_value(fty, merged_types),
                            )
                            for k in key_values:
                                expr_k = _subst_binder(m["expr"], binder[1], k)
                                sval = _eval_map_expr(
                                    expr_k, impl_state, impl_spec, abs_spec, {binder[1]: k})
                                if not isinstance(sval, tuple) or sval[0] != "struct_val":
                                    _err(f"map for struct map '{logical}' must produce struct values", kind="type")
                                arr = z3.Store(arr, _z3_domain_value(ty[1], k), sval[2][fn])
                            alpha[f"{logical}__{fn}"] = arr
                else:
                    alpha[logical] = _build_map_array(
                        m["expr"], binder, ty[1], vty, impl_state, impl_spec, abs_spec,
                    )
            elif ty[0] == "seq":
                val = _eval_map_expr(m["expr"], impl_state, impl_spec, abs_spec)
                _expand_alpha_scalar(logical, ty, val, alpha, merged_types)
            else:
                _err(f"indexed map on non-Map/Seq variable '{logical}'", kind="type")
        else:
            _err(f"internal: unknown map kind for '{logical}'", kind="internal")

    return alpha


def _logical_eq_alpha(abs_spec, a1, a2):
    parts = [_logical_eq_var(abs_spec, a1, a2, n, ty) for n, ty in abs_spec["state"].items()]
    return z3.And(*parts) if parts else z3.BoolVal(True)


def _alpha_logical_values(model, alpha, abs_spec):
    """Evaluate alpha expressions in a model to logical JSON state."""
    concrete = {}
    for phys, expr in alpha.items():
        concrete[phys] = model.eval(expr, model_completion=True)
    return logical_state_values(model, concrete, abs_spec)


def _mismatch_paths(model, abs_spec, expected, actual):
    paths = []
    for n, ty in abs_spec["state"].items():
        _collect_mismatch(model, abs_spec, n, ty, expected.get(n), actual.get(n), n, paths)
    return paths


def _collect_mismatch(model, abs_spec, name, ty, exp, act, path, paths):
    if exp == act:
        return
    if ty[0] == "map" and isinstance(exp, dict) and isinstance(act, dict):
        for k in set(exp) | set(act):
            ev, av = exp.get(k), act.get(k)
            sub_ty = ty[2]
            if ev == av:
                continue
            if sub_ty[0] == "struct" and isinstance(ev, dict) and isinstance(av, dict):
                for fn in set(ev) | set(av):
                    if ev.get(fn) != av.get(fn):
                        paths.append(f"{path}[{k}].{fn}")
            else:
                paths.append(f"{path}[{k}]")
        return
    if exp != act:
        paths.append(path)


def _bounds_violation_expr(abs_spec, alpha):
    cons = []
    for inv in abs_spec["invariants"]:
        if not inv.get("implicit"):
            continue
        with _eval_cache_scope({}, None):
            cons.append(eval_expr(inv["expr"], alpha, {}, abs_spec))
    if not cons:
        return z3.BoolVal(False)
    return z3.Not(z3.And(*cons))


def _abs_init_constraints(abs_spec, alpha):
    from .model import phys_z3_sort
    sym = {
        p["phys"]: z3.Const(f"__abs_init_{p['phys']}", phys_z3_sort(p, abs_spec["types"]))
        for p in abs_spec["phys_vars"]
    }
    expected = init_constraints(abs_spec, sym)
    subst = [(sym[p], alpha[p]) for p in sym if p in alpha]
    out = []
    for c in expected:
        out.append(z3.substitute(c, subst))
    return out


def _find_abs_action(abs_spec, name):
    for act in abs_spec["actions"]:
        if act["name"] == name:
            return act
    return None


def _param_type(p, types_meta):
    tyname = p[3] if len(p) > 3 else None
    if tyname and tyname in types_meta:
        return types_meta[tyname]["ty"]
    if tyname == "Int":
        return ("int",)
    if tyname == "Bool":
        return ("bool",)
    if len(p) >= 3 and isinstance(p[1], int) and isinstance(p[2], int):
        return ("domain", p[1], p[2])
    return None


def _annotation_matches_param(annotation, impl_param, types_meta, consts):
    impl_tyname = impl_param[3] if len(impl_param) > 3 else None
    if annotation[0] == "name" and impl_tyname:
        return annotation[1] == impl_tyname
    annotated = resolve_type(annotation, types_meta, consts)
    expected = _param_type(impl_param, types_meta)
    return expected is None or _types_compatible(expected, annotated)


def _abs_action_instance(act, param_exprs, impl_state, impl_binds, impl_spec, abs_spec):
    binds = dict(impl_binds)
    for i, p in enumerate(act["params"]):
        pname = p[0]
        if i < len(param_exprs):
            e = param_exprs[i]
            if e[0] == "var" and e[1] in binds:
                val = binds[e[1]]
            else:
                val = _eval_map_expr(e, impl_state, impl_spec, abs_spec, binds)
        else:
            val = binds.get(pname, z3.IntVal(0))
        binds[pname] = val
    return binds


def _type_name_for(ty, types_meta):
    for name, info in types_meta.items():
        if info["ty"] == ty:
            return name
    return None


def _auto_map_entry(logical, abs_ty, impl_ty, merged_types):
    if not _types_compatible(abs_ty, impl_ty):
        _err(
            f"maps auto cannot synthesize map for '{logical}': "
            f"incompatible state types {impl_ty} -> {abs_ty}",
            kind="type",
        )
    if abs_ty[0] == "map" and abs_ty[2][0] in ("option", "struct"):
        key_name = _type_name_for(abs_ty[1], merged_types)
        if key_name is None:
            _err(
                f"maps auto cannot synthesize map for '{logical}': "
                "Map keys need a named bounded type for per-key identity mapping",
                kind="type",
            )
        binder = ("binder_typed", "_k", key_name, None)
        return {
            "kind": "indexed",
            "binder": binder,
            "expr": ("index", ("var", logical), ("var", "_k")),
            "loc": None,
        }
    return {"kind": "scalar", "expr": ("var", logical), "loc": None}


def _action_params_compatible(impl_act, abs_act, merged_types):
    if len(impl_act["params"]) != len(abs_act["params"]):
        return False
    for impl_param, abs_param in zip(impl_act["params"], abs_act["params"]):
        impl_ty = _param_type(impl_param, merged_types)
        abs_ty = _param_type(abs_param, merged_types)
        if impl_ty and abs_ty and not _types_compatible(abs_ty, impl_ty):
            return False
    return True


def _apply_auto_mappings(maps, actions, impl_spec, abs_spec):
    merged_types = _merge_types_meta(impl_spec, abs_spec)
    for logical, abs_ty in abs_spec["state"].items():
        if logical in maps or logical not in impl_spec["state"]:
            continue
        maps[logical] = _auto_map_entry(
            logical, abs_ty, impl_spec["state"][logical], merged_types)

    abs_actions = {act["name"]: act for act in abs_spec["actions"]}
    for impl_act in impl_spec["actions"]:
        aname = impl_act["name"]
        if aname in actions or aname not in abs_actions:
            continue
        abs_act = abs_actions[aname]
        if not _action_params_compatible(impl_act, abs_act, merged_types):
            _err(
                f"maps auto cannot synthesize action correspondence for '{aname}': "
                "parameter arity or types are incompatible",
                kind="type",
                loc=impl_act.get("loc"),
            )
        actions[aname] = {
            "kind": "map",
            "abs_action": aname,
            "arg_exprs": [("var", p[0]) for p in impl_act["params"]],
            "loc": None,
        }


def build_refinement(tree, impl_spec, abs_spec):
    """Validate and normalize a refinement mapping AST."""
    _, name, items = tree
    impl_name = None
    abs_name = None
    maps_auto = False
    maps = {}
    actions = {}
    progress_requested = False
    progress = []
    progress_loc = None

    for it in items:
        tag = it[0]
        if tag == "impl":
            impl_name = it[1]
        elif tag == "abs":
            abs_name = it[1]
        elif tag == "maps_auto":
            maps_auto = True
        elif tag == "map":
            _, logical, binder, expr, loc = it
            if logical in maps:
                _err(f"duplicate map for '{logical}'", kind="type", loc=loc)
            if logical not in abs_spec["state"]:
                _err(f"unknown abstract state variable '{logical}'", kind="type", loc=loc)
            if binder is None:
                maps[logical] = {"kind": "scalar", "expr": expr, "loc": loc}
            else:
                maps[logical] = {"kind": "indexed", "binder": binder, "expr": expr, "loc": loc}
        elif tag == "action_map":
            _, aname, params, target, loc = it
            if aname in actions:
                _err(f"duplicate action map for '{aname}'", kind="type", loc=loc)
            impl_act = _find_abs_action(impl_spec, aname)
            if impl_act is None:
                _err(f"unknown impl action '{aname}'", kind="type", loc=loc)
            param_names = [p[1] if isinstance(p, tuple) else p for p in params]
            impl_param_names = [p[0] for p in impl_act["params"]]
            if param_names != impl_param_names:
                _err(
                    f"action '{aname}' parameter names/order must match impl "
                    f"({impl_param_names})",
                    kind="type",
                    loc=loc,
                )
            merged_types = _merge_types_meta(impl_spec, abs_spec)
            for param, impl_param in zip(params, impl_act["params"]):
                if not isinstance(param, tuple):
                    continue
                _, pname, annotation = param
                if annotation is None:
                    continue
                if not _annotation_matches_param(
                        annotation, impl_param, merged_types, impl_spec.get("consts", {})):
                    _err(
                        f"action '{aname}' parameter '{pname}' type annotation "
                        f"mismatch: expected {impl_param[3]}, got {annotation}",
                        kind="type",
                        loc=loc,
                    )
            if target[0] == "stutter":
                actions[aname] = {"kind": "stutter", "loc": loc}
            else:
                _, abs_aname, arg_exprs = target
                abs_act = _find_abs_action(abs_spec, abs_aname)
                if abs_act is None:
                    _err(f"unknown abstract action '{abs_aname}'", kind="type", loc=loc)
                abs_params = [p[0] for p in abs_act["params"]]
                if len(arg_exprs) != len(abs_params):
                    _err(
                        f"action '{aname}' -> '{abs_aname}' expects {len(abs_params)} arguments",
                        kind="type",
                        loc=loc,
                    )
                actions[aname] = {
                    "kind": "map",
                    "abs_action": abs_aname,
                    "arg_exprs": arg_exprs,
                    "loc": loc,
                }
        elif tag == "preserve_progress":
            _, items, loc = it
            if progress_requested:
                _err("duplicate preserve progress block", kind="type", loc=loc)
            progress_requested = True
            progress_loc = loc
            for item in items:
                if item[0] != "progress_respond":
                    _err(f"unknown progress item '{item[0]}'", kind="type", loc=loc)
                _, leadsto_name, action_names, item_loc = item
                progress.append({
                    "kind": "respond",
                    "leadsto": leadsto_name,
                    "actions": action_names,
                    "loc": item_loc,
                })

    if impl_name is None:
        _err("refinement missing impl spec name", kind="type")
    if abs_name is None:
        _err("refinement missing abs spec name", kind="type")
    if impl_name != impl_spec["name"]:
        _err(
            f"impl name '{impl_name}' does not match impl spec '{impl_spec['name']}'",
            kind="type",
        )
    if abs_name != abs_spec["name"]:
        _err(
            f"abs name '{abs_name}' does not match abs spec '{abs_spec['name']}'",
            kind="type",
        )

    if maps_auto:
        _apply_auto_mappings(maps, actions, impl_spec, abs_spec)

    for logical in abs_spec["state"]:
        if logical not in maps:
            _err(f"missing map for abstract state variable '{logical}'", kind="type")

    for act in impl_spec["actions"]:
        if act["name"] not in actions:
            _err(f"missing action correspondence for impl action '{act['name']}'", kind="type")

    if progress_requested:
        abs_leadstos = {lt["name"]: lt for lt in abs_spec.get("leadstos", [])}
        if not progress:
            progress = [
                {
                    "kind": "respond",
                    "leadsto": lt["name"],
                    "actions": [],
                    "loc": progress_loc,
                }
                for lt in abs_spec.get("leadstos", [])
            ]
        if not progress:
            _err("preserve progress requested, but abs spec has no leadsTo declarations",
                 kind="type", loc=progress_loc)
        seen_progress = set()
        impl_actions = {act["name"]: act for act in impl_spec["actions"]}
        for decl in progress:
            lt_name = decl["leadsto"]
            if lt_name in seen_progress:
                _err(f"duplicate progress declaration for leadsTo '{lt_name}'",
                     kind="type", loc=decl.get("loc"))
            seen_progress.add(lt_name)
            if lt_name not in abs_leadstos:
                _err(f"unknown abstract leadsTo '{lt_name}'",
                     kind="type", loc=decl.get("loc"))
            for aname in decl["actions"]:
                if aname not in impl_actions:
                    _err(f"unknown impl progress action '{aname}'",
                         kind="type", loc=decl.get("loc"))

    merged_types = _merge_types_meta(impl_spec, abs_spec)
    merged_impl = {**impl_spec, "types": merged_types}

    # static type check map expressions
    for logical, m in maps.items():
        abs_ty = abs_spec["state"][logical]
        env = {}
        if m["kind"] == "scalar":
            ty = _expr_static_type(m["expr"], merged_impl, env)
            if ty and not _types_compatible(abs_ty, ty):
                _err(
                    f"map for '{logical}' type mismatch: expected {abs_ty}, got {ty}",
                    kind="type",
                    loc=m.get("loc"),
                )
        elif m["kind"] == "indexed":
            binder = m["binder"]
            if binder[0] == "binder_typed":
                env = {binder[1]: merged_types[binder[2]]["ty"]}
            ty = _expr_static_type(m["expr"], merged_impl, env)
            if abs_ty[0] == "map":
                expected = abs_ty[2]
            elif abs_ty[0] == "seq":
                expected = abs_ty[1]
            else:
                expected = abs_ty
            if ty and not _types_compatible(expected, ty):
                _err(
                    f"map for '{logical}' element type mismatch: expected {expected}, got {ty}",
                    kind="type",
                    loc=m.get("loc"),
                )

    # static type check action-map argument expressions (DESIGN-refinement §3).
    # Defensive: only flag when both expected and inferred types are determinable.
    for aname, amap in actions.items():
        if amap.get("kind") != "map":
            continue
        impl_act = _find_abs_action(impl_spec, aname)
        abs_act = _find_abs_action(abs_spec, amap["abs_action"])
        if impl_act is None or abs_act is None:
            continue
        env = {}
        for p in impl_act["params"]:
            pt = _param_type(p, merged_types)
            if pt:
                env[p[0]] = pt
        for arg_expr, abs_p in zip(amap["arg_exprs"], abs_act["params"]):
            expected = _param_type(abs_p, merged_types)
            got = _expr_static_type(arg_expr, merged_impl, env)
            if expected and got and not _types_compatible(expected, got):
                _err(
                    f"action '{aname}' -> '{amap['abs_action']}' argument type "
                    f"mismatch: expected {expected}, got {got}",
                    kind="type",
                    loc=amap.get("loc"),
                )

    return {
        "name": name,
        "impl": impl_name,
        "abs": abs_name,
        "maps": maps,
        "actions": actions,
        "progress": progress,
    }


def _check_map_out_of_bounds(
        s, alpha_t, abs_spec, impl_spec, mapping, explored, step, at):
    s.push()
    s.add(_bounds_violation_expr(abs_spec, alpha_t))
    if s.check() != z3.sat:
        s.pop()
        return None
    m = s.model()
    s.pop()
    instances = explored["instances"]
    choices = explored["choices"]
    return _failure(
        impl_spec, abs_spec, mapping, explored, m, at=at,
        kind="map_out_of_bounds",
        step=step,
        impl_action=_last_action(m, choices, instances, step, impl_spec) if step > 0 else None,
        alpha_before=_alpha_logical_values(m, alpha_t, abs_spec),
        alpha_after_expected=None,
        alpha_after_actual=_alpha_logical_values(m, alpha_t, abs_spec),
        mismatch=_mismatch_paths(
            m, abs_spec, {}, _alpha_logical_values(m, alpha_t, abs_spec)),
    )


def _solver_for_prefix(ctx, upto):
    s = z3.Solver()
    s.set(unsat_core=True)
    s.add(*ctx.get("init_cons", []))
    for cons in ctx.get("step_cons", [])[:upto]:
        s.add(*cons)
    return s


def _eval_abs_on_alpha(expr, alpha, binds, abs_spec, expr_cache, cache_key):
    with _eval_cache_scope(expr_cache, cache_key):
        return eval_expr(expr, alpha, dict(binds), abs_spec)


def _progress_action_summary(decl):
    return {
        "leadsTo": decl["leadsto"],
        "actions": list(decl.get("actions") or []),
    }


def _progress_failure(
        impl_spec, abs_spec, ctx, model, leadsto, decl, binding_types,
        extra_binds, step, pending_since, loop_start=None, stutter=False):
    progress_failure = "deadlock_or_stall_blocks_progress" if stutter else "lasso_blocks_progress"
    out = {
        "result": "refinement_failed",
        "impl": impl_spec["name"],
        "abs": abs_spec["name"],
        "kind": "progress_lost",
        "progress_failure": progress_failure,
        "violation_kind": "leadsTo",
        "invariant": leadsto["name"],
        "bindings": _display_leadsto_bindings(model, extra_binds, abs_spec, binding_types),
        "pending_since": pending_since,
        "stutter": stutter,
        "impl_trace": _build_trace(
            model, ctx["states"], ctx["choices"], ctx["instances"], impl_spec, step),
        "progress": _progress_action_summary(decl),
        "hint": _PROGRESS_HINT,
    }
    if leadsto.get("loc"):
        out["loc"] = leadsto["loc"]
    if loop_start is not None:
        out["loop_start"] = loop_start
    return with_faithfulness(out)


def _check_progress_lasso(
        impl_spec, abs_spec, ctx, alpha_fn, leadsto, decl, binding_types, expr_cache):
    states = ctx["states"]
    choices = ctx["choices"]
    instances = ctx["instances"]
    K = len(states) - 1
    if K <= 0:
        return None

    solver = _solver_for_prefix(ctx, K)
    alpha_cache = {}

    def alpha_at(t):
        if t not in alpha_cache:
            alpha_cache[t] = alpha_fn(states[t])
        return alpha_cache[t]

    for extra_binds in expand_leadsto_bindings(leadsto, abs_spec):
        candidates = []
        meta = []
        bind_key = tuple(sorted(extra_binds.items()))
        for i in range(K):
            for j in range(i + 1, K + 1):
                loop = _logical_eq(impl_spec, states[i], states[j])
                for p in range(j):
                    not_q = [
                        z3.Not(_eval_abs_on_alpha(
                            leadsto["Q"], alpha_at(q), extra_binds, abs_spec,
                            expr_cache, ("progress_q", leadsto["name"], q, bind_key)))
                        for q in range(min(i, p), j)
                    ]
                    p_hold = _eval_abs_on_alpha(
                        leadsto["P"], alpha_at(p), extra_binds, abs_spec,
                        expr_cache, ("progress_p", leadsto["name"], p, bind_key))
                    fair_ok = _fairness_ok(
                        instances, states, choices, i, j, impl_spec, {}, expr_cache)
                    cond = z3.And(
                        loop,
                        p_hold,
                        z3.And(*not_q) if not_q else z3.BoolVal(True),
                        fair_ok,
                    )
                    candidates.append(cond)
                    meta.append((i, j, p, cond))

        if not candidates:
            continue

        solver.push()
        solver.add(z3.Or(*candidates))
        if solver.check() == z3.sat:
            model = solver.model()
            chosen = None
            for i, j, p, cond in meta:
                if z3.is_true(model.eval(cond, model_completion=True)):
                    chosen = (i, j, p)
                    break
            solver.pop()
            if chosen is None:
                return None
            i, j, p = chosen
            return _progress_failure(
                impl_spec, abs_spec, ctx, model, leadsto, decl, binding_types,
                extra_binds, step=j, pending_since=p, loop_start=i, stutter=False)
        solver.pop()
    return None


def _check_progress_deadlock(
        impl_spec, abs_spec, ctx, alpha_fn, leadsto, decl, binding_types, expr_cache):
    states = ctx["states"]
    instances = ctx["instances"]
    alpha_cache = {}

    def alpha_at(t):
        if t not in alpha_cache:
            alpha_cache[t] = alpha_fn(states[t])
        return alpha_cache[t]

    for extra_binds in expand_leadsto_bindings(leadsto, abs_spec):
        bind_key = tuple(sorted(extra_binds.items()))
        for t in range(len(states)):
            solver = _solver_for_prefix(ctx, t)
            enabled = _action_enabled_exprs(states[t], instances, impl_spec, expr_cache)
            candidates = []
            meta = []
            for p in range(t + 1):
                not_q = [
                    z3.Not(_eval_abs_on_alpha(
                        leadsto["Q"], alpha_at(q), extra_binds, abs_spec,
                        expr_cache, ("progress_dead_q", leadsto["name"], q, bind_key)))
                    for q in range(p, t + 1)
                ]
                p_hold = _eval_abs_on_alpha(
                    leadsto["P"], alpha_at(p), extra_binds, abs_spec,
                    expr_cache, ("progress_dead_p", leadsto["name"], p, bind_key))
                cond = z3.And(
                    _deadlock_from_enabled(enabled),
                    p_hold,
                    z3.And(*not_q) if not_q else z3.BoolVal(True),
                )
                candidates.append(cond)
                meta.append((p, cond))
            if not candidates:
                continue
            solver.push()
            solver.add(z3.Or(*candidates))
            if solver.check() == z3.sat:
                model = solver.model()
                pending = None
                for p, cond in meta:
                    if z3.is_true(model.eval(cond, model_completion=True)):
                        pending = p
                        break
                solver.pop()
                if pending is None:
                    return None
                return _progress_failure(
                    impl_spec, abs_spec, ctx, model, leadsto, decl, binding_types,
                    extra_binds, step=t, pending_since=pending, stutter=True)
            solver.pop()
    return None


def _check_progress_preservation(impl_spec, abs_spec, mapping, ctx, alpha_fn, depth):
    progress = mapping.get("progress") or []
    if not progress:
        return None, None

    leadstos = {lt["name"]: lt for lt in abs_spec.get("leadstos", [])}
    checked = {}
    expr_cache = {}
    for decl in progress:
        leadsto = leadstos[decl["leadsto"]]
        binding_types = _leadsto_binding_types(leadsto, abs_spec)
        failure = _check_progress_deadlock(
            impl_spec, abs_spec, ctx, alpha_fn, leadsto, decl, binding_types, expr_cache)
        if failure is not None:
            return failure, None
        failure = _check_progress_lasso(
            impl_spec, abs_spec, ctx, alpha_fn, leadsto, decl, binding_types, expr_cache)
        if failure is not None:
            return failure, None
        checked[leadsto["name"]] = {
            "checked_to_depth": min(depth, len(ctx["states"]) - 1),
            "actions": list(decl.get("actions") or []),
        }
    return None, checked


def refine(impl_spec, abs_spec, mapping, depth, alpha_fn=None):
    """Check that impl refines abs under the mapping to bounded depth.

    `alpha_fn(impl_state) -> abs physical-state dict` defaults to the single-layer
    mapping. `refine_chain` passes a *composed* alpha to check an end-to-end chain
    (bounded refinement is transitive at equal depth, so composing the per-layer
    maps and checking directly is sound — see DESIGN-refinement §7)."""
    if alpha_fn is None:
        def alpha_fn(st):
            return build_alpha(st, mapping, impl_spec, abs_spec)
    explored = _bmc_explore(impl_spec, depth, deadlock_mode="ignore")
    if explored["result"] != "explored":
        # The impl spec is not internally consistent within the bound (e.g. it
        # violates its own invariant). That is a property of the refinement
        # *input*, not a refinement (refines/refinement_failed) verdict — make
        # that explicit so it isn't mistaken for a fidelity failure (LANGUAGE §10).
        explored = dict(explored)
        explored["note"] = (
            "this result is a property of the impl spec itself (a refinement "
            "input), not a refinement failure; verify the impl spec independently "
            "before checking refinement"
        )
        return with_faithfulness(explored)

    instances = explored["instances"]

    # Build our own incremental unrolling instead of reusing the verify-side
    # solver: the impl may deadlock before `depth`, which makes a full
    # depth-length unrolling unsatisfiable and would hide every violation
    # (refine would vacuously report "refines"). Check each reachable prefix
    # length and stop once the prefix becomes unsatisfiable.
    expr_cache = {}
    states = [make_state(impl_spec, 0)]
    choices = []
    with _eval_cache_scope(expr_cache, id(states[0])):
        init_cons_impl = list(init_constraints(impl_spec, states[0]))
    s = z3.Solver()
    s.set(unsat_core=True)
    s.add(*init_cons_impl)

    # Only expand reachable prefixes. If impl deadlocks before depth, a full
    # expansion at "exactly depth" becomes unsatisfiable, so every violation
    # check turns into unsat (a miss) and we vacuously return refines.
    # step_cons[k] = transition constraint states[k] -> states[k+1] (saved for
    # prefix checking).
    step_cons = []
    for step in range(depth):
        if s.check() != z3.sat:
            break
        nxt = make_state(impl_spec, step + 1)
        ch = z3.Int(f"__refine_choice@{step}")
        cons = [ch >= 0, ch < len(instances)]
        with _eval_cache_scope(expr_cache, id(states[step])):
            cons.append(transition(impl_spec, instances, states[step], nxt, ch, expr_cache))
        s.push()
        s.add(*cons)
        reachable = s.check() == z3.sat
        s.pop()
        if not reachable:
            break  # this transition is unreachable (deadlock) — stop here
        s.add(*cons)
        step_cons.append(cons)
        states.append(nxt)
        choices.append(ch)
    ctx = {
        "states": states,
        "choices": choices,
        "instances": instances,
        "init_cons": init_cons_impl,
        "step_cons": step_cons,
    }

    # Each prefix is checked with a dedicated solver sp that holds "only the
    # constraints up to step t". Checking on s, which has stacked all
    # transitions, excludes violating transitions that cannot have a successor
    # (those reaching a deadlock/terminal state within the bound) from every
    # model, missing the violation (a non-monotonic bug where raising the depth
    # reduces detection). sp only appends transitions incrementally as t grows
    # and never demands any future transition.
    sp = z3.Solver()
    sp.set(unsat_core=True)
    sp.add(*init_cons_impl)

    for t in range(len(states)):
        if t > 0:
            sp.add(*step_cons[t - 1])
        alpha_t = alpha_fn(states[t])

        if t == 0:
            failure = _check_map_out_of_bounds(
                sp, alpha_t, abs_spec, impl_spec, mapping, ctx, step=0, at="init")
            if failure is not None:
                return failure

            init_cons = _abs_init_constraints(abs_spec, alpha_t)
            if init_cons:
                sp.push()
                sp.add(z3.Not(z3.And(*init_cons)))
                if sp.check() == z3.sat:
                    m = sp.model()
                    sp.pop()
                    return _failure(
                        impl_spec, abs_spec, mapping, ctx, m, at="init",
                        kind="abs_state_mismatch",
                        step=0,
                        impl_action=None,
                        alpha_before=_alpha_logical_values(m, alpha_t, abs_spec),
                        alpha_after_expected=None,
                        alpha_after_actual=_alpha_logical_values(m, alpha_t, abs_spec),
                        mismatch=["init"],
                    )
                sp.pop()
            continue

        prev = states[t - 1]
        alpha_prev = alpha_fn(prev)
        alpha_cur = alpha_t

        for idx, inst in enumerate(instances):
            act_name = inst["action"]
            amap = mapping["actions"][act_name]
            sp.push()
            sp.add(choices[t - 1] == idx)
            guards, binds = _eval_requires(
                inst["requires"], inst["lets"], prev, inst["binds"], impl_spec)
            if guards:
                sp.add(z3.And(*guards))

            if amap["kind"] == "stutter":
                sp.add(z3.Not(_logical_eq_alpha(abs_spec, alpha_prev, alpha_cur)))
                if sp.check() == z3.sat:
                    m = sp.model()
                    sp.pop()
                    ab = _alpha_logical_values(m, alpha_prev, abs_spec)
                    aa = _alpha_logical_values(m, alpha_cur, abs_spec)
                    return _failure(
                        impl_spec, abs_spec, mapping, ctx, m, at="step",
                        kind="stutter_changed_abs",
                        step=t,
                        impl_action=_inst_action(m, inst, impl_spec),
                        alpha_before=ab,
                        alpha_after_expected=ab,
                        alpha_after_actual=aa,
                        mismatch=_mismatch_paths(m, abs_spec, ab, aa),
                    )
                sp.pop()
                continue

            abs_act = _find_abs_action(abs_spec, amap["abs_action"])
            abs_binds = _abs_action_instance(
                abs_act, amap["arg_exprs"], prev, binds, impl_spec, abs_spec)
            req_guards, abs_binds = _eval_requires(
                abs_act["requires"], abs_act["lets"], alpha_prev, abs_binds, abs_spec)
            requires_ok = z3.And(*req_guards) if req_guards else z3.BoolVal(True)

            with _eval_cache_scope(expr_cache, ("alpha_prev", t)):
                pend = compute_updates(abs_act["stmts"], alpha_prev, abs_binds, abs_spec)
            alpha_expected = dict(alpha_prev)
            alpha_expected.update(pend)

            violation = z3.Or(
                z3.Not(requires_ok),
                z3.Not(_logical_eq_alpha(abs_spec, alpha_expected, alpha_cur)),
            )
            sp.add(violation)
            if sp.check() == z3.sat:
                m = sp.model()
                sp.pop()
                ab = _alpha_logical_values(m, alpha_prev, abs_spec)
                ae = _alpha_logical_values(m, alpha_expected, abs_spec)
                aa = _alpha_logical_values(m, alpha_cur, abs_spec)
                req_fail = any(
                    z3.is_false(m.eval(g, model_completion=True)) for g in req_guards
                ) if req_guards else False
                kind = "abs_requires_failed" if req_fail else "abs_state_mismatch"
                return _failure(
                    impl_spec, abs_spec, mapping, ctx, m, at="step",
                    kind=kind,
                    step=t,
                    impl_action=_inst_action(m, inst, impl_spec),
                    alpha_before=ab,
                    alpha_after_expected=ae,
                    alpha_after_actual=aa,
                    mismatch=_mismatch_paths(m, abs_spec, ae, aa),
                )
            sp.pop()

        failure = _check_map_out_of_bounds(
            sp, alpha_t, abs_spec, impl_spec, mapping, ctx, step=t, at="step")
        if failure is not None:
            return failure

    action_map = {}
    for aname, am in mapping["actions"].items():
        if am["kind"] == "stutter":
            action_map[aname] = "stutter"
        else:
            action_map[aname] = am["abs_action"]

    progress_failure, progress_checked = _check_progress_preservation(
        impl_spec, abs_spec, mapping, ctx, alpha_fn, depth)
    if progress_failure is not None:
        return progress_failure

    note = _ENSURES_NOTE if any(a.get("ensures") for a in abs_spec["actions"]) else None
    result = {
        "result": "refines",
        "impl": impl_spec["name"],
        "abs": abs_spec["name"],
        "checked_to_depth": depth,
        "action_map": action_map,
    }
    if progress_checked is not None:
        result["progress"] = progress_checked
    if note:
        result["note"] = note
    return result


def _vars_in(expr):
    """Set of variable names referenced in a mapping-expression AST."""
    out = set()

    def walk(e):
        if not isinstance(e, tuple):
            return
        if e[0] == "var":
            out.add(e[1])
            return
        for x in e[1:]:
            if isinstance(x, tuple):
                walk(x)
            elif isinstance(x, dict):
                for v in x.values():
                    walk(v)
            elif isinstance(x, list):
                for v in x:
                    walk(v)

    walk(expr)
    return out


def _subst_param_exprs(expr, name_to_expr):
    """Replace ("var", name) nodes with a replacement expression AST."""
    def walk(e):
        if not isinstance(e, tuple):
            return e
        tag = e[0]
        if tag == "var" and e[1] in name_to_expr:
            return name_to_expr[e[1]]
        if tag in ("num", "bool", "none", "var"):
            return e
        if tag == "index":
            base = e[1] if isinstance(e[1], str) else walk(e[1])
            return ("index", base, walk(e[2]))
        if tag == "field":
            return ("field", walk(e[1]), e[2])
        if tag == "struct_lit":
            return ("struct_lit", e[1], {k: walk(v) for k, v in e[2].items()})
        if tag == "bin":
            return ("bin", e[1], walk(e[2]), walk(e[3]))
        if tag == "ite":
            return ("ite", walk(e[1]), walk(e[2]), walk(e[3]))
        if tag == "neg":
            return ("neg", walk(e[1]))
        if tag == "not":
            return ("not", walk(e[1]))
        if tag == "some":
            return ("some", walk(e[1]))
        if tag in ("forall", "exists"):
            return (tag, e[1], walk(e[2]))
        if tag == "method":
            return ("method", walk(e[1]), e[2], [walk(a) for a in e[3]])
        return e

    return walk(expr)


def _compose_action_maps(am_low, am_high, mid_spec):
    """Compose impl→mid (am_low) with mid→high (am_high) into impl→high.

    stutter composes to stutter; a→b→c composes the argument expressions by
    binding b's parameters to a's mapped argument expressions (over impl state)."""
    mid_state = set(mid_spec["state"].keys())
    composed = {}
    for aname, m1 in am_low.items():
        loc = m1.get("loc")
        if m1["kind"] == "stutter":
            composed[aname] = {"kind": "stutter", "loc": loc}
            continue
        b = m1["abs_action"]
        m2 = am_high.get(b)
        if m2 is None:
            _err(f"chain: mid action '{b}' (image of '{aname}') has no correspondence "
                 f"to the next layer", kind="type")
        if m2["kind"] == "stutter":
            composed[aname] = {"kind": "stutter", "loc": loc}
            continue
        b_act = _find_abs_action(mid_spec, b)
        b_params = [p[0] for p in b_act["params"]]
        name_to_expr = dict(zip(b_params, m1["arg_exprs"]))
        new_args = []
        for e in m2["arg_exprs"]:
            for v in _vars_in(e):
                if v in mid_state and v not in name_to_expr:
                    _err(
                        "chain composition does not support action arguments that read "
                        f"intermediate-layer state ('{v}' in correspondence of '{b}')",
                        kind="type",
                    )
            new_args.append(_subst_param_exprs(e, name_to_expr))
        composed[aname] = {
            "kind": "map", "abs_action": m2["abs_action"],
            "arg_exprs": new_args, "loc": loc,
        }
    return composed


def refine_chain(specs, mappings, depth):
    """Check specs[0] ⊒ specs[-1] end-to-end by composing adjacent mappings.

    specs:    [low, mid, ..., top]            (N specs, N >= 2)
    mappings: [m_{0→1}, ..., m_{N-2→N-1}]      (m_i maps specs[i] as impl to specs[i+1] as abs)

    Bounded refinement is transitive at equal depth (per-step local checking),
    so composing the per-layer maps and checking low ⊒ top directly is sound and
    equivalent to all adjacent links holding (DESIGN-refinement §7)."""
    if len(specs) < 2 or len(mappings) != len(specs) - 1:
        _err("chain: need N specs and N-1 mappings (impl abs map [abs map]...)", kind="type")
    for i, mp in enumerate(mappings):
        if mp["impl"] != specs[i]["name"]:
            _err(f"chain: mapping {i} impl '{mp['impl']}' != spec '{specs[i]['name']}'", kind="type")
        if mp["abs"] != specs[i + 1]["name"]:
            _err(f"chain: mapping {i} abs '{mp['abs']}' != spec '{specs[i + 1]['name']}'", kind="type")
        if mp.get("progress"):
            link = refine(specs[i], specs[i + 1], mp, depth)
            if link.get("result") != "refines":
                link = dict(link)
                link["chain"] = [s["name"] for s in specs]
                link["failed_link"] = {
                    "from": specs[i]["name"],
                    "to": specs[i + 1]["name"],
                    "kind": link.get("kind"),
                }
                return link
    low, top = specs[0], specs[-1]

    # Composed alpha: fold build_alpha across layers (Z3-level composition).
    def alpha_fn(low_state):
        st = low_state
        for i, mp in enumerate(mappings):
            st = build_alpha(st, mp, specs[i], specs[i + 1])
        return st

    # Composed action correspondence: fold adjacent maps.
    composed_actions = mappings[0]["actions"]
    for i in range(1, len(mappings)):
        composed_actions = _compose_action_maps(
            composed_actions, mappings[i]["actions"], specs[i])

    composed_mapping = {
        "name": f"{low['name']}RefinesChain",
        "impl": low["name"], "abs": top["name"],
        "maps": {},  # unused: alpha_fn supplies the (composed) state mapping
        "actions": composed_actions,
    }
    result = refine(low, top, composed_mapping, depth, alpha_fn=alpha_fn)
    result["chain"] = [s["name"] for s in specs]

    # On failure, pinpoint the first broken adjacent link (more actionable than
    # the composed end-to-end trace alone).
    if result.get("result") == "refinement_failed":
        for i, mp in enumerate(mappings):
            link = refine(specs[i], specs[i + 1], mp, depth)
            if link.get("result") != "refines":
                result["failed_link"] = {
                    "from": specs[i]["name"], "to": specs[i + 1]["name"],
                    "kind": link.get("kind"),
                }
                break
    return result


def _inst_action(model, inst, spec):
    act = inst["action_def"]
    la = annotate_display_name({
        "name": inst["action"],
        "params": {
            pk: _display_param(pk, pv, act, spec) for pk, pv in inst["binds"].items()
        },
    }, inst["action"], spec)
    if act.get("loc"):
        la["loc"] = act["loc"]
    meta = act.get("meta")
    if meta:
        la["requirement"] = {"id": meta["id"], "text": meta.get("text")}
    return la


def _failure(
        impl_spec, abs_spec, mapping, explored, model, at, kind, step,
        impl_action, alpha_before, alpha_after_expected, alpha_after_actual, mismatch):
    trace = _build_trace(
        model, explored["states"], explored["choices"],
        explored["instances"], impl_spec, step,
    )
    out = {
        "result": "refinement_failed",
        "impl": impl_spec["name"],
        "abs": abs_spec["name"],
        "at": at,
        "violated_at_step": step,
        "impl_action": impl_action,
        "kind": kind,
        "impl_trace": trace,
        "abs_before": alpha_before,
        "abs_after_expected": alpha_after_expected,
        "abs_after_actual": alpha_after_actual,
        "mismatch": mismatch,
        "hint": _REFINE_HINT,
    }
    # Hoist the involved impl action's requirement to the root, for parity with
    # verify violations (which carry `requirement` at the top level).
    req = impl_action.get("requirement") if isinstance(impl_action, dict) else None
    if req:
        out["requirement"] = req
    return with_faithfulness(out)
