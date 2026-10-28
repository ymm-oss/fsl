// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_dependency_cases.rs"]
mod cases;
#[path = "support/edition_cases.rs"]
mod common;
/// The single browser-only negative is a literal, not another case registry.
/// Pin its bytes to the strict issue pair in the shared Rust table.
#[test]
fn dependency_browser_literal_is_the_exact_rust_issue_pair() {
    let browser = include_str!("../../fsl-wasm/test-browser.mjs");
    let literal = |name: &str| {
        browser
            .split_once(&format!("const {name} = String.raw`"))
            .unwrap()
            .1
            .split_once("`;")
            .unwrap()
            .0
    };
    let row = cases::flat_rows()
        .into_iter()
        .find(|row| row.id == "E01/N")
        .unwrap();
    let fixture = cases::fixture(&row);
    assert_eq!(literal("dependencyRoot"), fixture.root);
    assert_eq!(literal("dependencyChild"), fixture.files[&fixture.child]);
    println!(
        "BROWSER_LITERAL root Rust={} JS={} child Rust={} JS={} all bytes equal",
        fixture.root.len(),
        literal("dependencyRoot").len(),
        fixture.files[&fixture.child].len(),
        literal("dependencyChild").len()
    );
}
