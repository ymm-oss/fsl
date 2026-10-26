// SPDX-License-Identifier: Apache-2.0

//! Command-line argument plumbing shared by the command parsers, and the
//! option types the verification commands read.

use std::path::PathBuf;

use serde_json::Value;

use crate::cli_output::semantic_error_output;

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
    pub(crate) shared: SharedOptions,
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
            shared: SharedOptions::default(),
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

/// `mutate`'s built-in mutant cap when `--max-mutants` is omitted. Must equal
/// the `max_mutants` default the published CLI contract advertises
/// (`rust/fslc/cli-contract.json`), which `docs/design/DESIGN-mutate.md`,
/// `skills/fsl/references/commands.md`, and the frozen `src/fslc/mutate.py`
/// (`DEFAULT_MAX_MUTANTS`) all fix at 200: a smaller runtime default silently
/// evaluates a different mutant set and reports a different kill rate
/// (issue #524).
pub(crate) const DEFAULT_MAX_MUTANTS: usize = 200;

/// Options every command that reads `--edition` carries as one value, so a
/// setting that applies to how a spec is read reaches each of them through
/// this struct rather than through each command's own option loop.
///
/// `edition` is the requested `--edition`; `None` is `current`, which reads a
/// document in the edition its header names.
#[derive(Clone, Debug, Default)]
pub(crate) struct SharedOptions {
    pub(crate) edition: Option<fsl_syntax::Edition>,
}

impl SharedOptions {
    /// Take `option` (and its value) when it is a shared option; `false`
    /// leaves it to the command's own arms.
    fn accept(
        &mut self,
        option: &str,
        args: &mut impl Iterator<Item = String>,
    ) -> Result<bool, String> {
        match option {
            "--edition" => self.edition = parse_edition(args)?,
            _ => return Ok(false),
        }
        Ok(true)
    }
}

pub(crate) fn parse_edition(
    args: &mut impl Iterator<Item = String>,
) -> Result<Option<fsl_syntax::Edition>, String> {
    fsl_syntax::Edition::parse_option(
        &required_option_value(args, "--edition")?,
        crate::process::parse_options(),
    )
}

fn parse_instance_override(raw: &str) -> Result<(String, i64), String> {
    let (name, value) = raw
        .split_once('=')
        .ok_or_else(|| format!("invalid --instances value '{raw}': expected NAME=N"))?;
    let value = value
        .parse::<i64>()
        .map_err(|_| format!("invalid --instances value '{raw}': N must be an integer"))?;
    if name.trim().is_empty() || value < 1 {
        return Err(format!(
            "invalid --instances value '{raw}': expected a non-empty NAME and N >= 1"
        ));
    }
    Ok((name.trim().to_owned(), value))
}

pub(crate) fn parse_sweep_range(raw: &str, flag: &str) -> Result<(String, (i64, i64)), String> {
    let (name, range) = raw
        .split_once('=')
        .ok_or_else(|| format!("invalid {flag} value '{raw}': expected NAME=LO..HI"))?;
    let (lo, hi) = range
        .split_once("..")
        .ok_or_else(|| format!("invalid {flag} value '{raw}': expected NAME=LO..HI"))?;
    let lo = lo
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("invalid {flag} value '{raw}': bounds must be integers"))?;
    let hi = hi
        .trim()
        .parse::<i64>()
        .map_err(|_| format!("invalid {flag} value '{raw}': bounds must be integers"))?;
    if name.trim().is_empty() || lo > hi {
        return Err(format!(
            "invalid {flag} value '{raw}': lower bound must be <= upper bound"
        ));
    }
    Ok((name.trim().to_owned(), (lo, hi)))
}

fn parse_depth(args: &mut impl Iterator<Item = String>) -> Result<usize, String> {
    args.next()
        .ok_or_else(|| "--depth requires a value".to_owned())?
        .parse()
        .map_err(|_| "--depth must be a non-negative integer".to_owned())
}

