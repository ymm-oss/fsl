"""Spec model: AST -> validated spec dict, plus shared helpers."""
from __future__ import annotations

import z3


class FslError(Exception):
    def __init__(self, message, kind="semantics", loc=None, expected=None, hint=None):
        super().__init__(message)
        self.kind = kind
        self.loc = loc
        self.expected = expected
        self.hint = hint


def _err(message, kind="semantics", loc=None, expected=None, hint=None):
    raise FslError(message, kind=kind, loc=loc, expected=expected, hint=hint)


def eval_const(e, consts, binds=None):
    """Evaluate an expression that must be a compile-time integer."""
    binds = binds or {}
    tag = e[0]
    if tag == "num":
        return e[1]
    if tag == "neg":
        return -eval_const(e[1], consts, binds)
    if tag == "var":
        if e[1] in binds:
            return binds[e[1]]
        if e[1] in consts:
            return consts[e[1]]
        _err(f"'{e[1]}' is not a constant (ranges must be compile-time integers)", kind="type")
    if tag == "bin" and e[1] in ("+", "-", "*"):
        a, b = eval_const(e[2], consts, binds), eval_const(e[3], consts, binds)
        return {"+": a + b, "-": a - b, "*": a * b}[e[1]]
    _err(f"range bound is not a compile-time integer: {e}", kind="type")


def resolve_type(ty, types):
    if ty[0] in ("int", "bool"):
        return ty
    if ty[0] == "name":
        n = ty[1]
        if n not in types:
            _err(f"unknown type '{n}'", kind="type")
        return types[n]["ty"]
    if ty[0] == "map":
        return ("map", resolve_type(ty[1], types), resolve_type(ty[2], types))
    if ty[0] == "set":
        return ("set", resolve_type(ty[1], types))
    if ty[0] == "option":
        return ("option", resolve_type(ty[1], types))
    _err(f"unknown type form {ty}", kind="type")


def is_bounded(ty, types_meta):
    """Return True if ty is a bounded (domain or enum) type."""
    if ty[0] == "domain":
        return True
    if ty[0] == "enum":
        return True
    if ty[0] == "map":
        return is_bounded(ty[1], types_meta)
    if ty[0] == "set":
        return is_bounded(ty[1], types_meta)
    return False


def domain_range(ty, types_meta):
    if ty[0] == "domain":
        return ty[1], ty[2]
    if ty[0] == "enum":
        return 0, len(types_meta[ty[1]]["members"]) - 1
    _err(f"expected bounded type, got {ty}", kind="type")


def enum_member_index(types_meta, member_name):
    for ename, info in types_meta.items():
        if info["kind"] == "enum" and member_name in info["members"]:
            return ename, info["members"].index(member_name)
    _err(f"unknown enum member '{member_name}'", kind="name")


def collect_types(items, consts):
    types_meta = {}
    enum_members = {}

    for it in items:
        if it[0] == "type":
            _, n, lo, hi = it
            lo_i, hi_i = eval_const(lo, consts, {}), eval_const(hi, consts, {})
            if lo_i > hi_i:
                _err(f"type '{n}' has empty range {lo_i}..{hi_i}", kind="type")
            types_meta[n] = {
                "kind": "domain",
                "lo": lo_i,
                "hi": hi_i,
                "ty": ("domain", lo_i, hi_i),
            }
        elif it[0] == "enum":
            _, n, members = it
            if not members:
                _err(f"enum '{n}' has no members", kind="type")
            for m in members:
                if m in enum_members:
                    _err(
                        f"enum member '{m}' is already declared in enum '{enum_members[m]}'",
                        kind="name",
                    )
                enum_members[m] = n
            types_meta[n] = {
                "kind": "enum",
                "members": list(members),
                "ty": ("enum", n),
            }
        elif it[0] == "struct":
            _, n, fields = it
            resolved = {fn: None for fn in fields}
            types_meta[n] = {"kind": "struct", "fields": fields, "ty": ("struct", n)}

    for n, info in types_meta.items():
        if info["kind"] == "struct":
            info["fields"] = {
                fn: resolve_type(ft, types_meta) for fn, ft in info["fields"].items()
            }
            for fn, fty in info["fields"].items():
                if fty[0] not in ("int", "bool", "domain", "enum"):
                    _err(
                        f"struct field '{n}.{fn}' has non-scalar type",
                        kind="type",
                        hint=(
                            "struct fields must be scalar (domain type, enum, Bool, Int) "
                            "in v1; model an optional field with an enum state, or use a separate Map"
                        ),
                    )
            info["ty"] = ("struct", n)

    return types_meta


