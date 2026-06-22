# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""pygls language server for FSL files."""
from __future__ import annotations

import re
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple
from urllib.parse import unquote, urlparse

from lark.exceptions import UnexpectedInput, VisitError

from fslc.lsp.index import DocumentIndex, NameResolver, Range, Symbol, build_index


SERVER_NAME = "fslc-lsp"
SERVER_VERSION = "0.1.0"


_TOP_LEVEL_NAME = re.compile(
    r"^\s*(?:spec|compose|requirements|business|governance)\s+([A-Za-z_][A-Za-z0-9_]*)\b",
    re.MULTILINE,
)


def check_source(
    source: str,
    path: Optional[str],
    name_resolver: Optional[NameResolver] = None,
) -> Dict[str, Any]:
    """Run the same in-process check path as ``fslc check`` for editor text."""

    from fslc.acceptance import validate_acceptance, validate_forbidden
    from fslc.cli import (
        _add_strict_tag_warnings,
        _envelope,
        _error_envelope,
        _governance_result,
        _implements_result,
        _loc_from_exc,
        _parse_error_result,
    )
    from fslc.model import FslError, build_spec
    from fslc.parser import parse_src
    from fslc.refine import build_refinement

    def acceptance_error(spec):
        checked = validate_acceptance(spec)
        if checked.get("ok"):
            return None
        out = dict(checked)
        out.pop("ok", None)
        return {"result": "error", **out}

    def forbidden_error(spec):
        checked = validate_forbidden(spec)
        if checked.get("ok"):
            return None
        out = dict(checked)
        out.pop("ok", None)
        return {"result": "error", **out}

    try:
        base_dir = str(Path(path).parent) if path else "."
        ast, display_names = parse_src(source, base_dir)
        if ast[0] == "refinement":
            impl_name, abs_name = _refinement_ast_names(ast)
            impl_spec = _build_resolved_spec(impl_name, name_resolver, parse_src, build_spec)
            abs_spec = _build_resolved_spec(abs_name, name_resolver, parse_src, build_spec)
            if impl_spec is not None and abs_spec is not None:
                build_refinement(ast, impl_spec, abs_spec)
            return _envelope({
                "result": "ok",
                "refinement": ast[1],
                "warnings": [],
            })
        spec = build_spec(ast, display_names, semantic_check=False)
        acc = acceptance_error(spec)
        if acc:
            return _envelope(acc)
        forb = forbidden_error(spec)
        if forb:
            return _envelope(forb)
        out: Dict[str, Any] = {
            "result": "ok",
            "spec": spec["name"],
            "warnings": spec["warnings"],
        }
        impl = _implements_result(spec)
        if impl:
            out["implements"] = impl
        gov = _governance_result(spec)
        if gov:
            out["governance"] = gov
        return _envelope(_add_strict_tag_warnings(out, spec, False, None))
    except UnexpectedInput as exc:
        return _parse_error_result(exc)
    except VisitError as exc:
        orig = exc.orig_exc
        return _error_envelope(
            getattr(orig, "kind", "semantics"),
            str(orig),
            _loc_from_exc(orig),
            getattr(orig, "expected", None),
            getattr(orig, "hint", None),
        )
    except FslError as exc:
        return _error_envelope(
            exc.kind,
            str(exc),
            _loc_from_exc(exc),
            getattr(exc, "expected", None),
            getattr(exc, "hint", None),
        )
    except FileNotFoundError as exc:
        return _envelope({
            "result": "error",
            "kind": "io",
            "message": f"file not found: {exc.filename}",
        })


def _refinement_ast_names(ast) -> Tuple[Optional[str], Optional[str]]:
    impl_name = None
    abs_name = None
    for item in ast[2]:
        if item[0] == "impl":
            impl_name = item[1]
        elif item[0] == "abs":
            abs_name = item[1]
    return impl_name, abs_name


