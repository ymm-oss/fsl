# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

"""Regression tests for the raw-tree LSP index."""
from pathlib import Path

from fslc.lsp.index import (
    SEMANTIC_TOKEN_MODIFIERS,
    SEMANTIC_TOKEN_TYPES,
    CompletionCandidate,
    HoverInfo,
    Range,
    SemanticToken,
    build_index,
    default_load_index,
    encode_semantic_tokens,
)
from fslc.lsp.server import check_source


ROOT = Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"


def _slice(source, rng):
    lines = source.splitlines()
    if rng.start.line == rng.end.line:
        return lines[rng.start.line][rng.start.character:rng.end.character]
    parts = [lines[rng.start.line][rng.start.character:]]
    for line in range(rng.start.line + 1, rng.end.line):
        parts.append(lines[line])
    parts.append(lines[rng.end.line][:rng.end.character])
    return "\n".join(parts)


def _symbol(index, name, role):
    matches = [sym for sym in index.symbols if sym.name == name and sym.role == role]
    assert matches, f"missing symbol {role}:{name}"
    return matches[0]


def _reference(index, name, role, qualifier=None):
    matches = [
        ref for ref in index.references
        if ref.name == name and ref.role == role and ref.qualifier == qualifier
    ]
    assert matches, f"missing reference {role}:{qualifier or ''}.{name}"
    return matches[0]


def _refinement_fixture_indexes():
    paths = {
        "refinement": SPECS / "cart_refines.fsl",
        "impl": SPECS / "cart_impl.fsl",
        "abs": SPECS / "cart_v1.fsl",
    }
    indexes = {
        str(path.resolve()): build_index(path.read_text(encoding="utf-8"), str(path))
        for path in paths.values()
    }
    resolver = {
        "CartImpl": str(paths["impl"].resolve()),
        "ShoppingCart": str(paths["abs"].resolve()),
    }

    def load_index(path):
        return indexes.get(str(Path(path).resolve()))

    def name_resolver(name):
        return resolver.get(name)

    return paths, indexes[str(paths["refinement"].resolve())], load_index, name_resolver


def _definition_for_ref(index, ref, load_index, name_resolver):
    loc = index.definition_at(
        ref.range.start.line,
        ref.range.start.character,
        load_index,
        name_resolver,
    )
    assert loc is not None, f"unresolved reference {ref.role}:{ref.name}"
    return loc


def _target_text(loc):
    assert loc.path is not None
    return _slice(Path(loc.path).read_text(encoding="utf-8"), loc.range)


def _ref(index, name, role, target_spec=None):
    matches = [
        ref for ref in index.references
        if ref.name == name
        and ref.role == role
        and (target_spec is None or ref.target_spec == target_spec)
    ]
    assert matches, f"missing reference {role}:{target_spec or ''}.{name}"
    return matches[0]


def test_raw_tree_index_extracts_spec_definitions_references_and_ranges():
    path = SPECS / "cart_v1.fsl"
    source = path.read_text(encoding="utf-8")
    index = build_index(source, str(path))

    assert _symbol(index, "ShoppingCart", "spec").detail == "spec"
    stock = _symbol(index, "stock", "state_var")
    checkout = _symbol(index, "checkout", "action")
    sold_out = _symbol(index, "SoldOut", "property")

    assert _slice(source, stock.selection_range) == "stock"
    assert _slice(source, checkout.selection_range) == "checkout"
    assert _slice(source, sold_out.selection_range) == "SoldOut"

    stock_ref = _reference(index, "stock", "value")
    stock_loc = index.resolve_reference(stock_ref)
    assert stock_loc is not None
    assert stock_loc.path == str(path.resolve())
    assert stock_loc.range == stock.selection_range

    item_type = _symbol(index, "ItemId", "type")
    item_ref = _reference(index, "ItemId", "type")
    item_loc = index.resolve_reference(item_ref)
    assert item_loc is not None
    assert item_loc.range == item_type.selection_range

    init_i = next(
        sym for sym in index.symbols
        if sym.name == "i" and sym.role == "binder" and sym.scope_range is not None
    )
    init_i_ref = next(
        ref for ref in index.references
        if ref.name == "i"
        and ref.role == "value"
        and init_i.scope_range is not None
        and init_i.scope_range.contains_range_start(ref.range)
    )
    init_i_loc = index.resolve_reference(init_i_ref)
    assert init_i_loc is not None
    assert init_i_loc.range == init_i.selection_range


