"""Parsing entry point: source text -> FSL AST.

Raises ``lark.exceptions.UnexpectedInput`` on syntax errors and
``lark.exceptions.VisitError`` (wrapping an :class:`fslc.model.FslError`)
on transform-time semantic errors. The CLI translates both into the
machine-readable JSON error envelope.
"""
from .grammar import PARSER, Ast


def parse(src):
    """Parse FSL source text into the tuple-based AST (``("spec", name, items)``)."""
    tree = PARSER.parse(src)
    return Ast().transform(tree)


def parse_refinement(src):
    """Parse a refinement mapping file (``("refinement", name, items)``)."""
    ast = parse(src)
    if ast[0] != "refinement":
        raise ValueError("expected refinement mapping file")
    return ast
