// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Names a user declares may not contain the separator `__` (#1256,
//! decisions 051).
//!
//! The separator joins the parts of a name a lowering generates: a compose
//! component declaration `alias__name`, a requirements branch split
//! `action__bN`. Those publish as `alias.name` and `action.bN`
//! ([`crate::NameMap`]). A declared `foo__bar` read the same way would publish
//! as `foo.bar` in one output and `foo__bar` in another, so check rejects it
//! before lowering, at the declaration. Referencing a generated name (a
//! refinement mapping onto `submit__b1`) stays allowed: only the positions
//! that introduce a name are walked.
//!
//! The walk runs on the surface document, before any lowering adds generated
//! names, for the documents whose declarations become Kernel names: `spec`,
//! `requirements`, `business` and `compose` (and every compose component, which
//! is a `spec`).

use fsl_syntax::{
    ActionItem, BusinessItem, ComposeItem, Param, ProcessItem, RequirementActionItem,
    RequirementBlockItem, RequirementsItem, SourcePos, Span, SpecItem, SurfaceBusiness,
    SurfaceCompose, SurfaceDocument, SurfaceRequirements, SurfaceSpec, TimeItem,
};

use crate::naming::contains_separator;
use crate::{CoreError, CoreErrorKind, OriginChain, OriginId, OriginSite};

/// One declared name, the position it is declared in, and the span of the
/// declaration when the surface keeps one.
struct Declared<'a> {
    name: &'a str,
    position: &'static str,
    span: Option<Span>,
}

#[derive(Default)]
struct Walk<'a> {
    names: Vec<Declared<'a>>,
}

impl<'a> Walk<'a> {
    fn name(&mut self, name: &'a str, position: &'static str, span: Option<Span>) {
        self.names.push(Declared {
            name,
            position,
            span,
        });
    }

    fn params(&mut self, params: &'a [Param], span: Span) {
        for param in params {
            let (Param::Typed(name, _) | Param::Range(name, ..)) = param;
            self.name(name, "parameter", Some(span));
        }
    }

    fn action_items(&mut self, items: impl IntoIterator<Item = &'a ActionItem>) {
        for item in items {
            if let ActionItem::Let(name, _, span) = item {
                self.name(name, "let binding", Some(*span));
            }
        }
    }

    fn spec_item(&mut self, item: &'a SpecItem) {
        match item {
            SpecItem::Const { name, .. } => self.name(name, "const", None),
            SpecItem::Def {
                name, params, span, ..
            } => {
                self.name(name, "def", Some(*span));
                for (param, _) in params {
                    self.name(param, "def parameter", Some(*span));
                }
            }
            SpecItem::Type { name, .. } => self.name(name, "type", None),
            SpecItem::Enum {
                name,
                members,
                member_spans,
                ..
            } => {
                self.name(name, "enum", None);
                for (index, member) in members.iter().enumerate() {
                    self.name(member, "enum member", member_spans.get(index).copied());
                }
            }
            SpecItem::Struct { name, fields, span } => {
                self.name(name, "struct", Some(*span));
                for (field, _) in fields {
                    self.name(field, "struct field", Some(*span));
                }
            }
            SpecItem::Entity(name, span) => self.name(name, "entity", Some(*span)),
            SpecItem::Number(name, span) => self.name(name, "number", Some(*span)),
            SpecItem::State(fields) => {
                for field in fields {
                    self.name(&field.name, "state variable", Some(field.span));
                }
            }
            SpecItem::Action {
                name,
                params,
                items,
                span,
                ..
            } => {
                self.name(name, "action", Some(*span));
                self.params(params, *span);
                self.action_items(items);
            }
            SpecItem::Invariant { name, span, .. }
            | SpecItem::Trans { name, span, .. }
            | SpecItem::Reachable { name, span, .. }
            | SpecItem::Until { name, span, .. }
            | SpecItem::Unless { name, span, .. }
            | SpecItem::LeadsTo { name, span, .. } => self.name(name, "property", Some(*span)),
            SpecItem::Init { .. } | SpecItem::Terminal { .. } | SpecItem::VerifyBounds { .. } => {}
        }
    }

