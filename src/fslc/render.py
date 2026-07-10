# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Deterministic AST-to-text rendering, shared by `explain.py` and `bmc.py`.

FSL has no AST pretty-printer proper -- `explain.py` originally grew this one
to render skeletons/counterfactuals from the tuple AST (source-line slicing
fails for compose-component locs, and a mutated/synthesized expression has no
source line at all). Counterexample blame assignment (issue #170) needs the
same renderer from `bmc.py` (to print a false conjunct's text, or a blamed
guard/effect's text), but `bmc.py` cannot import `explain.py` (`explain`
already imports `bmc`) -- hence this leaf module, self-contained and
importable from both.

`explain.py` re-imports every name here under its original private spelling
(`_expr_to_text = expr_to_text`, etc.) so existing internal call sites and
`tests/test_explain.py`'s `from fslc.explain import _expr_to_text` keep
working unchanged.
"""
from __future__ import annotations

from .model import display_label


def public_var(name, spec):
    """A physical/internal variable name as the public dotted name a human
    or agent should see (`display_label` + `__` -> `.`), never a raw
    double-underscore-joined internal name."""
    label = display_label(name, spec)
    if isinstance(label, str) and "__" in label:
        return label.replace("__", ".")
    return label


def _display_from(display_names, name):
    label = (display_names or {}).get(name, name)
    if isinstance(label, str) and "__" in label:
        return label.replace("__", ".")
    return label


def _type_ref_to_text(ty):
    if isinstance(ty, list):
        ty = tuple(ty)
    if not isinstance(ty, tuple) or not ty:
        return str(ty)
    tag = ty[0]
    if tag == "named":
        return ty[1].replace("__", ".")
    if tag == "name":
        return ty[1].replace("__", ".")
    if tag == "int":
        return "Int"
    if tag == "bool":
        return "Bool"
    if tag == "domain":
        return f"{ty[1]}..{ty[2]}"
    if tag == "enum":
        return ty[1].replace("__", ".")
    if tag == "map":
        return f"Map<{_type_ref_to_text(ty[1])}, {_type_ref_to_text(ty[2])}>"
    if tag == "option":
        return f"Option<{_type_ref_to_text(ty[1])}>"
    if tag == "set":
        return f"Set<{_type_ref_to_text(ty[1])}>"
    if tag == "seq":
        return f"Seq<{_type_ref_to_text(ty[1])}, {ty[2]}>"
    if tag == "struct":
        return ty[1].replace("__", ".")
    return str(ty)


_PREC_QUANT = 0
_PREC_NOT = 4
_PREC_IS = 5
_PREC_NEG = 9
_PREC_ATOM = 10

# bin op -> (own precedence, min precedence required of its LHS, of its RHS).
# Mirrors the grammar's chain (loosest to tightest): implies(1) < or(2) <
# and(3) < not(4) < is(5) < cmp(6) < sum(7) < product(8) < unary/atom(9-10).
# `cmp` and `-`/`/`/`%`'s non-left operand deliberately require *strictly*
# tighter than their own tier (not "same tier allowed") because the grammar
# doesn't let them chain at the same level without an explicit paren -- e.g.
# `sum: sum "-" product` means a `-`'s right side can only ever be another
# top-level `-`/`+` in the AST if the source wrote explicit parens there.
_BIN_PREC = {
    "=>": (1, 2, 1),
    "or": (2, 2, 3),
    "and": (3, 3, 4),
    "==": (6, 7, 7), "!=": (6, 7, 7),
    "<": (6, 7, 7), "<=": (6, 7, 7), ">": (6, 7, 7), ">=": (6, 7, 7),
    "+": (7, 7, 8), "-": (7, 7, 8),
    "*": (8, 8, 9), "/": (8, 8, 9), "%": (8, 8, 9),
}


def _expr_prec(expr):
    """Precedence tier of `expr`'s top-level constructor (higher binds
    tighter); see `_BIN_PREC`. Anything not in the chain (literals, index/
    field/method, function-call-like forms) is atom-tight and never needs
    parenthesizing as a child."""
    if not isinstance(expr, tuple) or not expr:
        return _PREC_ATOM
    tag = expr[0]
    if tag == "bin":
        return _BIN_PREC.get(expr[1], (_PREC_ATOM, 0, 0))[0]
    if tag == "not":
        return _PREC_NOT
    if tag == "is":
        return _PREC_IS
    if tag == "neg":
        return _PREC_NEG
    if tag in ("forall", "exists"):
        return _PREC_QUANT
    return _PREC_ATOM


def _render_operand(expr, display_names, min_prec):
    """Render `expr` as a child appearing where the grammar requires at least
    `min_prec` tightness, parenthesizing when `expr`'s own precedence is
    looser. This is the fix for `expr_to_text` silently dropping semantically
    necessary parens -- e.g. `not (A and B)` rendering as `not A and B`,
    which reads as `(not A) and B`."""
    text = expr_to_text(expr, display_names)
    if _expr_prec(expr) < min_prec:
        return f"({text})"
    return text


def expr_to_text(expr, display_names=None):
    if not isinstance(expr, tuple):
        return str(expr)
    tag = expr[0]
    if tag == "var":
        return _display_from(display_names, expr[1])
    if tag == "num":
        return str(expr[1])
    if tag == "bool":
        return "true" if expr[1] else "false"
    if tag == "none":
        return "none"
    if tag == "bin":
        op = expr[1]
        _own_prec, lhs_min, rhs_min = _BIN_PREC.get(op, (_PREC_ATOM, 0, 0))
        lhs = _render_operand(expr[2], display_names, lhs_min)
        rhs = _render_operand(expr[3], display_names, rhs_min)
        return f"{lhs} {op} {rhs}"
    if tag == "not":
        return f"not {_render_operand(expr[1], display_names, _PREC_NOT)}"
    if tag == "neg":
        return f"-{_render_operand(expr[1], display_names, _PREC_NEG)}"
    if tag == "index":
        base = _render_operand(expr[1], display_names, _PREC_ATOM)
        return f"{base}[{expr_to_text(expr[2], display_names)}]"
    if tag == "field":
        base = _render_operand(expr[1], display_names, _PREC_ATOM)
        return f"{base}.{expr[2]}"
    if tag == "method":
        base = _render_operand(expr[1], display_names, _PREC_ATOM)
        args = ", ".join(expr_to_text(a, display_names) for a in expr[3])
        return f"{base}.{expr[2]}({args})"
    if tag == "some":
        return f"some({expr_to_text(expr[1], display_names)})"
    if tag == "is":
        lhs = _render_operand(expr[1], display_names, _PREC_IS + 1)
        pat = expr[2]
        if pat[0] == "pat_none":
            return f"{lhs} is none"
        return f"{lhs} is some({pat[1]})"
    if tag == "ite":
        return (
            f"if {expr_to_text(expr[1], display_names)} then "
            f"{expr_to_text(expr[2], display_names)} else "
            f"{expr_to_text(expr[3], display_names)}"
        )
    if tag in ("set_lit", "seq_lit"):
        head = "Set" if tag == "set_lit" else "Seq"
        return f"{head} {{{', '.join(expr_to_text(e, display_names) for e in expr[1])}}}"
    if tag == "struct_lit":
        fields = ", ".join(
            f"{k}: {expr_to_text(v, display_names)}"
            for k, v in sorted(expr[2].items())
        )
        return f"{_type_ref_to_text(('name', expr[1]))} {{ {fields} }}"
    if tag in ("old", "abs"):
        return f"{tag}({expr_to_text(expr[1], display_names)})"
    if tag == "rel_reachable":
        return (
            f"reachable({expr_to_text(expr[1], display_names)}, "
            f"{expr_to_text(expr[2], display_names)}, "
            f"{expr_to_text(expr[3], display_names)})"
        )
    if tag in ("rel_acyclic", "rel_functional", "rel_injective", "rel_domain", "rel_range"):
        name = {
            "rel_acyclic": "acyclic",
            "rel_functional": "functional",
            "rel_injective": "injective",
            "rel_domain": "domain",
            "rel_range": "range",
        }[tag]
        return f"{name}({expr_to_text(expr[1], display_names)})"
    if tag in ("min", "max"):
        return (
            f"{tag}({expr_to_text(expr[1], display_names)}, "
            f"{expr_to_text(expr[2], display_names)})"
        )
    if tag == "count":
        domain = _type_ref_to_text(("name", expr[2]))
        return f"count({expr[1]}: {domain} where {expr_to_text(expr[3], display_names)})"
    if tag == "sum":
        domain = _type_ref_to_text(("name", expr[2]))
        where = f" where {expr_to_text(expr[4], display_names)}" if expr[4] is not None else ""
        return (
            f"sum({expr[1]}: {domain} of "
            f"{expr_to_text(expr[3], display_names)}{where})"
        )
    if tag in ("forall", "exists"):
        return f"{tag} {_binder_to_text(expr[1], display_names)}: {expr_to_text(expr[2], display_names)}"
    if tag == "set_bounds":
        return f"{_display_from(display_names, expr[1])}'s members are within its declared domain"
    if tag == "map_value_bounds":
        return f"{_display_from(display_names, expr[1])}'s values are within their declared domain"
    return tag


def _binder_to_text(binder, display_names=None):
    if not isinstance(binder, tuple) or len(binder) < 3:
        return str(binder)
    if binder[0] == "binder_typed":
        text = f"{binder[1]}: {_type_ref_to_text(('name', binder[2]))}"
        if len(binder) > 3 and binder[3] is not None:
            text += f" where {expr_to_text(binder[3], display_names)}"
        return text
    if binder[0] == "binder_range":
        return (
            f"{binder[1]} in {expr_to_text(binder[2], display_names)}.."
            f"{expr_to_text(binder[3], display_names)}"
        )
    return str(binder)
