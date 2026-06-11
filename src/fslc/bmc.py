"""Bounded model checker for FSL v1."""
from __future__ import annotations

import itertools
from contextlib import contextmanager

import z3

from .model import FslError, binder_range, domain_range, eval_const, phys_z3_sort, z3_sort


def _err(msg, kind="semantics", loc=None, expected=None, hint=None):
    raise FslError(msg, kind=kind, loc=loc, expected=expected, hint=hint)


def _warn(message, hint=None):
    w = {"message": message}
    if hint:
        w["hint"] = hint
    return w


def _public_bindings(binds):
    out = {}
    for k, v in binds.items():
        out["key" if k == "__k" else k] = v
    return out


_DROP_BINDING = object()
_EVAL_CACHE = None
_EVAL_CACHE_TOKEN = None
_EXPR_HAS_IS_CACHE = {}


@contextmanager
def _eval_cache_scope(cache, token):
    global _EVAL_CACHE, _EVAL_CACHE_TOKEN
    old_cache, old_token = _EVAL_CACHE, _EVAL_CACHE_TOKEN
    _EVAL_CACHE, _EVAL_CACHE_TOKEN = cache, token
    try:
        yield
    finally:
        _EVAL_CACHE, _EVAL_CACHE_TOKEN = old_cache, old_token


def _expr_contains_is(e):
    if not isinstance(e, tuple):
        return False
    cache_key = id(e)
    cached = _EXPR_HAS_IS_CACHE.get(cache_key)
    if cached is not None:
        cached_e, found = cached
        if cached_e is e:
            return found
    found = e[0] == "is"
    if not found:
        for child in e[1:]:
            if isinstance(child, tuple):
                found = _expr_contains_is(child)
            elif isinstance(child, dict):
                found = any(_expr_contains_is(v) for v in child.values() if isinstance(v, tuple))
            elif isinstance(child, list):
                found = any(_expr_contains_is(v) for v in child if isinstance(v, tuple))
            if found:
                break
    _EXPR_HAS_IS_CACHE[cache_key] = (e, found)
    return found


def _freeze_cache_value(v):
    if isinstance(v, (int, bool, str)) or v is None:
        return v
    if isinstance(v, z3.ExprRef):
        return ("z3", v.get_id())
    if isinstance(v, tuple):
        out = []
        for item in v:
            frozen = _freeze_cache_value(item)
            if frozen is _DROP_BINDING:
                return _DROP_BINDING
            out.append(frozen)
        return tuple(out)
    return _DROP_BINDING


def _freeze_binds_for_cache(binds):
    items = []
    for k, v in sorted(binds.items()):
        frozen = _freeze_cache_value(v)
        if frozen is _DROP_BINDING:
            return None
        items.append((k, frozen))
    return tuple(items)


def _json_binding_value(model, value, ty, spec):
    if isinstance(value, bool):
        return value
    if isinstance(value, int):
        return _display_value(ty, value, spec) if ty else value
    if value is None or isinstance(value, str):
        return value
    if not isinstance(value, z3.ExprRef):
        return _DROP_BINDING

    try:
        concrete = model.eval(value, model_completion=True)
    except z3.Z3Exception:
        return _DROP_BINDING

    if z3.is_true(concrete):
        return True
    if z3.is_false(concrete):
        return False
    if z3.is_int_value(concrete):
        raw = concrete.as_long()
        return _display_value(ty, raw, spec) if ty else raw
    if z3.is_bv_value(concrete):
        raw = concrete.as_long()
        return _display_value(ty, raw, spec) if ty else raw
    return _DROP_BINDING


def _public_model_bindings(model, binds, spec, binding_types):
    out = {}
    for k, v in binds.items():
        public_key = "key" if k == "__k" else k
        public_value = _json_binding_value(model, v, binding_types.get(k), spec)
        if public_value is not _DROP_BINDING:
            out[public_key] = public_value
    return out


def make_state(spec, t):
    return {p["phys"]: z3.Const(f"{p['phys']}@{t}", phys_z3_sort(p, spec["types"]))
            for p in spec["phys_vars"]}


def make_ind_state(spec, t):
    return {p["phys"]: z3.Const(f"{p['phys']}@ind{t}", phys_z3_sort(p, spec["types"]))
            for p in spec["phys_vars"]}


_CTI_HINT = (
    "this state sequence satisfies all invariants but leads to a violation; "
    "the start state may be unreachable — add an auxiliary invariant that excludes it, "
    "then re-run"
)


def _inv_constraint(inv, state, spec, expr_cache):
    with _eval_cache_scope(expr_cache, id(state)):
        return eval_expr(inv["expr"], state, {}, spec)


def _enum_phys_constraints(spec, state):
    """Physical enum range when not already covered by _bounds_* invariants."""
    cons = []
    covered = {inv.get("logical_var") for inv in spec["invariants"] if inv.get("implicit")}
    for n, ty in spec["state"].items():
        if ty[0] != "enum" or n in covered:
            continue
        lo, hi = domain_range(ty, spec["types"])
        phys = n
        for p in spec["phys_vars"]:
            if p["logical"] == n and "part" not in p:
                phys = p["phys"]
                break
        cons.append(state[phys] >= lo)
        cons.append(state[phys] <= hi)
    return cons


def _is_enum_member(name, spec):
    for info in spec["types"].values():
        if info["kind"] == "enum" and name in info["members"]:
            return info["members"].index(name)
    return None


def _type_of_expr(e, spec, ctx_ty=None):
    """Rough type inference for literals; ctx_ty used for enum member names."""
    tag = e[0]
    if tag == "num":
        return ("int",)
    if tag == "bool":
        return ("bool",)
    if tag == "none":
        return ("option", ("int",))
    if tag == "var":
        n = e[1]
        if n in spec["state"]:
            return spec["state"][n]
        ei = _is_enum_member(n, spec)
        if ei is not None:
            return ("enum", None)
    return ctx_ty


def eval_expr(e, state, binds, spec, old_state=None, in_ensures=False):
    cache_key = None
    if (
        _EVAL_CACHE is not None
        and _EVAL_CACHE_TOKEN is not None
        and not in_ensures
        and not _expr_contains_is(e)
    ):
        frozen_binds = _freeze_binds_for_cache(binds)
        if frozen_binds is not None:
            cache_key = (_EVAL_CACHE_TOKEN, id(e), frozen_binds)
            if cache_key in _EVAL_CACHE:
                return _EVAL_CACHE[cache_key]
    result = _eval_expr_uncached(e, state, binds, spec, old_state, in_ensures)
    if cache_key is not None:
        _EVAL_CACHE[cache_key] = result
    return result


