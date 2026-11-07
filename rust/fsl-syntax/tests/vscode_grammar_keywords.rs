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
//!   reachable;
//! - every group holding a Japanese spelling stands between
//!   `(?<![\p{XID_Continue}])` and `(?![\p{XID_Continue}])` (or `\s+`), so a
//!   Japanese keyword or particle matches only as a whole identifier, as the
//!   lexer reads it, and never inside a name (`従業員が申請を`).
//!
//! Not covered: whether a pattern matches where the parser reads the
//! keyword, and how Oniguruma evaluates the lookarounds. The grammar is
//! regex-based and approximate; this test checks its vocabulary and the
//! written shape of its Japanese boundaries.

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

/// The lookbehind and lookahead that make a Japanese spelling a whole
/// identifier: no identifier character directly before or after it.
const NOT_AFTER_IDENTIFIER: &str = r"(?<![\p{XID_Continue}])";
const NOT_BEFORE_IDENTIFIER: &str = r"(?![\p{XID_Continue}])";

/// Every innermost capturing group of `regex`, as the byte range of the
/// group including its parentheses and the text between them. Lookarounds
/// and other `(?…)` groups are skipped, and so are escaped parentheses (`\(`).
fn groups(regex: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut found = Vec::new();
    let mut open = None;
    let mut escaped = false;
    for (index, ch) in regex.char_indices() {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '(' => open = Some(index),
            ')' => {
                if let Some(start) = open.take() {
                    let group = &regex[start + 1..index];
                    if !group.starts_with('?') {
                        found.push((start..index + 1, group));
                    }
                }
            }
            _ => {}
        }
    }
    found
}

/// The alternatives of every innermost capturing group of `regex`
/// (`\b(a|b\s+c)\b` gives `a` and `b c`).
fn alternatives(regex: &str) -> Vec<String> {
    groups(regex)
        .into_iter()
        .flat_map(|(_, group)| group.split('|').map(|alt| alt.replace("\\s+", " ")))
        .collect()
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

/// Every `match` and `begin` regex below `value`.
fn regexes<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                match (key.as_str(), child.as_str()) {
                    ("match" | "begin", Some(regex)) => found.push(regex),
                    _ => regexes(child, found),
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                regexes(child, found);
            }
        }
        _ => {}
    }
}

#[test]
fn japanese_spellings_match_only_as_whole_identifiers() {
    let grammar = grammar();
    let mut all = Vec::new();
    regexes(&grammar, &mut all);
    let mut guarded = BTreeSet::new();
    for regex in all {
        for (range, group) in groups(regex) {
            if group.is_ascii() {
                continue;
            }
            let after = &regex[range.end..];
            assert!(
                regex[..range.start].ends_with(NOT_AFTER_IDENTIFIER)
                    && (after.starts_with(NOT_BEFORE_IDENTIFIER) || after.starts_with(r"\s+")),
                "the Japanese group ({group}) of {regex:?} is not between \
                 {NOT_AFTER_IDENTIFIER} and {NOT_BEFORE_IDENTIFIER} (or \\s+)"
            );
            guarded.extend(group.split('|'));
        }
    }
    let japanese = KEYWORDS
        .iter()
        .filter(|keyword| keyword.status == Status::Active)
        .map(|keyword| keyword.japanese)
        .chain(PARTICLES.iter().map(|(_, spelling)| *spelling))
        .filter(|spelling| !guarded.contains(spelling))
        .collect::<Vec<_>>();
    assert!(
        japanese.is_empty(),
        "{japanese:?} are in no guarded Japanese group"
    );
}
