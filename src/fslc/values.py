# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Shared pure helpers for symbolic and concrete value handling."""
from __future__ import annotations

from .model import _err, binder_range, domain_range

_OPTION_EQ_HINT = "use `x is some(v)` to compare the contained value"


def _is_enum_member(name, spec):
    for info in spec["types"].values():
        if info["kind"] == "enum" and name in info["members"]:
            return info["members"].index(name)
    return None


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


def _struct_field_ty(spec, sname, field):
    try:
        return spec["types"][sname]["fields"][field]
    except KeyError:
        _err(f"unknown field '{field}' in struct {sname}")


def _seq_val_parts(base):
    if isinstance(base, tuple) and base[0] == "seq_val":
        return base[1], base[2], base[3], base[4]
    return None


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


def _enum_name(spec, ename, val):
    members = spec["types"][ename]["members"]
    i = int(val)
    if 0 <= i < len(members):
        return members[i]
    return str(val)


def _display_state_keys(logical, spec):
    dn = spec.get("display_names") or {}
    if not dn:
        return logical
    return {dn.get(k, k): v for k, v in logical.items()}


def eval_count(e, state, binds, spec, old_state, in_ensures, dom, ev):
    _, v, ty_name, cond = e
    if ty_name not in spec["types"]:
        _err(f"unknown type '{ty_name}' in count")
    ty = spec["types"][ty_name]["ty"]
    lo, hi = domain_range(ty, spec["types"])
    acc = dom.int_lit(0)
    for i in range(lo, hi + 1):
        b2 = {**binds, v: i}
        c = ev(cond, state, b2, spec, old_state, in_ensures)
        acc = acc + dom.select_int(c, lambda: dom.int_lit(1))
    return acc


def eval_sum(e, state, binds, spec, old_state, in_ensures, dom, ev):
    _, v, ty_name, body, cond = e
    if ty_name not in spec["types"]:
        _err(f"unknown type '{ty_name}' in sum")
    ty = spec["types"][ty_name]["ty"]
    lo, hi = domain_range(ty, spec["types"])
    acc = dom.int_lit(0)
    for i in range(lo, hi + 1):
        b2 = {**binds, v: i}
        if cond is None:
            acc = acc + ev(body, state, b2, spec, old_state, in_ensures)
        else:
            c = ev(cond, state, b2, spec, old_state, in_ensures)
            acc = acc + dom.select_int(
                c, lambda b2=b2: ev(body, state, b2, spec, old_state, in_ensures)
            )
    return acc


def iter_binder_terms(binder, state, binds, spec, old_state, in_ensures, dom, ev):
    if binder[0] == "binder_collection":
        _, v, collection, where = binder
        value = ev(collection, state, binds, spec, old_state, in_ensures)
        if isinstance(value, tuple) and value[0] == "set_val":
            m, elem_ty = value[1], value[2]
            lo, hi = domain_range(elem_ty, spec["types"])
            for i in range(lo, hi + 1):
                b2 = dict(binds)
                member = dom.int_lit(i)
                b2[v] = i
                w = dom.select(m, member)
                if where is not None:
                    w = dom.and_(w, ev(where, state, b2, spec, old_state, in_ensures))
                yield w, b2
            return
        parts = _seq_val_parts(value)
        if parts is not None:
            data, length, _elem_ty, cap = parts
            for i in range(cap):
                b2 = dict(binds)
                idx = dom.int_lit(i)
                b2[v] = dom.select(data, idx)
                w = dom.lt(idx, length)
                if where is not None:
                    w = dom.and_(w, ev(where, state, b2, spec, old_state, in_ensures))
                yield w, b2
            return
        _err("collection binder expects a Set or Seq expression", kind="type")

    v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
    for i in range(lo, hi + 1):
        b2 = dict(binds)
        b2[v] = i
        w = ev(where, state, b2, spec, old_state, in_ensures) if where is not None else None
        yield w, b2


def eval_quant(e, state, binds, spec, old_state, in_ensures, dom, ev):
    qop, binder, body = e[0], e[1], e[2]

    def terms():
        for w, b2 in iter_binder_terms(
            binder, state, binds, spec, old_state, in_ensures, dom, ev
        ):
            yield w, (lambda b2=b2: ev(body, state, b2, spec, old_state, in_ensures))

    return dom.quantify(qop, terms())


def eval_one(e, state, binds, spec, old_state, in_ensures, dom, ev):
    tag, binder = e[0], e[1]
    acc = dom.int_lit(0)
    for w, _b2 in iter_binder_terms(
        binder, state, binds, spec, old_state, in_ensures, dom, ev
    ):
        acc = acc + dom.select_int(w if w is not None else dom.true_(), lambda: dom.int_lit(1))
    if tag == "unique":
        return acc <= dom.int_lit(1)
    return acc == dom.int_lit(1)