def _eval_expr_uncached(e, state, binds, spec, old_state=None, in_ensures=False):
    consts = spec["consts"]
    tag = e[0]
    if tag == "num":
        return z3.IntVal(e[1])
    if tag == "bool":
        return z3.BoolVal(e[1])
    if tag == "none":
        return ("none",)
    if tag == "some":
        v = eval_expr(e[1], state, binds, spec, old_state, in_ensures)
        if isinstance(v, tuple) and v[0] == "option_val":
            _err("nested Option in some()")
        return ("option_val", z3.BoolVal(True), v)
    if tag == "set_lit":
        _err("bare Set literal must appear in assignment (use shipped = Set {} on a Set-typed variable)")
    if tag == "struct_lit":
        sname, fields = e[1], e[2]
        vals = {}
        for fn, fe in fields.items():
            vals[fn] = eval_expr(fe, state, binds, spec, old_state, in_ensures)
        return ("struct_val", sname, vals)
    if tag == "neg":
        return -eval_expr(e[1], state, binds, spec, old_state, in_ensures)
    if tag == "var":
        n = e[1]
        if n in binds:
            b = binds[n]
            return b if not isinstance(b, int) else z3.IntVal(b)
        if n in consts:
            return z3.IntVal(consts[n])
        ei = _is_enum_member(n, spec)
        if ei is not None:
            return z3.IntVal(ei)
        if n in spec["state"]:
            ty = spec["state"][n]
            if ty[0] == "option":
                return ("option_val", state[f"{n}__present"], state[f"{n}__value"])
            if ty[0] == "struct":
                sname = ty[1]
                return ("struct_val", sname, {
                    fn: state[f"{n}__{fn}"]
                    for fn in spec["types"][sname]["fields"]
                })
            if ty[0] == "set":
                return ("set_val", state[n], ty[1])
        if n in state:
            return state[n]
        _err(f"unknown identifier '{n}'")
    if tag == "index":
        base_e = e[1]
        idx = eval_expr(e[2], state, binds, spec, old_state, in_ensures)
        return _eval_index(base_e, idx, state, binds, spec, old_state, in_ensures)
    if tag == "field":
        base = eval_expr(e[1], state, binds, spec, old_state, in_ensures)
        return _eval_field(base, e[2], state, binds, spec)
    if tag == "method":
        base = eval_expr(e[1], state, binds, spec, old_state, in_ensures)
        return _eval_method(base, e[2], e[3], state, binds, spec, old_state, in_ensures)
    if tag == "is":
        return _eval_is(e[1], e[2], state, binds, spec, old_state, in_ensures)
    if tag == "not":
        return z3.Not(eval_expr(e[1], state, binds, spec, old_state, in_ensures))
    if tag == "bin":
        op = e[1]
        a = eval_expr(e[2], state, binds, spec, old_state, in_ensures)
        b = eval_expr(e[3], state, binds, spec, old_state, in_ensures)
        if op in ("==", "!="):
            none_cmp = _option_none_cmp(a, b, op)
            if none_cmp is not None:
                return none_cmp
            struct_cmp = _struct_compare(a, b, op, spec)
            if struct_cmp is not None:
                return struct_cmp
            _reject_option_binop(a, b, op)
        else:
            _reject_option_binop(a, b, op)
        a, b = _unify_option_cmp(a, b)
        if op == "+":
            return a + b
        if op == "-":
            return a - b
        if op == "*":
            return a * b
        if op == "==":
            return a == b
        if op == "!=":
            return a != b
        if op == "<":
            return a < b
        if op == "<=":
            return a <= b
        if op == ">":
            return a > b
        if op == ">=":
            return a >= b
        if op == "and":
            return z3.And(a, b)
        if op == "or":
            return z3.Or(a, b)
        if op == "=>":
            return z3.Implies(a, b)
        _err(f"unknown operator '{op}'")
    if tag in ("forall", "exists"):
        return _eval_quant(e, state, binds, spec, old_state, in_ensures)
    if tag == "count":
        return _eval_count(e, state, binds, spec, old_state, in_ensures)
    if tag == "sum":
        return _eval_sum(e, state, binds, spec, old_state, in_ensures)
    if tag == "min":
        a, b = eval_expr(e[1], state, binds, spec, old_state, in_ensures), \
               eval_expr(e[2], state, binds, spec, old_state, in_ensures)
        return z3.If(a <= b, a, b)
    if tag == "max":
        a, b = eval_expr(e[1], state, binds, spec, old_state, in_ensures), \
               eval_expr(e[2], state, binds, spec, old_state, in_ensures)
        return z3.If(a >= b, a, b)
    if tag == "abs":
        a = eval_expr(e[1], state, binds, spec, old_state, in_ensures)
        return z3.If(a >= 0, a, -a)
    if tag == "old":
        if not in_ensures:
            _err("old() is only allowed in ensures clauses", kind="type")
        if old_state is None:
            _err("old() used without old state context")
        with _eval_cache_scope(None, None):
            return eval_expr(e[1], old_state, binds, spec, None, False)
    _err(f"cannot evaluate expression node {e}")


def _struct_info(val, spec):
    if not isinstance(val, tuple):
        return None, None
    if val[0] == "struct_val":
        return val[1], val[2]
    if val[0] == "struct_map_val":
        logical = val[1]
        ty = spec["state"].get(logical)
        if ty and ty[0] == "map" and ty[2][0] == "struct":
            return ty[2][1], val[3]
    return None, None


def _struct_compare(a, b, op, spec):
    sa, fa = _struct_info(a, spec)
    sb, fb = _struct_info(b, spec)
    if sa is None and sb is None:
        return None
    if sa is None or sb is None:
        _err("struct comparison requires two struct values", kind="type")
    if sa != sb:
        _err(f"struct comparison between '{sa}' and '{sb}'", kind="type")
    if set(fa) != set(fb):
        _err(f"struct field mismatch in comparison of '{sa}'", kind="type")
    if not fa:
        res = z3.BoolVal(True)
    else:
        res = z3.And(*[fa[k] == fb[k] for k in fa])
    return z3.Not(res) if op == "!=" else res


def _option_none_cmp(a, b, op):
    if isinstance(a, tuple) and a[0] == "option_val" and isinstance(b, tuple) and b[0] == "none":
        present = a[1]
        return z3.Not(present) if op == "==" else present
    if isinstance(b, tuple) and b[0] == "option_val" and isinstance(a, tuple) and a[0] == "none":
        present = b[1]
        return z3.Not(present) if op == "==" else present
    return None


_OPTION_EQ_HINT = "use `x is some(v)` to compare the contained value"


def _option_tag(v):
    if isinstance(v, tuple) and v[0] in ("option_val", "none"):
        return v[0]
    return None


def _reject_option_binop(a, b, op):
    ta, tb = _option_tag(a), _option_tag(b)
    if ta is None and tb is None:
        return
    if op in ("==", "!="):
        if ta == "none" and tb == "none":
            return
        _err(
            "Option == and != are only defined against none",
            kind="type",
            hint=_OPTION_EQ_HINT,
        )
    _err(f"Option values cannot be used with '{op}'", kind="type")


def _unify_option_cmp(a, b):
    if isinstance(a, tuple) and a[0] == "option_val":
        if isinstance(b, tuple) and b[0] == "none":
            return a[1], z3.BoolVal(False)
    if isinstance(b, tuple) and b[0] == "option_val":
        if isinstance(a, tuple) and a[0] == "none":
            return z3.BoolVal(False), b[1]
    return a, b


def _logical_map_access(logical, idx, state, spec):
    ty = spec["state"][logical]
    if ty[0] != "map":
        _err(f"'{logical}' is not a map")
    kty, vty = ty[1], ty[2]
    if vty[0] == "struct":
        sname = vty[1]
        fields = spec["types"][sname]["fields"]
        return ("struct_map_val", logical, idx, {
            fn: z3.Select(state[f"{logical}__{fn}"], idx) for fn in fields
        })
    if vty[0] == "option":
        return ("option_val",
                z3.Select(state[f"{logical}__present"], idx),
                z3.Select(state[f"{logical}__value"], idx))
    return z3.Select(state[logical], idx)


def _eval_index(base_e, idx, state, binds, spec, old_state, in_ensures):
    if isinstance(base_e, str):
        name = base_e
    elif base_e[0] == "var":
        name = base_e[1]
    else:
        _err("complex index base not supported")
    if name in spec["state"]:
        return _logical_map_access(name, idx, state, spec)
    if name in state:
        return z3.Select(state[name], idx)
    _err(f"unknown map '{name}'")


def _eval_field(base, field, state, binds, spec):
    if isinstance(base, tuple) and base[0] == "struct_map_val":
        logical, idx, fields = base[1], base[2], base[3]
        if field not in fields:
            _err(f"unknown field '{field}'")
        return fields[field]
    if isinstance(base, tuple) and base[0] == "struct_val":
        _, sname, vals = base
        if field not in vals:
            _err(f"unknown field '{field}' in struct {sname}")
        return vals[field]
    _err(f"cannot access field '{field}' on this value")


