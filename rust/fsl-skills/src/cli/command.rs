// SPDX-License-Identifier: Apache-2.0

//! `fslc skills`: put the embedded Agent Skills on disk, report, take them back.
//!
//! Argument reading and dispatch. The response shape lives in [`super::report`],
//! and the placement rules in [`crate::installation::project`] and
//! [`crate::installation::user`].

use serde_json::Value;

use crate::embedded::embedded_skill_names;
use crate::installation::manifest::Manifest;
use crate::installation::{Installation, Placement};
use crate::installation::{project, user};

use crate::installation::plan::{Plan, PlanEntry};

use super::report::{
    Header, Listable, NotInstalled, ProjectInstall, Refusal, Removal, Status, UserInstall, Verdict,
    io_failure, respond,
};
use super::selection::{Selection, resolve};

const USAGE: &str = "usage: fslc skills <install|status|uninstall> \
                     [--dir <path>] [--user] [--force] [--dry-run]";

/// Everything a run was asked to do.
#[derive(Debug, Clone, Default)]
struct Options {
    /// Which installation to act on.
    selection: Selection,
    /// Whether to plan without writing.
    dry_run: bool,
}

/// Run `fslc skills`.
///
/// # Errors
///
/// When the arguments are unusable, the target cannot be chosen, or the
/// filesystem refuses an operation.
pub fn run(mut args: impl Iterator<Item = String>, version: &str) -> Result<(Value, i32), String> {
    let subcommand = args.next().ok_or_else(|| USAGE.to_owned())?;
    // Name the subcommand before reading its options, the way every sibling
    // does: `fslc causal bogus --bogus` reports the subcommand, not the flag.
    if !matches!(subcommand.as_str(), "install" | "status" | "uninstall") {
        return Err(format!("unknown skills subcommand '{subcommand}'. {USAGE}"));
    }
    let options = parse(&mut args)?;
    // `status` reports; it never writes. Accepting flags the help and the
    // published contract do not declare makes the contract wrong for that leaf.
    if subcommand == "status" {
        for (given, flag) in [
            (options.selection.force, "--force"),
            (options.dry_run, "--dry-run"),
        ] {
            if given {
                return Err(format!("fslc skills status does not take {flag}"));
            }
        }
    }
    // Everything past argument reading touches the filesystem, and a fault
    // there is not a usage error. Report it the way every other command does.
    let installation = match resolve(&options.selection, version) {
        Ok(installation) => installation,
        Err(error) => return Ok(io_failure(&error)),
    };
    let header = Header::new(&subcommand, &installation, version, options.dry_run);
    let acted = match subcommand.as_str() {
        "install" => install(&installation, header, &options, version),
        "status" => status(&installation, header, version),
        _ => uninstall(&installation, header, &options),
    };
    Ok(acted.unwrap_or_else(|error| io_failure(&error)))
}

fn parse(args: &mut impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--dir" => {
                let value = args
                    .next()
                    .ok_or_else(|| "fslc skills --dir requires a path".to_owned())?;
                if value.is_empty() {
                    // An empty path is the working directory, which nobody
                    // means to name this way.
                    return Err("fslc skills --dir requires a path".to_owned());
                }
                if value.starts_with('-') {
                    // `--dir --force` took `--force` as the path, created a
                    // directory of that name in the working directory, and
                    // dropped the flag the user actually passed. The contract
                    // declares `--dir` with one argument, and argparse
                    // refuses an option-shaped value there.
                    return Err(format!(
                        "fslc skills --dir requires a path, not the option '{value}'"
                    ));
                }
                options.selection.dir = Some(value.into());
            }
            "-u" | "--user" => options.selection.user = true,
            "--force" => options.selection.force = true,
            "--dry-run" => options.dry_run = true,
            other => return Err(format!("unknown skills option '{other}'")),
        }
    }
    if options.selection.user && options.selection.dir.is_some() {
        return Err("fslc skills takes --dir or --user, not both".to_owned());
    }
    Ok(options)
}

fn install(
    installation: &Installation,
    header: Header,
    options: &Options,
    version: &str,
) -> Result<(Value, i32), String> {
    match installation {
        Installation::Project(installation) => {
            install_project(installation, header, options, version)
        }
        Installation::User(installation) => install_user(installation, header, options, version),
    }
}

fn install_project(
    installation: &project::Installation,
    header: Header,
    options: &Options,
    version: &str,
) -> Result<(Value, i32), String> {
    let plan = installation.plan_install()?;
    let blocked = plan.blocked();
    if !blocked.is_empty() {
        return refuse(header, blocked, &plan);
    }
    let written = plan.writes();
    if !options.dry_run {
        installation.install(&plan, version)?;
    }
    let result = settled_or_installed(plan.is_settled());
    respond(
        &ProjectInstall {
            header,
            result,
            written,
            skills: embedded_skill_names(),
            listing: plan.listing(),
        },
        result,
    )
}