def test_domain_canonical_enum_is_indexed_with_members():
    source = """domain Orders {
  enum Status { Pending, Approved }
  aggregate Order {
    state { status: Status = Pending; }
  }
}
"""
    index = build_index(source, "orders.fsl")
    assert _symbol(index, "Status", "domain_type").detail == "enum"
    assert _symbol(index, "Pending", "enum_member").detail == "enum member"
    assert _symbol(index, "Approved", "enum_member").detail == "enum member"

    canonical = check_source(source, "orders.fsl")
    assert not any(
        warning.get("code") == "deprecated_domain_enum_union"
        for warning in canonical.get("warnings", [])
    )

    legacy = check_source(
        source.replace("enum Status { Pending, Approved }", "type Status = Pending | Approved"),
        "orders.fsl",
    )
    warning = next(
        warning
        for warning in legacy["warnings"]
        if warning.get("code") == "deprecated_domain_enum_union"
    )
    assert warning["canonical_replacement"] == "enum Status { Pending, Approved }"


def test_compose_definition_resolution_crosses_use_imports():
    path = SPECS / "order_system.fsl"
    source = path.read_text(encoding="utf-8")
    index = build_index(source, str(path))

    assert {binding.alias: binding.path for binding in index.imports} == {
        "cart": "cart_v1.fsl",
        "pay": "payment.fsl",
    }

    checkout_ref = _reference(index, "checkout", "action", "cart")
    assert _slice(source, checkout_ref.range) == "checkout"
    checkout_loc = index.resolve_reference(checkout_ref, default_load_index)
    assert checkout_loc is not None
    assert checkout_loc.path == str((SPECS / "cart_v1.fsl").resolve())
    assert _slice((SPECS / "cart_v1.fsl").read_text(encoding="utf-8"), checkout_loc.range) == "checkout"

    pay_id_ref = _reference(index, "PayId", "type", "pay")
    pay_id_loc = index.resolve_reference(pay_id_ref, default_load_index)
    assert pay_id_loc is not None
    assert pay_id_loc.path == str((SPECS / "payment.fsl").resolve())
    assert _slice((SPECS / "payment.fsl").read_text(encoding="utf-8"), pay_id_loc.range) == "PayId"

    payments_ref = _reference(index, "payments", "value", "pay")
    payments_loc = index.resolve_reference(payments_ref, default_load_index)
    assert payments_loc is not None
    assert payments_loc.path == str((SPECS / "payment.fsl").resolve())
    assert _slice((SPECS / "payment.fsl").read_text(encoding="utf-8"), payments_loc.range) == "payments"


def test_lsp_check_accepts_refinement_mapping_without_state_block_diagnostic():
    paths, _, _, name_resolver = _refinement_fixture_indexes()
    source = paths["refinement"].read_text(encoding="utf-8")

    result = check_source(source, str(paths["refinement"]), name_resolver)

    assert result["result"] == "ok"
    assert "spec has no state block" not in str(result)


def test_refinement_spec_definitions_resolve_across_workspace_names():
    paths, index, load_index, name_resolver = _refinement_fixture_indexes()

    impl_loc = _definition_for_ref(
        index,
        _ref(index, "CartImpl", "spec", "CartImpl"),
        load_index,
        name_resolver,
    )
    assert impl_loc.path == str(paths["impl"].resolve())
    assert _target_text(impl_loc) == "CartImpl"

    abs_loc = _definition_for_ref(
        index,
        _ref(index, "ShoppingCart", "spec", "ShoppingCart"),
        load_index,
        name_resolver,
    )
    assert abs_loc.path == str(paths["abs"].resolve())
    assert _target_text(abs_loc) == "ShoppingCart"


