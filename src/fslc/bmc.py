"""Bounded model checker for FSL v1."""
from __future__ import annotations

import itertools
from contextlib import contextmanager

import z3

from .model import (
    FslError,
    binder_range,
    display_keyed,
    display_label,
    domain_range,
    eval_const,
    phys_z3_sort,
    resolve_action_name,
    z3_sort,
)


def _err(msg, kind="semantics", loc=None, expected=None, hint=None):
    raise FslError(msg, kind=kind, loc=loc, expected=expected, hint=hint)


def _warn(message, hint=None):
    w = {"message": message}
    if hint:
        w["hint"] = hint
    return w


def _format_state_summary(state):
    def fmt(value):
        if isinstance(value, bool):
            return "true" if value else "false"
        if value is None:
            return "null"
        if isinstance(value, dict):
            return "{" + ", ".join(f"{k}: {fmt(v)}" for k, v in value.items()) + "}"
        if isinstance(value, list):
            return "[" + ", ".join(fmt(v) for v in value) + "]"
        return str(value)

    return ", ".join(f"{key}={fmt(value)}" for key, value in state.items())


def _select_invariants(spec, property_name=None):
    invariants = spec["invariants"]
    if property_name is None:
        return invariants, None
    selected = [inv for inv in invariants if inv["name"] == property_name]
    if selected:
        return selected, None
    available = ", ".join(inv["name"] for inv in invariants)
    return [], {
        "result": "error",
        "kind": "usage",
        "message": f"no such invariant: {property_name} (available: {available})",
    }


_VACUOUS_IMPLICATION_HINT = (
    "the antecedent is not reachable within this depth; check whether an action "
    "that should establish it is missing, or whether the antecedent expression is wrong"
)
_VACUOUS_LEADSTO_HINT = (
    "the leadsTo trigger is not reachable within this depth; check whether an action "
    "that should establish it is missing, or whether the trigger expression is wrong"
)
_ALWAYS_TRUE_REQUIRES_HINT = (
    "this requires clause is not acting as a constraint within this depth; decide "
    "whether the model is missing a path to states where it matters, or the clause is redundant"
)
_TAUTOLOGY_OVER_FROZEN_HINT = (
    "make such variables 'const', or add the action that should modify them"
)


def _requirement(source):
    if not source:
        return None
    meta = source.get("meta") if isinstance(source, dict) else None
    if not meta:
        return None
    return {"id": meta["id"], "text": meta.get("text")}


def _attach_requirement(out, source):
    req = _requirement(source)
    if req is not None:
        out["requirement"] = req
    return out


def _vacuity_warning(kind, name, loc, message, hint, source, spec):
    out = {
        "kind": kind,
        "name": display_label(name, spec),
        "message": message,
        "hint": hint,
    }
    if loc:
        out["loc"] = loc
    return _attach_requirement(out, source)


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


def _pin_binds_for_cache(binds):
    return tuple((k, v) for k, v in sorted(binds.items()))


def _cache_binds_match(cached_pins, binds):
    if len(cached_pins) != len(binds):
        return False
    missing = object()
    for k, cached_v in cached_pins:
        if binds.get(k, missing) is not cached_v:
            return False
    return True


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

_PARTIAL_OP_HINT = (
    "guard the action with requires q.size() > 0 (or bound the index)"
)
_DIV_PARTIAL_OP_HINT = "guard the division: requires y != 0"


def _partial_op_hint(site_expr):
    if isinstance(site_expr, tuple) and site_expr[0] == "bin" and site_expr[1] in ("/", "%"):
        return _DIV_PARTIAL_OP_HINT
    return _PARTIAL_OP_HINT


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
            cached = _EVAL_CACHE.get(cache_key)
            if cached is not None:
                cached_e, cached_binds, cached_result = cached
                if cached_e is e and _cache_binds_match(cached_binds, binds):
                    return cached_result
    result = _eval_expr_uncached(e, state, binds, spec, old_state, in_ensures)
    if cache_key is not None:
        _EVAL_CACHE[cache_key] = (e, _pin_binds_for_cache(binds), result)
    return result


def _eval_expr_uncached(e, state, binds, spec, old_state=None, in_ensures=False):
    consts = spec["consts"]
    tag = e[0]
    if tag == "set_bounds":
        _, name, elem_ty = e
        lo, hi = domain_range(elem_ty, spec["types"])
        k = z3.Int(f"__bounds_{name}_elem")
        return z3.ForAll([k], z3.Implies(z3.Select(state[name], k), z3.And(k >= lo, k <= hi)))
    if tag == "map_value_bounds":
        _, phys_name, value_ty = e[:3]
        key_ty = e[3] if len(e) > 3 else ("int",)

        def scalar_bounds(vty, select_expr):
            if vty[0] in ("domain", "enum"):
                lo, hi = domain_range(vty, spec["types"])
                return z3.And(select_expr >= lo, select_expr <= hi)
            return None

        def value_bounds_for(vty, phys_base):
            if vty[0] in ("domain", "enum"):
                return scalar_bounds(vty, z3.Select(state[phys_base], k))
            if vty[0] == "option":
                inner_b = scalar_bounds(vty[1], z3.Select(state[f"{phys_base}__value"], k))
                if inner_b is None:
                    return None
                return z3.Implies(z3.Select(state[f"{phys_base}__present"], k), inner_b)
            if vty[0] == "struct":
                parts = []
                for fn, fty in spec["types"][vty[1]]["fields"].items():
                    part = value_bounds_for(fty, f"{phys_base}__{fn}")
                    if part is not None:
                        parts.append(part)
                if not parts:
                    return None
                return z3.And(*parts)
            return None

        parts = []
        for i in _map_domain(key_ty, spec):
            k = _z3_domain_value(key_ty, i)
            body = value_bounds_for(value_ty, phys_name)
            if body is not None:
                parts.append(body)
        return z3.And(*parts) if parts else z3.BoolVal(True)
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
    if tag == "seq_lit":
        _err("bare Seq literal must appear in assignment (use q = Seq {} on a Seq-typed variable)")
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
                    fn: (
                        ("option_val", state[f"{n}__{fn}__present"], state[f"{n}__{fn}__value"])
                        if fty[0] == "option"
                        else state[f"{n}__{fn}"]
                    )
                    for fn, fty in spec["types"][sname]["fields"].items()
                })
            if ty[0] == "set":
                return ("set_val", state[n], ty[1])
            if ty[0] == "seq":
                return ("seq_val", state[f"{n}__data"], state[f"{n}__len"], ty[1], ty[2])
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
    if tag == "ite":
        c = eval_expr(e[1], state, binds, spec, old_state, in_ensures)
        if not isinstance(c, z3.ExprRef) or c.sort().kind() != z3.Z3_BOOL_SORT:
            _err("if condition must be Bool", kind="type")
        a = eval_expr(e[2], state, binds, spec, old_state, in_ensures)
        b = eval_expr(e[3], state, binds, spec, old_state, in_ensures)
        return _ite_value(c, a, b, spec)
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
            seq_cmp = _seq_compare(a, b, op, spec)
            if seq_cmp is not None:
                return seq_cmp
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
        if op == "/":
            return _z3_div(a, b)
        if op == "%":
            return _z3_mod(a, b)
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


def _ite_value(c, a, b, spec):
    if isinstance(a, tuple) and a[0] == "none":
        if isinstance(b, tuple) and b[0] == "none":
            return ("none",)
        if isinstance(b, tuple) and b[0] == "option_val":
            return ("option_val", z3.If(c, z3.BoolVal(False), b[1]), b[2])
        _err("if arms must have the same type", kind="type")
    if isinstance(b, tuple) and b[0] == "none":
        if isinstance(a, tuple) and a[0] == "option_val":
            return ("option_val", z3.If(c, a[1], z3.BoolVal(False)), a[2])
        _err("if arms must have the same type", kind="type")
    if isinstance(a, tuple) and a[0] == "option_val":
        if not (isinstance(b, tuple) and b[0] == "option_val"):
            _err("if arms must have the same type", kind="type")
        if a[2].sort() != b[2].sort():
            _err("if Option arms must have the same value type", kind="type")
        return ("option_val", z3.If(c, a[1], b[1]), z3.If(c, a[2], b[2]))
    if isinstance(b, tuple) and b[0] == "option_val":
        _err("if arms must have the same type", kind="type")
    if isinstance(a, tuple) and a[0] == "struct_val":
        if not (isinstance(b, tuple) and b[0] == "struct_val"):
            _err("if arms must have the same type", kind="type")
        if a[1] != b[1]:
            _err(f"if struct arms must have the same type: {a[1]} vs {b[1]}", kind="type")
        if set(a[2]) != set(b[2]):
            _err("if struct arms must have the same fields", kind="type")
        return ("struct_val", a[1], {
            fn: _ite_value(c, a[2][fn], b[2][fn], spec)
            for fn in a[2]
        })
    if isinstance(b, tuple) and b[0] == "struct_val":
        _err("if arms must have the same type", kind="type")
    if isinstance(a, z3.ExprRef) and isinstance(b, z3.ExprRef):
        if a.sort() != b.sort():
            _err("if arms must have the same type", kind="type")
        if a.sort().kind() not in (z3.Z3_BOOL_SORT, z3.Z3_INT_SORT):
            _err("if arms only support Bool, Int/domain/enum, Option, and struct values", kind="type")
        return z3.If(c, a, b)
    _err("if arms must have the same type", kind="type")


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


