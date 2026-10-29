// SPDX-License-Identifier: Apache-2.0

#[path = "support/ledger_warning_census.rs"]
mod census;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn producer_inventory_resolves_all_named_kinds_and_the_legacy_kindless_shape() {
    let inventory = census::producers(&root()).unwrap();
    assert_eq!(inventory.named.len(), 28, "{inventory:#?}");
    assert_eq!(
        inventory.kindless, 1,
        "deprecated Map warning is separately accounted for"
    );
    assert_eq!(
        inventory.suggestions, 2,
        "live exclusions: frontend insert and migration replace suggestions are edit objects, not warning kinds"
    );
}

#[test]
fn every_producer_kind_has_exactly_one_private_table_entry() {
    let produced = census::producers(&root()).unwrap();
    let ledger = std::fs::read_to_string(root().join("rust/fsl-tools/src/ledger.rs")).unwrap();
    let keys = census::table_keys(&ledger).unwrap();
    census::complete(&produced.named, &keys).unwrap();
    // Pair source completeness with no-fallback/no-drop controls. The independent
    // fsl-tools matrix pins each complete row, not merely the presence of a key.
    let model = fsl_core::build_model(fsl_core::parse_kernel_source(
        "spec Census { state { x: Bool = false } action flip() { x = not x } invariant T { true } }",
        &fsl_core::FsResolver::new("."),
    ).unwrap()).unwrap();
    for kind in produced.named {
        let body = fsl_tools::render_ledger(
            "census.fsl",
            &model,
            &serde_json::json!({"result":"verified", "checked_to_depth":2, "completeness":"bounded", "warnings":[{"kind":kind,"message":"census"}]}),
            &serde_json::json!({}),
            None,
            &[],
        );
        assert!(
            !body.contains("未分類の警告"),
            "registered kind used fallback: {kind}"
        );
        assert_eq!(body.matches("- **検出**:").count(), 1, "{kind}");
    }
}

fn keys(kinds: &[&str]) -> Vec<String> {
    kinds.iter().map(|kind| (*kind).to_owned()).collect()
}

#[test]
fn calibration_new_literal_without_table_entry_is_rejected() {
    let source = r#"fn emit() { warnings.push(json!({"kind":"existing", "message":"m"})); warnings.push(json!({"kind":"new_lane"})); }"#;
    let produced = census::json_kinds(source).unwrap();
    let result = census::complete(&produced.named, &keys(&["existing"]));
    assert_eq!(result.unwrap_err(), "missing=[\"new_lane\"]; stale=[]");
}

#[test]
fn calibration_constant_codes_are_resolved_not_read_as_identifiers() {
    let source = r#"const NEW_CODE: &str = "constant_lane"; fn emit() { warnings.push(json!({"kind":NEW_CODE, "message":"m"})); }"#;
    let produced = census::json_kinds(source).unwrap();
    assert_eq!(produced.named, BTreeSet::from(["constant_lane".to_owned()]));
    assert!(census::complete(&produced.named, &keys(&["old_code"])).is_err());
    census::complete(&produced.named, &keys(&["constant_lane"])).unwrap();
}

#[test]
fn calibration_solver_match_change_is_detected() {
    let before = r#"impl Finding { fn kind(&self) -> &'static str { match self { Self::A => "old_solver" } } }"#;
    let after = r#"impl Finding { fn kind(&self) -> &'static str { match self { Self::A => "old_solver", Self::B => "new_solver" } } }"#;
    let baseline = census::solver_kinds(before)
        .unwrap()
        .into_iter()
        .collect::<Vec<_>>();
    let changed = census::solver_kinds(after).unwrap();
    assert_eq!(
        census::complete(&changed, &baseline).unwrap_err(),
        "missing=[\"new_solver\"]; stale=[]"
    );
}

#[test]
fn calibration_new_split_case_input_derives_a_new_skipped_kind() {
    let source = r#"impl Contract { fn split_cases(&self) { a.map(|case| ("acceptance",case)).chain(b.map(|case| ("new_trace",case))); } }"#;
    let inputs = census::skipped_inputs(source).unwrap();
    assert_eq!(
        inputs,
        BTreeSet::from(["acceptance".to_owned(), "new_trace".to_owned()])
    );
    // The same production warning formatter must combine the derived inputs.
    let formatter =
        r#"fn warning(&self) { json!({"kind":format!("{}_skipped", self.kind),"message":"m"}) }"#;
    let derived = census::skipped_warning_kinds(formatter, inputs).unwrap();
    assert_eq!(
        census::complete(&derived, &keys(&["acceptance_skipped"])).unwrap_err(),
        "missing=[\"new_trace_skipped\"]; stale=[]"
    );
}

#[test]
fn calibration_unknown_computed_forms_fail_closed() {
    let source = r#"fn emit() { warnings.push(json!({"kind":compute_kind(),"message":"m"})); }"#;
    assert!(
        census::json_kinds(source)
            .unwrap_err()
            .contains("unrecognized computed warning expression")
    );
    let split =
        r"impl Contract { fn split_cases(&self) { a.map(|case| (computed_kind(),case)); } }";
    assert!(
        census::skipped_inputs(split)
            .unwrap_err()
            .contains("unrecognized computed warning expression")
    );
}

#[test]
fn calibration_comments_and_test_only_fakes_do_not_count() {
    let source = r#"
        // warnings.push(json!({"kind":"comment_fake", "message":"m"}));
        #[cfg(test)] fn fake() { warnings.push(json!({"kind":"test_fake", "message":"m"})); }
        fn emit() { warnings.push(json!({"kind":"real", "message":"m"})); }
    "#;
    assert_eq!(
        census::json_kinds(source).unwrap().named,
        BTreeSet::from(["real".to_owned()])
    );
}

#[test]
fn calibration_table_duplicate_stale_and_missing_keys_are_rejected() {
    let table = r#"const TABLE: &[(&str, WarningPresentation)] = &[("lane", WarningPresentation { trace_type:"warning" }), ("lane", WarningPresentation { trace_type:"warning" })];"#;
    let parsed = census::table_keys(table).unwrap();
    assert_eq!(parsed, keys(&["lane", "lane"]));
    assert_eq!(
        census::complete(&BTreeSet::from(["lane".to_owned()]), &parsed).unwrap_err(),
        "duplicate warning table key"
    );
    assert_eq!(
        census::complete(&BTreeSet::new(), &keys(&["stale"])).unwrap_err(),
        "missing=[]; stale=[\"stale\"]"
    );
    let named = r#"const TABLE: &[WarningPresentation] = &[WarningPresentation { kind:"lane", trace_type:"warning" }]; const FALLBACK: WarningPresentation = WarningPresentation { kind:"fallback", trace_type:"warning" };"#;
    assert_eq!(census::table_keys(named).unwrap(), keys(&["lane"]));
}
