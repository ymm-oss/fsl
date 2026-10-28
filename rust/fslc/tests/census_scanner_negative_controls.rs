// SPDX-License-Identifier: Apache-2.0

//! Scratch-source calibrations: legacy extraction misses a real product site,
//! while syn with the inventory's item-level test policy retains it.
//! Phase I replaces only `support/census_fixture_scanner.rs` with the shared
//! scanner adapter; these controls and the frozen legacy extractors stay pinned.

#[path = "support/census_legacy.rs"]
mod legacy;

#[path = "support/census_fixture_scanner.rs"]
mod fixture_scanner;

use fixture_scanner::scan;

#[test]
fn trace_census_keeps_product_after_an_impl_local_test_item() {
    let source = r#"
struct Reader;
impl Reader {
    #[cfg(test)]
    fn test_only() { requirements_trace_contract("test"); }
    fn product() { requirements_trace_contract("product"); }
}
"#;
    let legacy = legacy::trace_product(source)
        .matches("requirements_trace_contract(")
        .count();
    assert_eq!(
        legacy, 0,
        "the frozen #1285 extractor misses the product call"
    );
    let found = scan(source)
        .paths
        .into_iter()
        .filter(|p| p == "requirements_trace_contract")
        .count();
    assert_eq!(found, 1, "the product call counts, not the test call");
}

#[test]
fn outcome_census_keeps_public_types_after_a_test_module() {
    let source = r#"
#[cfg(test)]
mod tests { pub struct TestResult; }
pub struct MissedResult { pub outcome: bool }
"#;
    assert!(!legacy::outcome_product(source).contains("pub struct MissedResult"));
    assert_eq!(scan(source).public_structs, ["MissedResult"]);
}

#[test]
fn naming_census_does_not_confuse_a_feature_name_with_cfg_test() {
    let source = r#"
#[cfg(feature = "test-adapter")]
fn product() { display_name("product"); }
#[cfg(test)]
fn test_only() { display_name("test"); }
"#;
    let legacy = legacy::naming_product(source)
        .join("\n")
        .matches("display_name(")
        .count();
    assert_eq!(
        legacy, 0,
        "#1284's substring test incorrectly discards a product feature"
    );
    let found = scan(source)
        .paths
        .into_iter()
        .filter(|p| p == "display_name")
        .count();
    assert_eq!(
        found, 1,
        "feature-gated product code counts, test-only code does not"
    );
}

#[test]
fn helper_census_keeps_a_root_dependency_after_slashes_in_a_string() {
    let source = r#"
fn product() { let _url = "https://example.invalid"; crate::main_helper(); }
#[cfg(test)]
mod tests { fn test_only() { crate::test_helper(); } }
"#;
    let legacy = legacy::helper_product(source)
        .into_iter()
        .filter(|(_, line)| line.contains("crate::main_helper"))
        .count();
    assert_eq!(
        legacy, 0,
        "#1171's line-comment cut drops the product dependency"
    );
    assert_eq!(scan(source).paths, ["crate::main_helper"]);
}
