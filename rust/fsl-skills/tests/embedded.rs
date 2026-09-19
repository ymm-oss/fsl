// SPDX-License-Identifier: Apache-2.0

//! The binary's embedded skills must equal the repository's `skills/` tree.
//!
//! Without this gate, "the packaged skills are the ones in the repository" is a
//! claim rather than a checked property.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fsl_skills::{EMBEDDED_SKILL_FILES, embedded_skill_files, embedded_skill_names};

fn skills_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("skills")
}

fn collect(dir: &Path, root: &Path, into: &mut BTreeMap<String, String>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
        .map(|entry| entry.expect("read skills entry").path())
        .collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            collect(&entry, root, into);
            continue;
        }
        let relative = entry
            .strip_prefix(root)
            .expect("skills-relative path")
            .to_string_lossy()
            .replace('\\', "/");
        let contents = std::fs::read_to_string(&entry)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", entry.display()));
        into.insert(relative, contents);
    }
}

fn on_disk() -> BTreeMap<String, String> {
    let root = skills_root();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", root.display()))
        .map(|entry| entry.expect("read skills entry").path())
        .filter(|path| path.is_dir())
        .collect();
    entries.sort();
    let mut files = BTreeMap::new();
    for skill in entries {
        collect(&skill, &root, &mut files);
    }
    files
}

fn embedded() -> BTreeMap<String, String> {
    EMBEDDED_SKILL_FILES
        .iter()
        .map(|(path, contents)| ((*path).to_owned(), (*contents).to_owned()))
        .collect()
}

#[test]
fn every_repository_skill_file_is_embedded() {
    let disk: Vec<String> = on_disk().keys().cloned().collect();
    let built: Vec<String> = embedded().keys().cloned().collect();
    assert_eq!(
        built, disk,
        "the embedded skill files and skills/ on disk disagree"
    );
}

#[test]
fn every_embedded_skill_file_matches_its_source() {
    for (path, expected) in on_disk() {
        let actual = embedded()
            .get(&path)
            .unwrap_or_else(|| panic!("{path} is not embedded"))
            .clone();
        assert_eq!(
            actual, expected,
            "embedded {path} differs from skills/{path}"
        );
    }
}

#[test]
fn a_file_directly_under_skills_is_not_a_skill() {
    let stray: Vec<&str> = EMBEDDED_SKILL_FILES
        .iter()
        .map(|(path, _)| *path)
        .filter(|path| !path.contains('/'))
        .collect();
    assert!(
        stray.is_empty(),
        "skills/ holds files that are not inside a skill: {stray:?}"
    );
}

#[test]
fn every_embedded_skill_carries_a_skill_md() {
    let names = embedded_skill_names();
    assert!(!names.is_empty(), "no skill is embedded");
    for name in names {
        let manifest = format!("{name}/SKILL.md");
        assert!(
            embedded_skill_files(name)
                .iter()
                .any(|(path, _)| *path == manifest),
            "{name} has no SKILL.md"
        );
    }
}

#[test]
fn the_embedded_skill_names_match_the_directories_on_disk() {
    let root = skills_root();
    let mut expected: Vec<String> = std::fs::read_dir(&root)
        .expect("read skills/")
        .map(|entry| entry.expect("read skills entry").path())
        .filter(|path| path.is_dir())
        .map(|path| {
            path.file_name()
                .expect("skill directory name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    expected.sort();
    let actual: Vec<String> = embedded_skill_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(actual, expected);
}