def _set_elem_ty(base, state, spec):
    if isinstance(base, tuple) and base[0] == "set_val":
        return base[1], base[2]
    if isinstance(base, z3.ArrayRef):
        for n, ty in spec["state"].items():
            if ty[0] == "set" and state.get(n) is base:
                return base, ty[1]
    _err("method call on non-set value")


def _eval_method(base, method, args, state, binds, spec, old_state, in_ensures):
    m, elem_ty = _set_elem_ty(base, state, spec)

    if method == "contains":
        if len(args) != 1:
            _err("contains expects 1 argument")
        e = eval_expr(args[0], state, binds, spec, old_state, in_ensures)
        return z3.Select(m, e)
    if method == "add":
        if len(args) != 1:
            _err("add expects 1 argument")
        e = eval_expr(args[0], state, binds, spec, old_state, in_ensures)
        return ("set_val", z3.Store(m, e, z3.BoolVal(True)), elem_ty)
    if method == "remove":
        if len(args) != 1:
            _err("remove expects 1 argument")
        e = eval_expr(args[0], state, binds, spec, old_state, in_ensures)
        return ("set_val", z3.Store(m, e, z3.BoolVal(False)), elem_ty)
    if method == "size":
        if args:
            _err("size expects no arguments")
        lo, hi = domain_range(elem_ty, spec["types"])
        terms = [z3.If(z3.Select(m, z3.IntVal(i)), z3.IntVal(1), z3.IntVal(0))
                 for i in range(lo, hi + 1)]
        acc = z3.IntVal(0)
        for t in terms:
            acc = acc + t
        return acc
    _err(f"unknown method '{method}'")


def _eval_is(inner, pat, state, binds, spec, old_state, in_ensures):
    val = eval_expr(inner, state, binds, spec, old_state, in_ensures)
    if pat[0] == "pat_none":
        if isinstance(val, tuple) and val[0] == "option_val":
            return z3.Not(val[1])
        _err("is none applied to non-Option value", kind="type")
    if pat[0] == "pat_some":
        vname = pat[1]
        if isinstance(val, tuple) and val[0] == "option_val":
            present, value = val[1], val[2]
            binds[vname] = value
            return present
        _err("is some applied to non-Option value", kind="type")
    _err("invalid pattern")


def _eval_quant(e, state, binds, spec, old_state, in_ensures):
    qop, binder, body = e[0], e[1], e[2]
    v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
    insts = []
    for i in range(lo, hi + 1):
        b2 = dict(binds)
        b2[v] = i
        if where is not None:
            w = eval_expr(where, state, b2, spec, old_state, in_ensures)
            if qop == "forall":
                body_inst = eval_expr(body, state, b2, spec, old_state, in_ensures)
                insts.append(z3.Implies(w, body_inst))
            else:
                body_inst = eval_expr(body, state, b2, spec, old_state, in_ensures)
                insts.append(z3.And(w, body_inst))
        else:
            insts.append(eval_expr(body, state, b2, spec, old_state, in_ensures))
    if not insts:
        return z3.BoolVal(qop == "forall")
    return z3.And(*insts) if qop == "forall" else z3.Or(*insts)


def _eval_count(e, state, binds, spec, old_state, in_ensures):
    _, v, ty_name, cond = e
    if ty_name not in spec["types"]:
        _err(f"unknown type '{ty_name}' in count")
    ty = spec["types"][ty_name]["ty"]
    lo, hi = domain_range(ty, spec["types"])
    terms = []
    for i in range(lo, hi + 1):
        b2 = {**binds, v: i}
        c = eval_expr(cond, state, b2, spec, old_state, in_ensures)
        terms.append(z3.If(c, z3.IntVal(1), z3.IntVal(0)))
    acc = z3.IntVal(0)
    for t in terms:
        acc = acc + t
    return acc


def _eval_sum(e, state, binds, spec, old_state, in_ensures):
    _, v, ty_name, body, cond = e
    if ty_name not in spec["types"]:
        _err(f"unknown type '{ty_name}' in sum")
    ty = spec["types"][ty_name]["ty"]
    lo, hi = domain_range(ty, spec["types"])
    terms = []
    for i in range(lo, hi + 1):
        b2 = {**binds, v: i}
        if cond is not None:
            c = eval_expr(cond, state, b2, spec, old_state, in_ensures)
            val = eval_expr(body, state, b2, spec, old_state, in_ensures)
            terms.append(z3.If(c, val, z3.IntVal(0)))
        else:
            terms.append(eval_expr(body, state, b2, spec, old_state, in_ensures))
    acc = z3.IntVal(0)
    for t in terms:
        acc = acc + t
    return acc


def _lvalue_key(lv):
    if lv[0] == "var":
        return ("scalar", lv[1])
    if lv[0] == "index":
        return ("map", lv[1], lv[2])
    if lv[0] == "field_lv":
        base, field = lv[1], lv[2]
        if base[0] == "index":
            return ("map_field", base[1], base[2], field)
        return ("field", base[1], field)
    _err(f"invalid lvalue {lv}")


def _apply_assign(lv, rhs, pend, state, binds, spec):
    key = _lvalue_key(lv)

    if key[0] == "scalar":
        n = key[1]
        if n not in spec["state"]:
            _err(f"assignment to unknown state variable '{n}'")
        ty = spec["state"][n]
        if ty[0] == "set" and rhs[0] == "set_lit":
            elem_ty = ty[1]
            m = z3.K(z3_sort(elem_ty, spec["types"]), z3.BoolVal(False))
            for lit in rhs[1]:
                idx = eval_expr(lit, state, binds, spec)
                m = z3.Store(m, idx, z3.BoolVal(True))
            pend[n] = m
            return ("scalar", n)

    val = eval_expr(rhs, state, binds, spec)

    if key[0] == "scalar":
        n = key[1]
        if n not in spec["state"]:
            _err(f"assignment to unknown state variable '{n}'")
        ty = spec["state"][n]
        if ty[0] == "set":
            if isinstance(val, tuple) and val[0] == "set_val":
                pend[n] = val[1]
            else:
                pend[n] = val
        elif ty[0] == "option":
            if isinstance(val, tuple) and val[0] == "option_val":
                pend[f"{n}__present"] = val[1]
                pend[f"{n}__value"] = val[2]
            elif val == ("none",):
                pend[f"{n}__present"] = z3.BoolVal(False)
            else:
                _err("Option assignment requires none or some(...)")
        elif ty[0] == "struct":
            if isinstance(val, tuple) and val[0] == "struct_val":
                sname, fields = val[1], val[2]
                for fn, fv in fields.items():
                    pend[f"{n}__{fn}"] = fv
            else:
                _err("struct assignment requires struct literal")
        else:
            pend[n] = val
        return ("scalar", n)

    if key[0] == "map":
        n, idx_e = key[1], key[2]
        if n not in spec["state"]:
            _err(f"assignment to unknown map '{n}'")
        ty = spec["state"][n]
        idx = eval_expr(idx_e, state, binds, spec)
        vty = ty[2]
        if vty[0] == "option":
            if isinstance(val, tuple) and val[0] == "option_val":
                base_p = pend.get(f"{n}__present", state[f"{n}__present"])
                base_v = pend.get(f"{n}__value", state[f"{n}__value"])
                pend[f"{n}__present"] = z3.Store(base_p, idx, val[1])
                pend[f"{n}__value"] = z3.Store(base_v, idx, val[2])
            elif rhs[0] == "none":
                base_p = pend.get(f"{n}__present", state[f"{n}__present"])
                pend[f"{n}__present"] = z3.Store(base_p, idx, z3.BoolVal(False))
            else:
                _err("Option map assignment requires none or some(...)")
        elif vty[0] == "struct":
            if isinstance(val, tuple) and val[0] == "struct_val":
                _, sname, fields = val
                for fn, fv in fields.items():
                    phys = f"{n}__{fn}"
                    base = pend.get(phys, state[phys])
                    pend[phys] = z3.Store(base, idx, fv)
            else:
                _err("struct map assignment requires struct literal")
        else:
            base = pend.get(n, state[n])
            pend[n] = z3.Store(base, idx, val)
        return ("map", n, idx_e)

    if key[0] == "map_field":
        n, idx_e, field = key[1], key[2], key[3]
        idx = eval_expr(idx_e, state, binds, spec)
        phys = f"{n}__{field}"
        base = pend.get(phys, state[phys])
        pend[phys] = z3.Store(base, idx, val)
        return ("map_field", n, idx_e, field)

    if key[0] == "field":
        n, field = key[1], key[2]
        phys = f"{n}__{field}"
        pend[phys] = val
        return ("field", n, field)

    return None