def _seq_compare(a, b, op, spec):
    if not (isinstance(a, tuple) and a[0] == "seq_val"
            and isinstance(b, tuple) and b[0] == "seq_val"):
        if isinstance(a, tuple) and a[0] == "seq_val":
            _err("Seq comparison requires two Seq values", kind="type")
        if isinstance(b, tuple) and b[0] == "seq_val":
            _err("Seq comparison requires two Seq values", kind="type")
        return None
    data1, len1, _, cap1 = a[1], a[2], a[3], a[4]
    data2, len2, _, cap2 = b[1], b[2], b[3], b[4]
    if cap1 != cap2:
        _err("Seq comparison between different capacities", kind="type")
    parts = [len1 == len2]
    for i in range(cap1):
        parts.append(z3.Implies(i < len1, z3.Select(data1, i) == z3.Select(data2, i)))
    res = z3.And(*parts)
    return z3.Not(res) if op == "!=" else res


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
    fields = spec["types"][sa]["fields"]
    parts = []
    for k in fa:
        fty = fields[k]
        if fty[0] == "option":
            parts.append(_option_logical_eq(fa[k], fb[k]))
        else:
            parts.append(fa[k] == fb[k])
    res = z3.And(*parts) if parts else z3.BoolVal(True)
    return z3.Not(res) if op == "!=" else res


def _option_logical_eq(a, b):
    if isinstance(a, tuple) and a[0] == "option_val":
        if isinstance(b, tuple) and b[0] == "option_val":
            return z3.And(a[1] == b[1], z3.Implies(a[1], a[2] == b[2]))
        if isinstance(b, tuple) and b[0] == "none":
            return z3.Not(a[1])
    if isinstance(b, tuple) and b[0] == "option_val":
        if isinstance(a, tuple) and a[0] == "none":
            return z3.Not(b[1])
    if isinstance(a, tuple) and a[0] == "none" and isinstance(b, tuple) and b[0] == "none":
        return z3.BoolVal(True)
    _err("struct Option field comparison requires Option values", kind="type")


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


def _z3_div(a, b):
    return z3.If(b == 0, z3.IntVal(0), a / b)


def _z3_mod(a, b):
    return z3.If(b == 0, z3.IntVal(0), a % b)


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
            fn: (
                ("option_val",
                 z3.Select(state[f"{logical}__{fn}__present"], idx),
                 z3.Select(state[f"{logical}__{fn}__value"], idx))
                if fty[0] == "option"
                else z3.Select(state[f"{logical}__{fn}"], idx)
            )
            for fn, fty in fields.items()
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


def _struct_field_ty(spec, sname, field):
    try:
        return spec["types"][sname]["fields"][field]
    except KeyError:
        _err(f"unknown field '{field}' in struct {sname}")


def _assign_option_to_phys(pend, state, present_phys, value_phys, val, none_ok=True):
    if isinstance(val, tuple) and val[0] == "option_val":
        pend[present_phys] = val[1]
        pend[value_phys] = val[2]
        return
    if none_ok and val == ("none",):
        pend[present_phys] = z3.BoolVal(False)
        return
    _err("Option assignment requires none or some(...)")


def _store_option_to_phys(pend, state, present_phys, value_phys, idx, val, none_ok=True):
    if isinstance(val, tuple) and val[0] == "option_val":
        base_p = pend.get(present_phys, state[present_phys])
        base_v = pend.get(value_phys, state[value_phys])
        pend[present_phys] = z3.Store(base_p, idx, val[1])
        pend[value_phys] = z3.Store(base_v, idx, val[2])
        return
    if none_ok and val == ("none",):
        base_p = pend.get(present_phys, state[present_phys])
        pend[present_phys] = z3.Store(base_p, idx, z3.BoolVal(False))
        return
    _err("Option map assignment requires none or some(...)")


def _assign_struct_field(pend, state, phys_base, fty, fv):
    if fty[0] == "option":
        _assign_option_to_phys(pend, state, f"{phys_base}__present", f"{phys_base}__value", fv)
    else:
        pend[phys_base] = fv


def _store_struct_field(pend, state, phys_base, idx, fty, fv):
    if fty[0] == "option":
        _store_option_to_phys(
            pend, state, f"{phys_base}__present", f"{phys_base}__value", idx, fv)
    else:
        base = pend.get(phys_base, state[phys_base])
        pend[phys_base] = z3.Store(base, idx, fv)


def _set_elem_ty(base, state, spec):
    if isinstance(base, tuple) and base[0] == "set_val":
        return base[1], base[2]
    if isinstance(base, z3.ArrayRef):
        for n, ty in spec["state"].items():
            if ty[0] == "set" and state.get(n) is base:
                return base, ty[1]
    return None


def _seq_val_parts(base):
    if isinstance(base, tuple) and base[0] == "seq_val":
        return base[1], base[2], base[3], base[4]
    return None


def _eval_set_method(m, elem_ty, method, args, state, binds, spec, old_state, in_ensures):
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
        terms = [
            z3.If(z3.Select(m, _z3_domain_value(elem_ty, i)), z3.IntVal(1), z3.IntVal(0))
            for i in _map_domain(elem_ty, spec)
        ]
        acc = z3.IntVal(0)
        for t in terms:
            acc = acc + t
        return acc
    return None


def _eval_seq_method(data, length, elem_ty, cap, method, args, state, binds, spec,
                       old_state, in_ensures):
    if method == "push":
        if len(args) != 1:
            _err("push expects 1 argument")
        e = eval_expr(args[0], state, binds, spec, old_state, in_ensures)
        return ("seq_val", z3.Store(data, length, e), length + 1, elem_ty, cap)
    if method == "pop":
        if args:
            _err("pop expects no arguments")
        new_data = data
        for i in range(cap - 1):
            new_data = z3.Store(new_data, z3.IntVal(i), z3.Select(data, i + 1))
        return ("seq_val", new_data, length - 1, elem_ty, cap)
    if method == "head":
        if args:
            _err("head expects no arguments")
        return z3.Select(data, 0)
    if method == "at":
        if len(args) != 1:
            _err("at expects 1 argument")
        idx = eval_expr(args[0], state, binds, spec, old_state, in_ensures)
        return z3.Select(data, idx)
    if method == "contains":
        if len(args) != 1:
            _err("contains expects 1 argument")
        e = eval_expr(args[0], state, binds, spec, old_state, in_ensures)
        terms = [
            z3.And(z3.IntVal(i) < length, z3.Select(data, z3.IntVal(i)) == e)
            for i in range(cap)
        ]
        return z3.Or(*terms) if terms else z3.BoolVal(False)
    if method == "size":
        if args:
            _err("size expects no arguments")
        return length
    return None


