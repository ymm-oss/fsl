# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Expansion of named predicate ``def`` declarations into kernel expressions."""
from __future__ import annotations

from .model import FslError


def _walk_values(node):
    if isinstance(node, tuple):
        yield from node[1:]
    elif isinstance(node, list):
        yield from node
    elif isinstance(node, dict):
        yield from node.values()


def _free_vars(node, bound=frozenset()):
    if not isinstance(node, (tuple, list, dict)):
        return set()
    if isinstance(node, tuple) and node:
        tag = node[0]
        if tag == "var":
            return set() if node[1] in bound else {node[1]}
        if tag in ("forall", "exists"):
            binder = node[1]
            name = binder[1]
            free = _free_vars(binder, bound | {name})
            free.update(_free_vars(node[2], bound | {name}))
            return free
        if tag == "count":
            return _free_vars(node[3], bound | {node[1]})
        if tag == "sum":
            name = node[1]
            free = _free_vars(node[3], bound | {name})
            if len(node) > 4 and node[4] is not None:
                free.update(_free_vars(node[4], bound | {name}))
            return free
    free = set()
    for child in _walk_values(node):
        free.update(_free_vars(child, bound))
    return free


def _bound_vars(node):
    if not isinstance(node, (tuple, list, dict)):
        return set()
    bound = set()
    if isinstance(node, tuple) and node:
        if node[0] in ("forall", "exists"):
            bound.add(node[1][1])
        elif node[0] in ("count", "sum"):
            bound.add(node[1])
        elif node[0] in ("unique", "exactly_one"):
            bound.add(node[1][1])
    for child in _walk_values(node):
        bound.update(_bound_vars(child))
    return bound


def _substitute(node, replacements):
    if isinstance(node, tuple):
        if node and node[0] == "var" and node[1] in replacements:
            return replacements[node[1]]
        return tuple(_substitute(part, replacements) for part in node)
    if isinstance(node, list):
        return [_substitute(part, replacements) for part in node]
    if isinstance(node, dict):
        return {key: _substitute(value, replacements) for key, value in node.items()}
    return node


def expand_named_predicates(ast):
    """Remove top-level ``def`` items and inline every predicate call."""
    if not isinstance(ast, tuple) or len(ast) != 3 or ast[0] not in (
        "spec", "compose", "requirements",
    ):
        return ast

    defs = {}
    items = []
    for item in ast[2]:
        if not isinstance(item, tuple) or not item or item[0] != "def":
            items.append(item)
            continue
        _, name, params, body, loc = item
        if name in defs:
            raise FslError(f"duplicate def '{name}'", kind="name", loc=loc)
        names = [param[1] for param in params]
        if len(names) != len(set(names)):
            raise FslError(f"duplicate parameter in def '{name}'", kind="name", loc=loc)
        shadowed = sorted(set(names) & _bound_vars(body))
        if shadowed:
            raise FslError(
                f"def '{name}' parameter is shadowed by binder '{shadowed[0]}'",
                kind="name", loc=loc,
            )
        defs[name] = {"params": names, "body": body, "loc": loc}

    def validate_definition(name, stack=()):
        if name in stack:
            cycle = " -> ".join((*stack, name))
            raise FslError(
                f"recursive predicate definition is not allowed: {cycle}",
                kind="semantics", loc=defs[name]["loc"],
            )
        definition = defs[name]

        def visit(node):
            if isinstance(node, tuple):
                if node and node[0] == "call":
                    _, called, args, loc = node
                    if called not in defs:
                        raise FslError(
                            f"undefined predicate '{called}'", kind="name", loc=loc,
                        )
                    expected = len(defs[called]["params"])
                    if len(args) != expected:
                        raise FslError(
                            f"predicate '{called}' expects {expected} argument(s), "
                            f"got {len(args)}",
                            kind="type", loc=loc,
                        )
                    validate_definition(called, (*stack, name))
                for part in node[1:]:
                    visit(part)
            elif isinstance(node, list):
                for part in node:
                    visit(part)
            elif isinstance(node, dict):
                for part in node.values():
                    visit(part)

        visit(definition["body"])

    for definition_name in defs:
        validate_definition(definition_name)

    def expand(node, stack=()):
        if isinstance(node, tuple):
            if node and node[0] == "call":
                _, name, args, loc = node
                if name not in defs:
                    raise FslError(f"undefined predicate '{name}'", kind="name", loc=loc)
                if name in stack:
                    cycle = " -> ".join((*stack, name))
                    raise FslError(
                        f"recursive predicate definition is not allowed: {cycle}",
                        kind="semantics", loc=loc,
                    )
                definition = defs[name]
                expanded_args = [expand(arg, stack) for arg in args]
                if len(expanded_args) != len(definition["params"]):
                    raise FslError(
                        f"predicate '{name}' expects {len(definition['params'])} "
                        f"argument(s), got {len(expanded_args)}",
                        kind="type", loc=loc,
                    )
                collisions = _bound_vars(definition["body"])
                free_args = set().union(*(_free_vars(arg) for arg in expanded_args)) \
                    if expanded_args else set()
                captured = sorted(collisions & free_args)
                if captured:
                    raise FslError(
                        f"predicate '{name}' call would capture variable '{captured[0]}'; "
                        "rename the binder in the def",
                        kind="semantics", loc=loc,
                    )
                body = expand(definition["body"], (*stack, name))
                return _substitute(body, dict(zip(definition["params"], expanded_args)))
            return tuple(expand(part, stack) for part in node)
        if isinstance(node, list):
            return [expand(part, stack) for part in node]
        if isinstance(node, dict):
            return {key: expand(value, stack) for key, value in node.items()}
        return node

    return (ast[0], ast[1], [expand(item) for item in items])
