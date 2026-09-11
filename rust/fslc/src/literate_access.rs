// SPDX-License-Identifier: Apache-2.0

//! The single owner of "does this command accept literate Markdown (`.md`)
//! input" (issue #665).
//!
//! Literate Markdown FSL (#193) extracts ```` ```fsl ```` fences from a `.md`
//! file so `check`/`verify`/`scenarios` can verify the fenced code as an
//! ordinary spec. Every other command that reads a spec path used to hand the
//! raw Markdown straight to the surface parser, which reported the user's
//! spec as having a *syntax error* at the position of the first character
//! the parser could not make sense of -- typically `1:2`, the `!` of the
//! Markdown file's own `<!--` comment. The diagnostic was truthful about the
//! exit code (2, the spec-error class) but not about the cause: the command
//! does not understand the input's *kind*, not the spec's syntax.
//!
//! [`materialize_literate`] used to live in `main.rs` with exactly two call
//! sites (`check`, and the shared `verify`/`scenarios` arm). A third command
//! would have reached the surface parser unfixed by simply not knowing this
//! function existed -- a helper called from N arms is not ownership, because
//! arm N+1 is written without it. [`literate_access`] is now the one
//! function every arm that reads a spec path calls immediately after
//! resolving its command key: it materializes for [`LiterateSupport::Supported`]
//! commands exactly as before, and fails closed with
//! [`LITERATE_UNSUPPORTED_CODE`] for [`LiterateSupport::Unsupported`] ones
//! instead of ever reaching the parser. [`LITERATE_REGISTRY`] is the
//! enumeration that *is* the registry (following `outcome::outcome_class`'s
//! idiom): [`literate_support`] returns `None` for a key nobody classified,
//! and `literate_registry_is_total_over_the_cli_surface` below fails the
//! build the moment a spec-path-taking leaf command in
//! `rust/fslc/cli-contract.json` is neither registered nor explicitly
//! excluded with a reason.
//!
//! # Scope boundary
//!
//! This registry's domain is the command keys in [`LITERATE_REGISTRY`]: leaf
//! commands that read at least one positional spec-shaped path
//! (`rust/fslc/cli-contract.json`'s notion of "takes a spec-shaped
//! argument") and are gated here before handing a `.md` input to a dialect
//! or shared-kernel frontend. [`LITERATE_EXCLUDED`] lists every other
//! spec-shaped leaf command together with why it is out of this registry's
//! scope, so the totality test can still be total over the *real* CLI
//! surface without this module silently claiming ownership of a command it
//! does not actually gate:
//!
//! - `chain`'s positional is a project manifest (`fsl-project.toml`); a
//!   `.md` there fails TOML parsing, never the FSL frontend (measured:
//!   `fslc chain examples/literate/toggle.md` reports `kind:"parse"` with
//!   message `"invalid TOML at line 1..."`, not the Markdown-as-spec lie
//!   this issue is about). It is not "a command that takes a spec path" in
//!   this issue's sense.
//! - `approval create`/`check`/`diff` share the same defect as the
//!   registered commands, but stay excluded (tracked as #980). Measured on
//!   `a3703211`: `approval create` always parses its positional as an FSL
//!   spec regardless of `--kind` (`approval create toggle.md
//!   --kind requirements_document|ledger …` both report `kind:"parse"`,
//!   `diagnostic_code:"FSL-PARSE"`, `unexpected character '!'` at `1:2`;
//!   the legitimately `.md`-shaped input under `requirements_document` is
//!   `--artifact`, not the positional) and cannot produce a record whose
//!   `spec.path` is `.md` (it fails with `FSL-PARSE` before any record is
//!   written). `approval check`/`diff` compare the record's `spec.path`
//!   against the positional first; a mismatch short-circuits to a clean
//!   target-mismatch message, but when they match -- including a hand-forged
//!   record whose `spec.path` is a `.md` file, which `create` cannot
//!   construct -- they reach the FSL frontend and reproduce the exact `1:2`
//!   lie (measured with such a forged record).
//! - `db`/`compat check`/`ai check`/`ai replay`/`ai compat`/`causal`/`domain`'s
//!   dialect-specific spec arguments used to hit the same Markdown-as-spec-syntax
//!   lie; they are now registered [`LiterateSupport::Unsupported`] and gated at
//!   their dispatch arms (#694). `db import` is the one `db` leaf that stays
//!   excluded: its positional is a SQL/Prisma schema artifact (`--source
//!   auto|sql|prisma`; `run_db_import` → `std::fs::read_to_string` →
//!   `fsl_tools::import_db`), and `fslc db import
//!   examples/literate/toggle.md` reads the Markdown as SQL DDL and reports
//!   `result:"imported_with_warnings"` with `unsupported_sql` and
//!   `empty_import` warnings, exit 0 -- `db import`'s own contract, never
//!   the FSL frontend.
//! - `ai eval`/`ai regress`/`ai drift` call `load_ai_project`, which has its
//!   own `.md` branch that calls `fsl_syntax::parse_ai_project` independently
//!   of this module; measured on `a3703211`, a valid literate AI project `.md`
//!   succeeds (exit 0) and other Markdown reports a clean semantic error
//!   (`expected fsl-ai project declarations`) with no false spec position --
//!   not the #665/#694 lie, and gating it here would regress working literate
//!   support.
//!
//! Implementing literate support for every command is issue #666, and is
//! deliberately not attempted here: [`LiterateSupport::Unsupported`] commands
//! stay unsupported after this change, and become unreachable code only once
//! #666 (or a future decision) reclassifies them -- this module does not
//! depend on that ordering.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// A `.md` input materialized into a process-owned `.fsl` sibling. Dropping
/// it removes the sibling file.
///
/// Each CLI process owns its own materialization; the original Markdown path
/// is used separately as the stable verify-cache identity, so physical
/// isolation does not trade away cache hits across invocations.
pub struct LiterateState {
    pub path: PathBuf,
}

