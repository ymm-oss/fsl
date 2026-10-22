// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The next-edition requirements members for several entities (fsl 5 plan
//! §3 S8): `R belongs to E as r via n`, `resource`, `measure`, `goal` and
//! `policy`.
//!
//! - **Relation.** `Order belongs to User as buyer via place` gives every
//!   `Order` the field `buyer: Option<User>`, initially `none`, and makes the
//!   event `place` (whose actor is the entity `User`) take the `User`
//!   instance that does it as its first input and record it:
//!   `buyer = some(u)`. Only that event takes the actor's instance
//!   (decisions/048 J7, design draft Q7 (a)); `only as` stays reserved. The
//!   role reads as a field everywhere else (`always Order not in Empty has
//!   buyer`), and `has <role>` is `<role> != none`.
//! - **Resource.** `resource stock: Qty = 2` is one state variable that
//!   belongs to no entity; events `set` it.
//! - **Measure.** `measure m = count C` or `count E in S, …` is a projection,
//!   the number of instances of the concept, with the concept's `where`
//!   condition; it adds no state (the generalised `kpi`, which stays,
//!   decisions/048 J14).
//! - **Goal and policy.** `goal G "t" some E can reach S` and
//!   `policy P "t" every E in S must eventually be T {or T}` are the business
//!   dialect's forms, lowered as the business dialect lowers them.
//!
//! Japanese: `R 所属先 E 役名 r 契機 n`, `資源`, `指標 m = 件数 C`,
//! `目標 G "t" いずれかの E 到達可能 S`,
//! `方針 P "t" すべての E 段階 S いずれ必ず T または T` (the keyword table).

use std::collections::{BTreeMap, BTreeSet};

use super::super::{Parser, join_span};
use super::rules::{Conjunct, Scope};
use super::{Entity, Word, join_names};
use crate::keywords::Kw;
use crate::{
    BusinessGoalBody, BusinessItem, BusinessPolicyBody, Expr, MeasureDeclaration,
    NextEditionRequirements, ParseError, QualifiedName, RelationDeclaration, ResourceDeclaration,
    Span, TokenKind,
};

/// `R belongs to E as r via n`, as read.
pub(in crate::parser) struct RelationSource {
    entity: Word,
    target: Word,
    role: Word,
    event: Word,
    span: Span,
}

/// `resource name: Type = initial`, as read.
pub(in crate::parser) struct ResourceSource {
    name: Word,
    type_name: QualifiedName,
    initial: Expr,
    span: Span,
}

/// `measure name = count …`, as read: a concept, or an entity and stages.
pub(in crate::parser) struct MeasureSource {
    name: Word,
    counted: Word,
    stages: Vec<Word>,
    span: Span,
}

enum GoalForm {
    /// `some E can reach S`.
    Reach { entity: Word, stage: Word },
    /// `every E in S must eventually be T {or T}`.
    Eventually {
        entity: Word,
        source: Word,
        targets: Vec<Word>,
    },
}

/// `goal …` or `policy …`, as read.
pub(in crate::parser) struct GoalSource {
    id: String,
    text: String,
    form: GoalForm,
    span: Span,
}

/// Whether the member starting here is a relation: a name, then
/// `belongs to`.
pub(in crate::parser) fn starts_relation(parser: &Parser<'_>) -> bool {
    matches!(parser.peek().kind, TokenKind::Ident(_)) && parser.peek_kw_at(1, Kw::BelongsTo)
}

