// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Documentation-consistency regression for issue #665's literate-input
//! registry, in the style of `mutation_docs_contract.rs` (issue #338):
//! extract the command names each document claims from its own prose and
//! assert the set equals `fslc_rust::literate_access::LITERATE_REGISTRY`'s
//! `Unsupported`/`Supported` halves, rather than trusting that three
//! hand-written copies of the same 19+3 names stay in sync with the
//! registry and with each other.
//!
//! Before this test the enumeration in `docs/LANGUAGE.md`,
//! `docs/LANGUAGE.ja.md`, and `skills/fsl/reference.md` was accurate but
//! *ungated*: registering a 20th `Unsupported` command in
//! `rust/fslc/src/literate_access.rs` would leave all three documents
//! silently stale, with nothing to fail. This is the gate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fslc_rust::literate_access::{LITERATE_REGISTRY, LITERATE_SUPPORTED_COMMANDS, LiterateSupport};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

/// The registry's `Unsupported` command names, as a set.
fn registry_unsupported() -> BTreeSet<String> {
    LITERATE_REGISTRY
        .iter()
        .filter(|(_, support)| *support == LiterateSupport::Unsupported)
        .map(|(key, _)| (*key).to_owned())
        .collect()
}

/// The registry's `Supported` command names, as a set.
fn registry_supported() -> BTreeSet<String> {
    LITERATE_SUPPORTED_COMMANDS
        .iter()
        .map(|&key| key.to_owned())
        .collect()
}

/// Slice `text` between the end of `start_anchor` and the start of
/// `end_anchor`, panicking with the anchor that went missing rather than
/// silently returning an empty or wrong slice -- a doc rewrite that drops or
/// rewords an anchor must fail loudly here, not produce a false pass.
fn between<'a>(text: &'a str, start_anchor: &str, end_anchor: &str) -> &'a str {
    let start = text
        .find(start_anchor)
        .unwrap_or_else(|| panic!("missing start anchor {start_anchor:?}"))
        + start_anchor.len();
    let rest = &text[start..];
    let end = rest
        .find(end_anchor)
        .unwrap_or_else(|| panic!("missing end anchor {end_anchor:?} after {start_anchor:?}"));
    &rest[..end]
}

/// The text strictly inside the first balanced `(...)` pair that starts
/// after `anchor`.
fn parenthesized_after<'a>(text: &'a str, anchor: &str) -> &'a str {
    let after_anchor = text
        .find(anchor)
        .unwrap_or_else(|| panic!("missing anchor {anchor:?}"))
        + anchor.len();
    let rest = &text[after_anchor..];
    let open = rest
        .find('(')
        .unwrap_or_else(|| panic!("expected '(' after anchor {anchor:?}"));
    let mut depth = 0_i32;
    for (offset, character) in rest[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced parentheses after anchor {anchor:?}");
}

/// Extract command names from backtick-quoted tokens in `list_text`.
///
/// Handles two shapes this prose uses: a bare single-word token (`` `lint` ``)
/// and a compound-command chain written as `` `document generate`/`claims`/`check` ``,
/// where only the first token spells the group's prefix and the rest are
/// joined by a literal `/` with no surrounding space. A token is also
/// stripped of a leading `"fslc "` (`skills/fsl/reference.md` spells the
/// supported three as `` `fslc check` `` etc.) before either rule applies.
fn command_names(list_text: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut search_from = 0;
    while let Some(relative_start) = list_text[search_from..].find('`') {
        let start = search_from + relative_start;
        let Some(relative_end) = list_text[start + 1..].find('`') else {
            break;
        };
        let end = start + 1 + relative_end;
        spans.push((start, end));
        search_from = end + 1;
    }

    let mut names = Vec::new();
    let mut carry: Option<String> = None;
    for (index, &(start, end)) in spans.iter().enumerate() {
        let raw = &list_text[start + 1..end];
        let content = raw.strip_prefix("fslc ").unwrap_or(raw);
        let joiner = if index == 0 {
            ""
        } else {
            &list_text[spans[index - 1].1 + 1..start]
        };
        if content.contains(' ') {
            names.push(content.to_owned());
            carry = content.split(' ').next().map(str::to_owned);
        } else if joiner.trim() == "/" && carry.is_some() {
            names.push(format!(
                "{} {content}",
                carry.as_ref().expect("checked Some")
            ));
        } else {
            names.push(content.to_owned());
            carry = None;
        }
    }
    names
}

