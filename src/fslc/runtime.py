"""Concrete runtime monitor for FSL specs (no Z3)."""
from __future__ import annotations

import itertools
from copy import deepcopy
from pathlib import Path

from .model import FslError, binder_range, display_label, domain_range, eval_const, resolve_action_name
from .parser import parse_src
from .model import build_spec
from .bmc import build_instances, compute_changes, _collect_partial_op_sites

_INIT_HINT = "runtime monitor requires a deterministic init"
_PARTIAL_OP_HINT = "guard the action with requires q.size() > 0 (or bound the index)"


class _PartialOp(Exception):
    def __init__(self, loc, name):
        self.loc = loc
        self.name = name


class _EvalError(Exception):
    def __init__(self, message):
        self.message = message


def _err(message, kind="semantics", loc=None, hint=None):
    raise FslError(message, kind=kind, loc=loc, hint=hint)


def _is_enum_member(name, spec):
    for info in spec["types"].values():
        if info["kind"] == "enum" and name in info["members"]:
            return info["members"].index(name)
    return None


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
        return _enum_name(spec, ty[1], val)
    return val


def _display_option_value(state, base, inner_ty, spec, key=None):
    if key is None:
        if not state[f"{base}__present"]:
            return None
        raw = state[f"{base}__value"]
    else:
        if not state[f"{base}__present"][key]:
            return None
        raw = state[f"{base}__value"][key]
    return _display_value(inner_ty, raw, spec)


def _map_domain(kty, spec):
    if kty[0] == "int":
        mx = max(spec["consts"].values()) if spec["consts"] else 1
        return range(0, mx + 1)
    lo, hi = domain_range(kty, spec["types"])
    return range(lo, hi + 1)


def _logical_var_from_lv(lv):
    if lv[0] == "var":
        return lv[1]
    if lv[0] == "index":
        return lv[1]
    if lv[0] == "field_lv":
        base = lv[1]
        if base[0] == "var":
            return base[1]
        if base[0] == "index":
            return base[1]
    return None


def _collect_state_refs(expr, spec, out=None):
    out = out if out is not None else set()
    if not isinstance(expr, tuple):
        return out
    tag = expr[0]
    if tag == "var":
        n = expr[1]
        if n in spec["state"]:
            out.add(n)
        return out
    if tag == "index":
        if isinstance(expr[1], str):
            if expr[1] in spec["state"]:
                out.add(expr[1])
        elif isinstance(expr[1], tuple) and expr[1][0] == "var":
            if expr[1][1] in spec["state"]:
                out.add(expr[1][1])
    for child in expr[1:]:
        if isinstance(child, tuple):
            _collect_state_refs(child, spec, out)
        elif isinstance(child, dict):
            for v in child.values():
                if isinstance(v, tuple):
                    _collect_state_refs(v, spec, out)
        elif isinstance(child, list):
            for v in child:
                if isinstance(v, tuple):
                    _collect_state_refs(v, spec, out)
    return out


def _check_deterministic_init(spec):
    assigned = set()
    allowed = set(spec["consts"].keys())

    def check_rhs(rhs):
        refs = _collect_state_refs(rhs, spec)
        bad = refs - allowed
        if bad:
            _err(
                f"init references state variable '{sorted(bad)[0]}' before it is assigned",
                kind="semantics",
                hint=_INIT_HINT,
            )

    def walk(stmts, in_forall=False):
        for st in stmts:
            tag = st[0]
            if tag == "assign":
                logical = _logical_var_from_lv(st[1])
                if logical is None:
                    _err("invalid init assignment target", kind="semantics", hint=_INIT_HINT)
                if logical in assigned and not in_forall:
                    _err(
                        f"state variable '{logical}' assigned more than once in init",
                        kind="semantics",
                        hint=_INIT_HINT,
                    )
                check_rhs(st[2])
                if not in_forall:
                    assigned.add(logical)
                    allowed.add(logical)
            elif tag == "forall_stmt":
                if in_forall:
                    _err("nested forall in init is not supported", kind="semantics", hint=_INIT_HINT)
                _, binder, body, _ = st
                targets = []
                for s2 in body:
                    if s2[0] == "forall_stmt":
                        _err("nested forall in init is not supported", kind="semantics", hint=_INIT_HINT)
                    if s2[0] == "if":
                        _err("if in init is not supported", kind="semantics", hint=_INIT_HINT)
                    if s2[0] != "assign":
                        continue
                    logical = _logical_var_from_lv(s2[1])
                    if logical is None:
                        _err("invalid init assignment target", kind="semantics", hint=_INIT_HINT)
                    if logical in assigned:
                        _err(
                            f"state variable '{logical}' assigned more than once in init",
                            kind="semantics",
                            hint=_INIT_HINT,
                        )
                    if logical in targets:
                        _err(
                            f"state variable '{logical}' assigned more than once in init forall",
                            kind="semantics",
                            hint=_INIT_HINT,
                        )
                    targets.append(logical)
                body_allowed = set(allowed)
                for s2 in body:
                    if s2[0] != "assign":
                        continue
                    refs = _collect_state_refs(s2[2], spec)
                    bad = refs - body_allowed
                    if bad:
                        _err(
                            f"init references state variable '{sorted(bad)[0]}' before it is assigned",
                            kind="semantics",
                            hint=_INIT_HINT,
                        )
                    logical = _logical_var_from_lv(s2[1])
                    body_allowed.add(logical)
                for logical in targets:
                    assigned.add(logical)
                    allowed.add(logical)
            elif tag == "if":
                _err("if in init is not supported", kind="semantics", hint=_INIT_HINT)

    walk(spec["init"])
    missing = set(spec["state"]) - assigned
    if missing:
        _err(
            f"init does not assign state variable(s): {', '.join(sorted(missing))}",
            kind="semantics",
            hint=_INIT_HINT,
        )


