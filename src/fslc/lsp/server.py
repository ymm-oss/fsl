# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""pygls language server for FSL files."""
from __future__ import annotations

import re
import os
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple
from urllib.parse import unquote, urlparse

from lark.exceptions import UnexpectedInput, VisitError

from fslc.lsp.index import (
    SEMANTIC_TOKEN_MODIFIERS,
    SEMANTIC_TOKEN_TYPES,
    CompletionCandidate,
    DocumentIndex,
    Location,
    NameResolver,
    Range,
    Symbol,
    build_index,
    encode_semantic_tokens,
)


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
    analysis_diagnostics: bool = False,
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
    from fslc.analysis import analyze as analyze_structure
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
        if analysis_diagnostics:
            out["analysis_findings"] = analyze_structure(spec, profile="ai-review").get("findings", [])
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
    server.fsl_analysis_diagnostics = os.environ.get(
        "FSLC_LSP_ANALYSIS_DIAGNOSTICS", ""
    ).lower() in {"1", "true", "yes", "on"}

    def publish(uri: str) -> None:
        source = _source_for_uri(server, uri)
        if source is None:
            return
        path = _uri_to_path(uri)
        index = _index_for_uri(server, uri)
        result = check_source(
            source,
            path,
            _name_resolver_for_server(server, path),
            analysis_diagnostics=getattr(server, "fsl_analysis_diagnostics", False),
        )
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

    @server.feature(types.TEXT_DOCUMENT_HOVER)
    def hover(ls, params):
        uri = params.text_document.uri
        index = _index_for_uri(server, uri)
        if index is None:
            return None
        info = index.hover_at(
            params.position.line,
            params.position.character,
            _loader_for_server(server),
            _name_resolver_for_server(server, _uri_to_path(uri)),
        )
        if info is None:
            return None
        return types.Hover(
            contents=types.MarkupContent(kind=types.MarkupKind.Markdown, value=info.markdown),
            range=_to_lsp_range(types, info.range),
        )

    @server.feature(types.TEXT_DOCUMENT_REFERENCES)
    def references(ls, params):
        uri = params.text_document.uri
        include_declaration = True
        if getattr(params, "context", None) is not None:
            include_declaration = params.context.include_declaration
        locations = _workspace_references(
            server, uri, params.position.line, params.position.character, include_declaration
        )
        return locations or None

    @server.feature(types.TEXT_DOCUMENT_COMPLETION, types.CompletionOptions(trigger_characters=["."]))
    def completion(ls, params):
        uri = params.text_document.uri
        index = _index_for_uri(server, uri)
        if index is None:
            return None
        candidates = index.completions_at(
            params.position.line,
            params.position.character,
            _loader_for_server(server),
            _name_resolver_for_server(server, _uri_to_path(uri)),
        )
        items = [_to_completion_item(types, c) for c in candidates]
        return types.CompletionList(is_incomplete=False, items=items)

    @server.feature(
        types.TEXT_DOCUMENT_SEMANTIC_TOKENS_FULL,
        types.SemanticTokensLegend(
            token_types=list(SEMANTIC_TOKEN_TYPES),
            token_modifiers=list(SEMANTIC_TOKEN_MODIFIERS),
        ),
    )
    def semantic_tokens_full(ls, params):
        index = _index_for_uri(server, params.text_document.uri)
        if index is None:
            return types.SemanticTokens(data=[])
        data = encode_semantic_tokens(index.semantic_tokens())
        return types.SemanticTokens(data=data)

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


def _definition_target(
    index: DocumentIndex,
    line: int,
    character: int,
    load_index,
    name_resolver,
) -> Optional[Tuple[str, Location]]:
    """The (name, declaration-location) pair for the symbol under the cursor, if any."""

    ref = index.reference_at(line, character)
    if ref is not None:
        loc = index.resolve_reference(ref, load_index, name_resolver)
        if loc is None:
            return None
        return ref.name, loc
    sym = index.symbol_at(line, character)
    if sym is not None:
        return sym.name, Location(index.path, sym.selection_range)
    return None


def _loc_equal(a: Location, b: Location) -> bool:
    def normalize(path: Optional[str]) -> Optional[str]:
        return str(Path(path).resolve(strict=False)) if path else None

    return normalize(a.path) == normalize(b.path) and a.range == b.range


def _dedupe_locations(locations: List[Any]) -> List[Any]:
    seen = set()
    deduped: List[Any] = []
    for loc in locations:
        key = (
            loc.uri,
            loc.range.start.line,
            loc.range.start.character,
            loc.range.end.line,
            loc.range.end.character,
        )
        if key in seen:
            continue
        seen.add(key)
        deduped.append(loc)
    return deduped


