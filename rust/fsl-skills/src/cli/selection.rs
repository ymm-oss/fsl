// SPDX-License-Identifier: Apache-2.0

//! Turning the flags a run was given into the installation it acts on.

use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::installation::Installation;
use crate::installation::project::location::resolve_dir;
use crate::installation::user::location::data_dir;
use crate::installation::{AGENT_DIR, SKILLS_SUBDIR, project, user};

/// How a run was told to choose its installation.
#[derive(Debug, Clone, Default)]
pub(super) struct Selection {
    /// An explicit directory, which skips the search entirely.
    pub dir: Option<PathBuf>,
    /// Whether to act on the user-wide installation.
    pub user: bool,
    /// Whether to go past a refusal.
    pub force: bool,
}

/// Open the installation a selection names, reading the environment for rest.
///
/// # Errors
///
/// When the environment is unusable, the project directory cannot be chosen,
/// or a manifest is present but unreadable.
pub(super) fn resolve(selection: &Selection, version: &str) -> Result<Installation, String> {
    if let Some(dir) = &selection.dir {
        // `--dir` skips the bounded search, and skipped its refusal with it.
        // Copies where `--user` expects links record project scope, and the
        // scope check then refuses every later `--user` run, so one mistyped
        // path wedged the user-wide installation for good.
        if let Ok(home) = home_dir() {
            refuse_user_scope(dir, &home)?;
        }
        return Ok(Installation::Project(project::Installation::open(
            dir.clone(),
            selection.force,
        )?));
    }
    let home = home_dir()?;
    if !selection.user {
        let cwd = std::env::current_dir()
            .map_err(|error| format!("failed to read the working directory: {error}"))?;
        let root = repository_root(&cwd);
        let chosen = resolve_dir(&cwd, root.as_deref(), &home, &|path| path.exists())
            .map_err(|error| error.to_string())?;
        // `resolve_dir` compares the candidate as written, which a link
        // defeats: a repository carrying `.claude/skills -> ~/.claude/skills`
        // passed the bound and wrote the user-wide directory, reporting
        // project scope. Ask again about the place the answer resolves to.
        refuse_user_scope(&chosen, &home)?;
        return Ok(Installation::Project(project::Installation::open(
            chosen,
            selection.force,
        )?));
    }
    let data = data_dir(
        directory_from_env("FSL_DATA_DIR")?.as_deref(),
        directory_from_env("XDG_DATA_HOME")?.as_deref(),
        &home,
    );
    Ok(Installation::User(user::Installation::open(
        &home,
        &data,
        version,
        &binary_digest()?,
        selection.force,
    )?))
}

/// The home directory, which has to be one.
///
/// An empty `HOME` is `Some("")`, and joining onto it yields a path relative
/// to the working directory: the run then writes `.claude/skills` and a whole
/// payload wherever the user happened to be standing, and reports success
/// over eight links that resolve to nothing.
/// Refuse a directory that is the user-wide one by another name.
fn refuse_user_scope(dir: &Path, home: &Path) -> Result<(), String> {
    let user_wide = home.join(AGENT_DIR).join(SKILLS_SUBDIR);
    if resolved(dir) == resolved(&user_wide) {
        return Err(format!(
            "{} is the user-wide directory; use --user to install there",
            dir.display()
        ));
    }
    Ok(())
}

/// The place a path names, with `.` and `..` folded and links followed.
///
/// Neither half is enough alone. `canonicalize` fails on a path that is not
/// there yet, which is the ordinary case for a directory an install is about
/// to create, and comparing components folds `.` but not `..`. So fold the
/// path first, then resolve the deepest part of it that exists.
fn resolved(path: &Path) -> PathBuf {
    let folded = fold(path);
    let mut rest = Vec::new();
    let mut current = folded.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(current) {
            return rest.iter().rev().fold(real, |mut out, part| {
                out.push(part);
                out
            });
        }
        let (Some(name), Some(parent)) = (current.file_name(), current.parent()) else {
            return folded.clone();
        };
        rest.push(name.to_owned());
        current = parent;
    }
}