def _default_phys_value(entry, spec):
    ty = entry["ty"]
    if ty[0] in ("int", "domain", "enum"):
        return 0
    if ty[0] == "bool":
        return False
    if ty[0] == "set":
        elem_ty = ty[1]
        return {i: False for i in _map_domain(elem_ty, spec)}
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        dom = _map_domain(kty, spec)
        if vty[0] == "option":
            return (
                {i: False for i in dom},
                {i: 0 for i in dom},
            )
        if vty[0] == "struct":
            sname = vty[1]
            defaults = {}
            for fn, fty in spec["types"][sname]["fields"].items():
                if fty[0] == "option":
                    defaults[f"{fn}__present"] = {i: False for i in dom}
                    defaults[f"{fn}__value"] = {i: _scalar_default(fty[1]) for i in dom}
                else:
                    defaults[fn] = {i: _scalar_default(fty) for i in dom}
            return defaults
        return {i: _scalar_default(vty) for i in dom}
    return 0


def _scalar_default(ty):
    if ty[0] == "bool":
        return False
    return 0


def _empty_phys_state(spec):
    state = {}
    for entry in spec["phys_vars"]:
        phys = entry["phys"]
        logical = entry["logical"]
        ty = spec["state"][logical]
        if ty[0] == "option":
            if entry.get("part") == "present":
                state[phys] = False
            elif entry.get("part") == "value":
                state[phys] = 0
        elif ty[0] == "seq":
            if entry.get("part") == "data":
                cap = ty[2]
                elem_ty = ty[1]
                state[phys] = [_scalar_default(elem_ty) for _ in range(cap)]
            elif entry.get("part") == "len":
                state[phys] = 0
        elif ty[0] == "map":
            if ty[2][0] == "struct":
                if entry.get("part"):
                    dom = _map_domain(ty[1], spec)
                    if entry.get("option_part") == "present":
                        state[phys] = {i: False for i in dom}
                    elif entry.get("option_part") == "value":
                        fty = spec["types"][ty[2][1]]["fields"][entry["field"]][1]
                        state[phys] = {i: _scalar_default(fty) for i in dom}
                    else:
                        fty = spec["types"][ty[2][1]]["fields"][entry["part"]]
                        state[phys] = {i: _scalar_default(fty) for i in dom}
            elif ty[2][0] == "option":
                dom = _map_domain(ty[1], spec)
                if entry.get("part") == "present":
                    state[phys] = {i: False for i in dom}
                else:
                    state[phys] = {i: 0 for i in dom}
            else:
                dom = _map_domain(ty[1], spec)
                state[phys] = {i: _scalar_default(ty[2]) for i in dom}
        elif ty[0] == "set":
            elem_ty = ty[1]
            state[phys] = {i: False for i in _map_domain(elem_ty, spec)}
        elif ty[0] == "struct":
            if entry.get("option_part") == "present":
                state[phys] = False
            elif entry.get("option_part") == "value":
                state[phys] = _scalar_default(entry["ty"])
            else:
                state[phys] = _scalar_default(entry["ty"])
        else:
            state[phys] = _scalar_default(ty)
    return state


def _as_bool(v):
    if isinstance(v, bool):
        return v
    raise _EvalError(f"expected bool, got {v!r}")


def eval_concrete(e, state, binds, spec, old_state=None, in_ensures=False):
    try:
        return _eval_concrete_impl(e, state, binds, spec, old_state, in_ensures)
    except _PartialOp:
        raise
    except _EvalError as ex:
        raise
    except Exception as ex:
        raise _EvalError(str(ex)) from ex