def _eval_method(base, method, args, state, binds, spec, old_state, in_ensures):
    set_parts = _set_elem_ty(base, state, spec)
    if set_parts is not None:
        res = _eval_set_method(
            set_parts[0], set_parts[1], method, args, state, binds, spec,
            old_state, in_ensures,
        )
        if res is not None:
            return res
        _err(f"unknown method '{method}' on Set")

    seq_parts = _seq_val_parts(base)
    if seq_parts is not None:
        data, length, elem_ty, cap = seq_parts
        res = _eval_seq_method(
            data, length, elem_ty, cap, method, args, state, binds, spec,
            old_state, in_ensures,
        )
        if res is not None:
            return res
        _err(f"unknown method '{method}' on Seq")

    _err("method call on value that is neither Set nor Seq")


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
        if ty[0] == "seq" and rhs[0] == "seq_lit":
            elem_ty, cap = ty[1], ty[2]
            if len(rhs[1]) > cap:
                _err(
                    f"Seq literal has {len(rhs[1])} elements but capacity is {cap}",
                    kind="type",
                )
            elem_sort = z3_sort(elem_ty, spec["types"])
            m = z3.K(elem_sort, z3.IntVal(0))
            for i, lit in enumerate(rhs[1]):
                val_i = eval_expr(lit, state, binds, spec)
                m = z3.Store(m, z3.IntVal(i), val_i)
            pend[f"{n}__data"] = m
            pend[f"{n}__len"] = z3.IntVal(len(rhs[1]))
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
        elif ty[0] == "seq":
            if isinstance(val, tuple) and val[0] == "seq_val":
                pend[f"{n}__data"] = val[1]
                pend[f"{n}__len"] = val[2]
            else:
                _err("Seq assignment requires Seq literal or Seq operation expression")
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
                    fty = _struct_field_ty(spec, sname, fn)
                    _assign_struct_field(pend, state, f"{n}__{fn}", fty, fv)
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
                    fty = _struct_field_ty(spec, sname, fn)
                    _store_struct_field(pend, state, f"{n}__{fn}", idx, fty, fv)
            else:
                _err("struct map assignment requires struct literal")
        else:
            base = pend.get(n, state[n])
            pend[n] = z3.Store(base, idx, val)
        return ("map", n, idx_e)

    if key[0] == "map_field":
        n, idx_e, field = key[1], key[2], key[3]
        idx = eval_expr(idx_e, state, binds, spec)
        ty = spec["state"][n]
        if ty[0] != "map" or ty[2][0] != "struct":
            _err(f"field assignment target '{n}.{field}' is not a struct")
        fty = _struct_field_ty(spec, ty[2][1], field)
        _store_struct_field(pend, state, f"{n}__{field}", idx, fty, val)
        return ("map_field", n, idx_e, field)

    if key[0] == "field":
        n, field = key[1], key[2]
        ty = spec["state"][n]
        if ty[0] != "struct":
            _err(f"field assignment target '{n}.{field}' is not a struct")
        fty = _struct_field_ty(spec, ty[1], field)
        _assign_struct_field(pend, state, f"{n}__{field}", fty, val)
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
            v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
            for i in range(lo, hi + 1):
                b2 = {**binds, v: i}
                if where is not None:
                    wcond = eval_expr(where, s0, b2, spec)
                    saved = []
                    for s2 in body:
                        run_collect(s2, b2, s0, saved)
                    for c in saved:
                        out.append(z3.Implies(wcond, c))
                else:
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


def _display_option_value(model, state, base, inner_ty, spec, key=None):
    if key is None:
        present = _py_val(model, state[f"{base}__present"])
        if not present:
            return None
        raw = _py_val(model, state[f"{base}__value"])
    else:
        present = _py_val(model, z3.Select(state[f"{base}__present"], key))
        if not present:
            return None
        raw = _py_val(model, z3.Select(state[f"{base}__value"], key))
    return _display_value(inner_ty, raw, spec)


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
    if kty[0] == "bool":
        return [False, True]
    if kty[0] == "int":
        mx = max(spec["consts"].values()) if spec["consts"] else 1
        return range(0, mx + 1)
    lo, hi = domain_range(kty, spec["types"])
    return range(lo, hi + 1)


def _z3_domain_value(kty, value):
    if kty[0] == "bool":
        return z3.BoolVal(bool(value))
    return z3.IntVal(value)


def _display_map_key(kty, value, spec):
    if kty[0] == "bool":
        return "true" if value else "false"
    if kty[0] == "enum":
        return str(_display_value(kty, value, spec))
    return str(value)


def _display_state_keys(logical, spec):
    dn = spec.get("display_names") or {}
    if not dn:
        return logical
    return {dn.get(k, k): v for k, v in logical.items()}


def logical_state_values(model, state, spec):
    out = {}
    for n, ty in spec["state"].items():
        out[n] = _logical_val(model, state, n, ty, spec)
    return _display_state_keys(out, spec)


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
            if _py_val(model, z3.Select(m, _z3_domain_value(elem_ty, i))):
                elems.append(_display_value(elem_ty, i, spec))
        return sorted(elems, key=str)
    if ty[0] == "seq":
        elem_ty, cap = ty[1], ty[2]
        data = state[f"{name}__data"]
        length = _py_val(model, state[f"{name}__len"])
        out = []
        for i in range(length):
            raw = _py_val(model, z3.Select(data, z3.IntVal(i)))
            out.append(_display_value(elem_ty, raw, spec))
        return out
    if ty[0] == "option":
        return _display_option_value(model, state, name, ty[1], spec)
    if ty[0] == "struct":
        sname = ty[1]
        obj = {}
        for fn, fty in spec["types"][sname]["fields"].items():
            if fty[0] == "option":
                obj[fn] = _display_option_value(model, state, f"{name}__{fn}", fty[1], spec)
            else:
                obj[fn] = _display_value(fty, _py_val(model, state[f"{name}__{fn}"]), spec)
        return obj
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        mout = {}
        for i in _map_domain(kty, spec):
            key = _display_map_key(kty, i, spec)
            zkey = _z3_domain_value(kty, i)
            if vty[0] == "option":
                mout[key] = _display_option_value(
                    model, state, name, vty[1], spec, zkey)
            elif vty[0] == "struct":
                sname = vty[1]
                obj = {}
                for fn, fty in spec["types"][sname]["fields"].items():
                    if fty[0] == "option":
                        obj[fn] = _display_option_value(
                            model, state, f"{name}__{fn}", fty[1], spec, zkey)
                    else:
                        raw = _py_val(model, z3.Select(state[f"{name}__{fn}"], zkey))
                        obj[fn] = _display_value(fty, raw, spec)
                mout[key] = obj
            else:
                raw = _py_val(model, z3.Select(state[name], zkey))
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


def _merge_ite_static_types(a, b):
    if a is None:
        return b
    if b is None:
        return a
    if a == b:
        return a
    if a[0] == "option" and b[0] == "option":
        inner_a, inner_b = a[1], b[1]
        if inner_a == ("int",):
            return b
        if inner_b == ("int",):
            return a
        return ("option", _merge_ite_static_types(inner_a, inner_b))
    if a[0] in ("int", "domain", "enum") and b[0] in ("int", "domain", "enum"):
        if a[0] == "int":
            return b
        return a
    _err(f"if arms must have the same type: {a} vs {b}", kind="type")


def _expr_static_type(e, spec, env):
    tag = e[0]
    if tag == "num":
        return ("int",)
    if tag == "bool":
        return ("bool",)
    if tag == "none":
        return ("option", ("int",))
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
    if tag == "struct_lit":
        return ("struct", e[1])
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
        if method in ("contains",):
            return ("bool",)
        if method == "size":
            return ("int",)
        if method in ("add", "remove", "push", "pop"):
            return base_ty
        if method == "head":
            if base_ty and base_ty[0] == "seq":
                return base_ty[1]
            return ("int",)
        if method == "at":
            if base_ty and base_ty[0] == "seq":
                return base_ty[1]
            return ("int",)
        return None
    if tag == "bin":
        if e[1] in ("+", "-", "*", "/", "%"):
            return ("int",)
        return ("bool",)
    if tag == "ite":
        c_ty = _expr_static_type(e[1], spec, env)
        if c_ty and c_ty != ("bool",):
            _err(f"if condition must be Bool, got {c_ty}", kind="type")
        a_ty = _expr_static_type(e[2], spec, env)
        b_ty = _expr_static_type(e[3], spec, env)
        return _merge_ite_static_types(a_ty, b_ty)
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
                "name": display_label(inst["action"], spec),
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
        "name": display_label(inst["action"], spec),
        "params": {pk: _display_param(pk, pv, act, spec) for pk, pv in inst["binds"].items()},
    }
    if act.get("loc"):
        la["loc"] = act["loc"]
    return la


_LEADSTO_HINT = (
    "P held at step {pending} but the loop from step {loop_start} can repeat forever "
    "without Q; if progress relies on some action being taken eventually, "
    "annotate it with `fair action ...`"
)

_LEADSTO_STUTTER_HINT = (
    "P held at step {pending} but execution deadlocks at step {deadlock} without Q"
)


def _phys_for_logical(spec, logical):
    for p in spec["phys_vars"]:
        if p["logical"] == logical and "part" not in p:
            return p["phys"]
    return logical


