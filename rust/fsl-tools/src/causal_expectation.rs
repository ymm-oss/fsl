// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Explicit expectation lowering for the causal profile (issue #323).
//!
//! An `expectation` is a human-carved observable contract, compiled — under
//! fail-closed conditions only — into an ordinary `leadsTo ... within N`
//! kernel property on the *imported* spec. **Causal claims themselves are
//! never lowered**: `derived_from_claim` is traceability only, and neither a
//! passing nor a violated expectation changes the claim's
//! `formal_assurance` (`not_run`) or `causal_support`.

use fsl_core::{FileResolver, KernelModel, build_surface_model};
use fsl_syntax::{
    ActionItem, CausalExpectationDecl, ExpectationTrigger, Expr, LValue, SpecItem, StateField,
    Statement, SurfaceDocument, TypeExpr, parse_expr,
};

use crate::causal::{CausalError, CausalModel};

/// One compiled expectation, ready for the existing verifier.
pub struct CompiledExpectation {
    pub id: String,
    /// Generated kernel property name (`_expectation_<id>`).
    pub property: String,
    pub derived_from_claim: Option<String>,
    pub clock: String,
    /// `within` in causal timebase units as written.
    pub within_units: u64,
    /// `within` converted to exact kernel ticks.
    pub within_ticks: u64,
    /// The augmented kernel model carrying the generated `leadsTo`.
    pub model: KernelModel,
    pub trigger_kind: &'static str,
}

#[allow(clippy::needless_pass_by_value)]
fn fail(expectation: &CausalExpectationDecl, message: String) -> CausalError {
    CausalError {
        kind: "causal_expectation_invalid",
        message: format!("expectation '{}': {message}", expectation.id),
        line: expectation.id_span.start.line,
        column: expectation.id_span.start.column,
    }
}

