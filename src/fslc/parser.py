"""Parsing entry point: source text -> FSL AST.

Raises ``lark.exceptions.UnexpectedInput`` on syntax errors and
``lark.exceptions.VisitError`` (wrapping an :class:`fslc.model.FslError`)
on transform-time semantic errors. The CLI translates both into the
machine-readable JSON error envelope.
"""
from .grammar import PARSER, Ast
from .compose import expand_compose


def parse_src(src, base_dir=None):
    """Parse FSL source; expand compose specs when ``base_dir`` is set."""
    tree = PARSER.parse(src)
    ast = Ast().transform(tree)
    display_names = {}
    if ast[0] == "compose":
        ast, display_names = expand_compose(ast, base_dir or ".")
    return ast, display_names


def parse(src, base_dir=None):
    """Parse FSL source text into the tuple-based AST (``("spec", name, items)``)."""
    ast, _ = parse_src(src, base_dir)
    return ast


def parse_refinement(src):
    """Parse a refinement mapping file (``("refinement", name, items)``)."""
    ast = parse(src)
    if ast[0] != "refinement":
        raise ValueError("expected refinement mapping file")
    return ast
