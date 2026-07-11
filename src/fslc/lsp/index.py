# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Raw Lark tree index used by the FSL language server.

This module intentionally reads ``fslc.grammar.PARSER`` directly and does not
touch the tuple AST transformer or verifier kernel. FSL keywords are mostly
contextual, so declarations and references are classified by parse-tree
position, not by spelling. The `ai_component`/`agent`/`dbsystem`/`domain`
frontend dialects have their own Lark grammars (``fslc.ai_parser``/
``fslc.db_parser``/``fslc.domain_parser``) that never reach the kernel
grammar at all -- ``build_index`` picks the matching raw parser via
``is_ai_source``/``is_dbsystem_source``/``is_domain_source`` before falling
back to the kernel ``PARSER``, so those files no longer fail to parse here
just because indexing hard-codes the kernel grammar. fsl-ai *project* files
(``is_ai_project_source``) are a further special case: they are not
Lark-parsed at all (``ai_project.py`` scans top-level blocks with regexes),
so ``build_index`` indexes them directly from ``ai_project._top_blocks``
instead of building a tree.

``tests/test_coupled_change_meta.py`` corpus-scans every grammar production
against this module's ``_visit_*`` handlers and fails CI if a new one ships
unindexed (or undocumented as an intentional exclusion) -- see
``docs/DESIGN-coupled-change-metatest.md``.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Dict, Iterable, List, Optional, Sequence, Tuple

from lark import Tree, Token

from fslc.grammar import PARSER
from fslc.literate import (
    LiterateParser,
    extract_literate_source,
    is_literate_source,
)
from fslc.ai_parser import AI_PARSER, is_ai_source
from fslc.ai_project import _top_blocks, is_ai_project_source
from fslc.db_parser import DB_PARSER, is_dbsystem_source
from fslc.domain_parser import PARSER as DOMAIN_PARSER, is_domain_source


VALUE_ROLES = {
    "actor",
    "binder",
    "const",
    "entity",
    "enum_member",
    "kpi",
    "let",
    "number",
    "parameter",
    "process",
    "stage",
    "state_var",
    "transition",
}
TYPE_ROLES = {"entity", "enum", "number", "struct", "type"}
ACTION_ROLES = {"action", "transition"}
PROPERTY_ROLES = {"property", "policy", "goal"}
SPEC_ROLES = {"business", "compose", "governance", "requirements", "spec"}
CONTROL_ROLES = {"control", "goal", "policy", "requirement"}

# Semantic-token legend (index = position in the list; server registers the same order)
SEMANTIC_TOKEN_TYPES = [
    "namespace", "type", "class", "struct", "enum", "enumMember",
    "function", "variable", "parameter", "property", "event", "keyword",
]
SEMANTIC_TOKEN_MODIFIERS = ["declaration", "definition", "readonly"]

# Symbol.role / Reference.role -> semantic token type name.
# Covers every role emitted by _add_symbol/_add_ref. Unknown roles fall back to "variable".
_ROLE_TO_TOKEN_TYPE = {
    # spec containers / namespaces
    "spec": "namespace", "compose": "namespace", "requirements": "namespace",
    "business": "namespace", "governance": "namespace", "refinement": "namespace",
    "alias": "namespace",
    # types
    "type": "type", "number": "type", "entity": "class", "struct": "struct",
    "enum": "enum", "enum_member": "enumMember",
    # callables
    "action": "function", "transition": "function",
    # values
    "const": "variable", "state_var": "variable", "binder": "variable",
    "let": "variable", "actor": "variable", "kpi": "variable", "authority": "variable",
    "value": "variable",
    "parameter": "parameter",
    "field": "property",
    "process": "class", "stage": "enumMember",
    # properties / obligations (declared-name highlighting)
    "property": "event", "requirement": "event", "acceptance": "event",
    "forbidden": "event", "control": "event", "policy": "event", "goal": "event",
    # ai_component / dbsystem dialects
    "ai_component": "namespace", "dbsystem": "namespace",
    "tool": "function", "table": "struct", "column": "property",
    "artifact": "class", "migration": "function", "environment": "class",
    "fallback_reason": "variable", "fallback_target": "variable",
}

# Roles that are synthetic block markers whose selection_range is a keyword, not an
# identifier -- excluded from semantic tokens (they'd double-color the keyword).
_NON_IDENTIFIER_SYMBOL_ROLES = {"state", "init", "time"}

# Roles that get the "readonly" modifier when they appear as a definition.
_READONLY_SYMBOL_ROLES = {"const"}

# FSL keywords for completion (from grammar.py). Order not important.
FSL_KEYWORDS = [
    "spec", "compose", "requirements", "business", "governance", "refinement",
    "verify", "instances", "values", "use", "as", "from", "internal",
    "const", "type", "symmetric", "enum", "struct", "entity", "number",
    "state", "init", "action", "fair", "requires", "ensures", "let",
    "if", "else", "forall", "exists", "invariant", "trans", "reachable",
    "terminal", "until", "unless", "leadsTo", "decreases", "within", "helpful",
    "relation", "acyclic", "functional", "injective", "domain", "range",
    "map", "maps", "auto", "impl", "abs", "preserve", "progress", "respond", "by",
    "implements", "requirement", "acceptance", "forbidden", "expect", "rejected",
    "time", "urgent", "age", "while", "deadline",
    "actor", "process", "with", "stages", "initial", "transition", "when", "set", "covers",
    "kpi", "count", "in", "control", "owner", "severity", "applies_to",
    "satisfies", "policy", "responds", "every", "reaching", "must", "have",
    "passed", "through", "eventually", "be", "goal", "some", "can", "reach", "all",
    "authority", "owns", "delegates", "require", "satisfied_by",
    "preservation", "before", "after", "checked_by",
    "Int", "Bool", "Map", "Set", "Seq", "Option",
    "true", "false", "none", "is", "and", "or", "not",
    "stage", "sum", "min", "max", "abs", "old", "unique", "exactlyOne",
    "add", "remove", "push", "pop", "head", "at", "size", "contains", "then",
]


@dataclass(frozen=True)
class Position:
    """Zero-based LSP-compatible position."""

    line: int
    character: int


@dataclass(frozen=True)
class Range:
    """Zero-based half-open LSP-compatible range."""

    start: Position
    end: Position

    def contains(self, line: int, character: int) -> bool:
        point = (line, character)
        return self.start_tuple <= point < self.end_tuple

    def contains_range_start(self, other: "Range") -> bool:
        return self.contains(other.start.line, other.start.character)

    @property
    def start_tuple(self) -> Tuple[int, int]:
        return self.start.line, self.start.character

    @property
    def end_tuple(self) -> Tuple[int, int]:
        return self.end.line, self.end.character

    @property
    def span(self) -> Tuple[int, int]:
        return (
            self.end.line - self.start.line,
            self.end.character - self.start.character,
        )


@dataclass
class Symbol:
    """A definition or outline item discovered in the raw parse tree."""

    name: str
    role: str
    range: Range
    selection_range: Range
    detail: str = ""
    scope_range: Optional[Range] = None
    parent: Optional[int] = None
    children: List[int] = field(default_factory=list)
    exported: bool = True
    outline: bool = True


@dataclass(frozen=True)
class Reference:
    """A named reference discovered in the raw parse tree."""

    name: str
    role: str
    range: Range
    qualifier: Optional[str] = None
    target_path: Optional[str] = None
    target_spec: Optional[str] = None


