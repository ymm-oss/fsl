// SPDX-License-Identifier: Apache-2.0

//! What an install or a removal would do, before it does any of it.

use serde::Serialize;

/// What is at one path, relative to what a previous install recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// Nothing is at the path.
    Absent,
    /// The manifest records the path and the bytes still match what was
    /// written there. Ours, untouched, so replacing it loses nothing.
    Managed,
    /// The manifest records the path and the bytes have changed since. Ours,
    /// edited by hand, so an install refuses it instead of discarding work.
    Modified,
    /// A file is at the path and no manifest of ours records it. Somebody
    /// else's, sitting where a skill would go. Never written over, never
    /// removed, only reported, because this tool did not put it there.
    Foreign,
}

/// What is at one link path in the user-wide skills directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinkState {
    /// Nothing is at the path.
    Absent,
    /// A link the manifest records, still pointing where it was recorded to
    /// point.
    Managed,
    /// A link the manifest records, now pointing somewhere else.
    Diverged,
    /// A link pointing where it should, at something that is not there.
    ///
    /// The payload it reaches through `current` was removed or never written.
    /// The link reads as correct and resolves to nothing, so reporting it as
    /// settled would call a broken install up to date.
    Broken,
    /// A link no manifest of ours records. Somebody else's, so it is
    /// reported rather than replaced.
    Foreign,
    /// A real directory or file, not a link at all.
    ///
    /// Reported apart from [`Self::Foreign`] because what `--force` does to
    /// it differs in kind. Replacing a link removes one entry. Replacing a
    /// directory removes it and everything under it, which may be work the
    /// user did by hand or a skill they installed with another tool.
    Occupied,
}

/// What an install intends to do with one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Write the embedded contents, or place the link.
    Write,
    /// Already what it should be. Leave it.
    Keep,
    /// Not ours. Leave it and say so.
    Skip,
    /// Ours and changed. Refuse without `--force`.
    Blocked,
    /// Ours. Take it back.
    Remove,
}

/// One file's state and the action chosen for it.
#[derive(Debug, Clone, Serialize)]
pub struct PlannedFile {
    /// Path relative to the skills directory.
    pub path: String,
    /// What is on disk now.
    pub state: EntryState,
    /// What will happen to it.
    pub action: Action,
}

/// One link's state and the action chosen for it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PlannedLink {
    /// The skill name, which is also the link's file name.
    pub skill: String,
    /// Where the link should point.
    pub target: String,
    /// What is there now.
    pub state: LinkState,
    /// What will happen to it.
    pub action: Action,
}

/// The parts of a planned item a [`Plan`] needs to summarize it.
pub trait PlanEntry {
    /// What will happen to this item.
    fn action(&self) -> Action;
    /// What to call it in a refusal.
    fn name(&self) -> &str;
}

impl PlanEntry for PlannedFile {
    fn action(&self) -> Action {
        self.action
    }
    fn name(&self) -> &str {
        &self.path
    }
}

impl PlanEntry for PlannedLink {
    fn action(&self) -> Action {
        self.action
    }
    fn name(&self) -> &str {
        &self.skill
    }
}

/// What an install or a removal would do, before it does any of it.
///
/// Planning never writes, so `--dry-run` and the refusal check run the same
/// code the real thing does.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct Plan<T> {
    entries: Vec<T>,
}

/// A plan over the files of a project install.
pub type FilePlan = Plan<PlannedFile>;
/// A plan over the links of a user-wide install.
pub(crate) type LinkPlan = Plan<PlannedLink>;

impl<T> Plan<T> {
    /// A plan over these items, in the order they will be handled.
    ///
    /// The field stays private so a plan cannot be edited after it is made:
    /// what `--dry-run` reports and what the run does are the same list.
    pub(super) const fn of(entries: Vec<T>) -> Self {
        Self { entries }
    }

    /// A plan with nothing in it, for when there is no manifest to undo.
    pub(super) const fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<T: PlanEntry> Plan<T> {
    /// The planned items, in the order they will be handled.
    #[must_use]
    pub fn entries(&self) -> &[T] {
        &self.entries
    }

    /// The items this plan refuses to touch without `--force`.
    #[must_use]
    pub fn blocked(&self) -> Vec<&str> {
        self.with_action(Action::Blocked)
    }

    /// How many items this plan would write.
    #[must_use]
    pub fn writes(&self) -> usize {
        self.count(Action::Write)
    }

    /// How many items this plan would remove.
    #[must_use]
    pub fn removals(&self) -> usize {
        self.count(Action::Remove)
    }

    /// Whether every item is already what it should be.
    ///
    /// Only [`Action::Keep`] settles. A skipped item is somebody else's, which
    /// means ours is not there: the set is incomplete, and saying otherwise
    /// would report an install that never happened as up to date.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.action() == Action::Keep)
    }

    fn count(&self, action: Action) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.action() == action)
            .count()
    }

    fn with_action(&self, action: Action) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|entry| entry.action() == action)
            .map(PlanEntry::name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, action: Action) -> PlannedFile {
        PlannedFile {
            path: path.to_owned(),
            state: EntryState::Absent,
            action,
        }
    }

    fn plan(actions: &[(&str, Action)]) -> FilePlan {
        Plan::of(
            actions
                .iter()
                .map(|(path, action)| entry(path, *action))
                .collect(),
        )
    }

    #[test]
    fn counts_answer_only_for_their_own_action() {
        let plan = plan(&[
            ("a", Action::Write),
            ("b", Action::Write),
            ("c", Action::Keep),
            ("d", Action::Remove),
            ("e", Action::Skip),
        ]);
        assert_eq!(plan.writes(), 2);
        assert_eq!(plan.removals(), 1);
        assert_eq!(plan.entries().len(), 5);
    }

    #[test]
    fn blocked_names_every_refusal_and_nothing_else() {
        let plan = plan(&[
            ("a", Action::Blocked),
            ("b", Action::Keep),
            ("c", Action::Blocked),
        ]);
        assert_eq!(plan.blocked(), vec!["a", "c"]);
    }

    /// A skip means ours is not there, so the set is not settled.
    #[test]
    fn only_keep_settles() {
        assert!(plan(&[("a", Action::Keep)]).is_settled());
        assert!(!plan(&[("a", Action::Keep), ("b", Action::Skip)]).is_settled());
        assert!(!plan(&[("a", Action::Write)]).is_settled());
        assert!(!plan(&[("a", Action::Blocked)]).is_settled());
        assert!(
            Plan::<PlannedFile>::empty().is_settled(),
            "nothing planned is vacuously settled"
        );
    }

    #[test]
    fn a_plan_serializes_as_its_entries() {
        let rendered = serde_json::to_value(plan(&[("a", Action::Write)])).expect("serialize");
        let entries = rendered.as_array().expect("an array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["path"], "a");
        assert_eq!(entries[0]["action"], "write");
        assert_eq!(entries[0]["state"], "absent");
    }
}
