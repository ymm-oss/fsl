// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The unstable gate is not advertised (decisions/048 J1): its environment
//! variable and the next edition's header must not appear in anything
//! published to users. Internal design notes under `docs/design/` are the one
//! exception. This check is removed when the gate is lifted.

use std::path::{Path, PathBuf};

/// Repo-relative files and directories published to users.
const PUBLISHED: &[&str] = &["docs", "skills", "README.md", "CHANGELOG.md", "changelog.d"];

/// Repo-relative directories inside [`PUBLISHED`] that are internal.
const INTERNAL: &[&str] = &["docs/design"];

/// The gate's environment variable (`fslc_rust::unstable_gate`).
const GATE_VARIABLE: &str = fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE;

/// Every leak in `text`, as `line: excerpt`. A leak is the gate variable
/// anywhere, or the word `fsl` followed by blanks and the edition number `5`
/// (a header line or an inline mention of it).
fn leaks_in_text(text: &str) -> Vec<String> {
    let mut leaks = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.contains(GATE_VARIABLE) || mentions_next_edition(line) {
            leaks.push(format!("{}: {}", index + 1, line.trim()));
        }
    }
    leaks
}

fn mentions_next_edition(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(found) = line[start..].find("fsl") {
        let at = start + found;
        let word_start = at == 0 || !is_word(bytes[at - 1]);
        let rest = &line[at + 3..];
        let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let after = &rest[blanks..];
        if word_start
            && blanks > 0
            && after.starts_with('5')
            && !after.as_bytes().get(1).is_some_and(|&next| is_word(next))
        {
            return true;
        }
        start = at + 3;
    }
    false
}

const fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

fn collect(root: &Path, path: &Path, out: &mut Vec<PathBuf>) {
    let relative = path
        .strip_prefix(root)
        .expect("under root")
        .to_string_lossy()
        .replace('\\', "/");
    if INTERNAL.contains(&relative.as_str()) {
        return;
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path).expect("read published directory") {
            collect(root, &entry.expect("directory entry").path(), out);
        }
    } else if path.is_file() {
        out.push(path.to_owned());
    }
}

#[test]
fn published_material_does_not_mention_the_unstable_gate() {
    let root = root();
    let mut files = Vec::new();
    for published in PUBLISHED {
        let path = root.join(published);
        assert!(path.exists(), "published path {published} is missing");
        collect(&root, &path, &mut files);
    }
    // A release folds changelog.d/ into CHANGELOG.md and empties it.
    let changelog_d = root.join("changelog.d");
    let lasting = files
        .iter()
        .filter(|file| !file.starts_with(&changelog_d))
        .count();
    assert!(
        lasting > 80,
        "scan floor: only {lasting} files outside changelog.d/"
    );

    let mut failures = Vec::new();
    for file in files {
        let bytes = std::fs::read(&file).expect("read published file");
        let text = String::from_utf8_lossy(&bytes);
        for leak in leaks_in_text(&text) {
            failures.push(format!(
                "{}:{leak}",
                file.strip_prefix(&root).expect("under root").display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "the unstable gate leaked into published material (keep it in docs/design/ only):\n{}",
        failures.join("\n")
    );
}

#[test]
fn leak_detector_flags_the_variable_and_the_header_only() {
    assert_eq!(
        leaks_in_text("intro\n  export FSLC_UNSTABLE=fsl5\n"),
        vec!["2: export FSLC_UNSTABLE=fsl5".to_owned()]
    );
    for leaked in [
        "fsl 5",
        "```fsl\nfsl 5\n```",
        "start with `fsl\t5` on line one",
    ] {
        assert!(!leaks_in_text(leaked).is_empty(), "{leaked:?}");
    }
    for clean in [
        "fslc 4.12.0",
        "fsl 50 widgets",
        "mfsl 5",
        "fsl 4",
        "fsl5_lowering",
        "fsl  4.5",
    ] {
        assert!(leaks_in_text(clean).is_empty(), "{clean:?}");
    }
}