def test_refinement_action_definitions_resolve_to_impl_and_abs_specs():
    paths, index, load_index, name_resolver = _refinement_fixture_indexes()

    checkout_loc = _definition_for_ref(
        index,
        _ref(index, "checkout", "action", "ShoppingCart"),
        load_index,
        name_resolver,
    )
    assert checkout_loc.path == str(paths["abs"].resolve())
    assert _target_text(checkout_loc) == "checkout"

    impl_checkout_loc = _definition_for_ref(
        index,
        _ref(index, "impl_checkout", "action", "CartImpl"),
        load_index,
        name_resolver,
    )
    assert impl_checkout_loc.path == str(paths["impl"].resolve())
    assert _target_text(impl_checkout_loc) == "impl_checkout"

    reserve_loc = _definition_for_ref(
        index,
        _ref(index, "reserve", "action", "CartImpl"),
        load_index,
        name_resolver,
    )
    assert reserve_loc.path == str(paths["impl"].resolve())
    assert _target_text(reserve_loc) == "reserve"


def test_refinement_map_definitions_resolve_lhs_to_abs_and_rhs_to_impl():
    paths, index, load_index, name_resolver = _refinement_fixture_indexes()

    for name in ("stock", "cart"):
        loc = _definition_for_ref(
            index,
            _ref(index, name, "value", "ShoppingCart"),
            load_index,
            name_resolver,
        )
        assert loc.path == str(paths["abs"].resolve())
        assert _target_text(loc) == name

    for name in ("impl_stock", "impl_cart"):
        loc = _definition_for_ref(
            index,
            _ref(index, name, "value", "CartImpl"),
            load_index,
            name_resolver,
        )
        assert loc.path == str(paths["impl"].resolve())
        assert _target_text(loc) == name


def test_refinement_local_binders_still_resolve_in_file():
    paths, index, load_index, name_resolver = _refinement_fixture_indexes()

    i_loc = _definition_for_ref(
        index,
        _ref(index, "i", "value", "CartImpl"),
        load_index,
        name_resolver,
    )
    assert i_loc.path == str(paths["refinement"].resolve())
    assert _target_text(i_loc) == "i"

    u_loc = _definition_for_ref(
        index,
        _ref(index, "u", "value", "ShoppingCart"),
        load_index,
        name_resolver,
    )
    assert u_loc.path == str(paths["refinement"].resolve())
    assert _target_text(u_loc) == "u"


def test_policy_precedence_case_name_and_stages_are_indexed():
    # grammar: policy_precedence: "every" NAME "reaching" stage_disjunction
    #   "must" "have" "passed" "through" stage_disjunction
    # (business-layer no-bypass control, docs/DESIGN-precedence-policy.md;
    # source shape mirrors tests/test_precedence_policy.py's BIZ_COMPLIANT_SRC)
    source = '''business ReturnHandling {
  actor Customer, Manager
  entity Return

  process Return {
    stages Requested, Approved, Rejected, Refunded
    initial Requested
    transition approve Requested -> Approved by Manager
    transition reject Requested -> Rejected by Manager
    transition refund Approved -> Refunded by Manager
  }

  policy CTRL-APPROVAL "no bypass"
    every Return reaching Refunded must have passed through Approved
}
verify {
  instances Return = 3
}
'''
    index = build_index(source)

    _reference(index, "Return", "type")
    _reference(index, "Refunded", "value")
    _reference(index, "Approved", "value")


def test_struct_fields_keys_are_indexed_as_field_references():
    # grammar: struct_fields: "{" NAME ":" expr ("," NAME ":" expr)* ","? "}"
    # (also reached via the ref_struct_fields alias inside refinement maps)
    source = '''spec StructFieldsDemo {
  type K = 0..1

  struct Point { x: K, y: K }

  state {
    p: Point
  }

  init {
    p = Point { x: 0, y: 0 }
  }
}
'''
    index = build_index(source)

    _reference(index, "x", "field")
    _reference(index, "y", "field")


