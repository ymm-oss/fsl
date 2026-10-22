// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The reference views of a requirements document written with entities,
//! events, rules and decision tables: a lifecycle diagram per entity
//! (Mermaid `stateDiagram-v2`), the event table, each decision table with
//! its exhaustiveness / overlap result, the rule statements per requirement,
//! and the glossary of canonical names and aliases.
//!
//! The views are read from the surface document (the declarations the
//! lowering consumed) and from the checked model (aliases, the decision
//! tables' enumeration). They are residue outside every claim block, so no
//! claim digest depends on them, and they are rendered only for a document
//! whose surface carries those declarations: any other document renders as
//! before.
//!
//! Names are canonical under `--lang en`; under `--lang ja` a name with a
//! glossary alias shows the alias, and the section and statement labels use
//! the Japanese keywords of [`fsl_syntax::keywords`]. Mermaid state IDs are
//! always the canonical names (ASCII identifiers); only their labels change.
//! Text quoted "as written" (a rule statement, a decision row's condition)
//! is the source's own spelling in either language.

use std::collections::BTreeMap;

use fsl_core::{DecisionTableResult, KernelModel, decision_table_results, source_expr_text};
use fsl_syntax::keywords::Kw;
use fsl_syntax::{
    Annotation, BusinessItem, DecisionTable, Expr, Param, ParseOptions, ProcessItem,
    ProcessTransition, RequirementsItem, SourceFile, SurfaceDocument, SurfaceRequirements,
    parse_document_with,
};

use crate::document::RequirementClaimSet;
use crate::document_glossary::markdown_label;
use crate::document_presentation::Locale;

/// One entity's lifecycle: its stages, initial stage and events.
struct Lifecycle<'a> {
    entity: &'a str,
    stages: &'a [String],
    initial: Option<&'a str>,
    events: Vec<&'a ProcessTransition>,
}

/// One rule statement, from an event's guard or a rule's property.
struct Statement<'a> {
    requirement: &'a str,
    kind: &'a str,
    detail: &'a str,
    /// The event a guard is added to; `None` for a property.
    event: Option<&'a str>,
    /// The property's Kernel name.
    name: Option<String>,
    /// The lowered guard or property expression; the statement's own
    /// condition when the lowering cannot be found.
    lowered: &'a Expr,
    offset: usize,
}

pub(crate) struct RequirementsViews<'a> {
    requirements: SurfaceRequirements,
    model: &'a KernelModel,
    locale: Locale,
}

impl<'a> RequirementsViews<'a> {
    /// The views of `source` read under `options`; `None` unless it is a
    /// requirements document declared with entities and events.
    pub(crate) fn of(
        source: &str,
        model: &'a KernelModel,
        options: ParseOptions,
        locale: Locale,
    ) -> Option<Self> {
        let parsed = parse_document_with(SourceFile::new(source), &options).ok()?;
        let SurfaceDocument::Requirements(requirements) = parsed.surface else {
            return None;
        };
        requirements.next.as_ref()?;
        Some(Self {
            requirements,
            model,
            locale,
        })
    }

    /// `name` as this locale shows it.
    fn name(&self, canonical: &str) -> String {
        match self.locale {
            Locale::En => canonical.to_owned(),
            Locale::Ja => self
                .model
                .names()
                .glossary()
                .alias(canonical)
                .unwrap_or(canonical)
                .to_owned(),
        }
    }

    /// `name` as shown, with the canonical name beside an alias.
    fn name_with_canonical(&self, canonical: &str) -> String {
        let shown = self.name(canonical);
        if shown == canonical {
            format!("`{canonical}`")
        } else {
            let shown = markdown_label(&shown);
            match self.locale {
                Locale::Ja => format!("{shown}（`{canonical}`）"),
                Locale::En => format!("{shown} (`{canonical}`)"),
            }
        }
    }