def _logical_eq_var(spec, s1, s2, name, ty):
    if ty[0] in ("int", "bool", "domain", "enum"):
        phys = _phys_for_logical(spec, name)
        return s1[phys] == s2[phys]
    if ty[0] == "option":
        pres1, pres2 = s1[f"{name}__present"], s2[f"{name}__present"]
        val1, val2 = s1[f"{name}__value"], s2[f"{name}__value"]
        inner = ty[1]
        if inner[0] in ("int", "bool", "domain", "enum"):
            val_eq = val1 == val2
        else:
            val_eq = _logical_eq_var(spec, s1, s2, f"{name}__value", inner)
        return z3.And(pres1 == pres2, z3.Implies(pres1, val_eq))
    if ty[0] == "seq":
        elem_ty, cap = ty[1], ty[2]
        len1, len2 = s1[f"{name}__len"], s2[f"{name}__len"]
        data1, data2 = s1[f"{name}__data"], s2[f"{name}__data"]
        parts = [len1 == len2]
        for idx in range(cap):
            idx_v = z3.IntVal(idx)
            in_range = idx < len1
            elem_eq = _logical_eq_scalar(elem_ty, z3.Select(data1, idx_v), z3.Select(data2, idx_v))
            parts.append(z3.Implies(in_range, elem_eq))
        return z3.And(*parts)
    if ty[0] == "set":
        elem_ty = ty[1]
        m1, m2 = s1[name], s2[name]
        parts = []
        for i in _map_domain(elem_ty, spec):
            key = _z3_domain_value(elem_ty, i)
            parts.append(z3.Select(m1, key) == z3.Select(m2, key))
        return z3.And(*parts) if parts else z3.BoolVal(True)
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        parts = []
        for i in _map_domain(kty, spec):
            key = _z3_domain_value(kty, i)
            parts.append(_logical_eq_map_value(spec, s1, s2, name, vty, key))
        return z3.And(*parts) if parts else z3.BoolVal(True)
    if ty[0] == "struct":
        sname = ty[1]
        parts = []
        for fn, fty in spec["types"][sname]["fields"].items():
            parts.append(_logical_eq_var(spec, s1, s2, f"{name}__{fn}", fty))
        return z3.And(*parts)
    return z3.BoolVal(True)


def _logical_eq_scalar(ty, v1, v2):
    return v1 == v2


def _logical_eq_map_value(spec, s1, s2, map_name, vty, key):
    if vty[0] in ("int", "bool", "domain", "enum"):
        return z3.Select(s1[map_name], key) == z3.Select(s2[map_name], key)
    if vty[0] == "option":
        pres1 = z3.Select(s1[f"{map_name}__present"], key)
        pres2 = z3.Select(s2[f"{map_name}__present"], key)
        val1 = z3.Select(s1[f"{map_name}__value"], key)
        val2 = z3.Select(s2[f"{map_name}__value"], key)
        inner = vty[1]
        if inner[0] in ("int", "bool", "domain", "enum"):
            val_eq = val1 == val2
        else:
            val_eq = _logical_eq_scalar(inner, val1, val2)
        return z3.And(pres1 == pres2, z3.Implies(pres1, val_eq))
    if vty[0] == "struct":
        sname = vty[1]
        parts = []
        for fn, fty in spec["types"][sname]["fields"].items():
            if fty[0] in ("int", "bool", "domain", "enum"):
                v1 = z3.Select(s1[f"{map_name}__{fn}"], key)
                v2 = z3.Select(s2[f"{map_name}__{fn}"], key)
                parts.append(v1 == v2)
            elif fty[0] == "option":
                pres1 = z3.Select(s1[f"{map_name}__{fn}__present"], key)
                pres2 = z3.Select(s2[f"{map_name}__{fn}__present"], key)
                val1 = z3.Select(s1[f"{map_name}__{fn}__value"], key)
                val2 = z3.Select(s2[f"{map_name}__{fn}__value"], key)
                parts.append(z3.And(pres1 == pres2, z3.Implies(pres1, val1 == val2)))
        return z3.And(*parts) if parts else z3.BoolVal(True)
    return z3.BoolVal(True)


def _logical_eq(spec, s1, s2):
    parts = [_logical_eq_var(spec, s1, s2, n, ty) for n, ty in spec["state"].items()]
    return z3.And(*parts) if parts else z3.BoolVal(True)


def _enabled_instance(inst, state, spec, extra_binds, expr_cache):
    with _eval_cache_scope(expr_cache, id(state)):
        guards, _ = _eval_requires(
            inst["requires"], inst["lets"], state, {**inst["binds"], **extra_binds}, spec)
    return z3.And(*guards) if guards else z3.BoolVal(True)


def _action_enabled_exprs(state, instances, spec, expr_cache):
    enabled = []
    with _eval_cache_scope(expr_cache, id(state)):
        for inst in instances:
            guards, _ = _eval_requires(
                inst["requires"], inst["lets"], state, inst["binds"], spec)
            enabled.append(z3.And(*guards) if guards else z3.BoolVal(True))
    return enabled


def _deadlock_from_enabled(enabled):
    if not enabled:
        return z3.BoolVal(False)
    return z3.Not(z3.Or(*enabled))


def _deadlock_at(state, instances, spec, extra_binds, expr_cache):
    enabled = []
    with _eval_cache_scope(expr_cache, id(state)):
        for inst in instances:
            enabled.append(_enabled_instance(inst, state, spec, extra_binds, expr_cache))
    return _deadlock_from_enabled(enabled)


def _fairness_ok(instances, states, choices, i, j, spec, extra_binds, expr_cache):
    fair_idxs = [idx for idx, inst in enumerate(instances) if inst["action_def"].get("fair")]
    if not fair_idxs:
        return z3.BoolVal(True)
    per_inst = []
    for idx in fair_idxs:
        disabled_somewhere = z3.Or(*[
            z3.Not(_enabled_instance(instances[idx], states[q], spec, extra_binds, expr_cache))
            for q in range(i, j)
        ])
        executed = z3.Or(*[choices[q] == idx for q in range(i, j)])
        per_inst.append(z3.Or(disabled_somewhere, executed))
    return z3.And(*per_inst)


def expand_leadsto_bindings(leadsto, spec):
    binders = leadsto["binders"]

    def expand(idx, current):
        if idx >= len(binders):
            yield dict(current)
            return
        b = binders[idx]
        v, lo, hi, where = binder_range(b, spec["consts"], spec["types"])
        for val in range(lo, hi + 1):
            b2 = {**current, v: val}
            if where is not None:
                from .runtime import eval_concrete

                if not eval_concrete(where, {}, b2, spec):
                    continue
            yield from expand(idx + 1, b2)

    yield from expand(0, {})


def _leadsto_binding_types(leadsto, spec):
    types = {}
    for b in leadsto["binders"]:
        if b[0] == "binder_typed":
            ty_name = b[2]
            if ty_name in spec["types"]:
                types[b[1]] = spec["types"][ty_name]["ty"]
    _collect_pattern_binding_types(leadsto["P"], spec, types, types)
    _collect_pattern_binding_types(leadsto["Q"], spec, types, types)
    return types


def _display_leadsto_bindings(model, binds, spec, binding_types):
    return _public_model_bindings(model, binds, spec, binding_types)


def _eval_at_state(expr, state, binds, spec, expr_cache):
    with _eval_cache_scope(expr_cache, id(state)):
        return eval_expr(expr, state, dict(binds), spec)


def _leadsto_binding_key(leadsto_name, extra_binds):
    return (leadsto_name, tuple(sorted(extra_binds.items())))


def _leadsto_binding_suffix(bindings):
    if not bindings:
        return ""
    return "_" + "_".join(f"{k}{v}" for k, v in bindings.items())


def _build_leadsto_stutter_violation(
        m, states, choices, instances, spec, leadsto, extra_binds, t, p, binding_types):
    return _attach_requirement({
        "result": "violated",
        "spec": spec["name"],
        "violation_kind": "leadsTo",
        "invariant": leadsto["name"],
        "loc": leadsto.get("loc"),
        "bindings": _display_leadsto_bindings(m, extra_binds, spec, binding_types),
        "pending_since": p,
        "stutter": True,
        "trace": _build_trace(m, states, choices, instances, spec, t),
        "hint": _LEADSTO_STUTTER_HINT.format(pending=p, deadlock=t),
    }, leadsto)


