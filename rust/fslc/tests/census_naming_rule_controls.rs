// SPDX-License-Identifier: Apache-2.0

//! #1284 real finding-pipeline controls.
//! Shared API in `tests/support/naming_census_rule.rs`:
//! `pub(crate) fn findings(path: &str, source: &str) -> Vec<Finding>`;
//! `pub(crate) enum Kind` retains the four existing variants;
//! `pub(crate) struct Finding` retains its existing fields, each `pub(crate)`:
//! `path: String, line: usize, kind: Kind, text: String`.
//! The census uses this same extraction/line matcher/owner exemption, retaining
//! its allowed set and drift assertions.

#[path = "support/census_legacy.rs"]
#[allow(dead_code)]
mod legacy;
#[path = "support/naming_census_rule.rs"]
mod rule;

use rule::Kind;

fn assert_one_finding(source: &str, kind: Kind, line: usize) {
    let found = rule::findings("fixture.rs", source);
    assert_eq!(found.len(), 1, "one real naming finding: {found:?}");
    assert_eq!(found[0].path, "fixture.rs");
    assert_eq!(found[0].line, line);
    assert_eq!(found[0].kind, kind);
}

#[test]
fn block_comment_before_display_name_parenthesis_preserves_detection() {
    let source = "fn product() { display_name/**/(value); }";
    assert_eq!(
        legacy::naming_product(source)
            .join("\n")
            .matches("display_name(")
            .count(),
        1,
        "the old lexer detected this valid Rust call"
    );
    assert_one_finding(source, Kind::DisplayName, 1);
}

#[test]
fn block_comment_before_unmapped_spelling_parenthesis_preserves_detection() {
    let source = "fn product() { unmapped_public_spelling/**/(value); }";
    assert_eq!(
        legacy::naming_product(source)
            .join("\n")
            .matches("unmapped_public_spelling(")
            .count(),
        1,
        "the old lexer detected this valid Rust call"
    );
    assert_one_finding(source, Kind::UnmappedSpelling, 1);
}

#[test]
fn a_unix_product_item_is_not_a_test_only_item() {
    // Real cfg semantics: this item is product code on Unix, not a made-up
    // feature containing the word `test`. The direct-item policy retains it.
    let source = "#[cfg(any(test, unix))]\nfn product() { display_name(value); }\n\
                  #[cfg(test)]\nfn test_only() { display_name(value); }\n";
    assert_eq!(
        legacy::naming_product(source)
            .join("\n")
            .matches("display_name(")
            .count(),
        0,
        "the old substring classifier incorrectly skips this Unix product item"
    );
    assert_one_finding(source, Kind::DisplayName, 2);
}
