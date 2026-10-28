// SPDX-License-Identifier: Apache-2.0

#[path = "edition_dependency_cases.rs"]
mod cases;
#[path = "edition_cases.rs"]
mod common;
use super::*;

fn fixture() -> (common::Scratch, std::path::PathBuf, String) {
    let scratch = common::Scratch::new();
    scratch.write("dep.fsl", &format!("fsl 5\n{}", cases::LEGACY_LEAF));
    let source = "compose C { use S as s from \"dep.fsl\" }\n".to_owned();
    let path = scratch.write("root.fsl", &source);
    (scratch, path, source)
}

fn consume(path: &Path, source: &str) -> (Value, i32) {
    match load_kernel_model_from_source(path, source) {
        Ok(_) => (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0),
        Err(error) => (spec_load_error_output(&error), 2),
    }
}

fn dependency_edition_o01_fresh_thread_has_no_owner_and_fails_closed() {
    assert!(
        crate::parse_options()
            .unstable()
            .contains(fsl_syntax::UnstableFeature::Fsl5),
        "run this adapter with FSLC_UNSTABLE=fsl5"
    );
    let (scratch, path, source) = fixture();
    scratch.write("dep.fsl", cases::LEAF);
    let canonical =
        std::thread::scope(|scope| scope.spawn(|| consume(&path, &source)).join().unwrap());
    assert_eq!(
        canonical,
        (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0),
        "canonical no-owner loader must really build a model"
    );
    scratch.write("dep.fsl", &format!("fsl 5\n{}", cases::LEGACY_LEAF));
    let result = std::thread::spawn(move || consume(&path, &source))
        .join()
        .unwrap();
    println!(
        "DEPENDENCY O01 produced={result:?} expected=nonzero and no Kernel schema/model/solver output"
    );
    let mut observations = cases::Observations::default();
    observations.compare(
        "O01",
        &result,
        "error/nonzero and no model",
        result.1 != 0 && result.0["result"] == "error" && result.0.get("$schema").is_none(),
    );
    observations.finish();
}

fn lenient_helper(id: &str, model: &KernelModel, path: &Path, source: &str) -> Value {
    match id {
        "P01" => reachable_counterfactuals_from_source(model, path, source, 2),
        "P02" => invariant_counterfactuals_from_source(model, path, source, 2),
        "P03" => json!(document_model_failure(source, path.parent().unwrap()).is_some()),
        "P04" => json!(readable_implements_text_from_source(path, source, model, 2)),
        _ => unreachable!(),
    }
}

fn dependency_edition_p01_p04_lenient_helpers_cannot_swallow_refusal() {
    let mut observations = cases::Observations::default();
    for id in ["P01", "P02", "P03", "P04"] {
        let (scratch, path, compose) = fixture();
        let source = if id == "P04" {
            cases::IMPLEMENTS
        } else {
            &compose
        };
        // Prepare names from a canonical twin before opening the critical
        // owner. Only the lenient helper reads the rejected child in that owner.
        let body = if id == "P01" {
            cases::LEAF
                .replace("{ done = true }", "{ requires false done = true }")
                .replace(
                    "invariant Typed",
                    "reachable Reached { done } invariant Typed",
                )
        } else {
            cases::LEAF.into()
        };
        scratch.write("dep.fsl", &body);
        let model = load_model_from_source(&path, source).unwrap();
        let body = if id == "P03" {
            "spec S { state {} invariant Typed { true } }\n".into()
        } else {
            body
        };
        scratch.write("dep.fsl", &body);
        let run = || {
            run_admitted_root(&path, Ok(source.into()), None, false, |source| {
                (
                    json!({"result":"explained","suggestions":lenient_helper(id,&model,&path,source)}),
                    0,
                )
            })
        };
        let positive = run();
        let repeated = run();
        assert_eq!(positive.1, 0);
        assert_eq!(repeated.1, 0);
        assert_eq!(cases::stable(positive.0.clone()), cases::stable(repeated.0));
        match id {
            "P01" | "P02" => assert!(
                !positive.0["suggestions"].as_array().unwrap().is_empty(),
                "canonical helper must reach a real result"
            ),
            "P03" => assert_eq!(positive.0["suggestions"], true),
            "P04" => assert_eq!(positive.0["suggestions"], "  - S: refines\n"),
            _ => unreachable!(),
        }
        let rejected = format!(
            "fsl 5\n{}",
            body.replace(
                "invariant Typed",
                "invariant Typed \"REQ-DEP-001: 日本語😀\""
            )
        );
        scratch.write("dep.fsl", &rejected);
        let produced = run();
        let child = scratch.0.join("dep.fsl").canonicalize().unwrap();
        let expected = (
            cases::refusal(&rejected, child.to_str().unwrap(), envelope()),
            2,
        );
        observations.compare(id, &produced, &expected, produced == expected);
    }
    observations.finish();
}

