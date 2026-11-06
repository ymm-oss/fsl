// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Every token stream outside the lossless tree leaves the edition header
//! (#1434). The lexer (`Lexer`, private to `fsl-syntax`) keeps the header's
//! tokens; only `lex_with` drops them after `edition_header` reads the line,
//! and every parse entry (`parse_document*`, `dialect_keyword*`, `lex`) goes
//! through it. A new entry that tokenized with `Lexer::new` or
//! `lex_with_header` directly would read `fsl 5` as the identifier `fsl`;
//! this scan names every place allowed to do so.
//!
//! Not covered: code outside `fsl-syntax` that scans raw source text without
//! tokens (for example the migration's raw operator pre-scan).

use std::collections::BTreeSet;
use std::path::Path;

/// `(file, needle)` pairs allowed to bypass the header-reading entry.
const ALLOWED: &[(&str, &str)] = &[
    // The lexer itself: `lex_with` and the identifier/warning helpers, which
    // read single names or report warnings, never a document's tokens.
    ("lexer.rs", "Lexer::new("),
    ("lexer.rs", "lex_with_header("),
    // The lossless tree keeps the header as trivia and reads it itself.
    ("lossless.rs", "lex_with_header("),
];

fn sources(directory: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("read fsl-syntax sources") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn only_the_lexer_and_the_lossless_tree_tokenize_around_the_edition_header() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    assert!(files.len() >= 10, "scan floor: only {} files", files.len());
    let mut found = BTreeSet::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("read source");
        let name = file
            .strip_prefix(&root)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        for needle in ["Lexer::new(", "lex_with_header("] {
            if text.contains(needle) {
                found.insert((name.clone(), needle));
            }
        }
    }
    let allowed = ALLOWED
        .iter()
        .map(|(file, needle)| ((*file).to_owned(), *needle))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        found, allowed,
        "a tokenizer use outside the header-reading entry (or a stale allowance)"
    );
}