fn install_user(
    installation: &user::Installation,
    header: Header,
    options: &Options,
    version: &str,
) -> Result<(Value, i32), String> {
    let skills = embedded_skill_names();
    let plan = installation.plan_install()?;
    let blocked = plan.blocked();
    if !blocked.is_empty() {
        return refuse(header, blocked, &plan);
    }
    let linked = plan.writes();
    // A dry run asks what the payload and `current` would need, rather than
    // assuming they need nothing.
    let payload = if options.dry_run {
        installation.pending_payload()?
    } else {
        installation.install(&plan, version)?
    };
    let payload_written = payload.written;
    let current_repointed = payload.repointed;
    let result =
        settled_or_installed(plan.is_settled() && payload_written == 0 && !current_repointed);
    respond(
        &UserInstall {
            header,
            result,
            release: installation.release().to_owned(),
            payload: installation.payload_dir().display().to_string(),
            payload_written,
            current_repointed,
            linked,
            skills,
            listing: plan.listing(),
        },
        result,
    )
}

fn status(
    installation: &Installation,
    header: Header,
    version: &str,
) -> Result<(Value, i32), String> {
    let Some(manifest) = installation.manifest()? else {
        return respond(&NotInstalled::new(header), Verdict::NotInstalled);
    };
    let stale = manifest.fslc_version != version;
    match installation {
        Installation::Project(installation) => survey(installation, header, &manifest, stale),
        Installation::User(installation) => survey(installation, header, &manifest, stale),
    }
}

/// Plan an install without carrying it out, and report what the plan implies.
fn survey<P: Placement>(
    placement: &P,
    header: Header,
    manifest: &Manifest,
    stale: bool,
) -> Result<(Value, i32), String>
where
    Plan<P::Item>: Listable,
{
    let plan = placement.plan_install()?;
    let result = if stale {
        Verdict::Stale
    } else if plan.is_settled() {
        Verdict::UpToDate
    } else {
        Verdict::Drifted
    };
    respond(
        &Status {
            header,
            result,
            installed_version: manifest.fslc_version.clone(),
            release: manifest.release.clone(),
            listing: plan.listing(),
        },
        result,
    )
}

fn uninstall(
    installation: &Installation,
    header: Header,
    options: &Options,
) -> Result<(Value, i32), String> {
    if installation.manifest()?.is_none() {
        return respond(&NotInstalled::new(header), Verdict::NotInstalled);
    }
    match installation {
        Installation::Project(installation) => take_back(installation, header, options.dry_run),
        Installation::User(installation) => take_back(installation, header, options.dry_run),
    }
}

/// Plan a removal and carry it out, unless the plan refuses or `--dry-run`.
fn take_back<P: Placement>(
    placement: &P,
    header: Header,
    dry_run: bool,
) -> Result<(Value, i32), String>
where
    Plan<P::Item>: Listable,
{
    let plan = placement.plan_uninstall()?;
    remove(header, &plan, dry_run, |plan| placement.uninstall(plan))
}

/// Refuse, or take the plan back and say how much it took.
fn remove<T: PlanEntry>(
    header: Header,
    plan: &Plan<T>,
    dry_run: bool,
    apply: impl FnOnce(&Plan<T>) -> Result<usize, String>,
) -> Result<(Value, i32), String>
where
    Plan<T>: Listable,
{
    let blocked = plan.blocked();
    if !blocked.is_empty() {
        return refuse(header, blocked, plan);
    }
    let removed = if dry_run {
        plan.removals()
    } else {
        apply(plan)?
    };
    respond(
        &Removal {
            header,
            result: Verdict::Removed,
            removed,
            listing: plan.listing(),
        },
        Verdict::Removed,
    )
}

/// `up_to_date` only when the plan asked for nothing at all.
///
/// Counting writes alone called a run that retired a skill up to date, while
/// it was busy removing one.
const fn settled_or_installed(settled: bool) -> Verdict {
    if settled {
        Verdict::UpToDate
    } else {
        Verdict::Installed
    }
}

fn refuse<'a>(
    header: Header,
    blocked: Vec<&'a str>,
    plan: &'a impl Listable,
) -> Result<(Value, i32), String> {
    respond(
        &Refusal {
            header,
            result: Verdict::Blocked,
            blocked,
            listing: plan.listing(),
        },
        Verdict::Blocked,
    )
}
