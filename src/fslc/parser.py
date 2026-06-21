# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Parsing entry point: source text -> FSL AST.

Raises ``lark.exceptions.UnexpectedInput`` on syntax errors and
``lark.exceptions.VisitError`` (wrapping an :class:`fslc.model.FslError`)
on grammar transform-time semantic errors. Compose/requirements expansion
(``expand_compose`` / ``expand_requirements_with_display``) raises
:class:`fslc.model.FslError` directly (not wrapped in ``VisitError``).
The CLI translates all of these into the machine-readable JSON error envelope.
"""
from lark.exceptions import UnexpectedInput

from .grammar import PARSER, Ast
from .compose import expand_compose
from .dialects import expand_business, expand_governance_with_display, expand_requirements_with_display


def parse_src(src, base_dir=None):
    """Parse FSL source; expand compose specs when ``base_dir`` is set."""
    try:
        tree = PARSER.parse(src)
    except UnexpectedInput as e:
        e.source = src
        raise
    ast = Ast().transform(tree)
    display_names = {}
    if ast[0] == "compose":
        ast, display_names = expand_compose(ast, base_dir or ".")
    elif ast[0] == "requirements":
        ast, display_names = expand_requirements_with_display(ast, base_dir or ".")
    elif ast[0] == "business":
        ast = expand_business(ast)
    elif ast[0] == "governance":
        ast, display_names = expand_governance_with_display(ast, base_dir or ".")
    return ast, display_names


def parse(src, base_dir=None):
    """Parse FSL source text into the tuple-based AST (``("spec", name, items)``).

    Convenience wrapper that drops the ``display_names`` map returned by
    :func:`parse_src`. For compose specs, pass ``base_dir`` so ``use`` paths
    resolve relative to the spec directory; and use :func:`parse_src` (not this
    wrapper) when you need ``display_names`` for ``build_spec`` — otherwise
    dotted aliases (``cart.stock``) surface as physical names (``cart__stock``).
    """
    ast, _ = parse_src(src, base_dir)
    return ast


def parse_refinement(src):
    """Parse a refinement mapping file (``("refinement", name, items)``)."""
    ast = parse(src)
    if ast[0] != "refinement":
        raise ValueError("expected refinement mapping file")
    return ast