@dataclass(frozen=True)
class ImportBinding:
    """A compose ``use`` binding."""

    alias: str
    spec_name: str
    path: str
    resolved_path: Optional[str]
    range: Range
    alias_range: Range
    spec_range: Range
    path_range: Range


@dataclass(frozen=True)
class Location:
    """A definition location."""

    path: Optional[str]
    range: Range


@dataclass(frozen=True)
class HoverInfo:
    markdown: str
    range: Range


@dataclass(frozen=True)
class SemanticToken:
    """Absolute-position token; server encodes to LSP relative form."""

    line: int
    start_char: int
    length: int
    token_type: str          # one of SEMANTIC_TOKEN_TYPES
    modifiers: Tuple[str, ...] = ()


@dataclass(frozen=True)
class CompletionCandidate:
    label: str
    role: str                # symbol role or "keyword"; server maps to CompletionItemKind
    detail: str = ""


LoadIndex = Callable[[str], Optional["DocumentIndex"]]
NameResolver = Callable[[str], Optional[str]]


@dataclass
class DocumentIndex:
    """Symbols, references, and compose imports for one FSL document."""

    source: str
    path: Optional[str]
    symbols: List[Symbol]
    references: List[Reference]
    imports: List[ImportBinding]

    def reference_at(self, line: int, character: int) -> Optional[Reference]:
        refs = [ref for ref in self.references if ref.range.contains(line, character)]
        if not refs:
            return None
        return min(refs, key=lambda ref: ref.range.span)

    def symbol_at(self, line: int, character: int) -> Optional[Symbol]:
        symbols = [
            sym for sym in self.symbols
            if sym.selection_range.contains(line, character)
        ]
        if not symbols:
            return None
        return min(symbols, key=lambda sym: sym.selection_range.span)

    def symbols_named(self, name: str, roles: Optional[Iterable[str]] = None) -> List[Symbol]:
        allowed = set(roles) if roles is not None else None
        return [
            sym for sym in self.symbols
            if sym.name == name and (allowed is None or sym.role in allowed)
        ]

    def import_for_alias(self, alias: str) -> Optional[ImportBinding]:
        for binding in self.imports:
            if binding.alias == alias:
                return binding
        return None

    def find_exported_symbol(self, name: str, role: str) -> Optional[Symbol]:
        roles = _roles_for_reference(role)
        candidates = [
            sym for sym in self.symbols
            if sym.exported and sym.name == name and sym.role in roles
        ]
        if candidates:
            return min(candidates, key=lambda sym: sym.selection_range.start_tuple)
        if role == "spec":
            tops = [
                sym for sym in self.symbols
                if sym.exported and sym.name == name and sym.role in SPEC_ROLES
            ]
            if tops:
                return min(tops, key=lambda sym: sym.selection_range.start_tuple)
        return None

    def resolve_reference(
        self,
        ref: Reference,
        load_index: Optional[LoadIndex] = None,
        name_resolver: Optional[NameResolver] = None,
    ) -> Optional[Location]:
        if ref.qualifier:
            binding = self.import_for_alias(ref.qualifier)
            if binding is None or binding.resolved_path is None:
                return None
            target = _load_import_index(binding.resolved_path, load_index)
            if target is None:
                return None
            sym = target.find_exported_symbol(ref.name, ref.role)
            if sym is None:
                return None
            return Location(target.path, sym.selection_range)

        if ref.target_spec and ref.role != "spec":
            scoped = self._resolve_scoped(ref)
            if scoped is not None:
                return Location(self.path, scoped.selection_range)

        if ref.target_spec and name_resolver is not None:
            target_path = name_resolver(ref.target_spec)
            if target_path:
                target = _load_import_index(target_path, load_index)
                if target is not None:
                    sym = target.find_exported_symbol(ref.name, ref.role)
                    if sym is None and ref.role == "spec":
                        top = _top_symbol(target)
                        sym = top if top and top.name == ref.name else None
                    if sym is not None:
                        return Location(target.path, sym.selection_range)

        if ref.target_path:
            target = _load_import_index(ref.target_path, load_index)
            if target is None:
                return None
            sym = target.find_exported_symbol(ref.name, ref.role)
            if sym is None:
                top = _top_symbol(target)
                sym = top if top and top.name == ref.name else None
            if sym is None:
                return None
            return Location(target.path, sym.selection_range)

        binding = self.import_for_alias(ref.name)
        if binding is not None:
            return Location(self.path, binding.alias_range)

        sym = self._resolve_local(ref)
        if sym is None:
            return None
        return Location(self.path, sym.selection_range)

    def definition_at(
        self,
        line: int,
        character: int,
        load_index: Optional[LoadIndex] = None,
        name_resolver: Optional[NameResolver] = None,
    ) -> Optional[Location]:
        ref = self.reference_at(line, character)
        if ref is not None:
            return self.resolve_reference(ref, load_index, name_resolver)
        sym = self.symbol_at(line, character)
        if sym is not None:
            return Location(self.path, sym.selection_range)
        return None

    def references_at(
        self,
        line: int,
        character: int,
        include_declaration: bool = True,
        load_index: Optional[LoadIndex] = None,
        name_resolver: Optional[NameResolver] = None,
    ) -> List[Range]:
        """All same-document ranges referring to the symbol at ``line``/``character``."""

        ref = self.reference_at(line, character)
        if ref is not None:
            target_loc = self.resolve_reference(ref, load_index, name_resolver)
            target_name = ref.name
        else:
            sym = self.symbol_at(line, character)
            if sym is None:
                return []
            target_loc = Location(self.path, sym.selection_range)
            target_name = sym.name

        if target_loc is None:
            return []

        matches: List[Range] = []
        for candidate in self.references:
            if candidate.name != target_name:
                continue
            loc = self.resolve_reference(candidate, load_index, name_resolver)
            if loc is not None and _same_location(loc, target_loc):
                matches.append(candidate.range)

        if include_declaration and _normalize_path(target_loc.path) == self.path:
            matches.append(target_loc.range)

        unique = list(dict.fromkeys(matches))
        unique.sort(key=lambda rng: (rng.start.line, rng.start.character))
        return unique

    def hover_at(
        self,
        line: int,
        character: int,
        load_index: Optional[LoadIndex] = None,
        name_resolver: Optional[NameResolver] = None,
    ) -> Optional[HoverInfo]:
        """Hover markdown for the reference or symbol at ``line``/``character``."""

        ref = self.reference_at(line, character)
        if ref is not None:
            loc = self.resolve_reference(ref, load_index, name_resolver)
            def_sym: Optional[Symbol] = None
            def_source = self.source
            if loc is not None:
                if loc.path == self.path:
                    def_sym = next(
                        (sym for sym in self.symbols if sym.selection_range == loc.range),
                        None,
                    )
                elif loc.path is not None:
                    target = _load_import_index(loc.path, load_index)
                    if target is not None:
                        def_sym = target.symbol_at(loc.range.start.line, loc.range.start.character)
                        def_source = target.source
            if def_sym is not None:
                snippet = _declaration_snippet(def_source, def_sym)
                markdown = f"```fsl\n{snippet}\n```\n**{def_sym.role}** `{def_sym.name}`"
            else:
                markdown = f"**{ref.role}** `{ref.name}`"
            return HoverInfo(markdown=markdown, range=ref.range)

        sym = self.symbol_at(line, character)
        if sym is not None:
            snippet = _declaration_snippet(self.source, sym)
            markdown = f"```fsl\n{snippet}\n```\n**{sym.role}** `{sym.name}`"
            return HoverInfo(markdown=markdown, range=sym.selection_range)

        return None

    def semantic_tokens(self) -> List[SemanticToken]:
        """Absolute-position semantic tokens for every classifiable symbol/reference."""

        tokens: List[SemanticToken] = []
        for sym in self.symbols:
            if sym.role in _NON_IDENTIFIER_SYMBOL_ROLES:
                continue
            sr = sym.selection_range
            if sr.start.line != sr.end.line:
                continue
            modifiers = ["definition"]
            if sym.role in _READONLY_SYMBOL_ROLES:
                modifiers.append("readonly")
            tokens.append(
                SemanticToken(
                    line=sr.start.line,
                    start_char=sr.start.character,
                    length=sr.end.character - sr.start.character,
                    token_type=_token_type_for_role(sym.role),
                    modifiers=tuple(modifiers),
                )
            )
        for ref in self.references:
            rr = ref.range
            if rr.start.line != rr.end.line:
                continue
            tokens.append(
                SemanticToken(
                    line=rr.start.line,
                    start_char=rr.start.character,
                    length=rr.end.character - rr.start.character,
                    token_type=_token_type_for_role(ref.role),
                    modifiers=(),
                )
            )

        tokens.sort(key=lambda tok: (tok.line, tok.start_char))
        deduped: List[SemanticToken] = []
        seen = set()
        for tok in tokens:
            key = (tok.line, tok.start_char)
            if key in seen:
                continue
            seen.add(key)
            deduped.append(tok)
        return deduped

    def completions_at(
        self,
        line: int,
        character: int,
        load_index: Optional[LoadIndex] = None,
        name_resolver: Optional[NameResolver] = None,
    ) -> List[CompletionCandidate]:
        """Completion candidates for the cursor at ``line``/``character``."""

        lines = self.source.splitlines()
        prefix = lines[line][:character] if 0 <= line < len(lines) else ""
        match = re.search(r'([A-Za-z_]\w*)\.\w*$', prefix)
        if match:
            binding = self.import_for_alias(match.group(1))
            if binding is not None:
                if binding.resolved_path is None:
                    return []
                target = _load_import_index(binding.resolved_path, load_index)
                if target is None:
                    return []
                return [
                    CompletionCandidate(label=sym.name, role=sym.role, detail=sym.detail)
                    for sym in target.symbols
                    if sym.exported
                ]

        candidates: Dict[str, CompletionCandidate] = {}
        for sym in self.symbols:
            if sym.outline or sym.exported:
                candidates.setdefault(
                    sym.name,
                    CompletionCandidate(label=sym.name, role=sym.role, detail=sym.detail),
                )
        for keyword in FSL_KEYWORDS:
            candidates.setdefault(keyword, CompletionCandidate(label=keyword, role="keyword"))
        return list(candidates.values())

    def _resolve_local(self, ref: Reference) -> Optional[Symbol]:
        scoped = self._resolve_scoped(ref)
        if scoped is not None:
            return scoped

        roles = _roles_for_reference(ref.role)
        candidates = [
            sym for sym in self.symbols
            if sym.name == ref.name and sym.role in roles
        ]
        if not candidates:
            return None

        globals_ = [
            sym for sym in candidates
            if sym.scope_range is None and sym.selection_range != ref.range
        ]
        if globals_:
            return min(globals_, key=lambda sym: sym.selection_range.start_tuple)
        return None

    def _resolve_scoped(self, ref: Reference) -> Optional[Symbol]:
        roles = _roles_for_reference(ref.role)
        scoped = [
            sym for sym in self.symbols
            if sym.name == ref.name
            and sym.role in roles
            and sym.scope_range is not None
            and sym.scope_range.contains_range_start(ref.range)
        ]
        if scoped:
            return min(scoped, key=lambda sym: _scope_sort_key(sym))
        return None