def compute_updates(stmts, state, binds, spec):
    pend = {}
    scalar_writes = set()

    def check_scalar(kind, *parts, loc=None):
        if kind in ("scalar", "field"):
            key = (kind,) + parts
            if key in scalar_writes:
                _err(
                    f"double assignment to '{parts[0]}' on the same execution path",
                    kind="semantics",
                    loc=loc,
                )
            scalar_writes.add(key)
        if kind == "map_field":
            key = ("map_field",) + parts
            if key in scalar_writes:
                _err(
                    f"double assignment to '{parts[0]}' field '{parts[2]}' on the same path",
                    kind="semantics",
                    loc=loc,
                )
            scalar_writes.add(key)

    def run(st, binds):
        tag = st[0]
        if tag == "assign":
            w = _apply_assign(st[1], st[2], pend, state, binds, spec)
            if w:
                check_scalar(*w, loc=st[3] if len(st) > 3 else None)
        elif tag == "if":
            _, cond, then_stmts, else_stmts, _ = st
            c = eval_expr(cond, state, binds, spec)
            then_pend = {}
            else_pend = {}
            save_scalars = set(scalar_writes)

            def run_branch(branch_stmts, target, binds):
                scalar_writes.clear()
                scalar_writes.update(save_scalars)
                local = {}
                for s2 in branch_stmts:
                    run_into(s2, binds, local)
                target.update(local)
                return set(scalar_writes)

            def run_into(st2, binds, local_pend):
                if st2[0] == "assign":
                    saved = dict(pend)
                    pend.clear()
                    pend.update(saved)
                    pend.update(local_pend)
                    w = _apply_assign(st2[1], st2[2], pend, state, binds, spec)
                    if w:
                        check_scalar(*w, loc=st2[3] if len(st2) > 3 else None)
                    local_pend.update({k: pend[k] for k in pend if k not in saved or pend[k] is not saved.get(k)})
                    pend.clear()
                    pend.update(saved)
                elif st2[0] == "if":
                    run_into_if(st2, binds, local_pend)
                elif st2[0] == "forall_stmt":
                    _, binder, body, _ = st2
                    v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
                    for i in range(lo, hi + 1):
                        b2 = {**binds, v: i}
                        if where is not None:
                            wcond = eval_expr(where, state, b2, spec)
                            saved = dict(local_pend)
                            inner = {}
                            for s3 in body:
                                run_into(s3, b2, inner)
                            for k, v2 in inner.items():
                                local_pend[k] = z3.If(wcond, v2, saved.get(k, state[k]))
                        else:
                            for s3 in body:
                                run_into(s3, b2, local_pend)
                else:
                    _err(f"unknown stmt {st2}")

            def run_into_if(st2, binds, local_pend):
                _, cond2, th, el, _ = st2
                c2 = eval_expr(cond2, state, binds, spec)
                th_p, el_p = {}, {}
                save_scalars2 = set(scalar_writes)
                scalar_writes.clear()
                scalar_writes.update(save_scalars2)
                for s3 in th:
                    run_into(s3, binds, th_p)
                th_writes = set(scalar_writes)
                scalar_writes.clear()
                scalar_writes.update(save_scalars2)
                for s3 in el:
                    run_into(s3, binds, el_p)
                el_writes = set(scalar_writes)
                scalar_writes.clear()
                scalar_writes.update(save_scalars2 | th_writes | el_writes)
                all_keys = set(th_p) | set(el_p) | set(local_pend)
                for k in all_keys:
                    tv = th_p.get(k, local_pend.get(k, state[k]))
                    ev = el_p.get(k, local_pend.get(k, state[k]))
                    local_pend[k] = z3.If(c2, tv, ev)

            then_writes = run_branch(then_stmts, then_pend, binds)
            else_writes = run_branch(else_stmts, else_pend, binds)
            scalar_writes.clear()
            scalar_writes.update(save_scalars | then_writes | else_writes)
            all_keys = set(then_pend) | set(else_pend) | set(pend)
            for k in all_keys:
                fb = pend.get(k, state[k])
                tv = then_pend.get(k, fb)
                ev = else_pend.get(k, fb)
                pend[k] = z3.If(c, tv, ev)
        elif tag == "forall_stmt":
            _, binder, body, _ = st
            v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
            for i in range(lo, hi + 1):
                b2 = {**binds, v: i}
                if where is not None:
                    wcond = eval_expr(where, state, b2, spec)
                    saved = dict(pend)
                    inner = {}
                    pend.clear()
                    pend.update(saved)
                    for s2 in body:
                        run(s2, b2)
                    for k, v2 in pend.items():
                        if k not in saved:
                            saved[k] = z3.If(wcond, v2, state[k])
                        elif pend[k] is not saved.get(k):
                            saved[k] = z3.If(wcond, v2, saved.get(k, state[k]))
                    pend.clear()
                    pend.update(saved)
                else:
                    for s2 in body:
                        run(s2, b2)
        else:
            _err(f"unknown stmt {st}")

    for st in stmts:
        run(st, binds)
    return pend


def init_constraints(spec, s0):
    cons = []

    def run(st, binds):
        tag = st[0]
        if tag == "assign":
            pend = {}
            _apply_assign(st[1], st[2], pend, s0, binds, spec)
            for _, c in _pend_to_constraints(pend, s0):
                cons.append(c)
        elif tag == "forall_stmt":
            _, binder, body, _ = st
            v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
            for i in range(lo, hi + 1):
                b2 = {**binds, v: i}
                if where is not None:
                    w = eval_expr(where, s0, b2, spec)
                    saved = []
                    for s2 in body:
                        run_collect(s2, b2, s0, saved)
                    for c in saved:
                        cons.append(z3.Implies(w, c))
                else:
                    for s2 in body:
                        run(s2, b2)
        elif tag == "if":
            _err("if in init is not supported", kind="semantics")

    def run_collect(st, binds, s0, out):
        if st[0] == "assign":
            pend = {}
            _apply_assign(st[1], st[2], pend, s0, binds, spec)
            for _, c in _pend_to_constraints(pend, s0):
                out.append(c)
        elif st[0] == "forall_stmt":
            _, binder, body, _ = st
            v, lo, hi, _where = binder_range(binder, spec["consts"], spec["types"])
            for i in range(lo, hi + 1):
                b2 = {**binds, v: i}
                for s2 in body:
                    run_collect(s2, b2, s0, out)

    for st in spec["init"]:
        run(st, {})
    return cons


def _pend_to_constraints(pend, s0):
    out = []
    for phys, val in pend.items():
        out.append((phys, s0[phys] == val))
    return out


def _init_pend_from_assign(st, s0, binds, spec):
    pend = {}
    _apply_assign(st[1], st[2], pend, s0, binds, spec)
    return _pend_to_constraints(pend, s0)


def build_instances(spec):
    instances = []
    for act in spec["actions"]:
        names = [p[0] for p in act["params"]]
        ranges = [range(p[1], p[2] + 1) for p in act["params"]]
        combos = itertools.product(*ranges) if ranges else [()]
        for combo in combos:
            instances.append({
                "action": act["name"],
                "action_def": act,
                "binds": dict(zip(names, combo)),
                "requires": act["requires"],
                "lets": act["lets"],
                "stmts": act["stmts"],
                "ensures": act["ensures"],
            })
    return instances