def test_postfix_multi_level_field_access_indexes_tail_as_field_reference():
    # grammar: postfix: atom postfix_suffix*, field_suffix: "." NAME
    # a.b.c: the first field_suffix after a leading var (b) stays the
    # existing alias.member "value" reference (compose namespace lookups
    # depend on it); the second-level field_suffix (c) must now surface as
    # a "field" reference too.
    source = '''spec PostfixFieldChain {
  type K = 0..1

  struct Inner { amount: K }
  struct Outer { payload: Inner }

  state {
    outer: Outer
  }

  invariant TailFieldAccessed {
    outer.payload.amount == 0
  }
}
'''
    index = build_index(source)

    _reference(index, "outer", "value")
    _reference(index, "payload", "value", qualifier="outer")
    _reference(index, "amount", "field")


def test_hover_on_reference_reports_definition_role_and_snippet():
    source = '''spec HoverDemo {
  type K = 0..1

  state {
    counter: K
  }

  init {
    counter = 0
  }

  invariant CounterBounded {
    counter >= 0
  }
}
'''
    index = build_index(source)
    ref = _reference(index, "counter", "value")

    hover = index.hover_at(ref.range.start.line, ref.range.start.character)

    assert isinstance(hover, HoverInfo)
    assert hover.range == ref.range
    assert "state_var" in hover.markdown
    assert "counter" in hover.markdown
    assert "```fsl" in hover.markdown


def test_hover_on_definition_reports_own_role():
    source = '''spec HoverDefDemo {
  type K = 0..1

  state {
    counter: K
  }

  init {
    counter = 0
  }

  action bump() {
    requires counter < 1
    counter = counter + 1
  }
}
'''
    index = build_index(source)
    action_sym = _symbol(index, "bump", "action")

    hover = index.hover_at(
        action_sym.selection_range.start.line,
        action_sym.selection_range.start.character,
    )

    assert isinstance(hover, HoverInfo)
    assert hover.range == action_sym.selection_range
    assert "action" in hover.markdown
    assert "bump" in hover.markdown
    assert "```fsl" in hover.markdown


def test_references_at_collects_all_uses_and_declaration():
    source = '''spec ReferencesDemo {
  type K = 0..2

  state {
    counter: K
  }

  init {
    counter = 0
  }

  action bump() {
    requires counter < 2
    counter = counter + 1
  }

  invariant CounterBounded {
    counter >= 0
  }
}
'''
    index = build_index(source)
    counter_sym = _symbol(index, "counter", "state_var")
    value_refs = [r for r in index.references if r.name == "counter" and r.role == "value"]
    assert len(value_refs) >= 3  # init assignment, guard + body uses, invariant use

    with_decl = index.references_at(
        counter_sym.selection_range.start.line,
        counter_sym.selection_range.start.character,
        include_declaration=True,
    )
    without_decl = index.references_at(
        counter_sym.selection_range.start.line,
        counter_sym.selection_range.start.character,
        include_declaration=False,
    )

    assert all(isinstance(rng, Range) for rng in with_decl)
    assert len(with_decl) == len(value_refs) + 1
    assert len(without_decl) == len(value_refs)
    assert counter_sym.selection_range in with_decl
    assert counter_sym.selection_range not in without_decl
    for ref in value_refs:
        assert ref.range in with_decl
        assert ref.range in without_decl
        assert _slice(source, ref.range) == "counter"
    assert _slice(source, counter_sym.selection_range) == "counter"

    # querying from a usage position aggregates the identical set
    from_usage = index.references_at(
        value_refs[0].range.start.line,
        value_refs[0].range.start.character,
        include_declaration=True,
    )
    assert from_usage == with_decl


def test_semantic_tokens_classify_core_roles():
    source = '''spec SemanticDemo {
  type K = 0..1
  enum Status { Active, Done }

  state {
    counter: K,
    status: Status
  }

  init {
    counter = 0
    status = Active
  }

  action bump() {
    requires counter < 1
    counter = counter + 1
  }
}
'''
    index = build_index(source)
    tokens = index.semantic_tokens()

    assert tokens
    assert all(isinstance(tok, SemanticToken) for tok in tokens)
    assert all(tok.token_type in SEMANTIC_TOKEN_TYPES for tok in tokens)

    by_type = {}
    for tok in tokens:
        by_type.setdefault(tok.token_type, []).append(tok)

    assert "type" in by_type        # type K
    assert "function" in by_type    # action bump
    assert "variable" in by_type    # counter/status state vars and value refs
    assert "enum" in by_type        # enum Status
    assert "enumMember" in by_type  # Active, Done

    assert any("definition" in tok.modifiers for tok in by_type["function"])
    assert any("definition" in tok.modifiers for tok in by_type["enumMember"])


