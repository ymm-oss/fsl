// SPDX-License-Identifier: Apache-2.0

//! Typed spec-load failure shared by the native CLI and the browser Worker.
//!
//! Both delivery surfaces load a spec through
//! `fsl_core::parse_kernel_source_with_file`, and a failure there must be
//! reported with the same `kind` on both (`AGENTS.md`: "Native CLI and Worker
//! output must preserve the JSON envelope, exit codes, locations, and
//! replayable evidence contract"). The Worker used to classify that same
//! `CoreError` as `kind:"parse"` while native reported `semantics`/`type`
//! (issue #556), because each surface carried its own classification. There is
//! now one classifier -- [`kernel_load_error`] -- and one render dispatch --
//! [`render_spec_load_error`].
//!
//! This is the former `main.rs` block relocated, span work from issue #555
//! included. Native's classification and its locations are unchanged by the
//! move: a `type`/`semantics` diagnostic still carries the span the model bound
//! to the offending construct.

use serde_json::{Map, Value, json};

/// A spec-loading failure that keeps the diagnostic class the frontend
/// determined instead of flattening it to a message string.
///
/// `docs/DESIGN-v1.md` §7.2 fixes the error classification as a closed set and
/// guarantees `loc` for `parse`. Flattening a surface-parse failure into a
/// `String` erased that class, so every command loading a spec through
/// `load_kernel_model` re-classified a syntax error as `semantics` with no
/// `loc`, while `check` -- which runs the surface parser directly -- reported
/// `parse` with a span.
///
/// Issue 555 completed the other half: `Semantic` flattened the typed-model
/// diagnostic to a `String` too, dropping the span the model had already bound
/// to the offending construct, so `type` and `semantics` reported `loc: null`
/// on every command including `check`. It now carries the span alongside the
/// message.
#[derive(Debug)]
pub enum SpecLoadError {
    Io(String),
    Parse(Box<fsl_syntax::ParseError>),
    Semantic(SemanticDiagnostic),
}

/// A typed-model spec-load failure with the location the model recorded for the
/// construct that failed, when it recorded one.
#[derive(Debug)]
pub struct SemanticDiagnostic {
    pub message: String,
    pub loc: Option<Value>,
    /// Whether the failure was resolving a name, which `docs/DESIGN-v1.md`
    /// §7.2 classifies `kind:"name"` (issue 565).
    pub name_resolution: bool,
}

impl SemanticDiagnostic {
    /// A diagnostic raised where no origin is in scope. `loc` stays absent: a
    /// `{line: 0, column: 0}` placeholder would satisfy the schema while
    /// pointing a repair agent at a location that does not exist.
    pub fn unlocated(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            loc: None,
            name_resolution: false,
        }
    }

    /// A diagnostic carrying the origin the frontend bound to the offending
    /// construct. Yields the same `loc` as [`Self::unlocated`] when that origin
    /// has no span, rather than inventing one.
    #[must_use]
    pub fn located(message: impl Into<String>, origin: Option<&fsl_core::OriginChain>) -> Self {
        Self {
            message: message.into(),
            loc: crate::verification_output::origin_loc(origin),
            name_resolution: false,
        }
    }

    /// A core-lowering failure, preserving both its lowering origin and the
    /// frontend's name-resolution classification.
    #[must_use]
    pub fn from_core_error(error: &fsl_core::CoreError) -> Self {
        Self {
            message: error.to_string(),
            loc: crate::verification_output::origin_loc(error.origin.as_deref()),
            name_resolution: error.name_resolution,
        }
    }

    /// A typed-model failure, located by the span it recorded for itself or by
    /// its enclosing construct's lowering origin, and classified by whether it
    /// was resolving a name.
    #[must_use]
    pub fn from_model_error(error: &fsl_core::ModelError) -> Self {
        Self {
            message: error.to_string(),
            loc: crate::verification_output::model_error_loc(error),
            name_resolution: error.name_resolution,
        }
    }
}

impl SpecLoadError {
    /// A `semantics` failure that owns no construct in the specification: a
    /// rejected CLI selection, a whole-document shape mismatch, or a diagnostic
    /// whose span belongs to a different file than the one being reported on.
    pub fn unlocated_semantic(message: impl Into<String>) -> Self {
        Self::Semantic(SemanticDiagnostic::unlocated(message))
    }
}

impl std::fmt::Display for SpecLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => formatter.write_str(message),
            Self::Semantic(diagnostic) => formatter.write_str(&diagnostic.message),
            Self::Parse(error) => write!(formatter, "{error}"),
        }
    }
}

/// Recover the typed surface-parse diagnostic behind a lowering or projection
/// failure that only reports a message.
///
/// The kernel and document entrypoints run the surface parser themselves and
/// report the result as a `CoreError`/projection message, so the class is
/// recovered by re-running the same parser on the failure path only. A compose
/// document whose *component* fails to parse still lowers its own top level
/// successfully and therefore stays `semantics`, exactly as `check` reports it.
#[must_use]
pub fn surface_parse_failure(source: &str) -> Option<fsl_syntax::ParseError> {
    fsl_syntax::parse_document(fsl_syntax::SourceFile::new(source)).err()
}

/// Classify a kernel lowering failure, preserving a surface-parse span.
#[must_use]
pub fn kernel_load_error(source: &str, error: &fsl_core::CoreError) -> SpecLoadError {
    if let Some(parse_error) = surface_parse_failure(source) {
        return SpecLoadError::Parse(Box::new(parse_error));
    }
    // The substituted "spec has no state block" message describes the document
    // as a whole rather than the construct the lowering gate stopped at, so it
    // keeps no location; the original diagnostic keeps whatever origin the
    // frontend recorded.
    if error.message == "top-level document has not reached the kernel lowering gate" {
        return SpecLoadError::Semantic(SemanticDiagnostic::unlocated("spec has no state block"));
    }
    SpecLoadError::Semantic(SemanticDiagnostic::from_core_error(error))
}

/// Render a classified spec-load failure into the public error envelope.
#[must_use]
pub fn render_spec_load_error(mut output: Map<String, Value>, error: &SpecLoadError) -> Value {
    match error {
        SpecLoadError::Io(message) => {
            output.insert("result".to_owned(), json!("error"));
            output.insert("kind".to_owned(), json!("io"));
            output.insert("message".to_owned(), json!(message));
            Value::Object(output)
        }
        SpecLoadError::Parse(error) => {
            crate::frontend_output::render_surface_parse_error(output, error)
        }
        SpecLoadError::Semantic(diagnostic) => crate::verification_output::render_semantic_error(
            output,
            &diagnostic.message,
            diagnostic.loc.clone(),
            diagnostic.name_resolution,
        ),
    }
}