def _eval_concrete_impl(e, state, binds, spec, old_state=None, in_ensures=False):
    consts = spec["consts"]
    tag = e[0]
    if tag == "num":
        return e[1]
    if tag == "bool":
        return e[1]
    if tag == "none":
        return ("none",)
    if tag == "some":
        v = eval_concrete(e[1], state, binds, spec, old_state, in_ensures)
        if isinstance(v, tuple) and v[0] == "option_val":
            _err("nested Option in some()")
        return ("option_val", True, v)
    if tag == "set_lit":
        _err("bare Set literal must appear in assignment")
    if tag == "seq_lit":
        _err("bare Seq literal must appear in assignment")
    if tag == "struct_lit":
        sname, fields = e[1], e[2]
        vals = {fn: eval_concrete(fe, state, binds, spec, old_state, in_ensures) for fn, fe in fields.items()}
        return ("struct_val", sname, vals)
    if tag == "neg":
        return -eval_concrete(e[1], state, binds, spec, old_state, in_ensures)
    if tag == "var":
        n = e[1]
        if n in binds:
            return binds[n]
        if n in consts:
            return consts[n]
        ei = _is_enum_member(n, spec)
        if ei is not None:
            return ei
        if n in spec["state"]:
            return _read_logical(n, spec["state"][n], state, spec)
        if n in state:
            return state[n]
        _err(f"unknown identifier '{n}'")
    if tag == "index":
        base_e = e[1]
        idx = eval_concrete(e[2], state, binds, spec, old_state, in_ensures)
        return _eval_index(base_e, idx, state, spec)
    if tag == "field":
        base = eval_concrete(e[1], state, binds, spec, old_state, in_ensures)
        return _eval_field(base, e[2], spec)
    if tag == "method":
        base = eval_concrete(e[1], state, binds, spec, old_state, in_ensures)
        return _eval_method(base, e[2], e[3], state, binds, spec, old_state, in_ensures)
    if tag == "is":
        return _eval_is(e[1], e[2], state, binds, spec, old_state, in_ensures)
    if tag == "not":
        return not _as_bool(eval_concrete(e[1], state, binds, spec, old_state, in_ensures))
    if tag == "bin":
        op = e[1]
        if op == "and":
            a = eval_concrete(e[2], state, binds, spec, old_state, in_ensures)
            if not _as_bool(a):
                return False
            return _as_bool(eval_concrete(e[3], state, binds, spec, old_state, in_ensures))
        if op == "or":
            a = eval_concrete(e[2], state, binds, spec, old_state, in_ensures)
            if _as_bool(a):
                return True
            return _as_bool(eval_concrete(e[3], state, binds, spec, old_state, in_ensures))
        if op == "=>":
            a = eval_concrete(e[2], state, binds, spec, old_state, in_ensures)
            if not _as_bool(a):
                return True
            return _as_bool(eval_concrete(e[3], state, binds, spec, old_state, in_ensures))
        a = eval_concrete(e[2], state, binds, spec, old_state, in_ensures)
        b = eval_concrete(e[3], state, binds, spec, old_state, in_ensures)
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
        _err(f"unknown operator '{op}'")
    if tag in ("forall", "exists"):
        return _eval_quant(e, state, binds, spec, old_state, in_ensures)
    if tag == "count":
        return _eval_count(e, state, binds, spec, old_state, in_ensures)
    if tag == "sum":
        return _eval_sum(e, state, binds, spec, old_state, in_ensures)
    if tag == "min":
        a = eval_concrete(e[1], state, binds, spec, old_state, in_ensures)
        b = eval_concrete(e[2], state, binds, spec, old_state, in_ensures)
        return a if a <= b else b
    if tag == "max":
        a = eval_concrete(e[1], state, binds, spec, old_state, in_ensures)
        b = eval_concrete(e[2], state, binds, spec, old_state, in_ensures)
        return a if a >= b else b
    if tag == "abs":
        a = eval_concrete(e[1], state, binds, spec, old_state, in_ensures)
        return a if a >= 0 else -a
    if tag == "old":
        if not in_ensures:
            _err("old() is only allowed in ensures clauses", kind="type")
        if old_state is None:
            _err("old() used without old state context")
        return eval_concrete(e[1], old_state, binds, spec, None, False)
    _err(f"cannot evaluate expression node {e}")


def _read_logical(name, ty, state, spec):
    if ty[0] == "option":
        return ("option_val", state[f"{name}__present"], state[f"{name}__value"])
    if ty[0] == "struct":
        sname = ty[1]
        return ("struct_val", sname, {
            fn: (
                ("option_val", state[f"{name}__{fn}__present"], state[f"{name}__{fn}__value"])
                if fty[0] == "option"
                else state[f"{name}__{fn}"]
            )
            for fn, fty in spec["types"][sname]["fields"].items()
        })
    if ty[0] == "set":
        m = state[name]
        return ("set_val", m, ty[1])
    if ty[0] == "seq":
        return ("seq_val", state[f"{name}__data"], state[f"{name}__len"], ty[1], ty[2])
    return state[name]


def _logical_map_access(logical, idx, state, spec):
    ty = spec["state"][logical]
    if ty[0] != "map":
        _err(f"'{logical}' is not a map")
    vty = ty[2]
    if vty[0] == "struct":
        sname = vty[1]
        return ("struct_map_val", logical, idx, {
            fn: (
                ("option_val",
                 state[f"{logical}__{fn}__present"][idx],
                 state[f"{logical}__{fn}__value"][idx])
                if fty[0] == "option"
                else state[f"{logical}__{fn}"][idx]
            )
            for fn, fty in spec["types"][sname]["fields"].items()
        })
    if vty[0] == "option":
        return (
            "option_val",
            state[f"{logical}__present"][idx],
            state[f"{logical}__value"][idx],
        )
    return state[logical][idx]