/// Compile every expectation in `surface` against `model`'s imports.
///
/// # Errors
///
/// Returns the first fail-closed [`CausalError`] (`causal_expectation_invalid`
/// or `causal_unknown_reference`): missing/foreign/fractional clock mapping,
/// unresolved trigger/response, a response that does not type-check in the
/// target spec's state space, or a target that is not a plain kernel spec.
#[allow(clippy::too_many_lines)]
pub fn compile_expectations(
    causal_source: &fsl_syntax::CausalSource,
    model: &CausalModel,
    resolver: &dyn FileResolver,
) -> Result<Vec<CompiledExpectation>, CausalError> {
    let mut compiled = Vec::new();
    for declaration in &causal_source.expectations {
        let trigger = declaration
            .trigger
            .as_ref()
            .ok_or_else(|| fail(declaration, "requires a trigger field".to_owned()))?;
        let (response_alias, response_source, _) = declaration
            .response
            .as_ref()
            .ok_or_else(|| fail(declaration, "requires a response predicate".to_owned()))?;
        let (within_units, _) = declaration
            .within
            .ok_or_else(|| fail(declaration, "requires a within field".to_owned()))?;
        let (clock_name, _) = declaration
            .clock
            .clone()
            .ok_or_else(|| fail(declaration, "requires a named clock reference".to_owned()))?;
        let Some(clock) = model.clocks.get(&clock_name) else {
            return Err(fail(
                declaration,
                format!("references unknown clock '{clock_name}'"),
            ));
        };
        let trigger_alias = match trigger {
            ExpectationTrigger::Action(reference) => &reference.alias,
            ExpectationTrigger::Predicate { alias, .. } => alias,
        };
        for (what, alias) in [("trigger", trigger_alias), ("response", response_alias)] {
            if alias != &clock.kernel_alias {
                return Err(fail(
                    declaration,
                    format!(
                        "{what} targets spec alias '{alias}' but clock '{clock_name}' maps spec '{}'; a clock is never implicitly applied to a different spec",
                        clock.kernel_alias
                    ),
                ));
            }
        }
        if let Some((claim_id, _)) = &declaration.derived_from_claim
            && !model.claims.contains_key(claim_id)
        {
            return Err(fail(
                declaration,
                format!("derived_from_claim references unknown claim '{claim_id}'"),
            ));
        }
        // Exact integer tick conversion: within_units * ticks / units.
        let scaled = within_units.checked_mul(clock.ticks).ok_or_else(|| {
            fail(
                declaration,
                "within overflows the tick conversion".to_owned(),
            )
        })?;
        if scaled % clock.units != 0 {
            return Err(fail(
                declaration,
                format!(
                    "within {within_units} does not convert to an exact integer number of kernel ticks under clock '{clock_name}' ({} tick = {} {}); nothing is rounded",
                    clock.ticks, clock.units, model.timebase
                ),
            ));
        }
        let within_ticks = scaled / clock.units;
        // Re-parse the imported file as a plain kernel surface spec.
        let import = causal_source
            .uses
            .iter()
            .find(|import| &import.alias == trigger_alias)
            .ok_or_else(|| fail(declaration, format!("unknown uses alias '{trigger_alias}'")))?;
        let source = resolver.read(&import.path).map_err(|error| {
            fail(
                declaration,
                format!("cannot read '{}': {}", import.path, error.message),
            )
        })?;
        let parsed = fsl_syntax::parse_document(fsl_syntax::SourceFile::new(&source))
            .map_err(|error| fail(declaration, format!("target spec parse: {error}")))?;
        let SurfaceDocument::Spec(mut spec) = parsed.surface else {
            return Err(fail(
                declaration,
                format!(
                    "expectation lowering requires a plain kernel spec target; '{}' is a different dialect (its lowered state space is not a stable expectation surface)",
                    import.path
                ),
            ));
        };
        let response_expr = parse_expr(response_source)
            .map_err(|error| fail(declaration, format!("response predicate parse: {error}")))?;
        let property = format!("_expectation_{}", declaration.id);
        let span = declaration.id_span;
        let (before, trigger_kind): (Expr, &'static str) = match trigger {
            ExpectationTrigger::Predicate { source, .. } => (
                parse_expr(source).map_err(|error| {
                    fail(declaration, format!("trigger predicate parse: {error}"))
                })?,
                "predicate",
            ),
            ExpectationTrigger::Action(reference) => {
                // Pulse ghost: true exactly in states immediately following the
                // trigger action (every action rewrites it, so BMC steps keep
                // it a one-step pulse; no guard changes, so enabledness and
                // deadlock behavior are untouched).
                let ghost = format!("_expectation_fired_{}", declaration.id);
                let mut found = false;
                for item in &mut spec.items {
                    match item {
                        SpecItem::State(fields) => fields.push(StateField {
                            name: ghost.clone(),
                            ty: TypeExpr::Bool,
                            initializer: None,
                            span,
                            initializer_span: None,
                        }),
                        SpecItem::Init { statements, .. } => {
                            statements.push(assign_bool(&ghost, false, span));
                        }
                        SpecItem::Action { name, items, .. } => {
                            let fires = *name == reference.name;
                            found |= fires;
                            items.push(ActionItem::Statement(assign_bool(&ghost, fires, span)));
                        }
                        _ => {}
                    }
                }
                if !found {
                    return Err(fail(
                        declaration,
                        format!(
                            "trigger action '{}' does not exist in spec '{}'",
                            reference.name, import.path
                        ),
                    ));
                }
                (
                    parse_expr(&ghost).map_err(|error| fail(declaration, error.to_string()))?,
                    "action",
                )
            }
        };
        spec.items.push(SpecItem::LeadsTo {
            name: property.clone(),
            binders: Vec::new(),
            before: Box::new(before),
            after: Box::new(response_expr),
            span,
            meta: None,
            decreases: None,
            within: Some(Box::new(
                parse_expr(&within_ticks.to_string())
                    .map_err(|error| fail(declaration, error.to_string()))?,
            )),
            helpful: Vec::new(),
            annotations: fsl_syntax::Annotations::default(),
        });
        let model = build_surface_model(spec).map_err(|error| {
            fail(
                declaration,
                format!(
                    "generated property does not type-check in the target spec's state space: {} (KPI deltas, averages, and effect sizes are the evidence layer's job, not an expectation's)",
                    error.message
                ),
            )
        })?;
        compiled.push(CompiledExpectation {
            id: declaration.id.clone(),
            property,
            derived_from_claim: declaration
                .derived_from_claim
                .as_ref()
                .map(|(id, _)| id.clone()),
            clock: clock_name,
            within_units,
            within_ticks,
            model,
            trigger_kind,
        });
    }
    Ok(compiled)
}

fn assign_bool(name: &str, value: bool, span: fsl_syntax::Span) -> Statement {
    Statement::Assign {
        target: LValue::Var(name.to_owned()),
        value: Expr::Bool(value),
        span,
    }
}