def _check_leadsto_stutter_at_step(
        s, states, choices, instances, spec, leadsto, extra_binds, t, enabled, expr_cache,
        binding_types):
    dl = _deadlock_from_enabled(enabled)
    candidates = []
    for p in range(t + 1):
        not_q = [
            z3.Not(_eval_at_state(leadsto["Q"], states[q], extra_binds, spec, expr_cache))
            for q in range(p, t + 1)
        ]
        p_hold = _eval_at_state(leadsto["P"], states[p], extra_binds, spec, expr_cache)
        candidates.append(z3.And(
            dl,
            p_hold,
            z3.And(*not_q) if not_q else z3.BoolVal(True),
        ))
    if not candidates:
        return None

    s.push()
    s.add(z3.Or(*candidates))
    if s.check() != z3.sat:
        s.pop()
        return None

    m = s.model()
    p_val = None
    for p in range(t + 1):
        not_q = [
            z3.Not(_eval_at_state(leadsto["Q"], states[q], extra_binds, spec, expr_cache))
            for q in range(p, t + 1)
        ]
        p_hold = _eval_at_state(leadsto["P"], states[p], extra_binds, spec, expr_cache)
        cond = z3.And(
            dl,
            p_hold,
            z3.And(*not_q) if not_q else z3.BoolVal(True),
        )
        if z3.is_true(m.eval(cond, model_completion=True)):
            p_val = p
            break
    s.pop()
    if p_val is None:
        return None
    return _build_leadsto_stutter_violation(
        m, states, choices, instances, spec, leadsto, extra_binds, t, p_val, binding_types)


def _check_single_leadsto(explored, spec, leadsto):
    depth = explored["depth"]
    states = explored["states"]
    choices = explored["choices"]
    instances = explored["instances"]
    s = explored["solver"]
    expr_cache = explored["expr_cache"]
    K = depth

    binding_types = _leadsto_binding_types(leadsto, spec)

    for extra_binds in expand_leadsto_bindings(leadsto, spec):
        candidates = []
        meta = []

        for i in range(K):
            for j in range(i + 1, K + 1):
                loop = _logical_eq(spec, states[i], states[j])
                for p in range(j):
                    not_q = [
                        z3.Not(_eval_at_state(leadsto["Q"], states[q], extra_binds, spec, expr_cache))
                        for q in range(min(i, p), j)
                    ]
                    p_hold = _eval_at_state(leadsto["P"], states[p], extra_binds, spec, expr_cache)
                    fair_ok = _fairness_ok(
                        instances, states, choices, i, j, spec, extra_binds, expr_cache)
                    cond = z3.And(
                        loop,
                        p_hold,
                        z3.And(*not_q) if not_q else z3.BoolVal(True),
                        fair_ok,
                    )
                    sel = z3.Bool(f"__lt_lasso_{i}_{j}_{p}")
                    candidates.append(sel)
                    meta.append(("lasso", i, j, p, cond, sel))

        if not candidates:
            continue

        s.push()
        for _, _, _, _, cond, sel in meta:
            s.add(sel == cond)
        s.add(z3.Or(*candidates))
        if s.check() == z3.sat:
            m = s.model()
            i_val = j_val = p_val = None
            for _, i_c, j_c, p_c, cond, _sel in meta:
                if z3.is_true(m.eval(cond, model_completion=True)):
                    i_val, j_val, p_val = i_c, j_c, p_c
                    break
            s.pop()

            violation = _attach_requirement({
                "result": "violated",
                "spec": spec["name"],
                "violation_kind": "leadsTo",
                "invariant": leadsto["name"],
                "loc": leadsto.get("loc"),
                "bindings": _display_leadsto_bindings(m, extra_binds, spec, binding_types),
                "pending_since": p_val,
                "loop_start": i_val,
                "stutter": False,
                "trace": _build_trace(m, states, choices, instances, spec, j_val),
                "hint": _LEADSTO_HINT.format(pending=p_val, loop_start=i_val),
            }, leadsto)
            return violation
        s.pop()

    return None


def _check_leadstos(explored, spec):
    if not spec.get("leadstos"):
        return None, None
    stutter_violation = explored.get("leadsto_stutter_violation")
    if stutter_violation is not None:
        return stutter_violation, None
    depth = explored["depth"]
    for lt in spec["leadstos"]:
        violation = _check_single_leadsto(explored, spec, lt)
        if violation is not None:
            return violation, None
    leads_to = {lt["name"]: {"checked_to_depth": depth} for lt in spec["leadstos"]}
    return None, leads_to


def _build_leadsto_response_scenarios(explored, spec):
    scenarios_out = []
    warnings = []
    if not spec.get("leadstos"):
        return scenarios_out, warnings

    depth = explored["depth"]
    states = explored["states"]
    choices = explored["choices"]
    instances = explored["instances"]
    s = explored["solver"]
    expr_cache = explored["expr_cache"]

    for leadsto in spec["leadstos"]:
        binding_types = _leadsto_binding_types(leadsto, spec)
        for extra_binds in expand_leadsto_bindings(leadsto, spec):
            display_bindings = None
            found = False
            for t in range(depth + 1):
                candidates = []
                for p in range(t + 1):
                    p_hold = _eval_at_state(
                        leadsto["P"], states[p], extra_binds, spec, expr_cache)
                    q_hold = _eval_at_state(
                        leadsto["Q"], states[t], extra_binds, spec, expr_cache)
                    not_q_before_t = [
                        z3.Not(_eval_at_state(
                            leadsto["Q"], states[q], extra_binds, spec, expr_cache))
                        for q in range(p, t)
                    ]
                    candidates.append(z3.And(
                        p_hold,
                        q_hold,
                        z3.And(*not_q_before_t) if not_q_before_t else z3.BoolVal(True),
                    ))

                s.push()
                s.add(z3.Or(*candidates))
                if s.check() == z3.sat:
                    m = s.model()
                    display_bindings = _display_leadsto_bindings(
                        m, extra_binds, spec, binding_types)
                    pending_at = None
                    for p, cond in enumerate(candidates):
                        if z3.is_true(m.eval(cond, model_completion=True)):
                            pending_at = p
                            break
                    trace = _build_trace(m, states, choices, instances, spec, t)
                    s.pop()

                    if pending_at is None:
                        pending_at = t
                    steps, expected_states = _trace_to_scenario_steps(trace)
                    suffix = _leadsto_binding_suffix(display_bindings)
                    scenario = _attach_requirement({
                        "name": f"respond_{leadsto['name']}{suffix}",
                        "kind": "leadsTo",
                        "property": leadsto["name"],
                        "bindings": display_bindings,
                        "steps": steps,
                        "pending_at": pending_at,
                        "satisfied_at": t,
                        "initial_state": trace[0]["state"],
                        "expected_states": expected_states,
                    }, leadsto)
                    scenarios_out.append(scenario)
                    found = True
                    break
                s.pop()

            if found:
                continue

            if display_bindings is None:
                display_bindings = _public_model_bindings(
                    None, extra_binds, spec, binding_types)

            p_candidates = [
                _eval_at_state(leadsto["P"], states[t], extra_binds, spec, expr_cache)
                for t in range(depth + 1)
            ]
            s.push()
            s.add(z3.Or(*p_candidates))
            p_ever_holds = s.check() == z3.sat
            if p_ever_holds:
                m = s.model()
                display_bindings = _display_leadsto_bindings(
                    m, extra_binds, spec, binding_types)
            s.pop()

            if not p_ever_holds:
                warnings.append(_warn(
                    f"leadsTo {leadsto['name']} {display_bindings}: "
                    f"P never holds within depth {depth}",
                    "the antecedent is unreachable for this binding within the bound; "
                    "check the property or increase --depth",
                ))

    return scenarios_out, warnings


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


def _exists_wrap(binders, expr):
    out = expr
    for binder in reversed(binders):
        out = ("exists", binder, out)
    return out


def _implication_antecedent_candidate(inv):
    if inv.get("implicit"):
        return None
    expr = inv["expr"]
    binders = []
    while isinstance(expr, tuple) and expr[0] == "forall":
        binders.append(expr[1])
        expr = expr[2]
    if not (isinstance(expr, tuple) and expr[0] == "bin" and expr[1] == "=>"):
        return None
    return {
        "kind": "vacuous_implication",
        "name": inv["name"],
        "source": inv,
        "loc": inv.get("loc"),
        "expr": _exists_wrap(binders, expr[2]),
    }


def _leadsto_trigger_candidate(leadsto):
    return {
        "kind": "vacuous_leadsto",
        "name": leadsto["name"],
        "source": leadsto,
        "loc": leadsto.get("loc"),
        "expr": _exists_wrap(leadsto.get("binders") or [], leadsto["P"]),
    }


def _lvalue_base_name(lv):
    tag = lv[0]
    if tag in ("var", "index"):
        return lv[1]
    if tag == "field_lv":
        base = lv[1]
        if base[0] in ("var", "index"):
            return base[1]
    return None


def _assigned_state_roots(stmts):
    assigned = set()

    def walk(stmt_list):
        for st in stmt_list:
            tag = st[0]
            if tag == "assign":
                name = _lvalue_base_name(st[1])
                if name is not None:
                    assigned.add(name)
            elif tag == "if":
                walk(st[2])
                walk(st[3])
            elif tag == "forall_stmt":
                walk(st[2])

    walk(stmts)
    return assigned


