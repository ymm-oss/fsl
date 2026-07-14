// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{
    DomainAggregate, DomainEffect, DomainEvolve, DomainField, DomainLoc, DomainSaga,
    DomainSagaStep, DomainSpec, DomainType, SourcePos, Span, SyntaxExpr, SyntaxExprKind,
};

fn synthetic_num(value: i64, loc: DomainLoc) -> SyntaxExpr {
    let position = SourcePos {
        offset: 0,
        line: loc.line,
        column: loc.column,
    };
    SyntaxExpr {
        kind: SyntaxExprKind::Num(value),
        span: Span {
            start: position,
            end: position,
        },
    }
}

fn safe(name: &str) -> String {
    let mut value = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.is_empty() {
        value.push('x');
    }
    if value.starts_with(|character: char| character.is_ascii_digit()) {
        value.insert(0, '_');
    }
    value
}

fn lower_name(name: &str) -> String {
    let mut output = String::new();
    let characters = name.chars().collect::<Vec<_>>();
    for (index, character) in characters.iter().enumerate() {
        let previous = index.checked_sub(1).and_then(|index| characters.get(index));
        let next = characters.get(index + 1);
        if character.is_ascii_uppercase()
            && index > 0
            && (previous.is_some_and(char::is_ascii_lowercase)
                || previous.is_some_and(char::is_ascii_digit)
                || next.is_some_and(char::is_ascii_lowercase))
        {
            output.push('_');
        }
        output.push(character.to_ascii_lowercase());
    }
    safe(&output)
}

fn identifiers(type_ref: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut current = String::new();
    for character in type_ref.chars().chain(std::iter::once(' ')) {
        if character.is_ascii_alphanumeric() || character == '_' {
            current.push(character);
        } else if !current.is_empty() {
            if matches!(
                current.as_str(),
                "Int" | "Bool" | "Map" | "Set" | "Seq" | "Option" | "relation"
            ) {
                current.clear();
            } else {
                output.push(std::mem::take(&mut current));
            }
        }
    }
    output
}

fn replace_identifier(source: &str, from: &str, to: &str) -> String {
    let mut output = String::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let matches = source[index..].starts_with(from);
        let before = index
            .checked_sub(1)
            .and_then(|position| bytes.get(position))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
        let after = bytes
            .get(index + from.len())
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
        if matches && !before && !after {
            output.push_str(to);
            index += from.len();
        } else {
            let character = source[index..].chars().next().expect("valid char boundary");
            output.push(character);
            index += character.len_utf8();
        }
    }
    output
}

fn compact(expression: &str) -> String {
    expression.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone)]
struct Context<'a> {
    domain: &'a DomainSpec,
    types: Vec<DomainType>,
    enum_members: BTreeMap<(String, String), String>,
}

impl<'a> Context<'a> {
    fn new(domain: &'a DomainSpec) -> Self {
        let mut types = domain.types.clone();
        let declared = types
            .iter()
            .map(|ty| ty.name.clone())
            .collect::<BTreeSet<_>>();
        let mut references = BTreeSet::new();
        for aggregate in &domain.aggregates {
            if let Some(id) = &aggregate.id_type {
                references.insert(id.clone());
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
                references.extend(identifiers(&field.type_name));
            }
        }
        for name in references {
            if !declared.contains(&name) {
                types.push(DomainType {
                    name,
                    kind: "external".to_owned(),
                    members: Vec::new(),
                    lo: Some(synthetic_num(0, domain.loc)),
                    hi: Some(synthetic_num(1, domain.loc)),
                    fields: Vec::new(),
                    invariants: Vec::new(),
                    loc: domain.loc,
                });
            }
        }
        let enum_members = types
            .iter()
            .filter(|ty| ty.kind == "enum")
            .flat_map(|ty| {
                ty.members.iter().map(|member| {
                    (
                        (ty.name.clone(), member.clone()),
                        format!("{}_{}", ty.name, member),
                    )
                })
            })
            .collect();
        Self {
            domain,
            types,
            enum_members,
        }
    }

