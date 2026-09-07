// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native contract coverage for inline `implements` bounds propagation (#1003).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/inline_implements_bounds";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn fixture(name: &str) -> String {
    format!("{FIXTURE_DIR}/{name}")
}

fn run_verify(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc verify");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn is_timing_leaf_key(key: &str) -> bool {
    key == "elapsed_s" || key == "check_elapsed_s"
}

fn flatten_leaves(value: &Value, prefix: &str, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_leaves(child, &path, out);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                flatten_leaves(child, &format!("{prefix}.{index}"), out);
            }
        }
        _ => {
            out.insert(prefix.to_string(), value.clone());
        }
    }
}

fn timing_leaf_paths(value: &Value) -> Vec<String> {
    let mut leaves = BTreeMap::new();
    flatten_leaves(value, "", &mut leaves);
    leaves
        .keys()
        .filter(|path| {
            let leaf = path.rsplit('.').next().unwrap_or(path.as_str());
            is_timing_leaf_key(leaf)
        })
        .cloned()
        .collect()
}

fn comparable_leaf_map(value: &Value) -> BTreeMap<String, Value> {
    let mut leaves = BTreeMap::new();
    flatten_leaves(value, "", &mut leaves);
    leaves
        .into_iter()
        .filter(|(path, _)| {
            let leaf = path.rsplit('.').next().unwrap_or(path.as_str());
            !is_timing_leaf_key(leaf) && !path.starts_with("bounds_overrides")
        })
        .collect()
}

/// Compare two verify envelopes in full, excluding only wall-clock timing leaves
/// (`elapsed_s`, `check_elapsed_s`) and the scoped-only `bounds_overrides` echo.
/// Timing fields are real-time measurements and differ across repeated runs with
/// the same input and binary (observed: 61 leaf keys total, 7 timing leaves differed).
/// `bounds_overrides` is asserted separately on the scoped side only.
fn assert_envelopes_agree_modulo_timing(left: &Value, right: &Value) {
    let left_timing = timing_leaf_paths(left);
    let right_timing = timing_leaf_paths(right);
    assert!(
        !left_timing.is_empty() && !right_timing.is_empty(),
        "dead timing exclusion: both envelopes lacked elapsed_s/check_elapsed_s leaves; \
         left={left_timing:?} right={right_timing:?}"
    );

    let left_map = comparable_leaf_map(left);
    let right_map = comparable_leaf_map(right);
    assert_eq!(
        left_map,
        right_map,
        "envelopes differ beyond timing leaves; left-only={:?} right-only={:?}",
        left_map
            .keys()
            .filter(|key| !right_map.contains_key(*key))
            .collect::<Vec<_>>(),
        right_map
            .keys()
            .filter(|key| !left_map.contains_key(*key))
            .collect::<Vec<_>>()
    );
}

/// detector (mutation D: restoring `has_scope` suppression drops `implements`)
#[test]
fn verify_with_instances_keeps_inline_implements_in_envelope() {
    let (output, status) = run_verify(&[
        "verify",
        &fixture("impl_broken.fsl"),
        "--depth",
        "2",
        "--strict-tags",
        "--no-cache",
        "--instances",
        "Item=1",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified");
    assert_eq!(output["implements"]["result"], "refinement_failed");
    assert_eq!(
        output["implements"]["violation"]["kind"],
        "stutter_changed_abs"
    );
}

/// detector (business abstract × shared entity instances)
#[test]
fn instances_override_propagates_to_business_abstract() {
    let (output, status) = run_verify(&[
        "verify",
        &fixture("impl_valid.fsl"),
        "--depth",
        "6",
        "--no-cache",
        "--instances",
        "Claim=1",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["implements"]["result"], "refines");
}

/// detector (business abstract × impl-only number values)
#[test]
fn impl_only_number_values_do_not_reach_business_abstract() {
    let (output, status) = run_verify(&[
        "verify",
        &fixture("impl_with_amount.fsl"),
        "--depth",
        "6",
        "--no-cache",
        "--instances",
        "Claim=1",
        "--values",
        "Amount=0..1",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["implements"]["result"], "refines");
    assert_eq!(
        output["bounds_overrides"]["values"]["Amount"],
        Value::Array(vec![Value::from(0), Value::from(1),])
    );
}

/// detector (spec abstract × shared number values)
#[test]
fn values_override_propagates_to_spec_abstract() {
    let (output, status) = run_verify(&[
        "verify",
        &fixture("impl_spec_number.fsl"),
        "--depth",
        "4",
        "--no-cache",
        "--values",
        "Limit=0..1",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["implements"]["result"], "refines");
}

/// detector (spec abstract × impl-only entity instances)
#[test]
fn impl_only_entity_instances_do_not_error_on_spec_abstract() {
    let (output, status) = run_verify(&[
        "verify",
        &fixture("impl_spec_extra.fsl"),
        "--depth",
        "4",
        "--no-cache",
        "--instances",
        "Item=1",
        "--instances",
        "Extra=1",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["implements"]["result"], "refines");
}

/// preservation control (mutation E: classifying `refines` as failure)
#[test]
fn valid_inline_refinement_preserves_success_with_and_without_bounds() {
    let base_args = [
        "verify",
        &fixture("impl_valid.fsl"),
        "--depth",
        "6",
        "--no-cache",
    ];
    let (without_bounds, status_without) = run_verify(&base_args);
    assert_eq!(status_without, 0, "{without_bounds:#}");
    assert_eq!(without_bounds["implements"]["result"], "refines");
    assert!(without_bounds.get("bounds_overrides").is_none());

    let (with_bounds, status_with) = run_verify(&[
        "verify",
        &fixture("impl_valid.fsl"),
        "--depth",
        "6",
        "--no-cache",
        "--instances",
        "Claim=3",
    ]);
    assert_eq!(status_with, 0, "{with_bounds:#}");
    assert_eq!(with_bounds["implements"]["result"], "refines");
    assert_eq!(with_bounds["bounds_overrides"]["instances"]["Claim"], 3);

    assert_eq!(status_without, status_with);
    assert_envelopes_agree_modulo_timing(&without_bounds, &with_bounds);
}

/// preservation control (mutation F: removing timing exclusions must red this)
#[test]
fn timing_exclusion_survives_on_repeated_runs() {
    let args = [
        "verify",
        &fixture("impl_valid.fsl"),
        "--depth",
        "6",
        "--no-cache",
    ];
    let (first, _) = run_verify(&args);
    let (second, _) = run_verify(&args);
    assert_envelopes_agree_modulo_timing(&first, &second);
}