def _eval_index(base_e, idx, state, spec):
    if isinstance(base_e, str):
        name = base_e
    elif base_e[0] == "var":
        name = base_e[1]
    else:
        _err("complex index base not supported")
    if name in spec["state"]:
        return _logical_map_access(name, idx, state, spec)
    if name in state:
        val = state[name]
        if isinstance(val, dict):
            return val[idx]
        if isinstance(val, list):
            return val[idx]
        _err(f"cannot index '{name}'")
    _err(f"unknown map '{name}'")


def _eval_field(base, field, spec):
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


def _seq_val_parts(base):
    if isinstance(base, tuple) and base[0] == "seq_val":
        return base[1], base[2], base[3], base[4]
    return None


def _set_elem_ty(base):
    if isinstance(base, tuple) and base[0] == "set_val":
        return base[1], base[2]
    return None


def _eval_set_method(m, elem_ty, method, args, state, binds, spec, old_state, in_ensures):
    if method == "contains":
        if len(args) != 1:
            _err("contains expects 1 argument")
        e = eval_concrete(args[0], state, binds, spec, old_state, in_ensures)
        return bool(m.get(e, False)) if isinstance(m, dict) else False
    if method == "add":
        e = eval_concrete(args[0], state, binds, spec, old_state, in_ensures)
        nm = dict(m)
        nm[e] = True
        return ("set_val", nm, elem_ty)
    if method == "remove":
        e = eval_concrete(args[0], state, binds, spec, old_state, in_ensures)
        nm = dict(m)
        nm[e] = False
        return ("set_val", nm, elem_ty)
    if method == "size":
        return sum(1 for v in m.values() if v)
    return None


def _eval_seq_method(data, length, elem_ty, cap, method, args, state, binds, spec,
                       old_state, in_ensures, loc=None):
    if method == "push":
        e = eval_concrete(args[0], state, binds, spec, old_state, in_ensures)
        if length >= cap:
            raise _PartialOp(loc, "_partial_seq_push")
        nd = list(data)
        nd[length] = e
        return ("seq_val", nd, length + 1, elem_ty, cap)
    if method == "pop":
        if length <= 0:
            raise _PartialOp(loc, "_partial_seq_pop")
        nd = list(data)
        for i in range(length - 1):
            nd[i] = nd[i + 1]
        return ("seq_val", nd, length - 1, elem_ty, cap)
    if method == "head":
        if length <= 0:
            raise _PartialOp(loc, "_partial_seq_head")
        return data[0]
    if method == "at":
        idx = eval_concrete(args[0], state, binds, spec, old_state, in_ensures)
        if idx < 0 or idx >= length:
            raise _PartialOp(loc, "_partial_seq_at")
        return data[idx]
    if method == "contains":
        e = eval_concrete(args[0], state, binds, spec, old_state, in_ensures)
        return any(data[i] == e for i in range(length))
    if method == "size":
        return length
    return None


def _eval_method(base, method, args, state, binds, spec, old_state, in_ensures, loc=None):
    set_parts = _set_elem_ty(base)
    if set_parts is not None:
        res = _eval_set_method(set_parts[0], set_parts[1], method, args, state, binds, spec,
                               old_state, in_ensures)
        if res is not None:
            return res
        _err(f"unknown method '{method}' on Set")
    seq_parts = _seq_val_parts(base)
    if seq_parts is not None:
        data, length, elem_ty, cap = seq_parts
        res = _eval_seq_method(data, length, elem_ty, cap, method, args, state, binds, spec,
                               old_state, in_ensures, loc=loc)
        if res is not None:
            return res
        _err(f"unknown method '{method}' on Seq")
    _err("method call on value that is neither Set nor Seq")


def _eval_is(inner, pat, state, binds, spec, old_state, in_ensures):
    val = eval_concrete(inner, state, binds, spec, old_state, in_ensures)
    if pat[0] == "pat_none":
        if isinstance(val, tuple) and val[0] == "option_val":
            return not val[1]
        if isinstance(val, tuple) and val[0] == "none":
            return True
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
    if qop == "forall":
        for i in range(lo, hi + 1):
            b2 = dict(binds)
            b2[v] = i
            if where is not None:
                w = eval_concrete(where, state, b2, spec, old_state, in_ensures)
                if not _as_bool(w):
                    continue
            if not _as_bool(eval_concrete(body, state, b2, spec, old_state, in_ensures)):
                return False
        return True
    for i in range(lo, hi + 1):
        b2 = dict(binds)
        b2[v] = i
        if where is not None:
            w = eval_concrete(where, state, b2, spec, old_state, in_ensures)
            if not _as_bool(w):
                continue
        if _as_bool(eval_concrete(body, state, b2, spec, old_state, in_ensures)):
            return True
    return False


def _eval_count(e, state, binds, spec, old_state, in_ensures):
    _, v, ty_name, cond = e
    ty = spec["types"][ty_name]["ty"]
    lo, hi = domain_range(ty, spec["types"])
    total = 0
    for i in range(lo, hi + 1):
        b2 = {**binds, v: i}
        if _as_bool(eval_concrete(cond, state, b2, spec, old_state, in_ensures)):
            total += 1
    return total