def _build_resolved_spec(name, name_resolver, parse_src, build_spec):
    if not name or name_resolver is None:
        return None
    try:
        target_path = name_resolver(name)
    except Exception:
        return None
    if not target_path:
        return None
    target = Path(target_path)
    try:
        target_source = target.read_text(encoding="utf-8")
    except OSError:
        return None
    ast, display_names = parse_src(target_source, str(target.parent))
    return build_spec(ast, display_names)


def _create_server():
    try:
        from pygls.server import LanguageServer
        from lsprotocol import types
    except ImportError as exc:
        raise RuntimeError(
            "fslc-lsp requires the optional LSP dependencies. "
            "Install with: pip install -e '.[lsp]'"
        ) from exc

    server = LanguageServer(SERVER_NAME, SERVER_VERSION)
    server.fsl_index_cache = {}
    server.fsl_name_cache = None

    def publish(uri: str) -> None:
        source = _source_for_uri(server, uri)
        if source is None:
            return
        path = _uri_to_path(uri)
        index = _index_for_uri(server, uri)
        result = check_source(source, path, _name_resolver_for_server(server, path))
        diagnostics = _result_to_diagnostics(types, source, result, index)
        server.publish_diagnostics(uri, diagnostics)

    @server.feature("textDocument/didOpen")
    def did_open(ls, params):
        _drop_name_cache(server)
        publish(params.text_document.uri)

    @server.feature("textDocument/didChange")
    def did_change(ls, params):
        _drop_index(server, params.text_document.uri)
        _drop_name_cache(server)
        publish(params.text_document.uri)

    @server.feature("textDocument/didSave")
    def did_save(ls, params):
        _drop_index(server, params.text_document.uri)
        _drop_name_cache(server)
        publish(params.text_document.uri)

    @server.feature("textDocument/didClose")
    def did_close(ls, params):
        _drop_index(server, params.text_document.uri)
        _drop_name_cache(server)
        server.publish_diagnostics(params.text_document.uri, [])

    @server.feature("textDocument/documentSymbol")
    def document_symbol(ls, params):
        index = _index_for_uri(server, params.text_document.uri)
        if index is None:
            return []
        root_ids = [
            idx for idx, sym in enumerate(index.symbols)
            if sym.parent is None and sym.outline
        ]
        return [_to_document_symbol(types, index, idx) for idx in root_ids]

    @server.feature("textDocument/definition")
    def definition(ls, params):
        uri = params.text_document.uri
        index = _index_for_uri(server, uri)
        if index is None:
            return None
        loc = index.definition_at(
            params.position.line,
            params.position.character,
            _loader_for_server(server),
            _name_resolver_for_server(server, _uri_to_path(uri)),
        )
        if loc is None:
            return None
        target_uri = _path_to_uri(loc.path) if loc.path else uri
        return types.Location(uri=target_uri, range=_to_lsp_range(types, loc.range))

    return server


def main() -> None:
    """Start the FSL language server over stdio."""

    _create_server().start_io()


def _source_for_uri(server, uri: str) -> Optional[str]:
    try:
        return server.workspace.get_text_document(uri).source
    except Exception:
        path = _uri_to_path(uri)
        if path is None:
            return None
        try:
            return Path(path).read_text(encoding="utf-8")
        except OSError:
            return None


def _index_for_uri(server, uri: str) -> Optional[DocumentIndex]:
    source = _source_for_uri(server, uri)
    if source is None:
        return None
    path = _uri_to_path(uri)
    cache: Dict[str, Tuple[str, Optional[DocumentIndex]]] = server.fsl_index_cache
    cached = cache.get(uri)
    if cached is not None and cached[0] == source:
        return cached[1]
    try:
        index = build_index(source, path)
    except Exception:
        index = None
    cache[uri] = (source, index)
    return index


def _drop_index(server, uri: str) -> None:
    server.fsl_index_cache.pop(uri, None)


def _drop_name_cache(server) -> None:
    server.fsl_name_cache = None


def _name_resolver_for_server(server, current_path: Optional[str]) -> NameResolver:
    def resolve(name: str) -> Optional[str]:
        return _workspace_spec_name_map(server, current_path).get(name)

    return resolve