def _parser_for_source(source: str):
    if is_literate_source(source):
        extracted = extract_literate_source(source)
        return LiterateParser(_parser_for_source(extracted))
    if is_dbsystem_source(source):
        return DB_PARSER
    if is_domain_source(source):
        return DOMAIN_PARSER
    if is_ai_source(source):
        return AI_PARSER
    return PARSER


def _build_ai_project_index(source: str, path: Optional[str]) -> "DocumentIndex":
    """fsl-ai project files (``is_ai_project_source``) are not Lark-parsed at
    all -- ``ai_project.py`` scans top-level ``kind name { ... }`` blocks with
    regexes/brace-matching (``_top_blocks``). Index each top-level block as an
    outline symbol (kind as role) so go-to-definition/outline at least finds
    the block; nested content (dataset records, statistical thresholds, ...)
    is this dialect's own semantics, not the LSP's, and stays unindexed."""
    symbols: List[Symbol] = []
    for block in _top_blocks(source):
        if not block.name:
            continue
        header_line = block.text.split("\n", 1)[0]
        col = header_line.find(block.name)
        if col < 0:
            col = 0
        pos = Position(block.line - 1, col)
        sel_range = Range(pos, Position(pos.line, pos.character + len(block.name)))
        symbols.append(Symbol(
            name=block.name,
            role=block.kind,
            range=sel_range,
            selection_range=sel_range,
            detail=block.kind,
        ))
    return DocumentIndex(
        source=source,
        path=_normalize_path(path),
        symbols=symbols,
        references=[],
        imports=[],
    )


def build_index(source: str, path: Optional[str] = None) -> DocumentIndex:
    """Parse ``source`` and return a raw-tree symbol/reference index."""

    if is_ai_project_source(source):
        return _build_ai_project_index(source, path)
    tree = _parser_for_source(source).parse(source)
    builder = _IndexBuilder(source, path)
    builder.visit(tree, None, None)
    return DocumentIndex(
        source=source,
        path=_normalize_path(path),
        symbols=builder.symbols,
        references=builder.references,
        imports=builder.imports,
    )


def default_load_index(path: str) -> Optional[DocumentIndex]:
    """Load and index a file, returning ``None`` if it cannot be resolved."""

    try:
        src = Path(path).read_text(encoding="utf-8")
        return build_index(src, path)
    except Exception:
        return None


def definition_at(
    source: str,
    path: Optional[str],
    line: int,
    character: int,
    load_index: Optional[LoadIndex] = None,
    name_resolver: Optional[NameResolver] = None,
) -> Optional[Location]:
    """Convenience wrapper for one-shot go-to-definition lookup."""

    return build_index(source, path).definition_at(
        line,
        character,
        load_index,
        name_resolver,
    )


def encode_semantic_tokens(
    tokens: Sequence[SemanticToken],
    token_types: Sequence[str] = SEMANTIC_TOKEN_TYPES,
    token_modifiers: Sequence[str] = SEMANTIC_TOKEN_MODIFIERS,
) -> List[int]:
    """LSP relative encoding: [dLine, dStartChar, length, typeIdx, modBits]* .

    Assumes ``tokens`` sorted by (line, start_char).
    """

    encoded: List[int] = []
    prev_line = 0
    prev_char = 0
    for tok in tokens:
        delta_line = tok.line - prev_line
        delta_char = tok.start_char if delta_line != 0 else tok.start_char - prev_char
        type_name = tok.token_type if tok.token_type in token_types else "variable"
        type_idx = token_types.index(type_name)
        mod_bits = 0
        for modifier in tok.modifiers:
            if modifier in token_modifiers:
                mod_bits |= 1 << token_modifiers.index(modifier)
        encoded.extend([delta_line, delta_char, tok.length, type_idx, mod_bits])
        prev_line = tok.line
        prev_char = tok.start_char
    return encoded