def _eval_sum(e, state, binds, spec, old_state, in_ensures):
    _, v, ty_name, body, cond = e
    ty = spec["types"][ty_name]["ty"]
    lo, hi = domain_range(ty, spec["types"])
    total = 0
    for i in range(lo, hi + 1):
        b2 = {**binds, v: i}
        if cond is not None:
            if not _as_bool(eval_concrete(cond, state, b2, spec, old_state, in_ensures)):
                continue
        total += eval_concrete(body, state, b2, spec, old_state, in_ensures)
    return total


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
    if not (isinstance(a, tuple) and a[0] == "seq_val" and isinstance(b, tuple) and b[0] == "seq_val"):
        return None
    _, data1, len1, _, cap1 = a
    _, data2, len2, _, cap2 = b
    if cap1 != cap2:
        _err("Seq comparison between different capacities", kind="type")
    eq = len1 == len2 and all(data1[i] == data2[i] for i in range(len1))
    return (not eq) if op == "!=" else eq


def _struct_compare(a, b, op, spec):
    sa, fa = _struct_info(a, spec)
    sb, fb = _struct_info(b, spec)
    if sa is None and sb is None:
        return None
    if sa is None or sb is None:
        _err("struct comparison requires two struct values", kind="type")
    if sa != sb:
        _err(f"struct comparison between '{sa}' and '{sb}'", kind="type")
    fields = spec["types"][sa]["fields"]
    eq = True
    for k in set(fa) | set(fb):
        if fields[k][0] == "option":
            eq = eq and _option_logical_eq(fa.get(k), fb.get(k))
        else:
            eq = eq and fa.get(k) == fb.get(k)
    return (not eq) if op == "!=" else eq


def _option_logical_eq(a, b):
    if isinstance(a, tuple) and a[0] == "option_val":
        if isinstance(b, tuple) and b[0] == "option_val":
            return a[1] == b[1] and (not a[1] or a[2] == b[2])
        if isinstance(b, tuple) and b[0] == "none":
            return not a[1]
    if isinstance(b, tuple) and b[0] == "option_val":
        if isinstance(a, tuple) and a[0] == "none":
            return not b[1]
    if isinstance(a, tuple) and a[0] == "none" and isinstance(b, tuple) and b[0] == "none":
        return True
    _err("struct Option field comparison requires Option values", kind="type")


def _option_none_cmp(a, b, op):
    if isinstance(a, tuple) and a[0] == "option_val" and isinstance(b, tuple) and b[0] == "none":
        present = a[1]
        return (not present) if op == "==" else present
    if isinstance(b, tuple) and b[0] == "option_val" and isinstance(a, tuple) and a[0] == "none":
        present = b[1]
        return (not present) if op == "==" else present
    return None


def _reject_option_binop(a, b, op):
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
        _err("Option == and != are only defined against none", kind="type")
    _err(f"Option values cannot be used with '{op}'", kind="type")


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


def _struct_field_ty(spec, sname, field):
    try:
        return spec["types"][sname]["fields"][field]
    except KeyError:
        _err(f"unknown field '{field}' in struct {sname}")


def _assign_option_to_phys(pend, state, present_phys, value_phys, val):
    if isinstance(val, tuple) and val[0] == "option_val":
        pend[present_phys] = val[1]
        pend[value_phys] = val[2]
        return
    if val == ("none",):
        pend[present_phys] = False
        return
    _err("Option assignment requires none or some(...)")


def _store_option_to_phys(pend, state, present_phys, value_phys, idx, val):
    base_p = dict(pend.get(present_phys, state[present_phys]))
    if isinstance(val, tuple) and val[0] == "option_val":
        base_v = dict(pend.get(value_phys, state[value_phys]))
        base_p[idx] = val[1]
        base_v[idx] = val[2]
        pend[present_phys] = base_p
        pend[value_phys] = base_v
        return
    if val == ("none",):
        base_p[idx] = False
        pend[present_phys] = base_p
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
        base = dict(pend.get(phys_base, state[phys_base]))
        base[idx] = fv
        pend[phys_base] = base