    fn ty(&self, name: &str) -> Option<&DomainType> {
        self.types.iter().find(|ty| ty.name == name)
    }

    fn enum_value(&self, ty: &str, value: &str) -> String {
        self.enum_members
            .get(&(ty.to_owned(), value.to_owned()))
            .cloned()
            .unwrap_or_else(|| value.to_owned())
    }

    fn state_name(aggregate: &DomainAggregate, field: &str) -> String {
        format!("{}_{}", lower_name(&aggregate.name), safe(field))
    }

    fn event_flag(event: &str) -> String {
        format!("event_{}", safe(event))
    }

    fn status_type(effect: &DomainEffect) -> String {
        format!("{}EffectStatus", safe(&effect.name))
    }

    fn status_member(effect: &DomainEffect, member: &str) -> String {
        format!("{}EffectStatus_{member}", safe(&effect.name))
    }

    fn status_var(effect: &DomainEffect) -> String {
        format!("{}_status", lower_name(&effect.name))
    }

    fn attempt_type(effect: &DomainEffect) -> String {
        format!("{}Attempt", safe(&effect.name))
    }

    fn attempt_var(effect: &DomainEffect) -> String {
        format!("{}_attempts", lower_name(&effect.name))
    }

    fn request_event(effect: &DomainEffect) -> Option<&str> {
        effect
            .handles
            .as_deref()
            .or(effect.request_event.as_deref())
    }

    fn correlation_field(effect: &DomainEffect) -> Option<String> {
        effect
            .correlation_id
            .as_ref()
            .map(SyntaxExpr::render_source)
            .map(|value| {
                value
                    .rsplit_once('.')
                    .map_or(value.clone(), |(_, field)| field.to_owned())
            })
    }

    fn event(&self, name: &str) -> Option<(&DomainAggregate, &fsl_syntax::DomainEvent)> {
        self.domain.aggregates.iter().find_map(|aggregate| {
            aggregate
                .events
                .iter()
                .find(|event| event.name == name)
                .map(|event| (aggregate, event))
        })
    }

    fn correlation_type(&self, effect: &DomainEffect) -> Option<String> {
        let field = Self::correlation_field(effect)?;
        let (_, event) = self.event(Self::request_event(effect)?)?;
        event
            .fields
            .iter()
            .find(|candidate| candidate.name.as_str() == field)
            .map(|candidate| candidate.type_name.render_source())
    }

