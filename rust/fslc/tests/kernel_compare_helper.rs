// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Tests of the shared kernel comparison (`support/kernel_compare.rs`). They
//! live here once instead of in every consumer of the helper.

use std::path::{Path, PathBuf};

use serde_json::json;

#[path = "support/kernel_compare.rs"]
mod kernel_compare;

fn fixture(example: &str, file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5")
        .join(example)
        .join(file)
}

#[test]
fn ignores_location_records_and_nothing_else() {
    let left = json!({
        "spec": {"name": "S", "source": {"file": "a.fsl", "dialect": "requirements"}},
        "provenance": {"origins": [1]},
        "actions": [{
            "name": "go",
            "origin": {"line": 1},
            "requirement": "REQ-1",
            "meta": {"x": 1},
            "annotations": [],
            "guards": [{"expression": {"operator": "<=", "span": {"line": 3}}}]
        }]
    });
    let moved = json!({
        "spec": {"name": "S", "source": {"file": "b.fsl", "dialect": "requirements"}},
        "provenance": {"origins": [2]},
        "actions": [{
            "name": "go",
            "origin": {"line": 9},
            "requirement": "REQ-9",
            "guards": [{"expression": {"operator": "<=", "span": {"line": 7}}}]
        }]
    });
    assert_eq!(kernel_compare::difference(&left, &moved), None);

    let weakened = json!({
        "spec": {"name": "S"},
        "actions": [{"name": "go", "guards": [{"expression": {"operator": "<"}}]}]
    });
    assert_eq!(
        kernel_compare::difference(&left, &weakened).as_deref(),
        Some(r#"$.actions[0].guards[0].expression.operator: "<=" vs "<""#)
    );

    let renamed = json!({"spec": {"name": "T"}, "actions": []});
    assert_eq!(
        kernel_compare::difference(&json!({"spec": {"name": "S"}, "actions": []}), &renamed)
            .as_deref(),
        Some(r#"$.spec.name: "S" vs "T""#)
    );
    assert_eq!(
        kernel_compare::difference(&json!({"a": [1, 2]}), &json!({"a": [1]})).as_deref(),
        Some("$.a: length 2 vs 1")
    );
    assert_eq!(
        kernel_compare::difference(&json!({"a": 1}), &json!({"a": 1, "b": 2})).as_deref(),
        Some("$.b: only on the right")
    );
}

/// On real kernels: moving every line (spans, origins, provenance and the
/// file name all change) compares equal, while the design draft's
/// `original.fsl` and `lowered.fsl` for e2e differ only in `submit`'s
/// parameter name (`a` vs `amount`, recorded in the draft's verification
/// log), which the comparison must report.
#[test]
fn real_kernels_compare_by_meaning_not_location() {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kernel_compare_helper");
    std::fs::create_dir_all(&scratch).expect("scratch directory");
    for file in ["1_business.fsl", "original.fsl"] {
        std::fs::copy(fixture("e2e", file), scratch.join(file)).expect("copy fixture");
    }
    let original_source =
        std::fs::read_to_string(fixture("e2e", "original.fsl")).expect("read original");
    let moved = scratch.join("moved.fsl");
    std::fs::write(&moved, format!("// moved\n\n{original_source}")).expect("write moved copy");

    let original = kernel_compare::kernel_v2(&fixture("e2e", "original.fsl"));
    let moved = kernel_compare::kernel_v2(&moved);
    assert_ne!(original, moved, "the copy must really move every location");
    kernel_compare::assert_same_kernel(&original, &moved, "moved copy");

    let lowered = kernel_compare::kernel_v2(&fixture("e2e", "lowered.fsl"));
    let found = kernel_compare::difference(&original, &lowered).expect("submit's parameter");
    assert!(
        found.starts_with("$.actions[") && found.contains("\"a\" vs \"amount\""),
        "{found}"
    );
}
