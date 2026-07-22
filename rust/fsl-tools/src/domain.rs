// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{DomainEffect, DomainSpec, SyntaxExpr};
use serde_json::{Value, json};

use crate::domain_naming::snake;

fn assumptions(domain: &DomainSpec) -> Vec<Value> {
    let mut values = vec![
        json!({"id":"DOMAIN-ASSUME-FINITE-DOMAIN-MODEL","text":"domain IDs and undeclared scalar input types are modeled as finite 0..1 ranges unless declared explicitly"}),
        json!({"id":"DOMAIN-ASSUME-GENERATED-SCAFFOLD","text":"generated Functional DDD code is an implementation scaffold; runtime conformance still requires an adapter/replay evidence boundary"}),
    ];
    if !domain.sagas.is_empty() {
        values.push(json!({"id":"DOMAIN-ASSUME-SAGA-OBSERVED-HISTORY","text":"saga awaits and compensation 'after' clauses are lowered with per-step event observations; durable process history requires runtime replay evidence"}));
    }
    values
}
fn effect_findings(
    domain: &DomainSpec,
    effect: &DomainEffect,
    assumptions: &[Value],
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut add = |kind: &str, severity: &str, rule: &str, witness: Value| {
        out.push(json!({"schema_version":"fsl-domain-finding.v0","fsl":"fsl-domain-effect.v0","result":"violated","kind":kind,"severity":severity,"domain":domain.name,"failed_rule":rule,"guarantee_kind":"structural","evidence":{"kind":"static_check","formal_proof":false},"witness":witness,"repair_candidates":[],"assumptions":assumptions,"effect":effect.name}));
    };
    if effect.irreversible && effect.idempotency_key.is_none() {
        add(
            "irreversible_effect_without_idempotency_key",
            "error",
            "idempotency_for_irreversible_effect",
            json!({"effect":effect.name,"irreversible":true}),
        );
    }
    if effect.async_effect && effect.timeout_event.is_none() && effect.retry.max_attempts.is_none()
    {
        add(
            "pending_effect_without_timeout_or_fallback",
            "warning",
            "timeout_or_fallback_for_pending_effect",
            json!({"effect":effect.name}),
        );
    }
    if effect.irreversible && effect.compensation_events.is_empty() {
        add(
            "missing_compensation_for_irreversible_effect",
            "warning",
            "irreversible_effect_has_compensation_or_acceptance",
            json!({"effect":effect.name,"irreversible":true}),
        );
    }
    if effect.reliable && effect.outbox.is_none() {
        add(
            "reliable_effect_without_outbox_boundary",
            "warning",
            "reliable_effect_has_outbox",
            json!({"effect":effect.name}),
        );
    }
    out
}
fn actions(domain: &DomainSpec) -> Vec<String> {
    let mut out = Vec::new();
    for aggregate in &domain.aggregates {
        for decide in &aggregate.decides {
            out.push(format!(
                "{}_{}",
                snake(&aggregate.name),
                snake(&decide.command)
            ));
        }
    }
    for effect in &domain.effects {
        for outcome in effect.outcome_events() {
            out.push(format!(
                "{}_complete_{}",
                snake(&effect.name),
                snake(outcome)
            ));
        }
        if effect.retry.max_attempts.is_some() {
            out.push(format!("{}_retry", snake(&effect.name)));
        }
    }
    for saga in &domain.sagas {
        for event in saga.steps.iter().flat_map(|step| step.awaits.iter()) {
            out.push(format!(
                "saga_{}_observe_{}",
                snake(&saga.name),
                snake(event)
            ));
        }
        for step in &saga.steps {
            out.push(format!("saga_{}_{}", snake(&saga.name), snake(&step.name)));
            if step.timeout_event.is_some() {
                out.push(format!(
                    "saga_{}_{}_timeout",
                    snake(&saga.name),
                    snake(&step.name)
                ));
            }
        }
        for item in &saga.compensations {
            out.push(format!(
                "saga_{}_compensate_{}_after_{}",
                snake(&saga.name),
                snake(&item.trigger_event),
                snake(&item.after_event)
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Build a specialized domain check envelope around the shared kernel result.
///
/// # Errors
///
/// Returns an error when textual Kernel rendering rejects the domain AST.
pub fn check_domain(domain: &DomainSpec, kernel: &Value) -> Result<Value, fsl_core::CoreError> {
    let kernel_source = domain_kernel_source(domain)?;
    let assumptions = assumptions(domain);
    let findings = domain
        .effects
        .iter()
        .flat_map(|effect| effect_findings(domain, effect, &assumptions))
        .collect::<Vec<_>>();
    let hard = findings
        .iter()
        .any(|finding| finding["severity"] == "error");
    if hard {
        Ok(
            json!({"result":"violated","dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"formal_result":"not_run","findings":findings,"assumptions":assumptions,"kernel_source":kernel_source}),
        )
    } else {
        Ok(
            json!({"result":"verified_under_assumptions","dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"spec":domain.name,"formal_result":"verified","kernel":kernel,"findings":findings,"assumptions":assumptions,"generated_actions":actions(domain)}),
        )
    }
}

/// Emit the stable structural domain analysis projection.
#[must_use]
pub fn analyze_domain(domain: &DomainSpec) -> Value {
    let assumptions = assumptions(domain);
    let findings = domain
        .effects
        .iter()
        .flat_map(|effect| effect_findings(domain, effect, &assumptions))
        .collect::<Vec<_>>();
    json!({"result":"analyzed","dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"profile":domain.implementation_profile,"aggregates":domain.aggregates.iter().map(|a|json!({"name":a.name,"id_type":a.id_type,"state":a.state.iter().map(|f|json!({"name":f.name.text,"type":f.type_name.render_source()})).collect::<Vec<_>>(),"commands":a.commands.iter().map(|x|&x.name).collect::<Vec<_>>(),"events":a.events.iter().map(|x|&x.name).collect::<Vec<_>>(),"errors":a.errors.iter().map(|x|&x.name).collect::<Vec<_>>(),"invariants":a.invariants.iter().map(|x|&x.name.text).collect::<Vec<_>>() })).collect::<Vec<_>>(),"effects":domain.effects.iter().map(|e|json!({"name":e.name,"async":e.async_effect,"reliable":e.reliable,"irreversible":e.irreversible,"handles":e.handles.as_ref().or(e.request_event.as_ref()),"outcomes":e.outcome_events(),"correlation_id":e.correlation_id.as_ref().map(SyntaxExpr::render_source),"idempotency_key":e.idempotency_key.as_ref().map(SyntaxExpr::render_source),"retry_max_attempts":e.retry.max_attempts,"timeout_event":e.timeout_event,"outbox":e.outbox,"inbox":e.inbox})).collect::<Vec<_>>(),"sagas":domain.sagas.iter().map(|s|json!({"name":s.name,"starts_on":s.starts_on,"steps":s.steps.iter().map(|x|json!({"name":x.name,"async":x.async_step,"requires":x.requires.iter().map(SyntaxExpr::render_source).collect::<Vec<_>>(),"emits":x.emits,"awaits_mode":x.awaits_mode,"awaits":x.awaits,"timeout_event":x.timeout_event})).collect::<Vec<_>>(),"compensations":s.compensations.iter().map(|x|json!({"trigger_event":x.trigger_event,"after_event":x.after_event,"emits":x.emits})).collect::<Vec<_>>(),"outboxes":s.outboxes,"inboxes":s.inboxes,"invariants":s.invariants.iter().map(|x|&x.name.text).collect::<Vec<_>>() })).collect::<Vec<_>>(),"findings":findings,"assumptions":assumptions})
}

/// Render a compact executable kernel catalog used by expand and review tools.
///
/// # Errors
///
/// Returns an error when the domain AST has conflicting explicit outcome roles.
pub fn domain_kernel_source(domain: &DomainSpec) -> Result<String, fsl_core::CoreError> {
    fsl_core::domain_kernel_source(domain)
}

/// Generate the native implementation scaffold for a supported target.
///
/// # Errors
///
/// Returns an error for an unsupported target instead of silently treating it
/// as TypeScript.
/// Project only the source-level grouping and spelling that the closed Public
/// Kernel contract cannot represent. Emitters consume the resulting JSON, not
/// this private frontend model.
#[must_use]
pub fn domain_scaffold_metadata(domain: &DomainSpec) -> Value {
    let field = |value: &fsl_syntax::DomainField| json!({"name":value.name.text,"type_name":value.type_name.render_source()});
    json!({
        "$schema":crate::domain_codegen::METADATA_SCHEMA_ID,
        "schema_version":crate::domain_codegen::METADATA_SCHEMA_VERSION,
        "name":domain.name,
        "types":domain.types.iter().map(|value|json!({
            "name":value.name,
            "kind":value.kind,
            "members":value.members,
            "fields":value.fields.iter().map(&field).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "aggregates":domain.aggregates.iter().map(|aggregate|json!({
            "name":aggregate.name,
            "id_type":aggregate.id_type,
            "state":aggregate.state.iter().map(&field).collect::<Vec<_>>(),
            "commands":aggregate.commands.iter().map(|value|json!({
                "name":value.name,
                "inputs":value.inputs.iter().map(&field).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "events":aggregate.events.iter().map(|value|json!({
                "name":value.name,
                "fields":value.fields.iter().map(&field).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "errors":aggregate.errors.iter().map(|value|&value.name).collect::<Vec<_>>(),
            "decides":aggregate.decides.iter().map(|value|json!({
                "command":value.command,
                "requires":value.requires.iter().map(SyntaxExpr::render_source).collect::<Vec<_>>(),
                "rejects":value.rejects.iter().map(|reject|json!({
                    "error":reject.error,
                    "condition":reject.condition.render_source()
                })).collect::<Vec<_>>(),
                "emits":value.emits
            })).collect::<Vec<_>>(),
            "evolves":aggregate.evolves.iter().map(|value|json!({
                "event":value.event,
                "assignments":value.assignments.iter().map(|assignment|json!({
                    "target":assignment.target.render_source(),
                    "value":assignment.value.render_source()
                })).collect::<Vec<_>>()
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "effects":domain.effects.iter().map(|value|json!({
            "name":value.name,
            "handles":value.handles,
            "request_event":value.request_event,
            "outcomes":value.outcome_events(),
            "retry_max_attempts":value.retry.max_attempts
        })).collect::<Vec<_>>(),
        "sagas":domain.sagas.iter().map(|value|json!({
            "name":value.name,
            "starts_on":value.starts_on,
            "steps":value.steps.iter().map(|step|json!({
                "name":step.name,
                "emits":step.emits,
                "timeout_event":step.timeout_event
            })).collect::<Vec<_>>(),
            "compensations":value.compensations.iter().map(|item|json!({
                "trigger_event":item.trigger_event,
                "after_event":item.after_event,
                "emits":item.emits
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })
}

/// Generate target files from Public Kernel v1 plus the versioned compatibility
/// metadata needed for source-level DDD names that lowering does not retain.
///
/// # Errors
///
/// Returns an error when either input contract is incompatible or inconsistent,
/// or when the target is unsupported.
pub fn domain_scaffold(kernel: &Value, metadata: &Value, target: &str) -> Result<Value, String> {
    let files = crate::domain_codegen::generate(kernel, metadata, target)?;
    let domain = kernel
        .pointer("/spec/name")
        .and_then(Value::as_str)
        .ok_or_else(|| "public Kernel root.spec.name must be a string".to_owned())?;
    Ok(json!({
        "result":"generated",
        "dialect":"fsl-domain-effect.v0",
        "domain":domain,
        "target":target,
        "files":files.into_iter().map(|(path, content)| json!({"path":path,"content":content})).collect::<Vec<_>>()
    }))
}

/// Reuse the TypeScript generator for the adapter snippets embedded by
/// `domain testgen`; this prevents a second adapter/effect implementation.
///
/// # Errors
///
/// Returns an error when the Public Kernel or metadata contract is invalid or
/// inconsistent.
pub fn domain_adapter_files(
    kernel: &Value,
    metadata: &Value,
) -> Result<Vec<(String, String)>, String> {
    let mut files = crate::domain_codegen::generate(kernel, metadata, "typescript")?
        .into_iter()
        .filter(|(path, _)| path.ends_with("/adapter.ts") || path == "effects.ts")
        .collect::<Vec<_>>();
    files.sort_by_key(|(path, _)| (path == "effects.ts", path.clone()));
    Ok(files)
}
