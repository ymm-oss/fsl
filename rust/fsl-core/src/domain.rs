// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{
    DomainAggregate, DomainEffect, DomainEvolve, DomainField, DomainLoc, DomainSaga,
    DomainSagaStep, DomainSpec, DomainType, DomainTypeSourceForm, SourcePos, Span, SyntaxExpr,
    SyntaxExprKind, SyntaxTypeExpr, SyntaxTypeExprKind,
};

use crate::{
    CoreError, LoweringStep, OriginChain, OriginId, OriginSite,
    domain_lowering::{
        domain_effect_owns_event, effect_outcome_member, validate_effect_outcome_roles,
    },
};

/// A rendering-time failure located at a span in the original `DomainSpec`.
///
/// This module runs before text serialization, so both the public location and
/// the internal origin chain point at authored domain source rather than the
/// ephemeral Kernel text produced by [`domain_kernel_source`].
fn error_at(message: impl Into<String>, span: Span) -> CoreError {
    CoreError {
        message: message.into(),
        line: span.start.line,
        column: span.start.column,
        origin: Some(Box::new(OriginChain {
            id: OriginId(format!(
                "domain:render-error:{}:{}",
                span.start.offset, span.end.offset
            )),
            dialect: "domain".to_owned(),
            primary: Some(OriginSite {
                source_file: None,
                span: Some(span),
                dialect: "domain".to_owned(),
                declaration_path: Vec::new(),
            }),
            secondary: Vec::new(),
            lowering_steps: vec![LoweringStep {
                kind: "render_domain_kernel_source".to_owned(),
                detail: None,
            }],
            generated: false,
        })),
        name_resolution: false,
    }
}

/// If `type_name` is a top-level `Map<K, V>` application, its key and value
/// type expressions; `None` for every other shape (including a malformed
/// `Map` arity, which falls through to [`Context::default_for_type`]'s
/// generic "unsupported domain type constructor" rejection instead of being
/// treated as a renderable Map here).
fn map_key_value(type_name: &SyntaxTypeExpr) -> Option<(&SyntaxTypeExpr, &SyntaxTypeExpr)> {
    match &type_name.kind {
        SyntaxTypeExprKind::Apply {
            constructor,
            arguments,
        } if constructor.text == "Map" => match arguments.as_slice() {
            [key, value] => Some((key, value)),
            _ => None,
        },
        _ => None,
    }
}

/// [`domain_type_default`]'s result: either a single value a field's
/// initializer would render as `= value`, or -- for a top-level `Map<K, V>`
/// field only -- the dense per-key `forall` init [`domain_kernel_source`]'s
/// state-field loop builds directly, since a bare `Map` has no single
/// default value of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainDefault {
    /// A field initializer's right-hand side, e.g. `none`, `Set {}`, a bare
    /// enum member, or a `value_object` struct literal.
    Value(String),
    /// The per-key value `field[k] = value` renders for every key, the only
    /// supported default for a top-level `Map<K, V>` field
    /// (`field: Map<K, V> = expr;` is always rejected).
    MapPerKey(String),
}

/// The domain-*source* form of the renderer's default value for `type_name`
/// in `domain` -- the same total dispatch [`domain_kernel_source`] uses via
/// `Context::default_for_type` to choose every field's implicit value,
/// exposed so the `implicit_initial_value` warning (issue #731) reports the
/// value this function -- the single owner -- selects, rather than a second
/// hand-rolled copy of the dispatch that can silently drift from it (as the
/// pre-#731 warning did for every container type).
///
/// "Domain-source form" matters for one case: an enum default renders as the
/// bare member name a domain-source initializer would accept (`Pending`),
/// not `domain_kernel_source`'s kernel-scoped mangled identifier
/// (`Status_Pending`), and that bare form is threaded through recursion --
/// into a `value_object`'s struct-literal fields and a `Map`'s per-key value
/// -- so an enum default is never mangled no matter how deep it is nested
/// (issue #731 review, M1: an earlier version of this fix special-cased only
/// a field's own top-level enum type in the warning, which still mangled an
/// enum nested inside a `value_object` or a `Map` value).
///
/// # Errors
///
/// Returns [`CoreError`] wherever `Context::default_for_type` does: an
/// unknown domain type name, an enum with no members, or a type shape it
/// fails closed on -- including a `Map` nested as another `Map`'s value,
/// which is never renderable (`"Map state requires explicit initialization
/// through supported semantics"`).
pub fn domain_type_default(
    domain: &DomainSpec,
    type_name: &SyntaxTypeExpr,
    span: Span,
) -> Result<DomainDefault, CoreError> {
    let context = Context::new(domain);
    if let Some((_key, value)) = map_key_value(type_name) {
        return context
            .default_for_type(value, span, &BTreeMap::new(), DefaultForm::DomainSource)
            .map(DomainDefault::MapPerKey);
    }
    context
        .default_for_type(type_name, span, &BTreeMap::new(), DefaultForm::DomainSource)
        .map(DomainDefault::Value)
}