def _frozen_state_vars(spec):
    assigned = set()
    for act in spec.get("actions", []):
        assigned.update(_assigned_state_roots(act.get("stmts", [])))
    return set(spec.get("state", {})) - assigned


def _referenced_state_vars(expr, spec):
    refs = set()
    state_names = set(spec.get("state", {}))

    def walk(node):
        if isinstance(node, tuple):
            if node and node[0] == "var" and node[1] in state_names:
                refs.add(node[1])
            for part in node[1:]:
                walk(part)
        elif isinstance(node, list):
            for part in node:
                walk(part)
        elif isinstance(node, dict):
            for part in node.values():
                walk(part)

    walk(expr)
    return refs


def _init_model_for_frozen_check(spec):
    s0 = make_state(spec, "frozen_init")
    solver = z3.Solver()
    solver.add(*init_constraints(spec, s0))
    if solver.check() != z3.sat:
        return None, None
    return solver.model(), s0


def _phys_vars_for_logicals(spec, logicals):
    return [p for p in spec["phys_vars"] if p["logical"] in logicals]


def _is_tautology_over_frozen(spec, inv, frozen_refs, init_model, init_state):
    state = make_state(spec, f"frozen_taut_{inv['name']}")
    solver = z3.Solver()
    for p in _phys_vars_for_logicals(spec, frozen_refs):
        phys = p["phys"]
        solver.add(state[phys] == init_model.eval(init_state[phys], model_completion=True))
    expr_cache = {}
    for implicit in spec["invariants"]:
        if implicit.get("implicit"):
            solver.add(_inv_constraint(implicit, state, spec, expr_cache))
    with _eval_cache_scope(expr_cache, id(state)):
        solver.add(z3.Not(eval_expr(inv["expr"], state, {}, spec)))
    return solver.check() == z3.unsat


def _frozen_tautology_candidates(spec):
    frozen = _frozen_state_vars(spec)
    if not frozen:
        return []
    init_model, init_state = _init_model_for_frozen_check(spec)
    if init_model is None:
        return []

    pending = []
    for inv in spec.get("user_invariants", []):
        if inv.get("implicit"):
            continue
        refs = _referenced_state_vars(inv["expr"], spec)
        if not refs:
            continue
        frozen_refs = refs & frozen
        if not frozen_refs:
            continue
        if not _is_tautology_over_frozen(spec, inv, frozen_refs, init_model, init_state):
            continue
        pending.append({
            "kind": "tautology_over_frozen",
            "name": inv["name"],
            "source": inv,
            "loc": inv.get("loc"),
            "frozen_vars": tuple(sorted(frozen_refs)),
        })
    return pending


def _vacuity_candidates(spec):
    pending = []
    pending.extend(_frozen_tautology_candidates(spec))
    for inv in spec.get("user_invariants", []):
        cand = _implication_antecedent_candidate(inv)
        if cand is not None:
            pending.append(cand)
    for leadsto in spec.get("leadstos", []):
        pending.append(_leadsto_trigger_candidate(leadsto))
    return pending


def _requires_clause_locally_implied(inst, req_idx, spec):
    if req_idx == 0:
        return True
    state = make_ind_state(spec, f"vac_{inst['action']}_{req_idx}")
    expr_cache = {}
    s = z3.Solver()
    s.add(*_enum_phys_constraints(spec, state))
    for inv in spec["invariants"]:
        if inv.get("implicit"):
            s.add(_inv_constraint(inv, state, spec, expr_cache))
    with _eval_cache_scope(expr_cache, id(state)):
        guards, _ = _eval_requires(inst["requires"], inst["lets"], state, inst["binds"], spec)
    if req_idx >= len(guards):
        return False
    if req_idx:
        s.add(*guards[:req_idx])
    s.add(z3.Not(guards[req_idx]))
    return s.check() == z3.unsat


def _requires_vacuity_candidates(instances, spec):
    pending = {}
    suppress = {}
    for idx, inst in enumerate(instances):
        if inst["action_def"].get("sync"):
            # compose の同期アクションは対象外: 句は複数成分からの継承複製であり、
            # 成分間の同一ガード(例: bank_system の deposit_audited が bank と
            # audit の両方から `a > 0` を継承)は「各成分が自分の契約を自衛する」
            # 設計どおりで、除去可能な冗長ではない。各句は成分 spec 単体の
            # verify で正しい文脈の検査を受ける(検出損失なし)。
            continue
        aname = inst["action"]
        action_pending = pending.setdefault(aname, {})
        action_suppress = suppress.setdefault(aname, set())
        for req_idx, req in enumerate(inst["requires"]):
            key = _requires_loc_key(req) + (req_idx,)
            if not _requires_clause_locally_implied(inst, req_idx, spec):
                action_suppress.add(key)
            entry = action_pending.setdefault(key, {
                "kind": "always_true_requires",
                "name": aname,
                "source": inst["action_def"],
                "loc": req.get("loc"),
                "req_idx": req_idx,
                "instances": [],
            })
            entry["instances"].append(idx)
    for aname, keys in suppress.items():
        for key in keys:
            pending.get(aname, {}).pop(key, None)
    return {aname: by_clause for aname, by_clause in pending.items() if by_clause}


def _check_requires_clause_can_constrain(s, inst, req_idx, state, spec, expr_cache):
    with _eval_cache_scope(expr_cache, id(state)):
        guards, _ = _eval_requires(inst["requires"], inst["lets"], state, inst["binds"], spec)
    if req_idx >= len(guards):
        return False
    s.push()
    if req_idx:
        s.add(*guards[:req_idx])
    s.add(z3.Not(guards[req_idx]))
    can_constrain = s.check() == z3.sat
    s.pop()
    return can_constrain


def _finalize_vacuity_findings(pending_vacuity, pending_requires_vacuity, coverage, depth, spec):
    findings = []
    for item in pending_vacuity:
        if item["kind"] == "vacuous_implication":
            findings.append(_vacuity_warning(
                "vacuous_implication",
                item["name"],
                item.get("loc"),
                (
                    f"invariant '{display_label(item['name'], spec)}' has an implication "
                    f"antecedent that is unreachable within depth {depth}"
                ),
                _VACUOUS_IMPLICATION_HINT,
                item.get("source"),
                spec,
            ))
        elif item["kind"] == "vacuous_leadsto":
            findings.append(_vacuity_warning(
                "vacuous_leadsto",
                item["name"],
                item.get("loc"),
                (
                    f"leadsTo '{display_label(item['name'], spec)}' has a trigger "
                    f"that is unreachable within depth {depth}"
                ),
                _VACUOUS_LEADSTO_HINT,
                item.get("source"),
                spec,
            ))
        elif item["kind"] == "tautology_over_frozen":
            frozen_vars = ", ".join(item.get("frozen_vars", ()))
            findings.append(_vacuity_warning(
                "tautology_over_frozen",
                item["name"],
                item.get("loc"),
                (
                    f"invariant '{display_label(item['name'], spec)}' is a tautology "
                    f"over frozen state ({frozen_vars}): it holds for all dynamics "
                    "because every state variable it depends on is never modified by any action"
                ),
                _TAUTOLOGY_OVER_FROZEN_HINT,
                item.get("source"),
                spec,
            ))

    for aname, by_clause in pending_requires_vacuity.items():
        if coverage.get(aname) is not True:
            continue
        for item in by_clause.values():
            findings.append(_vacuity_warning(
                "always_true_requires",
                aname,
                item.get("loc"),
                (
                    f"action '{display_label(aname, spec)}' has a requires clause "
                    f"that is always true within depth {depth} when preceding clauses hold"
                ),
                _ALWAYS_TRUE_REQUIRES_HINT,
                item.get("source"),
                spec,
            ))
    return findings


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
        label = display_label(aname, spec)
        if fired:
            out[label] = True
        else:
            diag = _diagnose_action_coverage(
                s, aname, by_action[aname], instances, states, depth, spec, expr_cache,
                source_lines=source_lines,
            )
            action_def = instances[by_action[aname][0]]["action_def"]
            out[label] = _attach_requirement(diag, action_def)
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


