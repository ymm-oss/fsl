"""Dialect AST expansion for requirement-oriented FSL frontends."""
from __future__ import annotations

from copy import deepcopy
from pathlib import Path

from .compose import expand_compose
from .grammar import Ast, PARSER
from .model import FslError


def _err(message, kind="type", loc=None):
    raise FslError(message, kind=kind, loc=loc)


def _parse_file(path):
    src = Path(path).read_text(encoding="utf-8")
    ast = Ast().transform(PARSER.parse(src))
    if ast[0] == "compose":
        return expand_compose(ast, str(Path(path).parent))
    if ast[0] == "requirements":
        return _expand_requirements_with_display(ast, str(Path(path).parent))
    return ast, {}


def _expr_to_str(expr):
    if not isinstance(expr, tuple):
        return str(expr)
    tag = expr[0]
    if tag == "var":
        return expr[1]
    if tag == "num":
        return str(expr[1])
    if tag == "bool":
        return "true" if expr[1] else "false"
    if tag == "none":
        return "none"
    if tag == "bin":
        return f"{_expr_to_str(expr[2])} {expr[1]} {_expr_to_str(expr[3])}"
    if tag == "not":
        return f"not {_expr_to_str(expr[1])}"
    if tag == "neg":
        return f"-{_expr_to_str(expr[1])}"
    if tag == "index":
        return f"{_expr_to_str(expr[1])}[{_expr_to_str(expr[2])}]"
    if tag == "field":
        return f"{_expr_to_str(expr[1])}.{expr[2]}"
    if tag == "method":
        args = ", ".join(_expr_to_str(a) for a in expr[3])
        return f"{_expr_to_str(expr[1])}.{expr[2]}({args})"
    if tag == "some":
        return f"some({_expr_to_str(expr[1])})"
    if tag == "is":
        pat = expr[2]
        if pat[0] == "pat_none":
            return f"{_expr_to_str(expr[1])} is none"
        return f"{_expr_to_str(expr[1])} is some({pat[1]})"
    if tag == "ite":
        return (
            f"if {_expr_to_str(expr[1])} then {_expr_to_str(expr[2])} "
            f"else {_expr_to_str(expr[3])}"
        )
    return tag


def _meta(req_id, text):
    return {"id": req_id, "text": text}


def _with_meta(item, meta):
    tag = item[0]
    if tag == "req_action":
        return item[:6] + (meta, item[7])
    if tag == "action":
        return item[:6] + (meta,)
    if tag in ("invariant", "reachable"):
        return item[:4] + (meta,)
    if tag == "leadsto":
        return item[:6] + (meta,)
    return item


def _kernel_action_from_req(item, meta=None):
    _, name, params, body, loc, fair, own_meta, maps = item
    out_meta = meta if meta is not None else own_meta
    return ("action", name, params, body, loc, fair, out_meta), maps


def _param_names(params):
    return [p[1] for p in params]


def _target_from_maps(maps, loc):
    if maps is None:
        _err("action in requirements implements block needs a maps clause", loc=loc)
    return maps[1]


def _action_map(name, params, maps, loc):
    return ("action_map", name, _param_names(params), _target_from_maps(maps, loc), loc)


def _split_branch_action(item, req_meta, display_names, action_aliases):
    _, name, params, body, loc, fair, own_meta, action_maps = item
    meta = req_meta if req_meta is not None else own_meta
    branches = [b for b in body if b[0] == "branches"]
    if not branches:
        action, maps = _kernel_action_from_req(item, meta)
        action_aliases.setdefault(name, []).append(name)
        return [action], [_action_map(name, params, maps or action_maps, loc)] if (maps or action_maps) else []
    if len(branches) > 1:
        _err(f"action '{name}' has multiple branches blocks", loc=loc)
    if action_maps is not None:
        _err(f"branched action '{name}' must put maps on each when branch", loc=loc)

    base_items = [deepcopy(b) for b in body if b[0] != "branches"]
    out_actions = []
    out_maps = []
    action_aliases.setdefault(name, [])
    for idx, branch in enumerate(branches[0][1], start=1):
        _, cond, stmts, maps, bloc = branch
        bname = f"{name}__b{idx}"
        bbody = deepcopy(base_items)
        bbody.append(("requires", cond, bloc))
        bbody.extend(deepcopy(stmts))
        out_actions.append(("action", bname, deepcopy(params), bbody, loc, fair, meta))
        out_maps.append(_action_map(bname, params, maps, bloc))
        display_names[bname] = f"{name}[{_expr_to_str(cond)}]"
        action_aliases[name].append(bname)
    return out_actions, out_maps


def _expand_item(item, req_meta, display_names, action_aliases):
    tag = item[0]
    if tag == "req_action":
        return _split_branch_action(item, req_meta, display_names, action_aliases)
    if tag in ("action", "invariant", "reachable", "leadsto"):
        return [_with_meta(item, req_meta) if req_meta is not None else item], []
    return [item], []


def _expand_requirements_with_display(ast, base_dir):
    _, name, items = ast
    out = []
    display_names = {}
    action_maps = []
    action_aliases = {}
    implements = None
    acceptances = []

    for item in items:
        tag = item[0]
        if tag == "implements":
            if implements is not None:
                _err("requirements may declare implements only once", loc=item[4])
            _, abs_name, path, map_items, loc = item
            abs_path = Path(base_dir) / path
            if not abs_path.is_file():
                _err(f"file not found: {path}", kind="io", loc=loc)
            abs_ast, abs_display_names = _parse_file(abs_path)
            if abs_ast[0] != "spec" or abs_ast[1] != abs_name:
                _err(f"spec name mismatch: expected '{abs_name}', got '{abs_ast[1]}'", loc=loc)
            implements = {
                "abs": abs_name,
                "path": str(abs_path),
                "abs_ast": abs_ast,
                "abs_display_names": abs_display_names,
                "maps": map_items,
                "loc": loc,
            }
        elif tag == "requirement":
            _, req_id, text, req_items, loc = item
            req_meta = _meta(req_id, text)
            for child in req_items:
                expanded, maps = _expand_item(child, req_meta, display_names, action_aliases)
                out.extend(expanded)
                action_maps.extend(maps)
        elif tag == "acceptance":
            _, ac_id, text, steps, expect, loc = item
            if expect is None:
                _err(f"acceptance '{ac_id}' missing expect", loc=loc)
            acceptances.append({
                "id": ac_id,
                "text": text,
                "steps": steps,
                "expect": expect[1],
                "loc": loc,
            })
        else:
            expanded, maps = _expand_item(item, None, display_names, action_aliases)
            out.extend(expanded)
            action_maps.extend(maps)

    if implements is not None:
        mapping_items = [("impl", name), ("abs", implements["abs"])]
        mapping_items.extend(implements["maps"])
        mapping_items.extend(action_maps)
        implements["mapping_ast"] = ("refinement", f"{name}Implements{implements['abs']}", mapping_items)
        out.append(("__implements", implements))
    if display_names:
        out.append(("__display_names", display_names))
    if action_aliases:
        out.append(("__action_aliases", action_aliases))
    if acceptances:
        out.append(("__acceptance", acceptances))
    return ("spec", name, out), display_names


def expand_requirements(ast, base_dir):
    """Expand requirements AST to a kernel spec AST."""
    expanded, _display_names = _expand_requirements_with_display(ast, base_dir)
    return expanded


def expand_requirements_with_display(ast, base_dir):
    """Expand requirements AST and return display names for parser plumbing."""
    return _expand_requirements_with_display(ast, base_dir)