    fn default(&self, field: &DomainField, type_env: &BTreeMap<String, String>) -> String {
        if let Some(value) = &field.default {
            return self.normalize(
                &value.render_source(),
                None,
                type_env,
                Some(&field.type_name),
                true,
            );
        }
        match field.type_name.as_str() {
            "Bool" => "false".to_owned(),
            "Int" => "0".to_owned(),
            _ => match self.ty(&field.type_name) {
                Some(ty) if ty.kind == "enum" => self.enum_value(&ty.name, &ty.members[0]),
                Some(ty) if matches!(ty.kind.as_str(), "range" | "external") => ty
                    .lo
                    .as_ref()
                    .map_or_else(|| "0".to_owned(), SyntaxExpr::render_source),
                Some(ty) if ty.kind == "value_object" => format!(
                    "{} {{ {} }}",
                    ty.name,
                    ty.fields
                        .iter()
                        .map(|field| format!("{}: {}", field.name, self.default(field, type_env)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => "0".to_owned(),
            },
        }
    }

    fn normalize(
        &self,
        expression: &str,
        aggregate: Option<&DomainAggregate>,
        type_env: &BTreeMap<String, String>,
        target_type: Option<&str>,
        replace_state: bool,
    ) -> String {
        let mut output = compact(expression)
            .replace("&&", " and ")
            .replace("||", " or ")
            .replace("->", "=>");
        if let Some(aggregate) = aggregate {
            for decide in &aggregate.decides {
                let pattern = format!("can({})", decide.command);
                if output.contains(&pattern) {
                    let mut pieces = decide
                        .requires
                        .iter()
                        .map(SyntaxExpr::render_source)
                        .collect::<Vec<_>>();
                    pieces.extend(
                        decide
                            .rejects
                            .iter()
                            .map(|reject| format!("not ({})", reject.condition)),
                    );
                    let replacement = if pieces.is_empty() {
                        "true".to_owned()
                    } else {
                        format!(
                            "({})",
                            pieces
                                .iter()
                                .map(|piece| self.normalize(
                                    piece,
                                    Some(aggregate),
                                    type_env,
                                    None,
                                    false
                                ))
                                .collect::<Vec<_>>()
                                .join(" and ")
                        )
                    };
                    output = output.replace(&pattern, &replacement);
                }
            }
        }
        for (variable, ty) in type_env {
            if self.ty(ty).is_some_and(|ty| ty.kind == "enum") {
                if let Some(definition) = self.ty(ty) {
                    for member in &definition.members {
                        let value = self.enum_value(ty, member);
                        output = output.replace(
                            &format!("{variable} == {member}"),
                            &format!("{variable} == {value}"),
                        );
                        output = output.replace(
                            &format!("{variable} != {member}"),
                            &format!("{variable} != {value}"),
                        );
                    }
                }
            }
            let marker = format!("{variable} in [");
            while let Some(start) = output.find(&marker) {
                let values_start = start + marker.len();
                let Some(relative_end) = output[values_start..].find(']') else {
                    break;
                };
                let end = values_start + relative_end;
                let values = output[values_start..end]
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        let value = if self.ty(ty).is_some_and(|ty| ty.kind == "enum") {
                            self.enum_value(ty, value)
                        } else {
                            value.to_owned()
                        };
                        format!("{variable} == {value}")
                    })
                    .collect::<Vec<_>>();
                output.replace_range(start..=end, &format!("({})", values.join(" or ")));
            }
        }
        if let Some(target) = target_type
            && self.ty(target).is_some_and(|ty| ty.kind == "enum")
            && output
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            output = self.enum_value(target, &output);
        }
        if replace_state && let Some(aggregate) = aggregate {
            let mut fields = aggregate.state.iter().collect::<Vec<_>>();
            fields.sort_by_key(|field| std::cmp::Reverse(field.name.len()));
            for field in fields {
                output = replace_identifier(
                    &output,
                    &field.name,
                    &Self::state_name(aggregate, &field.name),
                );
            }
        }
        compact(&output)
    }
}

fn event_assignments(domain: &DomainSpec, emitted: &[String]) -> Vec<String> {
    let emitted = emitted.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let mut names = domain
        .aggregates
        .iter()
        .flat_map(|aggregate| aggregate.events.iter().map(|event| event.name.as_str()))
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
        .into_iter()
        .map(|name| {
            format!(
                "{} = {}",
                Context::event_flag(name),
                if emitted.contains(name) {
                    "true"
                } else {
                    "false"
                }
            )
        })
        .collect()
}

