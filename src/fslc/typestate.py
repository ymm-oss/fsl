# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Typestate derivation — judge where a design spec's state machine is soundly
expressible as host-language phantom types, and emit a TypeScript skeleton.

The judgment is the product. For each (entity, action) we decide:

  derivable  — the from-state is a LOCAL guard on the entity's own status field
               (`requires e.status == S`) and the to-state is a local assignment.
               The runtime guard becomes a compile-time type, soundly.
  branching  — the to-state is assigned only inside an `if` (data-dependent).
               Emitted, but flagged: the impl must prove exhaustiveness.
  relational — the action assigns the status but no local guard pins the
               from-state on the SAME entity instance. The precondition lives
               elsewhere (a queue, another structure); a phantom tag cannot carry
               it. Refused, with a diagnostic tied to the action's requirement id.

Two machine shapes are recognised: an enum-valued struct field, and an
`Option<_>` slot (none/some ≈ Empty/Filled). Applicability is `full` only when
EVERY transition of the entity is derivable (or branching) — never when a
transition was simply not understood.
"""
from __future__ import annotations

RESERVED_TS = {"void", "delete", "new", "default", "function", "return",
               "switch", "case", "class", "enum", "interface", "type"}


# --------------------------------------------------------------------------
# expression rendering / structural keys
# --------------------------------------------------------------------------

def expr_src(e):
    if not isinstance(e, tuple):
        return str(e)
    t = e[0]
    if t == "var":
        return e[1]
    if t == "num":
        return str(e[1])
    if t == "bool":
        return "true" if e[1] else "false"
    if t == "none":
        return "none"
    if t == "bin":
        return f"({expr_src(e[2])} {e[1]} {expr_src(e[3])})"
    if t == "not":
        return f"not {expr_src(e[1])}"
    if t == "neg":
        return f"-{expr_src(e[1])}"
    if t == "field":
        return f"{expr_src(e[1])}.{e[2]}"
    if t == "field_lv":
        return f"{expr_src(e[1])}.{e[2]}"
    if t == "index":
        return f"{expr_src(e[1])}[{expr_src(e[2])}]"
    if t == "method":
        return f"{expr_src(e[1])}.{e[2]}(...)"
    if t == "some":
        return f"some({expr_src(e[1])})"
    if t == "is":
        pat = e[2]
        return f"{expr_src(e[1])} is {'none' if pat[0] == 'pat_none' else 'some(...)'}"
    return t


def _norm_index(node):
    """An lvalue index carries a bare base name ('index','orders',k); normalise
    to the expr form ('index',('var','orders'),k) so guard and assignment refs
    compare equal."""
    if isinstance(node, tuple) and node[0] == "index" and isinstance(node[1], str):
        return ("index", ("var", node[1]), node[2])
    return node


def _base_var(node):
    if not isinstance(node, tuple):
        return None
    if node[0] == "var":
        return node[1]
    if node[0] == "index":
        b = node[1]
        return b if isinstance(b, str) else _base_var(b)
    if node[0] in ("field", "field_lv"):
        return _base_var(node[1])
    return None


# --------------------------------------------------------------------------
# enum-field state machines
# --------------------------------------------------------------------------

# An enum machine's "status location" is matched two ways:
#   field-based  — a struct field `<obj>.status`; the entity instance is <obj>.
#   var-based    — an enum-typed state var `v` or map element `v[k]`; the instance
#                  is that location itself (business `process`/stage expands to this).

def _field_loc(field):
    def match(node):
        if isinstance(node, tuple) and node[0] in ("field", "field_lv") and node[2] == field:
            return expr_src(_norm_index(node[1]))
        return None
    return match


def _var_loc(vname):
    def match(node):
        if (isinstance(node, tuple) and node[0] in ("var", "index")
                and _base_var(node) == vname):
            return expr_src(_norm_index(node))
        return None
    return match


def _enum_guard_states(expr, match, members):
    """{entity_ref: {enum vals}} asserted `== E` (over `or`/`and`)."""
    out = {}
    if not isinstance(expr, tuple):
        return out
    if expr[0] == "bin" and expr[1] in ("or", "and"):
        for sub in (expr[2], expr[3]):
            for ref, vs in _enum_guard_states(sub, match, members).items():
                out.setdefault(ref, set()).update(vs)
    elif expr[0] == "bin" and expr[1] == "==":
        for a, b in ((expr[2], expr[3]), (expr[3], expr[2])):
            ref = match(a)
            if ref is not None and isinstance(b, tuple) and b[0] == "var" and b[1] in members:
                out.setdefault(ref, set()).add(b[1])
    return out


def _copy_state_map(states):
    return {ref: set(vs) for ref, vs in states.items()}


def _and_path_states(base, constraint):
    out = _copy_state_map(base)
    for ref, vs in constraint.items():
        if ref in out:
            out[ref].intersection_update(vs)
        else:
            out[ref] = set(vs)
    return out


def _enum_assignments(stmts, match, members, field):
    """[(entity_ref, enum_val, guarded, branch_states)] for status assignments."""
    out = []

    def walk(stmt, guarded, branch_states):
        if not isinstance(stmt, tuple):
            return
        if stmt[0] == "assign":
            lv, rhs = stmt[1], stmt[2]
            ref = match(lv)
            # <status location> = Enum
            if ref is not None and isinstance(rhs, tuple) and rhs[0] == "var" and rhs[1] in members:
                out.append((ref, rhs[1], guarded, _copy_state_map(branch_states)))
            # whole-entity struct literal  x = Struct { field: Enum, ... }  (field-based only)
            elif (field is not None and isinstance(rhs, tuple) and rhs[0] == "struct_lit"
                  and field in rhs[2] and lv[0] in ("var", "index")):
                fv = rhs[2][field]
                if isinstance(fv, tuple) and fv[0] == "var" and fv[1] in members:
                    out.append((expr_src(_norm_index(lv)), fv[1], guarded,
                                _copy_state_map(branch_states)))
        elif stmt[0] == "if":
            cond_states = _enum_guard_states(stmt[1], match, members)
            then_states = _and_path_states(branch_states, cond_states)

            else_cond = {}
            if _enum_is_status_only(stmt[1], match):
                for ref, vs in cond_states.items():
                    else_cond[ref] = set(members) - set(vs)
            else_states = _and_path_states(branch_states, else_cond)

            for s in stmt[2]:
                walk(s, True, then_states)
            for s in stmt[3]:
                walk(s, True, else_states)
        elif stmt[0] == "forall_stmt":
            for s in stmt[2]:
                walk(s, guarded, branch_states)

    for s in stmts:
        walk(s, False, {})
    return out


def _enum_is_status_only(expr, match):
    if not isinstance(expr, tuple):
        return False
    if expr[0] == "bin" and expr[1] == "and":
        return _enum_is_status_only(expr[2], match) and _enum_is_status_only(expr[3], match)
    if expr[0] == "bin" and expr[1] == "or":
        return _enum_is_status_only(expr[2], match) and _enum_is_status_only(expr[3], match)
    if expr[0] == "bin" and expr[1] == "==":
        return match(expr[2]) is not None or match(expr[3]) is not None
    return False


# --------------------------------------------------------------------------
# Option (none/some) state machines
# --------------------------------------------------------------------------

EMPTY, FILLED = "Empty", "Filled"


def _opt_guard_states(expr, vname):
    out = {}
    if not isinstance(expr, tuple):
        return out
    if expr[0] == "bin" and expr[1] in ("or", "and"):
        for sub in (expr[2], expr[3]):
            for ref, vs in _opt_guard_states(sub, vname).items():
                out.setdefault(ref, set()).update(vs)
    elif expr[0] == "bin" and expr[1] in ("==", "!="):
        for a, b in ((expr[2], expr[3]), (expr[3], expr[2])):
            if _base_var(a) == vname and isinstance(b, tuple) and b[0] == "none":
                state = EMPTY if expr[1] == "==" else FILLED
                out.setdefault(expr_src(_norm_index(a)), set()).add(state)
    elif expr[0] == "is":
        obj, pat = expr[1], expr[2]
        if _base_var(obj) == vname:
            state = EMPTY if pat[0] == "pat_none" else FILLED
            out.setdefault(expr_src(_norm_index(obj)), set()).add(state)
    return out


def _opt_assignments(stmts, vname):
    out = []

    def walk(stmt, guarded, branch_states):
        if not isinstance(stmt, tuple):
            return
        if stmt[0] == "assign":
            lv, rhs = stmt[1], stmt[2]
            if _base_var(lv) == vname and lv[0] in ("var", "index"):
                if isinstance(rhs, tuple) and rhs[0] == "none":
                    out.append((expr_src(_norm_index(lv)), EMPTY, guarded,
                                _copy_state_map(branch_states)))
                elif isinstance(rhs, tuple) and rhs[0] == "some":
                    out.append((expr_src(_norm_index(lv)), FILLED, guarded,
                                _copy_state_map(branch_states)))
        elif stmt[0] == "if":
            cond_states = _opt_guard_states(stmt[1], vname)
            then_states = _and_path_states(branch_states, cond_states)

            else_cond = {}
            if _opt_is_state_only(stmt[1], vname):
                for ref, vs in cond_states.items():
                    else_cond[ref] = {EMPTY, FILLED} - set(vs)
            else_states = _and_path_states(branch_states, else_cond)

            for s in stmt[2]:
                walk(s, True, then_states)
            for s in stmt[3]:
                walk(s, True, else_states)
        elif stmt[0] == "forall_stmt":
            for s in stmt[2]:
                walk(s, guarded, branch_states)

    for s in stmts:
        walk(s, False, {})
    return out


def _opt_is_state_only(expr, vname):
    if not isinstance(expr, tuple):
        return False
    if expr[0] == "bin" and expr[1] == "and":
        return _opt_is_state_only(expr[2], vname) and _opt_is_state_only(expr[3], vname)
    if expr[0] == "bin" and expr[1] == "or":
        return _opt_is_state_only(expr[2], vname) and _opt_is_state_only(expr[3], vname)
    if expr[0] == "bin" and expr[1] in ("==", "!="):
        return ((_base_var(expr[2]) == vname and expr[3] == ("none",))
                or (_base_var(expr[3]) == vname and expr[2] == ("none",)))
    if expr[0] == "is":
        return _base_var(expr[1]) == vname
    return False


# --------------------------------------------------------------------------
# shared verdict
# --------------------------------------------------------------------------

def _classify(action, guard_fn, assign_fn, only_fn):
    """Return a per-action verdict dict, or None if the action does not transition
    this entity. guard_fn/assign_fn/only_fn close over (field|vname, members)."""
    requires = [r["expr"] for r in action.get("requires", [])]
    assigns = assign_fn(action.get("stmts", []))
    if not assigns:
        return None

    guards = {}
    for r in requires:
        for ref, vs in guard_fn(r).items():
            guards.setdefault(ref, set()).update(vs)

    transitions, verdict, diagnostics = [], "derivable", []
    for assignment in assigns:
        if len(assignment) == 3:
            ref, to_state, guarded = assignment
            branch_states = {}
        else:
            ref, to_state, guarded, branch_states = assignment

        require_froms = set(guards.get(ref, set()))
        branch_froms = set(branch_states.get(ref, set()))
        if require_froms and branch_froms:
            from_set = require_froms & branch_froms
        else:
            from_set = require_froms | branch_froms
        froms = sorted(from_set)
        if not froms:
            verdict = "relational"
            diagnostics.append(
                f"assigns `{ref} → {to_state}` but no local `requires` pins its "
                f"from-state; the precondition is relational (it lives outside the "
                f"entity), so it cannot be carried by a phantom type and remains a "
                f"runtime/verification obligation."
            )
        elif guarded and verdict != "relational":
            verdict = "branching"
            diagnostics.append(f"`{ref} → {to_state}` is inside an `if` (data-dependent target).")
        transitions.append({"entity": ref, "from": froms, "to": to_state, "conditional": guarded})

    value_pre = [expr_src(r) for r in requires if not only_fn(r)]
    out = {
        "action": action["name"],
        "verdict": verdict,
        "params": [p[0] for p in action.get("params", [])],
        "transitions": transitions,
        "value_preconditions": value_pre,
    }
    if action.get("meta"):
        out["requirement"] = action["meta"]
    if diagnostics:
        out["diagnostics"] = diagnostics
    return out


# --------------------------------------------------------------------------
# TypeScript emission
# --------------------------------------------------------------------------

def _ts_type(typ):
    if not isinstance(typ, tuple):
        return "unknown"
    t = typ[0]
    if t in ("domain", "int"):
        return "number"
    if t == "bool":
        return "boolean"
    if t == "enum":
        return typ[1]
    if t == "option":
        return f"{_ts_type(typ[1])} | null"
    if t == "set":
        return f"Set<{_ts_type(typ[1])}>"
    if t == "struct":
        return typ[1]
    return "unknown"


def _emit_ts(spec_name, type_name, states, data_fields, actions, note):
    state_t = type_name + "State"
    L = [
        f"// Typestate skeleton for `{type_name}` from spec `{spec_name}`.",
        f"// {note}",
        f"// Only transitions with a LOCAL from-state guard are typed; the rest stay dynamic.",
        "",
        f"export type {state_t} = " + " | ".join(f'"{s}"' for s in states) + ";",
        "",
        "declare const __state: unique symbol;",
        f"export interface {type_name}<S extends {state_t}> {{",
    ]
    for fn, ft in data_fields:
        L.append(f"  {fn}: {_ts_type(ft)};")
    L += [f"  readonly [__state]: S;", "}", ""]

    for a in actions:
        if a["verdict"] not in ("derivable", "branching"):
            continue
        froms = sorted({f for tr in a["transitions"] for f in tr["from"]})
        tos = sorted({tr["to"] for tr in a["transitions"]})
        from_t = " | ".join(f'"{f}"' for f in froms) or state_t
        to_t = " | ".join(f'"{t}"' for t in tos) or from_t
        fn = a["action"] + ("_" if a["action"] in RESERVED_TS else "")
        extra = ", ".join(f"{p}: number" for p in a["params"])
        extra = (", " + extra) if extra else ""
        if a["value_preconditions"]:
            L.append(f"  // runtime precondition (not in type): {'; '.join(a['value_preconditions'])}")
        if a["verdict"] == "branching":
            L.append("  // branching: to-state is data-dependent; verify exhaustiveness at the impl")
        L.append(f"export function {fn}(self: {type_name}<{from_t}>{extra}): {type_name}<{to_t}>;")
    return "\n".join(L)


# --------------------------------------------------------------------------
# entity discovery + top-level analysis
# --------------------------------------------------------------------------

def _applicability(actions):
    if not actions:
        return "none"
    derivable = sum(1 for a in actions if a["verdict"] == "derivable")
    relational = sum(1 for a in actions if a["verdict"] == "relational")
    if relational == 0:
        return "full"
    if derivable == 0 and all(a["verdict"] == "relational" for a in actions):
        return "none"
    return "partial"


def _camel(name):
    return "".join(p[:1].upper() + p[1:] for p in name.split("_"))


def _enum_entities(spec):
    out = []
    # field-based: a struct field typed as an enum
    for tname, tinfo in spec["types"].items():
        if tinfo.get("kind") != "struct":
            continue
        for fname, ftyp in tinfo["fields"].items():
            if isinstance(ftyp, tuple) and ftyp[0] == "enum":
                ename = ftyp[1]
                data_fields = [(f, t) for f, t in tinfo["fields"].items() if f != fname]
                out.append({
                    "kind": "enum", "type_name": tname, "field": fname,
                    "var": None, "enum": ename,
                    "states": spec["types"][ename]["members"], "data_fields": data_fields,
                })
    # var-based: an enum-typed state var, scalar or map-valued (business `process`/stages)
    for vname, vtyp in spec["state"].items():
        et = None
        if isinstance(vtyp, tuple) and vtyp[0] == "enum":
            et = vtyp[1]
        elif isinstance(vtyp, tuple) and vtyp[0] == "map" and isinstance(vtyp[2], tuple) and vtyp[2][0] == "enum":
            et = vtyp[2][1]
        if et is None:
            continue
        out.append({
            "kind": "enum", "type_name": _camel(vname), "field": None,
            "var": vname, "enum": et,
            "states": spec["types"][et]["members"], "data_fields": [],
        })
    return out


def _option_entities(spec):
    out = []
    for vname, vtyp in spec["state"].items():
        inner = None
        if isinstance(vtyp, tuple) and vtyp[0] == "option":
            inner = vtyp[1]
        elif isinstance(vtyp, tuple) and vtyp[0] == "map" and isinstance(vtyp[2], tuple) and vtyp[2][0] == "option":
            inner = vtyp[2][1]
        if inner is None:
            continue
        tname = vname[:1].upper() + vname[1:]
        out.append({
            "kind": "option", "type_name": tname, "var": vname,
            "states": [EMPTY, FILLED],
            "data_fields": [("value", ("option", inner))],
        })
    return out


def analyze(spec):
    """Return a typestate report for a built spec dict."""
    report = {"result": "typestate", "spec": spec["name"], "entities": []}

    for ent in _enum_entities(spec) + _option_entities(spec):
        if ent["kind"] == "enum":
            field, members = ent["field"], ent["states"]
            match = _field_loc(field) if field is not None else _var_loc(ent["var"])
            guard_fn = lambda e, m=match, mem=members: _enum_guard_states(e, m, mem)
            assign_fn = lambda ss, m=match, mem=members, f=field: _enum_assignments(ss, m, mem, f)
            only_fn = lambda e, m=match: _enum_is_status_only(e, m)
            key = f"{ent['type_name']}.{field}" if field is not None else f"{ent['var']} ({ent['enum']})"
            note = (f"FSL holds these in a collection; phantom types track one "
                    f"entity, so each becomes an independently-typed handle.")
        else:
            vname = ent["var"]
            guard_fn = lambda e, v=vname: _opt_guard_states(e, v)
            assign_fn = lambda ss, v=vname: _opt_assignments(ss, v)
            only_fn = lambda e, v=vname: _opt_is_state_only(e, v)
            key = f"{ent['var']} (Option)"
            note = f"`{vname}` is an Option slot; states are Empty (none) / Filled (some)."

        actions = []
        for a in spec["actions"]:
            v = _classify(a, guard_fn, assign_fn, only_fn)
            if v:
                actions.append(v)
        if not actions:
            continue

        report["entities"].append({
            "entity": key,
            "kind": ent["kind"],
            "enum": ent.get("enum"),
            "states": ent["states"],
            "applicability": _applicability(actions),
            "actions": actions,
            "typescript": _emit_ts(spec["name"], ent["type_name"], ent["states"],
                                   ent["data_fields"], actions, note),
        })

    n = len(report["entities"])
    report["summary"] = {
        "entities": n,
        "full": sum(1 for e in report["entities"] if e["applicability"] == "full"),
        "partial": sum(1 for e in report["entities"] if e["applicability"] == "partial"),
        "none": sum(1 for e in report["entities"] if e["applicability"] == "none"),
    }
    if n == 0:
        report["note"] = "no enum-field or Option state machine found — nothing to derive."
    return report
