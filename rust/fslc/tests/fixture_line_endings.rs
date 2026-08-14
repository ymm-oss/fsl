// SPDX-License-Identifier: Apache-2.0

//! Rejecting control for the checkout form of every CLI test fixture.
//!
//! `error_envelope_parity` compares CLI envelopes byte-for-byte against these
//! fixtures, so a fixture whose checkout depends on the platform does not mean
//! the same thing on every runner. `.gitattributes` covered `*.fsl` and four
//! named files, but not the `.md`/`.json` fixtures added later: on
//! `windows-latest` those were checked out CRLF, `error_envelope_document.md`'s
//! leading `---` stopped parsing as document frontmatter, and three parity
//! cells failed on that runner alone while Linux and macOS stayed green
//! (run 31759050211).
//!
//! The `.gitattributes` rule fixes the checkout. This test is the control that
//! notices when a new fixture escapes it: it reads the working tree, so on a
//! runner that would reintroduce CRLF it fails with the offending path instead
//! of surfacing as an unrelated envelope mismatch.

use std::path::{Path, PathBuf};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root above rust/fslc")
        .to_path_buf()
}

/// Files that are deliberately not text and are excluded from the check by
/// `.gitattributes`' `binary` rule.
fn is_excluded_binary(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".z3-trace")
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("readable fixture directory") {
        let path = entry.expect("readable fixture entry").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else if !is_excluded_binary(&path) {
            files.push(path);
        }
    }
}

#[test]
fn every_cli_fixture_is_checked_out_with_line_feed_endings() {
    let root = repository_root();
    let fixtures = root.join("rust/fslc/tests/fixtures");

    let mut files = Vec::new();
    collect_files(&fixtures, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "no fixtures found under {}",
        fixtures.display()
    );

    let offenders = files
        .iter()
        .filter(|path| {
            std::fs::read(path)
                .expect("readable fixture")
                .windows(2)
                .any(|pair| pair == b"\r\n")
        })
        .map(|path| {
            path.strip_prefix(&root)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect::<Vec<_>>();

    assert!(
        offenders.is_empty(),
        "fixtures checked out with CRLF endings; add them to `.gitattributes` \
         (`rust/fslc/tests/fixtures/** text eol=lf`) so every runner sees the \
         same bytes: {}",
        offenders.join(", ")
    );
}