impl Drop for LiterateState {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Extract literate Markdown FSL's ```` ```fsl ```` fences from `path` into a
/// temporary `.fsl` sibling, when `path`'s extension is `.md`.
///
/// Returns `Ok(None)` when `path` is not a `.md` path at all, so callers use
/// `path` unchanged in that case.
///
/// # Errors
///
/// Returns a message when `path` cannot be read or contains no ```` ```fsl ````
/// fenced code blocks.
pub fn materialize_literate(path: &Path) -> Result<Option<LiterateState>, String> {
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
        return Ok(None);
    }
    let raw =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let blanked = fsl_syntax::extract_literate_fsl(&raw).ok_or_else(|| {
        format!(
            "{}: Markdown file does not contain any ```fsl fenced code blocks",
            path.display()
        )
    })?;
    let stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("literate");
    let materialized = literate_materialization_path(path, stem, std::process::id());
    std::fs::write(&materialized, &blanked).map_err(|error| error.to_string())?;
    Ok(Some(LiterateState { path: materialized }))
}

fn literate_materialization_path(path: &Path, stem: &str, process_id: u32) -> PathBuf {
    path.with_file_name(format!(".{stem}.literate-{process_id}.fsl"))
}

/// Whether a registered command extracts ```` ```fsl ```` fences from a `.md`
/// input, or fails closed instead of handing raw Markdown to the parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiterateSupport {
    /// Materializes the `.md` input exactly as `check` does.
    Supported,
    /// Fails closed with [`LITERATE_UNSUPPORTED_CODE`] naming the commands
    /// that do support it, instead of reaching the surface parser.
    Unsupported,
}

/// The command keys [`LiterateSupport::Supported`] classifies -- named once
/// so the registry and the unsupported-command diagnostic's message cannot
/// drift apart from each other.
pub const LITERATE_SUPPORTED_COMMANDS: &[&str] = &["check", "verify", "scenarios"];

