// SPDX-License-Identifier: Apache-2.0

//! Issue #1946: each caller hands the verify cache the root it captured, not a
//! second read of the file.
//!
//! The key itself is covered in `verification.rs` (`issue_1946_*`). These
//! tests guard the three call sites that choose the bytes: `verify`'s root
//! (the literate document), each `sweep` cell, and a `chain` spec layer. Each
//! captures S1, finds S2 on disk when it verifies, and must store S1's verdict
//! under S1's key: a later run that captures S2 misses and reports S2's
//! verdict. A site that re-reads the file stores S1's verdict under S2's key,
//! and that later run is served the wrong verdict.

use super::*;
use crate::verification::tests::{PrivateCache, RACE_PROVED, RACE_VIOLATED};

fn explicit() -> CliVerifyOptions {
    CliVerifyOptions {
        engine: "explicit".to_owned(),
        ..CliVerifyOptions::default()
    }
}

#[test]
fn verify_keys_a_literate_root_by_the_document_it_captured() {
    let cache = PrivateCache::new("call-site-literate");
    let document = cache.dir.join("spec.md");
    let markdown = |source: &str| format!("# Race\n\n```fsl\n{source}```\n");
    let capture = || {
        literate_access_with_document("verify", &document)
            .expect("capture")
            .expect("a literate root")
    };
    let verify = |captured: (fslc_rust::literate_access::LiterateState, String)| {
        let path = captured.0.path.clone();
        run_verify_command_root(&path, &document, Some(captured), &explicit())
    };

    std::fs::write(&document, markdown(RACE_PROVED)).expect("write D1");
    let first_capture = capture();
    std::fs::write(&document, markdown(RACE_VIOLATED)).expect("the document is now D2");
    let (first, status) = verify(first_capture);
    assert_eq!(
        (first["result"].clone(), status),
        (json!("proved"), 0),
        "{first:#}"
    );
    assert_eq!(cache.entries(), 1, "calibration: D1's verdict is stored");

    let (second, status) = verify(capture());
    assert!(second.get("cache").is_none(), "D2 must miss: {second:#}");
    assert_eq!(
        (second["result"].clone(), status),
        (json!("violated"), 1),
        "{second:#}"
    );
    let (third, _) = verify(capture());
    assert_eq!(
        third["cache"]["hit"], true,
        "calibration: D2 hits: {third:#}"
    );
}

fn sweep_from_captured(path: &Path, source: &str) -> (Value, i32) {
    let none = std::collections::BTreeMap::new();
    spec_loading::run_admitted_roots(
        vec![(path, Ok(source.to_owned()))],
        None,
        false,
        |sources, editions| {
            run_sweep_grid_from_source(
                path,
                &sources[0],
                &editions[0],
                8,
                8,
                "warn",
                "bmc",
                1,
                "warn",
                None,
                false,
                None,
                &none,
                &none,
            )
        },
    )
}

fn sweep_cell(output: &Value) -> &Value {
    let results = output["sweep"]["results"]
        .as_array()
        .expect("sweep results");
    assert_eq!(results.len(), 1, "{output:#}");
    &results[0]["verification"]
}

#[test]
fn sweep_cells_key_by_the_root_the_sweep_captured() {
    let cache = PrivateCache::new("call-site-sweep");
    let path = cache.dir.join("spec.fsl");
    std::fs::write(&path, RACE_VIOLATED).expect("the file holds S2");
    let (first, _) = sweep_from_captured(&path, RACE_PROVED);
    assert_eq!(sweep_cell(&first)["result"], "verified", "{first:#}");
    assert_eq!(cache.entries(), 1, "calibration: S1's cell is stored");

    let (second, _) = sweep_from_captured(&path, RACE_VIOLATED);
    let cell = sweep_cell(&second);
    assert!(
        cell.get("cache").is_none(),
        "S2's cell must miss: {second:#}"
    );
    assert_eq!(cell["result"], "violated", "{second:#}");
    let (third, _) = sweep_from_captured(&path, RACE_VIOLATED);
    assert_eq!(
        sweep_cell(&third)["cache"]["hit"],
        true,
        "calibration: {third:#}"
    );
}

#[test]
fn chain_spec_layers_key_by_the_root_the_chain_captured() {
    let cache = PrivateCache::new("call-site-chain");
    let base = cache.dir.clone();
    let spec = base.join("spec.fsl");
    let sections = parse_project_manifest("[requirements]\nfile = \"spec.fsl\"\ndepth = 8\n")
        .expect("parse manifest");
    let run_layer =
        |roots: &[ProjectRoot]| run_chain_step("spec", "requirements", &sections, &base, roots).0;

    std::fs::write(&spec, RACE_PROVED).expect("write S1");
    let captured = prepare_project_roots(&sections, &base, read_spec_source).expect("capture S1");
    std::fs::write(&spec, RACE_VIOLATED).expect("the file is now S2");
    let first = run_layer(&captured);
    assert_eq!(first["detail"]["result"], "verified", "{first:#}");
    assert_eq!(cache.entries(), 1, "calibration: S1's layer is stored");

    let current = prepare_project_roots(&sections, &base, read_spec_source).expect("capture S2");
    let second = run_layer(&current);
    assert!(
        second["detail"].get("cache").is_none(),
        "S2 must miss: {second:#}"
    );
    assert_eq!(second["detail"]["result"], "violated", "{second:#}");
    let third = run_layer(&current);
    assert_eq!(
        third["detail"]["cache"]["hit"], true,
        "calibration: {third:#}"
    );
}
