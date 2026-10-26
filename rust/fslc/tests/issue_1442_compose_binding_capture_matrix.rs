// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Alpha-equivalence across every finite compose/sync binding-source product.
//! Fresh controls must violate Capture at step1 in both engines. Only statement
//! forall/Collection may instead report the exact known BMC unsupported error;
//! check must still succeed and explicit must still violate Capture at step1.

#[path = "support/compose_scratch.rs"]
mod compose_scratch;
#[path = "support/compose_binding_matrix.rs"]
mod matrix;

use std::collections::BTreeMap;
use std::time::Instant;

use serde_json::{Value, json};

fn observe(bundle: &matrix::SourceBundle) -> Vec<(Value, i32)> {
    let files: Vec<_> = bundle
        .files
        .iter()
        .map(|(name, source)| (name.as_str(), source.as_str()))
        .collect();
    let dir = compose_scratch::scratch("issue-1442-capture-matrix", &files);
    vec![
        compose_scratch::check(&dir, "s.fsl"),
        compose_scratch::verify(&dir, "s.fsl"),
        compose_scratch::run(
            &dir,
            &[
                "verify",
                "s.fsl",
                "--engine",
                "explicit",
                "--depth",
                "4",
                "--no-cache",
            ],
        ),
    ]
}

const COLLECTION_BMC_ERROR: &str =
    "collection binders are not implemented in the current verifier slice";

fn collection_bmc_error((value, code): &(Value, i32)) -> bool {
    *code == 2 && value["result"] == "error" && value["message"] == COLLECTION_BMC_ERROR
}

fn violated((value, code): &(Value, i32)) -> bool {
    *code == 1
        && value["result"] == "violated"
        && value["invariant"] == "Capture"
        && value["violation_kind"] == "invariant"
        && value["violated_at_step"] == 1
}

fn calibrated(observations: &[(Value, i32)], collection_statement: bool) -> bool {
    observations[0].1 == 0
        && observations[0].0["result"] == "ok"
        && violated(&observations[2])
        && (violated(&observations[1])
            || (collection_statement && collection_bmc_error(&observations[1])))
}

fn compare(
    same: &[(Value, i32)],
    fresh: &[(Value, i32)],
    same_bundle: &matrix::SourceBundle,
    fresh_bundle: &matrix::SourceBundle,
) -> Vec<String> {
    let mut errors = Vec::new();
    for (index, (left, right)) in same.iter().zip(fresh).enumerate() {
        let normalize = |observation: &(Value, i32), bundle: &matrix::SourceBundle| {
            let mut value = observation.0.clone();
            let has_cost = index != 0 && !(index == 1 && collection_bmc_error(observation));
            let parameter = if has_cost {
                bundle.alpha_parameter.as_ref()
            } else {
                None
            };
            let status = matrix::normalize(&mut value, parameter, has_cost);
            (value, status)
        };
        let (left_json, ls) = normalize(left, same_bundle);
        let (right_json, rs) = normalize(right, fresh_bundle);
        if left.1 != right.1 || left_json != right_json || ls.is_err() || rs.is_err() {
            errors.push(format!("full outcome differs at mode{index}; normalization={ls:?}/{rs:?}; left={left_json}; right={right_json}"));
        }
    }
    errors
}