/// `--kernel-version`'s value, or the `semantics` envelope an unsupported
/// version is reported with as soon as it is parsed.
fn parse_kernel_version(
    args: &mut impl Iterator<Item = String>,
) -> Result<Result<fsl_core::PublicKernelVersion, (Value, i32)>, String> {
    let value = required_option_value(args, "--kernel-version")?;
    Ok(fsl_core::PublicKernelVersion::parse(&value)
        .map_err(|error| (semantic_error_output(&error.to_string()), 2)))
}

impl CliVerifyOptions {
    pub(crate) fn parse(
        args: &mut CommandArgs<impl Iterator<Item = String>>,
    ) -> Result<Self, String> {
        let mut options = Self::default();
        while let Some(option) = args.next() {
            match option.as_str() {
                "--depth" => {
                    options.depth = required_option_value(args, "--depth")?
                        .parse()
                        .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                }
                "--deadlock" => {
                    options.deadlock = required_option_value(args, "--deadlock")?;
                    if !matches!(options.deadlock.as_str(), "warn" | "error" | "ignore") {
                        return Err("--deadlock must be warn, error, or ignore".to_owned());
                    }
                }
                "--engine" => {
                    options.engine = required_option_value(args, "--engine")?;
                    if !matches!(
                        options.engine.as_str(),
                        "bmc" | "induction" | "explicit" | "auto"
                    ) {
                        return Err("--engine must be bmc, induction, explicit, or auto".to_owned());
                    }
                }
                "--explicit-budget" => {
                    options.explicit_budget = required_option_value(args, "--explicit-budget")?
                        .parse()
                        .map_err(|_| "--explicit-budget must be a positive integer".to_owned())?;
                    if options.explicit_budget == 0 {
                        return Err("--explicit-budget must be a positive integer".to_owned());
                    }
                }
                "--k" => {
                    options.k_ind = required_option_value(args, "--k")?
                        .parse()
                        .map_err(|_| "--k must be a positive integer".to_owned())?;
                    if options.k_ind == 0 {
                        return Err("--k must be a positive integer".to_owned());
                    }
                }
                "--vacuity" => {
                    options.vacuity = required_option_value(args, "--vacuity")?;
                    if !matches!(options.vacuity.as_str(), "warn" | "error" | "ignore") {
                        return Err("--vacuity must be warn, error, or ignore".to_owned());
                    }
                }
                "--property" => {
                    options.property = Some(required_option_value(args, "--property")?);
                }
                "--exclude-property" => options
                    .exclude_properties
                    .push(required_option_value(args, "--exclude-property")?),
                "--instances" => {
                    let raw = required_option_value(args, "--instances")?;
                    let (name, value) = parse_instance_override(&raw)?;
                    options.scope.instances.insert(name, value);
                }
                "--values" => {
                    let raw = required_option_value(args, "--values")?;
                    let (name, value) = parse_sweep_range(&raw, "--values")?;
                    options.scope.values.insert(name, value);
                }
                "--strict-tags" => options.strict_tags = true,
                "--requirements" => {
                    options.requirements = Some(PathBuf::from(required_option_value(
                        args,
                        "--requirements",
                    )?));
                }
                "--no-cache" => options.use_cache = false,
                "--lemma" => options.lemmas.push(required_option_value(args, "--lemma")?),
                "--from-state" => {
                    options.from_state =
                        Some(PathBuf::from(required_option_value(args, "--from-state")?));
                }
                _ if options.shared.accept(&option, args)? => {}
                _ => {
                    args.positional(option, |option| format!("unknown verify option '{option}'"))?;
                }
            }
        }
        Ok(options)
    }

    /// `scenarios` takes `verify`'s options but honours only `--depth` and
    /// `--deadlock`: every other option must still hold its default.
    pub(crate) fn only_depth_and_deadlock(&self) -> bool {
        self.engine == "bmc"
            && self.explicit_budget == DEFAULT_EXPLICIT_BUDGET
            && self.k_ind == 1
            && self.vacuity == "warn"
            && self.property.is_none()
            && self.exclude_properties.is_empty()
            && self.scope.instances.is_empty()
            && self.scope.values.is_empty()
            && !self.strict_tags
            && self.requirements.is_none()
            && self.use_cache
            && self.lemmas.is_empty()
            && self.from_state.is_none()
            && self.shared.edition.is_none()
    }
}