/// Which identifier form [`Context::default_for_type`] renders an enum
/// member's default as. Every other arm (`Bool`, `Int`, range/external,
/// `Option`, `Set`, `Map`) renders identically in both forms; only the enum
/// leaf differs, so this is threaded through rather than duplicating the
/// whole dispatch per form (issue #731 review, M1).
#[derive(Clone, Copy, PartialEq, Eq)]
enum DefaultForm {
    /// `domain_kernel_source`'s own flat kernel namespace, where enum
    /// members are mangled (`Enum_Member`) to avoid cross-enum collisions.
    Kernel,
    /// The syntax a domain-source field initializer itself accepts, where an
    /// enum member is referenced by its bare declared name.
    DomainSource,
}

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
                    member_spans: Vec::new(),
                    lo: Some(synthetic_num(0, domain.loc)),
                    hi: Some(synthetic_num(1, domain.loc)),
                    fields: Vec::new(),
                    invariants: Vec::new(),
                    source_form: DomainTypeSourceForm::External,
                    span: domain.loc.span(),
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

    /// The field-level default: an explicit `= expr` wins; otherwise falls
    /// through to [`Self::default_for_type`]'s total dispatch on the field's
    /// type. Mirrors `domain_lowering.rs`'s `Resolver::default_value`
    /// (field-level, checks for an explicit default) versus
    /// `Resolver::default_for_type` (type-level, total).
    fn default(
        &self,
        field: &DomainField,
        type_env: &BTreeMap<String, String>,
        form: DefaultForm,
    ) -> Result<String, CoreError> {
        if let Some(value) = &field.default {
            return Ok(self.normalize(
                &value.render_source(),
                None,
                type_env,
                Some(&field.type_name),
                true,
                form,
            ));
        }
        self.default_for_type(&field.type_name, field.span, type_env, form)
    }

    /// Total dispatch over `SyntaxTypeExprKind` -- the same two-variant AST
    /// enum `domain_lowering.rs`'s `LogicalType` is itself derived from --
    /// with no catch-all arm that can produce a value. An unrecognized
    /// `SyntaxTypeExprKind::Apply` constructor reaches the final named arm,
    /// which returns [`CoreError`] (the same "unsupported domain type
    /// constructor" message `domain_lowering.rs`'s
    /// `logical_type`/`surface_type` already use for the identical case), not
    /// a silently rendered `"0"`. `form` selects only how the enum leaf
    /// renders a selected member (see [`DefaultForm`]); every other arm is
    /// identical in both forms and threads `form` through unchanged so a
    /// nested enum -- inside a `value_object`'s fields or a `Map`'s value --
    /// never mangles regardless of how deep it is (issue #731 review, M1).
    fn default_for_type(
        &self,
        type_name: &SyntaxTypeExpr,
        span: Span,
        type_env: &BTreeMap<String, String>,
        form: DefaultForm,
    ) -> Result<String, CoreError> {
        match &type_name.kind {
            SyntaxTypeExprKind::Name(ident) => match ident.text.as_str() {
                "Bool" => Ok("false".to_owned()),
                "Int" => Ok("0".to_owned()),
                other => match self.ty(other) {
                    Some(ty) if ty.kind == "enum" => {
                        let Some(member) = ty.members.first() else {
                            return Err(error_at(
                                format!("enum '{}' has no members", ty.name),
                                span,
                            ));
                        };
                        Ok(match form {
                            DefaultForm::Kernel => self.enum_value(&ty.name, member),
                            DefaultForm::DomainSource => member.clone(),
                        })
                    }
                    Some(ty) if matches!(ty.kind.as_str(), "range" | "external") => Ok(ty
                        .lo
                        .as_ref()
                        .map_or_else(|| "0".to_owned(), SyntaxExpr::render_source)),
                    Some(ty) if ty.kind == "value_object" => Ok(format!(
                        "{} {{ {} }}",
                        ty.name,
                        ty.fields
                            .iter()
                            .map(|field| Ok(format!(
                                "{}: {}",
                                field.name,
                                self.default(field, type_env, form)?
                            )))
                            .collect::<Result<Vec<_>, CoreError>>()?
                            .join(", ")
                    )),
                    // Only "enum" | "range" | "external" | "value_object" are
                    // ever constructed (fsl-syntax's domain parser and this
                    // crate's own "external" backfill for referenced names
                    // are the only producers); a fifth kind string reaching
                    // here is unrecognized and must fail closed, not render
                    // "0".
                    Some(ty) => Err(error_at(
                        format!("unsupported domain type kind '{}'", ty.kind),
                        span,
                    )),
                    None => Err(error_at(format!("unknown domain type '{other}'"), span)),
                },
            },
            SyntaxTypeExprKind::Apply {
                constructor,
                arguments,
            } => match (constructor.text.as_str(), arguments.as_slice()) {
                ("Option", [_]) => Ok("none".to_owned()),
                ("Set", [_]) => Ok("Set {}".to_owned()),
                // A bare `Map` default (no top-level state-field forall
                // context) is always rejected, matching
                // `domain_lowering.rs`'s `default_for_type` Map arm exactly:
                // the only supported Map default is the per-key `forall`
                // form `domain_kernel_source`'s state-field loop builds
                // directly, before this function is ever called on the
                // Map's own type.
                ("Map", [_, _]) => Err(error_at(
                    "Map state requires explicit initialization through supported semantics",
                    span,
                )),
                // Includes `Seq` (out of scope: path A already rejects Seq
                // domain state at `surface_type`, before any default is
                // requested) and any unrecognized constructor/arity -- both
                // fail closed here instead of falling through to "0".
                (unsupported_constructor, unsupported_arguments) => Err(error_at(
                    format!(
                        "unsupported domain type constructor '{}'/{}",
                        unsupported_constructor,
                        unsupported_arguments.len()
                    ),
                    span,
                )),
            },
        }
    }

    /// `form` governs only the `target_type`-driven bare-enum-member
    /// mangling below: every kernel-text call site (guards, invariants,
    /// evolve assignments) always passes [`DefaultForm::Kernel`], since
    /// those render directly into `domain_kernel_source`'s output; only
    /// [`Self::default`]'s explicit-default branch threads its caller's
    /// `form` through, so a `value_object` field's own explicit enum
    /// default (e.g. `status: OrderStatus = Draft;`) renders bare when the
    /// whole struct literal is being computed in domain-source form (issue
    /// #731 review, M1 follow-up: this is the second of two enum-mangling
    /// sites -- [`Self::default_for_type`]'s own enum arm is the first --
    /// that must both honor `form` for a nested `value_object` field's
    /// explicit default to stay unmangled).
    #[allow(clippy::too_many_lines)]
    fn normalize(
        &self,
        expression: &str,
        aggregate: Option<&DomainAggregate>,
        type_env: &BTreeMap<String, String>,
        target_type: Option<&str>,
        replace_state: bool,
        form: DefaultForm,
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
                                .map(|piece| format!(
                                    "({})",
                                    self.normalize(
                                        piece,
                                        Some(aggregate),
                                        type_env,
                                        None,
                                        false,
                                        DefaultForm::Kernel,
                                    )
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
            if self.ty(ty).is_some_and(|ty| ty.kind == "enum")
                && let Some(definition) = self.ty(ty)
            {
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
        if form == DefaultForm::Kernel
            && let Some(target) = target_type
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
                    true,
                    DefaultForm::Kernel,
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
            DefaultForm::Kernel,
        );
        format!("{target} = {expression}")
    }));
    output
}

