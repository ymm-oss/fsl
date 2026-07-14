// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use fslc_rust::origin_coverage::{ORIGIN_COVERAGE_SCHEMA_ID, origin_coverage_matrix_v2};
use serde_json::Value;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

#[test]
fn v2_origin_coverage_meets_every_required_row_and_matches_golden() {
    let matrix = origin_coverage_matrix_v2().expect("origin coverage matrix");
    let golden =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conformance_coverage.v2.json");
    if std::env::var_os("FSLC_ORIGIN_COVERAGE_UPDATE").is_some() {
        std::fs::write(
            &golden,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&matrix).expect("serialize matrix")
            ),
        )
        .expect("update v2 coverage golden");
    }
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(golden).expect("read v2 coverage golden"))
            .expect("v2 coverage golden JSON");
    assert_eq!(matrix, expected);
    assert!(
        matrix["features"]
            .as_array()
            .is_some_and(|rows| { rows.iter().all(|row| row["level"] == "exercised") })
    );
}

#[test]
fn published_v2_coverage_schema_id_matches_the_rust_api_constant() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            workspace_root().join("schemas/fslc/kernel/conformance-coverage.v2.schema.json"),
        )
        .expect("read v2 coverage schema"),
    )
    .expect("v2 coverage schema JSON");
    assert_eq!(schema["$id"], ORIGIN_COVERAGE_SCHEMA_ID);
    assert_eq!(
        schema["properties"]["kernel_schema_version"]["const"],
        "2.0.0"
    );
}
