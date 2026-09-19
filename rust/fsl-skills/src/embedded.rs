// SPDX-License-Identifier: Apache-2.0

//! The generated table of Agent Skills compiled into the binary.
//!
//! `build.rs` generates the table below from the repository's `skills/`
//! directory. Every directory there is a skill. A file sitting directly under
//! `skills/`, such as its README, is not one, so it is not embedded.
//!
//! Declared on the library side because integration tests under `tests/` link
//! the library rather than the binary.

include!(concat!(env!("OUT_DIR"), "/embedded_skills.rs"));

/// The embedded skill names, sorted by name, each appearing once.
///
/// `EMBEDDED_SKILL_FILES` is ordered by relative path, so `fsl-business/...`
/// precedes `fsl/...`: `-` sorts before `/`. Sort here rather than relying on
/// that order, so the two orderings cannot drift apart.
#[must_use]
pub fn embedded_skill_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = EMBEDDED_SKILL_FILES
        .iter()
        .map(|(path, _)| skill_name_of(path))
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// The files belonging to one skill, keyed by their path relative to `skills/`.
#[must_use]
pub fn embedded_skill_files(skill: &str) -> Vec<(&'static str, &'static str)> {
    EMBEDDED_SKILL_FILES
        .iter()
        .filter(|(path, _)| skill_name_of(path) == skill)
        .copied()
        .collect()
}

fn skill_name_of(path: &'static str) -> &'static str {
    path.split('/').next().unwrap_or(path)
}