    fn spec(&mut self, spec: &'a SurfaceSpec) {
        self.name(&spec.name, "specification", None);
        for item in &spec.items {
            self.spec_item(item);
        }
    }

    fn business_item(&mut self, item: &'a BusinessItem) {
        match item {
            BusinessItem::Actor(names, span) => {
                for name in names {
                    self.name(name, "actor", Some(*span));
                }
            }
            BusinessItem::Entity(name, span) => self.name(name, "entity", Some(*span)),
            BusinessItem::Process {
                name,
                fields,
                items,
                span,
            } => {
                for (index, segment) in name.segments().iter().enumerate() {
                    let span = name.segment_spans().get(index).copied().or(Some(*span));
                    self.name(segment, "process", span);
                }
                for field in fields.iter().flat_map(|fields| &fields.fields) {
                    self.name(&field.name, "process field", Some(field.span));
                }
                for item in items {
                    match item {
                        ProcessItem::Stages(stages, span) => {
                            for stage in stages {
                                self.name(stage, "stage", Some(*span));
                            }
                        }
                        ProcessItem::Transition(transition) => {
                            self.name(&transition.name, "transition", Some(transition.span));
                            self.params(&transition.inputs, transition.span);
                        }
                        ProcessItem::Initial(..) => {}
                    }
                }
            }
            BusinessItem::Kpi { name, span, .. } => self.name(name, "kpi", Some(*span)),
            BusinessItem::Control { .. }
            | BusinessItem::Policy { .. }
            | BusinessItem::Goal { .. }
            | BusinessItem::VerifyBounds { .. } => {}
        }
    }

    fn business(&mut self, business: &'a SurfaceBusiness) {
        self.name(&business.name, "business", None);
        for item in &business.items {
            self.business_item(item);
        }
    }

    fn requirement_action(&mut self, action: &'a fsl_syntax::RequirementAction) {
        self.name(&action.name, "action", Some(action.span));
        self.params(&action.params, action.span);
        for item in &action.items {
            if let RequirementActionItem::Action(item) = item {
                self.action_items([item.as_ref()]);
            }
        }
    }

    fn requirements(&mut self, requirements: &'a SurfaceRequirements) {
        self.name(&requirements.name, "requirements", None);
        for item in &requirements.items {
            match item {
                RequirementsItem::Requirement { items, .. } => {
                    for item in items {
                        match item {
                            RequirementBlockItem::Action(action) => self.requirement_action(action),
                            RequirementBlockItem::Property(item) => self.spec_item(item),
                            RequirementBlockItem::Deadline { name, span, .. } => {
                                self.name(name, "deadline", Some(*span));
                            }
                        }
                    }
                }
                RequirementsItem::Process(item) | RequirementsItem::Kpi(item) => {
                    self.business_item(item);
                }
                RequirementsItem::Action(action) => self.requirement_action(action),
                RequirementsItem::Time { items, .. } => {
                    for item in items {
                        if let TimeItem::Age { name, span, .. } = item {
                            self.name(name, "age", Some(*span));
                        }
                    }
                }
                RequirementsItem::Common(item) => self.spec_item(item),
                RequirementsItem::Glossary(_)
                | RequirementsItem::Implements { .. }
                | RequirementsItem::Acceptance { .. }
                | RequirementsItem::Forbidden { .. } => {}
            }
        }
        if let Some(next) = &requirements.next {
            for actor in &next.actors {
                self.name(actor, "actor", None);
            }
            for concept in &next.concepts {
                self.name(&concept.name, "concept", Some(concept.span));
            }
        }
    }

    fn compose(&mut self, compose: &'a SurfaceCompose) {
        self.name(&compose.name, "compose", None);
        for item in &compose.items {
            match item {
                ComposeItem::Use { alias, span, .. } => self.name(alias, "use alias", Some(*span)),
                ComposeItem::SyncAction(action) => {
                    self.name(&action.name, "action", Some(action.span));
                    self.params(&action.params, action.span);
                    self.action_items(&action.items);
                }
                ComposeItem::Common(item) => self.spec_item(item),
                ComposeItem::Internal { .. } => {}
            }
        }
    }
}

