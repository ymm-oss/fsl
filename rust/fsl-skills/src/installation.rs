// SPDX-License-Identifier: Apache-2.0

//! The two ways to install the skills, and which of them a run acts on.
//!
//! | Module | Holds |
//! | --- | --- |
//! | [`manifest`] | what a previous run recorded |
//! | [`plan`] | what a run would do, before it does any of it |
//! | [`project`] | a project's copies, and where they go |
//! | [`user`] | the machine's links, the payload, and `current` |

pub mod manifest;
pub mod plan;
pub mod project;
pub mod user;

use std::path::Path;

use manifest::{Manifest, Scope};
use plan::{Plan, PlanEntry};

/// The agent directory this tool looks for and writes into.
///
/// Claude Code only, for now. `README.md` documents the Claude Code skill
/// layout and no other, so a second built-in agent directory would be a
/// surface with nothing behind it. `--dir` reaches `.agents/skills`, or any
/// other location, in the meantime.
pub(crate) const AGENT_DIR: &str = ".claude";
/// The skills directory inside the agent directory.
pub(crate) const SKILLS_SUBDIR: &str = "skills";

/// What both scopes can do, so the parts that do not differ are written once.
///
/// `install` is deliberately absent. The two scopes report different things,
/// a project a file count and the machine a payload outcome, so sharing it
/// would mean flattening two JSON shapes into one.
pub(crate) trait Placement {
    /// What this scope plans over: files for a project, links for the machine.
    type Item: PlanEntry;

    /// Plan putting every embedded skill here.
    ///
    /// # Errors
    ///
    /// When a path exists but cannot be inspected.
    fn plan_install(&self) -> Result<Plan<Self::Item>, String>;

    /// Plan taking back what the manifest records.
    ///
    /// # Errors
    ///
    /// When a path exists but cannot be inspected.
    fn plan_uninstall(&self) -> Result<Plan<Self::Item>, String>;

    /// Carry out a removal plan, returning how much it took back.
    ///
    /// # Errors
    ///
    /// When a path or the manifest cannot be removed.
    fn uninstall(&self, plan: &Plan<Self::Item>) -> Result<usize, String>;
}

impl Placement for project::Installation {
    type Item = plan::PlannedFile;

    fn plan_install(&self) -> Result<Plan<Self::Item>, String> {
        Self::plan_install(self)
    }

    fn plan_uninstall(&self) -> Result<Plan<Self::Item>, String> {
        Self::plan_uninstall(self)
    }

    fn uninstall(&self, plan: &Plan<Self::Item>) -> Result<usize, String> {
        Self::uninstall(self, plan)
    }
}

impl Placement for user::Installation {
    type Item = plan::PlannedLink;

    fn plan_install(&self) -> Result<Plan<Self::Item>, String> {
        Self::plan_install(self)
    }

    fn plan_uninstall(&self) -> Result<Plan<Self::Item>, String> {
        Self::plan_uninstall(self)
    }

    fn uninstall(&self, plan: &Plan<Self::Item>) -> Result<usize, String> {
        Self::uninstall(self, plan)
    }
}

/// A project's copies, or the machine's links.
pub(crate) enum Installation {
    /// Copies in a project directory.
    Project(project::Installation),
    /// Links in the user-wide directory.
    User(user::Installation),
}

impl Installation {
    /// Where this installation keeps its skills.
    #[must_use]
    pub(crate) fn skills_dir(&self) -> &Path {
        match self {
            Self::Project(installation) => installation.skills_dir(),
            Self::User(installation) => installation.skills_dir(),
        }
    }

    /// Which scope this is.
    #[must_use]
    pub(crate) const fn scope(&self) -> Scope {
        match self {
            Self::Project(_) => Scope::Project,
            Self::User(_) => Scope::User,
        }
    }

    /// The manifest a previous run left here, if there is one.
    ///
    /// # Errors
    ///
    /// When a manifest is present but unreadable, or records the other scope.
    pub(crate) fn manifest(&self) -> Result<Option<Manifest>, String> {
        Manifest::read_for(self.skills_dir(), self.scope())
    }
}
