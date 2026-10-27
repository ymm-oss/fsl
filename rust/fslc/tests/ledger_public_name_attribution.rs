// SPDX-License-Identifier: Apache-2.0

#[path = "support/ledger_name_cases.rs"]
mod cases;
#[path = "support/report_cli.rs"]
mod cli;
#[path = "support/ledger_findings.rs"]
#[allow(dead_code)]
mod findings;
#[path = "support/ledger_fixtures.rs"]
mod support;

fn check_cli_cell(case: &cases::Case, key: &str, composed: bool, annotated: bool) {
    let directory = support::directory("name-attribution");
    let component = directory.join("component.fsl");
    std::fs::write(
        &component,
        case.source.replace("OWNER", case.annotation(annotated)),
    )
    .unwrap();
    let path = if composed {
        let path = directory.join("composed.fsl");
        std::fs::write(
            &path,
            "compose Composed { use Subject as ab from \"component.fsl\" }",
        )
        .unwrap();
        path
    } else {
        component
    };
    let extra = [
        "--engine",
        case.engine,
        "--deadlock",
        if case.label == "deadlock" {
            "error"
        } else {
            "ignore"
        },
    ];
    let name = cases::public_name(case.name, composed);
    let (verified, status) = cli::run_json("verify", path.to_str().unwrap(), case.depth, &extra);
    assert_eq!(status, 1, "premise {key}: {verified}");
    assert_eq!(verified["result"], case.result, "premise {key}: {verified}");
    assert_eq!(verified["invariant"], name, "premise {key}: {verified}");
    if let Some(kind) = case.kind {
        assert_eq!(
            verified["violation_kind"], kind,
            "premise {key}: {verified}"
        );
    }
    if let Some(generated) = case.generated {
        assert_eq!(
            verified["generated_name"], generated,
            "premise {key}: {verified}"
        );
    }
    if let Some(generated) = case.action_generated {
        assert_eq!(
            verified["last_action"]["generated_name"], generated,
            "premise {key}: {verified}"
        );
        assert_ne!(
            verified["last_action"]["name"], verified["last_action"]["generated_name"],
            "live authored/generated distinction: {verified}"
        );
    }
    let (body, status) = cli::run("ledger", path.to_str().unwrap(), case.depth, &extra);
    assert_eq!(status, 1, "ledger {key}: {body}");
    cases::assert_ledger_with_migration(&body, case, &name, annotated, path.to_str().unwrap());
}

#[test]
fn obligation_result_namespace_matrix() {
    let mut red = Vec::new();
    let mut green = Vec::new();
    for (case, namespace) in cases::cases()
        .into_iter()
        .map(|case| (case, "native"))
        .chain(
            cases::lowered_cases()
                .into_iter()
                .map(|case| (case, "lowered")),
        )
    {
        for &composed in if namespace == "native" {
            &[false, true][..]
        } else {
            &[false][..]
        } {
            for annotated in [false, true] {
                // Domain lowering always attaches DOMAIN-INVARIANT. Its
                // unlinked renderer control cannot be authored through the CLI.
                if case.implicit_requirement && !annotated {
                    continue;
                }
                let key = case.key(
                    if composed {
                        "compose"
                    } else if namespace == "lowered" {
                        "lowered"
                    } else {
                        "plain"
                    },
                    annotated,
                );
                cases::collect_cell(&mut green, &mut red, key.clone(), || {
                    check_cli_cell(&case, &key, composed, annotated);
                });
            }
        }
    }
    cases::finish_matrix(&green, &red);
}