def _apply_assign(lv, rhs, pend, state, binds, spec, loc=None):
    key = _lvalue_key(lv)

    if key[0] == "scalar":
        n = key[1]
        ty = spec["state"][n]
        if ty[0] == "set" and rhs[0] == "set_lit":
            elem_ty = ty[1]
            m = {i: False for i in _map_domain(elem_ty, spec)}
            for lit in rhs[1]:
                idx = eval_concrete(lit, state, binds, spec)
                m[idx] = True
            pend[n] = m
            return ("scalar", n)
        if ty[0] == "seq" and rhs[0] == "seq_lit":
            elem_ty, cap = ty[1], ty[2]
            if len(rhs[1]) > cap:
                _err(f"Seq literal has {len(rhs[1])} elements but capacity is {cap}", kind="type")
            data = [_scalar_default(elem_ty) for _ in range(cap)]
            for i, lit in enumerate(rhs[1]):
                data[i] = eval_concrete(lit, state, binds, spec)
            pend[f"{n}__data"] = data
            pend[f"{n}__len"] = len(rhs[1])
            return ("scalar", n)

    val = eval_concrete(rhs, state, binds, spec)

    if key[0] == "scalar":
        n = key[1]
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
                pend[f"{n}__present"] = False
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
        ty = spec["state"][n]
        idx = eval_concrete(idx_e, state, binds, spec)
        vty = ty[2]
        if vty[0] == "option":
            base_p = dict(pend.get(f"{n}__present", state[f"{n}__present"]))
            base_v = dict(pend.get(f"{n}__value", state[f"{n}__value"]))
            if isinstance(val, tuple) and val[0] == "option_val":
                base_p[idx] = val[1]
                base_v[idx] = val[2]
            elif rhs[0] == "none":
                base_p[idx] = False
            else:
                _err("Option map assignment requires none or some(...)")
            pend[f"{n}__present"] = base_p
            pend[f"{n}__value"] = base_v
        elif vty[0] == "struct":
            if isinstance(val, tuple) and val[0] == "struct_val":
                _, sname, fields = val
                for fn, fv in fields.items():
                    fty = _struct_field_ty(spec, sname, fn)
                    _store_struct_field(pend, state, f"{n}__{fn}", idx, fty, fv)
            else:
                _err("struct map assignment requires struct literal")
        else:
            base = dict(pend.get(n, state[n]))
            base[idx] = val
            pend[n] = base
        return ("map", n, idx_e)

    if key[0] == "map_field":
        n, idx_e, field = key[1], key[2], key[3]
        idx = eval_concrete(idx_e, state, binds, spec)
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
                _err(f"double assignment to '{parts[0]}' on the same execution path", kind="semantics", loc=loc)
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

    def run(st, binds, path_ast=None):
        tag = st[0]
        if tag == "assign":
            w = _apply_assign(st[1], st[2], pend, state, binds, spec, loc=st[3] if len(st) > 3 else None)
            if w:
                check_scalar(*w, loc=st[3] if len(st) > 3 else None)
        elif tag == "if":
            _, cond, then_stmts, else_stmts, loc = st
            c = eval_concrete(cond, state, binds, spec)
            if _as_bool(c):
                for s2 in then_stmts:
                    run(s2, binds, _and_path(cond, path_ast))
            else:
                for s2 in else_stmts:
                    run(s2, binds, _and_path(("not", cond), path_ast))
        elif tag == "forall_stmt":
            _, binder, body, _ = st
            v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
            for i in range(lo, hi + 1):
                b2 = {**binds, v: i}
                if where is not None:
                    wcond = eval_concrete(where, state, b2, spec)
                    if not _as_bool(wcond):
                        continue
                for s2 in body:
                    run(s2, b2, path_ast)
        else:
            _err(f"unknown stmt {st}")

    for st in stmts:
        run(st, binds)
    return pend


def _and_path(cond, path_ast):
    if path_ast is None:
        return cond
    return ("bin", "and", path_ast, cond)


def _exec_init(spec):
    state = _empty_phys_state(spec)
    binds = {}

    def run(st, binds):
        tag = st[0]
        if tag == "assign":
            pend = {}
            _apply_assign(st[1], st[2], pend, state, binds, spec)
            state.update(pend)
        elif tag == "forall_stmt":
            _, binder, body, _ = st
            v, lo, hi, where = binder_range(binder, spec["consts"], spec["types"])
            for i in range(lo, hi + 1):
                b2 = {**binds, v: i}
                if where is not None:
                    w = eval_concrete(where, state, b2, spec)
                    if not _as_bool(w):
                        continue
                for s2 in body:
                    run(s2, b2)
        elif tag == "if":
            _err("if in init is not supported", kind="semantics")

    for st in spec["init"]:
        run(st, binds)
    return state


def _display_state_keys(logical, spec):
    dn = spec.get("display_names") or {}
    if not dn:
        return logical
    return {dn.get(k, k): v for k, v in logical.items()}


def phys_to_logical(state, spec):
    out = {}
    for n, ty in spec["state"].items():
        out[n] = _logical_val(state, n, ty, spec)
    return _display_state_keys(out, spec)


def _logical_val(state, name, ty, spec):
    if ty[0] in ("int", "domain", "enum"):
        phys = name
        for p in spec["phys_vars"]:
            if p["logical"] == name and "part" not in p:
                phys = p["phys"]
                break
        raw = state[phys]
        return _display_value(ty, raw, spec)
    if ty[0] == "bool":
        return state[name]
    if ty[0] == "set":
        elem_ty = ty[1]
        m = state[name]
        elems = [_display_value(elem_ty, i, spec) for i in _map_domain(elem_ty, spec) if m.get(i)]
        return sorted(elems, key=str)
    if ty[0] == "seq":
        elem_ty, cap = ty[1], ty[2]
        data = state[f"{name}__data"]
        length = state[f"{name}__len"]
        return [_display_value(elem_ty, data[i], spec) for i in range(length)]
    if ty[0] == "option":
        return _display_option_value(state, name, ty[1], spec)
    if ty[0] == "struct":
        sname = ty[1]
        out = {}
        for fn, fty in spec["types"][sname]["fields"].items():
            if fty[0] == "option":
                out[fn] = _display_option_value(state, f"{name}__{fn}", fty[1], spec)
            else:
                out[fn] = _display_value(fty, state[f"{name}__{fn}"], spec)
        return out
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        mout = {}
        for i in _map_domain(kty, spec):
            key = str(_display_value(kty, i, spec) if kty[0] == "enum" else i)
            if vty[0] == "option":
                mout[key] = _display_option_value(state, name, vty[1], spec, i)
            elif vty[0] == "struct":
                sname = vty[1]
                obj = {}
                for fn, fty in spec["types"][sname]["fields"].items():
                    if fty[0] == "option":
                        obj[fn] = _display_option_value(state, f"{name}__{fn}", fty[1], spec, i)
                    else:
                        obj[fn] = _display_value(fty, state[f"{name}__{fn}"][i], spec)
                mout[key] = obj
            else:
                mout[key] = _display_value(vty, state[name][i], spec)
        return mout
    return None


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


