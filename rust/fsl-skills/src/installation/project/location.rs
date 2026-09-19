// SPDX-License-Identifier: Apache-2.0

//! Choosing a project's skills directory, without reaching the home directory.

use std::path::{Path, PathBuf};

use crate::installation::{AGENT_DIR, SKILLS_SUBDIR};

/// Why a project skills directory could not be chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No `.claude` was found and there is no repository root to create one in.
    OutsideRepository {
        /// The one directory that was looked at.
        ///
        /// Outside a repository there is no bound to walk within, so the
        /// working directory is the only candidate and the search is one
        /// step. Reporting a list here promised a search that by construction
        /// never happened.
        looked_at: PathBuf,
    },
    /// The search landed on the user-wide directory.
    WouldWriteUserScope {
        /// The directory it landed on.
        path: PathBuf,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutsideRepository { looked_at } => write!(
                formatter,
                "no {AGENT_DIR} directory at {} and no repository root to create one in; \
                 pass --dir, or use --user",
                looked_at.display()
            ),
            Self::WouldWriteUserScope { path } => write!(
                formatter,
                "{} is the user-wide directory; use --user to install there",
                path.display()
            ),
        }
    }
}

/// Choose the skills directory for a project install.
///
/// The walk starts at `cwd` and stops at `repository_root`. The first existing
/// `.claude` wins. Without one, the repository root gets a new one.
///
/// The walk is bounded on purpose. `~/.claude` exists for every Claude Code
/// user, so an unbounded walk from a directory under the home directory always
/// reaches it, quietly turning a project install into a machine-wide one. The
/// bound stops that, and [`settle`] refuses it outright.
///
/// `exists` is a parameter rather than a filesystem call so this decision can
/// be tested without a real home directory.
///
/// # Errors
///
/// When there is no repository root to fall back to, or when the answer would
/// be the user-wide directory.
pub fn resolve_dir(
    cwd: &Path,
    repository_root: Option<&Path>,
    home: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Result<PathBuf, ResolveError> {
    let user_agent_dir = home.join(AGENT_DIR);
    let mut current = Some(cwd);
    while let Some(directory) = current {
        let candidate = directory.join(AGENT_DIR);
        if exists(&candidate) {
            return settle(candidate, &user_agent_dir);
        }
        if repository_root == Some(directory) {
            break;
        }
        current = match repository_root {
            // Outside a repository there is no bound to walk within, so the
            // working directory is the only candidate.
            None => None,
            Some(_) => directory.parent(),
        };
    }
    let Some(root) = repository_root else {
        return Err(ResolveError::OutsideRepository {
            looked_at: cwd.join(AGENT_DIR),
        });
    };
    settle(root.join(AGENT_DIR), &user_agent_dir)
}

/// Accept an agent directory, unless it is the user-wide one.
fn settle(agent_dir: PathBuf, user_agent_dir: &Path) -> Result<PathBuf, ResolveError> {
    if agent_dir == user_agent_dir {
        return Err(ResolveError::WouldWriteUserScope { path: agent_dir });
    }
    Ok(agent_dir.join(SKILLS_SUBDIR))
}
