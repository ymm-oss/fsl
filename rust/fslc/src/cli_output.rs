// SPDX-License-Identifier: Apache-2.0

//! The JSON envelope every command prints, its error renderers, and the
//! display spellings shared across command outputs.

use std::path::Path;

use fsl_core::TraceStep;
use fslc_rust::spec_load::SpecLoadError;
use fslc_rust::verification_output::DiagnosticClass;
use serde_json::{Map, Value, json};

pub(crate) fn located_error_output(kind: &str, message: &str, span: fsl_syntax::Span) -> Value {
    let mut output = error_output(kind, message);
    output.as_object_mut().expect("error envelope").extend([
        ("loc".to_owned(), span.python_loc()),
        ("span".to_owned(), json!(span)),
    ]);
    output
}

pub(crate) fn display_binding(value: &fsl_core::FslValue) -> String {
    match value {
        fsl_core::FslValue::Int(value) => value.to_string(),
        fsl_core::FslValue::Bool(value) => value.to_string(),
        fsl_core::FslValue::Enum { member, .. } => member.clone(),
        _ => "value".to_owned(),
    }
}

pub(crate) fn analysis_display_path(path: &Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = std::env::current_dir()
        .ok()
        .and_then(|path| path.canonicalize().ok());
    root.as_deref()
        .and_then(|root| resolved.strip_prefix(root).ok())
        .unwrap_or(&resolved)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn envelope() -> Map<String, Value> {
    let mut output = Map::new();
    output.insert("fsl".to_owned(), json!("1.0"));
    output
}

pub(crate) fn error_output(kind: &str, message: &str) -> Value {
    let mut output = envelope();
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!(kind));
    output.insert("message".to_owned(), json!(message));
    Value::Object(output)
}

pub(crate) fn surface_parse_error_output(error: &fsl_syntax::ParseError) -> Value {
    fslc_rust::frontend_output::render_surface_parse_error(envelope(), error)
}

pub(crate) fn semantic_error_output(message: &str) -> Value {
    fslc_rust::verification_output::render_semantic_error(
        envelope(),
        message,
        None,
        &DiagnosticClass::Message,
        None,
        None,
    )
}

/// Render a spec-loading failure through the class the loader preserved, so
/// every spec-reading command emits the envelope `check` emits for the same
/// file (`kind:"parse"` + `diagnostic_code` + `loc` for a syntax error).
pub(crate) fn spec_load_error_output(error: &SpecLoadError) -> Value {
    fslc_rust::spec_load::render_spec_load_error(envelope(), error)
}

/// Internal name of the action that took the last step of `trace`: the action
/// a violation, CTI, or kill found at that step was produced by.
pub(crate) fn trace_last_action(trace: &[TraceStep]) -> Option<&str> {
    trace
        .last()
        .and_then(|step| step.action.as_ref())
        .map(|action| action.name.as_str())
}