def _display_scenario(scenario, spec):
    out = dict(scenario)
    prop = out.get("property")
    action = out.get("action")
    kind = out.get("kind")
    if prop is not None:
        out["property"] = display_label(prop, spec)
    if action is not None:
        out["action"] = display_label(action, spec)
    if out.get("final_check") is not None:
        out["final_check"] = display_label(out["final_check"], spec)
    if kind == "reachable" and prop is not None:
        out["name"] = f"reach_{display_label(prop, spec)}"
    elif kind == "action_coverage" and action is not None:
        out["name"] = f"cover_{display_label(action, spec)}"
    elif kind == "leadsTo" and prop is not None:
        out["name"] = f"respond_{display_label(prop, spec)}{_leadsto_binding_suffix(out.get('bindings') or {})}"
    return out


def _display_output(result, spec):
    if not spec.get("display_names"):
        return result
    out = dict(result)
    if "invariants_checked" in out:
        out["invariants_checked"] = [display_label(n, spec) for n in out["invariants_checked"]]
    if "reachables" in out:
        out["reachables"] = display_keyed(out["reachables"], spec)
    if "k_used" in out:
        out["k_used"] = display_keyed(out["k_used"], spec)
    if "leads_to" in out:
        out["leads_to"] = display_keyed(out["leads_to"], spec)
    if "invariant" in out:
        out["invariant"] = display_label(out["invariant"], spec)
    if "unreached" in out:
        out["unreached"] = [
            {**u, "name": display_label(u["name"], spec)} for u in out["unreached"]
        ]
    if "scenarios" in out:
        out["scenarios"] = [_display_scenario(s, spec) for s in out["scenarios"]]
    return out


def _and_path_ast(path_ast, extra):
    if path_ast is None:
        return extra
    return ("bin", "and", path_ast, extra)


def _collect_partial_op_sites(action_def):
    sites = []

    def walk_expr(e, loc, path_ast):
        if not isinstance(e, tuple):
            return
        if e[0] == "method" and e[2] in ("pop", "head", "at"):
            sites.append({"expr": e, "loc": loc, "path_ast": path_ast})
        if e[0] == "bin" and e[1] in ("/", "%"):
            sites.append({"expr": e, "loc": loc, "path_ast": path_ast})
        for child in e[1:]:
            if isinstance(child, tuple):
                walk_expr(child, loc, path_ast)
            elif isinstance(child, dict):
                for sub in child.values():
                    if isinstance(sub, tuple):
                        walk_expr(sub, loc, path_ast)
            elif isinstance(child, list):
                for sub in child:
                    if isinstance(sub, tuple):
                        walk_expr(sub, loc, path_ast)

    def walk_stmts(stmts, path_ast):
        for st in stmts:
            if st[0] == "assign":
                walk_expr(st[2], st[3] if len(st) > 3 else None, path_ast)
            elif st[0] == "if":
                _, cond, then_stmts, else_stmts, loc = st
                walk_expr(cond, loc, path_ast)
                walk_stmts(then_stmts, _and_path_ast(path_ast, cond))
                walk_stmts(else_stmts, _and_path_ast(path_ast, ("not", cond)))
            elif st[0] == "forall_stmt":
                _, _binder, body, _ = st
                walk_stmts(body, path_ast)

    for req in action_def["requires"]:
        walk_expr(req["expr"], req.get("loc"), None)
    for let in action_def["lets"]:
        walk_expr(let["expr"], let.get("loc"), None)
    walk_stmts(action_def["stmts"], None)
    for ens in action_def["ensures"]:
        walk_expr(ens["expr"], ens.get("loc"), None)
    return sites


def _partial_op_well_defined(site_expr, state, binds, spec):
    if site_expr[0] == "bin" and site_expr[1] in ("/", "%"):
        divisor = eval_expr(site_expr[3], state, binds, spec)
        return divisor != 0
    method = site_expr[2]
    base = eval_expr(site_expr[1], state, binds, spec)
    if not isinstance(base, tuple) or base[0] != "seq_val":
        return z3.BoolVal(True)
    _, _data, length, _elem_ty, _cap = base
    if method in ("pop", "head"):
        return length > 0
    if method == "at":
        idx = eval_expr(site_expr[3][0], state, binds, spec)
        return z3.And(idx >= 0, idx < length)
    return z3.BoolVal(True)


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