def _requires_text(req, source_lines):
    loc = req.get("loc") or {}
    text = None
    if source_lines:
        line = loc.get("line")
        if line and 1 <= line <= len(source_lines):
            text = source_lines[line - 1].strip() or None
    out = {}
    if loc:
        out["loc"] = loc
    if text:
        out["text"] = text
    return out


def _partial_op_well_defined_concrete(site_expr, state, binds, spec):
    method = site_expr[2]
    base = eval_concrete(site_expr[1], state, binds, spec)
    seq_parts = _seq_val_parts(base)
    if seq_parts is None:
        return True
    data, length, _, cap = seq_parts
    if method in ("pop", "head"):
        return length > 0
    if method == "at":
        idx = eval_concrete(site_expr[3][0], state, binds, spec)
        return 0 <= idx < length
    return True


def _eval_requires(requires, lets, state, param_binds, spec, source_lines=None, action_name=None):
    binds = dict(param_binds)
    for let in lets:
        binds[let["name"]] = eval_concrete(let["expr"], state, binds, spec)
    for req in requires:
        b = dict(binds)
        try:
            ok = eval_concrete(req["expr"], state, b, spec)
        except _PartialOp as po:
            pname = f"_partial_{action_name}" if action_name else po.name
            return None, binds, {
                "kind": "partial_op",
                "name": pname,
                "loc": po.loc or req.get("loc"),
                "hint": _PARTIAL_OP_HINT,
            }
        if not _as_bool(ok):
            return False, binds, {
                "kind": "requires_failed",
                "requires": _requires_text(req, source_lines),
            }
        for k, v in b.items():
            if k not in param_binds:
                binds[k] = v
    return True, binds, None