/// `fslc verify` / `fslc scenarios`.
pub(crate) struct VerifyCommandOptions {
    pub(crate) display_path: PathBuf,
    pub(crate) verify: CliVerifyOptions,
}

impl VerifyCommandOptions {
    pub(crate) fn parse(command: &str, args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let verify = CliVerifyOptions::parse(&mut args)?;
        let display_path = args.single_positional(
            || format!("usage: fslc {command} SPEC [options]"),
            |option| format!("unknown verify option '{option}'"),
        )?;
        Ok(Self {
            display_path,
            verify,
        })
    }
}

/// `fslc check`.
pub(crate) struct CheckOptions {
    pub(crate) display_path: PathBuf,
    pub(crate) strict_tags: bool,
    pub(crate) requirements: Option<PathBuf>,
    pub(crate) shared: SharedOptions,
    /// Every token after `check`, a `--` included; a causal model accepts
    /// only its path.
    pub(crate) argument_count: usize,
}

impl CheckOptions {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let arguments = args.collect::<Vec<_>>();
        let argument_count = arguments.len();
        let mut args = CommandArgs::new(arguments.into_iter());
        let mut strict_tags = false;
        let mut requirements = None;
        let mut shared = SharedOptions::default();
        while let Some(option) = args.next() {
            match option.as_str() {
                "--strict-tags" => strict_tags = true,
                "--requirements" => {
                    requirements = Some(PathBuf::from(required_option_value(
                        &mut args,
                        "--requirements",
                    )?));
                }
                _ if shared.accept(&option, &mut args)? => {}
                _ => {
                    args.positional(option, |option| format!("unknown check option '{option}'"))?;
                }
            }
        }
        let display_path = args.single_positional(
            || "usage: fslc check SPEC [options]".to_owned(),
            |argument| format!("unknown check option '{argument}'"),
        )?;
        Ok(Self {
            display_path,
            strict_tags,
            requirements,
            shared,
            argument_count,
        })
    }
}

/// `fslc kernel`.
pub(crate) struct KernelOptions {
    pub(crate) path: PathBuf,
    pub(crate) version: fsl_core::PublicKernelVersion,
}

impl KernelOptions {
    /// The options, or the envelope an unsupported `--kernel-version` ends
    /// the command with.
    pub(crate) fn parse(
        args: impl Iterator<Item = String>,
    ) -> Result<Result<Self, (Value, i32)>, String> {
        let mut args = CommandArgs::new(args);
        let mut version = fsl_core::PublicKernelVersion::V1;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--kernel-version" => match parse_kernel_version(&mut args)? {
                    Ok(parsed) => version = parsed,
                    Err(early_return) => return Ok(Err(early_return)),
                },
                _ => args.positional(argument, |option| {
                    format!("unknown kernel option '{option}'")
                })?,
            }
        }
        let path = args.single_positional(
            || "usage: fslc kernel SPEC [--kernel-version MAJOR]".to_owned(),
            |value| format!("unexpected kernel argument '{value}'"),
        )?;
        Ok(Ok(Self { path, version }))
    }
}

/// `fslc conformance`.
pub(crate) struct ConformanceOptions {
    pub(crate) path: PathBuf,
    pub(crate) depth: usize,
    pub(crate) version: fsl_core::PublicKernelVersion,
}

impl ConformanceOptions {
    /// The options, or the envelope an unsupported `--kernel-version` ends
    /// the command with.
    pub(crate) fn parse(
        args: impl Iterator<Item = String>,
    ) -> Result<Result<Self, (Value, i32)>, String> {
        let mut args = CommandArgs::new(args);
        let mut depth = 4_usize;
        let mut version = fsl_core::PublicKernelVersion::V1;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--depth" => {
                    depth = required_option_value(&mut args, "--depth")?
                        .parse()
                        .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                }
                "--kernel-version" => match parse_kernel_version(&mut args)? {
                    Ok(parsed) => version = parsed,
                    Err(early_return) => return Ok(Err(early_return)),
                },
                _ => args.positional(argument, |option| {
                    format!("unknown conformance option '{option}'")
                })?,
            }
        }
        let path = args.single_positional(
            || "usage: fslc conformance SPEC [--depth N] [--kernel-version MAJOR]".to_owned(),
            |value| format!("unexpected conformance argument '{value}'"),
        )?;
        Ok(Ok(Self {
            path,
            depth,
            version,
        }))
    }
}

