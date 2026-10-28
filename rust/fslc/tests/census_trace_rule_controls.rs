// SPDX-License-Identifier: Apache-2.0

//! #1285 real-rule detector, not the fixture AST scanner.
//! Phase I API in `tests/support/trace_census_rule.rs`:
//! `pub(crate) fn calls_in_source(source: &str, name: &str) -> usize`.
//! Move the real extraction + `calls` rule there and route the census through
//! it; do not implement a second recognizer just for this test.

#[path = "support/census_legacy.rs"]
#[allow(dead_code)]
mod legacy;
#[path = "support/trace_census_rule.rs"]
mod rule;

#[test]
fn the_real_trace_rule_counts_product_after_an_impl_local_test() {
    let source = r#"
struct Reader;
impl Reader {
    #[cfg(test)]
    fn test_only() { requirements_trace_contract("test"); }
    fn product() { requirements_trace_contract("product"); }
}
"#;
    assert_eq!(
        legacy::trace_product(source)
            .matches("requirements_trace_contract(")
            .count(),
        0,
        "the frozen extractor misses the product call"
    );
    assert_eq!(
        rule::calls_in_source(source, "requirements_trace_contract"),
        1,
        "the census must count the product call, and not the test call"
    );
}
