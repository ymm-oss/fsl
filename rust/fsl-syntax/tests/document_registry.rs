// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_syntax::{
    Annotation, AnnotationValue, DiagnosticCode, Dialect, FrontendRegistration, SourceFile,
    SurfaceDocument, parse_document, supported_dialect_keywords, validate_frontend_registry,
};

fn span_at(offset: usize) -> fsl_syntax::Span {
    let position = fsl_syntax::SourcePos {
        offset,
        line: 1,
        column: u32::try_from(offset + 1).unwrap(),
    };
    fsl_syntax::Span {
        start: position,
        end: position,
    }
}

const DIALECTS: [(&str, Dialect); 10] = [
    ("spec", Dialect::Spec),
    ("refinement", Dialect::Refinement),
    ("compose", Dialect::Compose),
    ("business", Dialect::Business),
    ("governance", Dialect::Governance),
    ("requirements", Dialect::Requirements),
    ("domain", Dialect::Domain),
    ("dbsystem", Dialect::DbSystem),
    ("ai_component", Dialect::AiComponent),
    ("agent", Dialect::Agent),
];

#[test]
fn registry_classifies_every_supported_top_level_keyword() {
    for (keyword, dialect) in DIALECTS {
        let source = format!("{keyword} Demo {{}}");
        let header = fsl_syntax::classify_document(&SourceFile::anonymous(&source))
            .unwrap_or_else(|error| panic!("classify {keyword}: {error}"));
        assert_eq!(header.dialect, dialect, "{keyword}");
    }
    assert_eq!(
        supported_dialect_keywords(),
        DIALECTS.map(|(keyword, _)| keyword).as_slice()
    );
}

#[test]
fn every_registered_frontend_parses_from_the_shared_document_entrypoint() {
    let sources = [
        "spec Demo {}",
        "refinement Demo {}",
        "compose Demo {}",
        "business Demo {}",
        "governance Demo {}",
        "requirements Demo {}",
        "domain Demo {}",
        "dbsystem Demo { database app { schema 0 } }",
        "ai_component Demo {}",
        "agent Demo {}",
    ];
    for (source, (keyword, dialect)) in sources.into_iter().zip(DIALECTS) {
        let parsed = parse_document(&SourceFile::anonymous(source))
            .unwrap_or_else(|error| panic!("parse {keyword}: {error}"));
        assert_eq!(parsed.dialect, dialect, "{keyword}");
    }
}

#[test]
fn dispatch_skips_bom_comments_whitespace_and_top_level_annotations() {
    let source = "\u{feff} // leading\n\n@acme.review.owner(\"language\", 247, true, policy.strict)\n spec Demo {}";
    let parsed = parse_document(&SourceFile::anonymous(source)).expect("parse annotated spec");

    assert_eq!(parsed.dialect, Dialect::Spec);
    assert!(matches!(parsed.surface, SurfaceDocument::Spec(_)));
    let [
        Annotation::Custom {
            namespace,
            arguments,
            span,
        },
    ] = parsed.annotations.source_order()
    else {
        panic!("expected one custom document annotation")
    };
    assert_eq!(
        namespace
            .segments()
            .iter()
            .map(fsl_syntax::SyntaxIdent::as_str)
            .collect::<Vec<_>>(),
        ["acme", "review", "owner"]
    );
    assert_eq!(namespace.span().start.line, 3);
    assert_eq!(namespace.span().end.column, 19);
    assert_eq!(span.start.line, 3);
    assert_eq!(arguments.len(), 4);
    assert!(matches!(arguments[0], AnnotationValue::String(ref value) if value == "language"));
    assert!(matches!(arguments[1], AnnotationValue::Integer(247)));
    assert!(matches!(arguments[2], AnnotationValue::Boolean(true)));
    assert!(
        matches!(arguments[3], AnnotationValue::Symbol(ref value) if value.to_string() == "policy.strict")
    );
}

#[test]
fn annotation_argument_keywords_do_not_select_the_frontend() {
    let source = "@routing(spec, domain, ai_component) business Demo {}";
    let header = fsl_syntax::classify_document(&SourceFile::anonymous(source)).unwrap();
    assert_eq!(header.dialect, Dialect::Business);
    assert_eq!(header.declaration_span.start.column, 38);
}

#[test]
fn qualified_syntax_uses_multi_segment_symbol_paths_and_structural_equality() {
    let source = "spec Multi { state { value: acme.types.Status } }";
    let parsed = parse_document(&SourceFile::anonymous(source)).expect("parse qualified type");
    let SurfaceDocument::Spec(spec) = parsed.surface else {
        panic!("expected spec")
    };
    let fsl_syntax::SpecItem::State(fields) = &spec.items[0] else {
        panic!("expected state")
    };
    assert_eq!(
        fields[0].ty,
        fsl_syntax::TypeExpr::Name("acme.types.Status".to_owned())
    );

    let first = fsl_syntax::SymbolPath::new(
        ["acme".to_owned(), "types".to_owned(), "Status".to_owned()],
        span_at(1),
    )
    .unwrap();
    let second = fsl_syntax::SymbolPath::new(
        ["acme".to_owned(), "types".to_owned(), "Status".to_owned()],
        span_at(99),
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.to_string(), "acme.types.Status");
}

#[test]
fn duplicate_registry_keys_are_rejected() {
    let duplicate = [
        FrontendRegistration::new("spec", Dialect::Spec),
        FrontendRegistration::new("spec", Dialect::Business),
    ];
    let error = validate_frontend_registry(&duplicate).expect_err("duplicate key");
    assert_eq!(error.code(), DiagnosticCode::DuplicateDialectKey);
    assert!(error.to_string().contains("spec"));
}

#[test]
fn empty_and_unknown_documents_have_stable_structured_diagnostics() {
    let empty_source = SourceFile::anonymous("\u{feff}// nothing here\n  ");
    let empty = parse_document(&empty_source).expect_err("empty document");
    assert_eq!(empty.code(), DiagnosticCode::EmptyDocument);
    assert_eq!(empty.span.start.offset, empty_source.text().len());
    assert!(empty.supported_keywords().is_empty());

    let unknown =
        parse_document(&SourceFile::anonymous("\n mystery Demo {}")).expect_err("unknown dialect");
    assert_eq!(unknown.code(), DiagnosticCode::UnsupportedDialect);
    assert_eq!((unknown.span.start.line, unknown.span.start.column), (2, 2));
    assert_eq!(unknown.supported_keywords(), supported_dialect_keywords());
}

#[test]
fn agent_is_dispatched_through_the_shared_document_api() {
    let parsed = parse_document(&SourceFile::anonymous(
        "// agent fixture\nagent Parent { agent Child {} }",
    ))
    .expect("parse agent document");
    let SurfaceDocument::Agent(agent) = parsed.surface else {
        panic!("expected agent surface")
    };
    assert_eq!(parsed.dialect, Dialect::Agent);
    assert_eq!(agent.name, "Parent");
    assert_eq!(agent.span.start.line, 2);
}

#[test]
fn ai_project_is_a_semantic_variant_of_the_ai_component_registry_frontend() {
    let source = r"ai_component Demo {}
statistical_property Quality { target Demo }
";
    let parsed = parse_document(&SourceFile::anonymous(source)).expect("parse AI project");
    assert_eq!(parsed.dialect, Dialect::AiComponent);
    assert!(matches!(parsed.surface, SurfaceDocument::AiProject(_)));
}