def _bmc_explore(
        spec, depth, deadlock_mode="warn", track_cover=False, vacuity_mode="warn",
        property_name=None):
    invariants, property_error = _select_invariants(spec, property_name)
    if property_error is not None:
        return property_error

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
    pending_vacuity = _vacuity_candidates(spec) if vacuity_mode != "ignore" else []

    by_action = {}
    for idx, inst in enumerate(instances):
        by_action.setdefault(inst["action"], []).append(idx)
    coverage = {aname: False for aname in by_action}
    coverage_pending = set(by_action)
    pending_requires_vacuity = (
        _requires_vacuity_candidates(instances, spec) if vacuity_mode != "ignore" else {}
    )
    cover_info = {}

    deadlock_info = {"found": False}
    deadlock_violation = None
    dl_warn = []
    leadsto_stutter_violation = None
    leadsto_stutter_found = set()
    leadsto_binding_types = {
        lt["name"]: _leadsto_binding_types(lt, spec) for lt in spec.get("leadstos", [])
    }

    for t in range(depth + 1):
        if t > 0:
            for idx, inst in enumerate(instances):
                act = inst["action_def"]
                sites = _collect_partial_op_sites(act)
                if not sites:
                    continue
                with _eval_cache_scope(expr_cache, id(states[t - 1])):
                    guards, binds = _eval_requires(
                        inst["requires"], inst["lets"], states[t - 1], inst["binds"], spec)
                for site in sites:
                    with _eval_cache_scope(expr_cache, id(states[t - 1])):
                        wd = _partial_op_well_defined(site["expr"], states[t - 1], binds, spec)
                        path_ast = site.get("path_ast")
                        if path_ast is not None:
                            path_cond = eval_expr(path_ast, states[t - 1], binds, spec)
                            wd_check = z3.Implies(path_cond, wd)
                        else:
                            wd_check = wd
                    s.push()
                    s.add(choices[t - 1] == idx)
                    if guards:
                        s.add(z3.And(*guards))
                    s.add(z3.Not(wd_check))
                    if s.check() == z3.sat:
                        m = s.model()
                        trace = _build_trace(m, states, choices, instances, spec, t)
                        s.pop()
                        return _attach_requirement({
                            "result": "violated",
                            "spec": spec["name"],
                            "violation_kind": "partial_op",
                            "invariant": f"_partial_{inst['action']}",
                            "loc": site.get("loc"),
                            "hint": _partial_op_hint(site["expr"]),
                            "violated_at_step": t,
                            "violating_bindings": None,
                            "last_action": _last_action(m, choices, instances, t, spec),
                            "trace": trace,
                        }, act)
                    s.pop()

        passed_invariants = []
        for inv in invariants:
            with _eval_cache_scope(expr_cache, id(states[t])):
                inv_cond = eval_expr(inv["expr"], states[t], {}, spec)
            inv_s.push()
            inv_s.add(z3.Not(inv_cond))
            if inv_s.check() == z3.sat:
                m = inv_s.model()
                trace = _build_trace(m, states, choices, instances, spec, t)
                return _attach_requirement({
                    "result": "violated",
                    "spec": spec["name"],
                    "violation_kind": _violation_kind(inv),
                    "invariant": inv["name"],
                    "loc": inv.get("loc"),
                    "violated_at_step": t,
                    "violating_bindings": violating_bindings(m, inv["expr"], states[t], spec),
                    "last_action": _last_action(m, choices, instances, t, spec),
                    "trace": trace,
                }, inv)
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
                        return _attach_requirement({
                            "result": "violated",
                            "spec": spec["name"],
                            "violation_kind": "ensures",
                            "invariant": inst["action"],
                            "loc": ens.get("loc"),
                            "violated_at_step": t,
                            "violating_bindings": None,
                            "last_action": _last_action(m, choices, instances, t, spec),
                            "trace": trace,
                        }, inst["action_def"])
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

        if pending_vacuity:
            still_pending = []
            for item in pending_vacuity:
                if item["kind"] == "tautology_over_frozen":
                    still_pending.append(item)
                    continue
                with _eval_cache_scope(expr_cache, id(states[t])):
                    prop = eval_expr(item["expr"], states[t], {}, spec)
                s.push()
                s.add(prop)
                reachable = s.check() == z3.sat
                s.pop()
                if not reachable:
                    still_pending.append(item)
            pending_vacuity = still_pending

        enabled = _action_enabled_exprs(states[t], instances, spec, expr_cache)

        if spec.get("leadstos") and leadsto_stutter_violation is None:
            for lt in spec["leadstos"]:
                binding_types = leadsto_binding_types[lt["name"]]
                for extra_binds in expand_leadsto_bindings(lt, spec):
                    key = _leadsto_binding_key(lt["name"], extra_binds)
                    if key in leadsto_stutter_found:
                        continue
                    violation = _check_leadsto_stutter_at_step(
                        s, states, choices, instances, spec, lt, extra_binds, t, enabled,
                        expr_cache, binding_types,
                    )
                    if violation is not None:
                        leadsto_stutter_found.add(key)
                        leadsto_stutter_violation = violation
                        break
                if leadsto_stutter_violation is not None:
                    break

        if deadlock_mode != "ignore" and not deadlock_info.get("found"):
            if enabled:
                s.push()
                s.add(_deadlock_from_enabled(enabled))
                term_expr = spec.get("terminal")
                if term_expr is not None:
                    with _eval_cache_scope(expr_cache, id(states[t])):
                        term_cond = eval_expr(term_expr, states[t], {}, spec)
                    s.add(z3.Not(term_cond))
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
                        state_summary = _format_state_summary(dl_trace[-1]["state"])
                        dl_warn.append(_warn(
                            f"deadlock reachable at step {t} (state: {state_summary})",
                            "add an enabled action, declare intended stops in a terminal { } "
                            "block, or use --deadlock=ignore if intentional",
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

        if pending_requires_vacuity:
            empty_actions = []
            for aname, by_clause in list(pending_requires_vacuity.items()):
                discharged = []
                for key, item in by_clause.items():
                    for idx in item["instances"]:
                        inst = instances[idx]
                        if _check_requires_clause_can_constrain(
                            s, inst, item["req_idx"], states[t], spec, expr_cache,
                        ):
                            discharged.append(key)
                            break
                for key in discharged:
                    by_clause.pop(key, None)
                if not by_clause:
                    empty_actions.append(aname)
            for aname in empty_actions:
                pending_requires_vacuity.pop(aname, None)

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
        "pending_vacuity": pending_vacuity,
        "pending_requires_vacuity": pending_requires_vacuity,
        "deadlock_info": deadlock_info,
        "deadlock_violation": deadlock_violation,
        "dl_warn": dl_warn,
        "cover_info": cover_info,
        "leadsto_stutter_violation": leadsto_stutter_violation,
        "invariants_checked": [i["name"] for i in invariants],
    }


def verify(
        spec, depth, deadlock_mode="warn", source_lines=None, vacuity_mode="warn",
        property_name=None):
    explored = _bmc_explore(
        spec,
        depth,
        deadlock_mode=deadlock_mode,
        vacuity_mode=vacuity_mode,
        property_name=property_name,
    )
    if explored["result"] != "explored":
        return _display_output(explored, spec)

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
        return _display_output({
            "result": "reachable_failed",
            "spec": explored["spec"],
            "unreached": unreached,
            "depth": depth,
            "action_coverage": coverage,
            "hint": "within depth {} no trace satisfies the property; guards may be too strong (see action_coverage), or increase --depth".format(depth),
        }, spec)

    if deadlock_violation is not None:
        return _display_output(deadlock_violation, spec)

    lt_violation, leads_to = _check_leadstos(explored, spec)
    if lt_violation is not None:
        return _display_output(lt_violation, spec)

    vacuity_findings = _finalize_vacuity_findings(
        explored.get("pending_vacuity", []),
        explored.get("pending_requires_vacuity", {}),
        explored["coverage"],
        depth,
        spec,
    ) if vacuity_mode != "ignore" else []
    if vacuity_mode == "error" and vacuity_findings:
        return _display_output({
            "result": "error",
            "spec": explored["spec"],
            "kind": vacuity_findings[0]["kind"],
            "findings": vacuity_findings,
        }, spec)

    warnings = [_warn(w["message"], w.get("hint")) if isinstance(w, dict) and "message" in w
                else _warn(str(w)) for w in spec.get("warnings", [])]
    warnings.extend(dl_warn)
    warnings.extend(vacuity_findings)
    for aname, cov in coverage.items():
        if cov is not True:
            hint = cov.get("hint", "review requires clauses and init")
            warnings.append(_warn(
                f"action '{aname}' is never enabled within depth {depth} — "
                f"the spec may be vacuous (check its requires clauses)",
                hint,
            ))

    result = {
        "result": "verified",
        "spec": explored["spec"],
        "depth": depth,
        "invariants_checked": explored["invariants_checked"],
        "reachables": reachables_result,
        "action_coverage": coverage,
        "deadlock": deadlock_info,
        "warnings": warnings,
        "note": f"bounded verification: no violation within depth {depth}",
    }
    if leads_to is not None:
        result["leads_to"] = leads_to
    return _display_output(result, spec)


def scenarios(spec, depth, deadlock_mode="warn", source_lines=None):
    explored = _bmc_explore(
        spec, depth, deadlock_mode=deadlock_mode, track_cover=True, vacuity_mode="ignore")
    if explored["result"] != "explored":
        return _display_output(explored, spec)

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
        return _display_output({
            "result": "reachable_failed",
            "spec": explored["spec"],
            "unreached": [{"name": r["name"], "loc": r.get("loc")} for r in pending_reachables],
            "depth": depth,
            "action_coverage": coverage_diag,
            "hint": "within depth {} no trace satisfies the property; guards may be too strong (see action_coverage), or increase --depth".format(depth),
        }, spec)

    if deadlock_violation is not None:
        return _display_output(deadlock_violation, spec)

    scenario_list = []
    reachable_by_name = {r["name"]: r for r in spec["reachables"]}

    for rname, rdata in reachables_result.items():
        trace = rdata["witness"]
        steps, expected_states = _trace_to_scenario_steps(trace)
        scenario = _attach_requirement({
            "name": f"reach_{rname}",
            "kind": "reachable",
            "property": rname,
            "steps": steps,
            "initial_state": trace[0]["state"],
            "expected_states": expected_states,
            "final_check": rname,
        }, reachable_by_name.get(rname))
        scenario_list.append(scenario)

    warnings = []
    leadsto_scenarios, leadsto_warnings = _build_leadsto_response_scenarios(explored, spec)
    scenario_list.extend(leadsto_scenarios)
    warnings.extend(leadsto_warnings)

    for aname, cov in coverage_diag.items():
        if cov is True:
            info = cover_info.get(resolve_action_name(aname, spec))
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
            scenario = _attach_requirement({
                "name": f"cover_{aname}",
                "kind": "action_coverage",
                "action": aname,
                "steps": steps,
                "initial_state": trace[0]["state"],
                "expected_states": expected_states,
            }, instances[info["idx"]]["action_def"])
            scenario_list.append(scenario)
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

    if spec.get("acceptance"):
        from .acceptance import validate_acceptance
        acceptance = validate_acceptance(spec)
        if not acceptance.get("ok"):
            out = dict(acceptance)
            out.pop("ok", None)
            return {"result": "error", **out}
        scenario_list.extend(acceptance["scenarios"])

    if spec.get("forbidden"):
        from .acceptance import validate_forbidden
        forbidden = validate_forbidden(spec)
        if not forbidden.get("ok"):
            out = dict(forbidden)
            out.pop("ok", None)
            return {"result": "error", **out}
        scenario_list.extend(forbidden["scenarios"])

    return _display_output({
        "result": "scenarios",
        "spec": explored["spec"],
        "depth": depth,
        "convention": _SCENARIOS_CONVENTION,
        "scenarios": scenario_list,
        "warnings": warnings,
    }, spec)


def prove(
        spec, k_ind, base_depth, deadlock_mode="warn", vacuity_mode="warn",
        property_name=None):
    """k-induction: base BMC then step-case invariant proof."""
    invariants, property_error = _select_invariants(spec, property_name)
    if property_error is not None:
        return _display_output(property_error, spec)

    base = verify(
        spec,
        base_depth,
        deadlock_mode=deadlock_mode,
        vacuity_mode=vacuity_mode,
        property_name=property_name,
    )
    if base["result"] in ("violated", "reachable_failed", "error"):
        return base

    instances = build_instances(spec)
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
        return _display_output(_attach_requirement({
            "result": "unknown_cti",
            "spec": spec["name"],
            "invariant": inv["name"],
            "k": k,
            "cti": {
                "states": trace,
                "violated_at": k,
            },
            "hint": _CTI_HINT,
        }, inv), spec)

    warnings = [
        w for w in base.get("warnings", [])
        if not (isinstance(w, dict) and "deadlock" in w.get("message", ""))
    ]

    result = {
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
    if base.get("leads_to") is not None:
        result["leads_to"] = base["leads_to"]
        result["note"] = (
            f"invariants proved for all depths; leadsTo checked to depth {base_depth} only"
        )
    return _display_output(result, spec)