/// The total registry of command keys whose spec argument is classified
/// through `main.rs`'s top-level `command()` dispatch (including the `compat`
/// arm), `fmt_command()`, `document_command`'s shared `path` prelude,
/// `db_command`, `ai_command`, `domain_command`, or `causal::causal_command`
/// -- the same textual seams `materialize_literate`'s original two call sites
/// (`check`, and the combined `verify`/`scenarios` arm) already sat in.
///
/// A nested command's key is its space-joined path (`"document generate"`),
/// matching `rust/fslc/cli-contract.json`'s own leaf-path convention so the
/// totality test below can compare directly against it.
pub const LITERATE_REGISTRY: &[(&str, LiterateSupport)] = &[
    ("check", LiterateSupport::Supported),
    ("verify", LiterateSupport::Supported),
    ("scenarios", LiterateSupport::Supported),
    ("lint", LiterateSupport::Unsupported),
    ("migrate", LiterateSupport::Unsupported),
    ("kernel", LiterateSupport::Unsupported),
    ("conformance", LiterateSupport::Unsupported),
    ("counterexample export", LiterateSupport::Unsupported),
    ("explain", LiterateSupport::Unsupported),
    ("mutate", LiterateSupport::Unsupported),
    ("typestate", LiterateSupport::Unsupported),
    ("testgen", LiterateSupport::Unsupported),
    ("testplan", LiterateSupport::Unsupported),
    ("html", LiterateSupport::Unsupported),
    ("ledger", LiterateSupport::Unsupported),
    ("analyze", LiterateSupport::Unsupported),
    ("diff", LiterateSupport::Unsupported),
    ("refine", LiterateSupport::Unsupported),
    ("replay", LiterateSupport::Unsupported),
    ("sweep", LiterateSupport::Unsupported),
    ("fmt", LiterateSupport::Unsupported),
    ("document generate", LiterateSupport::Unsupported),
    ("document claims", LiterateSupport::Unsupported),
    ("document check", LiterateSupport::Unsupported),
    ("db check", LiterateSupport::Unsupported),
    ("db observe", LiterateSupport::Unsupported),
    ("compat check", LiterateSupport::Unsupported),
    ("domain check", LiterateSupport::Unsupported),
    ("domain analyze", LiterateSupport::Unsupported),
    ("domain expand", LiterateSupport::Unsupported),
    ("domain generate", LiterateSupport::Unsupported),
    ("domain replay", LiterateSupport::Unsupported),
    ("domain testgen", LiterateSupport::Unsupported),
    ("ai check", LiterateSupport::Unsupported),
    ("ai replay", LiterateSupport::Unsupported),
    ("ai compat", LiterateSupport::Unsupported),
    ("causal check", LiterateSupport::Unsupported),
    ("causal analyze", LiterateSupport::Unsupported),
    ("causal diff", LiterateSupport::Unsupported),
    ("causal ledger", LiterateSupport::Unsupported),
    ("causal observe-expectations", LiterateSupport::Unsupported),
    ("causal verify-expectations", LiterateSupport::Unsupported),
];

