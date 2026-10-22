// SPDX-License-Identifier: Apache-2.0

//! The one ordered list of stages that decides whether `check` accepts a spec.
//!
//! The native CLI and the browser Worker both answer `check`, and
//! `docs/design/DESIGN-rust-components.md` hard gate 6 forbids either of them
//! from deciding semantic validity on its own. They used to hand-write the stage
//! list separately, and the Worker's copy lacked the specialized-document
//! validation, the source-diagnostic preflight, the Agent dispatch, and the
//! init write-ownership check, so it answered `ok` for specs native `check`
//! rejects (issue #1163; #1059 is the same shape on `kernel`). Both surfaces
//! now call [`run_check_stages`] with their own resolver and envelope, and
//! only compose the success envelope (warnings, `implements`, `governance`)
//! themselves.
//!
//! The last stage asks a solver the requirements checks the finite-domain
//! enumeration deferred ([`crate::requirements_solver`], decisions/048 J3).
//! The lowering stages are synchronous; this one is not, because the Worker's
//! solver answers through a promise, so [`run_check_stages`] is `async` and
//! each surface passes its own solver (native Z3, or the Worker's z3js).

use fsl_core::{FileResolver, KernelModel, KernelSpec};
use serde_json::{Map, Value, json};

use crate::spec_load::{SemanticDiagnostic, SpecLoadError, kernel_load_error};
use crate::verification_output::{render_runtime_error, render_semantic_error, require_trace_gate};

/// A spec that passed every validity stage of `check`.
#[derive(Debug)]
pub struct CheckedSpec {
    pub kernel: KernelSpec,
    pub model: KernelModel,
}

/// Run `check`'s validity stages in their one order.
///
/// `source_path` names the root source for lowering and for the file label in
/// specialized-validation messages; `display_path` is the label shown in the
/// AI-project and source-diagnostic envelopes (they differ for a materialized
/// literate input). `resolver` reads imports and `envelope` produces the
/// surface's base envelope for every rendered result.
///
/// `new_solver` makes the solver of the requirements stage, once per
/// question; it is not called for a spec that has none.
///
/// `Err` carries a finished envelope and exit status: a rejection, or the
/// complete result of a document `check` does not lower to a Kernel model (a
/// legacy AI project or an Agent).
///
/// # Errors
///
/// Returns the rendered envelope and exit status of the first stage that ends
/// the check.
pub async fn run_check_stages<S: fsl_solver::SmtSolver>(
    source: &str,
    source_path: &str,
    display_path: &str,
    resolver: &dyn FileResolver,
    envelope: &dyn Fn() -> Map<String, Value>,
    options: &fsl_syntax::ParseOptions,
    new_solver: &mut crate::requirements_solver::NewSolver<'_, S>,
) -> Result<CheckedSpec, (Value, i32)> {
    let checked = match check_stages(
        source,
        source_path,
        display_path,
        resolver,
        envelope,
        *options,
    ) {
        Ok(checked) => {
            match crate::requirements_solver::refute(source, &checked.model, options, new_solver)
                .await
            {
                Some(rejection) => Err((rejection.render(envelope()), 2)),
                None => Ok(checked),
            }
        }
        Err(failure) => Err(failure),
    };
    checked.map_err(|(mut output, status)| {
        display_alias(&mut output, source, *options);
        (output, status)
    })
}