/// `fslc testplan`.
pub(crate) struct TestplanOptions {
    pub(crate) path: PathBuf,
    pub(crate) depth: usize,
}

impl TestplanOptions {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut depth = 4_usize;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--depth" => {
                    depth = required_option_value(&mut args, "--depth")?
                        .parse()
                        .map_err(|_| "--depth must be a non-negative integer".to_owned())?;
                }
                _ => args.positional(argument, |option| {
                    format!("unknown testplan option '{option}'")
                })?,
            }
        }
        let path = args.single_positional(
            || "usage: fslc testplan SPEC [--depth N]".to_owned(),
            |value| format!("unexpected testplan argument '{value}'"),
        )?;
        Ok(Self { path, depth })
    }
}

/// `fslc compat check`.
pub(crate) struct CompatCheckOptions {
    pub(crate) path: PathBuf,
    pub(crate) include_ai: bool,
}

impl CompatCheckOptions {
    /// `args` starts at the subcommand (`check`).
    pub(crate) fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        if args.next().as_deref() != Some("check") {
            return Err("usage: fslc compat check DBSYSTEM [--include-ai]".to_owned());
        }
        let mut args = CommandArgs::new(args);
        let mut include_ai = false;
        while let Some(option) = args.next() {
            match option.as_str() {
                "--include-ai" => include_ai = true,
                _ => args.positional(option, |option| {
                    format!("unknown compat check option '{option}'")
                })?,
            }
        }
        let path = args.single_positional(
            || "compat check requires a dbsystem".to_owned(),
            |option| format!("unknown compat check option '{option}'"),
        )?;
        Ok(Self { path, include_ai })
    }
}

/// `fslc explain`, `fslc mutate` and `fslc typestate`, which share one
/// option set; the mutation gate flags are `mutate`'s alone.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct MutationFamilyOptions {
    pub(crate) path: PathBuf,
    pub(crate) depth: usize,
    pub(crate) readable: bool,
    pub(crate) max_mutants: usize,
    pub(crate) by_requirement: bool,
    pub(crate) oracle_attribution: bool,
    pub(crate) typescript_only: bool,
    pub(crate) external_mutants: Option<PathBuf>,
    pub(crate) fail_on_survivors: bool,
    pub(crate) min_kill_rate: Option<f64>,
    /// `explain --names`, read only behind the next edition's gate.
    pub(crate) names: Option<crate::name_display::NameSpelling>,
    /// `explain --lowering`, read only behind the next edition's gate.
    pub(crate) lowering: bool,
}