def test_encode_semantic_tokens_roundtrip():
    tokens = [
        SemanticToken(line=2, start_char=4, length=3, token_type="type", modifiers=("definition",)),
        SemanticToken(line=2, start_char=10, length=5, token_type="variable", modifiers=()),
        SemanticToken(
            line=5, start_char=2, length=6, token_type="function",
            modifiers=("definition", "readonly"),
        ),
    ]

    encoded = encode_semantic_tokens(tokens)

    assert all(isinstance(n, int) for n in encoded)
    assert len(encoded) == 5 * len(tokens)

    # first token: absolute deltaLine/deltaStartChar (previous position is (0, 0))
    assert encoded[0:5] == [
        2,
        4,
        3,
        SEMANTIC_TOKEN_TYPES.index("type"),
        1 << SEMANTIC_TOKEN_MODIFIERS.index("definition"),
    ]
    # second token: same line -> deltaLine 0, deltaStartChar relative to previous start
    assert encoded[5:10] == [
        0,
        10 - 4,
        5,
        SEMANTIC_TOKEN_TYPES.index("variable"),
        0,
    ]
    # third token: new line -> deltaLine relative, deltaStartChar absolute again
    expected_mod = (
        (1 << SEMANTIC_TOKEN_MODIFIERS.index("definition"))
        | (1 << SEMANTIC_TOKEN_MODIFIERS.index("readonly"))
    )
    assert encoded[10:15] == [
        5 - 2,
        2,
        6,
        SEMANTIC_TOKEN_TYPES.index("function"),
        expected_mod,
    ]


def test_completion_includes_symbols_and_keywords():
    source = '''spec CompletionDemo {
  type K = 0..1

  state {
    counter: K
  }

  init {
    counter = 0
  }

  invariant CounterBounded {
    counter >= 0
  }
}
'''
    index = build_index(source)
    completions = index.completions_at(0, 0)

    assert all(isinstance(c, CompletionCandidate) for c in completions)
    labels = {c.label for c in completions}
    assert "K" in labels
    assert "CounterBounded" in labels
    assert "invariant" in labels
    assert "forall" in labels

    type_candidates = [c for c in completions if c.label == "K"]
    assert type_candidates and type_candidates[0].role == "type"
    keyword_candidates = [c for c in completions if c.label == "invariant"]
    assert keyword_candidates and keyword_candidates[0].role == "keyword"


def test_completion_member_after_alias_dot():
    path = SPECS / "order_system.fsl"
    source = path.read_text(encoding="utf-8")
    index = build_index(source, str(path))

    lines = source.splitlines()
    line_no = next(i for i, text in enumerate(lines) if "cart." in text)
    character = lines[line_no].index("cart.") + len("cart.")

    completions = index.completions_at(line_no, character, load_index=default_load_index)

    assert completions
    assert all(isinstance(c, CompletionCandidate) for c in completions)
    assert all(c.role != "keyword" for c in completions)

    cart_index = default_load_index(str((SPECS / "cart_v1.fsl").resolve()))
    exported_names = {sym.name for sym in cart_index.symbols if sym.exported}
    labels = {c.label for c in completions}
    assert labels == exported_names


