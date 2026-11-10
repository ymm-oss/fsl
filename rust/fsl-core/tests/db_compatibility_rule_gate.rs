// SPDX-License-Identifier: Apache-2.0

//! #1473: CLI caller guards cannot substitute for a total DB lowering API.

use fsl_core::{CoreError, CoreErrorKind, FsResolver, KernelSpec};

const INVALID_DB: &str =
    include_str!("../../fslc/tests/fixtures/error_envelope_db_invalid_rule.fsl");
const SOURCE_FILE: &str = "fixtures/unknown-db-rule.fsl";

// Independent specification vocabulary, not the production rule registry.
const RULES: &[&str] = &[
    "all_active_reads_exist",
    "all_active_writes_exist",
    "api_calls_accepted",
    "api_responses_expected",
    "artifact_capabilities_provided",
    "data_preserved",
    "destructive_operations_annotated",
    "not_null_after_backfill",
    "offline_payloads_accepted",
    "preservation_transforms_annotated",
    "removed_only_after_unused",
    "rollback_equivalent",
];

fn system(source: &str) -> fsl_syntax::DbSystem {
    let fsl_syntax::SurfaceDocument::Db(system) =
        fsl_syntax::parse_surface_document(source).expect("syntactically valid DB fixture")
    else {
        panic!("fixture must select the DB frontend");
    };
    system
}

fn assert_rule_error(api: &str, result: Result<KernelSpec, CoreError>, source_file: Option<&str>) {
    let expected = format!(
        "unknown db compatibility rule 'unregistered_compatibility_rule'; supported rules: {}",
        RULES.join(", ")
    );
    let error = result.err().unwrap_or_else(|| {
        panic!("{api}: expected Err(Semantics) at 12:5 with message {expected:?}; produced Ok")
    });
    assert_eq!(error.kind, CoreErrorKind::Semantics, "{api}: {error}");
    assert_eq!(
        error.message, expected,
        "{api}: full tools-compatible message"
    );
    assert_eq!((error.line, error.column), (12, 5), "{api}");
    assert!(error.is_located(), "{api}: {error}");
    let origin = error.origin.as_ref().expect("authored DB lowering origin");
    assert_eq!(origin.dialect, "dbsystem", "{api}");
    assert!(!origin.generated, "{api}: {origin:?}");
    let site = origin.primary.as_ref().expect("authored rule site");
    assert_eq!(site.dialect, "dbsystem", "{api}");
    assert_eq!(site.source_file.as_deref(), source_file, "{api}");
    let span = site.span.expect("authored rule span");
    assert_eq!((span.start.line, span.start.column), (12, 5), "{api}");
    eprintln!("1473 PASS core/{api}: Semantics, full message, authored dbsystem origin at 12:5");
}

#[test]
fn unknown_db_rule_is_rejected_by_direct_lowering() {
    // Deliberately bypass every source loader and CLI validator. A guard in
    // any caller must not conceal an unchecked public lower_db implementation.
    assert_rule_error("lower_db", fsl_core::lower_db(&system(INVALID_DB)), None);
}

#[test]
fn unknown_db_rule_is_rejected_by_source_lowering() {
    assert_rule_error(
        "parse_kernel_source",
        fsl_core::parse_kernel_source(INVALID_DB, &FsResolver::new(".")),
        None,
    );
}

#[test]
fn unknown_db_rule_keeps_its_origin_through_file_source_lowering() {
    assert_rule_error(
        "parse_kernel_source_with_file",
        fsl_core::parse_kernel_source_with_file(INVALID_DB, &FsResolver::new("."), SOURCE_FILE),
        Some(SOURCE_FILE),
    );
}

#[test]
fn every_supported_db_rule_is_accepted_by_direct_and_source_lowering() {
    for rule in RULES {
        let source = INVALID_DB.replace("unregistered_compatibility_rule", rule);
        let resolver = FsResolver::new(".");
        for (api, result) in [
            ("lower_db", fsl_core::lower_db(&system(&source))),
            (
                "parse_kernel_source",
                fsl_core::parse_kernel_source(&source, &resolver),
            ),
            (
                "parse_kernel_source_with_file",
                fsl_core::parse_kernel_source_with_file(&source, &resolver, SOURCE_FILE),
            ),
        ] {
            result.unwrap_or_else(|error| panic!("{rule}/{api}: expected Ok; produced {error}"));
            eprintln!("1473 PASS core/{rule}/{api}: Ok");
        }
    }
}