impl Parser<'_> {
    /// `R belongs to E as r via n`.
    pub(in crate::parser) fn relation_declaration(&mut self) -> Result<RelationSource, ParseError> {
        let span = self.peek().span;
        let entity = self.word()?;
        self.expect_kw(Kw::BelongsTo)?;
        let target = self.word()?;
        self.expect_kw(Kw::RelationAs)?;
        let role = self.word()?;
        self.expect_kw(Kw::RelationVia)?;
        let event = self.word()?;
        self.spellings
            .statements
            .push(join_span(span, self.last_span()));
        Ok(RelationSource {
            entity,
            target,
            role,
            event,
            span: join_span(span, self.last_span()),
        })
    }

    /// `resource name: Type = initial`.
    pub(in crate::parser) fn resource_declaration(&mut self) -> Result<ResourceSource, ParseError> {
        let span = self.bump_kw(Kw::Resource);
        let name = self.word()?;
        self.expect_symbol(":")?;
        let type_name = self.qualified_name()?;
        if !self.eat_symbol("=") {
            return Err(ParseError::new(
                format!(
                    "resource '{}' needs an initial value (`{}: {} = …`)",
                    name.text, name.text, type_name.name
                ),
                name.span,
            ));
        }
        let initial = self.expression(0)?;
        self.spellings
            .statements
            .push(join_span(span, self.last_span()));
        Ok(ResourceSource {
            name,
            type_name,
            initial,
            span,
        })
    }

    /// `measure name = count Concept` or `count Entity in Stage {, Stage}`.
    pub(in crate::parser) fn measure_declaration(&mut self) -> Result<MeasureSource, ParseError> {
        let span = self.bump_kw(Kw::Measure);
        let name = self.word()?;
        self.expect_symbol("=")?;
        self.expect_kw(Kw::Count)?;
        let counted = self.word()?;
        let mut stages = Vec::new();
        if self.eat_kw(Kw::MeasureIn) {
            loop {
                stages.push(self.word()?);
                if !self.eat_symbol(",") {
                    break;
                }
            }
        }
        self.spellings
            .statements
            .push(join_span(span, self.last_span()));
        Ok(MeasureSource {
            name,
            counted,
            stages,
            span,
        })
    }

    /// `goal ID "text" some E can reach S`.
    pub(in crate::parser) fn goal_declaration(&mut self) -> Result<GoalSource, ParseError> {
        let start = self.bump_kw(Kw::Goal);
        let id = self.req_id()?;
        let (text, text_span) = self.expect_string_with_span()?;
        self.expect_kw(Kw::GoalSome)?;
        let entity = self.word()?;
        self.expect_kw(Kw::CanReach)?;
        let stage = self.word()?;
        self.spellings
            .statements
            .push(join_span(start, self.last_span()));
        Ok(GoalSource {
            id,
            text,
            form: GoalForm::Reach { entity, stage },
            span: join_span(start, text_span),
        })
    }

    /// `policy ID "text" every E in S must eventually be T {or T}`.
    pub(in crate::parser) fn policy_declaration(&mut self) -> Result<GoalSource, ParseError> {
        let start = self.bump_kw(Kw::Policy);
        let id = self.req_id()?;
        let (text, text_span) = self.expect_string_with_span()?;
        self.expect_kw(Kw::PolicyEvery)?;
        let entity = self.word()?;
        self.expect_kw(Kw::PolicyIn)?;
        let source = self.word()?;
        self.expect_kw(Kw::MustEventuallyBe)?;
        let mut targets = vec![self.word()?];
        while self.eat_kw(Kw::PolicyOr) {
            targets.push(self.word()?);
        }
        self.spellings
            .statements
            .push(join_span(start, self.last_span()));
        Ok(GoalSource {
            id,
            text,
            form: GoalForm::Eventually {
                entity,
                source,
                targets,
            },
            span: join_span(start, text_span),
        })
    }
}

/// What the relations give the event that makes them: for each event name,
/// the relations it records, as `(role, target)`.
pub(in crate::parser) type RelationEvents = BTreeMap<String, Vec<(String, String)>>;

/// Resolve the relations and resources before the events are desugared:
/// each role joins its entity's fields (so a rule or a concept reads it as
/// one) and its relations, and the names are checked.
///
/// `plain_entities` are the entities declared without a body (`entity
/// User`), which a relation may point to.
///
/// # Errors
///
/// Returns plain errors for undeclared entities and events, and for names
/// declared twice.
pub(in crate::parser) fn resolve(
    relations: Vec<RelationSource>,
    resources: Vec<ResourceSource>,
    entities: &mut BTreeMap<String, Entity>,
    plain_entities: &BTreeSet<String>,
    event_names: &BTreeMap<String, String>,
    next: &mut NextEditionRequirements,
) -> Result<RelationEvents, ParseError> {
    let mut by_event = RelationEvents::new();
    for relation in relations {
        let RelationSource {
            entity,
            target,
            role,
            event,
            span,
        } = relation;
        let Some(declared) = entities.get_mut(&entity.text) else {
            return Err(ParseError::new(
                format!(
                    "'{}' belongs to '{}': '{}' is not an entity with a lifecycle; declared: {}",
                    entity.text,
                    target.text,
                    entity.text,
                    join_names(entities.keys().map(String::as_str))
                ),
                entity.span,
            ));
        };
        if !plain_entities.contains(&target.text) && !next.entities.contains(&target.text) {
            return Err(ParseError::new(
                format!(
                    "'{}' belongs to '{}', which is not an entity (declare `entity {}`)",
                    entity.text, target.text, target.text
                ),
                target.span,
            ));
        }
        if declared.fields.contains_key(&role.text) {
            return Err(ParseError::new(
                format!(
                    "the role '{}' of '{}' is already a field of '{}'",
                    role.text, entity.text, entity.text
                ),
                role.span,
            ));
        }
        match event_names.get(&event.text) {
            Some(about) if *about == entity.text => {}
            Some(about) => {
                return Err(ParseError::new(
                    format!(
                        "'{}' belongs to '{}' via '{}', but event '{}' is about '{about}'",
                        entity.text, target.text, event.text, event.text
                    ),
                    event.span,
                ));
            }
            None => {
                return Err(ParseError::new(
                    format!(
                        "'{}' belongs to '{}' via '{}', which is not an event",
                        entity.text, target.text, event.text
                    ),
                    event.span,
                ));
            }
        }
        declared.fields.insert(
            role.text.clone(),
            QualifiedName {
                namespace: None,
                name: target.text.clone(),
            },
        );
        declared.roles.insert(role.text.clone());
        by_event
            .entry(event.text.clone())
            .or_default()
            .push((role.text.clone(), target.text.clone()));
        next.relations.push(RelationDeclaration {
            entity: entity.text,
            target: target.text,
            role: role.text,
            event: event.text,
            span,
        });
    }
    resolve_resources(resources, entities, next)?;
    Ok(by_event)
}

