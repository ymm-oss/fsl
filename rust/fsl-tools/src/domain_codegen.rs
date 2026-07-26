// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native multi-target generators for the fsl-domain frontend.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::domain_naming::snake;
use crate::public_kernel::{public_kernel_v1_root, required_array, required_object, required_str};

pub(crate) const METADATA_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/domain/scaffold-metadata.v1.schema.json";
pub(crate) const METADATA_SCHEMA_VERSION: &str = "1.0.0";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainScaffoldMetadata {
    #[serde(rename = "$schema")]
    schema: String,
    schema_version: String,
    name: String,
    types: Vec<ScaffoldType>,
    aggregates: Vec<ScaffoldAggregate>,
    effects: Vec<ScaffoldEffect>,
    sagas: Vec<ScaffoldSaga>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldType {
    name: String,
    kind: String,
    members: Vec<String>,
    fields: Vec<ScaffoldField>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldField {
    name: String,
    type_name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldCommand {
    name: String,
    inputs: Vec<ScaffoldField>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldEvent {
    name: String,
    fields: Vec<ScaffoldField>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldReject {
    error: String,
    condition: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldDecide {
    command: String,
    requires: Vec<String>,
    rejects: Vec<ScaffoldReject>,
    emits: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldAssignment {
    target: String,
    value: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldEvolve {
    event: String,
    assignments: Vec<ScaffoldAssignment>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldAggregate {
    name: String,
    id_type: Option<String>,
    state: Vec<ScaffoldField>,
    commands: Vec<ScaffoldCommand>,
    events: Vec<ScaffoldEvent>,
    errors: Vec<String>,
    decides: Vec<ScaffoldDecide>,
    evolves: Vec<ScaffoldEvolve>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldEffect {
    name: String,
    handles: Option<String>,
    request_event: Option<String>,
    outcomes: Vec<String>,
    retry_max_attempts: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldSagaStep {
    name: String,
    emits: Vec<String>,
    timeout_event: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldCompensation {
    trigger_event: String,
    after_event: String,
    emits: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldSaga {
    name: String,
    starts_on: Option<String>,
    steps: Vec<ScaffoldSagaStep>,
    compensations: Vec<ScaffoldCompensation>,
}

fn validate_public_kernel(kernel: &Value, metadata: &DomainScaffoldMetadata) -> Result<(), String> {
    let root = public_kernel_v1_root(kernel)?;
    let spec = required_object(
        root.get("spec")
            .ok_or_else(|| "public Kernel root.spec is required".to_owned())?,
        "root.spec",
    )?;
    let name = required_str(spec, "name", "root.spec")?;
    if name != metadata.name {
        return Err(format!(
            "public Kernel spec '{}' does not match domain scaffold metadata '{}'",
            name, metadata.name
        ));
    }
    let source = required_object(
        spec.get("source")
            .ok_or_else(|| "public Kernel root.spec.source is required".to_owned())?,
        "root.spec.source",
    )?;
    let dialect = required_str(source, "dialect", "root.spec.source")?;
    if dialect != "domain" {
        return Err(format!(
            "domain scaffold requires public Kernel dialect 'domain', got '{dialect}'"
        ));
    }

    validate_metadata_names(metadata)?;
    validate_kernel_members(root, metadata)
}

fn validate_kernel_members(
    root: &Map<String, Value>,
    metadata: &DomainScaffoldMetadata,
) -> Result<(), String> {
    let types = object_map(root, "types")?;
    let states = object_map(root, "state")?;
    let actions = object_map(root, "actions")?;
    let require = |items: &BTreeMap<String, &Map<String, Value>>, kind: &str, name: String| {
        items
            .contains_key(&name)
            .then_some(())
            .ok_or_else(|| format!("public Kernel is missing lowered domain {kind} '{name}'"))
    };

    for ty in &metadata.types {
        require(&types, "type", ty.name.clone())?;
    }
    for aggregate in &metadata.aggregates {
        for field in &aggregate.state {
            require(
                &states,
                "state",
                format!("{}_{}", snake(&aggregate.name), snake(&field.name)),
            )?;
        }
        for decide in &aggregate.decides {
            require(
                &actions,
                "action",
                format!("{}_{}", snake(&aggregate.name), snake(&decide.command)),
            )?;
        }
    }
    validate_effect_actions(&actions, metadata)?;
    validate_saga_actions(&actions, metadata)
}

fn validate_effect_actions(
    actions: &BTreeMap<String, &Map<String, Value>>,
    metadata: &DomainScaffoldMetadata,
) -> Result<(), String> {
    for effect in &metadata.effects {
        for outcome in &effect.outcomes {
            let name = format!("{}_complete_{}", snake(&effect.name), snake(outcome));
            if !actions.contains_key(&name) {
                return Err(format!(
                    "public Kernel is missing lowered domain action '{name}'"
                ));
            }
        }
        if effect.retry_max_attempts.is_some() {
            let name = format!("{}_retry", snake(&effect.name));
            if !actions.contains_key(&name) {
                return Err(format!(
                    "public Kernel is missing lowered domain action '{name}'"
                ));
            }
        }
    }
    Ok(())
}

fn validate_saga_actions(
    actions: &BTreeMap<String, &Map<String, Value>>,
    metadata: &DomainScaffoldMetadata,
) -> Result<(), String> {
    for saga in &metadata.sagas {
        let saga_name = snake(&saga.name);
        for step in &saga.steps {
            let name = format!("saga_{saga_name}_{}", snake(&step.name));
            if !actions.contains_key(&name) {
                return Err(format!(
                    "public Kernel is missing lowered domain action '{name}'"
                ));
            }
            if step.timeout_event.is_some() && !actions.contains_key(&format!("{name}_timeout")) {
                return Err(format!(
                    "public Kernel is missing lowered domain action '{name}_timeout'"
                ));
            }
        }
        for item in &saga.compensations {
            let name = format!(
                "saga_{saga_name}_compensate_{}_after_{}",
                snake(&item.trigger_event),
                snake(&item.after_event)
            );
            if !actions.contains_key(&name) {
                return Err(format!(
                    "public Kernel is missing lowered domain action '{name}'"
                ));
            }
        }
    }
    Ok(())
}

fn object_map<'a>(
    root: &'a Map<String, Value>,
    key: &str,
) -> Result<BTreeMap<String, &'a Map<String, Value>>, String> {
    let mut output = BTreeMap::new();
    for item in required_array(root, key, "root")? {
        let object = required_object(item, &format!("root.{key}[]"))?;
        let name = required_str(object, "name", &format!("root.{key}[]"))?.to_owned();
        if output.insert(name.clone(), object).is_some() {
            return Err(format!("public Kernel root.{key} has duplicate '{name}'"));
        }
    }
    Ok(output)
}

fn validate_metadata_names(metadata: &DomainScaffoldMetadata) -> Result<(), String> {
    let require = |kind: &str, name: &str| {
        if name.is_empty() {
            Err(format!(
                "domain scaffold metadata {kind} name must not be empty"
            ))
        } else {
            Ok(())
        }
    };
    require("domain", &metadata.name)?;
    for ty in &metadata.types {
        require("type", &ty.name)?;
        if !matches!(
            ty.kind.as_str(),
            "enum" | "range" | "value_object" | "external"
        ) {
            return Err(format!(
                "unsupported domain scaffold metadata type kind '{}'",
                ty.kind
            ));
        }
        for field in &ty.fields {
            require("type field", &field.name)?;
            require("type field type", &field.type_name)?;
        }
    }
    for aggregate in &metadata.aggregates {
        require("aggregate", &aggregate.name)?;
        for field in &aggregate.state {
            require("state", &field.name)?;
            require("state type", &field.type_name)?;
        }
        for command in &aggregate.commands {
            require("command", &command.name)?;
            for field in &command.inputs {
                require("command input", &field.name)?;
                require("command input type", &field.type_name)?;
            }
        }
        for event in &aggregate.events {
            require("event", &event.name)?;
            for field in &event.fields {
                require("event field", &field.name)?;
                require("event field type", &field.type_name)?;
            }
        }
        for error in &aggregate.errors {
            require("error", error)?;
        }
        for decide in &aggregate.decides {
            require("decide command", &decide.command)?;
        }
        for evolution in &aggregate.evolves {
            require("evolve event", &evolution.event)?;
        }
    }
    for effect in &metadata.effects {
        require("effect", &effect.name)?;
    }
    for saga in &metadata.sagas {
        require("saga", &saga.name)?;
        for step in &saga.steps {
            require("saga step", &step.name)?;
        }
    }
    Ok(())
}

pub(crate) fn generate(
    kernel: &Value,
    metadata: &Value,
    target: &str,
) -> Result<BTreeMap<String, String>, String> {
    let domain = serde_json::from_value::<DomainScaffoldMetadata>(metadata.clone())
        .map_err(|error| format!("invalid domain scaffold metadata: {error}"))?;
    if domain.schema != METADATA_SCHEMA_ID {
        return Err(format!(
            "unsupported domain scaffold metadata $schema '{}'; expected '{METADATA_SCHEMA_ID}'",
            domain.schema
        ));
    }
    if domain.schema_version != METADATA_SCHEMA_VERSION {
        return Err(format!(
            "unsupported domain scaffold metadata schema_version '{}'; expected '{METADATA_SCHEMA_VERSION}'",
            domain.schema_version
        ));
    }
    validate_public_kernel(kernel, &domain)?;
    match target {
        "typescript" => Ok(generate_typescript(&domain)),
        "python" => Ok(BTreeMap::from([(
            "domain_scaffold.py".to_owned(),
            generate_python(&domain),
        )])),
        "kotlin" => Ok(BTreeMap::from([(
            "DomainScaffold.kt".to_owned(),
            generate_kotlin(&domain),
        )])),
        "swift" => Ok(BTreeMap::from([(
            "DomainScaffold.swift".to_owned(),
            generate_swift(&domain),
        )])),
        "rust" => Ok(BTreeMap::from([(
            "domain_scaffold.rs".to_owned(),
            generate_rust(&domain),
        )])),
        _ => Err(format!("unsupported domain generation target: {target}")),
    }
}

fn generate_python(domain: &DomainScaffoldMetadata) -> String {
    let mut lines = vec![
        "# Auto-generated by fslc domain generate. Treat as scaffold.".to_owned(),
        "from dataclasses import dataclass".to_owned(),
        "from typing import Any".to_owned(),
        String::new(),
    ];
    for aggregate in &domain.aggregates {
        lines.push("@dataclass(frozen=True)".to_owned());
        lines.push(format!("class {}State:", aggregate.name));
        lines.extend(
            aggregate
                .state
                .iter()
                .map(|field| format!("    {}: Any", field.name)),
        );
        lines.push(String::new());
        lines.push(format!(
            "def decide_{}(state: {}State, command: dict) -> list[dict]:",
            snake(&aggregate.name),
            aggregate.name
        ));
        lines.push("    match command.get('type'):".to_owned());
        for command in &aggregate.commands {
            let decision = aggregate
                .decides
                .iter()
                .find(|decision| decision.command == command.name);
            lines.push(format!("        case '{}':", command.name));
            if let Some(decision) = decision {
                lines.extend(
                    decision
                        .requires
                        .iter()
                        .map(|requirement| format!("            # requires {requirement}")),
                );
                lines.extend(decision.rejects.iter().map(|rejection| {
                    format!(
                        "            # rejects {} when {}",
                        rejection.error, rejection.condition
                    )
                }));
                let events = decision
                    .emits
                    .iter()
                    .map(|event| format!("{{'type': '{event}'}}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("            return [{events}]"));
            } else {
                lines.push("            return []".to_owned());
            }
        }
        lines.extend([
            "        case _:".to_owned(),
            "            raise ValueError(f\"unknown command: {command.get('type')}\")".to_owned(),
            String::new(),
            format!(
                "def evolve_{}(state: {}State, event: dict) -> {}State:",
                snake(&aggregate.name),
                aggregate.name,
                aggregate.name
            ),
            "    # Fill assignment details from the FSL evolve blocks before production use."
                .to_owned(),
            "    return state".to_owned(),
            String::new(),
        ]);
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn generate_kotlin(domain: &DomainScaffoldMetadata) -> String {
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Treat as scaffold.".to_owned(),
        String::new(),
    ];
    for aggregate in &domain.aggregates {
        let fields = aggregate
            .state
            .iter()
            .map(|field| format!("val {}: Any", field.name))
            .collect::<Vec<_>>()
            .join(", ");
        lines.extend([
            format!("data class {}State({fields})", aggregate.name),
            format!("sealed interface {}Command", aggregate.name),
            format!("sealed interface {}Event", aggregate.name),
            format!("sealed interface {}Error", aggregate.name),
            String::new(),
            format!(
                "fun decide{}(state: {}State, command: {}Command): List<{}Event> =",
                aggregate.name, aggregate.name, aggregate.name, aggregate.name
            ),
            "    when (command) {".to_owned(),
            "        else -> emptyList()".to_owned(),
            "    }".to_owned(),
            String::new(),
            format!(
                "fun evolve{}(state: {}State, event: {}Event): {}State = state",
                aggregate.name, aggregate.name, aggregate.name, aggregate.name
            ),
            String::new(),
        ]);
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn lower_first(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_lowercase().to_string() + chars.as_str()
    })
}

fn generate_swift(domain: &DomainScaffoldMetadata) -> String {
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Treat as scaffold.".to_owned(),
        String::new(),
    ];
    for aggregate in &domain.aggregates {
        lines.push(format!("struct {}State {{", aggregate.name));
        lines.extend(
            aggregate
                .state
                .iter()
                .map(|field| format!("    var {}: Any", field.name)),
        );
        lines.push("}".to_owned());
        lines.push(format!("enum {}Command {{", aggregate.name));
        lines.extend(
            aggregate
                .commands
                .iter()
                .map(|command| format!("    case {}", lower_first(&command.name))),
        );
        lines.push("}".to_owned());
        lines.push(format!("enum {}Event {{", aggregate.name));
        lines.extend(
            aggregate
                .events
                .iter()
                .map(|event| format!("    case {}", lower_first(&event.name))),
        );
        lines.extend([
            "}".to_owned(),
            String::new(),
            format!(
                "func decide{}(state: {}State, command: {}Command) -> [{}Event] {{",
                aggregate.name, aggregate.name, aggregate.name, aggregate.name
            ),
            "    return []".to_owned(),
            "}".to_owned(),
            String::new(),
            format!(
                "func evolve{}(state: {}State, event: {}Event) -> {}State {{",
                aggregate.name, aggregate.name, aggregate.name, aggregate.name
            ),
            "    return state".to_owned(),
            "}".to_owned(),
            String::new(),
        ]);
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn generate_rust(domain: &DomainScaffoldMetadata) -> String {
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Treat as scaffold.".to_owned(),
        String::new(),
    ];
    for aggregate in &domain.aggregates {
        lines.push(format!("pub struct {}State {{", aggregate.name));
        lines.extend(
            aggregate
                .state
                .iter()
                .map(|field| format!("    pub {}: (), ", field.name)),
        );
        lines.push("}".to_owned());
        lines.push(format!("pub enum {}Command {{", aggregate.name));
        lines.extend(
            aggregate
                .commands
                .iter()
                .map(|command| format!("    {},", command.name)),
        );
        lines.push("}".to_owned());
        lines.push(format!("pub enum {}Event {{", aggregate.name));
        lines.extend(
            aggregate
                .events
                .iter()
                .map(|event| format!("    {},", event.name)),
        );
        lines.extend([
            "}".to_owned(),
            format!(
                "pub fn decide_{}(_state: &{}State, _command: {}Command) -> Vec<{}Event> {{",
                snake(&aggregate.name),
                aggregate.name,
                aggregate.name,
                aggregate.name
            ),
            "    Vec::new()".to_owned(),
            "}".to_owned(),
            format!(
                "pub fn evolve_{}(state: {}State, _event: {}Event) -> {}State {{",
                snake(&aggregate.name),
                aggregate.name,
                aggregate.name,
                aggregate.name
            ),
            "    state".to_owned(),
            "}".to_owned(),
            String::new(),
        ]);
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn safe(name: &str) -> String {
    let mut output = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if output.is_empty() {
        output.push('x');
    }
    if output.starts_with(|character: char| character.is_ascii_digit()) {
        output.insert(0, '_');
    }
    output
}

fn camel(name: &str) -> String {
    let safe = safe(name);
    let mut parts = safe.split('_');
    let mut output = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.push(first.to_ascii_uppercase());
            output.push_str(chars.as_str());
        }
    }
    output
}

fn module_name(name: &str) -> String {
    lower_first(&camel(name))
}

fn type_name(raw: &str) -> String {
    if raw == "Int" {
        return "number".to_owned();
    }
    if raw == "Bool" {
        return "boolean".to_owned();
    }
    if let Some(inner) = raw
        .strip_prefix("Option<")
        .and_then(|value| value.strip_suffix('>'))
    {
        return format!("{} | null", type_name(inner.trim()));
    }
    raw.to_owned()
}

fn field_line(field: &ScaffoldField) -> String {
    format!("  {}: {}", camel(&field.name), type_name(&field.type_name))
}

fn type_references(raw: &str) -> BTreeSet<String> {
    raw.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
                && !matches!(*part, "Int" | "Bool" | "Map" | "Set" | "Seq" | "Option")
        })
        .map(str::to_owned)
        .collect()
}

fn external_references(aggregate: &ScaffoldAggregate) -> BTreeSet<String> {
    let mut references = BTreeSet::new();
    if let Some(id_type) = &aggregate.id_type {
        references.insert(id_type.clone());
    }
    for field in aggregate
        .state
        .iter()
        .chain(
            aggregate
                .commands
                .iter()
                .flat_map(|command| &command.inputs),
        )
        .chain(aggregate.events.iter().flat_map(|event| &event.fields))
    {
        references.extend(type_references(&field.type_name));
    }
    references
}

fn generate_typescript(domain: &DomainScaffoldMetadata) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    files.insert("types.ts".to_owned(), emit_types(domain));
    for aggregate in &domain.aggregates {
        let prefix = module_name(&aggregate.name);
        files.insert(format!("{prefix}/decide.ts"), emit_decide(aggregate));
        files.insert(format!("{prefix}/evolve.ts"), emit_evolve(aggregate));
        files.insert(format!("{prefix}/adapter.ts"), emit_adapter(aggregate));
    }
    if !domain.effects.is_empty() {
        files.insert("effects.ts".to_owned(), emit_effects(domain));
    }
    if !domain.sagas.is_empty() {
        files.insert(
            "process-manager.ts".to_owned(),
            emit_process_manager(domain),
        );
    }
    files
}

fn emit_types(domain: &DomainScaffoldMetadata) -> String {
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Treat as scaffold.".to_owned(),
        String::new(),
        "export type Ok<T> = { ok: true; value: T }".to_owned(),
        "export type Err<E> = { ok: false; error: E }".to_owned(),
        "export type Result<T, E> = Ok<T> | Err<E>".to_owned(),
        "export const ok = <T>(value: T): Ok<T> => ({ ok: true, value })".to_owned(),
        "export const err = <E>(error: E): Err<E> => ({ ok: false, error })".to_owned(),
        String::new(),
    ];
    for ty in &domain.types {
        match ty.kind.as_str() {
            "enum" => lines.push(format!(
                "export type {} = {}",
                ty.name,
                ty.members
                    .iter()
                    .map(|member| format!("\"{member}\""))
                    .collect::<Vec<_>>()
                    .join(" | ")
            )),
            "range" => lines.push(format!("export type {} = number", ty.name)),
            "value_object" => {
                lines.push(format!("export type {} = {{", ty.name));
                lines.extend(ty.fields.iter().map(field_line));
                lines.push("}".to_owned());
            }
            _ => {}
        }
        lines.push(String::new());
    }
    let declared = domain
        .types
        .iter()
        .map(|ty| ty.name.as_str())
        .collect::<BTreeSet<_>>();
    for aggregate in &domain.aggregates {
        for reference in external_references(aggregate) {
            if !declared.contains(reference.as_str())
                && !matches!(reference.as_str(), "Int" | "Bool")
            {
                lines.push(format!("export type {reference} = number"));
            }
        }
        lines.push(String::new());
        lines.push(format!("export type {}State = {{", aggregate.name));
        lines.extend(aggregate.state.iter().map(field_line));
        lines.extend(["}".to_owned(), String::new()]);
        lines.push(format!("export type {}Command =", aggregate.name));
        if aggregate.commands.is_empty() {
            lines.push("  never".to_owned());
        } else {
            for command in &aggregate.commands {
                let mut payload = vec![format!("type: \"{}\"", command.name)];
                payload.extend(command.inputs.iter().map(|field| {
                    format!("{}: {}", camel(&field.name), type_name(&field.type_name))
                }));
                lines.push(format!("  | {{ {} }}", payload.join("; ")));
            }
        }
        lines.push(String::new());
        lines.push(format!("export type {}Event =", aggregate.name));
        if aggregate.events.is_empty() {
            lines.push("  never".to_owned());
        } else {
            for event in &aggregate.events {
                let mut payload = vec![format!("type: \"{}\"", event.name)];
                payload.extend(event.fields.iter().map(|field| {
                    format!("{}: {}", camel(&field.name), type_name(&field.type_name))
                }));
                lines.push(format!("  | {{ {} }}", payload.join("; ")));
            }
        }
        lines.push(String::new());
        lines.push(format!("export type {}Error =", aggregate.name));
        if aggregate.errors.is_empty() {
            lines.push("  never".to_owned());
        } else {
            lines.extend(
                aggregate
                    .errors
                    .iter()
                    .map(|error| format!("  | {{ type: \"{error}\" }}")),
            );
        }
        lines.push(String::new());
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn event_construct(event: &ScaffoldEvent, aggregate: &ScaffoldAggregate) -> String {
    let mut fields = vec![format!("type: \"{}\"", event.name)];
    fields.extend(event.fields.iter().map(|field| {
        let name = camel(&field.name);
        format!("{name}: (command as any).{name}")
    }));
    if fields.is_empty() {
        format!("{{ type: \"{}\" }} as {}Event", event.name, aggregate.name)
    } else {
        format!("{{ {} }}", fields.join(", "))
    }
}

fn emit_decide(aggregate: &ScaffoldAggregate) -> String {
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Fill policy gaps before production use."
            .to_owned(),
        format!(
            "import {{ err, ok, type Result, type {}Command, type {}Error, type {}Event, type {}State }} from '../types'",
            aggregate.name, aggregate.name, aggregate.name, aggregate.name
        ),
        String::new(),
        format!("export function decide{}(", aggregate.name),
        format!("  state: {}State,", aggregate.name),
        format!("  command: {}Command", aggregate.name),
        format!(
            "): Result<{}Event[], {}Error> {{",
            aggregate.name, aggregate.name
        ),
        "  switch (command.type) {".to_owned(),
    ];
    for command in &aggregate.commands {
        let decision = aggregate
            .decides
            .iter()
            .find(|decision| decision.command == command.name);
        lines.push(format!("    case \"{}\": {{", command.name));
        let Some(decision) = decision else {
            lines.extend([
                format!(
                    "      return err({{ type: \"Unhandled{}\" }} as {}Error)",
                    command.name, aggregate.name
                ),
                "    }".to_owned(),
            ]);
            continue;
        };
        lines.extend(decision.rejects.iter().map(|rejection| {
            format!(
                "      // rejects {} when {}",
                rejection.error, rejection.condition
            )
        }));
        lines.extend(
            decision
                .requires
                .iter()
                .map(|requirement| format!("      // requires {requirement}")),
        );
        let events = decision
            .emits
            .iter()
            .map(|event_name| {
                aggregate
                    .events
                    .iter()
                    .find(|event| event.name == *event_name)
                    .map_or_else(
                        || format!("{{ type: \"{event_name}\" }} as {}Event", aggregate.name),
                        |event| event_construct(event, aggregate),
                    )
            })
            .collect::<Vec<_>>()
            .join(", ");
        lines.extend([format!("      return ok([{events}])"), "    }".to_owned()]);
    }
    lines.extend([
        "    default:".to_owned(),
        format!(
            "      return err({{ type: 'UnknownCommand' }} as unknown as {}Error)",
            aggregate.name
        ),
        "  }".to_owned(),
        "}".to_owned(),
        String::new(),
    ]);
    lines.join("\n")
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn render_ts_assignment(assignment: &ScaffoldAssignment) -> Option<String> {
    let target = assignment.target.clone();
    if !is_identifier(&target) {
        return None;
    }
    let value = assignment.value.clone();
    let expression = value.trim();
    let expression = if is_identifier(expression) {
        format!("\"{expression}\"")
    } else {
        expression.to_owned()
    };
    Some(format!("{}: {expression}", camel(&target)))
}

fn emit_evolve(aggregate: &ScaffoldAggregate) -> String {
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Keep pure: no I/O, clocks, repositories, or network calls."
            .to_owned(),
        format!(
            "import {{ type {}Event, type {}State }} from '../types'",
            aggregate.name, aggregate.name
        ),
        String::new(),
        format!(
            "export function evolve{}(state: {}State, event: {}Event): {}State {{",
            aggregate.name, aggregate.name, aggregate.name, aggregate.name
        ),
        "  switch (event.type) {".to_owned(),
    ];
    for event in &aggregate.events {
        let evolution = aggregate
            .evolves
            .iter()
            .find(|evolution| evolution.event == event.name);
        lines.push(format!("    case \"{}\":", event.name));
        let Some(evolution) = evolution.filter(|evolution| !evolution.assignments.is_empty())
        else {
            lines.push("      return state".to_owned());
            continue;
        };
        let mut fields = Vec::new();
        for assignment in &evolution.assignments {
            if let Some(rendered) = render_ts_assignment(assignment) {
                fields.push(rendered);
            } else {
                lines.push(format!(
                    "      // {} = {}",
                    assignment.target, assignment.value
                ));
            }
        }
        lines.push(format!(
            "      return {{ ...state, {} }}",
            fields.join(", ")
        ));
    }
    lines.extend([
        "    default:".to_owned(),
        "      return state".to_owned(),
        "  }".to_owned(),
        "}".to_owned(),
        String::new(),
    ]);
    lines.join("\n")
}

fn action_name(name: &str) -> String {
    let mut output = String::new();
    let chars = name.chars().collect::<Vec<_>>();
    for (index, character) in chars.iter().copied().enumerate() {
        if index > 0 && character.is_ascii_uppercase() {
            let previous = chars[index - 1];
            let next_is_lower = chars.get(index + 1).is_some_and(char::is_ascii_lowercase);
            if previous.is_ascii_lowercase() || previous.is_ascii_digit() || next_is_lower {
                output.push('_');
            }
        }
        output.push(character.to_ascii_lowercase());
    }
    output
}

fn emit_adapter(aggregate: &ScaffoldAggregate) -> String {
    let name = &aggregate.name;
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Wire this adapter to fslc testgen output."
            .to_owned(),
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
        format!(
            "export function mapFslActionToCommand(action: string, params: Record<string, unknown>): {name}Command {{"
        ),
        "  switch (action) {".to_owned(),
    ];
    for command in &aggregate.commands {
        lines.extend([
            format!(
                "    case \"{}_{}\":",
                action_name(name),
                action_name(&command.name)
            ),
            format!(
                "      return {{ type: \"{}\", ...(params as any) }} as {name}Command",
                command.name
            ),
        ]);
    }
    lines.extend([
        "    default:".to_owned(),
        "      throw new Error(`Unknown FSL action: ${action}`)".to_owned(),
        "  }".to_owned(),
        "}".to_owned(),
        String::new(),
    ]);
    lines.join("\n")
}

fn all_event_union(domain: &DomainScaffoldMetadata) -> String {
    let union = domain
        .aggregates
        .iter()
        .map(|aggregate| format!("{}Event", aggregate.name))
        .collect::<Vec<_>>()
        .join(" | ");
    if union.is_empty() {
        "never".to_owned()
    } else {
        union
    }
}

fn event_type_for(domain: &DomainScaffoldMetadata, event_name: &str) -> String {
    domain
        .aggregates
        .iter()
        .find(|aggregate| {
            aggregate
                .events
                .iter()
                .any(|event| event.name == event_name)
        })
        .map_or_else(
            || "never".to_owned(),
            |aggregate| {
                format!(
                    "Extract<{}Event, {{ type: \"{event_name}\" }}>",
                    aggregate.name
                )
            },
        )
}

fn emit_effects(domain: &DomainScaffoldMetadata) -> String {
    let imports = domain
        .aggregates
        .iter()
        .map(|aggregate| format!("{}Event", aggregate.name))
        .collect::<Vec<_>>()
        .join(", ");
    let union = all_event_union(domain);
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Effect handlers are outside the pure domain core."
            .to_owned(),
        format!("import type {{ {imports} }} from './types'"),
        String::new(),
    ];
    for effect in &domain.effects {
        let outcomes = effect
            .outcomes
            .iter()
            .map(|event| event_type_for(domain, event))
            .collect::<Vec<_>>()
            .join(" | ");
        let outcomes = if outcomes.is_empty() {
            "never".to_owned()
        } else {
            outcomes
        };
        let request = effect
            .handles
            .as_deref()
            .or(effect.request_event.as_deref())
            .unwrap_or("unknown");
        lines.extend([
            format!("export interface {}Handler {{", effect.name),
            format!(
                "  handle(event: Extract<{union}, {{ type: \"{request}\" }}>): Promise<{outcomes}>"
            ),
            "}".to_owned(),
            String::new(),
        ]);
    }
    lines.join("\n")
}

fn saga_initial_emits(saga: &ScaffoldSaga) -> &[String] {
    saga.steps.first().map_or(&[], |step| step.emits.as_slice())
}

fn event_placeholder(domain: &DomainScaffoldMetadata, event_name: &str) -> String {
    for aggregate in &domain.aggregates {
        if let Some(event) = aggregate
            .events
            .iter()
            .find(|event| event.name == event_name)
        {
            let mut fields = vec![format!("type: \"{}\"", event.name)];
            fields.extend(event.fields.iter().map(|field| {
                format!(
                    "{}: undefined as unknown as {}",
                    camel(&field.name),
                    type_name(&field.type_name)
                )
            }));
            return format!("{{ {} }} as {}Event", fields.join(", "), aggregate.name);
        }
    }
    format!("{{ type: \"{event_name}\" }} as DomainEvent")
}

fn emit_actions(domain: &DomainScaffoldMetadata, events: &[String]) -> String {
    events
        .iter()
        .map(|event| {
            format!(
                "{{ type: \"emit\", event: {} }}",
                event_placeholder(domain, event)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn emit_process_manager(domain: &DomainScaffoldMetadata) -> String {
    let imports = domain
        .aggregates
        .iter()
        .map(|aggregate| format!("{}Event", aggregate.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines = vec![
        "// Auto-generated by fslc domain generate. Process managers coordinate events; keep side effects in handlers."
            .to_owned(),
        format!("import type {{ {imports} }} from './types'"),
        String::new(),
        format!("type DomainEvent = {}", all_event_union(domain)),
        "type ProcessAction = { type: string; event: DomainEvent }".to_owned(),
        String::new(),
    ];
    for saga in &domain.sagas {
        lines.extend([
            format!(
                "export function on{}(event: DomainEvent): ProcessAction[] {{",
                saga.name
            ),
            "  switch (event.type) {".to_owned(),
        ]);
        if let Some(starts_on) = &saga.starts_on {
            lines.push(format!("    case \"{starts_on}\":"));
            let emitted = saga_initial_emits(saga);
            if emitted.is_empty() {
                lines.push("      return []".to_owned());
            } else {
                lines.push(format!("      return [{}]", emit_actions(domain, emitted)));
            }
        }
        for compensation in &saga.compensations {
            lines.push(format!("    case \"{}\":", compensation.trigger_event));
            if compensation.emits.is_empty() {
                lines.push("      return []".to_owned());
            } else {
                lines.extend([
                    format!("      // after {}", compensation.after_event),
                    format!(
                        "      return [{}]",
                        emit_actions(domain, &compensation.emits)
                    ),
                ]);
            }
        }
        lines.extend([
            "    default:".to_owned(),
            "      return []".to_owned(),
            "  }".to_owned(),
            "}".to_owned(),
            String::new(),
        ]);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use fsl_core::{FsResolver, build_model, parse_kernel_source, public_kernel_contract};
    use fsl_syntax::{SurfaceDocument, parse_surface_document};

    fn inputs() -> (Value, Value) {
        let source =
            include_str!("../../fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl");
        let SurfaceDocument::Domain(domain) =
            parse_surface_document(source).expect("parse domain fixture")
        else {
            panic!("expected domain fixture");
        };
        let kernel = parse_kernel_source(source, &FsResolver::new(Path::new(".")))
            .expect("lower domain fixture");
        let model = build_model(kernel.clone()).expect("check domain fixture");
        let public = public_kernel_contract(&kernel, &model, "fixture.fsl", "domain")
            .expect("export public Kernel");
        (public, crate::domain::domain_scaffold_metadata(&domain))
    }

    #[test]
    fn adapter_fails_closed_on_kernel_and_metadata_versions() {
        let (mut kernel, mut metadata) = inputs();
        metadata["schema_version"] = Value::String("2.0.0".to_owned());
        assert!(
            generate(&kernel, &metadata, "typescript")
                .expect_err("reject metadata major")
                .contains("unsupported domain scaffold metadata schema_version")
        );

        let (_, valid_metadata) = inputs();
        kernel["schema_version"] = Value::String("2.0.0".to_owned());
        assert!(
            generate(&kernel, &valid_metadata, "typescript")
                .expect_err("reject Kernel major")
                .contains("unsupported public Kernel schema_version")
        );
    }

    #[test]
    fn adapter_rejects_metadata_that_is_not_grounded_in_the_kernel() {
        let (mut kernel, metadata) = inputs();
        kernel["actions"]
            .as_array_mut()
            .expect("actions")
            .retain(|action| action["name"] != "order_approve");
        assert_eq!(
            generate(&kernel, &metadata, "typescript").expect_err("reject missing action"),
            "public Kernel is missing lowered domain action 'order_approve'"
        );
    }

    #[test]
    fn shared_naming_keeps_analysis_and_codegen_grounding_aligned() {
        let source = r"
domain Naming {
  type Id = 0..0
  type Status = New | Approved
  aggregate Order2__Item {
    id Id
    state { status: Status = New; }
    command Approve2__Now {}
    event Approved2__Now {}
    decide Approve2__Now { emits Approved2__Now }
    evolve Approved2__Now { status = Approved }
  }
}
";
        let SurfaceDocument::Domain(domain) =
            parse_surface_document(source).expect("parse naming fixture")
        else {
            panic!("expected domain fixture");
        };
        let kernel = parse_kernel_source(source, &FsResolver::new(Path::new(".")))
            .expect("lower naming fixture");
        let model = build_model(kernel.clone()).expect("check naming fixture");
        let public = public_kernel_contract(&kernel, &model, "naming.fsl", "domain")
            .expect("export naming fixture");
        let metadata = crate::domain::domain_scaffold_metadata(&domain);
        let expected_action = "order2___item_approve2___now";

        let checked = crate::domain::check_domain(&domain, &public).expect("check domain");
        assert_eq!(
            checked["generated_actions"],
            serde_json::json!([expected_action])
        );

        let generated = generate(&public, &metadata, "python").expect("generate scaffold");
        assert!(generated["domain_scaffold.py"].contains("def decide_order2___item"));

        let mut divergent = public;
        let action = divergent["actions"]
            .as_array_mut()
            .expect("actions")
            .iter_mut()
            .find(|action| action["name"] == expected_action)
            .expect("expected action");
        action["name"] = Value::String("order2__item_approve2__now".to_owned());
        assert_eq!(
            generate(&divergent, &metadata, "python").expect_err("reject naming divergence"),
            "public Kernel is missing lowered domain action 'order2___item_approve2___now'"
        );
    }

    #[test]
    fn adapter_rejects_missing_or_duplicate_kernel_members() {
        let (kernel, metadata) = inputs();
        for (key, name) in [
            ("state", "order_status"),
            ("actions", "order_approve"),
            ("types", "Status"),
        ] {
            let mut changed = kernel.clone();
            changed[key]
                .as_array_mut()
                .expect("Kernel collection")
                .retain(|item| item["name"] != name);
            assert!(
                generate(&changed, &metadata, "typescript").is_err(),
                "missing {key}"
            );

            let mut changed = kernel.clone();
            let duplicate = changed[key][0].clone();
            changed[key]
                .as_array_mut()
                .expect("Kernel collection")
                .push(duplicate);
            assert!(
                generate(&changed, &metadata, "typescript").is_err(),
                "duplicate {key}"
            );
        }
    }

    #[test]
    fn published_metadata_schema_matches_the_adapter_version() {
        let schema: Value = serde_json::from_str(include_str!(
            "../../../schemas/fslc/domain/scaffold-metadata.v1.schema.json"
        ))
        .expect("metadata schema JSON");
        assert_eq!(schema["$id"], METADATA_SCHEMA_ID);
        assert_eq!(
            schema["properties"]["schema_version"]["const"],
            METADATA_SCHEMA_VERSION
        );
    }
}