def option_logical_eq(a, b, dom):
    if isinstance(a, tuple) and a[0] == "option_val":
        if isinstance(b, tuple) and b[0] == "option_val":
            return dom.and_(a[1] == b[1], dom.implies(a[1], a[2] == b[2]))
        if isinstance(b, tuple) and b[0] == "none":
            return dom.not_(a[1])
    if isinstance(b, tuple) and b[0] == "option_val":
        if isinstance(a, tuple) and a[0] == "none":
            return dom.not_(b[1])
    if isinstance(a, tuple) and a[0] == "none" and isinstance(b, tuple) and b[0] == "none":
        return dom.true_()
    _err("struct Option field comparison requires Option values", kind="type")


def option_none_cmp(a, b, op, dom):
    if isinstance(a, tuple) and a[0] == "option_val" and isinstance(b, tuple) and b[0] == "none":
        present = a[1]
        return dom.not_(present) if op == "==" else present
    if isinstance(b, tuple) and b[0] == "option_val" and isinstance(a, tuple) and a[0] == "none":
        present = b[1]
        return dom.not_(present) if op == "==" else present
    return None


def reject_option_binop(a, b, op):
    def tag(v):
        if isinstance(v, tuple) and v[0] in ("option_val", "none"):
            return v[0]
        return None
    ta, tb = tag(a), tag(b)
    if ta is None and tb is None:
        return
    if op in ("==", "!=") and ta == "none" and tb == "none":
        return
    if op in ("==", "!="):
        _err("Option == and != are only defined against none", kind="type", hint=_OPTION_EQ_HINT)
    _err(f"Option values cannot be used with '{op}'", kind="type")


def seq_compare(a, b, op, spec, dom):
    a_seq = isinstance(a, tuple) and a[0] == "seq_val"
    b_seq = isinstance(b, tuple) and b[0] == "seq_val"
    if not (a_seq and b_seq):
        if a_seq or b_seq:
            _err("Seq comparison requires two Seq values", kind="type")
        return None
    data1, len1, cap1 = a[1], a[2], a[4]
    data2, len2, cap2 = b[1], b[2], b[4]
    if cap1 != cap2:
        _err("Seq comparison between different capacities", kind="type")
    eq = dom.seq_eq(data1, len1, data2, len2, cap1)
    return dom.not_(eq) if op == "!=" else eq


def struct_compare(a, b, op, spec, dom):
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
        if fields[k][0] == "option":
            parts.append(option_logical_eq(fa[k], fb[k], dom))
        else:
            parts.append(fa[k] == fb[k])
    eq = dom.and_all(parts)
    return dom.not_(eq) if op == "!=" else eq


def logical_map_access(logical, idx, state, spec, dom):
    ty = spec["state"][logical]
    if ty[0] != "map":
        _err(f"'{logical}' is not a map")
    vty = ty[2]
    if vty[0] == "struct":
        sname = vty[1]
        return ("struct_map_val", logical, idx, {
            fn: (
                ("option_val",
                 dom.select(state[f"{logical}__{fn}__present"], idx),
                 dom.select(state[f"{logical}__{fn}__value"], idx))
                if fty[0] == "option"
                else dom.select(state[f"{logical}__{fn}"], idx)
            )
            for fn, fty in spec["types"][sname]["fields"].items()
        })
    if vty[0] == "option":
        return ("option_val",
                dom.select(state[f"{logical}__present"], idx),
                dom.select(state[f"{logical}__value"], idx))
    return dom.select(state[logical], idx)


def eval_index(base_e, idx, state, spec, dom):
    if isinstance(base_e, str):
        name = base_e
    elif base_e[0] == "var":
        name = base_e[1]
    else:
        _err("complex index base not supported")
    if name in spec["state"]:
        return logical_map_access(name, idx, state, spec, dom)
    if name in state:
        return dom.select(state[name], idx)
    _err(f"unknown map '{name}'")


def eval_field(base, field):
    # Production-log mapping expressions reuse the refinement expression AST,
    # but JSON objects arrive as ordinary dicts rather than FSL struct tuples.
    if isinstance(base, dict):
        if field not in base:
            _err(f"unknown field '{field}'")
        return base[field]
    if isinstance(base, tuple) and base[0] == "struct_map_val":
        fields = base[3]
        if field not in fields:
            _err(f"unknown field '{field}'")
        return fields[field]
    if isinstance(base, tuple) and base[0] == "struct_val":
        _, sname, vals = base
        if field not in vals:
            _err(f"unknown field '{field}' in struct {sname}")
        return vals[field]
    _err(f"cannot access field '{field}' on this value")


def eval_is(inner, pat, state, binds, spec, old_state, in_ensures, dom, ev):
    val = ev(inner, state, binds, spec, old_state, in_ensures)
    if pat[0] == "pat_none":
        if isinstance(val, tuple) and val[0] == "option_val":
            return dom.not_(val[1])
        if isinstance(val, tuple) and val[0] == "none":
            return dom.true_()
        _err("is none applied to non-Option value", kind="type")
    if pat[0] == "pat_some":
        vname = pat[1]
        if isinstance(val, tuple) and val[0] == "option_val":
            present, value = val[1], val[2]
            binds[vname] = value
            return present
        _err("is some applied to non-Option value", kind="type")
    _err("invalid pattern")