fn dependency_edition_o02_o03_sequential_owners_do_not_share_records() {
    let mut observations = cases::Observations::default();
    let (scratch, path, source) = fixture();
    let run = || {
        run_admitted_root(&path, Ok(source.clone()), None, false, |source| {
            consume(&path, source)
        })
    };
    let first = run();
    let child = scratch.0.join("dep.fsl").canonicalize().unwrap();
    let expected_a = (
        cases::refusal(
            &format!("fsl 5\n{}", cases::LEGACY_LEAF),
            child.to_str().unwrap(),
            envelope(),
        ),
        2,
    );
    observations.compare("O02/A", &first, &expected_a, first == expected_a);
    scratch.write("dep.fsl", cases::LEAF);
    let second = run();
    let expected_b = (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0);
    observations.compare("O02/B", &second, &expected_b, second == expected_b);
    scratch.write("dep.fsl", cases::LEGACY_LEAF);
    let warning_a = run();
    let warning_b = run();
    let expected = (
        json!({"fsl":"1.0","result":"ok","warnings":[cases::metadata_finding(cases::LEGACY_LEAF, child.to_str().unwrap(), "current")]}),
        0,
    );
    for (label, result) in [("O03/A", warning_a), ("O03/B", warning_b)] {
        observations.compare(label, &result, &expected, result == expected);
    }
    observations.finish();
}

fn dependency_edition_o04_nested_owner_and_o05_fresh_child_thread() {
    let (scratch, path, source) = fixture();
    scratch.write("dep.fsl", cases::LEGACY_LEAF);
    let nested = run_admitted_root(&path, Ok(source.clone()), None, false, |source| {
        run_admitted_root(&path, Ok(source.into()), None, false, |source| {
            consume(&path, source)
        })
    });
    let child = scratch.0.join("dep.fsl").canonicalize().unwrap();
    let expected = (
        json!({"fsl":"1.0","result":"ok","warnings":[cases::metadata_finding(cases::LEGACY_LEAF,child.to_str().unwrap(),"current")]}),
        0,
    );
    let mut observations = cases::Observations::default();
    observations.compare("O04", &nested, &expected, nested == expected);
    scratch.write("dep.fsl", &format!("fsl 5\n{}", cases::LEGACY_LEAF));
    let result = run_admitted_root(&path, Ok(source.clone()), None, false, |source| {
        std::thread::scope(|scope| scope.spawn(|| consume(&path, source)).join().unwrap())
    });
    observations.compare(
        "O05",
        &result,
        "error/2 and no model",
        result.1 == 2 && result.0["result"] == "error",
    );
    observations.finish();
}

