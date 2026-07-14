// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_syntax::{
    DomainTypeSourceForm, SyntaxBinder, SyntaxExpr, SyntaxExprKind, SyntaxLValue, SyntaxPattern,
    SyntaxTypeExprKind, parse_domain, parse_expr,
};

fn text<'a>(source: &'a str, expression: &SyntaxExpr) -> &'a str {
    &source[expression.span.start.offset..expression.span.end.offset]
}

#[test]
fn domain_expressions_are_typed_spanned_and_preserve_legacy_operators() {
    let source = r"domain TypedExpressions {
  type Index = 0..2
  type Status = Pending | Approved | Rejected
  aggregate Order {
    state {
      status: Status = Pending
      slots: Map<Index, Status>
    }
    command Approve {}
    event Approved {}
    error CannotApprove
    decide Approve {
      requires status == Pending || status == Rejected -> can(Approve)
      rejects CannotApprove when status in [Approved, Rejected]
      emits Approved
    }
    evolve Approved {
      slots[1 + 1].value = status
    }
  }
}";

    let domain = parse_domain(source).expect("parse typed domain expressions");
    let aggregate = &domain.aggregates[0];
    let status = &aggregate.state[0];
    assert_eq!(status.name.text, "status");
    assert_eq!(
        &source[status.name.span.start.offset..status.name.span.end.offset],
        "status"
    );
    let SyntaxTypeExprKind::Name(type_name) = &status.type_name.kind else {
        panic!("expected named field type: {:?}", status.type_name);
    };
    assert_eq!(type_name.text, "Status");
    assert_eq!(
        &source[status.span.start.offset..status.span.end.offset],
        "status: Status = Pending"
    );
    let requirement = &aggregate.decides[0].requires[0];

    assert_eq!(
        text(source, requirement),
        "status == Pending || status == Rejected -> can(Approve)"
    );
    let SyntaxExprKind::Binary { op, left, right } = &requirement.kind else {
        panic!("expected implication root: {requirement:?}");
    };
    assert_eq!(op.canonical, "=>");
    assert_eq!(op.spelling, "->");
    assert_eq!(&source[op.span.start.offset..op.span.end.offset], "->");
    let SyntaxExprKind::Binary { op, .. } = &left.kind else {
        panic!("expected legacy disjunction: {left:?}");
    };
    assert_eq!(op.canonical, "or");
    assert_eq!(op.spelling, "||");
    let SyntaxExprKind::Call { callee, args } = &right.kind else {
        panic!("expected unresolved can() call: {right:?}");
    };
    assert_eq!(callee.text, "can");
    assert_eq!(
        &source[callee.span.start.offset..callee.span.end.offset],
        "can"
    );
    assert!(matches!(args[0].kind, SyntaxExprKind::Name(_)));

    let rejection = &aggregate.decides[0].rejects[0].condition;
    let SyntaxExprKind::Membership { value, members } = &rejection.kind else {
        panic!("expected finite membership: {rejection:?}");
    };
    assert!(matches!(value.kind, SyntaxExprKind::Name(_)));
    assert_eq!(members.len(), 2);
    assert_eq!(text(source, &members[0]), "Approved");
    assert_eq!(text(source, &members[1]), "Rejected");

    let assignment = &aggregate.evolves[0].assignments[0];
    let SyntaxLValue::Field { base, field, span } = &assignment.target else {
        panic!("expected field lvalue: {:?}", assignment.target);
    };
    assert_eq!(field.text, "value");
    assert_eq!(
        &source[span.start.offset..span.end.offset],
        "slots[1 + 1].value"
    );
    let SyntaxLValue::Index { base, index, .. } = base.as_ref() else {
        panic!("expected indexed lvalue base: {base:?}");
    };
    assert!(matches!(base.as_ref(), SyntaxLValue::Name(_)));
    assert!(matches!(index.kind, SyntaxExprKind::Binary { .. }));
    assert_eq!(text(source, &assignment.value), "status");
    assert_eq!(
        &source[assignment.span.start.offset..assignment.span.end.offset],
        "slots[1 + 1].value = status"
    );
}