    fn keyword(&self, kw: Kw) -> &'static str {
        match self.locale {
            Locale::Ja => kw.japanese(),
            Locale::En => kw.english(),
        }
    }

    fn lifecycles(&self) -> Vec<Lifecycle<'_>> {
        let entities = self
            .requirements
            .next
            .as_ref()
            .map_or(&[][..], |next| next.entities.as_slice());
        self.requirements
            .items
            .iter()
            .filter_map(|item| match item {
                RequirementsItem::Process(BusinessItem::Process { name, items, .. })
                    if entities.iter().any(|entity| entity == name.name()) =>
                {
                    let mut stages: &[String] = &[];
                    let mut initial = None;
                    let mut events = Vec::new();
                    for item in items {
                        match item {
                            ProcessItem::Stages(listed, _) => stages = listed,
                            ProcessItem::Initial(stage, _) => initial = Some(stage.as_str()),
                            ProcessItem::Transition(transition) if transition.event.is_some() => {
                                events.push(transition.as_ref());
                            }
                            ProcessItem::Transition(_) => {}
                        }
                    }
                    Some(Lifecycle {
                        entity: name.name(),
                        stages,
                        initial,
                        events,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// `Actor verb` (`Actor が verb` in Japanese), the phrase on an edge and
    /// in a decision row's outcome.
    fn actor_phrase(&self, event: &ProcessTransition) -> String {
        let verb = event
            .event
            .as_ref()
            .map_or(event.name.as_str(), |form| form.verb.as_str());
        match self.locale {
            Locale::Ja => format!("{} が {}", self.name(&event.actor), self.name(verb)),
            Locale::En => format!("{} {verb}", event.actor),
        }
    }

    /// `Actor verb Entity` (`Actor が Entity を verb`): the event's phrase.
    fn event_phrase(&self, event: &ProcessTransition, entity: &str) -> String {
        let verb = event
            .event
            .as_ref()
            .map_or(event.name.as_str(), |form| form.verb.as_str());
        match self.locale {
            Locale::Ja => format!(
                "{} が {} を {}",
                self.name(&event.actor),
                self.name(entity),
                self.name(verb)
            ),
            Locale::En => format!("{} {verb} {entity}", event.actor),
        }
    }

    /// The display label of each event's action (`action:<event>`), for the
    /// action's claim heading.
    pub(crate) fn action_labels(&self) -> BTreeMap<String, String> {
        self.lifecycles()
            .iter()
            .flat_map(|lifecycle| {
                lifecycle.events.iter().map(|event| {
                    (
                        fsl_core::action_target(&event.name),
                        self.event_phrase(event, lifecycle.entity),
                    )
                })
            })
            .collect()
    }

    /// The overview section: the lifecycle diagrams, the event table, the
    /// decision tables, the rules and the glossary.
    pub(crate) fn overview_section(&self, claims: &RequirementClaimSet) -> String {
        let (head, intro) = match self.locale {
            Locale::Ja => (
                "## 要件の概観",
                "次の図表は、検査済みの仕様から決定論的に生成した参照用の表示である。規範文は「要件」節の生成ブロックにある。",
            ),
            Locale::En => (
                "## Overview of the requirements",
                "The diagrams and tables below are reference views generated deterministically from the checked specification. The normative text is in the generated blocks of the \"Requirements\" section.",
            ),
        };
        let mut parts = vec![head.to_owned(), intro.to_owned()];
        parts.push(self.lifecycle_part());
        parts.push(self.events_part());
        if let Some(part) = self.decisions_part() {
            parts.push(part);
        }
        if let Some(part) = self.rules_part(claims) {
            parts.push(part);
        }
        if let Some(part) = self.glossary_part() {
            parts.push(part);
        }
        parts.join("\n\n")
    }

    fn lifecycle_part(&self) -> String {
        let mut parts = vec![format!("### {}", heading_word(self.locale, Kw::Lifecycle))];
        for lifecycle in self.lifecycles() {
            if lifecycle.stages.is_empty() {
                continue;
            }
            parts.push(format!(
                "#### {}",
                self.name_with_canonical(lifecycle.entity)
            ));
            parts.push(self.mermaid(&lifecycle));
        }
        parts.join("\n\n")
    }

    /// The Mermaid `stateDiagram-v2` of one lifecycle. Every state ID is the
    /// canonical stage name; a stage whose label differs gets a `state "…"
    /// as ID` line, so two stages never share an ID however they display.
    fn mermaid(&self, lifecycle: &Lifecycle<'_>) -> String {
        let mut lines = vec!["```mermaid".to_owned(), "stateDiagram-v2".to_owned()];
        for stage in lifecycle.stages {
            let label = self.name(stage);
            if label != *stage {
                lines.push(format!("    state \"{}\" as {stage}", mermaid_text(&label)));
            }
        }
        if let Some(initial) = lifecycle.initial {
            lines.push(format!("    [*] --> {initial}"));
        }
        for event in &lifecycle.events {
            lines.push(format!(
                "    {} --> {} : {}",
                event.source,
                event.target,
                mermaid_text(&self.actor_phrase(event))
            ));
        }
        lines.push("```".to_owned());
        lines.join("\n")
    }

    fn events_part(&self) -> String {
        let header = match self.locale {
            Locale::Ja => "| 出来事 | 誰が | 何を | どうする | どこから | どこへ | 入力 | 要件 |",
            Locale::En => "| Event | Who | What | Verb | From | To | Input | Requirements |",
        };
        let mut rows = vec![
            header.to_owned(),
            "|---|---|---|---|---|---|---|---|".to_owned(),
        ];
        for lifecycle in self.lifecycles() {
            for event in &lifecycle.events {
                let form = event.event.as_ref();
                let mut name = format!("`{}`", event.name);
                if form.is_some_and(|form| !form.fair) {
                    name = match self.locale {
                        Locale::Ja => format!("{name}（{}）", Kw::Fault.japanese()),
                        Locale::En => format!("{name} ({})", Kw::Fault.english()),
                    };
                }
                let verb = form.map_or(event.name.as_str(), |form| form.verb.as_str());
                let to = if form.is_none_or(|form| form.moves) {
                    self.name(&event.target)
                } else {
                    "—".to_owned()
                };
                let inputs = event
                    .inputs
                    .iter()
                    .map(|param| match param {
                        Param::Typed(name, _) | Param::Range(name, _, _) => self.name(name),
                    })
                    .collect::<Vec<_>>();
                let mut requirements = event
                    .covers
                    .iter()
                    .map(|cover| cover.id.clone())
                    .chain(
                        event
                            .annotations
                            .source_order()
                            .iter()
                            .filter_map(|annotation| match annotation {
                                Annotation::Requirement { id, .. } => Some(id.clone()),
                                _ => None,
                            }),
                    )
                    .collect::<Vec<_>>();
                requirements.dedup();
                rows.push(format!(
                    "| {name} | {} | {} | {} | {} | {} | {} | {} |",
                    cell(&self.name(&event.actor)),
                    cell(&self.name(lifecycle.entity)),
                    cell(&self.name(verb)),
                    cell(&self.name(&event.source)),
                    cell(&to),
                    cell_or_dash(&inputs.join(", ")),
                    cell_or_dash(&requirements.join(", "))
                ));
            }
        }
        format!(
            "### {}\n\n{}",
            heading_word(self.locale, Kw::Event),
            rows.join("\n")
        )
    }

    fn decisions_part(&self) -> Option<String> {
        let tables = &self.requirements.next.as_ref()?.decisions;
        if tables.is_empty() {
            return None;
        }
        let results = decision_table_results(&self.requirements, self.model);
        let mut parts = vec![format!("### {}", heading_word(self.locale, Kw::Decide))];
        for (index, table) in tables.iter().enumerate() {
            let result = results
                .get(index)
                .filter(|(id, _)| *id == table.id)
                .map(|(_, result)| result);
            parts.push(self.decision_table(table, result));
        }
        Some(parts.join("\n\n"))
    }

    fn decision_table(
        &self,
        table: &DecisionTable,
        result: Option<&DecisionTableResult>,
    ) -> String {
        let head = format!("#### {}: {}", table.id, markdown_label(&table.text));
        let target = match self.locale {
            Locale::Ja => format!(
                "{}: {} {}",
                Kw::DecideFor.japanese(),
                self.name(&table.stage),
                self.name(&table.entity)
            ),
            Locale::En => format!("For: {} {}", table.stage, table.entity),
        };
        let header = match self.locale {
            Locale::Ja => "| 行 | 条件（原文） | 結果 |",
            Locale::En => "| Row | Condition (as written) | Outcome |",
        };
        let events = self
            .lifecycles()
            .into_iter()
            .filter(|lifecycle| lifecycle.entity == table.entity)
            .flat_map(|lifecycle| lifecycle.events)
            .collect::<Vec<_>>();
        let mut rows = vec![header.to_owned(), "|---|---|---|".to_owned()];
        for (index, row) in table.rows.iter().enumerate() {
            let condition = row
                .source
                .split_once("=>")
                .map_or(row.source.as_str(), |(condition, _)| condition.trim());
            let outcome = row
                .events
                .iter()
                .map(|name| {
                    events.iter().find(|event| event.name == *name).map_or_else(
                        || format!("`{name}`"),
                        |event| match self.locale {
                            Locale::Ja => format!("{}（`{name}`）", self.actor_phrase(event)),
                            Locale::En => format!("{} (`{name}`)", self.actor_phrase(event)),
                        },
                    )
                })
                .collect::<Vec<_>>();
            let separator = match self.locale {
                Locale::Ja => "、",
                Locale::En => ", ",
            };
            rows.push(format!(
                "| {} | {} | {} |",
                index + 1,
                cell(condition),
                cell(&outcome.join(separator))
            ));
        }
        let mut lines = vec![head, target, rows.join("\n")];
        if let Some(assuming) = &table.assuming {
            lines.push(format!(
                "{}: `{}`",
                match self.locale {
                    Locale::Ja => Kw::Assuming.japanese(),
                    Locale::En => "Assuming",
                },
                source_expr_text(self.model, assuming)
            ));
        }
        lines.push(self.decision_result(table, result));
        lines.join("\n\n")
    }

    fn decision_result(
        &self,
        table: &DecisionTable,
        result: Option<&DecisionTableResult>,
    ) -> String {
        let otherwise = table.rows.iter().any(|row| row.otherwise);
        let ja = matches!(self.locale, Locale::Ja);
        let mut items = Vec::new();
        match result {
            Some(DecisionTableResult::Enumerated {
                exhaustive,
                overlap,
                empty_rows,
            }) => {
                items.push(match (ja, *exhaustive, otherwise) {
                    (true, true, true) => {
                        "網羅性: 満たす（`それ以外` の行で閉じている）".to_owned()
                    }
                    (true, true, false) => {
                        "網羅性: 満たす（型の値域を列挙して確かめた）".to_owned()
                    }
                    (true, false, _) => "網羅性: 満たさない".to_owned(),
                    (false, true, true) => {
                        "Exhaustive: yes (closed by the `otherwise` row)".to_owned()
                    }
                    (false, true, false) => {
                        "Exhaustive: yes (checked by enumerating the types' domains)".to_owned()
                    }
                    (false, false, _) => "Exhaustive: no".to_owned(),
                });
                items.push(match (ja, overlap) {
                    (true, None) => "排他性: 満たす（どの値でも成り立つ行は高々 1 つ）".to_owned(),
                    (true, Some((first, second))) => format!(
                        "排他性: 満たさない（行 {} と行 {} が同じ値で成り立つ）",
                        first + 1,
                        second + 1
                    ),
                    (false, None) => {
                        "Overlap-free: yes (at most one row holds at any value)".to_owned()
                    }
                    (false, Some((first, second))) => format!(
                        "Overlap-free: no (rows {} and {} hold at the same value)",
                        first + 1,
                        second + 1
                    ),
                });
                for row in empty_rows {
                    items.push(if ja {
                        format!(
                            "行 {} はどの値でも成り立たない（`decide_row_emptied`）",
                            row + 1
                        )
                    } else {
                        format!("Row {} holds for no value (`decide_row_emptied`)", row + 1)
                    });
                }
            }
            Some(DecisionTableResult::Deferred(reason)) => items.push(if ja {
                format!(
                    "網羅性・排他性: 値域を列挙しなかったため本書では判定しない。`fslc check` がソルバで確かめる（理由: {}）",
                    markdown_label(reason)
                )
            } else {
                format!(
                    "Exhaustive / overlap-free: not decided here because the domains were not enumerated; `fslc check` asks the solver (reason: {})",
                    markdown_label(reason)
                )
            }),
            None => {}
        }
        items
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn statements(&self) -> Vec<Statement<'_>> {
        let mut statements = Vec::new();
        for lifecycle in self.lifecycles() {
            for event in lifecycle.events {
                let Some(form) = &event.event else { continue };
                // The action's `requires` are the stage guard, then one per
                // rule guard in order (`fsl_syntax::RuleGuard`).
                let requires = self
                    .model
                    .actions
                    .iter()
                    .find(|action| action.name == event.name)
                    .map(|action| action.requires.as_slice())
                    .filter(|requires| requires.len() == form.rule_guards.len() + 1);
                for (index, guard) in form.rule_guards.iter().enumerate() {
                    if guard.kind == "decide" {
                        continue;
                    }
                    statements.push(Statement {
                        requirement: &guard.requirement,
                        kind: guard.kind,
                        detail: &guard.detail,
                        event: Some(&event.name),
                        name: None,
                        lowered: requires.map_or(&guard.condition, |requires| &requires[index + 1]),
                        offset: guard.span.start.offset,
                    });
                }
            }
        }
        if let Some(next) = &self.requirements.next {
            let names = fsl_core::rule_property_names(&next.properties);
            for (property, name) in next.properties.iter().zip(names) {
                let lowered = self
                    .model
                    .invariants
                    .iter()
                    .chain(&self.model.transitions)
                    .find(|candidate| candidate.name == name)
                    .map_or(&property.condition, |candidate| &candidate.expr);
                statements.push(Statement {
                    requirement: &property.requirement,
                    kind: property.kind,
                    detail: &property.detail,
                    event: None,
                    name: Some(name),
                    lowered,
                    offset: property.span.start.offset,
                });
            }
        }
        statements.sort_by_key(|statement| statement.offset);
        statements
    }

    fn rules_part(&self, claims: &RequirementClaimSet) -> Option<String> {
        let statements = self.statements();
        if statements.is_empty() {
            return None;
        }
        let mut order: Vec<&str> = Vec::new();
        for statement in &statements {
            if !order.contains(&statement.requirement) {
                order.push(statement.requirement);
            }
        }
        let lowered = match self.locale {
            Locale::Ja => "lowering 後の条件",
            Locale::En => "Condition after lowering",
        };
        let mut parts = vec![format!("### {}", heading_word(self.locale, Kw::Rule))];
        for requirement in order {
            parts.push(format!("#### {requirement}"));
            if let Some(text) = claims
                .requirements
                .iter()
                .find(|candidate| candidate.id == requirement)
                .and_then(|candidate| candidate.statements.iter().find_map(|s| s.text.as_deref()))
            {
                parts.push(format!("> {text}"));
            }
            let items = statements
                .iter()
                .filter(|statement| statement.requirement == requirement)
                .map(|statement| {
                    let kind = self.keyword(statement_keyword(statement.kind));
                    let subject = match (statement.event, statement.name.as_deref()) {
                        (Some(event), _) => match self.locale {
                            Locale::Ja => format!("（出来事 `{event}`）"),
                            Locale::En => format!(" (event `{event}`)"),
                        },
                        (None, Some(name)) => match self.locale {
                            Locale::Ja => format!("（`{name}`）"),
                            Locale::En => format!(" (`{name}`)"),
                        },
                        (None, None) => String::new(),
                    };
                    format!(
                        "- **{kind}**{subject}: `{}`\n\n  <details><summary>{lowered}</summary>\n\n  `{}`\n\n  </details>",
                        statement.detail.replace('`', "'"),
                        source_expr_text(self.model, statement.lowered).replace('`', "'")
                    )
                })
                .collect::<Vec<_>>();
            parts.push(items.join("\n\n"));
        }
        Some(parts.join("\n\n"))
    }

    fn glossary_part(&self) -> Option<String> {
        let glossary = self.model.names().glossary();
        if glossary.is_empty() {
            return None;
        }
        let header = match self.locale {
            Locale::Ja => "| 正名 | 別名 |",
            Locale::En => "| Canonical name | Alias |",
        };
        let mut rows = vec![header.to_owned(), "|---|---|".to_owned()];
        for (canonical, alias) in glossary.entries() {
            rows.push(format!("| `{canonical}` | {} |", cell(alias)));
        }
        Some(format!(
            "### {}\n\n{}",
            heading_word(self.locale, Kw::Glossary),
            rows.join("\n")
        ))
    }
}

/// The section word for `kw`: the Japanese keyword, or an English heading.
fn heading_word(locale: Locale, kw: Kw) -> &'static str {
    match locale {
        Locale::Ja => kw.japanese(),
        Locale::En => match kw {
            Kw::Lifecycle => "Lifecycles",
            Kw::Event => "Events",
            Kw::Decide => "Decision tables",
            Kw::Rule => "Rules",
            Kw::Glossary => "Glossary",
            other => other.english(),
        },
    }
}

/// The keyword a rule statement of `kind` is written with.
fn statement_keyword(kind: &str) -> Kw {
    match kind {
        "never" => Kw::Never,
        "after" => Kw::OnlyAfter,
        "always" => Kw::Always,
        "enters" => Kw::Enters,
        "deadline" => Kw::Deadline,
        _ => Kw::Allow,
    }
}

/// A Markdown table cell: a `|` would end the cell early.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn cell_or_dash(text: &str) -> String {
    if text.is_empty() {
        "—".to_owned()
    } else {
        cell(text)
    }
}

/// Text inside a Mermaid label: a double quote or a colon would end it.
fn mermaid_text(text: &str) -> String {
    text.replace('"', "'").replace(':', " ")
}