def expand_phys_var(logical_name, ty, types_meta, out):
    """Flatten logical state variable into physical Z3-backed variables."""
    if ty[0] in ("int", "bool", "domain", "enum"):
        out.append({"phys": logical_name, "logical": logical_name, "ty": ty})
        return
    if ty[0] == "option":
        inner = ty[1]
        out.append({
            "phys": f"{logical_name}__present",
            "logical": logical_name,
            "part": "present",
            "parent": logical_name,
            "ty": ("bool",),
        })
        out.append({
            "phys": f"{logical_name}__value",
            "logical": logical_name,
            "part": "value",
            "parent": logical_name,
            "ty": inner,
        })
        return
    if ty[0] == "set":
        elem = ty[1]
        out.append({
            "phys": logical_name,
            "logical": logical_name,
            "ty": ("set", elem),
            "elem_ty": elem,
        })
        return
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        if vty[0] == "struct":
            sname = vty[1]
            for fn, fty in types_meta[sname]["fields"].items():
                out.append({
                    "phys": f"{logical_name}__{fn}",
                    "logical": logical_name,
                    "part": fn,
                    "parent": logical_name,
                    "map_key": kty,
                    "ty": ("map", kty, fty),
                })
        elif vty[0] == "option":
            inner = vty[1]
            out.append({
                "phys": f"{logical_name}__present",
                "logical": logical_name,
                "part": "present",
                "parent": logical_name,
                "map_key": kty,
                "ty": ("map", kty, ("bool",)),
            })
            out.append({
                "phys": f"{logical_name}__value",
                "logical": logical_name,
                "part": "value",
                "parent": logical_name,
                "map_key": kty,
                "ty": ("map", kty, inner),
            })
        else:
            out.append({
                "phys": logical_name,
                "logical": logical_name,
                "ty": ("map", kty, vty),
            })
        return
    if ty[0] == "struct":
        sname = ty[1]
        for fn, fty in types_meta[sname]["fields"].items():
            expand_phys_var(f"{logical_name}__{fn}", fty, types_meta, out)
        return
    _err(f"cannot expand state type {ty}", kind="type")


def z3_sort(ty, types_meta):
    if ty[0] == "int" or ty[0] == "domain" or ty[0] == "enum":
        return z3.IntSort()
    if ty[0] == "bool":
        return z3.BoolSort()
    if ty[0] == "map":
        return z3.ArraySort(z3_sort(ty[1], types_meta), z3_sort(ty[2], types_meta))
    if ty[0] == "set":
        return z3.ArraySort(z3_sort(ty[1], types_meta), z3.BoolSort())
    if ty[0] == "option":
        _err("Option cannot be a top-level Z3 sort; use lowering", kind="type")
    if ty[0] == "struct":
        _err("struct cannot be a top-level Z3 sort; use lowering", kind="type")
    _err(f"unknown type {ty}", kind="type")


def phys_z3_sort(entry, types_meta):
    ty = entry["ty"]
    if ty[0] == "set":
        return z3.ArraySort(z3_sort(ty[1], types_meta), z3.BoolSort())
    return z3_sort(ty, types_meta)


def binder_range(binder, consts, types_meta):
    if binder[0] == "binder_range":
        _, v, lo, hi = binder
        lo_i, hi_i = eval_const(lo, consts, {}), eval_const(hi, consts, {})
        return v, lo_i, hi_i, None
    _, v, ty_name, where = binder
    if ty_name not in types_meta:
        _err(f"unknown type '{ty_name}' in binder", kind="type")
    ty = types_meta[ty_name]["ty"]
    lo, hi = domain_range(ty, types_meta)
    return v, lo, hi, where


def normalize_params(params, consts, types_meta):
    out = []
    for p in params:
        if p[0] == "param_typed":
            _, n, ty_name = p
            if ty_name not in types_meta:
                _err(f"unknown parameter type '{ty_name}'", kind="type")
            ty = types_meta[ty_name]["ty"]
            lo, hi = domain_range(ty, types_meta)
            out.append((n, lo, hi, ty_name))
        else:
            _, n, lo, hi = p
            lo_i, hi_i = eval_const(lo, consts, {}), eval_const(hi, consts, {})
            out.append((n, lo_i, hi_i, None))
    return out


def normalize_action_items(items):
    requires, lets, stmts, ensures = [], [], [], []
    for it in items:
        tag = it[0]
        if tag == "requires":
            requires.append({"expr": it[1], "loc": it[2]})
        elif tag == "let":
            lets.append({"name": it[1], "expr": it[2], "loc": it[3]})
        elif tag == "ensures":
            ensures.append({"expr": it[1], "loc": it[2]})
        else:
            stmts.append(it)
    return requires, lets, stmts, ensures