/// A rejection names canonical names only: resolution replaced every alias
/// before lowering. For a document with a glossary, show the reader's word
/// beside it: each quoted canonical name in the message that has an alias
/// reads `'alias（name）'`, and the envelope gains `name` (canonical) and
/// `display` (alias) for the name the diagnostic is about: the alias written
/// at its location, else the first aliased name it quotes. Nothing changes
/// for a document without a glossary.
fn display_alias(output: &mut Value, source: &str, options: fsl_syntax::ParseOptions) {
    let options = &options;
    let Some(glossary) = fsl_syntax::document_glossary(source, options) else {
        return;
    };
    let Some(object) = output.as_object_mut() else {
        return;
    };
    let located = object
        .get("loc")
        .and_then(|loc| {
            Some((
                u32::try_from(loc.get("line")?.as_u64()?).ok()?,
                u32::try_from(loc.get("column")?.as_u64()?).ok()?,
            ))
        })
        .and_then(|(line, column)| fsl_syntax::alias_at(source, options, line, column));
    // A glossary diagnostic already names both spellings.
    let about_glossary = object
        .get("diagnostic_code")
        .and_then(Value::as_str)
        .is_some_and(|code| fsl_syntax::GLOSSARY_DIAGNOSTICS.contains(&code));
    let mut quoted = None;
    if let Some(Value::String(message)) = object.get_mut("message")
        && !about_glossary
    {
        let mut first = None;
        for entry in &glossary.entries {
            let name = format!("'{}'", entry.canonical.name);
            if let Some(position) = message.find(&name)
                && first.as_ref().is_none_or(|(at, _)| position < *at)
            {
                first = Some((position, entry));
            }
        }
        quoted = first.map(|(_, entry)| (entry.canonical.name.clone(), entry.alias.clone()));
        for entry in &glossary.entries {
            *message = message.replace(
                &format!("'{}'", entry.canonical.name),
                &format!("'{}（{}）'", entry.alias, entry.canonical.name),
            );
        }
    }
    if let Some((canonical, alias)) = located.or(quoted) {
        object
            .entry("name")
            .or_insert_with(|| Value::String(canonical));
        object
            .entry("display")
            .or_insert_with(|| Value::String(alias));
    }
}

fn check_stages(
    source: &str,
    source_path: &str,
    display_path: &str,
    resolver: &dyn FileResolver,
    envelope: &dyn Fn() -> Map<String, Value>,
    options: fsl_syntax::ParseOptions,
) -> Result<CheckedSpec, (Value, i32)> {
    let options = &options;
    // The fsl-ai project gate carries its own exit code: an unexecutable
    // `require` clause is a spec error (issue #542), so this may not be
    // flattened back to a fixed exit 0.
    if let Some(result) =
        crate::frontend_output::ai_project_check_output(source, display_path, envelope())
    {
        return Err(result);
    }
    match fsl_syntax::parse_document_with(fsl_syntax::SourceFile::new(source), options) {
        Ok(fsl_syntax::ParsedDocument {
            surface: fsl_syntax::SurfaceDocument::Agent(agent),
            ..
        }) => return Err(agent_check_output(&agent, envelope())),
        Ok(_) => {}
        Err(error) => {
            return Err((
                crate::frontend_output::render_surface_parse_error(envelope(), &error),
                2,
            ));
        }
    }
    // The source-diagnostic preflight lowers every regular dialect document.
    // Validate specialized documents first, so an invalid AI authority name
    // cannot reach `lower_ai_component`'s generated-member lookup. Surface
    // parsing stays ahead of this validation to retain parse-error envelopes.
    if let Err(error) = validate_specialized_document(source, source_path, options) {
        return Err((semantic_error(envelope(), &error), 2));
    }
    if let Some(diagnostic) =
        crate::source_diagnostic::diagnostics(source, display_path, resolver, options)
            .into_iter()
            .find(|diagnostic| diagnostic.kind != "migration")
    {
        // `check` returns here before it reaches the Kernel load, so the
        // location and classification have to travel through this branch too,
        // or `check` alone reports `loc: null` and `semantics` for a diagnostic
        // every other command locates and classifies (issues 555, 565).
        return Err((
            render_semantic_error(
                envelope(),
                &diagnostic.message,
                diagnostic.located.then(|| diagnostic.span.python_loc()),
                &diagnostic.class,
                Some(diagnostic.code.as_str()).filter(|code| {
                    *code != "FSL-SEMANTIC" && *code != "FSL-TYPE" && *code != "FSL-NAME"
                }),
                diagnostic.hint.as_deref(),
            ),
            2,
        ));
    }
    let (kernel, model) =
        load_kernel_model(source, source_path, resolver, options).map_err(|error| {
            (
                crate::spec_load::render_spec_load_error(envelope(), &error),
                2,
            )
        })?;
    // `build_model` does not check init assign-once ownership; without this
    // stage an init that writes one variable from two `forall`s is accepted.
    if let Err(error) = fsl_runtime::check_init_write_ownership(&model) {
        return Err((render_runtime_error(envelope(), &error), 2));
    }
    if let Err(refusal) = require_trace_gate(&envelope(), source, &model, options) {
        return Err((refusal, 2));
    }
    Ok(CheckedSpec { kernel, model })
}