def _eval_requires(requires, lets, state, param_binds, spec):
    binds = dict(param_binds)
    for let in lets:
        binds[let["name"]] = eval_expr(let["expr"], state, binds, spec)
    guards = []
    for req in requires:
        b = dict(binds)
        guards.append(eval_expr(req["expr"], state, b, spec))
        for k, v in b.items():
            if k not in param_binds:
                binds[k] = v
    return guards, binds


def transition(spec, instances, cur, nxt, choice, expr_cache=None):
    clauses = []
    with _eval_cache_scope(expr_cache, id(cur)):
        for idx, inst in enumerate(instances):
            guards, binds = _eval_requires(inst["requires"], inst["lets"], cur, inst["binds"], spec)
            pend = compute_updates(inst["stmts"], cur, binds, spec)
            frame = []
            for p in spec["phys_vars"]:
                phys = p["phys"]
                frame.append(nxt[phys] == pend.get(phys, cur[phys]))
            clauses.append(z3.Implies(choice == idx, z3.And(*guards, *frame)))
    return z3.And(*clauses)


def _enum_name(spec, ename, val):
    members = spec["types"][ename]["members"]
    i = int(val)
    if 0 <= i < len(members):
        return members[i]
    return str(val)


def _display_value(ty, val, spec):
    if ty[0] == "bool":
        return val
    if ty[0] in ("int", "domain"):
        return val
    if ty[0] == "enum":
        ename = ty[1]
        return _enum_name(spec, ename, val)
    return val


def _py_val(model, expr):
    v = model.eval(expr, model_completion=True)
    if z3.is_int_value(v):
        return v.as_long()
    if z3.is_true(v):
        return True
    if z3.is_false(v):
        return False
    return str(v)


def _map_domain(kty, spec):
    if kty[0] == "int":
        mx = max(spec["consts"].values()) if spec["consts"] else 1
        return range(0, mx + 1)
    lo, hi = domain_range(kty, spec["types"])
    return range(lo, hi + 1)


def logical_state_values(model, state, spec):
    out = {}
    for n, ty in spec["state"].items():
        out[n] = _logical_val(model, state, n, ty, spec)
    return out


def _logical_val(model, state, name, ty, spec):
    if ty[0] in ("int", "domain", "enum"):
        phys = name
        for p in spec["phys_vars"]:
            if p["logical"] == name and "part" not in p:
                phys = p["phys"]
                break
        raw = _py_val(model, state[phys])
        return _display_value(ty, raw, spec)
    if ty[0] == "bool":
        return _py_val(model, state[name])
    if ty[0] == "set":
        elem_ty = ty[1]
        m = state[name]
        elems = []
        for i in _map_domain(elem_ty, spec):
            if _py_val(model, z3.Select(m, z3.IntVal(i))):
                elems.append(_display_value(elem_ty, i, spec))
        return sorted(elems, key=str)
    if ty[0] == "option":
        present = _py_val(model, state[f"{name}__present"])
        if not present:
            return None
        inner = ty[1]
        raw = _py_val(model, state[f"{name}__value"])
        return _display_value(inner, raw, spec)
    if ty[0] == "struct":
        sname = ty[1]
        obj = {}
        for fn, fty in spec["types"][sname]["fields"].items():
            obj[fn] = _display_value(fty, _py_val(model, state[f"{name}__{fn}"]), spec)
        return obj
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        mout = {}
        for i in _map_domain(kty, spec):
            key = str(_display_value(kty, i, spec) if kty[0] == "enum" else i)
            if vty[0] == "option":
                pres = _py_val(model, z3.Select(state[f"{name}__present"], z3.IntVal(i)))
                if not pres:
                    mout[key] = None
                else:
                    raw = _py_val(model, z3.Select(state[f"{name}__value"], z3.IntVal(i)))
                    mout[key] = _display_value(vty[1], raw, spec)
            elif vty[0] == "struct":
                sname = vty[1]
                obj = {}
                for fn, fty in spec["types"][sname]["fields"].items():
                    raw = _py_val(model, z3.Select(state[f"{name}__{fn}"], z3.IntVal(i)))
                    obj[fn] = _display_value(fty, raw, spec)
                mout[key] = obj
            else:
                raw = _py_val(model, z3.Select(state[name], z3.IntVal(i)))
                mout[key] = _display_value(vty, raw, spec)
        return mout
    return None


def compute_changes(prev, curr):
    changes = {}

    def walk(path, a, b):
        if a == b:
            return
        if isinstance(a, dict) and isinstance(b, dict):
            keys = set(a) | set(b)
            for k in keys:
                walk(f"{path}[{k}]" if path else k, a.get(k), b.get(k))
            return
        if path:
            changes[path] = {"from": a, "to": b}

    for k in set(prev) | set(curr):
        pa, pb = prev.get(k), curr.get(k)
        if isinstance(pa, dict) and isinstance(pb, dict) and pa and pb and \
           all(isinstance(v, dict) for v in pa.values()) and \
           all(isinstance(v, dict) for v in pb.values()):
            for sk in set(pa) | set(pb):
                walk(f"{k}[{sk}]", pa.get(sk), pb.get(sk))
        elif isinstance(pa, dict) and isinstance(pb, dict):
            for sk in set(pa) | set(pb):
                sub_a, sub_b = pa.get(sk), pb.get(sk)
                if isinstance(sub_a, dict) and isinstance(sub_b, dict):
                    for fn in set(sub_a) | set(sub_b):
                        if sub_a.get(fn) != sub_b.get(fn):
                            changes[f"{k}[{sk}].{fn}"] = {"from": sub_a.get(fn), "to": sub_b.get(fn)}
                elif sub_a != sub_b:
                    changes[f"{k}[{sk}]"] = {"from": sub_a, "to": sub_b}
        elif pa != pb:
            changes[k] = {"from": pa, "to": pb}
    return changes


def _binder_static_type(binder, spec):
    if binder[0] == "binder_typed":
        ty_name = binder[2]
        if ty_name in spec["types"]:
            return spec["types"][ty_name]["ty"]
    return ("int",)


def _expr_static_type(e, spec, env):
    tag = e[0]
    if tag == "num":
        return ("int",)
    if tag == "bool":
        return ("bool",)
    if tag == "var":
        n = e[1]
        if n in env:
            return env[n]
        if n in spec["state"]:
            return spec["state"][n]
        for name, info in spec["types"].items():
            if info["kind"] == "enum" and n in info["members"]:
                return ("enum", name)
        return None
    if tag == "some":
        inner = _expr_static_type(e[1], spec, env)
        return ("option", inner or ("int",))
    if tag == "index":
        base = e[1]
        if isinstance(base, str):
            base_ty = spec["state"].get(base) or env.get(base)
        elif base[0] == "var":
            base_ty = spec["state"].get(base[1]) or env.get(base[1])
        else:
            base_ty = _expr_static_type(base, spec, env)
        if base_ty and base_ty[0] == "map":
            return base_ty[2]
        return None
    if tag == "field":
        base_ty = _expr_static_type(e[1], spec, env)
        if base_ty and base_ty[0] == "struct":
            return spec["types"][base_ty[1]]["fields"].get(e[2])
        return None
    if tag == "method":
        method = e[2]
        base_ty = _expr_static_type(e[1], spec, env)
        if method in ("contains", "size"):
            return ("bool",) if method == "contains" else ("int",)
        if method in ("add", "remove"):
            return base_ty
        return None
    if tag == "bin":
        if e[1] in ("+", "-", "*"):
            return ("int",)
        return ("bool",)
    if tag in ("not", "is", "forall", "exists"):
        return ("bool",)
    if tag in ("count", "sum", "min", "max", "abs"):
        return ("int",)
    return None