def test_leadsto_helpful_action_name_is_indexed_as_a_reference():
    # Regression: `helpful NAME(...)` inside `leadsTo` used to be a bare
    # Token skipped by the generic child walk, so go-to-definition and
    # find-references never saw the helpful action name.
    source = '''spec HelpfulRefDemo {
  state { x: Int }
  init { x = 0 }
  fair action step() {
    requires x < 3
    x = x + 1
  }
  leadsTo Finishes {
    x == 0 ~> x == 3
    helpful step()
  }
}
'''
    index = build_index(source)
    step_decl = _symbol(index, "step", "action")
    helpful_ref = _reference(index, "step", "action")

    loc = index.definition_at(helpful_ref.range.start.line, helpful_ref.range.start.character)
    assert loc is not None
    assert loc.range == step_decl.selection_range

    refs = index.references_at(
        step_decl.selection_range.start.line,
        step_decl.selection_range.start.character,
    )
    assert helpful_ref.range in refs
    assert step_decl.selection_range in refs


def test_deadline_name_is_a_reference_to_the_declared_age_not_a_new_property():
    # Regression: `deadline NAME <= expr` used to register NAME as a new
    # "property" symbol instead of a reference to the already-declared
    # `age NAME[...]` variable, so it neither resolved to nor showed up in
    # find-references for the age declaration.
    source = '''requirements DeadlineRefDemo {
  type CaseId = 0..0
  enum St { Waiting, Accepted, Responded }

  state {
    cases: Map<CaseId, St>
  }
  init {
    forall c: CaseId { cases[c] = Waiting }
  }

  requirement REQ-1 "accept" {
    fair action accept(c: CaseId) {
      requires cases[c] == Waiting
      cases[c] = Accepted
    }
  }

  time {
    age resp_age[c: CaseId] while cases[c] == Accepted
  }

  requirement REQ-2 "respond within deadline" {
    fair action respond(c: CaseId) {
      requires cases[c] == Accepted
      cases[c] = Responded
    }
    deadline resp_age <= 3
  }
}
'''
    index = build_index(source)
    age_decl = _symbol(index, "resp_age", "state_var")
    assert not any(sym.name == "resp_age" and sym.role == "property" for sym in index.symbols)

    deadline_ref = _reference(index, "resp_age", "state_var")
    loc = index.definition_at(deadline_ref.range.start.line, deadline_ref.range.start.character)
    assert loc is not None
    assert loc.range == age_decl.selection_range

    refs = index.references_at(age_decl.selection_range.start.line, age_decl.selection_range.start.character)
    assert deadline_ref.range in refs


def test_build_index_handles_ai_component_dialect_without_crashing():
    # Regression: build_index hard-coded the kernel-only PARSER, so any
    # ai_component/dbsystem file threw UnexpectedCharacters and every LSP
    # feature (go-to-def, references, hover, semanticTokens) went dark for
    # that file even though `fslc check` (parse_src) handled it fine.
    source = '''ai_component RefDemo {
  tool SearchOrder {
    schema SearchOrderV1;
  }
  authority {
    may_execute SearchOrder;
  }
}
'''
    index = build_index(source, "ref_demo.fsl")
    component = _symbol(index, "RefDemo", "ai_component")
    tool_decl = _symbol(index, "SearchOrder", "tool")
    tool_ref = _reference(index, "SearchOrder", "tool")

    loc = index.definition_at(tool_ref.range.start.line, tool_ref.range.start.character)
    assert loc is not None
    assert loc.range == tool_decl.selection_range
    assert component.range.start.line == 0


def test_build_index_handles_dbsystem_dialect_without_crashing():
    source = '''dbsystem RefDbDemo {
  database app {
    schema 0
    table users {
      column id: Int present not_null;
    }
  }
  artifact server {
    reads users.id;
  }
}
'''
    index = build_index(source, "ref_db_demo.fsl")
    _symbol(index, "RefDbDemo", "dbsystem")
    table_decl = _symbol(index, "users", "table")
    column_decl = _symbol(index, "id", "column")
    table_ref = _reference(index, "users", "table")
    column_ref = _reference(index, "id", "column")

    table_loc = index.definition_at(table_ref.range.start.line, table_ref.range.start.character)
    assert table_loc is not None
    assert table_loc.range == table_decl.selection_range

    column_loc = index.definition_at(column_ref.range.start.line, column_ref.range.start.character)
    assert column_loc is not None
    assert column_loc.range == column_decl.selection_range