/// Render the declared `evolve` for each event a saga step/timeout/compensation
/// action emits, pairing with `event_assignments`: an action that raises
/// `event_<E>` for an occurring event must apply E's declared evolve in the
/// same action (docs/DESIGN-domain.md's saga step pairing invariant).
fn saga_emit_evolve_lines(context: &Context<'_>, events: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    for event_name in events {
        let Some((aggregate, event)) = context.event(event_name) else {
            continue;
        };
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
                    .find(|item| item.event == *event_name),
                &environment,
            )
            .into_iter()
            .map(|line| format!("  {line}")),
        );
    }
    lines
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
    for event_name in effect.outcome_events() {
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
            Context::status_member(effect, effect_outcome_member(effect, event_name))
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

#[allow(clippy::too_many_lines)]
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
        if domain_effect_owns_event(context.domain, &event_name) {
            continue;
        }
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
        lines.extend(saga_emit_evolve_lines(context, &step.emits));
        lines.push("}".to_owned());
        if let Some(timeout) = &step.timeout_event {
            lines.push(format!("action {action}_timeout() {{"));
            lines.extend(guards.iter().map(|guard| format!("  requires {guard}")));
            lines.extend(
                event_assignments(context.domain, std::slice::from_ref(timeout))
                    .into_iter()
                    .map(|line| format!("  {line}")),
            );
            lines.extend(saga_emit_evolve_lines(
                context,
                std::slice::from_ref(timeout),
            ));
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
        lines.push(format!(
            "  requires {}",
            Context::event_flag(&compensation.after_event)
        ));
        lines.extend(
            event_assignments(context.domain, &compensation.emits)
                .into_iter()
                .map(|line| format!("  {line}")),
        );
        lines.extend(saga_emit_evolve_lines(context, &compensation.emits));
        lines.push("}".to_owned());
    }
    lines
}