impl MutationFamilyOptions {
    pub(crate) fn parse(command: &str, args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut depth = 8_usize;
        let mut readable = false;
        let mut max_mutants = DEFAULT_MAX_MUTANTS;
        let mut by_requirement = false;
        let mut oracle_attribution = false;
        let mut typescript_only = false;
        let mut external_mutants = None;
        let mut fail_on_survivors = false;
        let mut min_kill_rate = None;
        let mut names = None;
        let mut lowering = false;
        let next_edition = || {
            crate::process::parse_options()
                .unstable()
                .contains(fsl_syntax::UnstableFeature::Fsl5)
        };
        while let Some(option) = args.next() {
            match option.as_str() {
                "--lowering" if command == "explain" && next_edition() => lowering = true,
                "--names" if command == "explain" && next_edition() => {
                    names = Some(crate::name_display::NameSpelling::parse(
                        &required_option_value(&mut args, "--names")?,
                    )?);
                }
                // Issue #1237: the opt-in mutation gate belongs to `mutate`
                // alone; the other two commands sharing this loop reject it
                // instead of silently accepting a flag that cannot apply.
                "--fail-on-survivors" | "--min-kill-rate" if command != "mutate" => {
                    return Err(format!("unknown {command} option '{option}'"));
                }
                "--fail-on-survivors" => fail_on_survivors = true,
                "--min-kill-rate" => {
                    let rate = args
                        .next()
                        .ok_or_else(|| "--min-kill-rate requires a value".to_owned())?
                        .parse::<f64>()
                        .ok()
                        .filter(|rate| rate.is_finite() && (0.0..=1.0).contains(rate))
                        .ok_or_else(|| {
                            "--min-kill-rate must be a number between 0 and 1".to_owned()
                        })?;
                    min_kill_rate = Some(rate);
                }
                "--depth" => depth = parse_depth(&mut args)?,
                "--readable" => readable = true,
                "--max-mutants" => {
                    max_mutants = args
                        .next()
                        .ok_or_else(|| "--max-mutants requires a value".to_owned())?
                        .parse()
                        .map_err(|_| "--max-mutants must be an integer".to_owned())?;
                }
                "--by-requirement" => by_requirement = true,
                "--oracle-attribution" => oracle_attribution = true,
                "--from" => {
                    external_mutants = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| "--from requires a JSONL path".to_owned())?,
                    ));
                }
                "--ts" => typescript_only = true,
                _ => args.positional(option, |option| {
                    format!("unknown {command} option '{option}'")
                })?,
            }
        }
        let path = args.single_positional(
            || format!("fslc {command} requires a spec"),
            |option| format!("unknown {command} option '{option}'"),
        )?;
        if lowering && readable {
            return Err("explain --lowering prints JSON; drop --readable".to_owned());
        }
        Ok(Self {
            path,
            depth,
            readable,
            max_mutants,
            by_requirement,
            oracle_attribution,
            typescript_only,
            external_mutants,
            fail_on_survivors,
            min_kill_rate,
            names,
            lowering,
        })
    }
}

/// `fslc testgen`, `fslc html` and `fslc ledger`, which share one option
/// set; `--allow-unwired` is `testgen`'s, `--no-timing` is `html`'s,
/// and the evidence inputs are `ledger`'s.
pub(crate) struct ReportOptions {
    pub(crate) path: PathBuf,
    pub(crate) depth: usize,
    pub(crate) output: Option<PathBuf>,
    pub(crate) target: String,
    pub(crate) unwired: fsl_tools::UnwiredAdapter,
    pub(crate) engine: String,
    pub(crate) impl_log: Option<PathBuf>,
    pub(crate) evidence: Vec<PathBuf>,
    pub(crate) approval_records: Vec<PathBuf>,
    pub(crate) trust_keys: Vec<PathBuf>,
    pub(crate) deadlock: String,
    pub(crate) strict: bool,
    pub(crate) no_timing: bool,
}