struct DocAnchors {
    relative: &'static str,
    supported_start: &'static str,
    supported_end: &'static str,
    unsupported_anchor: &'static str,
}

const DOCS: &[DocAnchors] = &[
    DocAnchors {
        relative: "docs/LANGUAGE.md",
        supported_start: "is not supported.\n\n",
        supported_end: "are the only commands that extract fences",
        unsupported_anchor: "Every other command that reads a spec path ",
    },
    DocAnchors {
        relative: "docs/LANGUAGE.ja.md",
        supported_start: "サポートされません。\n\n",
        supported_end: "の 3 コマンドだけです",
        unsupported_anchor: "仕様パスを読み取る他のすべてのコマンド",
    },
    DocAnchors {
        relative: "skills/fsl/reference.md",
        supported_start: "**Literate Markdown FSL.** ",
        supported_end: "\naccept `.md` files containing",
        unsupported_anchor: "Every other spec-reading command ",
    },
];

/// Each document's `Unsupported` enumeration must equal
/// `LITERATE_REGISTRY`'s `Unsupported` half exactly -- no name missing, none
/// extra, regardless of which document is checked or which command was
/// registered most recently.
#[test]
fn documented_unsupported_commands_match_the_registry() {
    let expected = registry_unsupported();
    for doc in DOCS {
        let text = read(doc.relative);
        let list_text = parenthesized_after(&text, doc.unsupported_anchor);
        let documented: BTreeSet<String> = command_names(list_text).into_iter().collect();

        let missing: Vec<_> = expected.difference(&documented).collect();
        let extra: Vec<_> = documented.difference(&expected).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{} is stale against LITERATE_REGISTRY: missing {missing:?}, extra {extra:?}",
            doc.relative
        );
    }
}

/// Each document's `Supported` list must equal `LITERATE_SUPPORTED_COMMANDS`
/// exactly.
#[test]
fn documented_supported_commands_match_the_registry() {
    let expected = registry_supported();
    for doc in DOCS {
        let text = read(doc.relative);
        let list_text = between(&text, doc.supported_start, doc.supported_end);
        let documented: BTreeSet<String> = command_names(list_text).into_iter().collect();

        let missing: Vec<_> = expected.difference(&documented).collect();
        let extra: Vec<_> = documented.difference(&expected).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{} is stale against LITERATE_SUPPORTED_COMMANDS: missing {missing:?}, extra {extra:?}",
            doc.relative
        );
    }
}

/// Pin the extractor itself against a literal fixture, independent of the
/// registry, so a change to `command_names`'s parsing rules that breaks the
/// `document generate`/`claims`/`check` chain is caught here rather than
/// only as a confusing failure in the two tests above.
#[test]
fn command_names_expands_the_document_subcommand_chain() {
    let text = "`lint`, `migrate`, and\n`document generate`/`claims`/`check` remain";
    assert_eq!(
        command_names(text),
        vec![
            "lint".to_owned(),
            "migrate".to_owned(),
            "document generate".to_owned(),
            "document claims".to_owned(),
            "document check".to_owned(),
        ]
    );
}

#[test]
fn command_names_strips_an_fslc_prefix() {
    assert_eq!(
        command_names("`fslc check`, `fslc verify`, and `fslc scenarios`"),
        vec![
            "check".to_owned(),
            "verify".to_owned(),
            "scenarios".to_owned()
        ]
    );
}
