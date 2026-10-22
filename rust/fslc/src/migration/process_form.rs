// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Rewriting the published requirements `process` form into the next
//! edition's `entity` / `event` / `rule … allow` members (fsl 5 plan §3 S9,
//! design draft B.10).
//!
//! `process E with f { stages …; initial S; transition t S -> T by A with i
//! when g set f = e covers R }` becomes `entity E { f … lifecycle … }`, one
//! `event t = A t E with i S -> T set f = e covers R` per transition (the
//! verb is the event name, decisions/046 Q13) and, for the guarded
//! transitions, `rule R { allow A t E only when g }`. Nothing is rewritten
//! into a decision table or a concept: that needs a reading of the guards.
//!
//! The lifecycle lines list the stages in the order `stages` declares them
//! (the order of the generated stage enum) and exactly the transitions'
//! edges. The events lower through the same process lowering as the
//! transitions, so the checked kernel is the same; the migration checks it
//! ([`process_form_contract`]) and refuses a file whose kernel would change.
//!
//! A file with a process the next edition cannot express keeps every byte:
//! each such process gets a [`MIGRATE_MANUAL_REQUIRED`] finding naming why.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use fsl_syntax::{
    BusinessItem, Edition, LosslessKind, Param, ParseOptions, ProcessItem, ProcessTransition,
    RequirementsItem, Span, SurfaceDocument, Token, TokenKind, UnstableFeature, source_span,
};
use serde_json::Value;

use super::{MigrationDiagnostic, Origins, Taxonomy, kernel_contract, missing_edition_header};

/// A requirements `process` the migration rewrote into the next edition's
/// `entity` / `event` / `rule` members.
pub const LEGACY_PROCESS_FORM: &str = "legacy_process_form";
/// A requirements `process` the next edition cannot take and the migration
/// cannot rewrite; the finding's `reason` says why. Every input is left as
/// it is.
pub const MIGRATE_MANUAL_REQUIRED: &str = "migrate_manual_required";

/// Two processes name one entity (`claims.Claim` / `legacy.Claim`): an
/// entity has one lifecycle.
const SHARED_ENTITY: &str = "process_entity_shared";
/// A qualified process name: an entity is one name, and the qualified
/// process's generated names differ from the entity's.
const QUALIFIED_PROCESS: &str = "qualified_process_name";
/// A transition with an `@annotation`: an event takes none.
const TRANSITION_ANNOTATION: &str = "transition_annotation";
/// An input named like a field of its entity: an event assigns such an
/// input to the field, while a transition never reads it (its bare name is
/// the field).
const INPUT_SHARES_FIELD: &str = "input_shares_field_name";
/// An input typed with a range (`with n: 0..3`): an event's input takes a
/// type name.
const RANGE_INPUT: &str = "range_typed_input";
/// A guarded transition without `covers`: the guard becomes a rule, and a
/// rule is named by a requirement id.
const GUARD_WITHOUT_REQUIREMENT: &str = "guard_without_requirement";
/// `initial` is not the first of `stages`: an entity's first lifecycle line
/// starts with its initial stage and lists the stages in their enum order.
const INITIAL_NOT_FIRST: &str = "initial_not_first_stage";
/// A field without an initial value: an entity's field needs one.
const FIELD_WITHOUT_INITIAL: &str = "field_without_initial_value";
/// The process's tokens are not laid out as its parsed form says.
const UNRECOGNIZED: &str = "unrecognized_process_layout";
/// The rewritten document does not parse or check.
const CONVERTED_FORM_INVALID: &str = "converted_form_invalid";
/// The rewritten document's checked kernel differs from the original's.
const KERNEL_CHANGED: &str = "kernel_changed";

/// Where a [`MIGRATE_MANUAL_REQUIRED`] finding points: a process by its
/// position among the document's processes, or one of its transitions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Site {
    Process(usize),
    Transition(usize, usize),
}

/// One reason a process cannot be rewritten.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualRequired {
    pub reason: &'static str,
    pub message: String,
    pub site: Site,
}

/// A rewrite of every process of a requirements document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessFormConversion {
    pub processes: Vec<ConvertedProcess>,
}

/// One process, as the members that replace it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvertedProcess {
    /// The byte range of the `process … { … }` block.
    pub start: usize,
    pub end: usize,
    /// The indentation of the `process` line.
    pub indent: String,
    pub entity: String,
    /// Comments of the process's head, `stages` and `initial`.
    pub comments: Vec<String>,
    /// `name: Type = value`.
    pub fields: Vec<String>,
    pub lifecycles: Vec<Vec<String>>,
    pub events: Vec<ConvertedEvent>,
    /// Comments after the last transition.
    pub trailing_comments: Vec<String>,
}

