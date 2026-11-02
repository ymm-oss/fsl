// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    CoreError, FileResolver, KernelSpec, ModelError, OriginChain, OriginId, OriginSite,
    build_model, parse_kernel_source,
};
use fsl_syntax::{SourcePos, Span};

// Contract: DESIGN-sync-argument-root-validation, Ownership and Error phases;
// DESIGN-check-executable-forms, imported-component diagnostics. These literal
// oracles are independent of the lowering registry and private obligation queue.
struct Piece(String);

impl FileResolver for Piece {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        assert_eq!(path, "piece.fsl", "fixture has exactly one dependency");
        Ok(self.0.clone())
    }
}

fn kernel(formal: &str, references: &str) -> KernelSpec {
    let piece = format!(
        "spec Piece {{\n  type K = 0..1\n  state {{ last: K }}\n  init {{ last = 0 }}\n  action good(a: K) {{}}\n  action put({formal}) {{}}\n}}\n"
    );
    let root = format!(
        "compose Root {{\n  use Piece as part from \"piece.fsl\"\n\n  action step() =\n      {references} {{\n  }}\n\n  internal part.good\n  internal part.put\n}}\n"
    );
    parse_kernel_source(&root, &Piece(piece)).expect("acceptance fixture parses and lowers")
}

fn span(start: usize, end: usize, line: u32, column: u32, end_column: u32) -> Span {
    Span {
        start: SourcePos {
            offset: start,
            line,
            column,
        },
        end: SourcePos {
            offset: end,
            line,
            column: end_column,
        },
    }
}

fn error(message: String, at: Span, origin: OriginChain) -> ModelError {
    ModelError {
        message,
        origin: Some(Box::new(origin)),
        span: Some(at),
        name_resolution: false,
        diagnostic_code: None,
        hint: None,
        quick_fix: None,
    }
}

fn component_error(reason: &str, root_file: Option<&str>) -> ModelError {
    let at = span(17, 20, 2, 3, 6);
    error(
        format!("component \"piece.fsl\" failed to check (part.put: {reason} at piece.fsl:6:3)"),
        at,
        OriginChain {
            id: OriginId("compose:use:17:20".into()),
            dialect: "compose".into(),
            primary: Some(OriginSite {
                source_file: root_file.map(str::to_owned),
                span: Some(at),
                dialect: "compose".into(),
                declaration_path: vec!["use".into(), "piece.fsl".into()],
            }),
            secondary: vec![],
            lowering_steps: vec![],
            generated: false,
        },
    )
}

fn assert_component_rejection(kernel: KernelSpec, reason: &str) {
    for root_file in [None, Some("root.fsl")] {
        let value = match root_file {
            Some(file) => kernel.clone().with_source_file(file),
            None => kernel.clone(),
        };
        let expected = component_error(reason, root_file);
        let location = match root_file {
            Some(file) => format!("{file}:2:3"),
            None => "2:3".into(),
        };
        for (entry, result) in [
            ("build_model", build_model(value.clone()).map(|_| ())),
            (
                "into_validated_syntax",
                value.into_validated_syntax().map(|_| ()),
            ),
        ] {
            let actual = result.expect_err("invalid component formal must reject");
            assert_eq!(actual, expected, "{entry}, root file {root_file:?}");
            assert_eq!(
                actual.to_string(),
                format!("{} at {location}", expected.message)
            );
        }
    }
}

#[test]
fn non_finite_typed_internal_formal_keeps_component_ownership() {
    assert_component_rejection(
        kernel("a: Int", "part.put(0)"),
        "action parameter type is not a finite scalar",
    );
}

#[test]
fn empty_inline_range_internal_formal_keeps_component_ownership() {
    assert_component_rejection(
        kernel("a in 1..0", "part.put(0)"),
        "action parameter range is empty",
    );
}

#[test]
fn internal_formal_declarations_precede_even_an_earlier_invalid_actual() {
    // good(true) is the first queued actual and would fail against finite K.
    // The later put(0) has a valid numeric actual but an invalid declaration.
    for (formal, reason) in [
        ("a: Int", "action parameter type is not a finite scalar"),
        ("a in 1..0", "action parameter range is empty"),
    ] {
        assert_component_rejection(kernel(formal, "part.good(true) || part.put(0)"), reason);
    }
}

#[test]
fn valid_internal_formals_accept_numeric_actuals_and_keep_bad_actuals_root_owned() {
    for (formal, expected_type) in [("a: K", "Named(\"part.K\")"), ("a in 0..1", "Range(0, 1)")] {
        let valid = kernel(formal, "part.put(0)");
        build_model(valid.clone()).expect("valid internal formal and numeric actual");
        valid
            .into_validated_syntax()
            .expect("validated conversion accepts valid internal formal and actual");

        let at = span(86, 90, 5, 16, 20);
        let expected = error(
            format!(
                "invalid model expression: expression of type Bool is not assignable to {expected_type}"
            ),
            at,
            OriginChain {
                id: OriginId("kernel:type-diagnostic:86:90".into()),
                dialect: "kernel".into(),
                primary: Some(OriginSite {
                    source_file: None,
                    span: Some(at),
                    dialect: "kernel".into(),
                    declaration_path: vec![],
                }),
                secondary: vec![],
                lowering_steps: vec![],
                generated: false,
            },
        );
        for value in [
            kernel(formal, "part.put(true)"),
            kernel(formal, "part.put(true)").with_source_file("root.fsl"),
        ] {
            for result in [
                build_model(value.clone()).map(|_| ()),
                value.into_validated_syntax().map(|_| ()),
            ] {
                let actual = result.expect_err("Bool actual must reject against numeric formal");
                assert_eq!(actual, expected, "root actual ownership, formal {formal}");
                assert_eq!(actual.to_string(), format!("{} at 5:16", expected.message));
            }
        }
    }
}