/// Check the resources' names and record them: no two resources and no
/// resource and field share a name.
fn resolve_resources(
    resources: Vec<ResourceSource>,
    entities: &BTreeMap<String, Entity>,
    next: &mut NextEditionRequirements,
) -> Result<(), ParseError> {
    for ResourceSource {
        name,
        type_name,
        initial,
        span,
    } in resources
    {
        if next
            .resources
            .iter()
            .any(|resource| resource.name == name.text)
        {
            return Err(ParseError::new(
                format!("duplicate resource '{}'", name.text),
                name.span,
            ));
        }
        if let Some((entity, _)) = entities
            .iter()
            .find(|(_, entity)| entity.fields.contains_key(&name.text))
        {
            return Err(ParseError::new(
                format!(
                    "resource '{}' is spelled like a field of '{entity}'; an event of '{entity}' \
                     could not tell which one it sets",
                    name.text
                ),
                name.span,
            ));
        }
        next.resources.push(ResourceDeclaration {
            name: name.text,
            type_name,
            initial,
            span,
        });
    }
    Ok(())
}

/// Desugar the measures and the goals and policies, once the rules have
/// resolved: a measure's condition is the concept expression a rule would
/// read.
///
/// # Errors
///
/// Returns plain errors for undeclared concepts, entities and stages.
pub(in crate::parser) fn apply(
    measures: Vec<MeasureSource>,
    goals: Vec<GoalSource>,
    scope: &Scope<'_>,
    next: &mut NextEditionRequirements,
) -> Result<(), ParseError> {
    for MeasureSource {
        name,
        counted,
        stages,
        span,
    } in measures
    {
        let (entity, condition, written) = if stages.is_empty() {
            let Some(concept) = scope
                .concepts
                .iter()
                .find(|concept| concept.name == counted.text)
            else {
                return Err(ParseError::new(
                    format!(
                        "measure '{}' counts '{}', which is not a concept; write `count {} in \
                         <Stage>` to count an entity's instances in a stage",
                        name.text, counted.text, counted.text
                    ),
                    counted.span,
                ));
            };
            let entity = concept.entity.clone();
            let condition =
                scope.concept_condition(&[Conjunct::Concept(counted.clone())], &entity)?;
            (entity, condition, counted.text.clone())
        } else {
            let written = join_names(stages.iter().map(|stage| stage.text.as_str()));
            let condition = scope.stages_condition(&counted, &stages)?;
            (counted.text.clone(), condition, written)
        };
        if next
            .measures
            .iter()
            .any(|measure| measure.name == name.text)
        {
            return Err(ParseError::new(
                format!("duplicate measure '{}'", name.text),
                name.span,
            ));
        }
        next.measures.push(MeasureDeclaration {
            name: name.text,
            entity,
            counted: written,
            condition,
            span,
        });
    }
    for GoalSource {
        id,
        text,
        form,
        span,
    } in goals
    {
        let item = match form {
            GoalForm::Reach { entity, stage } => {
                scope.stage_of(&entity, &stage)?;
                BusinessItem::Goal {
                    id,
                    text,
                    body: BusinessGoalBody::SomeStage {
                        case_name: entity.text,
                        stage: stage.text,
                    },
                    span,
                    satisfies: Vec::new(),
                    annotations: crate::Annotations::default(),
                }
            }
            GoalForm::Eventually {
                entity,
                source,
                targets,
            } => {
                scope.stage_of(&entity, &source)?;
                for target in &targets {
                    scope.stage_of(&entity, target)?;
                }
                BusinessItem::Policy {
                    id,
                    text,
                    body: Box::new(BusinessPolicyBody::Eventually {
                        case_name: entity.text,
                        source_stage: source.text,
                        target_stages: targets.into_iter().map(|target| target.text).collect(),
                    }),
                    span,
                    satisfies: Vec::new(),
                    annotations: crate::Annotations::default(),
                }
            }
        };
        next.goals.push(item);
    }
    Ok(())
}
