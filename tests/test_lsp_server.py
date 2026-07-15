# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Regression tests for the pygls adapter layer (`src/fslc/lsp/server.py`).

`fslc.lsp.index` (`DocumentIndex`) is pure and already covered by
`tests/test_lsp_index.py`; these tests only exercise the thin lsprotocol
adapter: feature registration, role -> `CompletionItemKind` mapping, and the
hover/references/completion/semanticTokens handlers wired on top of it.
"""
from pathlib import Path

import pytest

pygls = pytest.importorskip("pygls")

from lsprotocol import types  # noqa: E402  (import after the skip guard)

from fslc.lsp.index import (  # noqa: E402
    CompletionCandidate,
    build_index,
    encode_semantic_tokens,
)
from fslc.lsp.server import (  # noqa: E402
    _create_server,
    _definition_target,
    _path_to_uri,
    _migration_code_actions,
    _result_to_diagnostics,
    _to_completion_item,
    _to_lsp_range,
    check_source,
)


ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"
CART_V1 = SPECS / "cart_v1.fsl"

# A minimal compose workspace used by the cross-file tests below, kept
# self-contained (via tmp_path) rather than reusing specs/ directly: several
# fixtures under specs/ intentionally redeclare `spec ShoppingCart` (buggy
# variants), so a workspace-wide name-based resolver lookup over the real
# specs/ directory is not deterministic enough to assert an exact match set
# against. Path-based `use ... from "..."` resolution (exercised here) does
# not go through that name map at all.
LIB_SOURCE = """spec Lib {
  type Id = 0..1

  state { x: Int }

  init { x = 0 }

  action bump(i: Id) {
    x = x + 1
  }
}
"""

MAIN_SOURCE = """compose Main {
  use Lib as lib from "lib.fsl"

  state { y: Int }
  init { y = 0 }

  action bump_and_log(i: lib.Id) = lib.bump(i) {
    y = y + 1
  }

  internal lib.bump
}
"""


def _symbol(index, name, role):
    matches = [sym for sym in index.symbols if sym.name == name and sym.role == role]
    assert matches, f"missing symbol {role}:{name}"
    return matches[0]


def _initialize(server) -> None:
    """Run the minimal LSP handshake.

    pygls' ``server.workspace`` raises ``RuntimeError`` ("has the server been
    initialized?") until ``initialize`` has run once -- real clients always
    send it first, but a handler invoked directly in a test must do the same
    before touching any code path that calls ``_workspace_roots`` (references,
    in this file). ``root_uri=None`` leaves workspace folders empty, so
    ``_workspace_roots`` falls back to the current document's own directory,
    same as an unconfigured client would get.
    """

    server.lsp.lsp_initialize(
        types.InitializeParams(
            process_id=None,
            root_uri=None,
            capabilities=types.ClientCapabilities(),
        )
    )


def _loc_key(loc):
    return (
        loc.uri,
        loc.range.start.line,
        loc.range.start.character,
        loc.range.end.line,
        loc.range.end.character,
    )


def test_create_server_registers_all_features():
    server = _create_server()
    features = set(server.lsp.fm.features.keys())
    expected = {
        types.TEXT_DOCUMENT_HOVER,
        types.TEXT_DOCUMENT_REFERENCES,
        types.TEXT_DOCUMENT_COMPLETION,
        types.TEXT_DOCUMENT_SEMANTIC_TOKENS_FULL,
        types.TEXT_DOCUMENT_DOCUMENT_SYMBOL,
        types.TEXT_DOCUMENT_DEFINITION,
    }
    assert expected <= features


def test_to_completion_item_maps_roles_to_kinds():
    item = _to_completion_item(types, CompletionCandidate("Foo", "type", "type"))
    assert item.kind == types.CompletionItemKind.Class

    item = _to_completion_item(types, CompletionCandidate("act", "action", "action"))
    assert item.kind == types.CompletionItemKind.Function

    item = _to_completion_item(types, CompletionCandidate("k", "keyword"))
    assert item.kind == types.CompletionItemKind.Keyword

    item = _to_completion_item(types, CompletionCandidate("mystery", "no_such_role"))
    assert item.kind == types.CompletionItemKind.Variable


def test_semantic_tokens_full_handler_returns_encoded_data():
    source = CART_V1.read_text(encoding="utf-8")
    index = build_index(source, str(CART_V1))

    data = encode_semantic_tokens(index.semantic_tokens())

    assert data
    assert len(data) % 5 == 0


def test_definition_target_finds_symbol_and_reference_targets():
    source = CART_V1.read_text(encoding="utf-8")
    index = build_index(source, str(CART_V1))
    stock = _symbol(index, "stock", "state_var")

    # Cursor directly on the declaration -> falls back to the symbol branch.
    target = _definition_target(
        index, stock.selection_range.start.line, stock.selection_range.start.character, None, None
    )
    assert target is not None
    name, loc = target
    assert name == "stock"
    assert loc.range == stock.selection_range

    # Cursor on a usage -> reference branch, resolving back to the same declaration.
    usage = next(ref for ref in index.references if ref.name == "stock" and ref.role == "value")
    target = _definition_target(
        index, usage.range.start.line, usage.range.start.character, None, None
    )
    assert target is not None
    name, loc = target
    assert name == "stock"
    assert loc.range == stock.selection_range


def test_hover_handler_via_index():
    server = _create_server()
    uri = _path_to_uri(str(CART_V1))
    index = build_index(CART_V1.read_text(encoding="utf-8"), str(CART_V1))
    stock = _symbol(index, "stock", "state_var")

    hover_handler = server.lsp.fm.features[types.TEXT_DOCUMENT_HOVER]
    result = hover_handler(
        types.HoverParams(
            text_document=types.TextDocumentIdentifier(uri=uri),
            position=types.Position(
                line=stock.selection_range.start.line,
                character=stock.selection_range.start.character,
            ),
        )
    )

    assert isinstance(result, types.Hover)
    assert "stock" in result.contents.value
    assert result.range == _to_lsp_range(types, stock.selection_range)


def test_references_handler_via_index():
    server = _create_server()
    _initialize(server)
    uri = _path_to_uri(str(CART_V1))
    index = build_index(CART_V1.read_text(encoding="utf-8"), str(CART_V1))
    stock = _symbol(index, "stock", "state_var")

    references_handler = server.lsp.fm.features[types.TEXT_DOCUMENT_REFERENCES]
    result = references_handler(
        types.ReferenceParams(
            text_document=types.TextDocumentIdentifier(uri=uri),
            position=types.Position(
                line=stock.selection_range.start.line,
                character=stock.selection_range.start.character,
            ),
            context=types.ReferenceContext(include_declaration=True),
        )
    )

    assert result
    assert all(isinstance(loc, types.Location) for loc in result)
    declaration = types.Location(uri=uri, range=_to_lsp_range(types, stock.selection_range))
    assert _loc_key(declaration) in {_loc_key(loc) for loc in result}


def test_workspace_references_finds_cross_file_alias_reference(tmp_path):
    lib_path = tmp_path / "lib.fsl"
    main_path = tmp_path / "main.fsl"
    lib_path.write_text(LIB_SOURCE, encoding="utf-8")
    main_path.write_text(MAIN_SOURCE, encoding="utf-8")

    lib_index = build_index(LIB_SOURCE, str(lib_path))
    bump = _symbol(lib_index, "bump", "action")
    main_index = build_index(MAIN_SOURCE, str(main_path))
    main_bump_refs = [ref for ref in main_index.references if ref.name == "bump"]
    assert len(main_bump_refs) == 2  # the `= lib.bump(i)` sync target and `internal lib.bump`

    server = _create_server()
    _initialize(server)
    lib_uri = _path_to_uri(str(lib_path))
    main_uri = _path_to_uri(str(main_path))

    references_handler = server.lsp.fm.features[types.TEXT_DOCUMENT_REFERENCES]
    result = references_handler(
        types.ReferenceParams(
            text_document=types.TextDocumentIdentifier(uri=lib_uri),
            position=types.Position(
                line=bump.selection_range.start.line,
                character=bump.selection_range.start.character,
            ),
            context=types.ReferenceContext(include_declaration=True),
        )
    )

    assert result is not None
    expected = {
        _loc_key(types.Location(uri=lib_uri, range=_to_lsp_range(types, bump.selection_range)))
    } | {
        _loc_key(types.Location(uri=main_uri, range=_to_lsp_range(types, ref.range)))
        for ref in main_bump_refs
    }
    assert {_loc_key(loc) for loc in result} == expected


def test_workspace_references_includes_cross_file_declaration(tmp_path):
    # Regression: cursor on a cross-file *reference* (main.fsl `lib.bump`) with
    # include_declaration=True must still return the declaration in lib.fsl.
    # references_at() only emits the declaration when it lives in the current
    # document, and the workspace loop scans other files' references (never their
    # declaration symbol), so the declaration used to be dropped for this case.
    lib_path = tmp_path / "lib.fsl"
    main_path = tmp_path / "main.fsl"
    lib_path.write_text(LIB_SOURCE, encoding="utf-8")
    main_path.write_text(MAIN_SOURCE, encoding="utf-8")

    lib_index = build_index(LIB_SOURCE, str(lib_path))
    bump = _symbol(lib_index, "bump", "action")
    main_index = build_index(MAIN_SOURCE, str(main_path))
    main_bump_refs = [ref for ref in main_index.references if ref.name == "bump"]
    assert len(main_bump_refs) == 2

    server = _create_server()
    _initialize(server)
    lib_uri = _path_to_uri(str(lib_path))
    main_uri = _path_to_uri(str(main_path))

    # Cursor on the first cross-file reference in main.fsl (the `= lib.bump(i)` target).
    cursor = main_bump_refs[0]
    references_handler = server.lsp.fm.features[types.TEXT_DOCUMENT_REFERENCES]
    result = references_handler(
        types.ReferenceParams(
            text_document=types.TextDocumentIdentifier(uri=main_uri),
            position=types.Position(
                line=cursor.range.start.line,
                character=cursor.range.start.character,
            ),
            context=types.ReferenceContext(include_declaration=True),
        )
    )

    assert result is not None
    declaration = types.Location(
        uri=lib_uri, range=_to_lsp_range(types, bump.selection_range)
    )
    got = {_loc_key(loc) for loc in result}
    assert _loc_key(declaration) in got  # the cross-file declaration must be present
    for ref in main_bump_refs:
        assert _loc_key(
            types.Location(uri=main_uri, range=_to_lsp_range(types, ref.range))
        ) in got


def test_completion_handler_suggests_alias_members(tmp_path):
    lib_path = tmp_path / "lib.fsl"
    main_path = tmp_path / "main.fsl"
    lib_path.write_text(LIB_SOURCE, encoding="utf-8")
    main_path.write_text(MAIN_SOURCE, encoding="utf-8")

    # Position the cursor right after "lib." on the `internal lib.bump` line.
    main_lines = MAIN_SOURCE.splitlines()
    target_line = next(i for i, line in enumerate(main_lines) if "internal lib.bump" in line)
    target_char = main_lines[target_line].index("lib.bump") + len("lib.")

    server = _create_server()
    main_uri = _path_to_uri(str(main_path))

    completion_handler = server.lsp.fm.features[types.TEXT_DOCUMENT_COMPLETION]
    result = completion_handler(
        types.CompletionParams(
            text_document=types.TextDocumentIdentifier(uri=main_uri),
            position=types.Position(line=target_line, character=target_char),
        )
    )

    assert isinstance(result, types.CompletionList)
    assert result.is_incomplete is False
    labels = {item.label: item for item in result.items}
    assert "bump" in labels
    assert labels["bump"].kind == types.CompletionItemKind.Function
    assert "Id" in labels
    assert labels["Id"].kind == types.CompletionItemKind.Class


def test_analysis_findings_can_surface_as_information_diagnostics():
    source = """spec LspAnalysis {
  state { x: Int }
  init { x = 0 }
  action broad() {
    x = x + 1
  }
  invariant Any "MODEL: baseline" { true }
}
"""
    index = build_index(source, "lsp_analysis.fsl")

    result = check_source(source, "lsp_analysis.fsl", analysis_diagnostics=True)
    diagnostics = _result_to_diagnostics(types, source, result, index)

    matches = [d for d in diagnostics if d.code == "unguarded_action"]
    assert matches
    diag = matches[0]
    assert diag.severity == types.DiagnosticSeverity.Information
    assert diag.source == "fslc analyze"
    assert "Structural review" in diag.message
    assert source.splitlines()[diag.range.start.line][diag.range.start.character:diag.range.end.character] == "broad"


def test_analysis_diagnostics_no_findings_file_adds_no_information_diagnostics():
    source = """spec LspNoFindings {
  state { x: Int }
  init { x = 0 }
  action guarded() "REQ-1: guarded write" {
    requires x == 0
    x = x + 1
  }
}
"""
    index = build_index(source, "lsp_no_findings.fsl")

    result = check_source(source, "lsp_no_findings.fsl", analysis_diagnostics=True)
    diagnostics = _result_to_diagnostics(types, source, result, index)

    assert not [d for d in diagnostics if d.source == "fslc analyze"]


def test_legacy_enum_diagnostic_exposes_a_machine_applicable_code_action():
    source = """domain Legacy {
  type Status = Pending | Done
  aggregate Job { state { status: Status = Pending; } }
}
"""
    result = check_source(source, "legacy.fsl")
    diagnostics = _result_to_diagnostics(types, source, result, None)
    diagnostic = next(d for d in diagnostics if d.code == "deprecated_domain_enum_union")
    assert diagnostic.data["taxonomy"] == "deprecated"
    assert diagnostic.data["machine_applicable"] is True

    uri = "file:///tmp/legacy.fsl"
    actions = _migration_code_actions(types, uri, [diagnostic])
    assert len(actions) == 1
    edit = actions[0].edit.changes[uri][0]
    assert edit.new_text == "enum Status { Pending, Done }"
    lines = source.splitlines()
    replaced = lines[edit.range.start.line][
        edit.range.start.character:edit.range.end.character
    ]
    assert replaced == "type Status = Pending | Done"