def _workspace_spec_name_map(server, current_path: Optional[str]) -> Dict[str, str]:
    roots = _workspace_roots(server, current_path)
    current_dir = _normalize_dir(Path(current_path).parent) if current_path else None
    cache_key = (
        tuple(str(root) for root in roots),
        str(current_dir) if current_dir is not None else None,
    )
    cached = getattr(server, "fsl_name_cache", None)
    if cached is not None and cached[0] == cache_key:
        return cached[1]

    candidates: Dict[str, List[Path]] = {}
    for root in roots:
        try:
            files = sorted(root.rglob("*.fsl"))
        except OSError:
            continue
        for file_path in files:
            if not file_path.is_file():
                continue
            name = _extract_top_level_name(file_path)
            if name is None:
                continue
            candidates.setdefault(name, []).append(file_path)

    name_map = {
        name: str(min(paths, key=lambda path: _resolver_path_rank(path, current_dir)))
        for name, paths in candidates.items()
    }
    server.fsl_name_cache = (cache_key, name_map)
    return name_map


def _workspace_roots(server, current_path: Optional[str]) -> List[Path]:
    roots: List[Path] = []
    workspace = getattr(server, "workspace", None)
    folders = getattr(workspace, "folders", None)
    if folders:
        values = folders.values() if isinstance(folders, dict) else folders
        for folder in values:
            uri = getattr(folder, "uri", None)
            path = _uri_to_path(uri) if uri else getattr(folder, "path", None)
            if path:
                roots.append(Path(path))

    root_uri = getattr(workspace, "root_uri", None)
    if root_uri:
        root_path = _uri_to_path(root_uri)
        if root_path:
            roots.append(Path(root_path))
    root_path = getattr(workspace, "root_path", None)
    if root_path:
        roots.append(Path(root_path))

    if current_path:
        roots.append(Path(current_path).parent)
    if not roots:
        roots.append(Path.cwd())

    deduped: List[Path] = []
    seen = set()
    for root in roots:
        normalized = _normalize_dir(root)
        if normalized in seen or not normalized.exists():
            continue
        seen.add(normalized)
        deduped.append(normalized)
    return deduped


def _extract_top_level_name(path: Path) -> Optional[str]:
    try:
        source = path.read_text(encoding="utf-8")
    except OSError:
        return None
    match = _TOP_LEVEL_NAME.search(source)
    return match.group(1) if match else None


def _resolver_path_rank(path: Path, current_dir: Optional[Path]) -> Tuple[int, int, int, str]:
    normalized = _normalize_dir(path)
    same_dir = current_dir is not None and normalized.parent == current_dir
    return (
        0 if same_dir else 1,
        len(normalized.stem),
        len(str(normalized)),
        str(normalized),
    )


def _normalize_dir(path: Path) -> Path:
    return path.expanduser().resolve(strict=False)


def _loader_for_server(server):
    def load(path: str) -> Optional[DocumentIndex]:
        uri = _path_to_uri(path)
        index = _index_for_uri(server, uri)
        if index is not None:
            return index
        try:
            return build_index(Path(path).read_text(encoding="utf-8"), path)
        except Exception:
            return None

    return load


def _result_to_diagnostics(types, source: str, result: Dict[str, Any], index: Optional[DocumentIndex]):
    diagnostics = []
    if result.get("result") == "error":
        diagnostics.append(
            _make_diagnostic(
                types,
                source,
                result,
                types.DiagnosticSeverity.Error,
                index,
            )
        )
    for warning in result.get("warnings", []) or []:
        diagnostics.append(
            _make_diagnostic(
                types,
                source,
                warning,
                types.DiagnosticSeverity.Warning,
                index,
            )
        )
    return diagnostics


def _make_diagnostic(types, source: str, item: Dict[str, Any], severity, index: Optional[DocumentIndex]):
    loc = item.get("loc") or {}
    rng = _range_from_loc(source, loc, index)
    message = item.get("message") or item.get("hint") or item.get("kind") or "FSL diagnostic"
    if item.get("hint") and item.get("hint") not in message:
        message = f"{message}\nHint: {item['hint']}"
    return types.Diagnostic(
        range=_to_lsp_range(types, rng),
        message=message,
        severity=severity,
        source="fslc",
        code=item.get("kind"),
    )


