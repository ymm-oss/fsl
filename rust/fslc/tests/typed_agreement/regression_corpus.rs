// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use super::engines;

#[test]
fn minimized_regression_corpus_is_valid_and_deduplicated_by_failure_signature() {
    let manifest: Value = serde_json::from_str(include_str!("regressions/manifest.v1.json"))
        .expect("valid regression corpus manifest JSON");
    assert_eq!(manifest["schema"], "fslc.fsl-logic-regression-corpus.v1");
    assert_eq!(manifest["schema_version"], 1);
    let entries = manifest["entries"].as_array().expect("regression entries");
    let mut signatures = BTreeSet::new();
    for entry in entries {
        let signature = entry["failure_signature"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .expect("regression failure signature");
        assert!(
            signatures.insert(signature),
            "duplicate semantic failure signature '{signature}'"
        );
        for key in ["case_id", "source", "raw_observation", "replay_command"] {
            assert!(
                entry[key]
                    .as_str()
                    .is_some_and(|value| !value.trim().is_empty()),
                "regression '{signature}' lacks {key}"
            );
        }
        let depth = entry["depth"].as_u64().expect("regression depth");
        let source_path = entry["source"].as_str().expect("regression source path");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repository root");
        let source = std::fs::read_to_string(root.join(source_path))
            .unwrap_or_else(|error| panic!("read regression '{source_path}': {error}"));
        let case_id = entry["case_id"].as_str().expect("regression case ID");
        let model = engines::build(case_id, &source);
        engines::compare_agreement(
            case_id,
            &model,
            usize::try_from(depth).expect("depth fits usize"),
        )
        .unwrap_or_else(|failure| panic!("regression '{signature}' returned: {failure}"));
    }
}
