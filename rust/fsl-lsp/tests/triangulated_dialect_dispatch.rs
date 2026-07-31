// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use fsl_lsp::DocumentIndex;
use serde_json::Value;

#[test]
fn lsp_consumes_the_same_raw_dispatch_manifest() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let manifest: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(
            root.join("rust/fslc/tests/fixtures/triangulated_dialect_dispatch.json"),
        )
        .expect("read P3 manifest"),
    )
    .expect("parse P3 manifest");

    for case in manifest {
        let id = case["id"].as_str().expect("case id");
        let source = case["source"].as_str().expect("case source");
        let accepted = case["accepted"].as_bool().expect("accepted flag");
        let observed = DocumentIndex::build(source, Some(id));
        if accepted {
            observed.unwrap_or_else(|error| panic!("{id}: LSP rejected accepted source: {error}"));
        } else {
            let error = observed.expect_err("LSP must reject unknown dialect");
            assert!(
                error.0.contains("unsupported top-level declaration")
                    && error.0.contains(&format!(
                        "at {}:{}",
                        case["line"].as_u64().expect("line"),
                        case["column"].as_u64().expect("column")
                    )),
                "{id}: {error}"
            );
        }
    }
}
