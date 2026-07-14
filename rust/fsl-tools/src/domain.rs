// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use fsl_syntax::{DomainEffect, DomainSpec, SyntaxExpr};
use serde_json::{Value, json};

fn snake(value: &str) -> String {
    let mut out = String::new();
    for (i, c) in value.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}
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
        for outcome in &effect.outcomes {
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
#[must_use]
pub fn check_domain(domain: &DomainSpec, kernel: &Value) -> Value {
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
        json!({"result":"violated","dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"formal_result":"not_run","findings":findings,"assumptions":assumptions,"kernel_source":domain_kernel_source(domain)})
    } else {
        json!({"result":"verified_under_assumptions","dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"spec":domain.name,"formal_result":"verified","kernel":kernel,"findings":findings,"assumptions":assumptions,"generated_actions":actions(domain)})
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
    json!({"result":"analyzed","dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"profile":domain.implementation_profile,"aggregates":domain.aggregates.iter().map(|a|json!({"name":a.name,"id_type":a.id_type,"state":a.state.iter().map(|f|json!({"name":f.name.text,"type":f.type_name.render_source()})).collect::<Vec<_>>(),"commands":a.commands.iter().map(|x|&x.name).collect::<Vec<_>>(),"events":a.events.iter().map(|x|&x.name).collect::<Vec<_>>(),"errors":a.errors.iter().map(|x|&x.name).collect::<Vec<_>>(),"invariants":a.invariants.iter().map(|x|&x.name.text).collect::<Vec<_>>() })).collect::<Vec<_>>(),"effects":domain.effects.iter().map(|e|json!({"name":e.name,"async":e.async_effect,"reliable":e.reliable,"irreversible":e.irreversible,"handles":e.handles.as_ref().or(e.request_event.as_ref()),"outcomes":e.outcomes,"correlation_id":e.correlation_id.as_ref().map(SyntaxExpr::render_source),"idempotency_key":e.idempotency_key.as_ref().map(SyntaxExpr::render_source),"retry_max_attempts":e.retry.max_attempts,"timeout_event":e.timeout_event,"outbox":e.outbox,"inbox":e.inbox})).collect::<Vec<_>>(),"sagas":domain.sagas.iter().map(|s|json!({"name":s.name,"starts_on":s.starts_on,"steps":s.steps.iter().map(|x|json!({"name":x.name,"async":x.async_step,"requires":x.requires.iter().map(SyntaxExpr::render_source).collect::<Vec<_>>(),"emits":x.emits,"awaits_mode":x.awaits_mode,"awaits":x.awaits,"timeout_event":x.timeout_event})).collect::<Vec<_>>(),"compensations":s.compensations.iter().map(|x|json!({"trigger_event":x.trigger_event,"after_event":x.after_event,"emits":x.emits})).collect::<Vec<_>>(),"outboxes":s.outboxes,"inboxes":s.inboxes,"invariants":s.invariants.iter().map(|x|&x.name.text).collect::<Vec<_>>() })).collect::<Vec<_>>(),"findings":findings,"assumptions":assumptions})
}

/// Render a compact executable kernel catalog used by expand and review tools.
#[must_use]
pub fn domain_kernel_source(domain: &DomainSpec) -> String {
    fsl_core::domain_kernel_source(domain)
}

fn domain_module_name(name: &str) -> String {
    let mut characters = name.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_ascii_lowercase().to_string() + characters.as_str()
    })
}

/// Render the adapter/effect TypeScript files embedded as comments by domain
/// conformance test generation.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn domain_adapter_files(domain: &DomainSpec) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for aggregate in &domain.aggregates {
        let name = &aggregate.name;
        let module = domain_module_name(name);
        let cases = aggregate
            .commands
            .iter()
            .map(|command| {
                format!(
                    "    case \"{}_{}\":\n      return {{ type: \"{}\", ...(params as any) }} as {name}Command",
                    snake(name), snake(&command.name), command.name
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let content = [
            "// Auto-generated by fslc domain generate. Wire this adapter to fslc testgen output.".to_owned(),
            format!("import {{ decide{name} }} from './decide'"),
            format!("import {{ evolve{name} }} from './evolve'"),
            format!("import {{ type {name}Command, type {name}State }} from '../types'"),
            String::new(),
            format!("export class {name}FslAdapter {{"),
            format!("  private state: {name}State"),
            String::new(),
            format!("  constructor(initialState: {name}State) {{"),
            "    this.state = initialState".to_owned(),
            "  }".to_owned(),
            String::new(),
            format!("  reset(initialState: {name}State): void {{"),
            "    this.state = initialState".to_owned(),
            "  }".to_owned(),
            String::new(),
            "  step(action: string, params: Record<string, unknown>) {".to_owned(),
            "    const command = mapFslActionToCommand(action, params)".to_owned(),
            format!("    const result = decide{name}(this.state, command)"),
            "    if (result.ok) {".to_owned(),
            format!("      this.state = result.value.reduce((s, e) => evolve{name}(s, e), this.state)"),
            "    }".to_owned(),
            "    return result".to_owned(),
            "  }".to_owned(),
            String::new(),
            "  observe() {".to_owned(),
            "    return { ...this.state }".to_owned(),
            "  }".to_owned(),
            "}".to_owned(),
            String::new(),
            format!("export function mapFslActionToCommand(action: string, params: Record<string, unknown>): {name}Command {{"),
            "  switch (action) {".to_owned(),
            cases,
            "    default:".to_owned(),
            "      throw new Error(`Unknown FSL action: ${action}`)".to_owned(),
            "  }".to_owned(),
            "}".to_owned(),
            String::new(),
        ]
        .join("\n");
        files.push((format!("{module}/adapter.ts"), content));
    }
    if !domain.effects.is_empty() {
        let imports = domain
            .aggregates
            .iter()
            .map(|aggregate| format!("{}Event", aggregate.name))
            .collect::<Vec<_>>()
            .join(", ");
        let union = domain
            .aggregates
            .iter()
            .map(|aggregate| format!("{}Event", aggregate.name))
            .collect::<Vec<_>>()
            .join(" | ");
        let mut content = format!(
            "// Auto-generated by fslc domain generate. Effect handlers are outside the pure domain core.\nimport type {{ {imports} }} from './types'\n\n"
        );
        for effect in &domain.effects {
            let request = effect
                .handles
                .as_deref()
                .or(effect.request_event.as_deref())
                .unwrap_or("unknown");
            let outcomes = effect
                .outcomes
                .iter()
                .map(|event| {
                    domain
                        .aggregates
                        .iter()
                        .find(|aggregate| aggregate.events.iter().any(|item| item.name == *event))
                        .map_or_else(
                            || "never".to_owned(),
                            |aggregate| {
                                format!("Extract<{}Event, {{ type: \"{event}\" }}>", aggregate.name)
                            },
                        )
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let _ = write!(
                content,
                "export interface {}Handler {{\n  handle(event: Extract<{union}, {{ type: \"{request}\" }}>): Promise<{outcomes}>\n}}\n",
                effect.name
            );
        }
        files.push(("effects.ts".to_owned(), content));
    }
    files
}

/// Generate the native implementation scaffold for a supported target.
///
/// # Errors
///
/// Returns an error for an unsupported target instead of silently treating it
/// as TypeScript.
pub fn domain_scaffold(domain: &DomainSpec, target: &str) -> Result<Value, String> {
    let files = crate::domain_codegen::generate(domain, target)?;
    Ok(json!({
        "result":"generated",
        "dialect":"fsl-domain-effect.v0",
        "domain":domain.name,
        "target":target,
        "files":files.into_iter().map(|(path, content)| json!({"path":path,"content":content})).collect::<Vec<_>>()
    }))
}
