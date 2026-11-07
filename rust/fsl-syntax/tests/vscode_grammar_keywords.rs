// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The VS Code extension's `TextMate` grammar
//! (`editors/vscode/syntaxes/fsl.tmLanguage.json`) highlights the keyword
//! table ([`fsl_syntax::keywords`]) in step with it:
//!
//! - every active row's English spelling (a multi-word one as one phrase,
//!   written `only\s+when` in the grammar) and Japanese spelling, and every
//!   particle, is an alternative of a pattern scoped `keyword.*`;
//! - the `fsl5-keywords` patterns hold nothing else, so a row removed from
//!   or reserved in the table leaves the grammar too;
//! - every repository rule is included somewhere, so the FSL 5 rules stay
//!   reachable.
//!
//! Not covered: whether a pattern matches where the parser reads the
//! keyword. The grammar is regex-based and approximate; this test only keeps
//! its vocabulary from falling behind the table.

use std::collections::BTreeSet;
use std::path::Path;

use fsl_syntax::keywords::{KEYWORDS, PARTICLES, Status};
use serde_json::Value;

fn grammar() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../editors/vscode/syntaxes/fsl.tmLanguage.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The alternatives of every innermost capturing group of `regex`
/// (`\b(a|b\s+c)\b` gives `a` and `b c`). Lookarounds and other `(?…)`
/// groups are skipped, and so are escaped parentheses (`\(`).
fn alternatives(regex: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut open = None;
    let mut escaped = false;
    for (index, ch) in regex.char_indices() {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '(' => open = Some(index + 1),
            ')' => {
                if let Some(start) = open.take() {
                    let group = &regex[start..index];
                    if !group.starts_with('?') {
                        found.extend(group.split('|').map(|alt| alt.replace("\\s+", " ")));
                    }
                }
            }
            _ => {}
        }
    }
    found
}

/// The alternatives of every `match` in a pattern scoped `keyword.*`, below
/// `value`.
fn keyword_alternatives(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            let scope = object.get("name").and_then(Value::as_str).unwrap_or("");
            if let (true, Some(regex)) = (
                scope.starts_with("keyword."),
                object.get("match").and_then(Value::as_str),
            ) {
                found.extend(alternatives(regex));
            }
            for child in object.values() {
                keyword_alternatives(child, found);
            }
        }
        Value::Array(items) => {
            for child in items {
                keyword_alternatives(child, found);
            }
        }
        _ => {}
    }
}

/// Every spelling the grammar must highlight: the active rows in English and
/// Japanese, and the particles.
fn table_spellings() -> BTreeSet<&'static str> {
    KEYWORDS
        .iter()
        .filter(|keyword| keyword.status == Status::Active)
        .flat_map(|keyword| [keyword.english, keyword.japanese])
        .chain(PARTICLES.iter().map(|(_, spelling)| *spelling))
        .collect()
}

#[test]
fn every_active_keyword_and_particle_is_highlighted() {
    let mut found = BTreeSet::new();
    keyword_alternatives(&grammar(), &mut found);
    let missing = table_spellings()
        .into_iter()
        .filter(|spelling| !found.contains(*spelling))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "fsl.tmLanguage.json does not highlight {missing:?}; add them to `fsl5-keywords` \
         (and a declaration head to `fsl5-declarations`)"
    );
}

#[test]
fn fsl5_keywords_hold_only_the_table() {
    let grammar = grammar();
    let rule = &grammar["repository"]["fsl5-keywords"];
    assert!(rule.is_object(), "the grammar has no `fsl5-keywords` rule");
    let mut found = BTreeSet::new();
    keyword_alternatives(rule, &mut found);
    let table = table_spellings();
    let stale = found
        .iter()
        .filter(|spelling| !table.contains(spelling.as_str()))
        .collect::<Vec<_>>();
    assert!(
        stale.is_empty(),
        "`fsl5-keywords` highlights {stale:?}, which are not active rows or particles of \
         keywords.rs"
    );
}

#[test]
fn every_repository_rule_is_included() {
    let grammar = grammar();
    let text = grammar.to_string();
    let repository = grammar["repository"]
        .as_object()
        .expect("the grammar has a repository");
    for name in repository.keys() {
        assert!(
            text.contains(&format!("\"#{name}\"")),
            "the repository rule `{name}` is never included"
        );
    }
}