#[test]
fn malformed_domain_expression_reports_the_original_source_span() {
    let source = r"domain Broken {
  aggregate Order {
    command Approve {}
    decide Approve {
      requires status ==
      emits Approved
    }
  }
}";

    let error = parse_domain(source).expect_err("broken requires must fail in domain parser");
    assert_eq!(error.span.start.line, 5);
    assert!(error.span.start.column >= 25);
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_domain_expression_position_uses_structured_syntax() {
    let source = r"domain TypedEverywhere {
  type Id = 0..2
  value_object Stamp {
    value: Id = 1
    invariant bounded { value >= 0 }
  }
  aggregate Order {
    state { count: Id = 0 }
    command Change {}
    event Changed {}
    event Rejected {}
    decide Change {
      requires count < 2
      rejects CannotChange when count == 2
      emits Changed
    }
    error CannotChange
    evolve Changed {
      requires count < 2
      count = count + 1
    }
    on_stale Changed when count > 0 { emits Rejected }
    invariant valid { count >= 0 }
  }
  effect Notify {
    idempotency_key Order.id
    correlation_id Changed.id
    handles Changed
    emits Rejected
  }
  await Routing {
    waits_for one_of [Changed]
    on Changed -> Rejected
  }
  saga Flow {
    step Run {
      requires Changed and not Rejected
      emits Changed
      awaits one_of [Changed]
    }
    invariant terminal { Changed -> not Rejected }
  }
}";

    let domain = parse_domain(source).expect("parse every domain expression position");
    let range = &domain.types[0];
    assert!(matches!(
        range.lo.as_ref().map(|expression| &expression.kind),
        Some(SyntaxExprKind::Num(0))
    ));
    assert!(matches!(
        range.hi.as_ref().map(|expression| &expression.kind),
        Some(SyntaxExprKind::Num(2))
    ));

    let value_object = &domain.types[1];
    assert!(matches!(
        value_object.fields[0]
            .default
            .as_ref()
            .map(|expression| &expression.kind),
        Some(SyntaxExprKind::Num(1))
    ));
    assert!(matches!(
        value_object.invariants[0].expr.kind,
        SyntaxExprKind::Binary { .. }
    ));

    let aggregate = &domain.aggregates[0];
    assert!(matches!(
        aggregate.state[0]
            .default
            .as_ref()
            .map(|expression| &expression.kind),
        Some(SyntaxExprKind::Num(0))
    ));
    assert!(matches!(
        aggregate.decides[0].requires[0].kind,
        SyntaxExprKind::Binary { .. }
    ));
    assert!(matches!(
        aggregate.decides[0].rejects[0].condition.kind,
        SyntaxExprKind::Binary { .. }
    ));
    assert!(matches!(
        aggregate.evolves[0].requires[0].kind,
        SyntaxExprKind::Binary { .. }
    ));
    assert!(matches!(
        aggregate.evolves[0].assignments[0].value.kind,
        SyntaxExprKind::Binary { .. }
    ));
    assert!(matches!(
        aggregate.stale_policies[0].condition.kind,
        SyntaxExprKind::Binary { .. }
    ));
    assert!(matches!(
        aggregate.invariants[0].expr.kind,
        SyntaxExprKind::Binary { .. }
    ));

    let effect = &domain.effects[0];
    assert!(matches!(
        effect
            .idempotency_key
            .as_ref()
            .map(|expression| &expression.kind),
        Some(SyntaxExprKind::Field { .. })
    ));
    assert!(matches!(
        effect
            .correlation_id
            .as_ref()
            .map(|expression| &expression.kind),
        Some(SyntaxExprKind::Field { .. })
    ));

    assert_eq!(
        domain.awaits[0].branches[0],
        ("Changed".into(), "Rejected".into())
    );
    assert!(matches!(
        domain.sagas[0].steps[0].requires[0].kind,
        SyntaxExprKind::Binary { .. }
    ));
    let SyntaxExprKind::Binary { op, .. } = &domain.sagas[0].invariants[0].expr.kind else {
        panic!("expected implication in saga invariant");
    };
    assert_eq!(op.canonical, "=>");
    assert_eq!(op.spelling, "->");
}

#[test]
fn unsupported_double_ampersand_remains_a_lexer_error() {
    let source = "domain Broken { aggregate A { invariant bad { left && right } } }";
    let error = parse_domain(source).expect_err("&& is not part of the language");
    assert!(error.message.contains("unexpected character '&'"));
}