impl ReportOptions {
    pub(crate) fn parse(command: &str, args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut depth = 8_usize;
        let mut output = None;
        let mut target = "pytest".to_owned();
        let mut unwired = fsl_tools::UnwiredAdapter::Fail;
        let mut engine = "bmc".to_owned();
        let mut impl_log = None;
        let mut evidence = Vec::new();
        let mut approval_records = Vec::new();
        let mut trust_keys = Vec::new();
        let mut deadlock = if command == "ledger" {
            "ignore".to_owned()
        } else {
            "warn".to_owned()
        };
        let mut strict = false;
        let mut no_timing = false;
        while let Some(option) = args.next() {
            match option.as_str() {
                "--depth" => depth = parse_depth(&mut args)?,
                "-o" | "--output" => {
                    output = Some(PathBuf::from(
                        args.next()
                            .ok_or_else(|| "--output requires a path".to_owned())?,
                    ));
                }
                "--target" => {
                    target = args
                        .next()
                        .ok_or_else(|| "--target requires a value".to_owned())?;
                }
                "--engine" => {
                    engine = args
                        .next()
                        .ok_or_else(|| "--engine requires a value".to_owned())?;
                    if !matches!(engine.as_str(), "bmc" | "induction") {
                        return Err("--engine must be bmc or induction".to_owned());
                    }
                }
                "--deadlock" => {
                    deadlock = args
                        .next()
                        .ok_or_else(|| "--deadlock requires a value".to_owned())?;
                    if !matches!(deadlock.as_str(), "warn" | "error" | "ignore") {
                        return Err("--deadlock must be warn, error, or ignore".to_owned());
                    }
                }
                "--impl-log" if command == "ledger" => {
                    impl_log = Some(PathBuf::from(required_option_value(
                        &mut args,
                        "--impl-log",
                    )?));
                }
                "--evidence" if command == "ledger" => evidence.push(PathBuf::from(
                    required_option_value(&mut args, "--evidence")?,
                )),
                "--approval" if command == "ledger" => approval_records.push(PathBuf::from(
                    required_option_value(&mut args, "--approval")?,
                )),
                "--trust-key" if command == "ledger" => trust_keys.push(PathBuf::from(
                    required_option_value(&mut args, "--trust-key")?,
                )),
                "--strict" => strict = true,
                "--no-timing" if command == "html" => no_timing = true,
                "--allow-unwired" if command == "testgen" => {
                    unwired = fsl_tools::UnwiredAdapter::Skip;
                }
                _ => args.positional(option, |option| {
                    format!("unknown {command} option '{option}'")
                })?,
            }
        }
        let path = args.single_positional(
            || format!("fslc {command} requires a spec"),
            |option| format!("unknown {command} option '{option}'"),
        )?;
        Ok(Self {
            path,
            depth,
            output,
            target,
            unwired,
            engine,
            impl_log,
            evidence,
            approval_records,
            trust_keys,
            deadlock,
            strict,
            no_timing,
        })
    }
}

/// `fslc analyze`.
pub(crate) struct AnalyzeOptions {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) projection: String,
    pub(crate) focus: Option<String>,
    pub(crate) output_format: String,
    pub(crate) profile: Option<String>,
    pub(crate) export_kind: Option<String>,
    pub(crate) code: Option<PathBuf>,
}

impl AnalyzeOptions {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut projection = "tsg".to_owned();
        let mut focus = None;
        let mut output_format = "json".to_owned();
        let mut profile = None;
        let mut export_kind = None;
        let mut code = None;
        while let Some(option) = args.next() {
            match option.as_str() {
                "--projection" => {
                    projection = args
                        .next()
                        .ok_or_else(|| "--projection requires a value".to_owned())?;
                }
                "--focus" => focus = Some(required_option_value(&mut args, "--focus")?),
                "--format" => {
                    output_format = args
                        .next()
                        .ok_or_else(|| "--format requires a value".to_owned())?;
                }
                "--profile" => profile = Some(required_option_value(&mut args, "--profile")?),
                "--export" => {
                    export_kind = Some(required_option_value(&mut args, "--export")?);
                }
                "--code" => {
                    code = Some(PathBuf::from(required_option_value(&mut args, "--code")?));
                }
                _ => args.positional(option, |option| {
                    format!("unknown analyze option '{option}'")
                })?,
            }
        }
        let paths = args
            .into_positionals()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return Err("fslc analyze requires a spec".to_owned());
        }
        Ok(Self {
            paths,
            projection,
            focus,
            output_format,
            profile,
            export_kind,
            code,
        })
    }
}

/// `fslc diff`. Whether the paths fit `OLD NEW` or `--git` is decided by the
/// command after each path passes its literate-access gate.
pub(crate) struct DiffOptions {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) depth: usize,
    pub(crate) git_range: Option<String>,
    pub(crate) mapping: Option<PathBuf>,
    pub(crate) forbid: Vec<String>,
}

