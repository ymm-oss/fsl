# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""pygls language server for FSL files."""
from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple
from urllib.parse import unquote, urlparse

from lark.exceptions import UnexpectedInput, VisitError

from fslc.lsp.index import DocumentIndex, Range, Symbol, build_index


SERVER_NAME = "fslc-lsp"
SERVER_VERSION = "0.1.0"


def check_source(source: str, path: Optional[str]) -> Dict[str, Any]:
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

    def publish(uri: str) -> None:
        source = _source_for_uri(server, uri)
        if source is None:
            return
        path = _uri_to_path(uri)
        index = _index_for_uri(server, uri)
        result = check_source(source, path)
        diagnostics = _result_to_diagnostics(types, source, result, index)
        server.publish_diagnostics(uri, diagnostics)

    @server.feature("textDocument/didOpen")
    def did_open(ls, params):
        publish(params.text_document.uri)

    @server.feature("textDocument/didChange")
    def did_change(ls, params):
        _drop_index(server, params.text_document.uri)
        publish(params.text_document.uri)

    @server.feature("textDocument/didSave")
    def did_save(ls, params):
        _drop_index(server, params.text_document.uri)
        publish(params.text_document.uri)

    @server.feature("textDocument/didClose")
    def did_close(ls, params):
        _drop_index(server, params.text_document.uri)
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
