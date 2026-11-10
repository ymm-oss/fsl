// SPDX-License-Identifier: Apache-2.0

//! The #858 outcome selection and annotation rule.

use std::collections::BTreeMap;

#[path = "product_source.rs"]
mod product_source;

/// Whether a declaration's body carries an outcome, or its name says it is
/// one. This is the same rule #858 measured the codebase with.
fn carries_an_outcome(name: &str, body: &str) -> bool {
    name.ends_with("Result")
        || name.ends_with("Outcome")
        || name.ends_with("Report")
        || name.ends_with("Verdict")
        || name.ends_with("Finding")
        // `Violation` is NOT in the rule #858 states, and that omission is a
        // defect in the rule rather than in its curated table: no `*Violation`
        // type matches any suffix above, and none carries a `violation:` field
        // of its own. `Violation`, `ExplicitViolation`, and `BmcViolation` were
        // annotated only because the hand-written table happened to list them.
        // Adding the suffix surfaced two more the table also missed --
        // `BoundedLivenessViolation` and `LeadsToViolation`.
        || name.ends_with("Violation")
        || body.contains("pub violation:")
        || body.contains("pub outcome:")
}

/// `(type name, annotated)` for public product structs/enums. Comments and
/// direct test-only items are blanked before applying the selection rule.
pub(crate) fn outcome_types(source: &str) -> BTreeMap<String, bool> {
    let mut found = BTreeMap::new();
    let product = product_source::product_source(source);
    let lines: Vec<&str> = product.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line
            .strip_prefix("pub struct ")
            .or_else(|| line.strip_prefix("pub enum "))
        else {
            continue;
        };
        let name = rest
            .split(|character: char| !character.is_alphanumeric() && character != '_')
            .next()
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        // The declaration body, for the `violation`/`outcome` field rule.
        let body = lines[index..]
            .iter()
            .take_while(|candidate| !candidate.starts_with('}'))
            .fold(String::new(), |mut accumulated, candidate| {
                accumulated.push_str(candidate);
                accumulated.push('\n');
                accumulated
            });
        if !carries_an_outcome(name, &body) {
            continue;
        }
        // The attribute block may contain blanked comments (including docs)
        // or whitespace. Stop at the preceding non-attribute product token.
        let annotated = lines[..index]
            .iter()
            .rev()
            .take_while(|candidate| {
                let trimmed = candidate.trim_start();
                trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with(']')
            })
            .any(|candidate| candidate.trim_start().starts_with("#[must_use"));
        found.insert(name.to_owned(), annotated);
    }
    found
}
