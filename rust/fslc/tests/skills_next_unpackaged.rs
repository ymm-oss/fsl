// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The next edition's skills (`skills-next/`, decisions/048 J4) stay out of
//! the release until #1437 moves them into `skills/`.
//!
//! This is the packaging half of the former `unstable_gate_leak.rs`, kept
//! when #1434 removed the gate: removing the gate did not move the skills, so
//! the release workflow and the installer must still not package them, and
//! the removed `FSLC_UNSTABLE` variable must not appear in them (it would be a
//! stale instruction once they move).

use std::path::{Path, PathBuf};

/// The directory of the next edition's skills (decisions/048 J4).
const NEXT_SKILLS: &str = "skills-next";

/// The removed opt-in variable.
const REMOVED_VARIABLE: &str = "FSLC_UNSTABLE";

/// How the release workflow lists the skill directories it packages and
/// declares (`.github/workflows/release.yml`).
const BUNDLE_LISTING: &str = "find skills -mindepth 1 -maxdepth 1 -type d";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for entry in std::fs::read_dir(path).expect("read skills directory") {
            collect(&entry.expect("directory entry").path(), out);
        }
    } else if path.is_file() {
        out.push(path.to_owned());
    }
}

#[test]
fn next_edition_skills_stay_unpackaged_and_never_name_the_removed_variable() {
    let root = root();
    let mut files = Vec::new();
    collect(&root.join(NEXT_SKILLS), &mut files);
    assert!(files.len() >= 8, "scan floor: only {} files", files.len());
    let mut failures = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("read next-edition skill");
        for (index, line) in text.lines().enumerate() {
            if line.contains(REMOVED_VARIABLE) {
                failures.push(format!(
                    "{}:{}: {}",
                    file.strip_prefix(&root).expect("under root").display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the removed {REMOVED_VARIABLE} variable appears in {NEXT_SKILLS}/:\n{}",
        failures.join("\n")
    );

    let release = std::fs::read_to_string(root.join(".github/workflows/release.yml"))
        .expect("read the release workflow");
    let listings = release
        .lines()
        .filter(|line| line.contains("find skills"))
        .map(str::trim)
        .collect::<Vec<_>>();
    assert!(
        listings.len() >= 3
            && listings.iter().all(|line| line.starts_with(BUNDLE_LISTING)
                && !line[BUNDLE_LISTING.len()..].contains("skills")),
        "every skill listing of the release must be `{BUNDLE_LISTING}`: {listings:#?}"
    );
    for published in [".github/workflows/release.yml", "install.sh"] {
        let text = std::fs::read_to_string(root.join(published)).expect("read release file");
        assert!(
            !text.contains(NEXT_SKILLS),
            "{published} names {NEXT_SKILLS}/, which must not be packaged before #1437 moves it"
        );
    }
}
