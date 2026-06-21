# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Raw Lark tree index used by the FSL language server.

This module intentionally reads ``fslc.grammar.PARSER`` directly and does not
touch the tuple AST transformer or verifier kernel. FSL keywords are mostly
contextual, so declarations and references are classified by parse-tree
position, not by spelling.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Dict, Iterable, List, Optional, Sequence, Tuple

from lark import Tree, Token

from fslc.grammar import PARSER


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


def build_index(source: str, path: Optional[str] = None) -> DocumentIndex:
    """Parse ``source`` and return a raw-tree symbol/reference index."""

    tree = PARSER.parse(source)
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
        token = _first_token(node)
        if token is not None:
            self._add_symbol(token, "property", _tree_range(node), parent=parent, detail="deadline")
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

    def _visit_struct_lit(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        token = _first_token(node)
        if token is not None:
            self._add_ref(token, "type")
        self._visit_children_after_first_token(node, parent, local_scope)

    def _visit_postfix(self, node: Tree, parent: Optional[int], local_scope: Optional[Range]) -> None:
        children = list(node.children)
        if len(children) >= 2 and _tree_data(children[0]) == "var" and _tree_data(children[1]) == "field_suffix":
            alias_token = _first_token(children[0])
            field_token = _first_token(children[1])
            if alias_token is not None and field_token is not None:
                self._add_ref(field_token, "value", qualifier=str(alias_token))
        for child in children:
            if _tree_data(child) == "field_suffix":
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
