// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The one kernel comparison for lowering contracts (fsl 5 plan §2, K).
//!
//! Two documents lower to the same model when their Public Kernel v2 JSON is
//! equal after removing what records *where* something came from rather than
//! *what* it is: source spans, origins and provenance, `requirement`
//! annotations, `meta`, and the spec's source location. Everything else, names
//! included, must match exactly.
//!
//! Consumers include this file with
//! `#[path = "support/kernel_compare.rs"] mod kernel_compare;`.
//! `kernel_compare_helper.rs` owns its tests, so each consumer does not carry
//! a copy of them. Items are `#[allow(dead_code)]` for the same reason as in
//! `support/mod.rs`: each integration test is its own crate.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Keys removed at every depth before comparing.
#[allow(dead_code)]
pub const EXCLUDED_KEYS: &[&str] = &[
    "span",
    "origin",
    "provenance",
    "requirement",
    "annotations",
    "meta",
];

/// `fslc kernel --kernel-version 2 <path>` in the published language, run
/// from `path`'s directory so relative `from "..."` dependencies resolve.
///
/// # Panics
///
/// When the command fails or prints something other than one JSON document.
#[allow(dead_code)]
pub fn kernel_v2(path: &Path) -> Value {
    run_kernel_v2(path, None)
}

/// [`kernel_v2`] with the next edition enabled behind the unstable gate.
///
/// # Panics
///
/// As [`kernel_v2`].
#[allow(dead_code)]
pub fn kernel_v2_fsl5(path: &Path) -> Value {
    run_kernel_v2(path, Some("fsl5"))
}

fn run_kernel_v2(path: &Path, unstable: Option<&str>) -> Value {
    let directory = path.parent().expect("kernel source directory");
    let file = path.file_name().expect("kernel source file name");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(["kernel", "--kernel-version", "2"])
        .arg(file)
        .current_dir(directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if let Some(features) = unstable {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, features);
    }
    let output = command.output().expect("run fslc kernel");
    assert!(
        output.status.success(),
        "fslc kernel {} failed: {}{}",
        path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("kernel JSON")
}

/// `kernel` without the [`EXCLUDED_KEYS`] and without `spec.source`.
#[allow(dead_code)]
#[must_use]
pub fn comparable(kernel: &Value) -> Value {
    let mut value = strip(kernel);
    if let Some(spec) = value.get_mut("spec").and_then(Value::as_object_mut) {
        spec.remove("source");
    }
    value
}

fn strip(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !EXCLUDED_KEYS.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), strip(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip).collect()),
        scalar => scalar.clone(),
    }
}

/// The first difference between the comparable forms of two kernels, as a
/// JSON path with both values, or `None` when they are equal.
#[allow(dead_code)]
#[must_use]
pub fn difference(left: &Value, right: &Value) -> Option<String> {
    first_difference(&comparable(left), &comparable(right), "$")
}

fn first_difference(left: &Value, right: &Value, path: &str) -> Option<String> {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            for (key, left_value) in left {
                let child = format!("{path}.{key}");
                match right.get(key) {
                    Some(right_value) => {
                        if let Some(found) = first_difference(left_value, right_value, &child) {
                            return Some(found);
                        }
                    }
                    None => return Some(format!("{child}: only on the left")),
                }
            }
            right
                .keys()
                .find(|key| !left.contains_key(*key))
                .map(|key| format!("{path}.{key}: only on the right"))
        }
        (Value::Array(left), Value::Array(right)) => {
            for (index, (left_item, right_item)) in left.iter().zip(right).enumerate() {
                if let Some(found) =
                    first_difference(left_item, right_item, &format!("{path}[{index}]"))
                {
                    return Some(found);
                }
            }
            (left.len() != right.len())
                .then(|| format!("{path}: length {} vs {}", left.len(), right.len()))
        }
        _ => (left != right).then(|| format!("{path}: {left} vs {right}")),
    }
}

/// Assert that two kernels lower to the same model.
///
/// # Panics
///
/// With the first difference when they do not.
#[allow(dead_code)]
pub fn assert_same_kernel(left: &Value, right: &Value, context: &str) {
    if let Some(found) = difference(left, right) {
        panic!("{context}: kernels differ at {found}");
    }
}