/// Leaf commands in `rust/fslc/cli-contract.json` that read at least one
/// positional path argument but are outside [`LITERATE_REGISTRY`]'s scope,
/// paired with why (see this module's "Scope boundary" section). Kept
/// alongside the registry, rather than only in prose, so
/// `literate_registry_is_total_over_the_cli_surface` can assert every
/// spec-shaped leaf command is *accounted for* -- registered or knowingly
/// excluded -- even though only the registered half is behaviorally fixed by
/// this change.
pub const LITERATE_EXCLUDED: &[(&str, &str)] = &[
    (
        "chain",
        "positional is a project manifest (TOML), not a spec; a .md there fails TOML parsing, never the FSL frontend",
    ),
    (
        "approval create",
        "the positional is always parsed as an FSL spec regardless of `--kind`; measured on a3703211: `approval create toggle.md --kind requirements_document|ledger ...` both report kind:\"parse\", diagnostic_code:\"FSL-PARSE\", unexpected character '!' at 1:2. Tracked as #980.",
    ),
    (
        "approval check",
        "approval create cannot produce a record whose spec.path is a .md file (measured on a3703211: approval create <.md> --kind requirements_document|ledger ... fails with FSL-PARSE before a record is written); if such a record exists from any other source, the record's spec.path must match the positional before parsing, and when it does, approval check/diff reach the FSL frontend and reproduce the exact 1:2 lie (measured with a hand-forged record whose spec.path matches a .md positional -- a mismatched record short-circuits to a clean target mismatch instead). Same defect this PR fixes elsewhere; gated because create cannot construct the precondition today. Tracked as #980.",
    ),
    (
        "approval diff",
        "approval create cannot produce a record whose spec.path is a .md file (measured on a3703211: approval create <.md> --kind requirements_document|ledger ... fails with FSL-PARSE before a record is written); if such a record exists from any other source, the record's spec.path must match the positional before parsing, and when it does, approval check/diff reach the FSL frontend and reproduce the exact 1:2 lie (measured with a hand-forged record whose spec.path matches a .md positional -- a mismatched record short-circuits to a clean target mismatch instead). Same defect this PR fixes elsewhere; gated because create cannot construct the precondition today. Tracked as #980.",
    ),
    (
        "db import",
        "positional is a SQL/Prisma schema artifact (--source auto|sql|prisma), read by run_db_import -> fsl_tools::import_db, never the FSL frontend; measured on a3703211: `fslc db import examples/literate/toggle.md` imports the Markdown as SQL DDL and reports result:\"imported_with_warnings\" with unsupported_sql and empty_import warnings, exit 0 -- db import's own contract, the same fact PARITY_REGISTRY states",
    ),
    (
        "ai eval",
        "calls load_ai_project, whose .md branch invokes fsl_syntax::parse_ai_project independently of this module; measured on a3703211, a valid literate AI project .md succeeds (exit 0) and other Markdown reports a clean semantic error with no false spec position -- not the #665/#694 lie, and gating here would regress working literate support",
    ),
    (
        "ai regress",
        "calls load_ai_project, whose .md branch invokes fsl_syntax::parse_ai_project independently of this module; measured on a3703211, a valid literate AI project .md succeeds (exit 0) and other Markdown reports a clean semantic error with no false spec position -- not the #665/#694 lie, and gating here would regress working literate support",
    ),
    (
        "ai drift",
        "calls load_ai_project, whose .md branch invokes fsl_syntax::parse_ai_project independently of this module; measured on a3703211, a valid literate AI project .md succeeds (exit 0) and other Markdown reports a clean semantic error with no false spec position -- not the #665/#694 lie, and gating here would regress working literate support",
    ),
];

/// The diagnostic code an [`LiterateSupport::Unsupported`] command reports
/// instead of `FSL-PARSE`.
///
/// Deliberately not routed through `fsl_syntax::ParseError::coded`: a
/// `ParseError` means "the spec is malformed", which is exactly the false
/// claim this diagnostic replaces. It is rendered directly into the JSON
/// envelope by [`literate_unsupported_output`] instead.
pub const LITERATE_UNSUPPORTED_CODE: &str = "FSL-INPUT-LITERATE-UNSUPPORTED";

/// Classify `command_key` against [`LITERATE_REGISTRY`]. `None` means the key
/// is not registered at all -- either it does not read a spec path through
/// this shared decision point, or (the case the totality test below exists
/// to catch) it does and nobody has classified it yet.
#[must_use]
pub fn literate_support(command_key: &str) -> Option<LiterateSupport> {
    LITERATE_REGISTRY
        .iter()
        .find(|(key, _)| *key == command_key)
        .map(|(_, support)| *support)
}

/// Render the `FSL-INPUT-LITERATE-UNSUPPORTED` envelope for `command_key`
/// receiving `.md` input it does not support.
///
/// `kind` is `"usage"` (issue #665 design constraint 3): the caller invoked a
/// real command with an input kind it does not handle, which is exactly what
/// `"usage"` already means elsewhere, and it already exits 2 -- reusing it
/// keeps this change from touching the exit-code contract at all. `loc`
/// deliberately never carries a spec line/column: it names the input file
/// only, so a repair agent is not pointed at a spec position that does not
/// exist.
#[must_use]
pub fn literate_unsupported_output(command_key: &str, path: &Path) -> Value {
    let supported = LITERATE_SUPPORTED_COMMANDS.join(", ");
    json!({
        "fsl": "1.0",
        "result": "error",
        "kind": "usage",
        "message": format!(
            "fslc {command_key} does not support literate Markdown input; only {supported} \
             extract ```fsl fenced code from a .md file -- pass '{}' to one of those, or \
             extract the fenced code into a .fsl file first",
            path.display(),
        ),
        "diagnostic_code": LITERATE_UNSUPPORTED_CODE,
        "loc": {"file": path.to_string_lossy()},
    })
}

