// SPDX-License-Identifier: Apache-2.0

//! #1171 real-rule detector.
//! Shared API in `tests/support/helper_census_rule.rs`:
//! `pub(crate) fn root_dependencies(source: &str, modules: &[String])
//!     -> Vec<(usize, String)>`.
//! The module census uses this same extraction and root-path rule, not an AST
//! fixture-scanner adapter.

#[path = "support/census_legacy.rs"]
#[allow(dead_code)]
mod legacy;
#[path = "support/helper_census_rule.rs"]
mod rule;

#[test]
fn the_real_helper_rule_keeps_a_dependency_after_a_url_string() {
    let source = "fn product() { let _url = \"https://example.invalid\"; crate::main_helper(); }\n\
                  #[cfg(test)]\nmod tests { fn test_only() { crate::test_helper(); } }\n";
    assert!(
        legacy::helper_product(source)
            .iter()
            .all(|(_, line)| !line.contains("crate::main_helper"))
    );
    assert_eq!(
        rule::root_dependencies(source, &[]),
        [(1, "crate::main_helper(); }".to_owned())],
        "the actual census must report the product dependency, not test code"
    );
}