def _workspace_references(
    server, uri: str, line: int, character: int, include_declaration: bool
) -> List[Any]:
    """Same-document references plus references from every other workspace ``.fsl`` file."""

    from lsprotocol import types

    index = _index_for_uri(server, uri)
    if index is None:
        return []
    loader = _loader_for_server(server)
    this_path = _uri_to_path(uri)
    resolver = _name_resolver_for_server(server, this_path)

    local_ranges = index.references_at(line, character, include_declaration, loader, resolver)
    results = [types.Location(uri=uri, range=_to_lsp_range(types, r)) for r in local_ranges]

    target = _definition_target(index, line, character, loader, resolver)
    if target is None:
        return _dedupe_locations(results)
    target_name, target_loc = target

    this_norm = str(Path(this_path).resolve(strict=False)) if this_path else None
    for root in _workspace_roots(server, this_path):
        for fsl_path in sorted(root.rglob("*.fsl")):
            if not fsl_path.is_file():
                continue
            normalized = str(fsl_path.resolve(strict=False))
            if this_norm is not None and normalized == this_norm:
                continue  # already covered by references_at() above
            other_uri = _path_to_uri(str(fsl_path))
            other_index = _index_for_uri(server, other_uri)
            if other_index is None:
                continue
            for candidate in other_index.references:
                if candidate.name != target_name:
                    continue
                loc = other_index.resolve_reference(candidate, loader, resolver)
                if loc is not None and _loc_equal(loc, target_loc):
                    results.append(
                        types.Location(uri=other_uri, range=_to_lsp_range(types, candidate.range))
                    )

    return _dedupe_locations(results)


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
    for finding in result.get("analysis_findings", []) or []:
        diagnostics.append(_make_analysis_diagnostic(types, source, finding, index))
    return diagnostics


def _make_analysis_diagnostic(types, source: str, finding: Dict[str, Any], index: Optional[DocumentIndex]):
    loc = finding.get("loc") or {}
    rng = _range_from_finding_nodes(finding, index)
    if rng is None:
        rng = _range_from_loc(source, loc, index) if loc else RangeLike(0, 0, 0, 1).to_range()
    message = (
        f"Structural review ({finding.get('finding_type', 'finding')}): "
        f"{finding.get('why_it_matters', 'review the structural finding')}"
    )
    return types.Diagnostic(
        range=_to_lsp_range(types, rng),
        message=message,
        severity=types.DiagnosticSeverity.Information,
        source="fslc analyze",
        code=finding.get("finding_type"),
        data={
            "finding_id": finding.get("finding_id"),
            "formal_status": finding.get("formal_status"),
            "candidate_repairs": finding.get("candidate_repairs", []),
            "do_not_assume": finding.get("do_not_assume", []),
        },
    )


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


def _range_from_finding_nodes(finding: Dict[str, Any], index: Optional[DocumentIndex]) -> Optional[Range]:
    if index is not None:
        for node_id in finding.get("involved_nodes", []) or []:
            rng = _range_for_analysis_node(index, node_id)
            if rng is not None:
                return rng
    return None


def _range_for_analysis_node(index: DocumentIndex, node_id: str) -> Optional[Range]:
    prefix, sep, name = node_id.partition(":")
    if not sep or not name:
        return None
    role_by_prefix = {
        "action": "action",
        "state": "state_var",
        "requirement": "requirement",
        "control": "control",
        "acceptance": "acceptance",
        "forbidden": "forbidden",
        "invariant": "property",
        "trans": "property",
        "reachable": "property",
        "leadsTo": "property",
    }
    role = role_by_prefix.get(prefix)
    if role is None:
        return None
    for sym in index.symbols:
        if sym.name == name and sym.role == role:
            return sym.selection_range
    return None


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


def _to_completion_item(types, candidate: CompletionCandidate):
    by_role = {
        "spec": types.CompletionItemKind.Module,
        "compose": types.CompletionItemKind.Module,
        "requirements": types.CompletionItemKind.Module,
        "business": types.CompletionItemKind.Module,
        "governance": types.CompletionItemKind.Module,
        "refinement": types.CompletionItemKind.Module,
        "alias": types.CompletionItemKind.Module,
        "type": types.CompletionItemKind.Class,
        "entity": types.CompletionItemKind.Class,
        "struct": types.CompletionItemKind.Struct,
        "number": types.CompletionItemKind.Class,
        "enum": types.CompletionItemKind.Enum,
        "enum_member": types.CompletionItemKind.EnumMember,
        "action": types.CompletionItemKind.Function,
        "transition": types.CompletionItemKind.Function,
        "const": types.CompletionItemKind.Constant,
        "state_var": types.CompletionItemKind.Variable,
        "binder": types.CompletionItemKind.Variable,
        "let": types.CompletionItemKind.Variable,
        "value": types.CompletionItemKind.Variable,
        "actor": types.CompletionItemKind.Variable,
        "kpi": types.CompletionItemKind.Variable,
        "authority": types.CompletionItemKind.Variable,
        "parameter": types.CompletionItemKind.Variable,
        "field": types.CompletionItemKind.Field,
        "process": types.CompletionItemKind.Class,
        "stage": types.CompletionItemKind.EnumMember,
        "property": types.CompletionItemKind.Event,
        "requirement": types.CompletionItemKind.Event,
        "acceptance": types.CompletionItemKind.Event,
        "forbidden": types.CompletionItemKind.Event,
        "control": types.CompletionItemKind.Event,
        "policy": types.CompletionItemKind.Event,
        "goal": types.CompletionItemKind.Event,
        "keyword": types.CompletionItemKind.Keyword,
    }
    kind = by_role.get(candidate.role, types.CompletionItemKind.Variable)
    return types.CompletionItem(
        label=candidate.label,
        kind=kind,
        detail=candidate.detail or candidate.role,
    )


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
