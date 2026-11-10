// SPDX-License-Identifier: Apache-2.0

//! The #1285 trace-call rule, shared by the census and its fixture controls.

#[path = "product_source.rs"]
mod product_source;

pub(crate) fn product_text(source: &str) -> String {
    product_source::product_source(source)
}

pub(crate) fn calls_in_source(source: &str, name: &str) -> usize {
    calls(&product_text(source), name)
}

/// Calls of `name`: the identifier followed by `(`, not preceded by another
/// identifier character or by `fn `.
pub(crate) fn calls(text: &str, name: &str) -> usize {
    let needle = format!("{name}(");
    text.match_indices(&needle)
        .filter(|(at, _)| {
            let before = &text[..*at];
            !before.ends_with(|character: char| character.is_alphanumeric() || character == '_')
                && !before.ends_with("fn ")
        })
        .count()
}
