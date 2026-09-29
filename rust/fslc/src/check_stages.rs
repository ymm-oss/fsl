// SPDX-License-Identifier: Apache-2.0

//! The one ordered list of stages that decides whether `check` accepts a spec.
//!
//! The native CLI and the browser Worker both answer `check`, and
//! `docs/DESIGN-rust-components.md` hard gate 6 forbids either of them from
//! deciding semantic validity on its own. They used to hand-write the stage
//! list separately, and the Worker's copy lacked the specialized-document
//! validation, the source-diagnostic preflight, the Agent dispatch, and the
//! init write-ownership check, so it answered `ok` for specs native `check`
//! rejects (issue #1163; #1059 is the same shape on `kernel`). Both surfaces
//! now call [`run_check_stages`] with their own resolver and envelope, and
//! only compose the success envelope (warnings, `implements`, `governance`)
//! themselves.

use fsl_core::{FileResolver, KernelModel, KernelSpec};
use serde_json::{Map, Value, json};

use crate::spec_load::{SemanticDiagnostic, SpecLoadError, kernel_load_error};
use crate::verification_output::{
    render_runtime_error, render_semantic_error, validate_requirement_trace_source,
};

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
/// `Err` carries a finished envelope and exit status: a rejection, or the
/// complete result of a document `check` does not lower to a Kernel model (a
/// legacy AI project or an Agent).
///
/// # Errors
///
/// Returns the rendered envelope and exit status of the first stage that ends
/// the check.
pub fn run_check_stages(
    source: &str,
    source_path: &str,
    display_path: &str,
    resolver: &dyn FileResolver,
    envelope: &dyn Fn() -> Map<String, Value>,
) -> Result<CheckedSpec, (Value, i32)> {
    // The fsl-ai project gate carries its own exit code: an unexecutable
    // `require` clause is a spec error (issue #542), so this may not be
    // flattened back to a fixed exit 0.
    if let Some(result) =
        crate::frontend_output::ai_project_check_output(source, display_path, envelope())
    {
        return Err(result);
    }
    match fsl_syntax::parse_document(fsl_syntax::SourceFile::new(source)) {
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
    if let Err(error) = validate_specialized_document(source, source_path) {
        return Err((semantic_error(envelope(), &error), 2));
    }
    if let Some(diagnostic) = crate::source_diagnostic::diagnostics(source, display_path, resolver)
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
                diagnostic.kind == "name",
                Some(diagnostic.code.as_str()).filter(|code| {
                    *code != "FSL-SEMANTIC" && *code != "FSL-TYPE" && *code != "FSL-NAME"
                }),
                diagnostic.hint.as_deref(),
            ),
            2,
        ));
    }
    let (kernel, model) = load_kernel_model(source, source_path, resolver).map_err(|error| {
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
    match validate_requirement_trace_source(&envelope(), source, &model) {
        Ok((Some(failure), _)) => return Err((failure, 2)),
        Ok((None, _)) => {}
        Err(error) => return Err((semantic_error(envelope(), &error), 2)),
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
pub fn validate_specialized_document(source: &str, source_path: &str) -> Result<(), String> {
    let document = fsl_syntax::parse_surface_document(source).map_err(|error| {
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
) -> Result<(KernelSpec, KernelModel), SpecLoadError> {
    let kernel = fsl_core::parse_kernel_source_with_file(source, resolver, source_path)
        .map_err(|error| kernel_load_error(source, &error))?;
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
    render_semantic_error(output, message, None, false, None, None)
}
