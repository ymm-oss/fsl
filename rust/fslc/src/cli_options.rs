// SPDX-License-Identifier: Apache-2.0

//! Command-line argument plumbing shared by the command parsers, and the
//! option types the verification commands read.

use std::path::PathBuf;

pub(crate) const DEFAULT_EXPLICIT_BUDGET: usize = 1_000_000;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ScopeBounds {
    pub(crate) instances: std::collections::BTreeMap<String, i64>,
    pub(crate) values: std::collections::BTreeMap<String, (i64, i64)>,
}

#[derive(Clone, Debug)]
pub(crate) struct CliVerifyOptions {
    pub(crate) depth: usize,
    pub(crate) deadlock: String,
    pub(crate) engine: String,
    pub(crate) explicit_budget: usize,
    pub(crate) k_ind: usize,
    pub(crate) vacuity: String,
    pub(crate) property: Option<String>,
    pub(crate) exclude_properties: Vec<String>,
    pub(crate) scope: ScopeBounds,
    pub(crate) strict_tags: bool,
    pub(crate) requirements: Option<PathBuf>,
    pub(crate) use_cache: bool,
    pub(crate) lemmas: Vec<String>,
    pub(crate) from_state: Option<PathBuf>,
    pub(crate) edition: String,
}

impl Default for CliVerifyOptions {
    fn default() -> Self {
        Self {
            depth: 8,
            deadlock: "warn".to_owned(),
            engine: "bmc".to_owned(),
            explicit_budget: DEFAULT_EXPLICIT_BUDGET,
            k_ind: 1,
            vacuity: "warn".to_owned(),
            property: None,
            exclude_properties: Vec::new(),
            scope: ScopeBounds::default(),
            strict_tags: false,
            requirements: None,
            use_cache: true,
            lemmas: Vec::new(),
            from_state: None,
            edition: "current".to_owned(),
        }
    }
}

pub(crate) fn required_option_value(
    args: &mut impl Iterator<Item = String>,
    option: &str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

/// One subcommand's arguments, with options and positionals in any order
/// (#1250, #1301).
///
/// Every usage line in `cli-contract.json` lists options before positionals,
/// and the frozen Python CLI (argparse) accepts either order, so a native
/// command must too. The command's own `match` arms stay the only owner of
/// each option's arity: a value-taking arm pulls its value with
/// `Iterator::next` (`--depth 3` never turns `3` into a positional, and
/// `--depth` with nothing after it stays `--depth requires a value`). Every
/// token the arms do not recognise goes through [`CommandArgs::positional`] —
/// the shape `parse_migration_options` already had — so `-` (stdin) and any
/// token not starting with `-` is a positional, while any other `-`-prefixed
/// token stays that command's unknown-option usage error. A bare `--` ends
/// option parsing as in argparse: every later token is a positional, which is
/// how a path that starts with `-` is passed.
pub(crate) struct CommandArgs<I: Iterator<Item = String>> {
    args: std::iter::Fuse<I>,
    positionals: Vec<String>,
}

impl<I: Iterator<Item = String>> CommandArgs<I> {
    pub(crate) fn new(args: I) -> Self {
        Self {
            args: args.fuse(),
            positionals: Vec::new(),
        }
    }

    /// Keep `token` as a positional, or reject it with the command's
    /// unknown-option message.
    pub(crate) fn positional(
        &mut self,
        token: String,
        unknown: impl FnOnce(&str) -> String,
    ) -> Result<(), String> {
        if token == "-" || !token.starts_with('-') {
            self.positionals.push(token);
            Ok(())
        } else {
            Err(unknown(&token))
        }
    }

    /// The positionals in command-line order (those after `--` last).
    pub(crate) fn into_positionals(mut self) -> std::vec::IntoIter<String> {
        self.positionals.extend(self.args);
        self.positionals.into_iter()
    }

    /// The single positional of a one-spec command. A missing one keeps the
    /// command's own usage message; a second one is reported the way the
    /// command reported a stray argument before #1250.
    pub(crate) fn single_positional(
        self,
        missing: impl FnOnce() -> String,
        extra: impl FnOnce(&str) -> String,
    ) -> Result<PathBuf, String> {
        let mut positionals = self.into_positionals();
        let path = positionals.next().ok_or_else(missing)?;
        if let Some(argument) = positionals.next() {
            return Err(extra(&argument));
        }
        Ok(PathBuf::from(path))
    }
}

impl<I: Iterator<Item = String>> Iterator for CommandArgs<I> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        let token = self.args.next()?;
        if token == "--" {
            self.positionals.extend(self.args.by_ref());
            return None;
        }
        Some(token)
    }
}