def check_map_key_warnings(state, types_meta):
    """Emit Map<Int,·> deprecation hints for v1-style specs (those with domain types)."""
    if not any(info["kind"] == "domain" for info in types_meta.values()):
        return []
    warnings = []
    for n, ty in state.items():
        if ty[0] == "map" and ty[1][0] == "int":
            warnings.append({
                "message": f"Map<Int, ...> on '{n}' is deprecated; use a bounded domain type as key",
                "hint": "declare `type K = 0..<max>` and use `Map<K, ...>`",
            })
    return warnings


def bounds_invariant_expr(var_name, ty, types_meta):
    """Build AST for implicit _bounds_<var> invariant."""
    if ty[0] == "domain":
        lo, hi = ty[1], ty[2]
        return ("bin", "and", ("bin", ">=", ("var", var_name), ("num", lo)),
                ("bin", "<=", ("var", var_name), ("num", hi)))
    if ty[0] == "enum":
        lo, hi = domain_range(ty, types_meta)
        return ("bin", "and", ("bin", ">=", ("var", var_name), ("num", lo)),
                ("bin", "<=", ("var", var_name), ("num", hi)))
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        k_lo, k_hi = domain_range(kty, types_meta)
        v_body = bounds_invariant_expr("__v", vty, types_meta)
        v_body = _subst_var(v_body, "__v", ("index", var_name, ("var", "__k")))
        b = ("binder_range", "__k", ("num", k_lo), ("num", k_hi))
        return ("forall", b, v_body)
    if ty[0] == "set":
        return None
    if ty[0] == "option":
        inner = ty[1]
        present = ("var", f"{var_name}__present")
        val = ("var", f"{var_name}__value")
        inner_b = bounds_invariant_expr("__v", inner, types_meta)
        inner_b = _subst_var(inner_b, "__v", ("var", f"{var_name}__value"))
        return ("bin", "=>", present, inner_b)
    if ty[0] == "struct":
        parts = []
        sname = ty[1]
        for fn, fty in types_meta[sname]["fields"].items():
            p = bounds_invariant_expr(f"{var_name}__{fn}", fty, types_meta)
            if p is not None:
                parts.append(p)
        if not parts:
            return None
        acc = parts[0]
        for p in parts[1:]:
            acc = ("bin", "and", acc, p)
        return acc
    return None


def bounds_invariant_expr_map_field(phys_name, map_key_ty, value_ty, types_meta):
    k_lo, k_hi = domain_range(map_key_ty, types_meta)

    def value_bounds_for(vty, select_expr):
        if vty[0] in ("domain", "enum"):
            lo, hi = domain_range(vty, types_meta)
            return ("bin", "and",
                    ("bin", ">=", select_expr, ("num", lo)),
                    ("bin", "<=", select_expr, ("num", hi)))
        if vty[0] == "option":
            inner = vty[1]
            present_sel = ("index", phys_name.replace("__value", "__present"), ("var", "__k"))
            val_sel = select_expr
            inner_b = value_bounds_for(inner, val_sel)
            return ("bin", "=>", present_sel, inner_b)
        if vty[0] == "struct":
            sname = vty[1]
            parts = []
            base = phys_name.rsplit("__", 1)[0]
            for fn, fty in types_meta[sname]["fields"].items():
                sel = ("index", f"{base}__{fn}", ("var", "__k"))
                parts.append(value_bounds_for(fty, sel))
            acc = parts[0]
            for p in parts[1:]:
                acc = ("bin", "and", acc, p)
            return acc
        return ("bool", True)

    if value_ty[0] == "struct":
        sname = value_ty[1]
        parts = []
        base = phys_name.rsplit("__", 1)[0] if "__" in phys_name else phys_name
        for fn, fty in types_meta[sname]["fields"].items():
            sel = ("index", f"{base}__{fn}", ("var", "__k"))
            parts.append(value_bounds_for(fty, sel))
        body = parts[0]
        for p in parts[1:]:
            body = ("bin", "and", body, p)
    elif value_ty[0] == "option":
        present_sel = ("index", phys_name.replace("__value", "__present"), ("var", "__k"))
        val_sel = ("index", phys_name, ("var", "__k"))
        body = value_bounds_for(value_ty, val_sel)
        body = ("bin", "=>", present_sel, body) if value_ty[0] == "option" else body
    else:
        sel = ("index", phys_name, ("var", "__k"))
        body = value_bounds_for(value_ty, sel)

    b = ("binder_range", "__k", ("num", k_lo), ("num", k_hi))
    return ("forall", b, body)