#[test]
fn effect_references_remain_dotted_identifier_paths() {
    for invalid in ["1 + 2", "build_key()", "items[0]"] {
        let source = format!("domain Broken {{ effect E {{ correlation_id {invalid} }} }}");
        let error = parse_domain(&source).expect_err("effect reference shape must stay restricted");
        assert_eq!(
            error.message,
            "effect reference must be a dotted identifier path"
        );
    }
}

#[test]
fn shared_expression_parser_preserves_trailing_comma_compatibility() {
    for source in ["call(1,)", "Set { 1, }", "items.contains(1,)"] {
        parse_expr(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
}

#[test]
fn domain_type_references_reject_more_than_two_arguments() {
    let source = "domain Broken { aggregate A { state { values: Map<Id, Id, Id> } } }";
    let error = parse_domain(source).expect_err("domain type arity must not expand");
    assert_eq!(error.message, "expected '>'");
}

#[test]
fn public_helper_nodes_retain_their_own_spans() {
    let source = r"domain SpannedHelpers {
  type Id = 0..1
  aggregate A {
    invariant quantified { forall item: acme.types.Id { item == 0 } }
    invariant patterned { maybe is some(value) }
  }
}";
    let domain = parse_domain(source).expect("parse helper syntax nodes");

    let quantified = &domain.aggregates[0].invariants[0];
    assert_eq!(quantified.name.text, "quantified");
    assert_eq!(
        &source[quantified.span.start.offset..quantified.span.end.offset],
        "invariant quantified { forall item: acme.types.Id { item == 0 } }"
    );
    let SyntaxExprKind::Quantified { binder, .. } = &quantified.expr.kind else {
        panic!("expected quantified expression");
    };
    let SyntaxBinder::Typed {
        type_name, span, ..
    } = binder
    else {
        panic!("expected typed binder");
    };
    assert_eq!(
        &source[span.start.offset..span.end.offset],
        "item: acme.types.Id"
    );
    let type_span = type_name.span();
    assert_eq!(
        &source[type_span.start.offset..type_span.end.offset],
        "acme.types.Id"
    );
    assert_eq!(type_name.path.segments(), ["acme", "types", "Id"]);
    assert_eq!(type_name.path.segment_spans().len(), 3);
    assert_eq!(
        type_name
            .path
            .segment_spans()
            .iter()
            .map(|span| &source[span.start.offset..span.end.offset])
            .collect::<Vec<_>>(),
        ["acme", "types", "Id"]
    );

    let patterned = &domain.aggregates[0].invariants[1].expr;
    let SyntaxExprKind::Is { pattern, .. } = &patterned.kind else {
        panic!("expected pattern expression");
    };
    let SyntaxPattern::Some { name, span } = pattern else {
        panic!("expected some pattern");
    };
    assert_eq!(name.text, "value");
    assert_eq!(&source[span.start.offset..span.end.offset], "some(value)");
}

#[test]
fn canonical_enum_and_legacy_union_retain_source_form_spans_and_comments() {
    let source = r"domain Types {
  enum Status {
    Pending, // keep the pending comment
    Approved,
  }
  type Legacy = Open | // keep legacy union trivia
    Closed
  type Quantity = 0..100
}
";
    let domain = parse_domain(source).expect("parse canonical and legacy declarations");
    let canonical = &domain.types[0];
    assert_eq!(canonical.source_form, DomainTypeSourceForm::CanonicalEnum);
    assert_eq!(canonical.members, ["Pending", "Approved"]);
    assert_eq!(canonical.member_spans.len(), 2);
    assert!(
        source[canonical.span.start.offset..canonical.span.end.offset]
            .contains("// keep the pending comment")
    );
    assert_eq!(
        &source[canonical.member_spans[1].start.offset..canonical.member_spans[1].end.offset],
        "Approved"
    );

    let legacy = &domain.types[1];
    assert_eq!(legacy.source_form, DomainTypeSourceForm::LegacyEnumUnion);
    assert_eq!(legacy.members, ["Open", "Closed"]);
    assert_eq!(legacy.member_spans.len(), 2);
    assert!(
        source[legacy.span.start.offset..legacy.span.end.offset]
            .contains("// keep legacy union trivia")
    );

    let range = &domain.types[2];
    assert_eq!(range.source_form, DomainTypeSourceForm::CanonicalRange);
    assert!(range.members.is_empty());
}
