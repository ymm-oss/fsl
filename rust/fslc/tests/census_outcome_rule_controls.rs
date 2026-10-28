// SPDX-License-Identifier: Apache-2.0

//! #858 real-rule controls, including the comment-blanking false failure.
//! Phase I API in `tests/support/outcome_census_rule.rs`:
//! `pub(crate) fn outcome_types(source: &str) -> BTreeMap<String, bool>`.
//! Move the census's extraction, outcome selection and annotation look-back
//! there; the existing census must aggregate this same rule's results.

use std::collections::BTreeMap;

#[path = "support/census_legacy.rs"]
#[allow(dead_code)]
mod legacy;
#[path = "support/outcome_census_rule.rs"]
mod rule;

#[test]
fn the_real_outcome_rule_keeps_a_public_type_after_a_test_module() {
    let source = "#[cfg(test)]\nmod tests { pub struct TestResult; }\n\
                  pub struct MissedResult { pub outcome: bool }\n";
    assert!(!legacy::outcome_product(source).contains("pub struct MissedResult"));
    assert_eq!(
        rule::outcome_types(source),
        BTreeMap::from([("MissedResult".to_owned(), false)]),
        "the real census must reject this unannotated product outcome only"
    );
}

#[test]
fn a_doc_comment_between_must_use_and_the_type_keeps_the_annotation() {
    let source = "#[must_use]\n/// Carries the verification outcome.\n\
                  pub struct DocumentedResult { pub outcome: bool }\n";
    assert_eq!(
        rule::outcome_types(source),
        BTreeMap::from([("DocumentedResult".to_owned(), true)]),
        "blanked documentation must not terminate the attribute look-back"
    );
}