/// `path` with `.` dropped and `..` applied, without touching the filesystem.
fn fold(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn home_dir() -> Result<PathBuf, String> {
    // Windows sets `USERPROFILE`, not `HOME`. Reading only `HOME` made even a
    // project install fail there with "HOME is not set", although a project
    // install never touches the home directory except to refuse writing it.
    let home = match directory_from_env("HOME")? {
        Some(home) => home,
        None => directory_from_env("USERPROFILE")?
            .ok_or_else(|| "neither HOME nor USERPROFILE is set".to_owned())?,
    };
    // `current_dir` resolves symbolic links and `HOME` does not, so comparing
    // the two as written lets a symlinked home slip past the refusal in
    // `project::location::settle`: the walk lands on the real path, the guard
    // compares it against the link, and a project install quietly writes the
    // user-wide directory. Resolve it here so both sides name the same place.
    // A home that cannot be resolved is left as given, which is what the
    // refusal already compared against.
    Ok(std::fs::canonicalize(&home).unwrap_or(home))
}

/// A directory named by an environment variable, treating empty as unset.
///
/// The XDG specification says an empty value must be read as unset, and the
/// same reasoning applies to every path this reads: an empty one silently
/// becomes the working directory.
fn directory_from_env(name: &str) -> Result<Option<PathBuf>, String> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        // `install.sh:14-17` refuses a relative data directory. Accepting one
        // here writes links that are dead from every other directory, and the
        // two routes would disagree about the same variable.
        return Err(format!(
            "{name} is {}, which is not an absolute path",
            path.display()
        ));
    }
    Ok(Some(path))
}

/// The nearest ancestor holding a `.git`, starting at `from`.
///
/// This is what bounds the upward search for a `.claude` directory. Without a
/// bound the search reaches the home directory, which every Claude Code user
/// has, and a project install becomes a machine-wide one.
fn repository_root(from: &Path) -> Option<PathBuf> {
    let mut current = Some(from);
    while let Some(directory) = current {
        if directory.join(".git").exists() {
            return Some(directory.to_path_buf());
        }
        current = directory.parent();
    }
    None
}

/// The digest of the running executable, which names its release directory.
fn binary_digest() -> Result<String, String> {
    let path = std::env::current_exe()
        .map_err(|error| format!("failed to locate the running fslc: {error}"))?;
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("fslc-root-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("create");
        base
    }

    /// The bound on the upward walk. Without it the search reaches the home
    /// directory, which every Claude Code user has.
    #[test]
    fn the_repository_root_is_the_nearest_ancestor_holding_a_git_entry() {
        let base = temp_dir("nearest");
        let outer = base.join("outer");
        let inner = outer.join("packages/app");
        std::fs::create_dir_all(inner.join("src")).expect("create");
        std::fs::create_dir_all(outer.join(".git")).expect("create");

        assert_eq!(repository_root(&inner.join("src")), Some(outer.clone()));

        // A nested repository wins over the one containing it.
        std::fs::create_dir_all(inner.join(".git")).expect("create");
        assert_eq!(repository_root(&inner.join("src")), Some(inner));
    }

    /// A submodule or worktree has `.git` as a file, not a directory.
    #[test]
    fn a_git_file_counts_as_a_repository_root() {
        let base = temp_dir("git-file");
        let root = base.join("worktree");
        std::fs::create_dir_all(root.join("src")).expect("create");
        std::fs::write(root.join(".git"), "gitdir: /elsewhere\n").expect("write");

        assert_eq!(repository_root(&root.join("src")), Some(root));
    }

    #[test]
    fn nothing_is_reported_outside_a_repository() {
        let base = temp_dir("outside");
        std::fs::create_dir_all(base.join("plain")).expect("create");
        // The sandbox lives under the system temporary directory, which is not
        // inside a repository on any platform this runs on.
        assert_eq!(repository_root(&base.join("plain")), None);
    }
}