impl DiffOptions {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut depth = 8_usize;
        let mut git_range = None;
        let mut mapping = None;
        let mut forbid = Vec::new();
        while let Some(option) = args.next() {
            match option.as_str() {
                "--depth" => {
                    depth = args
                        .next()
                        .ok_or_else(|| "--depth requires a value".to_owned())?
                        .parse()
                        .map_err(|_| "--depth must be an integer".to_owned())?;
                }
                "--git" => {
                    git_range = Some(required_option_value(&mut args, "--git")?);
                }
                "--mapping" => {
                    mapping = Some(PathBuf::from(required_option_value(
                        &mut args,
                        "--mapping",
                    )?));
                }
                "--forbid" => {
                    forbid.extend(
                        required_option_value(&mut args, "--forbid")?
                            .split(',')
                            .map(str::trim)
                            .filter(|item| !item.is_empty())
                            .map(str::to_owned),
                    );
                }
                _ => {
                    args.positional(option, |option| format!("unknown diff option '{option}'"))?;
                }
            }
        }
        let paths = args
            .into_positionals()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        Ok(Self {
            paths,
            depth,
            git_range,
            mapping,
            forbid,
        })
    }
}

/// `fslc chain`.
pub(crate) struct ChainOptions {
    pub(crate) path: PathBuf,
    pub(crate) keep_going: bool,
    pub(crate) jobs: usize,
}

impl ChainOptions {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut keep_going = false;
        let mut jobs = 1_usize;
        while let Some(option) = args.next() {
            match option.as_str() {
                "--keep-going" => keep_going = true,
                "--jobs" => {
                    jobs = args
                        .next()
                        .ok_or_else(|| "--jobs requires a value".to_owned())?
                        .parse()
                        .ok()
                        .filter(|jobs| *jobs > 0)
                        .ok_or_else(|| "--jobs must be a positive integer".to_owned())?;
                }
                _ => {
                    args.positional(option, |option| format!("unknown chain option '{option}'"))?;
                }
            }
        }
        let mut positionals = args.into_positionals();
        let path = PathBuf::from(
            positionals
                .next()
                .unwrap_or_else(|| "fsl-project.toml".to_owned()),
        );
        if let Some(argument) = positionals.next() {
            return Err(format!("unexpected chain argument '{argument}'"));
        }
        Ok(Self {
            path,
            keep_going,
            jobs,
        })
    }
}

/// `fslc refine IMPL ABS MAPPING [ABS MAPPING]...`.
pub(crate) struct RefineOptions {
    pub(crate) path: PathBuf,
    pub(crate) abstraction: PathBuf,
    pub(crate) mapping: PathBuf,
    /// The `(abs map)` operands after the first mapping, unpaired ones
    /// included: the command reports an odd count.
    pub(crate) rest: Vec<PathBuf>,
    pub(crate) depth: usize,
}

impl RefineOptions {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut depth = 8_usize;
        while let Some(option) = args.next() {
            match option.as_str() {
                "--depth" => depth = parse_depth(&mut args)?,
                _ => {
                    args.positional(option, |option| format!("unknown refine option '{option}'"))?;
                }
            }
        }
        let mut positionals = args.into_positionals().map(PathBuf::from);
        let mut operand = || {
            positionals
                .next()
                .ok_or_else(|| "refine requires IMPL ABS MAPPING".to_owned())
        };
        let path = operand()?;
        let abstraction = operand()?;
        let mapping = operand()?;
        let rest = positionals.collect::<Vec<_>>();
        Ok(Self {
            path,
            abstraction,
            mapping,
            rest,
            depth,
        })
    }
}

/// `fslc replay`. Which of `--trace` / `--from-log` / `--mapping` may be
/// combined is decided by the command after the spec passes its
/// literate-access gate.
pub(crate) struct ReplayOptions {
    pub(crate) path: PathBuf,
    pub(crate) trace: Option<PathBuf>,
    pub(crate) from_log: Option<PathBuf>,
    pub(crate) mapping: Option<PathBuf>,
}

