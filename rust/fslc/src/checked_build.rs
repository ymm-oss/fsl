// SPDX-License-Identifier: Apache-2.0

//! Checked-source admission after Core typing, preserving init diagnostic order.

use fsl_core::{KernelModel, KernelSpec, ModelError};

use crate::spec_load::{SemanticDiagnostic, SpecLoadError};
use crate::verification_output::DiagnosticClass;

#[derive(Debug)]
pub enum CheckedBuildError {
    Model(Box<ModelError>),
    InitOwnership(Box<fsl_runtime::RuntimeError>),
}

impl CheckedBuildError {
    /// Preserve the existing model/runtime diagnostic fields in a checked load.
    #[must_use]
    pub fn into_spec_load_error(self) -> SpecLoadError {
        let diagnostic = match self {
            Self::Model(error) => SemanticDiagnostic::from_model_error(&error),
            Self::InitOwnership(error) => SemanticDiagnostic {
                message: error.message,
                loc: error.span.map(fsl_syntax::Span::python_loc),
                class: DiagnosticClass::Message,
                diagnostic_code: None,
                hint: None,
                quick_fix: None,
            },
        };
        SpecLoadError::Semantic(Box::new(diagnostic))
    }
}

/// Build a checked source model without displacing existing init diagnostics.
///
/// # Errors
/// Returns the original Core error, an existing ownership/read failure ahead
/// of a deferred refusal, or that located init-domain or range-bound refusal.
pub fn build_checked_model(kernel: KernelSpec) -> Result<KernelModel, CheckedBuildError> {
    let origins = kernel.origins().clone();
    let mut model =
        fsl_core::build_model(kernel).map_err(|error| CheckedBuildError::Model(Box::new(error)))?;
    let pending = fsl_core::check_init_statement_domains(&model, &origins)
        .and_then(|()| fsl_core::check_range_admission(&mut model, &origins));
    if let Err(pending) = pending {
        fsl_runtime::check_init_write_ownership(&model)
            .map_err(|error| CheckedBuildError::InitOwnership(Box::new(error)))?;
        return Err(CheckedBuildError::Model(Box::new(pending)));
    }
    Ok(model)
}