def _collect_pattern_binding_types(e, spec, env, out):
    tag = e[0]
    if tag in ("num", "bool", "none", "var"):
        return
    if tag == "is":
        inner, pat = e[1], e[2]
        if pat[0] == "pat_some":
            inner_ty = _expr_static_type(inner, spec, env)
            if inner_ty and inner_ty[0] == "option":
                out[pat[1]] = inner_ty[1]
        _collect_pattern_binding_types(inner, spec, env, out)
        return
    if tag in ("forall", "exists"):
        binder, body = e[1], e[2]
        if binder[0] == "binder_typed":
            env = {**env, binder[1]: _binder_static_type(binder, spec)}
            where = binder[3]
            if where is not None:
                _collect_pattern_binding_types(where, spec, env, out)
        _collect_pattern_binding_types(body, spec, env, out)
        return
    for child in e[1:]:
        if isinstance(child, tuple):
            _collect_pattern_binding_types(child, spec, env, out)
        elif isinstance(child, dict):
            for sub in child.values():
                if isinstance(sub, tuple):
                    _collect_pattern_binding_types(sub, spec, env, out)
        elif isinstance(child, list):
            for sub in child:
                if isinstance(sub, tuple):
                    _collect_pattern_binding_types(sub, spec, env, out)


def violating_bindings(model, inv_expr, state, spec):
    binding_types = {}
    _collect_pattern_binding_types(inv_expr, spec, {}, binding_types)

    def search(e, binds, env):
        if e[0] in ("forall", "exists"):
            binder, body = e[1], e[2]
            v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
            bty = _binder_static_type(binder, spec)
            env = {**env, v: bty}
            binding_types[v] = bty
            bad = []
            for i in range(lo, hi + 1):
                b2 = {**binds, v: i}
                if where is not None:
                    w = eval_expr(where, state, b2, spec)
                    if z3.is_false(model.eval(w, model_completion=True)):
                        continue
                inst = eval_expr(body, state, b2, spec)
                if z3.is_false(model.eval(inst, model_completion=True)):
                    bad.append(_public_model_bindings(model, {**binds, v: i}, spec, binding_types))
            return bad if bad else None
        if e[0] == "bin" and e[1] == "and":
            left = search(e[2], binds, env)
            if left:
                return left
            return search(e[3], binds, env)
        inst = eval_expr(e, state, binds, spec)
        if z3.is_false(model.eval(inst, model_completion=True)):
            return [_public_model_bindings(model, dict(binds), spec, binding_types)] if binds else [{}]
        return None

    return search(inv_expr, {}, {})


def _display_max(spec):
    mx = 1
    for info in spec["types"].values():
        if info["kind"] == "domain":
            mx = max(mx, info["hi"])
        elif info["kind"] == "enum":
            mx = max(mx, len(info["members"]) - 1)
    mx = max(mx, max(spec["consts"].values(), default=0))
    return mx


def _build_trace(model, states, choices, instances, spec, upto):
    trace = []
    prev_logical = None
    for k in range(upto + 1):
        logical = logical_state_values(model, states[k], spec)
        entry = {"step": k, "state": logical}
        if k > 0:
            idx = model.eval(choices[k - 1], model_completion=True).as_long()
            inst = instances[idx]
            act = inst["action_def"]
            entry["action"] = {
                "name": inst["action"],
                "params": {pk: _display_param(pk, pv, act, spec)
                           for pk, pv in inst["binds"].items()},
            }
            if act.get("loc"):
                entry["action"]["loc"] = act["loc"]
            if prev_logical is not None:
                entry["changes"] = compute_changes(prev_logical, logical)
        trace.append(entry)
        prev_logical = logical
    return trace


def _display_param(name, val, act, spec):
    for p in act["params"]:
        if p[0] == name and p[3]:
            ty = spec["types"][p[3]]["ty"]
            return _display_value(ty, val, spec)
    return val


def _violation_kind(inv):
    if inv.get("implicit"):
        return "type_bound"
    return "invariant"


def _last_action(model, choices, instances, step, spec):
    if step <= 0:
        return None
    idx = model.eval(choices[step - 1], model_completion=True).as_long()
    inst = instances[idx]
    act = inst["action_def"]
    la = {
        "name": inst["action"],
        "params": {pk: _display_param(pk, pv, act, spec) for pk, pv in inst["binds"].items()},
    }
    if act.get("loc"):
        la["loc"] = act["loc"]
    return la


_COVERAGE_HINT = (
    "these requires clauses are unsatisfiable at every step up to depth K; "
    "weaken one of them, add an action that establishes them, or increase --depth"
)

_SCENARIOS_CONVENTION = (
    "set up initial_state, invoke each step as an API call, and after step i "
    "assert only the fields mentioned in expected_states[i]"
)


def _requires_loc_key(req):
    loc = req.get("loc") or {}
    return (loc.get("line"), loc.get("column"))


def _requires_blocking_entry(req, source_lines=None):
    entry = {}
    if req.get("loc"):
        entry["loc"] = req["loc"]
        if source_lines:
            line = req["loc"].get("line")
            if line and 1 <= line <= len(source_lines):
                text = source_lines[line - 1].strip()
                if text:
                    entry["text"] = text
    return entry


def _display_bindings(binds, inst, spec):
    act = inst["action_def"]
    return {pk: _display_param(pk, pv, act, spec) for pk, pv in binds.items()}


def _diagnose_action_coverage(s, aname, instance_idxs, instances, states, depth, spec, expr_cache,
                              source_lines=None):
    """At depth K, find blocking requires for an uncovered action via unsat core."""
    t = depth
    instance_cores = []
    instance_bindings = []

    for idx in instance_idxs:
        inst = instances[idx]
        with _eval_cache_scope(expr_cache, id(states[t])):
            _, binds = _eval_requires(inst["requires"], inst["lets"], states[t], inst["binds"], spec)
        if not inst["requires"]:
            continue

        s.push()
        assumptions = []
        lit_map = {}
        for j, req in enumerate(inst["requires"]):
            lit = z3.Bool(f"__cov_{aname}_{idx}_{j}")
            with _eval_cache_scope(expr_cache, id(states[t])):
                b = dict(binds)
                guard = eval_expr(req["expr"], states[t], b, spec)
            s.assert_and_track(guard, lit)
            assumptions.append(lit)
            lit_map[lit] = req

        blocking = []
        if assumptions and s.check(*assumptions) == z3.unsat:
            for c in s.unsat_core():
                if c in lit_map:
                    blocking.append(lit_map[c])
        s.pop()

        if blocking:
            instance_cores.append(blocking)
            instance_bindings.append((inst, binds))

    if not instance_cores:
        return {
            "covered": False,
            "blocking_requires": [],
            "hint": _COVERAGE_HINT.replace("K", str(depth)),
        }

    common_keys = {_requires_loc_key(r) for r in instance_cores[0]}
    for core in instance_cores[1:]:
        keys = {_requires_loc_key(r) for r in core}
        common_keys &= keys

    if common_keys:
        chosen_core = [r for r in instance_cores[0] if _requires_loc_key(r) in common_keys]
        chosen_inst = instance_bindings[0][0]
        chosen_binds = instance_bindings[0][1]
        use_bindings = False
    else:
        chosen_core = instance_cores[0]
        chosen_inst = instance_bindings[0][0]
        chosen_binds = instance_bindings[0][1]
        use_bindings = len(instance_cores) > 1

    out = {
        "covered": False,
        "blocking_requires": [
            _requires_blocking_entry(r, source_lines) for r in chosen_core
        ],
        "hint": _COVERAGE_HINT.replace("K", str(depth)),
    }
    if use_bindings:
        out["bindings"] = _display_bindings(chosen_binds, chosen_inst, spec)
    return out


def _finalize_action_coverage(coverage, s, instances, by_action, states, depth, spec, expr_cache,
                              source_lines=None):
    out = {}
    for aname, fired in coverage.items():
        if fired:
            out[aname] = True
        else:
            out[aname] = _diagnose_action_coverage(
                s, aname, by_action[aname], instances, states, depth, spec, expr_cache,
                source_lines=source_lines,
            )
    return out