fn evolve_assignments(
    context: &Context<'_>,
    aggregate: &DomainAggregate,
    evolve: Option<&DomainEvolve>,
    type_env: &BTreeMap<String, String>,
) -> Vec<String> {
    let Some(evolve) = evolve else {
        return Vec::new();
    };
    let state = aggregate
        .state
        .iter()
        .map(|field| (field.name.as_str(), field.type_name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut output = evolve
        .requires
        .iter()
        .map(|requirement| {
            format!(
                "requires {}",
                context.normalize(
                    &requirement.render_source(),
                    Some(aggregate),
                    type_env,
                    None,
                    true
                )
            )
        })
        .collect::<Vec<_>>();
    output.extend(evolve.assignments.iter().map(|assignment| {
        let rendered_target = assignment.target.render_source();
        let root = rendered_target
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .next()
            .unwrap_or_default();
        let target = replace_identifier(
            &rendered_target,
            root,
            &Context::state_name(aggregate, root),
        );
        let expression = context.normalize(
            &assignment.value.render_source(),
            Some(aggregate),
            type_env,
            state.get(root).copied(),
            true,
        );
        format!("{target} = {expression}")
    }));
    output
}

fn outcome_status(context: &Context<'_>, effect: &DomainEffect, event: &str) -> String {
    let lowered = event.to_ascii_lowercase();
    let member = if effect.timeout_event.as_deref() == Some(event)
        || lowered.contains("timeout")
        || lowered.contains("timedout")
    {
        "TimedOut"
    } else if effect.failure_event.as_deref() == Some(event) || lowered.contains("fail") {
        "Failed"
    } else if lowered.contains("cancel") {
        "Cancelled"
    } else {
        "Succeeded"
    };
    let _ = context;
    Context::status_member(effect, member)
}

fn render_effect_actions(context: &Context<'_>, effect: &DomainEffect) -> Vec<String> {
    let Some(correlation) = Context::correlation_field(effect) else {
        return Vec::new();
    };
    let Some(correlation_type) = context.correlation_type(effect) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let status = Context::status_var(effect);
    let attempts = Context::attempt_var(effect);
    for event_name in &effect.outcomes {
        let Some((aggregate, event)) = context.event(event_name) else {
            continue;
        };
        let mut parameters = event
            .fields
            .iter()
            .map(|field| format!("{}: {}", field.name, field.type_name))
            .collect::<Vec<_>>();
        if !event
            .fields
            .iter()
            .any(|field| field.name.as_str() == correlation)
        {
            parameters.insert(0, format!("{correlation}: {correlation_type}"));
        }
        let action = format!(
            "{}_complete_{}",
            lower_name(&effect.name),
            lower_name(event_name)
        );
        lines.push(format!("action {action}({}) {{", parameters.join(", ")));
        lines.push(format!(
            "  requires {status}[{correlation}] == {}",
            Context::status_member(effect, "Pending")
        ));
        lines.extend(
            event_assignments(context.domain, std::slice::from_ref(event_name))
                .into_iter()
                .map(|line| format!("  {line}")),
        );
        lines.push(format!(
            "  {status}[{correlation}] = {}",
            outcome_status(context, effect, event_name)
        ));
        let mut environment = aggregate
            .state
            .iter()
            .chain(&event.fields)
            .map(|field| (field.name.text.clone(), field.type_name.render_source()))
            .collect::<BTreeMap<_, _>>();
        environment.insert(correlation.clone(), correlation_type.clone());
        lines.extend(
            evolve_assignments(
                context,
                aggregate,
                aggregate
                    .evolves
                    .iter()
                    .find(|evolve| evolve.event == *event_name),
                &environment,
            )
            .into_iter()
            .map(|line| format!("  {line}")),
        );
        lines.push("}".to_owned());
    }
    if let Some(maximum) = effect.retry.max_attempts {
        let action = format!("{}_retry", lower_name(&effect.name));
        lines.push(format!(
            "action {action}({correlation}: {correlation_type}) {{"
        ));
        lines.push(format!(
            "  requires {status}[{correlation}] == {} or {status}[{correlation}] == {}",
            Context::status_member(effect, "Failed"),
            Context::status_member(effect, "TimedOut")
        ));
        lines.push(format!("  requires {attempts}[{correlation}] < {maximum}"));
        lines.extend(
            event_assignments(context.domain, &[])
                .into_iter()
                .map(|line| format!("  {line}")),
        );
        lines.push(format!(
            "  {status}[{correlation}] = {}",
            Context::status_member(effect, "Pending")
        ));
        lines.push(format!(
            "  {attempts}[{correlation}] = {attempts}[{correlation}] + 1"
        ));
        lines.push("}".to_owned());
    }
    lines
}

fn saga_condition(context: &Context<'_>, expression: &SyntaxExpr) -> String {
    let mut output = compact(&expression.render_source())
        .replace("&&", " and ")
        .replace("||", " or ")
        .replace("->", "=>");
    let mut events = context
        .domain
        .aggregates
        .iter()
        .flat_map(|aggregate| aggregate.events.iter().map(|event| event.name.as_str()))
        .collect::<Vec<_>>();
    events.sort_by_key(|event| std::cmp::Reverse(event.len()));
    for event in events {
        output = replace_identifier(&output, event, &Context::event_flag(event));
    }
    output
}

fn saga_guards(
    context: &Context<'_>,
    saga: &DomainSaga,
    step: &DomainSagaStep,
    first: bool,
) -> Vec<String> {
    let mut guards = Vec::new();
    if first && let Some(event) = &saga.starts_on {
        guards.push(Context::event_flag(event));
    }
    guards.extend(
        step.requires
            .iter()
            .map(|value| saga_condition(context, value)),
    );
    if step.emits.is_empty() && !step.awaits.is_empty() {
        guards.push(format!(
            "({})",
            step.awaits
                .iter()
                .map(|event| Context::event_flag(event))
                .collect::<Vec<_>>()
                .join(if step.awaits_mode == "all" {
                    " and "
                } else {
                    " or "
                })
        ));
    }
    guards
}

fn render_saga_actions(context: &Context<'_>, saga: &DomainSaga) -> Vec<String> {
    let mut lines = Vec::new();
    let mut observed = BTreeSet::new();
    for step in &saga.steps {
        observed.extend(step.awaits.iter().cloned());
    }
    for compensation in &saga.compensations {
        observed.insert(compensation.trigger_event.clone());
        observed.insert(compensation.after_event.clone());
    }
    for event_name in observed {
        let Some((aggregate, event)) = context.event(&event_name) else {
            continue;
        };
        let parameters = event
            .fields
            .iter()
            .map(|field| format!("{}: {}", field.name, field.type_name))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!(
            "action saga_{}_observe_{}({parameters}) {{",
            lower_name(&saga.name),
            lower_name(&event_name)
        ));
        lines.extend(
            event_assignments(context.domain, std::slice::from_ref(&event_name))
                .into_iter()
                .map(|line| format!("  {line}")),
        );
        let environment = aggregate
            .state
            .iter()
            .chain(&event.fields)
            .map(|field| (field.name.text.clone(), field.type_name.render_source()))
            .collect();
        lines.extend(
            evolve_assignments(
                context,
                aggregate,
                aggregate
                    .evolves
                    .iter()
                    .find(|item| item.event == event_name),
                &environment,
            )
            .into_iter()
            .map(|line| format!("  {line}")),
        );
        lines.push("}".to_owned());
    }
    for (index, step) in saga.steps.iter().enumerate() {
        let action = format!("saga_{}_{}", lower_name(&saga.name), lower_name(&step.name));
        let guards = saga_guards(context, saga, step, index == 0);
        lines.push(format!("action {action}() {{"));
        lines.extend(guards.iter().map(|guard| format!("  requires {guard}")));
        lines.extend(
            event_assignments(context.domain, &step.emits)
                .into_iter()
                .map(|line| format!("  {line}")),
        );
        lines.push("}".to_owned());
        if let Some(timeout) = &step.timeout_event {
            lines.push(format!("action {action}_timeout() {{"));
            lines.extend(guards.iter().map(|guard| format!("  requires {guard}")));
            lines.extend(
                event_assignments(context.domain, std::slice::from_ref(timeout))
                    .into_iter()
                    .map(|line| format!("  {line}")),
            );
            lines.push("}".to_owned());
        }
    }
    for compensation in &saga.compensations {
        lines.push(format!(
            "action saga_{}_compensate_{}_after_{}() {{",
            lower_name(&saga.name),
            lower_name(&compensation.trigger_event),
            lower_name(&compensation.after_event)
        ));
        lines.push(format!(
            "  requires {}",
            Context::event_flag(&compensation.trigger_event)
        ));
        lines.extend(
            event_assignments(context.domain, &compensation.emits)
                .into_iter()
                .map(|line| format!("  {line}")),
        );
        lines.push("}".to_owned());
    }
    lines
}

/// Render the full executable kernel source for a Functional-DDD document.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn domain_kernel_source(domain: &DomainSpec) -> String {
    let context = Context::new(domain);
    let mut lines = vec![format!(
        "spec {} \"domain: generated from fsl-domain/fsl-effect\" {{",
        domain.name
    )];
    for ty in &context.types {
        match ty.kind.as_str() {
            "enum" => lines.push(format!(
                "  enum {} {{ {} }}",
                ty.name,
                ty.members
                    .iter()
                    .map(|member| context.enum_value(&ty.name, member))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            "range" | "external" => lines.push(format!(
                "  type {} = {}..{}",
                ty.name,
                ty.lo
                    .as_ref()
                    .map_or_else(|| "0".to_owned(), SyntaxExpr::render_source),
                ty.hi
                    .as_ref()
                    .map_or_else(|| "1".to_owned(), SyntaxExpr::render_source)
            )),
            "value_object" => lines.push(format!(
                "  struct {} {{ {} }}",
                ty.name,
                ty.fields
                    .iter()
                    .map(|field| format!("{}: {}", field.name, field.type_name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            _ => {}
        }
    }
    for effect in &domain.effects {
        lines.push(format!(
            "  enum {} {{ {} }}",
            Context::status_type(effect),
            [
                "NotStarted",
                "Pending",
                "Succeeded",
                "Failed",
                "TimedOut",
                "Cancelled",
                "Compensated",
            ]
            .iter()
            .map(|member| Context::status_member(effect, member))
            .collect::<Vec<_>>()
            .join(", ")
        ));
        lines.push(format!(
            "  type {} = 0..{}",
            Context::attempt_type(effect),
            effect.retry.max_attempts.unwrap_or(1)
        ));
    }
    lines.push("  state {".to_owned());
    let mut state = Vec::new();
    let mut init = Vec::new();
    for aggregate in &domain.aggregates {
        let environment = aggregate
            .state
            .iter()
            .map(|field| (field.name.text.clone(), field.type_name.render_source()))
            .collect();
        for field in &aggregate.state {
            let name = Context::state_name(aggregate, &field.name);
            state.push(format!("    {name}: {},", field.type_name));
            init.push(format!(
                "    {name} = {}",
                context.default(field, &environment)
            ));
        }
    }
    let mut events = domain
        .aggregates
        .iter()
        .flat_map(|aggregate| aggregate.events.iter().map(|event| event.name.as_str()))
        .collect::<Vec<_>>();
    events.sort_unstable();
    for event in events {
        state.push(format!("    {}: Bool,", Context::event_flag(event)));
        init.push(format!("    {} = false", Context::event_flag(event)));
    }
    for effect in &domain.effects {
        if let Some(correlation_type) = context.correlation_type(effect) {
            let status = Context::status_var(effect);
            let attempts = Context::attempt_var(effect);
            state.push(format!(
                "    {status}: Map<{correlation_type}, {}>,",
                Context::status_type(effect)
            ));
            state.push(format!(
                "    {attempts}: Map<{correlation_type}, {}>,",
                Context::attempt_type(effect)
            ));
            init.push(format!(
                "    forall k: {correlation_type} {{ {status}[k] = {} }}",
                Context::status_member(effect, "NotStarted")
            ));
            init.push(format!(
                "    forall k: {correlation_type} {{ {attempts}[k] = 0 }}"
            ));
        }
    }
    lines.extend(state);
    lines.push("  }".to_owned());
    lines.push("  init {".to_owned());
    lines.extend(init);
    lines.push("  }".to_owned());
    let effects_by_request = domain
        .effects
        .iter()
        .filter_map(|effect| Context::request_event(effect).map(|event| (event, effect)))
        .fold(
            BTreeMap::<&str, Vec<&DomainEffect>>::new(),
            |mut map, (event, effect)| {
                map.entry(event).or_default().push(effect);
                map
            },
        );
    for aggregate in &domain.aggregates {
        for decide in &aggregate.decides {
            let Some(command) = aggregate
                .commands
                .iter()
                .find(|item| item.name == decide.command)
            else {
                continue;
            };
            let parameters = command
                .inputs
                .iter()
                .map(|field| format!("{}: {}", field.name, field.type_name))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!(
                "  action {}_{}({parameters}) {{",
                lower_name(&aggregate.name),
                lower_name(&command.name)
            ));
            let environment = aggregate
                .state
                .iter()
                .chain(&command.inputs)
                .map(|field| (field.name.text.clone(), field.type_name.render_source()))
                .collect::<BTreeMap<_, _>>();
            for requirement in &decide.requires {
                lines.push(format!(
                    "    requires {}",
                    context.normalize(
                        &requirement.render_source(),
                        Some(aggregate),
                        &environment,
                        None,
                        true
                    )
                ));
            }
            for reject in &decide.rejects {
                lines.push(format!(
                    "    requires not ({})",
                    context.normalize(
                        &reject.condition.render_source(),
                        Some(aggregate),
                        &environment,
                        None,
                        true
                    )
                ));
            }
            for event in &decide.emits {
                for effect in effects_by_request.get(event.as_str()).into_iter().flatten() {
                    if let Some(correlation) = Context::correlation_field(effect)
                        && environment.contains_key(&correlation)
                    {
                        let status = Context::status_var(effect);
                        lines.push(format!(
                            "    requires {status}[{correlation}] != {}",
                            Context::status_member(effect, "Pending")
                        ));
                        lines.push(format!(
                            "    requires {status}[{correlation}] != {}",
                            Context::status_member(effect, "Succeeded")
                        ));
                    }
                }
            }
            lines.extend(
                event_assignments(domain, &decide.emits)
                    .into_iter()
                    .map(|line| format!("    {line}")),
            );
            for event in &decide.emits {
                lines.extend(
                    evolve_assignments(
                        &context,
                        aggregate,
                        aggregate.evolves.iter().find(|item| item.event == *event),
                        &environment,
                    )
                    .into_iter()
                    .map(|line| format!("    {line}")),
                );
                for effect in effects_by_request.get(event.as_str()).into_iter().flatten() {
                    if let Some(correlation) = Context::correlation_field(effect)
                        && environment.contains_key(&correlation)
                    {
                        lines.push(format!(
                            "    {}[{correlation}] = {}",
                            Context::status_var(effect),
                            Context::status_member(effect, "Pending")
                        ));
                        lines.push(format!(
                            "    {}[{correlation}] = 1",
                            Context::attempt_var(effect)
                        ));
                    }
                }
            }
            lines.push("  }".to_owned());
        }
    }
    for effect in &domain.effects {
        lines.extend(
            render_effect_actions(&context, effect)
                .into_iter()
                .map(|line| format!("  {line}")),
        );
    }
    for saga in &domain.sagas {
        lines.extend(
            render_saga_actions(&context, saga)
                .into_iter()
                .map(|line| format!("  {line}")),
        );
    }
    for aggregate in &domain.aggregates {
        let environment = aggregate
            .state
            .iter()
            .map(|field| (field.name.text.clone(), field.type_name.render_source()))
            .collect();
        for invariant in &aggregate.invariants {
            lines.push(format!(
                "  invariant {}_{} \"DOMAIN-INVARIANT: {}.{}\" {{ {} }}",
                safe(&aggregate.name),
                safe(&invariant.name),
                aggregate.name,
                invariant.name,
                context.normalize(
                    &invariant.expr.render_source(),
                    Some(aggregate),
                    &environment,
                    None,
                    true
                )
            ));
        }
    }
    for saga in &domain.sagas {
        for invariant in &saga.invariants {
            lines.push(format!(
                "  invariant {}_{} \"DOMAIN-SAGA: {}.{}\" {{ {} }}",
                safe(&saga.name),
                safe(&invariant.name),
                saga.name,
                invariant.name,
                saga_condition(&context, &invariant.expr)
            ));
        }
    }
    for effect in &domain.effects {
        if let Some(correlation_type) = context.correlation_type(effect) {
            let status = Context::status_var(effect);
            let succeeded = Context::status_member(effect, "Succeeded");
            lines.push(format!(
                "  trans {}_SuccessSticky \"DOMAIN-EFFECT: {} success is sticky\" {{ forall k: {correlation_type} {{ old({status}[k]) == {succeeded} => {status}[k] == {succeeded} }} }}",
                safe(&effect.name), effect.name
            ));
        }
    }
    lines.push("  terminal { false }".to_owned());
    lines.push("}".to_owned());
    let mut source = lines.join("\n");
    source.push('\n');
    source
}
