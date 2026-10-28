// SPDX-License-Identifier: Apache-2.0

//! The #1284 naming finding pipeline, including its owner exemption.

use std::collections::BTreeSet;

#[path = "product_source.rs"]
mod product_source;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Kind {
    /// A string literal that starts with an engine-synthesised name prefix.
    SyntheticPrefix,
    /// A direct call of the internal-to-public string rule.
    DisplayName,
    /// `__` joined to a format placeholder: a name built from two parts.
    SeparatorJoin,
    /// The model-free string rule, for a context with no `NameMap`.
    UnmappedSpelling,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Finding {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) kind: Kind,
    pub(crate) text: String,
}

pub(crate) const OWNER: &str = "rust/fsl-core/src/naming.rs";

const SYNTHETIC_PREFIXES: &[&str] = &["_partial_", "_bounds_", "_requires_failed_", "_key_domain_"];

/// Comments and direct test-only items are blanked, retaining line positions
/// and string literal contents.
pub(crate) fn non_test_lines(source: &str) -> Vec<String> {
    product_source::product_source(source)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn call_offsets<'a>(text: &'a str, name: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(name).filter_map(move |(offset, _)| {
        text[offset + name.len()..]
            .trim_start()
            .starts_with('(')
            .then_some(offset)
    })
}

pub(crate) fn line_findings(path: &str, line: usize, text: &str) -> Vec<Finding> {
    let mut kinds = BTreeSet::new();
    if SYNTHETIC_PREFIXES
        .iter()
        .any(|prefix| text.contains(&format!("\"{prefix}")))
    {
        kinds.insert(Kind::SyntheticPrefix);
    }
    if call_offsets(text, "display_name").any(|offset| {
        !text[..offset]
            .chars()
            .next_back()
            .is_some_and(|previous| previous.is_alphanumeric() || previous == '_')
    }) {
        kinds.insert(Kind::DisplayName);
    }
    if text.contains("}__") || text.contains("__{") {
        kinds.insert(Kind::SeparatorJoin);
    }
    if call_offsets(text, "unmapped_public_spelling")
        .next()
        .is_some()
    {
        kinds.insert(Kind::UnmappedSpelling);
    }
    kinds
        .into_iter()
        .map(|kind| Finding {
            path: path.to_owned(),
            line,
            kind,
            text: text.trim().to_owned(),
        })
        .collect()
}

pub(crate) fn findings(path: &str, source: &str) -> Vec<Finding> {
    if path == OWNER {
        return Vec::new();
    }
    non_test_lines(source)
        .into_iter()
        .enumerate()
        .flat_map(|(index, text)| line_findings(path, index + 1, &text))
        .collect()
}