#[test]
fn all_compose_and_sync_binding_sources_preserve_alpha_equivalence() {
    let cells = matrix::cells();
    let report_dir = compose_scratch::scratch("issue-1442-matrix-report", &[]);
    let mut table = "family\torigin\tposition\tcollision_kind\tshape_or_order\n".to_owned();
    for cell in &cells {
        table.push_str(&cell.columns.join("\t"));
        table.push('\n');
    }
    std::fs::write(report_dir.join("cells.tsv"), table).expect("write observed cell inventory");
    let started = Instant::now();
    let mut reports = Vec::new();
    // Disjoint counts: defect-red / full green / invalid / BMC unsupported.
    let mut families: BTreeMap<String, [usize; 4]> = BTreeMap::new();
    let mut bad_controls = 0;
    let mut unsupported_bmc = Vec::new();
    for cell in cells {
        let same_bundle = cell.render(false);
        let fresh_bundle = cell.render(true);
        let same = observe(&same_bundle);
        let fresh = observe(&fresh_bundle);
        let collection_statement = cell.collection_statement();
        let control_ok = calibrated(&fresh, collection_statement);
        let same_ok = calibrated(&same, collection_statement);
        bad_controls += usize::from(!control_ok);
        let mut errors = Vec::new();
        if !control_ok {
            errors.push("fresh control violates the check/BMC/explicit oracle".to_owned());
        }
        if !same_ok {
            errors.push("collision form violates the check/BMC/explicit oracle".to_owned());
        }
        if control_ok && same_ok {
            errors.extend(compare(&same, &fresh, &same_bundle, &fresh_bundle));
        }
        let red = control_ok && !errors.is_empty();
        let bmc_unsupported = control_ok && same_ok && !red && collection_bmc_error(&fresh[1]);
        if bmc_unsupported {
            eprintln!("MATRIX_BMC_UNSUPPORTED {}", cell.id());
            unsupported_bmc.push(cell.id());
        }
        let classification = if control_ok {
            if bmc_unsupported {
                3
            } else {
                usize::from(!red)
            }
        } else {
            2
        };
        families.entry(cell.family().into()).or_default()[classification] += 1;
        if !control_ok || red {
            eprintln!(
                "MATRIX_{} {} control_ok={control_ok} same={} fresh={}",
                if control_ok { "RED" } else { "INVALID" },
                cell.id(),
                summary(&same),
                summary(&fresh)
            );
        }
        reports.push(json!({"id": cell.id(), "family": cell.family(), "red": red, "invalid": !control_ok, "control_ok": control_ok, "bmc_unsupported": bmc_unsupported, "same": same, "fresh": fresh, "errors": errors}));
    }
    let elapsed = started.elapsed().as_secs_f64();
    let red: usize = families.values().map(|counts| counts[0]).sum();
    let green: usize = families.values().map(|counts| counts[1]).sum();
    let report = json!({"cells": reports, "families": families, "red": red, "green": green, "invalid": bad_controls, "bad_controls": bad_controls, "unsupported_bmc": unsupported_bmc, "elapsed_s": elapsed});
    std::fs::write(
        report_dir.join("report.json"),
        serde_json::to_vec(&report).expect("serialize complete observations"),
    )
    .expect("write full matrix report");
    eprintln!(
        "MATRIX_RESULT red={red} green={green} invalid={bad_controls} bmc_unsupported={} elapsed_s={elapsed:.3} report={}",
        unsupported_bmc.len(),
        report_dir.display()
    );
    assert_eq!(
        bad_controls, 0,
        "invalid fresh controls; see complete report"
    );
    assert_eq!(
        red, 0,
        "binding-capture matrix has red cells; see complete report"
    );
}

#[test]
fn unsupported_bmc_requires_the_exact_factor_message_exit_and_explicit_control() {
    let control = vec![
        (json!({"result":"ok"}), 0),
        (json!({"result":"error","message":COLLECTION_BMC_ERROR}), 2),
        (
            json!({"result":"violated","invariant":"Capture","violation_kind":"invariant","violated_at_step":1}),
            1,
        ),
    ];
    for cell in matrix::cells() {
        assert_eq!(
            calibrated(&control, cell.collection_statement()),
            cell.collection_statement(),
            "{}",
            cell.id()
        );
    }
    let mut changed = control.clone();
    changed[1].0["message"] = json!(format!("{COLLECTION_BMC_ERROR}."));
    assert!(
        !calibrated(&changed, true),
        "no other error message admitted"
    );
    changed = control.clone();
    changed[1].1 = 1;
    assert!(!calibrated(&changed, true), "exit2 is mandatory");
    changed = control;
    changed[2] = (json!({"result":"proved"}), 0);
    assert!(!calibrated(&changed, true), "explicit may not be waived");
}

fn summary(observations: &[(Value, i32)]) -> Value {
    Value::Array(observations.iter().map(|(value, code)| json!({"exit":code,"result":value["result"],"invariant":value.get("invariant"),"step":value.get("violated_at_step")})).collect())
}