class Monitor:
    def __init__(self, source_or_path):
        if isinstance(source_or_path, (str, Path)) and Path(source_or_path).is_file():
            path = Path(source_or_path)
            src = path.read_text(encoding="utf-8")
            self._source_lines = src.splitlines()
            ast, display_names = parse_src(src, str(path.parent))
            self._spec = build_spec(ast, display_names)
        elif (
            isinstance(source_or_path, (str, Path))
            and "\n" not in str(source_or_path)
            and "\r" not in str(source_or_path)
            and str(source_or_path).endswith(".fsl")
        ):
            _err(f"file not found: {source_or_path}", kind="io")
        elif isinstance(source_or_path, str):
            self._source_lines = source_or_path.splitlines()
            ast, display_names = parse_src(source_or_path)
            self._spec = build_spec(ast, display_names)
        elif isinstance(source_or_path, dict) and "state" in source_or_path:
            self._spec = source_or_path
            self._source_lines = None
        else:
            _err("Monitor expects a spec source string, file path, or built spec dict")
        _check_deterministic_init(self._spec)
        self._instances = build_instances(self._spec)
        self._phys = None
        self._logical = None

    @property
    def spec(self):
        return self._spec

    def reset(self):
        self._phys = _exec_init(self._spec)
        self._logical = phys_to_logical(self._phys, self._spec)
        return dict(self._logical)

    @property
    def state(self):
        if self._logical is None:
            return self.reset()
        return dict(self._logical)

    def enabled(self):
        if self._phys is None:
            self.reset()
        out = []
        for inst in self._instances:
            try:
                guards_ok, _, viol = _eval_requires(
                    inst["requires"], inst["lets"], self._phys, inst["binds"], self._spec,
                    self._source_lines, action_name=inst["action"],
                )
            except (FslError, _EvalError):
                continue
            if viol is not None or guards_ok is not True:
                continue
            act = inst["action_def"]
            out.append({
                "action": display_label(inst["action"], self._spec),
                "params": {pk: _display_param(pk, pv, act, self._spec) for pk, pv in inst["binds"].items()},
            })
        return out

    def _find_action(self, name, params):
        name = resolve_action_name(name, self._spec)
        act_def = None
        for act in self._spec["actions"]:
            if act["name"] == name:
                act_def = act
                break
        if act_def is None:
            return None, {"kind": "bad_call", "message": f"unknown action '{name}'"}

        expected = {p[0]: p for p in act_def["params"]}
        if set(params.keys()) != set(expected.keys()):
            return None, {
                "kind": "bad_call",
                "message": f"parameter mismatch for action '{name}'",
                "expected": list(expected.keys()),
                "got": list(params.keys()),
            }

        binds = {}
        for pname, pdef in expected.items():
            val = params[pname]
            lo, hi = pdef[1], pdef[2]
            if isinstance(val, str):
                ei = _is_enum_member(val, self._spec)
                if ei is not None:
                    val = ei
                elif pname in self._spec["types"]:
                    ty = self._spec["types"][pname]["ty"]
                    if ty[0] == "enum":
                        members = self._spec["types"][ty[1]]["members"]
                        if val in members:
                            val = members.index(val)
            if not isinstance(val, int):
                return None, {"kind": "bad_call", "message": f"parameter '{pname}' must be int or enum name"}
            if val < lo or val > hi:
                return None, {
                    "kind": "bad_call",
                    "message": f"parameter '{pname}' out of range [{lo}..{hi}]",
                }
            binds[pname] = val

        inst = {
            "action": name,
            "action_def": act_def,
            "binds": binds,
            "requires": act_def["requires"],
            "lets": act_def["lets"],
            "stmts": act_def["stmts"],
            "ensures": act_def["ensures"],
        }
        return inst, None

    def step(self, action, params):
        if self._phys is None:
            self.reset()
        old_phys = deepcopy(self._phys)
        old_logical = dict(self._logical)

        inst, bad = self._find_action(action, params)
        if bad:
            return {"ok": False, **bad, "action": action, "params": params, "state": old_logical}

        disp = display_label(inst["action"], self._spec)

        try:
            guards_ok, binds, viol = _eval_requires(
                inst["requires"], inst["lets"], old_phys, inst["binds"], self._spec,
                self._source_lines, action_name=inst["action"],
            )
        except _PartialOp as po:
            return {
                "ok": False,
                "kind": "partial_op",
                "name": display_label(f"_partial_{inst['action']}", self._spec),
                "loc": po.loc,
                "action": disp,
                "params": params,
                "state": old_logical,
                "hint": _PARTIAL_OP_HINT,
            }
        except _EvalError as ex:
            return {
                "ok": False,
                "kind": "internal",
                "message": ex.message,
                "action": disp,
                "params": params,
                "state": old_logical,
            }
        except FslError as ex:
            return {
                "ok": False,
                "kind": "internal",
                "message": str(ex),
                "action": disp,
                "params": params,
                "state": old_logical,
            }

        if viol:
            out = {
                "ok": False,
                "kind": viol["kind"],
                "action": disp,
                "params": params,
                "state": old_logical,
            }
            if viol["kind"] == "requires_failed":
                out["requires"] = viol["requires"]
            else:
                out["name"] = display_label(viol["name"], self._spec)
                out["loc"] = viol.get("loc")
                out["hint"] = viol.get("hint")
            return out

        if guards_ok is not True:
            return {
                "ok": False,
                "kind": "requires_failed",
                "action": disp,
                "params": params,
                "state": old_logical,
            }

        try:
            pend = compute_updates(inst["stmts"], old_phys, binds, self._spec)
        except _PartialOp as po:
            return {
                "ok": False,
                "kind": "partial_op",
                "name": display_label(f"_partial_{inst['action']}", self._spec),
                "loc": po.loc,
                "action": disp,
                "params": params,
                "state": old_logical,
                "hint": _PARTIAL_OP_HINT,
            }
        except _EvalError as ex:
            return {
                "ok": False,
                "kind": "internal",
                "message": ex.message,
                "action": disp,
                "params": params,
                "state": old_logical,
            }
        except FslError as ex:
            return {
                "ok": False,
                "kind": "internal",
                "message": str(ex),
                "action": disp,
                "params": params,
                "state": old_logical,
            }

        new_phys = deepcopy(old_phys)
        new_phys.update(pend)
        new_logical = phys_to_logical(new_phys, self._spec)

        for ens in inst["ensures"]:
            try:
                cond = eval_concrete(
                    ens["expr"], new_phys, binds, self._spec,
                    old_state=old_phys, in_ensures=True,
                )
            except (_EvalError, FslError) as ex:
                msg = ex.message if isinstance(ex, _EvalError) else str(ex)
                return {
                    "ok": False,
                    "kind": "internal",
                    "message": msg,
                    "action": disp,
                    "params": params,
                    "state": old_logical,
                }
            if not _as_bool(cond):
                return {
                    "ok": False,
                    "kind": "ensures",
                    "name": disp,
                    "loc": ens.get("loc"),
                    "action": disp,
                    "params": params,
                    "state": old_logical,
                }

        for inv in self._spec["invariants"]:
            try:
                cond = eval_concrete(inv["expr"], new_phys, {}, self._spec)
            except (_EvalError, FslError) as ex:
                msg = ex.message if isinstance(ex, _EvalError) else str(ex)
                return {
                    "ok": False,
                    "kind": "internal",
                    "message": msg,
                    "action": disp,
                    "params": params,
                    "state": old_logical,
                }
            if not _as_bool(cond):
                return {
                    "ok": False,
                    "kind": _violation_kind(inv),
                    "name": display_label(inv["name"], self._spec),
                    "loc": inv.get("loc"),
                    "action": disp,
                    "params": params,
                    "state": old_logical,
                }

        self._phys = new_phys
        self._logical = new_logical
        changes = compute_changes(old_logical, new_logical)
        return {"ok": True, "action": disp, "state": dict(new_logical), "changes": changes}