/// Render the full executable kernel source for a Functional-DDD document.
///
/// # Errors
///
/// Returns [`CoreError`] when a domain declaration cannot be rendered into a
/// valid executable kernel, including invalid enums and conflicting explicit
/// effect outcome roles.
#[allow(clippy::too_many_lines)]
pub fn domain_kernel_source(domain: &DomainSpec) -> Result<String, CoreError> {
    validate_effect_outcome_roles(domain)?;
    crate::domain_lowering::validate_lowerable_constructs(domain)?;
    let context = Context::new(domain);
    crate::domain_lowering::validate_domain_enums(&context.types)?;
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
            match map_key_value(&field.type_name) {
                Some((key, value)) => {
                    // Mirrors `domain_lowering.rs`'s `expand_domain`
                    // (~line 2182): a top-level Map state field with no
                    // explicit default lowers to a dense per-key `forall`
                    // init, and an explicit whole-Map default is rejected
                    // here rather than rendered, matching path A's
                    // "whole-Map domain defaults are not supported".
                    if field.default.is_some() {
                        return Err(error_at(
                            "whole-Map domain defaults are not supported",
                            field.span,
                        ));
                    }
                    let key_type = match &key.kind {
                        SyntaxTypeExprKind::Name(ident) => ident.text.clone(),
                        SyntaxTypeExprKind::Apply { .. } => {
                            return Err(error_at(
                                "map keys require a scalar or named type",
                                field.span,
                            ));
                        }
                    };
                    let value_default = context.default_for_type(
                        value,
                        field.span,
                        &environment,
                        DefaultForm::Kernel,
                    )?;
                    init.push(format!(
                        "    forall k: {key_type} {{ {name}[k] = {value_default} }}"
                    ));
                }
                None => {
                    init.push(format!(
                        "    {name} = {}",
                        context.default(field, &environment, DefaultForm::Kernel)?
                    ));
                }
            }
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
                        true,
                        DefaultForm::Kernel,
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
                        true,
                        DefaultForm::Kernel,
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
                    true,
                    DefaultForm::Kernel,
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
    Ok(source)
}