/// One transition, as an event and the guard its rule allows it under.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvertedEvent {
    pub comments: Vec<String>,
    pub name: String,
    pub actor: String,
    /// `i: T, …`, as written.
    pub inputs: Option<String>,
    pub source: String,
    pub target: String,
    /// `f = e`, as written.
    pub sets: Vec<String>,
    /// The requirement id and `ID "text"` of the transition's `covers`.
    pub covers: Option<(String, String)>,
    /// The `when` condition, as written.
    pub guard: Option<String>,
}

/// What [`convert_process_forms`] made of a document. Dropping it drops
/// the refusal that keeps every file as it was.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub enum ProcessFormOutcome {
    /// Nothing to rewrite: not the next edition, not requirements, or no
    /// process.
    Unchanged,
    /// The rewritten document, headed with the edition, and one
    /// [`LEGACY_PROCESS_FORM`] finding per process.
    Converted {
        source: String,
        findings: Vec<MigrationDiagnostic>,
    },
    /// The [`MIGRATE_MANUAL_REQUIRED`] findings; nothing is rewritten.
    ManualRequired(Vec<MigrationDiagnostic>),
}

/// Rewrite the processes of `migrated` (the published-edition document the
/// other migrations produced from `original`) for the next edition, and
/// check the rewrite keeps the kernel. Only the next edition behind its
/// unstable gate rewrites; every other request is [`ProcessFormOutcome::Unchanged`].
/// Findings are located in `original`.
pub fn convert_process_forms(
    original: &str,
    migrated: &str,
    path: &Path,
    edition: Edition,
    options: &ParseOptions,
) -> ProcessFormOutcome {
    if edition != Edition::Fsl5 || !options.unstable().contains(UnstableFeature::Fsl5) {
        return ProcessFormOutcome::Unchanged;
    }
    let locations = Locations::of(original, *options).or_else(|| Locations::of(migrated, *options));
    let locate = |site: Site| {
        locations
            .as_ref()
            .and_then(|locations| locations.span(site))
            .unwrap_or_else(|| source_span(original, 0, 0))
    };
    let manual = |findings: Vec<ManualRequired>| {
        ProcessFormOutcome::ManualRequired(
            findings
                .into_iter()
                .map(|finding| {
                    manual_finding(finding.reason, finding.message, locate(finding.site))
                })
                .collect(),
        )
    };
    let conversion = match plan_process_conversion(migrated, options) {
        Ok(Some(conversion)) => conversion,
        Ok(None) => return ProcessFormOutcome::Unchanged,
        Err(findings) => return manual(findings),
    };
    let converted = conversion.render(migrated, options);
    if let Err((reason, message)) = process_form_contract(migrated, &converted, path, options) {
        return manual(vec![ManualRequired {
            reason,
            message,
            site: Site::Process(0),
        }]);
    }
    let findings = conversion
        .processes
        .iter()
        .enumerate()
        .map(|(index, process)| MigrationDiagnostic {
            code: LEGACY_PROCESS_FORM,
            taxonomy: Taxonomy::UnsupportedInEdition,
            message: format!(
                "this edition's requirements do not take 'process' and 'transition'; process \
                 '{}' becomes an entity with its lifecycle, one event per transition, and a rule \
                 per guarded requirement",
                process.entity
            ),
            span: locate(Site::Process(index)),
            canonical_replacement: format!("entity {} {{ … }}", process.entity),
            machine_applicable: true,
            reason: None,
            edits: Vec::new(),
        })
        .collect();
    ProcessFormOutcome::Converted {
        source: converted,
        findings,
    }
}

/// The migration contract of a process-form rewrite: `before` and `after`
/// check, and their kernels are equal without locations and origins (the
/// events carry their own lowering origins). Where `before` has no
/// `terminal { }`, `after` has the one this edition derives from the
/// lifecycles' final stages (as a business document derives its own), which
/// the process form never derived: that terminal is not compared.
///
/// # Errors
///
/// Returns the reason word and the message when `after` does not parse or
/// check, or its kernel differs from `before`'s.
pub fn process_form_contract(
    before: &str,
    after: &str,
    path: &Path,
    options: &ParseOptions,
) -> Result<(), (&'static str, String)> {
    let before = kernel_contract(before, path, options, Origins::Ignored)
        .map_err(|error| (CONVERTED_FORM_INVALID, error))?;
    let mut after = kernel_contract(after, path, options, Origins::Ignored).map_err(|error| {
        (
            CONVERTED_FORM_INVALID,
            format!("the rewritten entity / event / rule members do not check: {error}"),
        )
    })?;
    ignore_derived_terminal(&before, &mut after);
    if before == after {
        Ok(())
    } else {
        Err((
            KERNEL_CHANGED,
            "the rewritten entity / event / rule members lower to a different kernel than the \
             process; rewrite the process by hand"
                .to_owned(),
        ))
    }
}