def _trace_to_scenario_steps(trace):
    steps = []
    expected_states = []
    for entry in trace[1:]:
        if "action" in entry:
            steps.append({
                "action": entry["action"]["name"],
                "params": dict(entry["action"]["params"]),
            })
        expected_states.append(entry["state"])
    return steps, expected_states


def _build_cover_trace(s, states, choices, instances, spec, step, idx, expr_cache):
    if step >= len(choices):
        return None
    s.push()
    s.add(choices[step] == idx)
    if s.check() == z3.sat:
        m = s.model()
        trace = _build_trace(m, states, choices, instances, spec, step + 1)
        s.pop()
        return trace
    s.pop()
    return None


def _bmc_explore(spec, depth, deadlock_mode="warn", track_cover=False):
    instances = build_instances(spec)
    expr_cache = {}
    states = [make_state(spec, 0)]
    choices = []
    s = z3.Solver()
    s.set(unsat_core=True)
    inv_s = z3.Solver()
    with _eval_cache_scope(expr_cache, id(states[0])):
        init_cons = init_constraints(spec, states[0])
    s.add(*init_cons)
    inv_s.add(*init_cons)

    if s.check() != z3.sat:
        return {
            "result": "error",
            "kind": "vacuous",
            "message": "init constraints are unsatisfiable — the spec has no initial state",
        }

    reachables_result = {}
    pending_reachables = list(spec["reachables"])

    by_action = {}
    for idx, inst in enumerate(instances):
        by_action.setdefault(inst["action"], []).append(idx)
    coverage = {aname: False for aname in by_action}
    coverage_pending = set(by_action)
    cover_info = {}

    deadlock_info = {"found": False}
    deadlock_violation = None
    dl_warn = []

    for t in range(depth + 1):
        passed_invariants = []
        for inv in spec["invariants"]:
            with _eval_cache_scope(expr_cache, id(states[t])):
                inv_cond = eval_expr(inv["expr"], states[t], {}, spec)
            inv_s.push()
            inv_s.add(z3.Not(inv_cond))
            if inv_s.check() == z3.sat:
                m = inv_s.model()
                trace = _build_trace(m, states, choices, instances, spec, t)
                return {
                    "result": "violated",
                    "spec": spec["name"],
                    "violation_kind": _violation_kind(inv),
                    "invariant": inv["name"],
                    "loc": inv.get("loc"),
                    "violated_at_step": t,
                    "violating_bindings": violating_bindings(m, inv["expr"], states[t], spec),
                    "last_action": _last_action(m, choices, instances, t, spec),
                    "trace": trace,
                }
            inv_s.pop()
            passed_invariants.append(inv_cond)
        if passed_invariants:
            inv_s.add(*passed_invariants)

        if t > 0:
            for idx, inst in enumerate(instances):
                for ens in inst["ensures"]:
                    with _eval_cache_scope(expr_cache, id(states[t - 1])):
                        guards, binds = _eval_requires(
                            inst["requires"], inst["lets"], states[t - 1], inst["binds"], spec)
                    cond = eval_expr(
                        ens["expr"], states[t], binds, spec,
                        old_state=states[t - 1], in_ensures=True)
                    s.push()
                    s.add(choices[t - 1] == idx)
                    if guards:
                        s.add(z3.And(*guards))
                    s.add(z3.Not(cond))
                    if s.check() == z3.sat:
                        m = s.model()
                        trace = _build_trace(m, states, choices, instances, spec, t)
                        s.pop()
                        return {
                            "result": "violated",
                            "spec": spec["name"],
                            "violation_kind": "ensures",
                            "invariant": inst["action"],
                            "loc": ens.get("loc"),
                            "violated_at_step": t,
                            "violating_bindings": None,
                            "last_action": _last_action(m, choices, instances, t, spec),
                            "trace": trace,
                        }
                    s.pop()

        if pending_reachables:
            still_pending = []
            for reach in pending_reachables:
                with _eval_cache_scope(expr_cache, id(states[t])):
                    prop = eval_expr(reach["expr"], states[t], {}, spec)
                s.push()
                s.add(prop)
                if s.check() == z3.sat:
                    m = s.model()
                    witness_trace = _build_trace(m, states, choices, instances, spec, t)
                    reachables_result[reach["name"]] = {
                        "witnessed_at_step": t,
                        "witness": witness_trace,
                    }
                else:
                    still_pending.append(reach)
                s.pop()
            pending_reachables = still_pending

        if deadlock_mode != "ignore" and not deadlock_info.get("found"):
            enabled = []
            with _eval_cache_scope(expr_cache, id(states[t])):
                for inst in instances:
                    guards, _ = _eval_requires(inst["requires"], inst["lets"], states[t], inst["binds"], spec)
                    enabled.append(z3.And(*guards) if guards else z3.BoolVal(True))
            if enabled:
                s.push()
                s.add(z3.Not(z3.Or(*enabled)))
                if s.check() == z3.sat:
                    m = s.model()
                    dl_trace = _build_trace(m, states, choices, instances, spec, t)
                    deadlock_info = {"found": True, "at_step": t, "trace": dl_trace}
                    if deadlock_mode == "error":
                        deadlock_violation = {
                            "result": "violated",
                            "spec": spec["name"],
                            "violation_kind": "deadlock",
                            "invariant": "deadlock",
                            "loc": None,
                            "violated_at_step": t,
                            "violating_bindings": None,
                            "last_action": _last_action(m, choices, instances, t, spec) if t > 0 else None,
                            "trace": dl_trace,
                        }
                    else:
                        dl_warn.append(_warn(
                            f"deadlock reachable at step {t}",
                            "add an enabled action or use --deadlock=ignore if intentional",
                        ))
                s.pop()

        if coverage_pending:
            done = []
            for aname in list(coverage_pending):
                for idx in by_action[aname]:
                    inst = instances[idx]
                    with _eval_cache_scope(expr_cache, id(states[t])):
                        guards, _ = _eval_requires(
                            inst["requires"], inst["lets"], states[t], inst["binds"], spec)
                    s.push()
                    if guards:
                        s.add(z3.And(*guards))
                    enabled = s.check() == z3.sat
                    s.pop()
                    if enabled:
                        coverage[aname] = True
                        if track_cover and aname not in cover_info:
                            cover_info[aname] = {"step": t, "idx": idx}
                        done.append(aname)
                        break
            for aname in done:
                coverage_pending.discard(aname)

        if t < depth:
            nxt = make_state(spec, t + 1)
            ch = z3.Int(f"__choice@{t}")
            s.add(ch >= 0, ch < len(instances))
            step_transition = transition(spec, instances, states[t], nxt, ch, expr_cache)
            s.add(step_transition)
            inv_s.add(ch >= 0, ch < len(instances))
            inv_s.add(step_transition)
            states.append(nxt)
            choices.append(ch)

    return {
        "result": "explored",
        "spec": spec["name"],
        "depth": depth,
        "instances": instances,
        "states": states,
        "choices": choices,
        "solver": s,
        "expr_cache": expr_cache,
        "by_action": by_action,
        "coverage": coverage,
        "reachables_result": reachables_result,
        "pending_reachables": pending_reachables,
        "deadlock_info": deadlock_info,
        "deadlock_violation": deadlock_violation,
        "dl_warn": dl_warn,
        "cover_info": cover_info,
    }