def _range_from_loc(source: str, loc: Dict[str, Any], index: Optional[DocumentIndex]) -> Range:
    line = max(int(loc.get("line", 1)) - 1, 0)
    character = max(int(loc.get("column", 1)) - 1, 0)
    if index is not None:
        ref = index.reference_at(line, character)
        if ref is not None:
            return ref.range
        sym = index.symbol_at(line, character)
        if sym is not None:
            return sym.selection_range
    lines = source.splitlines()
    if line >= len(lines):
        return RangeLike(line, character, line, character + 1).to_range()
    text = lines[line]
    start = min(character, len(text))
    end = start
    if end < len(text) and _is_identifier_char(text[end]):
        while end < len(text) and _is_identifier_char(text[end]):
            end += 1
    elif end < len(text):
        end += 1
    else:
        end = start + 1
    return RangeLike(line, start, line, end).to_range()


def _to_document_symbol(types, index: DocumentIndex, symbol_id: int):
    sym = index.symbols[symbol_id]
    children = [
        _to_document_symbol(types, index, child_id)
        for child_id in sym.children
        if index.symbols[child_id].outline
    ]
    return types.DocumentSymbol(
        name=sym.name,
        kind=_symbol_kind(types, sym),
        range=_to_lsp_range(types, sym.range),
        selection_range=_to_lsp_range(types, sym.selection_range),
        detail=sym.detail or sym.role,
        children=children or None,
    )


def _symbol_kind(types, sym: Symbol):
    by_role = {
        "action": types.SymbolKind.Function,
        "actor": types.SymbolKind.Object,
        "alias": types.SymbolKind.Namespace,
        "business": types.SymbolKind.Module,
        "compose": types.SymbolKind.Module,
        "const": types.SymbolKind.Constant,
        "control": types.SymbolKind.Event,
        "entity": types.SymbolKind.Class,
        "enum": types.SymbolKind.Enum,
        "enum_member": types.SymbolKind.EnumMember,
        "field": types.SymbolKind.Field,
        "goal": types.SymbolKind.Event,
        "governance": types.SymbolKind.Module,
        "init": types.SymbolKind.Constructor,
        "kpi": types.SymbolKind.Number,
        "number": types.SymbolKind.Number,
        "parameter": types.SymbolKind.Variable,
        "policy": types.SymbolKind.Event,
        "process": types.SymbolKind.Class,
        "property": types.SymbolKind.Event,
        "refinement": types.SymbolKind.Module,
        "requirements": types.SymbolKind.Module,
        "spec": types.SymbolKind.Module,
        "stage": types.SymbolKind.EnumMember,
        "state": types.SymbolKind.Struct,
        "state_var": types.SymbolKind.Variable,
        "struct": types.SymbolKind.Struct,
        "transition": types.SymbolKind.Function,
        "type": types.SymbolKind.Class,
    }
    return by_role.get(sym.role, types.SymbolKind.Variable)


def _to_lsp_range(types, rng: Range):
    return types.Range(
        start=types.Position(line=rng.start.line, character=rng.start.character),
        end=types.Position(line=rng.end.line, character=rng.end.character),
    )


def _uri_to_path(uri: str) -> Optional[str]:
    parsed = urlparse(uri)
    if parsed.scheme != "file":
        return None
    return unquote(parsed.path)


def _path_to_uri(path: Optional[str]) -> str:
    if path is None:
        return ""
    return Path(path).expanduser().resolve(strict=False).as_uri()


def _is_identifier_char(ch: str) -> bool:
    return ch == "_" or ch.isalnum()


class RangeLike:
    def __init__(self, sl: int, sc: int, el: int, ec: int) -> None:
        self.sl = sl
        self.sc = sc
        self.el = el
        self.ec = ec

    def to_range(self) -> Range:
        from fslc.lsp.index import Position

        return Range(Position(self.sl, self.sc), Position(self.el, self.ec))


if __name__ == "__main__":
    main()
