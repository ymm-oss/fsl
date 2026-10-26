// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{
    ParseOptions, SourceFile, UnstableFeature, UnstableFeatures, parse_document_with,
};

fn options() -> ParseOptions {
    ParseOptions::with_unstable(UnstableFeatures::NONE.with(UnstableFeature::Fsl5))
}

#[test]
fn expression_positions_have_exact_operator_spans_but_standalone_is_unchanged() {
    for template in [
        "spec S { state { flag: Bool } invariant I { true OP false } }",
        "spec S { state { flag: Bool } action a() { requires true OP false flag = true } }",
        "spec S { state { flag: Bool } invariant I { predicate(true OP false) } }",
    ] {
        for operator in ["||", "->"] {
            let body = template.replace("OP", operator);
            let source = format!("fsl 5\n{body}");
            let error = parse_document_with(SourceFile::new(&source), &options())
                .expect_err("legacy expression");
            assert_eq!(error.code(), "unsupported_in_edition");
            assert_eq!(
                &source[error.span.start.offset..error.span.end.offset],
                operator
            );
            let current = parse_document_with(SourceFile::new(&body), &options())
                .expect_err("current still rejects");
            assert_eq!(current.code(), "FSL-PARSE");
            assert_eq!(
                fsl_syntax::parse_expr(&format!("true {operator} false"))
                    .expect_err("standalone")
                    .code(),
                "FSL-PARSE"
            );
        }
    }
}

#[test]
fn structural_operators_domain_and_literals_are_preserved() {
    for body in [
        "compose C { action both() = a.finish() || b.finish() {} }",
        "refinement R { impl I abs A action finish() -> finish() }",
        "business B { actor User entity Case process Case { stages New, Done initial New transition finish New -> Done by User } }",
        "domain D { enum E { A, B } aggregate X { invariant I { true || false -> true } } }",
        "spec S { state { flag: Bool } @requirement(\"REQ-PROBE-001\", \"literal || ->\") invariant I { true } } // || ->",
    ] {
        let source = format!("fsl 5\n{body}\n");
        parse_document_with(SourceFile::new(&source), &options())
            .expect("structural/domain/literal");
        assert!(fsl_syntax::legacy_expression_operator_error(&source, &options()).is_none());
    }
    for body in [
        "spec S { action a -> finish() }",
        "spec S { state { x: Bool } invariant I { true } -> }",
    ] {
        assert!(
            fsl_syntax::legacy_expression_operator_error(body, &options()).is_none(),
            "not an expression operator: {body}"
        );
    }
}