def verify(spec, depth, deadlock_mode="warn", source_lines=None):
    explored = _bmc_explore(spec, depth, deadlock_mode=deadlock_mode)
    if explored["result"] != "explored":
        return explored

    depth = explored["depth"]
    coverage = explored["coverage"]
    pending_reachables = explored["pending_reachables"]
    deadlock_violation = explored["deadlock_violation"]
    deadlock_info = explored["deadlock_info"]
    dl_warn = explored["dl_warn"]
    reachables_result = explored["reachables_result"]

    coverage = _finalize_action_coverage(
        coverage, explored["solver"], explored["instances"], explored["by_action"],
        explored["states"], depth, spec, explored["expr_cache"], source_lines=source_lines,
    )

    unreached = [{"name": reach["name"], "loc": reach.get("loc")} for reach in pending_reachables]

    if unreached:
        return {
            "result": "reachable_failed",
            "spec": explored["spec"],
            "unreached": unreached,
            "depth": depth,
            "action_coverage": coverage,
            "hint": "within depth {} no trace satisfies the property; guards may be too strong (see action_coverage), or increase --depth".format(depth),
        }

    if deadlock_violation is not None:
        return deadlock_violation

    warnings = [_warn(w["message"], w.get("hint")) if isinstance(w, dict) and "message" in w
                else _warn(str(w)) for w in spec.get("warnings", [])]
    warnings.extend(dl_warn)
    for aname, cov in coverage.items():
        if cov is not True:
            hint = cov.get("hint", "review requires clauses and init")
            warnings.append(_warn(
                f"action '{aname}' is never enabled within depth {depth} — "
                f"the spec may be vacuous (check its requires clauses)",
                hint,
            ))

    return {
        "result": "verified",
        "spec": explored["spec"],
        "depth": depth,
        "invariants_checked": [i["name"] for i in spec["invariants"]],
        "reachables": reachables_result,
        "action_coverage": coverage,
        "deadlock": deadlock_info,
        "warnings": warnings,
        "note": f"bounded verification: no violation within depth {depth}",
    }


def scenarios(spec, depth, deadlock_mode="warn", source_lines=None):
    explored = _bmc_explore(spec, depth, deadlock_mode=deadlock_mode, track_cover=True)
    if explored["result"] != "explored":
        return explored

    depth = explored["depth"]
    s = explored["solver"]
    instances = explored["instances"]
    states = explored["states"]
    choices = explored["choices"]
    expr_cache = explored["expr_cache"]
    coverage = explored["coverage"]
    cover_info = explored["cover_info"]
    reachables_result = explored["reachables_result"]
    pending_reachables = explored["pending_reachables"]
    deadlock_violation = explored["deadlock_violation"]
    deadlock_info = explored["deadlock_info"]

    coverage_diag = _finalize_action_coverage(
        coverage, s, instances, explored["by_action"], states, depth, spec, expr_cache,
        source_lines=source_lines,
    )

    if pending_reachables:
        return {
            "result": "reachable_failed",
            "spec": explored["spec"],
            "unreached": [{"name": r["name"], "loc": r.get("loc")} for r in pending_reachables],
            "depth": depth,
            "action_coverage": coverage_diag,
            "hint": "within depth {} no trace satisfies the property; guards may be too strong (see action_coverage), or increase --depth".format(depth),
        }

    if deadlock_violation is not None:
        return deadlock_violation

    scenario_list = []

    for rname, rdata in reachables_result.items():
        trace = rdata["witness"]
        steps, expected_states = _trace_to_scenario_steps(trace)
        scenario_list.append({
            "name": f"reach_{rname}",
            "kind": "reachable",
            "property": rname,
            "steps": steps,
            "initial_state": trace[0]["state"],
            "expected_states": expected_states,
            "final_check": rname,
        })

    warnings = []
    for aname, cov in coverage_diag.items():
        if cov is True:
            info = cover_info.get(aname)
            if info is None:
                continue
            trace = _build_cover_trace(
                s, states, choices, instances, spec, info["step"], info["idx"], expr_cache)
            if trace is None:
                warnings.append(_warn(
                    f"action '{aname}' was enabled at step {info['step']} but no cover trace "
                    f"could be built within depth {depth}",
                ))
                continue
            steps, expected_states = _trace_to_scenario_steps(trace)
            scenario_list.append({
                "name": f"cover_{aname}",
                "kind": "action_coverage",
                "action": aname,
                "steps": steps,
                "initial_state": trace[0]["state"],
                "expected_states": expected_states,
            })
        else:
            br = cov.get("blocking_requires") or []
            locs = ", ".join(
                f"line {e['loc']['line']}" for e in br if e.get("loc", {}).get("line")
            )
            detail = f" ({locs})" if locs else ""
            warnings.append(_warn(
                f"no cover scenario for action '{aname}': never enabled within depth {depth}{detail}",
                cov.get("hint"),
            ))

    if deadlock_info.get("found"):
        trace = deadlock_info["trace"]
        steps, expected_states = _trace_to_scenario_steps(trace)
        scenario_list.append({
            "name": "deadlock_terminal",
            "kind": "deadlock",
            "steps": steps,
            "initial_state": trace[0]["state"],
            "expected_states": expected_states,
            "note": "after these steps no action is enabled",
        })

    return {
        "result": "scenarios",
        "spec": explored["spec"],
        "depth": depth,
        "convention": _SCENARIOS_CONVENTION,
        "scenarios": scenario_list,
        "warnings": warnings,
    }


def prove(spec, k_ind, base_depth, deadlock_mode="warn"):
    """k-induction: base BMC then step-case invariant proof."""
    base = verify(spec, base_depth, deadlock_mode=deadlock_mode)
    if base["result"] in ("violated", "reachable_failed", "error"):
        return base

    instances = build_instances(spec)
    invariants = spec["invariants"]
    expr_cache = {}

    s = z3.Solver()
    states = []
    choices = []
    k_used = {}
    remaining = list(invariants)
    last_cti = None

    for k in range(1, k_ind + 1):
        if k == 1:
            states = [make_ind_state(spec, 0), make_ind_state(spec, 1)]
            ch = z3.Int("__ind_choice@0")
            s.add(ch >= 0, ch < len(instances))
            s.add(*_enum_phys_constraints(spec, states[0]))
            for inv in invariants:
                s.add(_inv_constraint(inv, states[0], spec, expr_cache))
            with _eval_cache_scope(expr_cache, id(states[0])):
                s.add(transition(spec, instances, states[0], states[1], ch, expr_cache))
            choices = [ch]
        else:
            nxt = make_ind_state(spec, k)
            prev = states[k - 1]
            ch = z3.Int(f"__ind_choice@{k - 1}")
            s.add(ch >= 0, ch < len(instances))
            s.add(*_enum_phys_constraints(spec, prev))
            for inv in invariants:
                s.add(_inv_constraint(inv, prev, spec, expr_cache))
            with _eval_cache_scope(expr_cache, id(prev)):
                s.add(transition(spec, instances, prev, nxt, ch, expr_cache))
            states.append(nxt)
            choices.append(ch)

        still_remaining = []
        for inv in remaining:
            inv_cond = _inv_constraint(inv, states[k], spec, expr_cache)
            s.push()
            s.add(z3.Not(inv_cond))
            if s.check() == z3.sat:
                still_remaining.append(inv)
                last_cti = (inv, k, s.model())
            else:
                k_used[inv["name"]] = k
            s.pop()

        remaining = still_remaining
        if not remaining:
            break

    if remaining:
        inv, k, model = last_cti
        trace = _build_trace(model, states, choices, instances, spec, k)
        return {
            "result": "unknown_cti",
            "spec": spec["name"],
            "invariant": inv["name"],
            "k": k,
            "cti": {
                "states": trace,
                "violated_at": k,
            },
            "hint": _CTI_HINT,
        }

    warnings = [
        w for w in base.get("warnings", [])
        if not (isinstance(w, dict) and "deadlock" in w.get("message", ""))
    ]

    return {
        "result": "proved",
        "spec": spec["name"],
        "engine": "induction",
        "k_used": k_used,
        "base_depth": base_depth,
        "invariants_checked": [i["name"] for i in invariants],
        "action_coverage": base["action_coverage"],
        "reachables": base["reachables"],
        "warnings": warnings,
    }
