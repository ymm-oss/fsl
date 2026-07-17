// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Glossary sidecar for `fslc document generate` (issue #330): a
//! presentation-only JSON file mapping a semantic target string
//! (`action:NAME` / `state:NAME` / `enum:Type.Member`) to a natural-language
//! display label. A label changes only how an identifier *displays*; it can
//! never change modality, negation, or conditional structure, because the
//! renderer (issue #326) only ever substitutes a label at a handful of
//! whole-identifier display points — never inside the safe-pattern
//! recognizer's connective/template logic. v1 adds no new FSL grammar: every
//! target is validated against the already-checked `KernelModel`. See
//! `docs/DESIGN-document-glossary.md`.

use std::collections::BTreeMap;
use std::fmt;

use fsl_core::{FslValue, KernelModel};
use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};

use crate::document_render::Locale;

pub const GLOSSARY_SCHEMA: &str = "fslc.document-glossary.v1";

#[derive(Debug)]
pub struct Glossary {
    pub locale: Locale,
    pub labels: BTreeMap<String, String>,
}

/// A parsed glossary paired with a digest of the raw sidecar file bytes
/// (`document_digest::sha256_bytes`) — recorded in a generated document's
/// frontmatter (`glossary_digest`) and compared by `fslc document check` so
/// a re-render always uses the same sidecar the artifact was generated
/// with. The digest is over raw file bytes, not the parsed `Glossary`, so
/// two byte-different-but-semantically-identical files (whitespace,
/// reordering) are correctly treated as a change to compare against — the
/// recorded fact is "this exact sidecar file", the same identity
/// `artifact_digest` uses for the rendered Markdown itself.
pub struct AppliedGlossary<'a> {
    pub glossary: &'a Glossary,
    pub digest: &'a str,
}

#[derive(Debug)]
pub enum GlossaryIssue {
    Json(String),
    UnsupportedSchema(String),
    UnsupportedLocale(String),
    DuplicateTarget(String),
    EmptyLabel(String),
}

impl fmt::Display for GlossaryIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(message) => write!(f, "glossary is not valid JSON: {message}"),
            Self::UnsupportedSchema(schema) => write!(
                f,
                "unsupported glossary schema '{schema}' (expected '{GLOSSARY_SCHEMA}')"
            ),
            Self::UnsupportedLocale(locale) => {
                write!(f, "unsupported glossary locale '{locale}'")
            }
            Self::DuplicateTarget(target) => {
                write!(f, "glossary target '{target}' has more than one label")
            }
            Self::EmptyLabel(target) => {
                write!(f, "glossary target '{target}' has an empty label")
            }
        }
    }
}

/// Every `labels` entry as it appears in the source text, duplicates
/// included. A plain `BTreeMap<String, String>`/`serde_json::Map` field
/// would silently collapse a duplicate JSON key to its last occurrence
/// before this code ever saw it — `serde`'s `MapAccess` visits entries in
/// source order as they are read, so a hand-written `Visitor` that collects
/// into a `Vec` (rather than inserting into a map) is the mechanism that
/// makes `FSL-DOC-LABEL-CONFLICT` detectable at all.
struct LabelEntries(Vec<(String, String)>);

impl<'de> Deserialize<'de> for LabelEntries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor;
        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = LabelEntries;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object mapping glossary targets to labels")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, String>()? {
                    entries.push(entry);
                }
                Ok(LabelEntries(entries))
            }
        }
        deserializer.deserialize_map(EntriesVisitor)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGlossary {
    schema: String,
    locale: String,
    labels: LabelEntries,
}

/// Parse and validate a glossary sidecar file's contents.
///
/// # Errors
///
/// A malformed JSON document returns a single [`GlossaryIssue::Json`]. Once
/// the document parses, every remaining issue (unsupported schema/locale,
/// each duplicate target, each empty label) is collected and returned
/// together, so a single run reports every offending entry rather than
/// stopping at the first.
///
/// # Panics
///
/// Never in practice: an unparseable locale always pushes
/// [`GlossaryIssue::UnsupportedLocale`], which short-circuits to `Err`
/// before the locale is unwrapped.
pub fn parse_glossary(text: &str) -> Result<Glossary, Vec<GlossaryIssue>> {
    let raw: RawGlossary =
        serde_json::from_str(text).map_err(|error| vec![GlossaryIssue::Json(error.to_string())])?;
    let mut issues = Vec::new();
    if raw.schema != GLOSSARY_SCHEMA {
        issues.push(GlossaryIssue::UnsupportedSchema(raw.schema));
    }
    let locale = Locale::parse(&raw.locale);
    if locale.is_none() {
        issues.push(GlossaryIssue::UnsupportedLocale(raw.locale.clone()));
    }
    let mut labels = BTreeMap::new();
    for (target, label) in raw.labels.0 {
        if label.is_empty() {
            issues.push(GlossaryIssue::EmptyLabel(target.clone()));
        }
        if labels.insert(target.clone(), label).is_some() {
            issues.push(GlossaryIssue::DuplicateTarget(target));
        }
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    Ok(Glossary {
        locale: locale.expect("checked above"),
        labels,
    })
}

/// A glossary target with no corresponding semantic element in the checked
/// model (`FSL-DOC-LABEL-UNKNOWN`).
#[derive(Debug, Clone)]
pub struct UnknownTarget {
    pub target: String,
    pub detail: String,
}

/// Every glossary target that does not resolve against `model`: an
/// `action:`/`state:`/`enum:` target the spec does not declare, or a target
/// outside those three namespaces v1 recognizes.
#[must_use]
pub fn unknown_targets(glossary: &Glossary, model: &KernelModel) -> Vec<UnknownTarget> {
    glossary
        .labels
        .keys()
        .filter_map(|target| {
            resolve_target(target, model)
                .err()
                .map(|detail| UnknownTarget {
                    target: target.clone(),
                    detail,
                })
        })
        .collect()
}

fn resolve_target(target: &str, model: &KernelModel) -> Result<(), String> {
    if let Some(name) = target.strip_prefix("action:") {
        return if model.actions.iter().any(|action| action.name == name) {
            Ok(())
        } else {
            Err(format!("no action named '{name}' is declared"))
        };
    }
    if let Some(name) = target.strip_prefix("state:") {
        return if model.state.iter().any(|(state_name, _)| state_name == name) {
            Ok(())
        } else {
            Err(format!("no state variable named '{name}' is declared"))
        };
    }
    if let Some(rest) = target.strip_prefix("enum:") {
        let Some((type_name, member)) = rest.rsplit_once('.') else {
            return Err("an 'enum:' target must be 'enum:Type.Member'".to_owned());
        };
        return match model.enum_members.get(member) {
            Some(FslValue::Enum {
                type_name: actual, ..
            }) if actual == type_name => Ok(()),
            Some(FslValue::Enum {
                type_name: actual, ..
            }) => Err(format!(
                "enum member '{member}' belongs to type '{actual}', not '{type_name}'"
            )),
            _ => Err(format!("no enum member named '{member}' is declared")),
        };
    }
    Err(
        "unrecognized glossary target namespace; v1 supports 'action:', 'state:', 'enum:'"
            .to_owned(),
    )
}