/// Validate a `dbsystem` or `ai_component` document beyond what lowering
/// checks; any other document kind passes.
///
/// # Errors
///
/// Returns the validation (or surface-parse) message; a parse message names
/// `source_path` as its file.
pub fn validate_specialized_document(
    source: &str,
    source_path: &str,
    options: &fsl_syntax::ParseOptions,
) -> Result<(), String> {
    let document = fsl_syntax::parse_surface_document_with(source, options).map_err(|error| {
        format!(
            "{} at {}:{}:{}",
            error.message, source_path, error.span.start.line, error.span.start.column
        )
    })?;
    match document {
        fsl_syntax::SurfaceDocument::Db(system) => {
            fsl_tools::validate_db(&system).map_err(|error| error.to_string())
        }
        fsl_syntax::SurfaceDocument::AiComponent(component) => {
            fsl_core::validate_ai_component(&component).map_err(|error| error.to_string())
        }
        _ => Ok(()),
    }
}

/// Lower and type-check a root source into its Kernel spec and checked model.
///
/// # Errors
///
/// Returns the classified spec-load failure.
pub fn load_kernel_model(
    source: &str,
    source_path: &str,
    resolver: &dyn FileResolver,
    options: &fsl_syntax::ParseOptions,
) -> Result<(KernelSpec, KernelModel), SpecLoadError> {
    let kernel = fsl_core::parse_kernel_source_with_file_with_options(
        source,
        resolver,
        source_path,
        options,
    )
    .map_err(|error| kernel_load_error(source, &error, options))?;
    let model = fsl_core::build_model(kernel.clone()).map_err(|error| {
        SpecLoadError::Semantic(Box::new(SemanticDiagnostic::from_model_error(&error)))
    })?;
    Ok((kernel, model))
}

/// `check` on an Agent document is deliberately lenient: the top-level
/// `result` stays "ok" even when the structural analysis finds a violation.
///
/// The analysis `result` is informational here (matching the frozen
/// reference); `fslc ai check` is the actual gate (exit 1 on
/// `agent_analysis_result: "violated"`). A grant-boundary or other
/// tree-validation failure is still a hard error.
#[must_use]
pub fn agent_check_output(
    agent: &fsl_syntax::SurfaceAgent,
    mut output: Map<String, Value>,
) -> (Value, i32) {
    let analysis = match fsl_tools::analyze_ai_agent(agent) {
        Ok(analysis) => analysis,
        Err(error) => return (agent_error_output(output, &error), 2),
    };
    let analysis_result = analysis
        .get("result")
        .cloned()
        .unwrap_or_else(|| json!("agent_analyzed"));
    output.insert("result".to_owned(), json!("ok"));
    output.insert("spec".to_owned(), json!(agent.name));
    output.insert("dialect".to_owned(), json!("fsl-ai-agent.v0"));
    output.insert("warnings".to_owned(), json!([]));
    output.insert("ai_analysis_result".to_owned(), analysis_result.clone());
    output.insert("agent_analysis_result".to_owned(), analysis_result);
    (Value::Object(output), 0)
}

/// Render an Agent analysis failure.
#[must_use]
pub fn agent_error_output(mut output: Map<String, Value>, error: &fsl_tools::AgentError) -> Value {
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!("semantics"));
    output.insert("message".to_owned(), json!(error.message));
    if let Some(loc) = error.loc {
        output.insert(
            "loc".to_owned(),
            json!({"line": loc.line, "column": loc.column}),
        );
    }
    if let Some(hint) = &error.hint {
        output.insert("hint".to_owned(), json!(hint));
    }
    Value::Object(output)
}

fn semantic_error(output: Map<String, Value>, message: &str) -> Value {
    render_semantic_error(
        output,
        message,
        None,
        &crate::verification_output::DiagnosticClass::Message,
        None,
        None,
    )
}