class _IndexBuilder:
    def __init__(self, source: str, path: Optional[str]) -> None:
        self.source = source
        self.path = _normalize_path(path)
        self.symbols: List[Symbol] = []
        self.references: List[Reference] = []
        self.imports: List[ImportBinding] = []
        self._refinement_impl_name: Optional[str] = None
        self._refinement_abs_name: Optional[str] = None
        self._reference_target_spec: Optional[str] = None

    def visit(
        self,
        node: object,
        parent: Optional[int],
        local_scope: Optional[Range],
    ) -> None:
        if not isinstance(node, Tree):
            return
        handler = getattr(self, f"_visit_{node.data}", None)
        if handler is not None:
            handler(node, parent, local_scope)
            return
        self._visit_children(node, parent, local_scope)

    def _visit_children(
        self,
        node: Tree,
        parent: Optional[int],
        local_scope: Optional[Range],
    ) -> None:
        for child in node.children:
            self.visit(child, parent, local_scope)

    def _add_symbol(
        self,
        token: Token,
        role: str,
        full_range: Optional[Range] = None,
        scope_range: Optional[Range] = None,
        parent: Optional[int] = None,
        detail: str = "",
        exported: bool = True,
        outline: bool = True,
    ) -> int:
        symbol = Symbol(
            name=str(token),
            role=role,
            range=full_range or _token_range(token),
            selection_range=_token_range(token),
            detail=detail,
            scope_range=scope_range,
            parent=parent,
            exported=exported,
            outline=outline,
        )
        idx = len(self.symbols)
        self.symbols.append(symbol)
        if parent is not None:
            self.symbols[parent].children.append(idx)
        return idx

    def _add_synthetic_symbol(
        self,
        name: str,
        role: str,
        full_range: Range,
        selection_range: Optional[Range] = None,
        scope_range: Optional[Range] = None,
        parent: Optional[int] = None,
        detail: str = "",
        exported: bool = False,
        outline: bool = True,
    ) -> int:
        symbol = Symbol(
            name=name,
            role=role,
            range=full_range,
            selection_range=selection_range or full_range,
            detail=detail,
            scope_range=scope_range,
            parent=parent,
            exported=exported,
            outline=outline,
        )
        idx = len(self.symbols)
        self.symbols.append(symbol)
        if parent is not None:
            self.symbols[parent].children.append(idx)
        return idx

    def _add_ref(
        self,
        token: Token,
        role: str,
        qualifier: Optional[str] = None,
        target_path: Optional[str] = None,
        target_spec: Optional[str] = None,
    ) -> None:
        self.references.append(
            Reference(
                name=str(token),
                role=role,
                range=_token_range(token),
                qualifier=qualifier,
                target_path=target_path,
                target_spec=target_spec or self._reference_target_spec,
            )
        )

    def _visit_with_target(
        self,
        node: object,
        parent: Optional[int],
        local_scope: Optional[Range],
        target_spec: Optional[str],
    ) -> None:
        previous = self._reference_target_spec
        self._reference_target_spec = target_spec
        try:
            self.visit(node, parent, local_scope)
        finally:
            self._reference_target_spec = previous

    def _visit_start(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_children(node, parent, local_scope)

    def _visit_top_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_children(node, parent, local_scope)

    def _visit_spec_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "spec", None)

    def _visit_compose_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "compose", None)

    def _visit_requirements_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "requirements", None)

    def _visit_business_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "business", None)

    def _visit_governance_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "governance", None)

    def _visit_refinement_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        previous_impl = self._refinement_impl_name
        previous_abs = self._refinement_abs_name
        self._refinement_impl_name, self._refinement_abs_name = _refinement_spec_names(node)
        try:
            self._visit_named_container(node, "refinement", None, exported=False)
        finally:
            self._refinement_impl_name = previous_impl
            self._refinement_abs_name = previous_abs

    # -- ai_component / dbsystem dialects (own grammars, see _parser_for_source) --

    def _visit_ai_component(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "ai_component", None)

    def _visit_agent_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "agent", None)

    def _visit_dbsystem(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "dbsystem", None)

    def _visit_tool_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "tool", _tree_range(node), parent=parent, detail="tool")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_name_list(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # authority may_suggest/may_execute/requires_human_approval/forbidden
        # each reference a tool declared elsewhere in the same ai_component.
        for child in node.children:
            if isinstance(child, Token) and child.type == "NAME":
                self._add_ref(child, "tool")

    def _visit_fallback_item(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `when <reason> require <target>`: both are free labels with no
        # corresponding declaration elsewhere in the grammar (see
        # docs/DESIGN-ai-hard.md); record them so they still show up in
        # outline/hover instead of silently vanishing.
        names = [c for c in node.children if isinstance(c, Token) and c.type == "NAME"]
        if len(names) >= 1:
            self._add_symbol(names[0], "fallback_reason", _tree_range(node), parent=parent,
                              detail="fallback reason", exported=False)
        if len(names) >= 2:
            self._add_symbol(names[1], "fallback_target", _tree_range(node), parent=parent,
                              detail="fallback target", exported=False)

    def _visit_delegation_edge(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `AgentA -> AgentB` in an `orchestration { ... }` block: both sides
        # reference sibling `agent` declarations.
        for token in _name_tokens(node):
            self._add_ref(token, "agent")

    def _visit_agent_event(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `AgentName.status` in a `failure_policy { ... }` block: the first
        # NAME references an agent; the status word (failed/uncertain/...) is
        # fixed vocabulary, not an in-file declaration -- recorded as a
        # non-exported symbol so it still shows up on hover instead of
        # silently vanishing (same treatment as `_visit_fallback_item`).
        names = _name_tokens(node)
        if names:
            self._add_ref(names[0], "agent")
        if len(names) > 1:
            self._add_symbol(names[1], "agent_status", _tree_range(node), parent=parent,
                              detail="agent status", exported=False)

    def _visit_agent_output_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `output Name visibility [parent, OtherAgent, ...]`: the declared
        # output name, then a visibility list of sibling agent references
        # (plus the literal "parent") -- collected directly rather than via
        # the shared name_list handler, which would mislabel them as "tool".
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "agent_output", _tree_range(node), parent=parent,
                              detail="agent output", exported=False)
        for ref_token in _all_name_tokens(node)[1:]:
            self._add_ref(ref_token, "agent")

    def _visit_table_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "table", _tree_range(node), parent=parent, detail="table")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_column_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(
                token, "column", _tree_range(node),
                scope_range=local_scope, parent=parent, detail="column", exported=False,
            )
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_artifact_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "artifact", _tree_range(node), parent=parent, detail="artifact")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_migration_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "migration", _tree_range(node), parent=parent, detail="migration")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_environment_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "environment", _tree_range(node), parent=parent, detail="environment")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_col_ref(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `qualifier` on Reference means an import alias (resolved via
        # import_for_alias), not "table.column" -- col_ref is same-file, so
        # both names are plain, unqualified references (like struct `field`,
        # column lookup is by name only and not scoped to its table).
        names = [c for c in node.children if isinstance(c, Token) and c.type == "NAME"]
        if len(names) == 2:
            self._add_ref(names[0], "table")
            self._add_ref(names[1], "column")

    def _visit_env_artifact(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        names = [c for c in node.children if isinstance(c, Token) and c.type == "NAME"]
        if names:
            self._add_ref(names[0], "artifact")
        # `env_window`/`env_flag_condition*` are Tree children (the artifact
        # NAME itself is the only bare Token) -- visit them too, or a
        # `when flag F=V` condition on this artifact silently vanishes
        # (pre-existing gap, found by the corpus-wide LSP coverage scan).
        for child in node.children:
            if isinstance(child, Tree):
                self.visit(child, parent, local_scope)

    def _visit_database_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "database", _tree_range(node), parent=parent, detail="database")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_env_flag(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "feature_flag", _tree_range(node), parent=parent, detail="feature flag")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_flag_variant_list(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _name_tokens(node):
            self._add_symbol(token, "flag_variant", _token_range(token), parent=parent,
                              detail="flag variant", exported=False)

    def _visit_flag_default(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "flag_variant")

    def _visit_env_flag_condition(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        names = _name_tokens(node)
        if len(names) == 2:
            self._add_ref(names[0], "feature_flag")
            self._add_ref(names[1], "flag_variant")

    # -- fsl-domain dialect (own grammar, see _parser_for_source) --
    #
    # ``state_def``/``invariant_def`` reuse the kernel handlers below
    # unchanged: both are generic (first-token + safely-skip-Token-children),
    # and domain's shapes ("state" "{" state_field* "}" / "invariant" NAME "{"
    # RAW_EXPR "}") satisfy them correctly. Reference-bearing productions that
    # wrap their NAME(s) in domain's inlined ``emit_names``/``bracket_name_list``/
    # ``name_list`` helper rules are handled by directly collecting every NAME
    # under the node (``_all_name_tokens``) and registering it, rather than
    # letting the generic child-walk reach ``name_list`` -- that rule already
    # has an ai_component-specific handler (tool authority lists) that would
    # mislabel a domain event/field reference as a "tool".

    def _visit_domain_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_named_container(node, "domain", None)

    def _visit_implementation_profile_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "implementation_profile", _tree_range(node), parent=parent,
                              detail="implementation profile", exported=False)
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_type_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # NOTE: the kernel grammar also has a (differently-shaped) `type_def`
        # rule (`type_def: plain_type_def | symmetric_type_def`, an unaliased
        # wrapper with no NAME child of its own) -- `_first_token` returning
        # None there is what tells the two shapes apart; falling through to
        # `_visit_children_after_first_token` is what still reaches
        # `plain_type_def`/`symmetric_type_def`'s own handlers in that case.
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "domain_type", _tree_range(node), parent=parent, detail="type")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_value_object_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "value_object", _tree_range(node), parent=parent, detail="value object")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_aggregate_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "aggregate", _tree_range(node), parent=parent, detail="aggregate")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_id_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "aggregate_id", _tree_range(node), parent=parent, detail="id", exported=False)
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_state_field(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "state_field", _tree_range(node), scope_range=local_scope,
                              parent=parent, detail="state field", exported=False)
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_input_field(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "field", _tree_range(node), parent=parent, detail="field", exported=False)
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_bare_field(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_input_field(node, parent, local_scope)

    def _visit_command_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "command", _tree_range(node), parent=parent, detail="command")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_event_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "event", _tree_range(node), parent=parent, detail="event")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_error_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "domain_error", _tree_range(node), parent=parent, detail="error")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_decide_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `decide CommandName { requires ...; rejects ...; emits ... }` --
        # the NAME references the command being decided, it is not a fresh
        # declaration (see examples/domain/*.fsl).
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "command")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_rejects_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "domain_error")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_emits_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _all_name_tokens(node):
            self._add_ref(token, "event")

    def _visit_evolve_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `evolve EventName { ... assign_def* }` -- NAME references the event
        # being evolved; assign_def children still need their own handler.
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "event")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_lvalue(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _name_tokens(node):
            self._add_ref(token, "state_field")

    def _visit_projection_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "projection", _tree_range(node), parent=parent, detail="projection")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_projection_from(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "aggregate")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_projection_fields(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _all_name_tokens(node):
            self._add_ref(token, "state_field")

    def _visit_on_stale_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "projection")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_effect_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "effect", _tree_range(node), parent=parent, detail="effect")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_handles_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "command")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_request_event_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "event")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_success_event_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_request_event_def(node, parent, local_scope)

    def _visit_failure_event_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_request_event_def(node, parent, local_scope)

    def _visit_timeout_event_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_request_event_def(node, parent, local_scope)

    def _visit_backoff_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `backoff exponential` -- a free retry-policy word, not a reference
        # to anything declared in-file (see docs/DESIGN-domain.md).
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "backoff_policy", _tree_range(node), parent=parent,
                              detail="backoff policy", exported=False)
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_timeout_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "event")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_waits_for_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _all_name_tokens(node):
            self._add_ref(token, "event")

    def _visit_await_on_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _all_name_tokens(node):
            self._add_ref(token, "event")

    def _visit_await_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "await", _tree_range(node), parent=parent, detail="await")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_saga_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "saga", _tree_range(node), parent=parent, detail="saga")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_starts_on_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "event")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_saga_step_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "saga_step", _tree_range(node), parent=parent, detail="saga step")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_awaits_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _all_name_tokens(node):
            self._add_ref(token, "event")

    def _visit_saga_compensation_item(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `when EventA after EventB { emits ... }` -- both NAMEs reference
        # events; the stale_item* (emits_def) children still need visiting.
        for token in _name_tokens(node):
            self._add_ref(token, "event")
        for child in node.children:
            if isinstance(child, Tree):
                self.visit(child, parent, local_scope)

    def _visit_outbox_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "outbox", _tree_range(node), parent=parent, detail="outbox", exported=False)
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_inbox_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "inbox", _tree_range(node), parent=parent, detail="inbox", exported=False)
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_type_name(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "domain_type")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_type_generic1(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _name_tokens(node):
            self._add_ref(token, "domain_type")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_type_generic2(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_type_generic1(node, parent, local_scope)

    def _visit_named_container(
        self,
        node: Tree,
        role: str,
        parent: Optional[int],
        exported: bool = True,
    ) -> None:
        token = _first_token(node)
        if token is None:
            self._visit_children(node, parent, _tree_range(node))
            return
        idx = self._add_symbol(
            token,
            role,
            full_range=_tree_range(node),
            parent=parent,
            detail=role,
            exported=exported,
        )
        self._visit_children_after_first_token(node, idx, _tree_range(node))

    def _visit_use_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        spec_name, alias, raw_path = _name_token_at(node, 0), _name_token_at(node, 1), _string_token_at(node, 0)
        if spec_name is None or alias is None or raw_path is None:
            return
        import_path = _unquote_string(raw_path)
        resolved = _resolve_relative_path(self.path, import_path)
        self._add_ref(spec_name, "spec", target_path=resolved)
        self._add_symbol(
            alias,
            "alias",
            full_range=_tree_range(node),
            scope_range=local_scope,
            parent=parent,
            detail=f"use {spec_name}",
            exported=False,
        )
        self.imports.append(
            ImportBinding(
                alias=str(alias),
                spec_name=str(spec_name),
                path=import_path,
                resolved_path=resolved,
                range=_tree_range(node),
                alias_range=_token_range(alias),
                spec_range=_token_range(spec_name),
                path_range=_token_range(raw_path),
            )
        )

    def _visit_internal_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        alias, action = _name_token_at(node, 0), _name_token_at(node, 1)
        if alias is not None:
            self._add_ref(alias, "alias")
        if action is not None and alias is not None:
            self._add_ref(action, "action", qualifier=str(alias))

    def _visit_sync_ref(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        alias, action = _name_token_at(node, 0), _name_token_at(node, 1)
        if alias is not None:
            self._add_ref(alias, "alias")
        if action is not None and alias is not None:
            self._add_ref(action, "action", qualifier=str(alias))
        for child in node.children[2:]:
            self.visit(child, parent, local_scope)

    def _visit_const_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "const", _tree_range(node), parent=parent, detail="const")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_plain_type_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_type_like(node, parent, local_scope, "type")

    def _visit_symmetric_type_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_type_like(node, parent, local_scope, "type", "symmetric type")

    def _visit_plain_enum_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_type_like(node, parent, local_scope, "enum")

    def _visit_symmetric_enum_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_type_like(node, parent, local_scope, "enum", "symmetric enum")

    def _visit_type_like(
        self,
        node: Tree,
        parent: Optional[int],
        local_scope: Optional[Range],
        role: str,
        detail: Optional[str] = None,
    ) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(
                token,
                role,
                _tree_range(node),
                parent=parent,
                detail=detail or role,
            )
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_enum_member(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "enum_member", _tree_range(node), parent=parent, detail="enum member")

    def _visit_struct_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "struct", _tree_range(node), parent=parent, detail="struct")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_field(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(
                token,
                "field",
                _tree_range(node),
                scope_range=local_scope,
                parent=parent,
                detail="field",
                exported=False,
            )
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_state_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_block_with_children(node, "state", "state", parent, local_scope)

    def _visit_compose_state(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_block_with_children(node, "state", "state", parent, local_scope)

    def _visit_init_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_block_with_children(node, "init", "init", parent, local_scope)

    def _visit_compose_init(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_block_with_children(node, "init", "init", parent, local_scope)

    def _visit_time_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_block_with_children(node, "time", "time", parent, local_scope)

    def _visit_block_with_children(
        self,
        node: Tree,
        name: str,
        role: str,
        parent: Optional[int],
        local_scope: Optional[Range],
    ) -> None:
        idx = self._add_synthetic_symbol(
            name,
            role,
            _tree_range(node),
            selection_range=_keyword_range(node, name),
            parent=parent,
            detail=role,
            exported=False,
        )
        self._visit_children(node, idx, local_scope)

    def _visit_var_decl(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "state_var", _tree_range(node), parent=parent, detail="state")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_entity_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "entity", _tree_range(node), parent=parent, detail="entity")

    def _visit_number_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "number", _tree_range(node), parent=parent, detail="number")

    def _visit_plain_action(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_action(node, parent, "action")

    def _visit_fair_action(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_action(node, parent, "fair action")

    def _visit_req_plain_action(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_action(node, parent, "action")

    def _visit_req_fair_action(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_action(node, parent, "fair action")

    def _visit_plain_sync_action(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_action(node, parent, "sync action")

    def _visit_fair_sync_action(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_action(node, parent, "fair sync action")

    def _visit_action(self, node: Tree, parent: Optional[int], detail: str) -> None:
        token = _first_token(node)
        action_scope = _tree_range(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(
                token,
                "action",
                action_scope,
                scope_range=None,
                parent=parent,
                detail=detail,
            )
        self._visit_children_after_first_token(node, idx, action_scope)

    def _visit_param_typed(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_parameter(node, parent, local_scope)

    def _visit_param_range(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_parameter(node, parent, local_scope)

    def _visit_refinement_param(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_parameter(node, parent, local_scope)

    def _visit_parameter(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(
                token,
                "parameter",
                _tree_range(node),
                scope_range=local_scope or _tree_range(node),
                parent=parent,
                detail="parameter",
                exported=False,
                outline=False,
            )
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_let_clause(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(
                token,
                "let",
                _tree_range(node),
                scope_range=local_scope or _tree_range(node),
                parent=parent,
                detail="let",
                exported=False,
                outline=False,
            )
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_binder_typed(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_binder(node, parent, local_scope)

    def _visit_binder_range(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_binder(node, parent, local_scope)

    def _visit_binder_collection(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_binder(node, parent, local_scope)

    def _visit_binder(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(
                token,
                "binder",
                _tree_range(node),
                scope_range=local_scope or _tree_range(node),
                parent=parent,
                detail="binder",
                exported=False,
                outline=False,
            )
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_pat_some(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(
                token,
                "binder",
                _tree_range(node),
                scope_range=local_scope or _tree_range(node),
                parent=parent,
                detail="some binding",
                exported=False,
                outline=False,
            )

    def _visit_invariant_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_property(node, parent, "invariant")

    def _visit_trans_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_property(node, parent, "trans")

    def _visit_reachable_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_property(node, parent, "reachable")

    def _visit_leadsto_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_property(node, parent, "leadsTo")

    def _visit_leadsto_helpful(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `helpful NAME(args)`: NAME references an action declared elsewhere;
        # it was previously a bare Token skipped by the generic child walk,
        # so rename/find-references never saw it.
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "action")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_until_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_property(node, parent, "until")

    def _visit_unless_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_property(node, parent, "unless")

    def _visit_terminal_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        idx = self._add_synthetic_symbol(
            "terminal",
            "property",
            _tree_range(node),
            selection_range=_keyword_range(node, "terminal"),
            parent=parent,
            detail="terminal",
        )
        self._visit_children(node, idx, local_scope)

    def _visit_property(self, node: Tree, parent: Optional[int], detail: str) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "property", _tree_range(node), parent=parent, detail=detail)
        self._visit_children_after_first_token(node, idx, _tree_range(node))

    def _visit_requirement_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_req_id_symbol(node, parent, "requirement")

    def _visit_acceptance_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_req_id_symbol(node, parent, "acceptance")

    def _visit_forbidden_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_req_id_symbol(node, parent, "forbidden")

    def _visit_req_id_symbol(self, node: Tree, parent: Optional[int], role: str) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, role, _tree_range(node), parent=parent, detail=role)
        self._visit_children_after_first_token(node, idx, _tree_range(node))

    def _visit_acceptance_step(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "action")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_acceptance_expect_stage(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        entity, stage = _name_token_at(node, 0), _name_token_at(node, 1)
        if entity is not None:
            self._add_ref(entity, "type")
        if stage is not None:
            self._add_ref(stage, "value")

    def _visit_implements_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        name = _name_token_at(node, 0)
        path_token = _string_token_at(node, 0)
        if name is not None:
            target_path = _resolve_relative_path(self.path, _unquote_string(path_token)) if path_token else None
            self._add_ref(name, "spec", target_path=target_path)
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_refinement_impl(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "spec", target_spec=str(token))

    def _visit_refinement_abs(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "spec", target_spec=str(token))

    def _visit_map_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        map_scope = _tree_range(node)
        if token is not None:
            self._add_ref(token, "value", target_spec=self._refinement_abs_name)
        skipped_name = False
        for child in node.children:
            if not skipped_name and isinstance(child, Token) and child.type == "NAME":
                skipped_name = True
                continue
            if _tree_data(child) in {"binder_typed", "binder_range", "binder_collection"}:
                self.visit(child, parent, map_scope)
            else:
                self._visit_with_target(child, parent, map_scope, self._refinement_impl_name)

    def _visit_refinement_action(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        action_scope = _tree_range(node)
        if token is not None:
            self._add_ref(token, "action", target_spec=self._refinement_impl_name)
        skipped_name = False
        for child in node.children:
            if not skipped_name and isinstance(child, Token) and child.type == "NAME":
                skipped_name = True
                continue
            if _tree_data(child) in {"action_target", "mapped_action_target"}:
                self._visit_with_target(child, parent, action_scope, self._refinement_abs_name)
            else:
                self.visit(child, parent, action_scope)

    def _visit_mapped_action_target(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_action_target_ref(node, parent, local_scope)

    def _visit_req_mapped_action_target(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_action_target_ref(node, parent, local_scope)

    def _visit_action_target_ref(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "action")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_progress_respond(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        first = _name_token_at(node, 0)
        if first is not None:
            self._add_ref(first, "property", target_spec=self._refinement_abs_name)
        for token in _name_tokens(node)[1:]:
            self._add_ref(token, "action", target_spec=self._refinement_impl_name)

    def _visit_verify_instances(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "type")

    def _visit_verify_values(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "type")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_urgent_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _name_tokens(node):
            self._add_ref(token, "action")

    def _visit_age_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(
                token,
                "state_var",
                _tree_range(node),
                parent=parent,
                detail="age",
                exported=True,
            )
        self._visit_children_after_first_token(node, parent, _tree_range(node))

    def _visit_deadline_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        # `deadline NAME <= expr`: NAME is a reference to an already-declared
        # `age NAME[...]` state variable (dialects.py rejects an undeclared
        # one as "deadline references undeclared age"), not a new property
        # name -- multiple `deadline` lines may even target the same age var.
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "state_var")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_actor_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _name_tokens(node):
            self._add_symbol(token, "actor", _tree_range(node), parent=parent, detail="actor")

    def _visit_process_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        proc_scope = _tree_range(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "process", proc_scope, parent=parent, detail="process")
        self._visit_children_after_first_token(node, idx, proc_scope)

    def _visit_proc_field(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_symbol(
                token,
                "field",
                _tree_range(node),
                scope_range=local_scope,
                parent=parent,
                detail="process field",
                exported=False,
            )
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_process_stages(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _name_tokens(node):
            self._add_symbol(
                token,
                "stage",
                _tree_range(node),
                scope_range=local_scope,
                parent=parent,
                detail="stage",
            )

    def _visit_process_initial(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "value")

    def _visit_process_transition(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        name, src, dst, actor = (
            _name_token_at(node, 0),
            _name_token_at(node, 1),
            _name_token_at(node, 2),
            _name_token_at(node, 3),
        )
        idx = parent
        if name is not None:
            idx = self._add_symbol(
                name,
                "transition",
                _tree_range(node),
                scope_range=local_scope,
                parent=parent,
                detail="transition",
            )
        for token in (src, dst):
            if token is not None:
                self._add_ref(token, "value")
        if actor is not None:
            self._add_ref(actor, "value")
        self._visit_children_after_name_count(node, 4, idx, _tree_range(node))

    def _visit_trans_input(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_children(node, parent, local_scope)

    def _visit_proc_assign(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "field")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_trans_covers(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "control")

    def _visit_kpi_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        name, case_name, stage = _name_token_at(node, 0), _name_token_at(node, 1), _name_token_at(node, 2)
        if name is not None:
            self._add_symbol(name, "kpi", _tree_range(node), parent=parent, detail="kpi")
        if case_name is not None:
            self._add_ref(case_name, "type")
        if stage is not None:
            self._add_ref(stage, "value")

    def _visit_control_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_req_id_symbol(node, parent, "control")

    def _visit_control_owner(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "value")

    def _visit_control_applies_to(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "type")

    def _visit_satisfies_clause(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _req_id_tokens(node):
            self._add_ref(token, "control")

    def _visit_policy_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_req_id_symbol(node, parent, "policy")

    def _visit_policy_eventually(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        case_name, source_stage = _name_token_at(node, 0), _name_token_at(node, 1)
        if case_name is not None:
            self._add_ref(case_name, "type")
        if source_stage is not None:
            self._add_ref(source_stage, "value")
        for child in node.children:
            self.visit(child, parent, local_scope)

    def _visit_policy_precedence(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        case_name = _name_token_at(node, 0)
        if case_name is not None:
            self._add_ref(case_name, "type")
        for child in node.children:
            self.visit(child, parent, local_scope)

    def _visit_goal_def(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_req_id_symbol(node, parent, "goal")

    def _visit_goal_some_stage(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        case_name, stage = _name_token_at(node, 0), _name_token_at(node, 1)
        if case_name is not None:
            self._add_ref(case_name, "type")
        if stage is not None:
            self._add_ref(stage, "value")

    def _visit_goal_all_stage(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        case_name = _name_token_at(node, 0)
        if case_name is not None:
            self._add_ref(case_name, "type")
        for child in node.children:
            self.visit(child, parent, local_scope)

    def _visit_stage_disjunction(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _name_tokens(node):
            self._add_ref(token, "value")

    def _visit_governance_authority(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        authority = _name_token_at(node, 0)
        if authority is not None:
            self._add_symbol(authority, "authority", _tree_range(node), parent=parent, detail="authority")
        for token in _req_id_tokens(node):
            self._add_ref(token, "control")

    def _visit_governance_require(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "control")

    def _visit_governance_satisfaction(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "control")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_governance_policy_ref(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "control")

    def _visit_governance_goal_ref(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "control")

    def _visit_governance_delegates(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _name_token_at(node, 0)
        if token is not None:
            self._add_ref(token, "spec")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_governance_preservation(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        idx = parent
        if token is not None:
            idx = self._add_symbol(token, "property", _tree_range(node), parent=parent, detail="preservation")
        self._visit_children_after_first_token(node, idx, local_scope)

    def _visit_preservation_before(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_preservation_spec_ref(node)

    def _visit_preservation_after(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_preservation_spec_ref(node)

    def _visit_preservation_spec_ref(self, node: Tree) -> None:
        name = _name_token_at(node, 0)
        path_token = _string_token_at(node, 0)
        target_path = _resolve_relative_path(self.path, _unquote_string(path_token)) if path_token else None
        if name is not None:
            self._add_ref(name, "spec", target_path=target_path)

    def _visit_preservation_preserve(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "control")

    def _visit_var(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "value")

    def _visit_t_name(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "type")

    def _visit_cap_name(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "value")

    def _visit_qname(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        names = _name_tokens(node)
        if len(names) == 1:
            self._add_ref(names[0], "type")
        elif len(names) >= 2:
            self._add_ref(names[0], "alias")
            self._add_ref(names[1], "type", qualifier=str(names[0]))

    def _visit_struct_fields(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        for token in _name_tokens(node):
            self._add_ref(token, "field")
        for child in node.children:
            if isinstance(child, Tree):
                self.visit(child, parent, local_scope)

    def _visit_struct_lit(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "type")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_postfix(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        children = list(node.children)
        first_is_alias_field = (
            len(children) >= 2
            and _tree_data(children[0]) == "var"
            and _tree_data(children[1]) == "field_suffix"
        )
        if first_is_alias_field:
            alias_token = _first_token(children[0])
            field_token = _first_token(children[1])
            if alias_token is not None and field_token is not None:
                self._add_ref(field_token, "value", qualifier=str(alias_token))
        for i, child in enumerate(children):
            if _tree_data(child) == "field_suffix":
                if first_is_alias_field and i == 1:
                    continue  # already recorded as alias.member above
                field_token = _first_token(child)
                if field_token is not None:
                    self._add_ref(field_token, "field")
                continue
            self.visit(child, parent, local_scope)

    def _visit_lvalue_var(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "value")

    def _visit_lvalue_index(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "value")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_lvalue_field(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        base, field_token = _name_token_at(node, 0), _name_token_at(node, 1)
        if base is not None:
            self._add_ref(base, "value")
        if field_token is not None:
            self._add_ref(field_token, "field")

    def _visit_lvalue_map_field(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        base, field_token = _name_token_at(node, 0), _name_token_at(node, 1)
        if base is not None:
            self._add_ref(base, "value")
        for child in node.children:
            if isinstance(child, Tree):
                self.visit(child, parent, local_scope)
        if field_token is not None:
            self._add_ref(field_token, "field")

    def _visit_count_e(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_quantifier_like_function(node, parent, local_scope)

    def _visit_sum_e(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        self._visit_quantifier_like_function(node, parent, local_scope)

    def _visit_quantifier_like_function(
        self,
        node: Tree,
        parent: Optional[int],
        local_scope: Optional[Range],
    ) -> None:
        names = _name_tokens(node)
        fn_scope = _tree_range(node)
        if names:
            self._add_symbol(
                names[0],
                "binder",
                _tree_range(node),
                scope_range=fn_scope,
                parent=parent,
                detail="binder",
                exported=False,
                outline=False,
            )
        if len(names) > 1:
            self._add_ref(names[1], "type")
        for child in node.children:
            if isinstance(child, Tree):
                self.visit(child, parent, fn_scope)

    def _visit_children_after_first_token(
        self,
        node: Tree,
        parent: Optional[int],
        local_scope: Optional[Range],
    ) -> None:
        skipped = False
        for child in node.children:
            if not skipped and isinstance(child, Token) and child.type in ("NAME", "REQ_ID"):
                skipped = True
                continue
            self.visit(child, parent, local_scope)

    def _visit_children_after_name_count(
        self,
        node: Tree,
        count: int,
        parent: Optional[int],
        local_scope: Optional[Range],
    ) -> None:
        skipped = 0
        for child in node.children:
            if isinstance(child, Token) and child.type == "NAME" and skipped < count:
                skipped += 1
                continue
            self.visit(child, parent, local_scope)


def _roles_for_reference(role: str) -> set:
    if role == "type":
        return set(TYPE_ROLES)
    if role == "action":
        return set(ACTION_ROLES)
    if role == "property":
        return set(PROPERTY_ROLES)
    if role == "spec":
        return set(SPEC_ROLES)
    if role == "control":
        return set(CONTROL_ROLES)
    if role == "alias":
        return {"alias"}
    if role == "field":
        return {"field"}
    if role == "tool":
        return {"tool"}
    if role == "table":
        return {"table"}
    if role == "column":
        return {"column"}
    if role == "artifact":
        return {"artifact"}
    return set(VALUE_ROLES)


def _scope_sort_key(sym: Symbol) -> Tuple[int, int, int, int]:
    scope = sym.scope_range or sym.range
    return (
        scope.end.line - scope.start.line,
        scope.end.character - scope.start.character,
        -sym.selection_range.start.line,
        -sym.selection_range.start.character,
    )


def _load_import_index(path: str, load_index: Optional[LoadIndex]) -> Optional[DocumentIndex]:
    if load_index is not None:
        try:
            loaded = load_index(path)
        except Exception:
            loaded = None
        if loaded is not None:
            return loaded
    return default_load_index(path)


def _same_location(a: Location, b: Location) -> bool:
    return _normalize_path(a.path) == _normalize_path(b.path) and a.range == b.range


def _declaration_snippet(source: str, sym: Symbol) -> str:
    lines = source.splitlines()
    line_no = sym.range.start.line
    if 0 <= line_no < len(lines):
        return lines[line_no].strip()
    return sym.name


def _token_type_for_role(role: str) -> str:
    return _ROLE_TO_TOKEN_TYPE.get(role, "variable")


def _refinement_spec_names(tree: Tree) -> Tuple[Optional[str], Optional[str]]:
    impl_name = None
    abs_name = None
    for child in tree.children:
        if not isinstance(child, Tree):
            continue
        token = _first_token(child)
        if token is None:
            continue
        if child.data == "refinement_impl":
            impl_name = str(token)
        elif child.data == "refinement_abs":
            abs_name = str(token)
    return impl_name, abs_name


def _top_symbol(index: DocumentIndex) -> Optional[Symbol]:
    tops = [sym for sym in index.symbols if sym.parent is None and sym.role in SPEC_ROLES]
    if not tops:
        return None
    return min(tops, key=lambda sym: sym.selection_range.start_tuple)


def _token_range(token: Token) -> Range:
    return Range(
        Position(token.line - 1, token.column - 1),
        Position((token.end_line or token.line) - 1, (token.end_column or token.column + len(str(token))) - 1),
    )


def _tree_range(tree: Tree) -> Range:
    return Range(
        Position(tree.meta.line - 1, tree.meta.column - 1),
        Position(tree.meta.end_line - 1, tree.meta.end_column - 1),
    )


def _keyword_range(tree: Tree, keyword: str) -> Range:
    rng = _tree_range(tree)
    return Range(
        rng.start,
        Position(rng.start.line, rng.start.character + len(keyword)),
    )


def _tree_data(node: object) -> Optional[str]:
    return node.data if isinstance(node, Tree) else None


def _first_token(tree: Tree, token_types: Sequence[str] = ("NAME", "REQ_ID")) -> Optional[Token]:
    for child in tree.children:
        if isinstance(child, Token) and child.type in token_types:
            return child
    return None


def _name_tokens(tree: Tree) -> List[Token]:
    return [child for child in tree.children if isinstance(child, Token) and child.type == "NAME"]


def _all_name_tokens(node: object) -> List[Token]:
    """Every NAME token anywhere under ``node``, regardless of nesting --
    used for fsl-domain reference lists (``emits``/``awaits``/``waits_for``)
    whose grammar wraps them in inlined/aliased helper rules
    (``one_of_names``/``bracket_name_list``/``name_list``) that would
    otherwise need their own handlers duplicating this same walk."""
    tokens: List[Token] = []
    if isinstance(node, Token):
        if node.type == "NAME":
            tokens.append(node)
    elif isinstance(node, Tree):
        for child in node.children:
            tokens.extend(_all_name_tokens(child))
    return tokens


def _req_id_tokens(tree: Tree) -> List[Token]:
    return [child for child in tree.children if isinstance(child, Token) and child.type == "REQ_ID"]


def _name_token_at(tree: Tree, index: int) -> Optional[Token]:
    tokens = _name_tokens(tree)
    if index >= len(tokens):
        return None
    return tokens[index]


def _string_token_at(tree: Tree, index: int) -> Optional[Token]:
    tokens = [child for child in tree.children if isinstance(child, Token) and child.type == "STRING"]
    if index >= len(tokens):
        return None
    return tokens[index]


def _unquote_string(token: Optional[Token]) -> str:
    if token is None:
        return ""
    text = str(token)
    if len(text) >= 2 and text[0] == '"' and text[-1] == '"':
        return text[1:-1]
    return text


def _normalize_path(path: Optional[str]) -> Optional[str]:
    if path is None:
        return None
    try:
        return str(Path(path).expanduser().resolve(strict=False))
    except Exception:
        return path


def _resolve_relative_path(base_path: Optional[str], relative: str) -> Optional[str]:
    if not relative:
        return None
    rel = Path(relative)
    if rel.is_absolute():
        return _normalize_path(str(rel))
    base_dir = Path(base_path).parent if base_path else Path.cwd()
    return _normalize_path(str(base_dir / rel))
