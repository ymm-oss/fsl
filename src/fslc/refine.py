"""Refinement checking: impl spec refines abs spec via a mapping file."""
from __future__ import annotations

import itertools

import z3

from .bmc import (
    _bmc_explore,
    _build_trace,
    _display_param,
    _eval_requires,
    _expr_static_type,
    _last_action,
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
    _z3_domain_value,
)
from .model import FslError, bounds_invariant_expr, domain_range as model_domain_range


_REFINE_HINT = (
    "the impl step does not correspond to the mapped abs action; "
    "fix the map expressions, the action correspondence, or guard the impl action"
)

_ENSURES_NOTE = (
    "abs ensures are not checked during refinement; verify/prove the abstract spec separately"
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
    if abs_ty[0] == "domain" and impl_ty[0] == "int":
        return True
    if abs_ty[0] == "enum" and impl_ty[0] == "enum" and abs_ty[1] == impl_ty[1]:
        return True
    return False


def _merge_types_meta(impl_spec, abs_spec):
    """Merge type metadata; abs types take precedence on name clash."""
    merged = dict(impl_spec["types"])
    for name, info in abs_spec["types"].items():
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


def build_refinement(tree, impl_spec, abs_spec):
    """Validate and normalize a refinement mapping AST."""
    _, name, items = tree
    impl_name = None
    abs_name = None
    maps = {}
    actions = {}

    for it in items:
        tag = it[0]
        if tag == "impl":
            impl_name = it[1]
        elif tag == "abs":
            abs_name = it[1]
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
            impl_param_names = [p[0] for p in impl_act["params"]]
            if list(params) != impl_param_names:
                _err(
                    f"action '{aname}' parameter names/order must match impl "
                    f"({impl_param_names})",
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

    for logical in abs_spec["state"]:
        if logical not in maps:
            _err(f"missing map for abstract state variable '{logical}'", kind="type")

    for act in impl_spec["actions"]:
        if act["name"] not in actions:
            _err(f"missing action correspondence for impl action '{act['name']}'", kind="type")

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

    return {
        "name": name,
        "impl": impl_name,
        "abs": abs_name,
        "maps": maps,
        "actions": actions,
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


def refine(impl_spec, abs_spec, mapping, depth):
    """Check that impl refines abs under the mapping to bounded depth."""
    explored = _bmc_explore(impl_spec, depth, deadlock_mode="ignore")
    if explored["result"] != "explored":
        return explored

    instances = explored["instances"]

    # Build our own incremental unrolling instead of reusing the verify-side
    # solver: the impl may deadlock before `depth`, which makes a full
    # depth-length unrolling unsatisfiable and would hide every violation
    # (refine would vacuously report "refines"). Check each reachable prefix
    # length and stop once the prefix becomes unsatisfiable.
    expr_cache = {}
    states = [make_state(impl_spec, 0)]
    choices = []
    s = z3.Solver()
    s.set(unsat_core=True)
    with _eval_cache_scope(expr_cache, id(states[0])):
        s.add(*init_constraints(impl_spec, states[0]))

    # 到達可能なプレフィックスだけを展開する。impl が depth より手前で
    # デッドロックすると「深さちょうど depth」の完全展開は充足不能になり、
    # 全ての違反検査が unsat=見逃しとなって空虚に refines を返してしまう。
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
            break  # この遷移は到達不能(デッドロック)— ここで打ち切る
        s.add(*cons)
        states.append(nxt)
        choices.append(ch)
    ctx = {"states": states, "choices": choices, "instances": instances}

    for t in range(len(states)):
        alpha_t = build_alpha(states[t], mapping, impl_spec, abs_spec)

        if t == 0:
            failure = _check_map_out_of_bounds(
                s, alpha_t, abs_spec, impl_spec, mapping, ctx, step=0, at="init")
            if failure is not None:
                return failure

            init_cons = _abs_init_constraints(abs_spec, alpha_t)
            if init_cons:
                s.push()
                s.add(z3.Not(z3.And(*init_cons)))
                if s.check() == z3.sat:
                    m = s.model()
                    s.pop()
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
                s.pop()
            continue

        prev = states[t - 1]
        alpha_prev = build_alpha(prev, mapping, impl_spec, abs_spec)
        alpha_cur = alpha_t

        for idx, inst in enumerate(instances):
            act_name = inst["action"]
            amap = mapping["actions"][act_name]
            s.push()
            s.add(choices[t - 1] == idx)
            guards, binds = _eval_requires(
                inst["requires"], inst["lets"], prev, inst["binds"], impl_spec)
            if guards:
                s.add(z3.And(*guards))

            if amap["kind"] == "stutter":
                s.add(z3.Not(_logical_eq_alpha(abs_spec, alpha_prev, alpha_cur)))
                if s.check() == z3.sat:
                    m = s.model()
                    s.pop()
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
                s.pop()
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
            s.add(violation)
            if s.check() == z3.sat:
                m = s.model()
                s.pop()
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
            s.pop()

        failure = _check_map_out_of_bounds(
            s, alpha_t, abs_spec, impl_spec, mapping, ctx, step=t, at="step")
        if failure is not None:
            return failure

    action_map = {}
    for aname, am in mapping["actions"].items():
        if am["kind"] == "stutter":
            action_map[aname] = "stutter"
        else:
            action_map[aname] = am["abs_action"]

    note = _ENSURES_NOTE if any(a.get("ensures") for a in abs_spec["actions"]) else None
    result = {
        "result": "refines",
        "impl": impl_spec["name"],
        "abs": abs_spec["name"],
        "checked_to_depth": depth,
        "action_map": action_map,
    }
    if note:
        result["note"] = note
    return result


def _inst_action(model, inst, spec):
    act = inst["action_def"]
    la = {
        "name": inst["action"],
        "params": {
            pk: _display_param(pk, pv, act, spec) for pk, pv in inst["binds"].items()
        },
    }
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
    return {
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
