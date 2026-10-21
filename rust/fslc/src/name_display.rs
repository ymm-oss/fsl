// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! How `explain` spells names for a reader of a document with a glossary.
//!
//! What `explain` computes is in canonical names (name resolution replaced
//! every alias before lowering), except where it quotes the source as
//! written (`weakening.source_text`). One spelling is shown throughout: with
//! aliases, each identifier that has an alias in the model's
//! [`fsl_core::Glossary`] is respelled as that alias; with canonical names,
//! each alias quoted from the source is respelled as its canonical name. This
//! covers identifier fields, expression and source text, and the readable
//! rendering. Free text (a requirement's own wording, a reason) is the
//! author's and is left as written.

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

/// Respell `output` (an `explain` envelope) in `spelling`.
pub(crate) fn respell(output: &mut Value, glossary: &fsl_core::Glossary, spelling: NameSpelling) {
    if glossary.is_empty() {
        return;
    }
    let mut prose = Vec::new();
    collect_prose(output, None, &mut prose);
    prose.sort_by_key(|text| std::cmp::Reverse(text.len()));
    prose.dedup();
    let rename = |word: &str| match spelling {
        NameSpelling::Alias => glossary.alias(word).map(str::to_owned),
        NameSpelling::Canonical => glossary.canonical(word).map(str::to_owned),
    };
    respell_value(output, None, &rename, &prose);
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

type Rename<'a> = dyn Fn(&str) -> Option<String> + 'a;

fn respell_value(value: &mut Value, key: Option<&str>, rename: &Rename<'_>, prose: &[String]) {
    match value {
        Value::String(text) => {
            if !key.is_some_and(|key| FREE_TEXT_KEYS.contains(&key)) {
                *text = respell_text(text, rename, prose);
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| respell_value(item, key, rename, prose)),
        Value::Object(object) => {
            let respelled = std::mem::take(object)
                .into_iter()
                .map(|(key, mut item)| {
                    respell_value(&mut item, Some(&key), rename, prose);
                    (respell_text(&key, rename, &[]), item)
                })
                .collect::<Map<_, _>>();
            *object = respelled;
        }
        _ => {}
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Respell every identifier of `text` that `rename` maps, leaving each
/// occurrence of a `prose` string as written. An identifier is a maximal run
/// of letters, digits and `_` that does not start with a digit.
pub(crate) fn respell_text(text: &str, rename: &Rename<'_>, prose: &[String]) -> String {
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
        let first = rest.chars().next().expect("non-empty rest");
        if is_word_char(first) {
            let end = rest
                .find(|ch: char| !is_word_char(ch))
                .unwrap_or(rest.len());
            let word = &rest[..end];
            match rename(word).filter(|_| !first.is_numeric()) {
                Some(renamed) => out.push_str(&renamed),
                None => out.push_str(word),
            }
            rest = &rest[end..];
        } else {
            out.push(first);
            rest = &rest[first.len_utf8()..];
        }
    }
    out
}