/// Reject a declared name that contains `__` in a `spec`, `requirements`,
/// `business` or `compose` document; other documents pass unchanged.
///
/// `source` is the document's text, used to locate a declaration whose surface
/// form keeps no span of its own (a `const`, a `type`, the document name).
///
/// # Errors
///
/// Returns a located [`CoreError`] of kind `name` for the first such
/// declaration in source order.
pub fn reject_declared_separator(
    document: &SurfaceDocument,
    source: &str,
) -> Result<(), CoreError> {
    let mut walk = Walk::default();
    match document {
        SurfaceDocument::Spec(spec) => walk.spec(spec),
        SurfaceDocument::Requirements(requirements) => walk.requirements(requirements),
        SurfaceDocument::Business(business) => walk.business(business),
        SurfaceDocument::Compose(compose) => walk.compose(compose),
        SurfaceDocument::Refinement(_)
        | SurfaceDocument::Governance(_)
        | SurfaceDocument::Db(_)
        | SurfaceDocument::Domain(_)
        | SurfaceDocument::AiComponent(_)
        | SurfaceDocument::Agent(_) => {}
    }
    let first = walk
        .names
        .iter()
        .filter(|declared| contains_separator(declared.name))
        .map(|declared| (locate(source, declared), declared))
        .min_by_key(|(span, _)| (span.start.line, span.start.column));
    let Some((span, declared)) = first else {
        return Ok(());
    };
    Err(CoreError {
        message: separator_message(declared.name, declared.position),
        line: span.start.line,
        column: span.start.column,
        origin: Some(Box::new(OriginChain {
            id: OriginId(format!(
                "naming:separator:{}:{}",
                span.start.offset, span.end.offset
            )),
            dialect: dialect(document).to_owned(),
            primary: Some(OriginSite {
                source_file: None,
                span: Some(span),
                dialect: dialect(document).to_owned(),
                declaration_path: Vec::new(),
            }),
            secondary: Vec::new(),
            lowering_steps: Vec::new(),
            generated: false,
        })),
        kind: CoreErrorKind::NameResolution,
        diagnostic_code: None,
    })
}

fn dialect(document: &SurfaceDocument) -> &'static str {
    match document {
        SurfaceDocument::Requirements(_) => "requirements",
        SurfaceDocument::Business(_) => "business",
        SurfaceDocument::Compose(_) => "compose",
        _ => "kernel",
    }
}

fn separator_message(name: &str, position: &str) -> String {
    format!(
        "{position} name '{name}' contains '__', which is reserved for the names FSL generates \
         (a compose component 'alias__name', a branch 'action__b1') and published with '.' in \
         its place; rename the declaration without '__'"
    )
}

/// The span of `declared`'s name: its first occurrence as a whole identifier
/// at or after the declaration's span, or in the whole source when the
/// declaration keeps no span. The declaration's own span when the name cannot
/// be found (a name a parser normalised), and the first character otherwise.
fn locate(source: &str, declared: &Declared<'_>) -> Span {
    let start = declared.span.map_or(0, |span| span.start.offset);
    let found = source
        .get(start..)
        .and_then(|rest| whole_identifier(rest, declared.name))
        .map(|offset| start + offset);
    match (found, declared.span) {
        (Some(offset), _) => Span {
            start: position(source, offset),
            end: position(source, offset + declared.name.len()),
        },
        (None, Some(span)) => span,
        (None, None) => Span {
            start: position(source, 0),
            end: position(source, 0),
        },
    }
}

fn whole_identifier(haystack: &str, name: &str) -> Option<usize> {
    let is_ident = |ch: char| ch.is_alphanumeric() || ch == '_';
    haystack
        .match_indices(name)
        .map(|(offset, _)| offset)
        .find(|offset| {
            let before = haystack[..*offset].chars().next_back();
            let after = haystack[offset + name.len()..].chars().next();
            !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
        })
}

fn position(source: &str, offset: usize) -> SourcePos {
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, tail)| tail)
        .chars()
        .count()
        + 1;
    SourcePos {
        offset,
        line: u32::try_from(line).unwrap_or(u32::MAX),
        column: u32::try_from(column).unwrap_or(u32::MAX),
    }
}