/// Where the process form's kernel `before` has no terminal, forget the one
/// the rewritten `after` derives from its lifecycles' final stages, so the
/// rewrite contract compares everything else.
pub fn ignore_derived_terminal(before: &Value, after: &mut Value) {
    if before["kernel"]["properties"]["terminal"].is_null()
        && let Some(properties) = after
            .pointer_mut("/kernel/properties")
            .and_then(Value::as_object_mut)
    {
        properties.insert("terminal".to_owned(), Value::Null);
    }
}

fn manual_finding(reason: &'static str, message: String, span: Span) -> MigrationDiagnostic {
    MigrationDiagnostic {
        code: MIGRATE_MANUAL_REQUIRED,
        taxonomy: Taxonomy::UnsupportedInEdition,
        message,
        span,
        canonical_replacement: "rewrite the process as an entity with its events and rules"
            .to_owned(),
        machine_applicable: false,
        reason: Some(reason),
        edits: Vec::new(),
    }
}

/// The processes of `source`'s requirements as their rewrite: `None` when
/// the document is not published-edition requirements or has no process.
///
/// # Errors
///
/// Returns every reason a process cannot be rewritten.
pub fn plan_process_conversion(
    source: &str,
    options: &ParseOptions,
) -> Result<Option<ProcessFormConversion>, Vec<ManualRequired>> {
    let Ok(SurfaceDocument::Requirements(requirements)) =
        fsl_syntax::parse_surface_document_with(source, options)
    else {
        return Ok(None);
    };
    // A next-edition document's entities are desugared into processes; it
    // has no process form to rewrite.
    if requirements.next.is_some() {
        return Ok(None);
    }
    let processes = requirements
        .items
        .iter()
        .filter_map(|item| match item {
            RequirementsItem::Process(BusinessItem::Process {
                name,
                fields,
                items,
                span,
            }) => Some((name, fields, items, *span)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if processes.is_empty() {
        return Ok(None);
    }
    let Ok(tokens) = fsl_syntax::lex_with(source, options) else {
        return Ok(None);
    };
    let comments = fsl_syntax::lossless_document_with(source, options)
        .nodes()
        .iter()
        .filter(|node| node.kind == LosslessKind::LineComment)
        .map(|node| (node.span.start.offset, node.text.trim_end().to_owned()))
        .collect::<Vec<_>>();
    let reader = Reader {
        source,
        tokens: &tokens,
    };
    let mut entity_count = BTreeMap::<&str, usize>::new();
    for (name, ..) in &processes {
        if let Some(entity) = name.segments().last() {
            *entity_count.entry(entity.as_str()).or_default() += 1;
        }
    }
    let mut converted = Vec::new();
    let mut refusals = Vec::new();
    for (index, (name, fields, items, span)) in processes.into_iter().enumerate() {
        let segments = name.segments();
        let entity = segments.last().map_or("", String::as_str);
        if segments.len() > 1 {
            refusals.push(ManualRequired {
                reason: if entity_count[entity] > 1 {
                    SHARED_ENTITY
                } else {
                    QUALIFIED_PROCESS
                },
                message: if entity_count[entity] > 1 {
                    format!(
                        "process '{}' shares entity '{entity}' with another process; an entity \
                         has one lifecycle, so split it into two entities",
                        segments.join(".")
                    )
                } else {
                    format!(
                        "process '{}' is qualified; an entity is one name, so rename it to \
                         '{entity}' by hand",
                        segments.join(".")
                    )
                },
                site: Site::Process(index),
            });
            continue;
        }
        let process = ProcessSource {
            index,
            entity,
            fields: fields.as_ref().map_or(&[][..], |fields| &fields.fields),
            items,
            span,
            name_end: name.span().end.offset,
        };
        match reader.process(&process, &comments) {
            Ok(process) => converted.push(process),
            Err(mut reasons) => refusals.append(&mut reasons),
        }
    }
    if refusals.is_empty() {
        Ok(Some(ProcessFormConversion {
            processes: converted,
        }))
    } else {
        Err(refusals)
    }
}

impl ProcessFormConversion {
    /// `source` with each process replaced by its members, an `actor` line
    /// before the first, and the edition header.
    #[must_use]
    pub fn render(&self, source: &str, options: &ParseOptions) -> String {
        let mut actors = Vec::<&str>::new();
        for event in self.processes.iter().flat_map(|process| &process.events) {
            if !actors.contains(&event.actor.as_str()) {
                actors.push(&event.actor);
            }
        }
        let mut output = source.to_owned();
        for (index, process) in self.processes.iter().enumerate().rev() {
            let mut lines = Vec::new();
            if index == 0 && !actors.is_empty() {
                lines.push(format!("actor {}", actors.join(", ")));
                lines.push(String::new());
            }
            lines.extend(process.lines());
            let text = lines
                .iter()
                .enumerate()
                .map(|(number, line)| {
                    if line.is_empty() {
                        String::new()
                    } else if number == 0 {
                        line.clone()
                    } else {
                        format!("{}{line}", process.indent)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            output.replace_range(process.start..process.end, &text);
        }
        // Not formatted: the formatter does not lay out this edition's
        // members yet, and the other migrations already formatted the rest.
        if let Some(header) = missing_edition_header(&output, Edition::Fsl5, options)
            && let Ok(headed) = fsl_syntax::apply_source_edits(&output, header.edits)
        {
            output = headed;
        }
        output
    }
}

impl ConvertedProcess {
    /// The members, unindented, one line each (blank lines between).
    fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("entity {} {{", self.entity)];
        lines.extend(self.comments.iter().map(|comment| format!("  {comment}")));
        lines.extend(self.fields.iter().map(|field| format!("  {field}")));
        lines.extend(
            self.lifecycles
                .iter()
                .map(|stages| format!("  lifecycle {}", stages.join(" -> "))),
        );
        lines.push("}".to_owned());
        for event in &self.events {
            lines.push(String::new());
            lines.extend(event.comments.iter().cloned());
            let inputs = event
                .inputs
                .as_ref()
                .map_or_else(String::new, |inputs| format!(" with {inputs}"));
            lines.push(format!(
                "event {} = {} {} {}{inputs} {} -> {}",
                event.name, event.actor, event.name, self.entity, event.source, event.target
            ));
            if !event.sets.is_empty() {
                lines.push(format!("  set {}", event.sets.join(", ")));
            }
            if let Some((_, covers)) = &event.covers {
                lines.push(format!("  covers {covers}"));
            }
        }
        if !self.trailing_comments.is_empty() {
            lines.push(String::new());
            lines.extend(self.trailing_comments.iter().cloned());
        }
        let mut rules = Vec::<(&str, &str, Vec<String>)>::new();
        for event in &self.events {
            let (Some(guard), Some((id, covers))) = (&event.guard, &event.covers) else {
                continue;
            };
            let allow = format!(
                "allow {} {} {} only when {guard}",
                event.actor, event.name, self.entity
            );
            match rules.iter_mut().find(|(rule, ..)| rule == id) {
                Some((.., statements)) => statements.push(allow),
                None => rules.push((id, covers, vec![allow])),
            }
        }
        for (_, header, statements) in rules {
            lines.push(String::new());
            lines.push(format!("rule {header} {{"));
            lines.extend(
                statements
                    .into_iter()
                    .map(|statement| format!("  {statement}")),
            );
            lines.push("}".to_owned());
        }
        lines
    }
}

/// The lifecycle lines of `stages` (initial first) and `edges`: every stage
/// first appears in `stages`' order, and the lines' edges are `edges`.
fn lifecycles(stages: &[String], edges: &[(String, String)]) -> Vec<Vec<String>> {
    let mut lines = Vec::<Vec<String>>::new();
    let mut seen = BTreeSet::<&str>::new();
    let mut emitted = vec![false; edges.len()];
    for stage in stages {
        let entering = edges.iter().enumerate().find(|(index, (from, to))| {
            !emitted[*index] && to == stage && seen.contains(from.as_str())
        });
        let leaving = edges.iter().enumerate().find(|(index, (from, to))| {
            !emitted[*index] && from == stage && (to == stage || seen.contains(to.as_str()))
        });
        match entering.or(leaving) {
            Some((index, (from, to))) => {
                emitted[index] = true;
                lines.push(vec![from.clone(), to.clone()]);
            }
            None => lines.push(vec![stage.clone()]),
        }
        seen.insert(stage);
        for (index, (from, to)) in edges.iter().enumerate() {
            if !emitted[index] && seen.contains(from.as_str()) && seen.contains(to.as_str()) {
                emitted[index] = true;
                lines.push(vec![from.clone(), to.clone()]);
            }
        }
    }
    let mut merged = Vec::<Vec<String>>::new();
    for line in lines {
        match merged.last_mut() {
            Some(previous) if previous.last() == line.first() => {
                previous.extend(line.into_iter().skip(1));
            }
            _ => merged.push(line),
        }
    }
    merged
}

/// A process as parsed, with what locates it in the tokens.
struct ProcessSource<'a> {
    index: usize,
    entity: &'a str,
    fields: &'a [fsl_syntax::ProcessField],
    items: &'a [ProcessItem],
    span: Span,
    name_end: usize,
}

struct Reader<'a> {
    source: &'a str,
    tokens: &'a [Token],
}

/// One process item's tokens: `start..end`.
struct ItemRange {
    start: usize,
    end: usize,
    transition: Option<usize>,
}

impl Reader<'_> {
    fn at(&self, offset: usize) -> Option<usize> {
        self.tokens
            .binary_search_by_key(&offset, |token| token.span.start.offset)
            .ok()
    }

    fn is_symbol(&self, index: usize, symbol: &str) -> bool {
        matches!(self.tokens.get(index).map(|token| &token.kind), Some(TokenKind::Symbol(value)) if value == symbol)
    }

    fn is_ident(&self, index: usize, word: &str) -> bool {
        matches!(self.tokens.get(index).map(|token| &token.kind), Some(TokenKind::Ident(value)) if value == word)
    }

    /// The text of tokens `start..end` as written, a gap holding a line
    /// break or a comment read as one space.
    fn text(&self, start: usize, end: usize) -> String {
        let mut text = String::new();
        for index in start..end {
            let span = self.tokens[index].span;
            if index > start {
                let gap = &self.source[self.tokens[index - 1].span.end.offset..span.start.offset];
                if gap.contains('\n') || gap.contains("//") {
                    text.push(' ');
                } else {
                    text.push_str(gap);
                }
            }
            text.push_str(&self.source[span.start.offset..span.end.offset]);
        }
        text
    }

    /// Tokens `start..end` split at their top-level commas.
    fn split_commas(&self, start: usize, end: usize) -> Vec<(usize, usize)> {
        let mut parts = Vec::new();
        let mut depth = 0usize;
        let mut part = start;
        for index in start..end {
            match &self.tokens[index].kind {
                TokenKind::Symbol(symbol) if ["(", "[", "{"].contains(&symbol.as_str()) => {
                    depth += 1;
                }
                TokenKind::Symbol(symbol) if [")", "]", "}"].contains(&symbol.as_str()) => {
                    depth = depth.saturating_sub(1);
                }
                TokenKind::Symbol(symbol) if symbol == "," && depth == 0 => {
                    parts.push((part, index));
                    part = index + 1;
                }
                _ => {}
            }
        }
        parts.push((part, end));
        parts
    }

    /// The index of the `}` matching the `{` at `open`.
    fn closing(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for index in open..self.tokens.len() {
            if self.is_symbol(index, "{") {
                depth += 1;
            } else if self.is_symbol(index, "}") {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
        }
        None
    }

    fn unrecognized(process: &ProcessSource<'_>, what: &str) -> Vec<ManualRequired> {
        vec![ManualRequired {
            reason: UNRECOGNIZED,
            message: format!(
                "process '{}': {what} is not where its parsed form puts it",
                process.entity
            ),
            site: Site::Process(process.index),
        }]
    }

    #[allow(clippy::too_many_lines)]
    fn process(
        &self,
        process: &ProcessSource<'_>,
        comments: &[(usize, String)],
    ) -> Result<ConvertedProcess, Vec<ManualRequired>> {
        let Some(keyword) = self.at(process.span.start.offset) else {
            return Err(Self::unrecognized(process, "the 'process' keyword"));
        };
        let head_end = process
            .fields
            .last()
            .map_or(process.name_end, |field| field.span.end.offset);
        let Some(open) = (keyword..self.tokens.len())
            .find(|index| self.tokens[*index].span.start.offset >= head_end)
            .filter(|index| self.is_symbol(*index, "{"))
        else {
            return Err(Self::unrecognized(process, "the body's '{'"));
        };
        let Some(close) = self.closing(open) else {
            return Err(Self::unrecognized(process, "the body's '}'"));
        };

        let mut refusals = Vec::new();
        let field_names = process
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        let mut fields = Vec::new();
        for field in process.fields {
            if field.initial.is_none() {
                refusals.push(ManualRequired {
                    reason: FIELD_WITHOUT_INITIAL,
                    message: format!(
                        "field '{}' of process '{}' has no initial value; an entity's field \
                         needs one (`{}: {} = …`)",
                        field.name, process.entity, field.name, field.type_name.name
                    ),
                    site: Site::Process(process.index),
                });
                continue;
            }
            let (Some(start), Some(last)) = (
                self.at(field.span.start.offset),
                self.tokens
                    .iter()
                    .rposition(|token| token.span.end.offset == field.span.end.offset),
            ) else {
                return Err(Self::unrecognized(process, "a field"));
            };
            fields.push(self.text(start, last + 1));
        }

        let mut ranges = Vec::new();
        let mut stages = Vec::new();
        let mut initial = None;
        let mut transitions = Vec::new();
        for item in process.items {
            let (offset, transition) = match item {
                ProcessItem::Stages(names, span) => {
                    stages.clone_from(names);
                    (span.start.offset, None)
                }
                ProcessItem::Initial(name, span) => {
                    initial = Some(name.clone());
                    (span.start.offset, None)
                }
                ProcessItem::Transition(transition) => {
                    transitions.push(transition.as_ref());
                    (transition.span.start.offset, Some(transitions.len() - 1))
                }
            };
            let Some(start) = self.at(offset) else {
                return Err(Self::unrecognized(process, "a process item"));
            };
            ranges.push(ItemRange {
                start,
                end: close,
                transition,
            });
        }
        ranges.sort_by_key(|range| range.start);
        for index in 1..ranges.len() {
            ranges[index - 1].end = ranges[index].start;
        }
        if initial.as_deref() != stages.first().map(String::as_str) {
            refusals.push(ManualRequired {
                reason: INITIAL_NOT_FIRST,
                message: format!(
                    "process '{}' starts in '{}', which is not the first of its stages; an \
                     entity's first lifecycle line starts with its initial stage and the stages \
                     appear in their declared order, so reorder `stages` by hand",
                    process.entity,
                    initial.as_deref().unwrap_or_default()
                ),
                site: Site::Process(process.index),
            });
        }

        let mut events = Vec::new();
        for (number, transition) in transitions.iter().enumerate() {
            let range = ranges
                .iter()
                .find(|range| range.transition == Some(number))
                .expect("every transition has a range");
            match self.event(process, number, transition, range, &field_names) {
                Ok(event) => events.push(event),
                Err(mut reasons) => refusals.append(&mut reasons),
            }
        }
        if !refusals.is_empty() {
            return Err(refusals);
        }

        // Comments: a comment belongs to the item it is in or ends the line
        // of, else to the next item; the head's, `stages`' and `initial`'s
        // go into the entity, those after the last transition after it.
        let mut entity_comments = Vec::new();
        let mut trailing_comments = Vec::new();
        let start = self.tokens[keyword].span.start.offset;
        let end = self.tokens[close].span.end.offset;
        let body = self.tokens[open].span.end.offset;
        for (offset, text) in comments
            .iter()
            .filter(|(offset, _)| (start..end).contains(offset))
        {
            if *offset < body {
                entity_comments.push(text.clone());
                continue;
            }
            let position = ranges
                .partition_point(|range| self.tokens[range.start].span.start.offset <= *offset);
            let owner = position.checked_sub(1).and_then(|current| {
                let range = &ranges[current];
                let last = &self.tokens[range.end - 1].span;
                (*offset < last.end.offset
                    || self.source[last.end.offset..*offset].find('\n').is_none())
                .then_some(current)
            });
            match owner.or((position < ranges.len()).then_some(position)) {
                Some(owner) => match ranges[owner].transition {
                    Some(number) => events[number].comments.push(text.clone()),
                    None => entity_comments.push(text.clone()),
                },
                None if ranges.is_empty() => entity_comments.push(text.clone()),
                None => trailing_comments.push(text.clone()),
            }
        }

        let mut edges = Vec::<(String, String)>::new();
        for event in &events {
            let edge = (event.source.clone(), event.target.clone());
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
        let line_start = self.source[..start]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let indent = &self.source[line_start..start];
        Ok(ConvertedProcess {
            start,
            end,
            indent: if indent.chars().all(char::is_whitespace) {
                indent.to_owned()
            } else {
                String::new()
            },
            entity: process.entity.to_owned(),
            comments: entity_comments,
            fields,
            lifecycles: lifecycles(&stages, &edges),
            events,
            trailing_comments,
        })
    }

    /// `transition t S -> T by A [with i…] [when g] [set f = e…] [covers R
    /// "text"]` within `range`.
    #[allow(clippy::too_many_lines)]
    fn event(
        &self,
        process: &ProcessSource<'_>,
        number: usize,
        transition: &ProcessTransition,
        range: &ItemRange,
        fields: &BTreeSet<&str>,
    ) -> Result<ConvertedEvent, Vec<ManualRequired>> {
        let site = Site::Transition(process.index, number);
        let refuse = |reason, message| {
            Err(vec![ManualRequired {
                reason,
                message,
                site,
            }])
        };
        if !transition.annotations.source_order().is_empty() {
            return refuse(
                TRANSITION_ANNOTATION,
                format!(
                    "transition '{}' has an annotation; an event takes none, so move it to \
                     `covers` or the requirement by hand",
                    transition.name
                ),
            );
        }
        let mut inputs = BTreeSet::new();
        for input in &transition.inputs {
            match input {
                Param::Range(name, ..) => {
                    return refuse(
                        RANGE_INPUT,
                        format!(
                            "input '{name}' of transition '{}' is typed with a range; an \
                             event's input takes a type name",
                            transition.name
                        ),
                    );
                }
                Param::Typed(name, _) if fields.contains(name.as_str()) => {
                    return refuse(
                        INPUT_SHARES_FIELD,
                        format!(
                            "input '{name}' of transition '{}' is named like a field of '{}': \
                             the transition never reads it (its bare name is the field), but \
                             an event assigns it to the field; rename the input by hand",
                            transition.name, process.entity
                        ),
                    );
                }
                Param::Typed(name, _) => {
                    inputs.insert(name.as_str());
                }
            }
        }
        if transition.guard.is_some() && transition.covers.is_none() {
            return refuse(
                GUARD_WITHOUT_REQUIREMENT,
                format!(
                    "transition '{}' has a `when` guard but no `covers`; the guard becomes a \
                     rule, which a requirement id names, so add `covers` by hand",
                    transition.name
                ),
            );
        }
        let start = range.start;
        let shape = self.is_ident(start, "transition")
            && self.is_ident(start + 1, &transition.name)
            && self.is_ident(start + 2, &transition.source)
            && self.is_symbol(start + 3, "->")
            && self.is_ident(start + 4, &transition.target)
            && self.is_ident(start + 5, "by")
            && self.is_ident(start + 6, &transition.actor);
        if !shape {
            return Err(Self::unrecognized(process, "a transition's head"));
        }
        let covers_at = match &transition.covers {
            Some(cover) => match self.at(cover.span.start.offset) {
                Some(index) => index,
                None => return Err(Self::unrecognized(process, "a transition's `covers`")),
            },
            None => range.end,
        };
        let clause = |word: &str, from: usize| {
            let mut depth = 0usize;
            for index in from..covers_at {
                match &self.tokens[index].kind {
                    TokenKind::Symbol(symbol) if ["(", "[", "{"].contains(&symbol.as_str()) => {
                        depth += 1;
                    }
                    TokenKind::Symbol(symbol) if [")", "]", "}"].contains(&symbol.as_str()) => {
                        depth = depth.saturating_sub(1);
                    }
                    TokenKind::Ident(value) if depth == 0 && value == word => return Some(index),
                    _ => {}
                }
            }
            None
        };
        let mut cursor = start + 7;
        let with_at = (!transition.inputs.is_empty()).then_some(cursor);
        if with_at.is_some() && !self.is_ident(cursor, "with") {
            return Err(Self::unrecognized(process, "a transition's `with`"));
        }
        let when_at = match transition.guard {
            Some(_) => match clause("when", cursor) {
                Some(index) => Some(index),
                None => return Err(Self::unrecognized(process, "a transition's `when`")),
            },
            None => None,
        };
        if let Some(index) = when_at {
            cursor = index;
        }
        let set_at = if transition.assignments.is_empty() {
            None
        } else {
            match clause("set", cursor) {
                Some(index) => Some(index),
                None => return Err(Self::unrecognized(process, "a transition's `set`")),
            }
        };
        let after = |from: usize| {
            [when_at, set_at, Some(covers_at)]
                .into_iter()
                .flatten()
                .find(|index| *index > from)
                .unwrap_or(covers_at)
        };
        let inputs_text = with_at.map(|at| self.text(at + 1, after(at)));
        let guard = when_at.map(|at| self.text(at + 1, after(at)));
        let sets = set_at.map_or_else(Vec::new, |at| {
            self.split_commas(at + 1, after(at))
                .into_iter()
                .map(|(from, to)| self.text(from, to))
                .collect()
        });
        if sets.len() != transition.assignments.len()
            || with_at.is_some_and(|at| {
                self.split_commas(at + 1, after(at)).len() != transition.inputs.len()
            })
        {
            return Err(Self::unrecognized(process, "a transition's clauses"));
        }
        let covers = match &transition.covers {
            Some(cover) => {
                let Some(last) = self
                    .tokens
                    .iter()
                    .rposition(|token| token.span.end.offset == cover.span.end.offset)
                else {
                    return Err(Self::unrecognized(process, "a transition's `covers`"));
                };
                Some((cover.id.clone(), self.text(covers_at + 1, last + 1)))
            }
            None => None,
        };
        Ok(ConvertedEvent {
            comments: Vec::new(),
            name: transition.name.clone(),
            actor: transition.actor.clone(),
            inputs: inputs_text,
            source: transition.source.clone(),
            target: transition.target.clone(),
            sets,
            covers,
            guard,
        })
    }
}

/// The spans findings point at: each process's `process` keyword and each
/// transition's `transition` keyword, by position.
struct Locations {
    processes: Vec<(Span, Vec<Span>)>,
}

impl Locations {
    fn of(source: &str, options: ParseOptions) -> Option<Self> {
        let Ok(SurfaceDocument::Requirements(requirements)) =
            fsl_syntax::parse_surface_document_with(source, &options)
        else {
            return None;
        };
        Some(Self {
            processes: requirements
                .items
                .iter()
                .filter_map(|item| match item {
                    RequirementsItem::Process(BusinessItem::Process { items, span, .. }) => Some((
                        *span,
                        items
                            .iter()
                            .filter_map(|item| match item {
                                ProcessItem::Transition(transition) => Some(transition.span),
                                _ => None,
                            })
                            .collect(),
                    )),
                    _ => None,
                })
                .collect(),
        })
    }

    fn span(&self, site: Site) -> Option<Span> {
        match site {
            Site::Process(index) => self.processes.get(index).map(|(span, _)| *span),
            Site::Transition(index, number) => self
                .processes
                .get(index)
                .and_then(|(_, transitions)| transitions.get(number).copied()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::lifecycles;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    fn edges(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(from, to)| ((*from).to_owned(), (*to).to_owned()))
            .collect()
    }

    fn rendered(lines: &[Vec<String>]) -> Vec<String> {
        lines.iter().map(|line| line.join(" -> ")).collect()
    }

    #[test]
    fn lifecycle_lines_keep_the_stage_order_and_exactly_the_edges() {
        let stages = names(&["Draft", "Submitted", "Approved", "Rejected", "Paid"]);
        let transitions = edges(&[
            ("Draft", "Submitted"),
            ("Submitted", "Approved"),
            ("Submitted", "Rejected"),
            ("Approved", "Paid"),
        ]);
        assert_eq!(
            rendered(&lifecycles(&stages, &transitions)),
            [
                "Draft -> Submitted -> Approved",
                "Submitted -> Rejected",
                "Approved -> Paid"
            ]
        );
    }

    #[test]
    fn lifecycle_lines_cover_unused_stages_back_edges_and_self_loops() {
        let stages = names(&["Open", "Done", "Archived", "Unused"]);
        let transitions = edges(&[
            ("Done", "Open"),
            ("Open", "Done"),
            ("Open", "Open"),
            ("Done", "Archived"),
        ]);
        let lines = lifecycles(&stages, &transitions);
        assert_eq!(
            rendered(&lines),
            ["Open -> Open -> Done -> Open", "Done -> Archived", "Unused"]
        );
        let mut order = Vec::new();
        let mut found = Vec::new();
        for line in &lines {
            for stage in line {
                if !order.contains(stage) {
                    order.push(stage.clone());
                }
            }
            for pair in line.windows(2) {
                found.push((pair[0].clone(), pair[1].clone()));
            }
        }
        assert_eq!(order, stages);
        found.sort();
        let mut expected = transitions;
        expected.sort();
        assert_eq!(found, expected);
    }
}