fn dependency_edition_c03_direct_slots_keep_individual_warning_cardinality() {
    let scratch = common::Scratch::new();
    let path = scratch.write("direct.fsl", cases::LEGACY_LEAF);
    let result = run_admitted_roots(
        vec![
            (&path, Ok(cases::LEGACY_LEAF.into())),
            (&path, Ok(cases::LEGACY_LEAF.into())),
        ],
        None,
        false,
        |_, editions| {
            finish_admitted(
                (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0),
                editions,
                false,
            )
        },
    );
    let finding = cases::metadata_finding(cases::LEGACY_LEAF, path.to_str().unwrap(), "current");
    let expected = (
        json!({"fsl":"1.0","result":"ok","warnings":[finding.clone(),finding]}),
        0,
    );
    let mut observations = cases::Observations::default();
    observations.compare("C03", &result, &expected, result == expected);
    observations.finish();
}

fn dependency_edition_t06_isolates_the_actual_unowned_refine_step() {
    let (scratch, path, source) = fixture();
    scratch.write("dep.fsl", cases::LEAF);
    scratch.write("abstract.fsl", &source);
    scratch.write("map.fsl", "refinement M { impl C abs C maps auto }\n");
    let sections=parse_project_manifest("[requirements]\nfile = \"abstract.fsl\"\n[design]\nfile = \"root.fsl\"\nrefine_against = \"requirements\"\nmapping = \"map.fsl\"\ndepth = 2\n").unwrap();
    let first = run_chain_step_isolated("refine", "design", &sections, &scratch.0);
    let second = run_chain_step_isolated("refine", "design", &sections, &scratch.0);
    assert!(!first.1 && !second.1);
    assert_eq!(first.0["detail"]["result"], "refines");
    assert_eq!(cases::stable(first.0), cases::stable(second.0));
    scratch.write("dep.fsl", &format!("fsl 5\n{}", cases::LEGACY_LEAF));
    let result = run_chain_step_isolated("refine", "design", &sections, path.parent().unwrap());
    let mut observations = cases::Observations::default();
    observations.compare(
        "T06/direct-step",
        &result,
        "unowned actual refine step fails closed, no solver result",
        result.1
            && result.0["exit_code"] == 2
            && result.0["detail"]["result"] == "error"
            && result.0["detail"].get("checked_to_depth").is_none(),
    );
    observations.finish();
}

fn dependency_edition_mapping_role_calibration() {
    let scratch = common::Scratch::new();
    let implementation = scratch.write("after.fsl", cases::LEAF);
    let abstraction = scratch.write("before.fsl", cases::LEAF);
    for id in ["E04/P", "E04/N"] {
        let row = cases::flat_rows().into_iter().find(|r| r.id == id).unwrap();
        let fixture = cases::fixture(&row);
        let source = &fixture.files[&fixture.child];
        let path = scratch.write("mapping.fsl", source);
        let run = || {
            run_admitted_root(&path, Ok(source.clone()), None, false, |_| {
                run_refine(&implementation, &abstraction, &path, 2)
            })
        };
        let actual = run();
        if row.variant == cases::Variant::N {
            let expected = (
                cases::refusal(source, path.to_str().unwrap(), envelope()),
                2,
            );
            println!("CALIBRATION {id}/mapping-role produced={actual:?} expected={expected:?}");
            assert_eq!(actual, expected);
        } else {
            assert_eq!(actual.1, 0);
            assert_eq!(actual.0["result"], "refines");
            let repeated = run();
            assert_eq!(cases::stable(actual.0), cases::stable(repeated.0));
        }
    }
}

#[test]
fn dependency_edition_private_adapter_executes_all_assigned_rows() {
    cases::exercise(
        "private",
        &[
            dependency_edition_o01_fresh_thread_has_no_owner_and_fails_closed,
            dependency_edition_o02_o03_sequential_owners_do_not_share_records,
            dependency_edition_o04_nested_owner_and_o05_fresh_child_thread,
            dependency_edition_p01_p04_lenient_helpers_cannot_swallow_refusal,
            dependency_edition_c03_direct_slots_keep_individual_warning_cardinality,
            dependency_edition_t06_isolates_the_actual_unowned_refine_step,
            dependency_edition_mapping_role_calibration,
        ],
    );
}
