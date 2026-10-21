// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! How `explain` spells names for a reader of a document with a glossary.
//!
//! Everything `explain` computes is in canonical names (name resolution
//! replaced every alias before lowering). With aliases shown, each
//! identifier that has an alias in the model's [`fsl_core::Glossary`] is
//! respelled as that alias, in identifier fields, in expression text and in
//! the readable rendering. Free text (a requirement's own wording, a reason)
//! is the author's and is left as written.

use serde_json::{Map, Value};

/// Which spelling `explain --names` shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NameSpelling {
    /// The canonical names implementations see.
    Canonical,
    /// Each canonical name's glossary alias, where it has one.
    Alias,
}

impl NameSpelling {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "canonical" => Ok(Self::Canonical),
            "alias" => Ok(Self::Alias),
            _ => Err("--names must be canonical or alias".to_owned()),
        }
    }
}

/// Keys whose string values are prose, never respelled.
const FREE_TEXT_KEYS: &[&str] = &["text", "reason", "recommended_action"];

/// Respell `output` (an `explain` envelope) in aliases.
pub(crate) fn show_aliases(output: &mut Value, glossary: &fsl_core::Glossary) {
    if glossary.is_empty() {
        return;
    }
    let mut prose = Vec::new();
    collect_prose(output, None, &mut prose);
    prose.sort_by_key(|text| std::cmp::Reverse(text.len()));
    prose.dedup();
    respell_value(output, None, glossary, &prose);
}

fn collect_prose(value: &Value, key: Option<&str>, prose: &mut Vec<String>) {
    match value {
        Value::String(text) if key.is_some_and(|key| FREE_TEXT_KEYS.contains(&key)) => {
            prose.push(text.clone());
        }
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_prose(item, key, prose)),
        Value::Object(object) => object
            .iter()
            .for_each(|(key, item)| collect_prose(item, Some(key), prose)),
        _ => {}
    }
}

fn respell_value(
    value: &mut Value,
    key: Option<&str>,
    glossary: &fsl_core::Glossary,
    prose: &[String],
) {
    match value {
        Value::String(text) => {
            if !key.is_some_and(|key| FREE_TEXT_KEYS.contains(&key)) {
                *text = respell_text(text, glossary, prose);
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| respell_value(item, key, glossary, prose)),
        Value::Object(object) => {
            let respelled = std::mem::take(object)
                .into_iter()
                .map(|(key, mut item)| {
                    respell_value(&mut item, Some(&key), glossary, prose);
                    (respell_text(&key, glossary, &[]), item)
                })
                .collect::<Map<_, _>>();
            *object = respelled;
        }
        _ => {}
    }
}

/// Respell every ASCII identifier of `text` that has an alias, leaving each
/// occurrence of a `prose` string as written.
fn respell_text(text: &str, glossary: &fsl_core::Glossary, prose: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'outer: while !rest.is_empty() {
        for kept in prose {
            if !kept.is_empty()
                && let Some(after) = rest.strip_prefix(kept.as_str())
            {
                out.push_str(kept);
                rest = after;
                continue 'outer;
            }
        }
        let mut chars = rest.char_indices();
        let (_, first) = chars.next().expect("non-empty rest");
        if first.is_ascii_alphabetic() || first == '_' {
            let end = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .unwrap_or(rest.len());
            let word = &rest[..end];
            out.push_str(glossary.alias(word).unwrap_or(word));
            rest = &rest[end..];
        } else if first.is_ascii_digit() {
            let end = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .unwrap_or(rest.len());
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        } else {
            out.push(first);
            rest = &rest[first.len_utf8()..];
        }
    }
    out
}
