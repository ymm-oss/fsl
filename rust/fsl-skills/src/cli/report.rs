// SPDX-License-Identifier: Apache-2.0

//! The shape of a `fslc skills` response.
//!
//! One type per response, so the JSON contract is the struct definition rather
//! than a list of pushed keys. Exit codes come from the verdict, so a response
//! cannot report success and exit non-zero.

use serde::Serialize;
use serde_json::Value;

use crate::installation::Installation;
use crate::installation::plan::{FilePlan, LinkPlan};

/// What a run concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Verdict {
    /// Something was written, linked, or repointed.
    Installed,
    /// Everything was already what it should be.
    UpToDate,
    /// A refusal. Something here is not ours to replace.
    Blocked,
    /// Installed, but no longer matching the payload.
    Drifted,
    /// Installed by a different version of `fslc`.
    Stale,
    /// The recorded files or links are gone.
    Removed,
    /// No manifest describes this directory.
    NotInstalled,
}

impl Verdict {
    /// The process exit code this verdict reports.
    ///
    /// Settled states succeed. Everything a caller would want to act on, a
    /// refusal or an installation that no longer matches, fails, so `status`
    /// works as a check and not only as a report.
    const fn exit_code(self) -> i32 {
        match self {
            Self::Installed | Self::UpToDate | Self::Removed => 0,
            Self::Blocked | Self::Drifted | Self::Stale | Self::NotInstalled => 1,
        }
    }
}

/// The envelope version every `fslc` response leads with.
///
/// `main.rs`'s `envelope()` puts this first on every other command, and
/// `docs/DESIGN-rust-port.md` states it as the shape. These responses are
/// built as typed structs rather than through that helper, so the field is
/// declared here, first, rather than quietly dropped.
const ENVELOPE_VERSION: &str = "1.0";

/// The fields every response carries.
#[derive(Debug, Clone, Serialize)]
pub(super) struct Header {
    fsl: &'static str,
    command: String,
    scope: &'static str,
    directory: String,
    fslc_version: String,
    dry_run: bool,
}

impl Header {
    pub(super) fn new(
        command: &str,
        installation: &Installation,
        version: &str,
        dry_run: bool,
    ) -> Self {
        Self {
            fsl: ENVELOPE_VERSION,
            command: format!("skills {command}"),
            scope: installation.scope().as_str(),
            directory: installation.skills_dir().display().to_string(),
            fslc_version: version.to_owned(),
            dry_run,
        }
    }
}

/// What a response lists, under the key naming what the scope deals in.
///
/// Flattened, so the variant name becomes the field name: a project response
/// carries `files`, a machine-wide one carries `links`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Listing<'a> {
    /// The files of a project install.
    Files(&'a FilePlan),
    /// The links of a machine-wide install.
    Links(&'a LinkPlan),
}

/// A plan that knows which key it lists itself under.
///
/// Without this, every call site picks `Listing::Files` or `Listing::Links` by
/// hand, and the two scopes differ by that one token in otherwise identical
/// code.
pub(super) trait Listable {
    /// This plan, under the key its scope uses.
    fn listing(&self) -> Listing<'_>;
}

impl Listable for FilePlan {
    fn listing(&self) -> Listing<'_> {
        Listing::Files(self)
    }
}

impl Listable for LinkPlan {
    fn listing(&self) -> Listing<'_> {
        Listing::Links(self)
    }
}

/// A project install that went through.
#[derive(Debug, Serialize)]
pub(super) struct ProjectInstall<'a> {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) result: Verdict,
    pub(super) written: usize,
    pub(super) skills: Vec<&'static str>,
    #[serde(flatten)]
    pub(super) listing: Listing<'a>,
}

/// A machine-wide install that went through.
#[derive(Debug, Serialize)]
pub(super) struct UserInstall<'a> {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) result: Verdict,
    pub(super) release: String,
    pub(super) payload: String,
    pub(super) payload_written: usize,
    pub(super) current_repointed: bool,
    pub(super) linked: usize,
    pub(super) skills: Vec<&'static str>,
    #[serde(flatten)]
    pub(super) listing: Listing<'a>,
}

/// A refusal, naming what it will not touch.
#[derive(Debug, Serialize)]
pub(super) struct Refusal<'a> {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) result: Verdict,
    pub(super) blocked: Vec<&'a str>,
    #[serde(flatten)]
    pub(super) listing: Listing<'a>,
}

/// What `status` found.
#[derive(Debug, Serialize)]
pub(super) struct Status<'a> {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) result: Verdict,
    pub(super) installed_version: String,
    pub(super) release: Option<String>,
    #[serde(flatten)]
    pub(super) listing: Listing<'a>,
}

/// What `uninstall` took back.
#[derive(Debug, Serialize)]
pub(super) struct Removal<'a> {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) result: Verdict,
    pub(super) removed: usize,
    #[serde(flatten)]
    pub(super) listing: Listing<'a>,
}

/// A directory no manifest describes.
#[derive(Debug, Serialize)]
pub(super) struct NotInstalled {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) result: Verdict,
}

impl NotInstalled {
    pub(super) const fn new(header: Header) -> Self {
        Self {
            header,
            result: Verdict::NotInstalled,
        }
    }
}

/// The error envelope for a filesystem failure.
///
/// A disk or permission fault is not a usage error. `main.rs` renders an `Err`
/// from a command as `kind: "usage"`, exit 2, which is right for a bad flag
/// and wrong for an unreadable file. Other commands report `kind: "io"` for
/// this, so this one does too.
pub(super) fn io_failure(message: &str) -> (Value, i32) {
    (
        serde_json::json!({
            "fsl": ENVELOPE_VERSION,
            "result": "error",
            "kind": "io",
            "message": message,
        }),
        2,
    )
}

/// Render a response and pair it with the exit code its verdict reports.
///
/// # Errors
///
/// When the response cannot be serialized. A response that cannot be rendered
/// is a defect, not a result, so it is reported rather than replaced by null.
pub(super) fn respond<T: Serialize>(body: &T, verdict: Verdict) -> Result<(Value, i32), String> {
    let value = serde_json::to_value(body)
        .map_err(|error| format!("failed to render the skills response: {error}"))?;
    Ok((value, verdict.exit_code()))
}