impl ReplayOptions {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut trace = None;
        let mut from_log = None;
        let mut mapping = None;
        while let Some(option) = args.next() {
            match option.as_str() {
                "--trace" => {
                    trace = Some(PathBuf::from(required_option_value(&mut args, "--trace")?));
                }
                "--from-log" => {
                    from_log = Some(PathBuf::from(required_option_value(
                        &mut args,
                        "--from-log",
                    )?));
                }
                "--mapping" => {
                    mapping = Some(PathBuf::from(required_option_value(
                        &mut args,
                        "--mapping",
                    )?));
                }
                _ => {
                    args.positional(option, |option| format!("unknown replay option '{option}'"))?;
                }
            }
        }
        let path = args.single_positional(
            || "usage: fslc replay SPEC --trace TRACE.json".to_owned(),
            |option| format!("unknown replay option '{option}'"),
        )?;
        Ok(Self {
            path,
            trace,
            from_log,
            mapping,
        })
    }
}

/// `fslc sweep`. `--depth`, `--instances` and `--values` stay raw: the
/// sweep reports a malformed range in its own envelope.
pub(crate) struct SweepOptions {
    pub(crate) path: PathBuf,
    pub(crate) depth: String,
    pub(crate) deadlock: String,
    pub(crate) engine: String,
    pub(crate) k_ind: usize,
    pub(crate) vacuity: String,
    pub(crate) property: Option<String>,
    pub(crate) strict_tags: bool,
    pub(crate) requirements: Option<PathBuf>,
    pub(crate) instances: Vec<String>,
    pub(crate) values: Vec<String>,
}

impl SweepOptions {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = CommandArgs::new(args);
        let mut depth = "0..8".to_owned();
        let mut deadlock = "warn".to_owned();
        let mut engine = "bmc".to_owned();
        let mut k_ind = 1_usize;
        let mut vacuity = "warn".to_owned();
        let mut property = None;
        let mut strict_tags = false;
        let mut requirements = None;
        let mut instances = Vec::new();
        let mut values = Vec::new();
        while let Some(option) = args.next() {
            match option.as_str() {
                "--depth" => {
                    depth = args
                        .next()
                        .ok_or_else(|| "--depth requires a value".to_owned())?;
                }
                "--deadlock" => {
                    deadlock = args
                        .next()
                        .ok_or_else(|| "--deadlock requires a value".to_owned())?;
                    if !matches!(deadlock.as_str(), "warn" | "error" | "ignore") {
                        return Err("--deadlock must be warn, error, or ignore".to_owned());
                    }
                }
                "--engine" => {
                    engine = args
                        .next()
                        .ok_or_else(|| "--engine requires a value".to_owned())?;
                    if !matches!(engine.as_str(), "bmc" | "induction") {
                        return Err("--engine must be bmc or induction".to_owned());
                    }
                }
                "--k" => {
                    k_ind = args
                        .next()
                        .ok_or_else(|| "--k requires a value".to_owned())?
                        .parse()
                        .map_err(|_| "--k must be a positive integer".to_owned())?;
                    if k_ind == 0 {
                        return Err("--k must be a positive integer".to_owned());
                    }
                }
                "--instances" => instances.push(
                    args.next()
                        .ok_or_else(|| "--instances requires a value".to_owned())?,
                ),
                "--values" => values.push(
                    args.next()
                        .ok_or_else(|| "--values requires a value".to_owned())?,
                ),
                "--property" => {
                    property = Some(required_option_value(&mut args, "--property")?);
                }
                "--vacuity" => {
                    vacuity = required_option_value(&mut args, "--vacuity")?;
                    if !matches!(vacuity.as_str(), "warn" | "error" | "ignore") {
                        return Err("--vacuity must be warn, error, or ignore".to_owned());
                    }
                }
                "--strict-tags" => strict_tags = true,
                "--requirements" => {
                    requirements = Some(PathBuf::from(required_option_value(
                        &mut args,
                        "--requirements",
                    )?));
                }
                _ => {
                    args.positional(option, |option| format!("unknown sweep option '{option}'"))?;
                }
            }
        }
        let path = args.single_positional(
            || "usage: fslc sweep SPEC [options]".to_owned(),
            |option| format!("unknown sweep option '{option}'"),
        )?;
        Ok(Self {
            path,
            depth,
            deadlock,
            engine,
            k_ind,
            vacuity,
            property,
            strict_tags,
            requirements,
            instances,
            values,
        })
    }
}