def _subst_var(expr, old, new):
    tag = expr[0]
    if tag == "var" and expr[1] == old:
        return new
    if tag == "num" or tag == "bool":
        return expr
    if tag == "index":
        return ("index", expr[1], _subst_var(expr[2], old, new))
    if tag == "bin":
        return ("bin", expr[1], _subst_var(expr[2], old, new), _subst_var(expr[3], old, new))
    if tag == "not":
        return ("not", _subst_var(expr[1], old, new))
    if tag == "neg":
        return ("neg", _subst_var(expr[1], old, new))
    if tag in ("forall", "exists"):
        return (tag, expr[1], _subst_var(expr[2], old, new))
    return expr


def generate_bounds_invariants(logical_state, phys_vars, types_meta):
    invs = []
    for logical, ty in logical_state.items():
        if ty[0] in ("int", "bool", "set"):
            continue
        if ty[0] in ("domain", "enum"):
            expr = bounds_invariant_expr(logical, ty, types_meta)
        elif ty[0] == "option":
            expr = bounds_invariant_expr(logical, ty, types_meta)
        elif ty[0] == "struct":
            expr = bounds_invariant_expr(logical, ty, types_meta)
        elif ty[0] == "map":
            kty, vty = ty[1], ty[2]
            if kty[0] == "int":
                continue
            if vty[0] in ("int", "bool"):
                continue
            if vty[0] == "struct":
                parts = []
                for fn in types_meta[vty[1]]["fields"]:
                    pexpr = bounds_invariant_expr_map_field(
                        f"{logical}__{fn}", kty, vty, types_meta)
                    parts.append(pexpr)
                expr = parts[0]
                for p in parts[1:]:
                    expr = ("bin", "and", expr, p)
            elif vty[0] == "option":
                expr = bounds_invariant_expr_map_field(
                    f"{logical}__value", kty, vty, types_meta)
            else:
                expr = bounds_invariant_expr_map_field(logical, kty, vty, types_meta)
        else:
            continue
        if expr:
            invs.append({
                "name": f"_bounds_{logical}",
                "expr": expr,
                "implicit": True,
                "loc": None,
                "logical_var": logical,
            })
    return invs


def build_spec(tree):
    _, name, items = tree
    consts = {}
    for it in items:
        if it[0] == "const":
            consts[it[1]] = eval_const(it[2], consts, {})

    types_meta = collect_types(items, consts)

    state = {}
    init = []
    actions = []
    invariants = []
    reachables = []

    for it in items:
        tag = it[0]
        if tag == "state":
            for _, n, ty_ast in it[1]:
                if n in state:
                    _err(f"duplicate state variable '{n}'", kind="name")
                state[n] = resolve_type(ty_ast, types_meta)
        elif tag == "init":
            init = it[1]
        elif tag == "action":
            aname, params, body_items, loc = it[1], it[2], it[3], it[4]
            nparams = normalize_params(params, consts, types_meta)
            requires, lets, stmts, ensures = normalize_action_items(body_items)
            actions.append({
                "name": aname,
                "params": nparams,
                "requires": requires,
                "lets": lets,
                "stmts": stmts,
                "ensures": ensures,
                "loc": loc,
            })
        elif tag == "invariant":
            invariants.append({
                "name": it[1],
                "expr": it[2],
                "implicit": False,
                "loc": it[3],
            })
        elif tag == "reachable":
            reachables.append({
                "name": it[1],
                "expr": it[2],
                "loc": it[3],
            })

    if not state:
        _err("spec has no state block", kind="semantics")

    phys_vars = []
    for n, ty in state.items():
        expand_phys_var(n, ty, types_meta, phys_vars)

    phys_names = [p["phys"] for p in phys_vars]
    if len(phys_names) != len(set(phys_names)):
        _err("internal error: duplicate physical state names", kind="internal")

    bounds_invs = generate_bounds_invariants(state, phys_vars, types_meta)
    all_invariants = bounds_invs + invariants

    warnings = check_map_key_warnings(state, types_meta)
    if not invariants:
        warnings.append({
            "message": "spec declares no user invariants (only implicit type bounds are checked)",
        })

    if not actions:
        _err("spec has no actions", kind="semantics")

    return {
        "name": name,
        "consts": consts,
        "types": types_meta,
        "state": state,
        "phys_vars": phys_vars,
        "init": init,
        "actions": actions,
        "invariants": all_invariants,
        "user_invariants": invariants,
        "reachables": reachables,
        "warnings": warnings,
    }


def check_spec(tree):
    """Syntax/name/type check only; returns result dict for fslc check."""
    spec = build_spec(tree)
    return {
        "result": "ok",
        "spec": spec["name"],
        "warnings": spec["warnings"],
    }