/// The single call every arm that reads a spec path makes immediately after
/// resolving `command_key`, in place of calling [`materialize_literate`]
/// directly.
///
/// Returns `Ok(None)` when `display_path` is not literate input at all (any
/// extension other than `.md`) -- callers use `display_path` unchanged, as
/// `check` and `verify`/`scenarios` already did before this function
/// existed. A supported command's `.md` input still materializes exactly as
/// before; an unsupported one's fails closed with the envelope
/// [`literate_unsupported_output`] renders, ready to return directly as the
/// command's own `(Value, i32)` result.
///
/// `command_key` must be present in [`LITERATE_REGISTRY`]; this is enforced
/// at test time (`literate_registry_is_total_over_the_cli_surface`), not at
/// runtime, per issue #665 design constraint 5 -- an unregistered key reaches
/// this function only if a future edit adds a new call site without
/// registering it, and the fallback below reports that as an internal
/// inconsistency rather than a spec verdict, exactly as
/// `outcome::outcome_class`'s unregistered-value arm does for `result`.
///
/// # Errors
///
/// Returns the `(Value, i32)` envelope and exit code the caller should
/// return verbatim: an IO failure materializing a supported command's input,
/// or the unsupported-command diagnostic.
pub fn literate_access(
    command_key: &str,
    display_path: &Path,
) -> Result<Option<LiterateState>, (Value, i32)> {
    if display_path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
        return Ok(None);
    }
    match literate_support(command_key) {
        Some(LiterateSupport::Supported) => materialize_literate(display_path).map_err(|message| {
            (
                json!({"fsl":"1.0","result":"error","kind":"io","message":message}),
                2,
            )
        }),
        Some(LiterateSupport::Unsupported) => {
            Err((literate_unsupported_output(command_key, display_path), 2))
        }
        None => Err((
            json!({
                "fsl": "1.0",
                "result": "error",
                "kind": "internal",
                "message": format!(
                    "command '{command_key}' is not registered in the literate-input registry"
                ),
            }),
            3,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LITERATE_EXCLUDED, LITERATE_REGISTRY, LITERATE_SUPPORTED_COMMANDS, LiterateSupport,
        literate_materialization_path, literate_support,
    };
    use serde_json::Value;

    #[test]
    fn literate_materialization_paths_are_process_owned() {
        let source = std::path::Path::new("spec.md");
        assert_ne!(
            literate_materialization_path(source, "spec", 41),
            literate_materialization_path(source, "spec", 42)
        );
    }

    /// Recover every leaf command path in the embedded CLI contract that
    /// reads at least one positional argument -- the contract's own notion
    /// of "takes a spec-shaped path". `native_cli_help_matches_the_embedded_contract_at_every_command_path`
    /// (`rust/fslc/tests/native_integration.rs`) keeps this file honest
    /// against the live `--help` tree, which is what makes it a real CLI
    /// surface to gate against rather than a second hand-written list.
    fn leaf_positional_commands(node: &Value, path: &mut Vec<String>, out: &mut Vec<String>) {
        let commands = node.get("commands").and_then(Value::as_array);
        match commands {
            Some(commands) if !commands.is_empty() => {
                for command in commands {
                    let Some(name) = command
                        .get("path")
                        .and_then(Value::as_array)
                        .and_then(|segments| segments.last())
                        .and_then(Value::as_str)
                    else {
                        continue;
                    };
                    path.push(name.to_owned());
                    leaf_positional_commands(command, path, out);
                    path.pop();
                }
            }
            _ => {
                let has_positional =
                    node.get("actions")
                        .and_then(Value::as_array)
                        .is_some_and(|actions| {
                            actions
                                .iter()
                                .any(|action| action.get("positional") == Some(&Value::Bool(true)))
                        });
                if has_positional && !path.is_empty() {
                    out.push(path.join(" "));
                }
            }
        }
    }

    fn cli_surface_spec_path_commands() -> Vec<String> {
        let contract: Value = serde_json::from_str(include_str!("../cli-contract.json"))
            .expect("valid embedded CLI contract");
        let mut leaves = Vec::new();
        leaf_positional_commands(&contract["root"], &mut Vec::new(), &mut leaves);
        leaves
    }

    /// Design constraint 2 (issue #665): the registry must be total and
    /// gated. Every leaf command in the real CLI surface that reads a
    /// spec-shaped positional is either registered (`LITERATE_REGISTRY`) or
    /// explicitly excluded with a reason (`LITERATE_EXCLUDED`) -- never
    /// neither. A command added to `command()`/`fmt_command()`/
    /// `document_command()` without updating either list fails this test
    /// instead of silently reaching the surface parser unfixed, which is the
    /// #689/#563 blind spot this issue's design explicitly avoids
    /// reproducing.
    #[test]
    fn literate_registry_is_total_over_the_cli_surface() {
        let unaccounted = cli_surface_spec_path_commands()
            .into_iter()
            .filter(|command| {
                !LITERATE_REGISTRY.iter().any(|(key, _)| *key == command)
                    && !LITERATE_EXCLUDED.iter().any(|(key, _)| *key == command)
            })
            .collect::<Vec<_>>();
        assert!(
            unaccounted.is_empty(),
            "commands with a spec-shaped positional argument are neither in \
             LITERATE_REGISTRY nor LITERATE_EXCLUDED: {unaccounted:?} -- classify each in \
             rust/fslc/src/literate_access.rs"
        );
    }

    /// The two lists must not overlap: a command claimed as both registered
    /// and knowingly-excluded is a self-contradiction the totality test
    /// above would not catch on its own.
    #[test]
    fn no_command_is_both_registered_and_excluded() {
        for (key, _) in LITERATE_REGISTRY {
            assert!(
                !LITERATE_EXCLUDED
                    .iter()
                    .any(|(excluded, _)| excluded == key),
                "'{key}' is in both LITERATE_REGISTRY and LITERATE_EXCLUDED"
            );
        }
    }

    /// An unregistered key classifies as `None`, never as a silent pass
    /// through to either behavior -- the same "unregistered is not a
    /// default" shape `outcome::outcome_class`'s `_ => Failure` arm fixes for
    /// `result` values (#554).
    #[test]
    fn an_unregistered_command_key_is_unclassified() {
        assert_eq!(literate_support("a_command_nobody_registered"), None);
        assert_eq!(literate_support(""), None);
    }

    /// [`LiterateSupport::Supported`] in the registry and
    /// [`LITERATE_SUPPORTED_COMMANDS`] must name exactly the same set, or the
    /// unsupported-command diagnostic's message could advertise a command
    /// the registry does not actually mark supported (or omit one it does).
    #[test]
    fn supported_commands_constant_matches_the_registry() {
        let mut from_registry = LITERATE_REGISTRY
            .iter()
            .filter(|(_, support)| *support == LiterateSupport::Supported)
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();
        from_registry.sort_unstable();
        let mut from_constant = LITERATE_SUPPORTED_COMMANDS.to_vec();
        from_constant.sort_unstable();
        assert_eq!(from_registry, from_constant);
    }

    /// `document generate`/`document claims`/`document check` share one
    /// `path` read in `document_command` before it dispatches on subcommand
    /// (`main.rs`), so all three must classify identically or that shared
    /// call site cannot single-source its decision.
    #[test]
    fn document_subcommands_classify_uniformly() {
        for key in ["document generate", "document claims", "document check"] {
            assert_eq!(
                literate_support(key),
                Some(LiterateSupport::Unsupported),
                "{key}"
            );
        }
    }
}
