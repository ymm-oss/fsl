// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::{FsResolver, ModelError, build_model, parse_kernel_source};

struct BinderPathCase {
    owner: &'static str,
    form: &'static str,
    source: &'static str,
    line: u32,
    column: u32,
}

const BINDER_PATH_CASES: [BinderPathCase; 13] = [
    BinderPathCase {
        owner: "invariant",
        form: "forall expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  invariant P { forall x: Missing { true } }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "invariant",
        form: "exists expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  invariant P { exists x: Missing { true } }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "invariant",
        form: "aggregate expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  invariant P { count(x: Missing) == 0 }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "ensures",
        form: "forall expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  action stay() { flag = flag ensures forall x: Missing { true } }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "ensures",
        form: "exists expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  action stay() { flag = flag ensures exists x: Missing { true } }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "ensures",
        form: "aggregate expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  action stay() { flag = flag ensures count(x: Missing) == 0 }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "reachable",
        form: "forall expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  reachable P { forall x: Missing { true } }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "reachable",
        form: "exists expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  reachable P { exists x: Missing { true } }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "reachable",
        form: "aggregate expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = false }\n  reachable P { count(x: Missing) == 0 }\n}",
        line: 4,
        column: 3,
    },
    BinderPathCase {
        owner: "init",
        form: "forall statement",
        source: "spec Bad {\n  state { flag: Bool }\n  init { forall x: Missing { flag = false } }\n}",
        line: 3,
        column: 10,
    },
    BinderPathCase {
        owner: "init",
        form: "forall expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { flag = forall x: Missing { true } }\n}",
        line: 3,
        column: 10,
    },
    BinderPathCase {
        owner: "init",
        form: "exists expression",
        source: "spec Bad {\n  state { flag: Bool }\n  init { if exists x: Missing { true } { flag = false } }\n}",
        line: 3,
        column: 10,
    },
    BinderPathCase {
        owner: "init",
        form: "aggregate expression",
        source: "spec Bad {\n  state { total: Int }\n  init { total = count(x: Missing) }\n}",
        line: 3,
        column: 10,
    },
];

fn rejected_model(case: &BinderPathCase) -> ModelError {
    let kernel = parse_kernel_source(case.source, &FsResolver::new("."))
        .unwrap_or_else(|error| panic!("{} / {} did not lower: {error}", case.owner, case.form));
    match build_model(kernel) {
        Err(error) => error,
        Ok(model) => panic!(
            "{} / {} unexpectedly built: {}",
            case.owner, case.form, model.name
        ),
    }
}

/// Mechanical inventory of every authored owner called out by issue #832.
///
/// Quantified and aggregate expressions recurse through the same
/// `validate_expression` arms regardless of owner. Init additionally has a
/// statement-form `forall`, so it gets one row for that distinct AST path and
/// separate expression rows for `forall`, `exists`, and aggregates.
#[test]
fn unknown_typed_binders_are_rejected_in_every_authored_owner_and_ast_form() {
    for case in &BINDER_PATH_CASES {
        let error = rejected_model(case);
        assert_eq!(
            error.message,
            if case.owner == "init" {
                "invalid init statement: unknown type 'Missing'"
            } else {
                "invalid model expression: unknown type 'Missing'"
            },
            "{} / {}",
            case.owner,
            case.form
        );
        let span = error
            .origin
            .as_deref()
            .and_then(|origin| origin.primary.as_ref())
            .and_then(|site| site.span)
            .or(error.span)
            .unwrap_or_else(|| panic!("{} / {} lost its location", case.owner, case.form));
        assert_eq!(
            (span.start.line, span.start.column),
            (case.line, case.column),
            "{} / {}",
            case.owner,
            case.form
        );
    }
}

#[test]
fn real_typed_binder_remains_accepted() {
    let source = "spec Good {\n  type Item = 0..1\n  state { flag: Bool }\n  init { flag = false }\n  invariant P { forall x: Item { true } }\n}";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("lower valid binder");
    build_model(kernel).expect("build valid binder");
}

/// Every spec-like dialect that can carry the shared expression AST either
/// reaches the checked-model binder gate or rejects the name earlier while
/// resolving its own typed domain vocabulary.
#[test]
fn all_binder_capable_dialects_reject_unknown_typed_names() {
    let checked_model_cases = [
        (
            "spec",
            "spec Bad { state { flag: Bool } init { flag = false } invariant P { forall x: Missing { true } } }",
        ),
        (
            "requirements",
            "requirements Bad { process Claim { stages Draft, Done initial Draft transition finish Draft -> Done by System } invariant P { forall x: Missing { true } } } verify { instances Claim = 1 }",
        ),
        (
            "business",
            "business Bad { actor System entity Claim process Claim { stages Draft, Done initial Draft transition finish Draft -> Done by System } goal G \"bad\" { forall x: Missing { true } } } verify { instances Claim = 1 }",
        ),
        (
            "compose",
            "compose Bad { state { flag: Bool } init { flag = false } invariant P { forall x: Missing { true } } }",
        ),
    ];

    for (dialect, source) in checked_model_cases {
        let kernel = parse_kernel_source(source, &FsResolver::new("."))
            .unwrap_or_else(|error| panic!("{dialect} did not lower: {error}"));
        let Err(error) = build_model(kernel) else {
            panic!("{dialect} accepted unknown binder type");
        };
        assert!(
            error.message.contains("unknown type 'Missing'"),
            "{dialect}: {error}"
        );
    }

    let domain = "domain Bad { type Status = Draft | Done aggregate Claim { state { status: Status = Draft; } command Finish {} event Finished {} decide Finish { emits Finished } evolve Finished { status = Done } invariant bad { forall x: Missing { true } } } }";
    let error = parse_kernel_source(domain, &FsResolver::new("."))
        .expect_err("domain type resolution must reject an unknown binder type");
    assert!(error.message.contains("Missing"), "domain: {error}");
}
