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


_STATE_TYPE_HINT = (
    "v1 state variables may use: scalar (Int, Bool, domain, enum), "
    "Option<scalar>, struct (scalar or Option<scalar> fields only), "
    "Map<bounded-scalar, scalar | Option<scalar> | struct>, "
    "Set<bounded-scalar>, or Seq<scalar, N>"
)


def is_scalar_type(ty):
    return ty[0] in ("int", "bool", "domain", "enum")


def is_bounded_scalar_type(ty):
    return ty[0] in ("domain", "enum")


def is_option_scalar_type(ty):
    return ty[0] == "option" and is_scalar_type(ty[1])


def is_struct_field_type(ty):
    return is_scalar_type(ty) or is_option_scalar_type(ty)


def resolve_seq_capacity(cap_ast, consts):
    cap = eval_const(cap_ast, consts, {})
    if cap <= 0:
        _err(f"Seq capacity must be a positive integer, got {cap}", kind="type")
    return cap


def resolve_type(ty, types, consts=None):
    if ty[0] in ("int", "bool"):
        return ty
    if ty[0] == "name":
        n = ty[1]
        if n not in types:
            _err(f"unknown type '{n}'", kind="type")
        return types[n]["ty"]
    if ty[0] == "map":
        return ("map", resolve_type(ty[1], types, consts), resolve_type(ty[2], types, consts))
    if ty[0] == "set":
        return ("set", resolve_type(ty[1], types, consts))
    if ty[0] == "seq":
        if consts is None:
            _err("internal error: Seq capacity requires consts", kind="internal")
        elem = resolve_type(ty[1], types, consts)
        cap = resolve_seq_capacity(ty[2], consts)
        return ("seq", elem, cap)
    if ty[0] == "option":
        return ("option", resolve_type(ty[1], types, consts))
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
                fn: resolve_type(ft, types_meta, consts) for fn, ft in info["fields"].items()
            }
            for fn, fty in info["fields"].items():
                if not is_struct_field_type(fty):
                    _err(
                        f"struct field '{n}.{fn}' has non-scalar type",
                        kind="type",
                        hint=(
                            "struct fields must be scalar (domain type, enum, Bool, Int) "
                            "or Option<scalar>; use a separate Map for Set/Map/Seq/struct fields"
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
    if ty[0] == "seq":
        elem, cap = ty[1], ty[2]
        idx_ty = ("domain", 0, cap - 1)
        out.append({
            "phys": f"{logical_name}__data",
            "logical": logical_name,
            "part": "data",
            "parent": logical_name,
            "ty": ("map", idx_ty, elem),
            "seq_cap": cap,
            "elem_ty": elem,
        })
        out.append({
            "phys": f"{logical_name}__len",
            "logical": logical_name,
            "part": "len",
            "parent": logical_name,
            "ty": ("int",),
            "seq_cap": cap,
        })
        return
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        if vty[0] == "struct":
            sname = vty[1]
            for fn, fty in types_meta[sname]["fields"].items():
                if fty[0] == "option":
                    inner = fty[1]
                    out.append({
                        "phys": f"{logical_name}__{fn}__present",
                        "logical": logical_name,
                        "part": f"{fn}__present",
                        "parent": logical_name,
                        "field": fn,
                        "option_part": "present",
                        "map_key": kty,
                        "ty": ("map", kty, ("bool",)),
                    })
                    out.append({
                        "phys": f"{logical_name}__{fn}__value",
                        "logical": logical_name,
                        "part": f"{fn}__value",
                        "parent": logical_name,
                        "field": fn,
                        "option_part": "value",
                        "map_key": kty,
                        "ty": ("map", kty, inner),
                    })
                else:
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
            if fty[0] == "option":
                inner = fty[1]
                out.append({
                    "phys": f"{logical_name}__{fn}__present",
                    "logical": logical_name,
                    "part": f"{fn}__present",
                    "parent": logical_name,
                    "field": fn,
                    "option_part": "present",
                    "ty": ("bool",),
                })
                out.append({
                    "phys": f"{logical_name}__{fn}__value",
                    "logical": logical_name,
                    "part": f"{fn}__value",
                    "parent": logical_name,
                    "field": fn,
                    "option_part": "value",
                    "ty": inner,
                })
            else:
                out.append({
                    "phys": f"{logical_name}__{fn}",
                    "logical": logical_name,
                    "part": fn,
                    "parent": logical_name,
                    "ty": fty,
                })
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
    if ty[0] == "seq":
        cap = ty[2]
        len_var = f"{var_name}__len"
        data_var = f"{var_name}__data"
        len_bounds = ("bin", "and",
                      ("bin", ">=", ("var", len_var), ("num", 0)),
                      ("bin", "<=", ("var", len_var), ("num", cap)))
        elem_ty = ty[1]
        if elem_ty[0] in ("domain", "enum"):
            lo, hi = domain_range(elem_ty, types_meta)
            elem_b = ("bin", "and",
                      ("bin", ">=", ("index", data_var, ("var", "__k")), ("num", lo)),
                      ("bin", "<=", ("index", data_var, ("var", "__k")), ("num", hi)))
            idx_body = ("bin", "=>", ("bin", "<", ("var", "__k"), ("var", len_var)), elem_b)
            b = ("binder_range", "__k", ("num", 0), ("num", cap - 1))
            data_bounds = ("forall", b, idx_body)
            return ("bin", "and", len_bounds, data_bounds)
        return len_bounds
    if ty[0] == "option":
        inner = ty[1]
        present = ("var", f"{var_name}__present")
        inner_b = bounds_invariant_expr("__v", inner, types_meta)
        if inner_b is None:
            return None
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

    def scalar_bounds(vty, select_expr):
        if vty[0] in ("domain", "enum"):
            lo, hi = domain_range(vty, types_meta)
            return ("bin", "and",
                    ("bin", ">=", select_expr, ("num", lo)),
                    ("bin", "<=", select_expr, ("num", hi)))
        return None

    def value_bounds_for(vty, phys_base):
        if vty[0] in ("domain", "enum"):
            return scalar_bounds(vty, ("index", phys_base, ("var", "__k")))
        if vty[0] == "option":
            inner = vty[1]
            inner_b = scalar_bounds(inner, ("index", f"{phys_base}__value", ("var", "__k")))
            if inner_b is None:
                return None
            present_sel = ("index", f"{phys_base}__present", ("var", "__k"))
            return ("bin", "=>", present_sel, inner_b)
        if vty[0] == "struct":
            sname = vty[1]
            parts = []
            for fn, fty in types_meta[sname]["fields"].items():
                p = value_bounds_for(fty, f"{phys_base}__{fn}")
                if p is not None:
                    parts.append(p)
            if not parts:
                return None
            acc = parts[0]
            for p in parts[1:]:
                acc = ("bin", "and", acc, p)
            return acc
        return None

    body = value_bounds_for(value_ty, phys_name)
    if body is None:
        return None

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


def _validate_map_value_type(vty, types_meta, path):
    if is_scalar_type(vty):
        return
    if vty[0] == "option":
        if not is_scalar_type(vty[1]):
            _err(
                f"{path}: Map value type Option<{vty[1][0]}> is not allowed",
                kind="type",
                hint=_STATE_TYPE_HINT,
            )
        return
    if vty[0] == "struct":
        sname = vty[1]
        for fn, fty in types_meta[sname]["fields"].items():
            if not is_struct_field_type(fty):
                _err(
                    f"{path}: struct field '{sname}.{fn}' has non-scalar type",
                    kind="type",
                    hint=_STATE_TYPE_HINT,
                )
        return
    _err(f"{path}: illegal Map value type", kind="type", hint=_STATE_TYPE_HINT)


def _resolve_lvalue_seq_capacity(lv, state, types_meta):
    """Return Seq capacity for an assign target, or None if not a Seq assignment."""
    if lv[0] == "var":
        ty = state.get(lv[1])
        if ty and ty[0] == "seq":
            return ty[2]
        return None
    if lv[0] == "field_lv":
        base, field = lv[1], lv[2]
        if base[0] != "var":
            return None
        ty = state.get(base[1])
        if ty and ty[0] == "struct":
            fty = types_meta[ty[1]]["fields"].get(field)
            if fty and fty[0] == "seq":
                return fty[2]
        return None
    if lv[0] == "index":
        ty = state.get(lv[1])
        if ty and ty[0] == "map":
            vty = ty[2]
            if vty[0] == "seq":
                return vty[2]
        return None
    return None


def _check_seq_literals_in_stmts(stmts, state, types_meta):
    for st in stmts:
        if st[0] == "assign":
            lv, rhs = st[1], st[2]
            if rhs[0] == "seq_lit":
                cap = _resolve_lvalue_seq_capacity(lv, state, types_meta)
                if cap is not None and len(rhs[1]) > cap:
                    _err(
                        f"Seq literal has {len(rhs[1])} elements but capacity is {cap}",
                        kind="type",
                        loc=st[3] if len(st) > 3 else None,
                        hint=_STATE_TYPE_HINT,
                    )
        elif st[0] == "if":
            _check_seq_literals_in_stmts(st[2], state, types_meta)
            _check_seq_literals_in_stmts(st[3], state, types_meta)
        elif st[0] == "forall_stmt":
            _check_seq_literals_in_stmts(st[2], state, types_meta)


def check_seq_literal_sizes(state, init, actions, types_meta):
    _check_seq_literals_in_stmts(init, state, types_meta)
    for act in actions:
        _check_seq_literals_in_stmts(act["stmts"], state, types_meta)


def validate_state_var_type(ty, types_meta, path):
    """Whitelist validation for state variable types (DESIGN-seq §7)."""
    if is_scalar_type(ty):
        return
    if ty[0] == "option":
        if not is_scalar_type(ty[1]):
            _err(f"{path}: Option element must be scalar", kind="type", hint=_STATE_TYPE_HINT)
        return
    if ty[0] == "struct":
        sname = ty[1]
        for fn, fty in types_meta[sname]["fields"].items():
            if not is_struct_field_type(fty):
                _err(
                    f"{path}: struct field '{sname}.{fn}' has non-scalar type",
                    kind="type",
                    hint=_STATE_TYPE_HINT,
                )
        return
    if ty[0] == "map":
        kty, vty = ty[1], ty[2]
        if kty[0] == "int":
            _validate_map_value_type(vty, types_meta, path)
            return
        if not is_bounded_scalar_type(kty):
            _err(
                f"{path}: Map key must be a bounded scalar (domain or enum)",
                kind="type",
                hint=_STATE_TYPE_HINT,
            )
        _validate_map_value_type(vty, types_meta, path)
        return
    if ty[0] == "set":
        if not is_bounded_scalar_type(ty[1]):
            _err(
                f"{path}: Set element must be a bounded scalar (domain or enum)",
                kind="type",
                hint=_STATE_TYPE_HINT,
            )
        return
    if ty[0] == "seq":
        if not is_scalar_type(ty[1]):
            _err(
                f"{path}: Seq element must be scalar",
                kind="type",
                hint=_STATE_TYPE_HINT,
            )
        return
    _err(f"{path}: illegal state variable type", kind="type", hint=_STATE_TYPE_HINT)


def generate_bounds_invariants(logical_state, phys_vars, types_meta):
    invs = []
    for logical, ty in logical_state.items():
        if ty[0] in ("int", "bool", "set", "seq"):
            if ty[0] == "seq":
                expr = bounds_invariant_expr(logical, ty, types_meta)
                if expr:
                    invs.append({
                        "name": f"_bounds_{logical}",
                        "expr": expr,
                        "implicit": True,
                        "loc": None,
                        "logical_var": logical,
                    })
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
                expr = bounds_invariant_expr_map_field(logical, kty, vty, types_meta)
            elif vty[0] == "option":
                expr = bounds_invariant_expr_map_field(
                    logical, kty, vty, types_meta)
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


def display_label(name, spec):
    """Map physical compose-prefixed identifiers to logical display labels."""
    if not name or not isinstance(name, str):
        return name
    dn = spec.get("display_names") or {}
    if name in dn:
        return dn[name]
    for prefix in ("_bounds_", "_partial_"):
        if name.startswith(prefix):
            inner = name[len(prefix):]
            if inner in dn:
                return f"{prefix}{dn[inner]}"
    return name


def resolve_action_name(name, spec):
    """Resolve a display action label back to the physical action name."""
    dn = spec.get("display_names") or {}
    for phys, disp in dn.items():
        if disp == name:
            return phys
    return name


def display_keyed(mapping, spec):
    if not mapping:
        return mapping
    return {display_label(k, spec): v for k, v in mapping.items()}


def _with_meta(entry, meta):
    entry["meta"] = meta
    return entry


def build_spec(tree, display_names=None):
    _, name, items = tree
    dialect_display_names = {}
    dialect_implements = None
    dialect_acceptance = []
    dialect_forbidden = []
    dialect_action_aliases = {}
    for it in items:
        if it[0] == "__display_names":
            dialect_display_names.update(it[1])
        elif it[0] == "__implements":
            dialect_implements = it[1]
        elif it[0] == "__acceptance":
            dialect_acceptance.extend(it[1])
        elif it[0] == "__forbidden":
            dialect_forbidden.extend(it[1])
        elif it[0] == "__action_aliases":
            dialect_action_aliases.update(it[1])

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
    leadstos = []

    for it in items:
        tag = it[0]
        if tag == "state":
            for _, n, ty_ast in it[1]:
                if n in state:
                    _err(f"duplicate state variable '{n}'", kind="name")
                state[n] = resolve_type(ty_ast, types_meta, consts)
        elif tag == "init":
            init = it[1]
        elif tag == "action":
            aname, params, body_items, loc = it[1], it[2], it[3], it[4]
            fair = it[5] if len(it) > 5 else False
            meta = it[6] if len(it) > 6 else None
            sync = bool(it[7]) if len(it) > 7 else False
            nparams = normalize_params(params, consts, types_meta)
            requires, lets, stmts, ensures = normalize_action_items(body_items)
            actions.append(_with_meta({
                "name": aname,
                "params": nparams,
                "requires": requires,
                "lets": lets,
                "stmts": stmts,
                "ensures": ensures,
                "loc": loc,
                "fair": bool(fair),
                "sync": sync,
            }, meta))
        elif tag == "leadsto":
            leadstos.append(_with_meta({
                "name": it[1],
                "binders": it[2],
                "P": it[3],
                "Q": it[4],
                "loc": it[5],
            }, it[6] if len(it) > 6 else None))
        elif tag == "invariant":
            invariants.append(_with_meta({
                "name": it[1],
                "expr": it[2],
                "implicit": False,
                "loc": it[3],
            }, it[4] if len(it) > 4 else None))
        elif tag == "reachable":
            reachables.append(_with_meta({
                "name": it[1],
                "expr": it[2],
                "loc": it[3],
            }, it[4] if len(it) > 4 else None))

    if not state:
        _err("spec has no state block", kind="semantics")

    for n, ty in state.items():
        validate_state_var_type(ty, types_meta, f"state variable '{n}'")

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

    check_seq_literal_sizes(state, init, actions, types_meta)

    all_display_names = dict(display_names or {})
    all_display_names.update(dialect_display_names)

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
        "leadstos": leadstos,
        "warnings": warnings,
        "display_names": all_display_names,
        "implements": dialect_implements,
        "acceptance": dialect_acceptance,
        "forbidden": dialect_forbidden,
        "action_aliases": dialect_action_aliases,
    }


def check_spec(tree, display_names=None):
    """Syntax/name/type check only; returns result dict for fslc check."""
    spec = build_spec(tree, display_names)
    return {
        "result": "ok",
        "spec": spec["name"],
        "warnings": spec["warnings"],
    }
