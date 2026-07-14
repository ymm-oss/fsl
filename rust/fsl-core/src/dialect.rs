// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{
    ActionItem, Binder, BusinessGoalBody, BusinessItem, BusinessPolicyBody, Expr,
    GovernanceArtifactRef, GovernanceDelegateItem, GovernanceItem, LValue, MetaTag, Param,
    PreservationItem, ProcessField, ProcessItem, ProcessTransition, QualifiedName,
    RequirementAction, RequirementActionItem, RequirementBlockItem, RequirementsItem, SpecItem,
    StateField, Statement, SurfaceBusiness, SurfaceDocument, SurfaceGovernance,
    SurfaceRequirements, SurfaceSpec, TimeItem, TypeExpr, VerifyItem,
};

use crate::{CoreError, KernelSpec, lower_direct_spec, substitute_expr};

#[derive(Clone)]
struct Process {
    name: String,
    stages: Vec<String>,
    initial: String,
    transitions: Vec<ProcessTransition>,
    span: fsl_syntax::Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceDelegate {
    pub business: String,
    pub required: Vec<String>,
    pub satisfied: BTreeMap<String, Vec<(String, String)>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernancePreservation {
    pub name: String,
    pub before_name: String,
    pub before_path: String,
    pub after_name: String,
    pub after_path: String,
    pub preserve: Vec<String>,
    pub refinement_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceContract {
    pub name: String,
    pub controls: Vec<String>,
    pub delegates: Vec<GovernanceDelegate>,
    pub preservations: Vec<GovernancePreservation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequirementsTraceStep {
    pub name: String,
    pub args: Vec<Expr>,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequirementsTraceExpectation {
    Expr(Expr),
    Stage {
        entity: String,
        instance: i64,
        stage: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequirementsTraceCase {
    pub id: String,
    pub text: String,
    pub steps: Vec<RequirementsTraceStep>,
    pub expectation: Option<RequirementsTraceExpectation>,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequirementsTraceContract {
    pub acceptance: Vec<RequirementsTraceCase>,
    pub forbidden: Vec<RequirementsTraceCase>,
}

fn qualified(name: &str) -> QualifiedName {
    QualifiedName {
        namespace: None,
        name: name.to_owned(),
    }
}

fn typed_binder(name: &str, type_name: &str) -> Binder {
    Binder::Typed {
        name: name.to_owned(),
        type_name: qualified(type_name),
        where_expr: None,
    }
}

fn process_state(name: &str) -> String {
    format!("{}_stage", name.to_lowercase())
}

fn process_enum(name: &str) -> String {
    format!("{name}Stage")
}

fn stage_is(process: &Process, binder: &str, stage: &str) -> Expr {
    Expr::Binary {
        op: "==".to_owned(),
        left: Box::new(Expr::Index(
            Box::new(Expr::Var(process_state(&process.name))),
            Box::new(Expr::Var(binder.to_owned())),
        )),
        right: Box::new(Expr::Var(stage.to_owned())),
    }
}

fn or_all(mut expressions: Vec<Expr>) -> Expr {
    if expressions.is_empty() {
        return Expr::Bool(false);
    }
    let mut expression = expressions.remove(0);
    for next in expressions {
        expression = Expr::Binary {
            op: "or".to_owned(),
            left: Box::new(expression),
            right: Box::new(next),
        };
    }
    expression
}

fn and_all(mut expressions: Vec<Expr>) -> Expr {
    if expressions.is_empty() {
        return Expr::Bool(true);
    }
    let mut expression = expressions.remove(0);
    for next in expressions {
        expression = Expr::Binary {
            op: "and".to_owned(),
            left: Box::new(expression),
            right: Box::new(next),
        };
    }
    expression
}

#[allow(clippy::unnecessary_wraps)]
fn meta(id: &str, text: impl Into<String>) -> Option<MetaTag> {
    Some(MetaTag {
        id: id.to_owned(),
        text: Some(text.into()),
    })
}

/// Lower the business process/policy dialect into the shared kernel.
///
/// # Errors
///
/// Returns [`CoreError`] for missing bounds, malformed processes, or unknown
/// process/stage references.
#[allow(clippy::too_many_lines)]
pub fn lower_business(business: SurfaceBusiness) -> Result<KernelSpec, CoreError> {
    let mut actors = BTreeSet::new();
    let mut entities = Vec::new();
    let mut bounds = BTreeMap::new();
    let mut process_items = Vec::new();
    let mut policies = Vec::new();
    let mut goals = Vec::new();
    for item in &business.items {
        match item {
            BusinessItem::Actor(names, _) => actors.extend(names.iter().cloned()),
            BusinessItem::Entity(name, span) => entities.push((name.clone(), *span)),
            BusinessItem::VerifyBounds { items, .. } => {
                for item in items {
                    if let VerifyItem::Instances(name, count, _) = item {
                        bounds.insert(name.clone(), *count);
                    }
                }
            }
            BusinessItem::Process { .. } => process_items.push(item),
            BusinessItem::Policy { .. } => policies.push(item),
            BusinessItem::Goal { .. } => goals.push(item),
            _ => {}
        }
    }
    let mut processes = Vec::new();
    for item in process_items {
        let BusinessItem::Process {
            name,
            fields,
            items,
            span,
        } = item
        else {
            unreachable!();
        };
        if fields.is_some() {
            return Err(core_error(
                "data guards/fields are a requirements-layer feature".to_owned(),
                *span,
            ));
        }
        let mut stages = None;
        let mut initial = None;
        let mut transitions = Vec::new();
        for item in items {
            match item {
                ProcessItem::Stages(values, _) => stages = Some(values.clone()),
                ProcessItem::Initial(value, _) => initial = Some(value.clone()),
                ProcessItem::Transition(transition) => {
                    transitions.push(transition.as_ref().clone());
                }
            }
        }
        let stages = stages
            .ok_or_else(|| core_error(format!("process '{name}' must declare stages"), *span))?;
        let initial = initial.ok_or_else(|| {
            core_error(
                format!("process '{name}' must declare initial stage"),
                *span,
            )
        })?;
        if !entities.iter().any(|(entity, _)| entity == name) {
            return Err(core_error(
                format!("process '{name}' has no matching entity declaration"),
                *span,
            ));
        }
        for transition in &transitions {
            if !actors.contains(&transition.actor) {
                return Err(core_error(
                    format!(
                        "transition '{}' uses undeclared actor '{}'",
                        transition.name, transition.actor
                    ),
                    transition.span,
                ));
            }
        }
        processes.push(Process {
            name: name.clone(),
            stages,
            initial,
            transitions,
            span: *span,
        });
    }

    let mut items = Vec::new();
    for (entity, span) in &entities {
        let count = bounds.get(entity).ok_or_else(|| {
            core_error(
                format!("entity '{entity}' has no 'instances' bound in verify block"),
                *span,
            )
        })?;
        if *count < 1 {
            return Err(core_error(
                format!("entity '{entity}' instances bound must be >= 1"),
                *span,
            ));
        }
        items.push(SpecItem::Type {
            name: entity.clone(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Num(*count - 1)),
            symmetric: false,
        });
    }
    for process in &processes {
        items.push(SpecItem::Enum {
            name: process_enum(&process.name),
            members: process.stages.clone(),
            symmetric: false,
        });
    }
    if !processes.is_empty() {
        items.push(SpecItem::State(
            processes
                .iter()
                .map(|process| {
                    StateField::generated(
                        process_state(&process.name),
                        TypeExpr::Map(
                            Box::new(TypeExpr::Name(process.name.clone())),
                            Box::new(TypeExpr::Name(process_enum(&process.name))),
                        ),
                        process.span,
                    )
                })
                .collect(),
        ));
        items.push(SpecItem::Init {
            statements: processes
                .iter()
                .map(|process| Statement::ForAll {
                    binder: typed_binder("c", &process.name),
                    statements: vec![Statement::Assign {
                        target: LValue::Index(
                            process_state(&process.name),
                            Expr::Var("c".to_owned()),
                        ),
                        value: Expr::Var(process.initial.clone()),
                        span: process.span,
                    }],
                    span: process.span,
                })
                .collect(),
            meta: None,
        });
    }
    for process in &processes {
        for transition in &process.transitions {
            items.push(SpecItem::Action {
                name: transition.name.clone(),
                params: vec![Param::Typed("c".to_owned(), qualified(&process.name))],
                items: vec![
                    ActionItem::Requires(
                        stage_is(process, "c", &transition.source),
                        transition.span,
                    ),
                    ActionItem::Statement(Statement::Assign {
                        target: LValue::Index(
                            process_state(&process.name),
                            Expr::Var("c".to_owned()),
                        ),
                        value: Expr::Var(transition.target.clone()),
                        span: transition.span,
                    }),
                ],
                span: transition.span,
                fair: true,
                meta: meta(&transition.name, format!("by {}", transition.actor)),
                sync: false,
            });
        }
    }
    let terminal = processes
        .iter()
        .map(|process| {
            let outgoing = process
                .transitions
                .iter()
                .map(|transition| transition.source.as_str())
                .collect::<BTreeSet<_>>();
            let sinks = process
                .stages
                .iter()
                .filter(|stage| !outgoing.contains(stage.as_str()))
                .map(|stage| stage_is(process, "c", stage))
                .collect::<Vec<_>>();
            Expr::Quantified {
                quantifier: "forall".to_owned(),
                binder: typed_binder("c", &process.name),
                body: Box::new(or_all(sinks)),
            }
        })
        .collect::<Vec<_>>();
    if !terminal.is_empty() {
        items.push(SpecItem::Terminal {
            expr: Box::new(and_all(terminal)),
            span: processes[0].span,
        });
    }
    let by_name = processes
        .iter()
        .map(|process| (process.name.as_str(), process))
        .collect::<BTreeMap<_, _>>();
    for policy in policies {
        let BusinessItem::Policy {
            id,
            text,
            body,
            span,
            ..
        } = policy
        else {
            unreachable!();
        };
        match body.as_ref() {
            BusinessPolicyBody::Invariant(expr) => items.push(SpecItem::Invariant {
                name: id.clone(),
                expr: Box::new(expr.clone()),
                span: *span,
                meta: meta(id, text),
            }),
            BusinessPolicyBody::Responds {
                binders,
                before,
                after,
                within,
            } => items.push(SpecItem::LeadsTo {
                name: id.clone(),
                binders: binders.clone(),
                before: before.clone(),
                after: after.clone(),
                span: *span,
                meta: meta(id, text),
                decreases: None,
                within: within.clone(),
                helpful: Vec::new(),
            }),
            BusinessPolicyBody::Eventually {
                case_name,
                source_stage,
                target_stages,
            } => {
                let process = by_name.get(case_name.as_str()).ok_or_else(|| {
                    core_error(format!("entity '{case_name}' has no process"), *span)
                })?;
                items.push(SpecItem::LeadsTo {
                    name: id.clone(),
                    binders: vec![typed_binder("c", case_name)],
                    before: Box::new(stage_is(process, "c", source_stage)),
                    after: Box::new(or_all(
                        target_stages
                            .iter()
                            .map(|stage| stage_is(process, "c", stage))
                            .collect(),
                    )),
                    span: *span,
                    meta: meta(id, text),
                    decreases: None,
                    within: None,
                    helpful: Vec::new(),
                });
            }
            BusinessPolicyBody::Precedence { .. } => {}
        }
    }
    for goal in goals {
        let BusinessItem::Goal {
            id,
            text,
            body,
            span,
            ..
        } = goal
        else {
            unreachable!();
        };
        let expr = match body {
            BusinessGoalBody::Expr(expr) => expr.clone(),
            BusinessGoalBody::SomeStage { case_name, stage } => {
                let process = by_name.get(case_name.as_str()).ok_or_else(|| {
                    core_error(format!("entity '{case_name}' has no process"), *span)
                })?;
                Expr::Quantified {
                    quantifier: "exists".to_owned(),
                    binder: typed_binder("c", case_name),
                    body: Box::new(stage_is(process, "c", stage)),
                }
            }
            BusinessGoalBody::AllStage { case_name, stages } => {
                let process = by_name.get(case_name.as_str()).ok_or_else(|| {
                    core_error(format!("entity '{case_name}' has no process"), *span)
                })?;
                Expr::Quantified {
                    quantifier: "forall".to_owned(),
                    binder: typed_binder("c", case_name),
                    body: Box::new(or_all(
                        stages
                            .iter()
                            .map(|stage| stage_is(process, "c", stage))
                            .collect(),
                    )),
                }
            }
        };
        items.push(SpecItem::Reachable {
            name: id.clone(),
            expr: Box::new(expr),
            span: *span,
            meta: meta(id, text),
        });
    }
    lower_direct_spec(SurfaceSpec {
        name: business.name,
        meta: None,
        items,
    })
}

fn core_error(message: String, span: fsl_syntax::Span) -> CoreError {
    CoreError {
        message,
        line: span.start.line,
        column: span.start.column,
        origin: None,
    }
}

#[derive(Clone)]
struct RequirementsProcess {
    process: Process,
    fields: Vec<ProcessField>,
}

fn process_field_state(process: &str, field: &str) -> String {
    format!("{}_{}", process.to_lowercase(), field)
}

fn with_meta(item: SpecItem, metadata: Option<MetaTag>) -> SpecItem {
    match item {
        SpecItem::Action {
            name,
            params,
            items,
            span,
            fair,
            sync,
            ..
        } => SpecItem::Action {
            name,
            params,
            items,
            span,
            fair,
            meta: metadata,
            sync,
        },
        SpecItem::Invariant {
            name, expr, span, ..
        } => SpecItem::Invariant {
            name,
            expr,
            span,
            meta: metadata,
        },
        SpecItem::Reachable {
            name, expr, span, ..
        } => SpecItem::Reachable {
            name,
            expr,
            span,
            meta: metadata,
        },
        SpecItem::LeadsTo {
            name,
            binders,
            before,
            after,
            span,
            decreases,
            within,
            helpful,
            ..
        } => SpecItem::LeadsTo {
            name,
            binders,
            before,
            after,
            span,
            meta: metadata,
            decreases,
            within,
            helpful,
        },
        other => other,
    }
}

fn lower_requirement_action(
    action: &RequirementAction,
    metadata: Option<MetaTag>,
) -> Vec<SpecItem> {
    let ordinary = action
        .items
        .iter()
        .filter_map(|item| match item {
            RequirementActionItem::Action(item) => Some(item.as_ref().clone()),
            RequirementActionItem::Branches { .. } => None,
        })
        .collect::<Vec<_>>();
    let branches = action.items.iter().find_map(|item| match item {
        RequirementActionItem::Branches { branches, .. } => Some(branches),
        RequirementActionItem::Action(_) => None,
    });
    match branches {
        None => vec![SpecItem::Action {
            name: action.name.clone(),
            params: action.params.clone(),
            items: ordinary,
            span: action.span,
            fair: action.fair,
            meta: metadata.or_else(|| action.meta.clone()),
            sync: false,
        }],
        Some(branches) => branches
            .iter()
            .enumerate()
            .map(|(index, branch)| {
                let mut items = ordinary.clone();
                items.push(ActionItem::Requires(branch.condition.clone(), branch.span));
                items.extend(branch.statements.iter().cloned().map(ActionItem::Statement));
                SpecItem::Action {
                    name: format!("{}__b{}", action.name, index + 1),
                    params: action.params.clone(),
                    items,
                    span: action.span,
                    fair: action.fair,
                    meta: metadata.clone().or_else(|| action.meta.clone()),
                    sync: false,
                }
            })
            .collect(),
    }
}

fn static_int(expr: &Expr, constants: &BTreeMap<String, i64>) -> Option<i64> {
    match expr {
        Expr::Num(value) => Some(*value),
        Expr::Var(name) => constants.get(name).copied(),
        Expr::Neg(value) => static_int(value, constants)?.checked_neg(),
        Expr::Binary { op, left, right } => {
            let left = static_int(left, constants)?;
            let right = static_int(right, constants)?;
            match op.as_str() {
                "+" => left.checked_add(right),
                "-" => left.checked_sub(right),
                "*" => left.checked_mul(right),
                _ => None,
            }
        }
        _ => None,
    }
}

fn action_enabled_expression(action: &SpecItem) -> Option<Expr> {
    let SpecItem::Action { params, items, .. } = action else {
        return None;
    };
    let mut replacements = std::collections::HashMap::new();
    let mut requires = Vec::new();
    for item in items {
        match item {
            ActionItem::Let(name, value, _) => {
                replacements.insert(name.clone(), substitute_expr(value.clone(), &replacements));
            }
            ActionItem::Requires(expression, _) => {
                requires.push(substitute_expr(expression.clone(), &replacements));
            }
            _ => {}
        }
    }
    let mut expression = and_all(requires);
    for param in params.iter().rev() {
        let binder = match param {
            Param::Typed(name, type_name) => Binder::Typed {
                name: name.clone(),
                type_name: type_name.clone(),
                where_expr: None,
            },
            Param::Range(name, lo, hi) => Binder::Range {
                name: name.clone(),
                lo: Box::new(lo.clone()),
                hi: Box::new(hi.clone()),
            },
        };
        expression = Expr::Quantified {
            quantifier: "exists".to_owned(),
            binder,
            body: Box::new(expression),
        };
    }
    Some(expression)
}

fn generated_age_type(name: &str, existing: &mut BTreeSet<String>) -> String {
    let suffix = name
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect::<String>();
    let base = if suffix.is_empty() {
        "_AgeCounter".to_owned()
    } else {
        format!("_Age{suffix}")
    };
    let mut candidate = base.clone();
    let mut index = 2;
    while existing.contains(&candidate) {
        candidate = format!("{base}{index}");
        index += 1;
    }
    existing.insert(candidate.clone());
    candidate
}

/// Lower the requirements dialect into the shared kernel.
///
/// # Errors
///
/// Returns [`CoreError`] for missing entity/number bounds or malformed process
/// declarations.
#[allow(clippy::too_many_lines)]
pub fn lower_requirements(requirements: SurfaceRequirements) -> Result<KernelSpec, CoreError> {
    let mut common = Vec::new();
    let mut entities = Vec::new();
    let mut numbers = Vec::new();
    let mut instance_bounds = BTreeMap::new();
    let mut value_bounds = BTreeMap::new();
    let mut process_items = Vec::new();
    let mut requirement_items = Vec::new();
    let mut standalone_actions = Vec::new();
    let mut time_items = None;
    for item in &requirements.items {
        match item {
            RequirementsItem::Common(SpecItem::Entity(name, span)) => {
                entities.push((name.clone(), *span));
            }
            RequirementsItem::Common(SpecItem::Number(name, span)) => {
                numbers.push((name.clone(), *span));
            }
            RequirementsItem::Common(SpecItem::VerifyBounds { items, .. }) => {
                for item in items {
                    match item {
                        VerifyItem::Instances(name, count, _) => {
                            instance_bounds.insert(name.clone(), *count);
                        }
                        VerifyItem::Values(name, lo, hi, _) => {
                            value_bounds
                                .insert(name.clone(), (lo.as_ref().clone(), hi.as_ref().clone()));
                        }
                    }
                }
            }
            RequirementsItem::Common(item) => common.push(item.clone()),
            RequirementsItem::Process(item) => process_items.push(item),
            RequirementsItem::Requirement { .. } => requirement_items.push(item),
            RequirementsItem::Action(action) => standalone_actions.push(action),
            RequirementsItem::Time { items, span } => {
                if time_items.is_some() {
                    return Err(core_error(
                        "requirements may declare time block only once".to_owned(),
                        *span,
                    ));
                }
                time_items = Some((items.clone(), *span));
            }
            _ => {}
        }
    }
    for item in &requirements.items {
        if let RequirementsItem::Process(BusinessItem::Process { name, span, .. }) = item
            && !entities.iter().any(|(candidate, _)| candidate == name)
        {
            entities.push((name.clone(), *span));
        }
    }
    let mut items = Vec::new();
    for (name, span) in &entities {
        let count = instance_bounds.get(name).ok_or_else(|| {
            core_error(
                format!("entity '{name}' has no 'instances' bound in verify block"),
                *span,
            )
        })?;
        items.push(SpecItem::Type {
            name: name.clone(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Num(*count - 1)),
            symmetric: false,
        });
    }
    for (name, span) in &numbers {
        let (lo, hi) = value_bounds.get(name).ok_or_else(|| {
            core_error(
                format!("number '{name}' has no 'values' bound in verify block"),
                *span,
            )
        })?;
        items.push(SpecItem::Type {
            name: name.clone(),
            lo: Box::new(lo.clone()),
            hi: Box::new(hi.clone()),
            symmetric: false,
        });
    }
    items.extend(common);

    let mut processes = Vec::new();
    for item in process_items {
        let BusinessItem::Process {
            name,
            fields,
            items,
            span,
        } = item
        else {
            return Err(core_error(
                "expected process declaration".to_owned(),
                zero_span(),
            ));
        };
        let mut stages = None;
        let mut initial = None;
        let mut transitions = Vec::new();
        for item in items {
            match item {
                ProcessItem::Stages(values, _) => stages = Some(values.clone()),
                ProcessItem::Initial(value, _) => initial = Some(value.clone()),
                ProcessItem::Transition(transition) => {
                    transitions.push(transition.as_ref().clone());
                }
            }
        }
        processes.push(RequirementsProcess {
            process: Process {
                name: name.clone(),
                stages: stages.ok_or_else(|| {
                    core_error(format!("process '{name}' must declare stages"), *span)
                })?,
                initial: initial.ok_or_else(|| {
                    core_error(
                        format!("process '{name}' must declare initial stage"),
                        *span,
                    )
                })?,
                transitions,
                span: *span,
            },
            fields: fields
                .as_ref()
                .map_or_else(Vec::new, |fields| fields.fields.clone()),
        });
    }
    for process in &processes {
        items.push(SpecItem::Enum {
            name: process_enum(&process.process.name),
            members: process.process.stages.clone(),
            symmetric: false,
        });
    }
    let mut state = Vec::new();
    let mut init = Vec::new();
    for process in &processes {
        let process_name = &process.process.name;
        state.push(StateField::generated(
            process_state(process_name),
            TypeExpr::Map(
                Box::new(TypeExpr::Name(process_name.clone())),
                Box::new(TypeExpr::Name(process_enum(process_name))),
            ),
            process.process.span,
        ));
        let mut init_body = vec![Statement::Assign {
            target: LValue::Index(process_state(process_name), Expr::Var("c".to_owned())),
            value: Expr::Var(process.process.initial.clone()),
            span: process.process.span,
        }];
        for field in &process.fields {
            let field_ty = if field.type_name.name == "Bool" {
                TypeExpr::Bool
            } else {
                TypeExpr::Name(field.type_name.name.clone())
            };
            state.push(StateField::generated(
                process_field_state(process_name, &field.name),
                TypeExpr::Map(
                    Box::new(TypeExpr::Name(process_name.clone())),
                    Box::new(field_ty),
                ),
                process.process.span,
            ));
            let initial = field.initial.clone().unwrap_or_else(|| {
                value_bounds
                    .get(&field.type_name.name)
                    .map_or(Expr::Num(0), |(lo, _)| lo.clone())
            });
            init_body.push(Statement::Assign {
                target: LValue::Index(
                    process_field_state(process_name, &field.name),
                    Expr::Var("c".to_owned()),
                ),
                value: initial,
                span: process.process.span,
            });
        }
        init.push(Statement::ForAll {
            binder: typed_binder("c", process_name),
            statements: init_body,
            span: process.process.span,
        });
    }
    if !state.is_empty() {
        items.push(SpecItem::State(state));
        items.push(SpecItem::Init {
            statements: init,
            meta: None,
        });
    }
    for process in &processes {
        let replacements = process
            .fields
            .iter()
            .map(|field| {
                (
                    field.name.clone(),
                    Expr::Index(
                        Box::new(Expr::Var(process_field_state(
                            &process.process.name,
                            &field.name,
                        ))),
                        Box::new(Expr::Var("c".to_owned())),
                    ),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        for transition in &process.process.transitions {
            let mut body = vec![ActionItem::Requires(
                stage_is(&process.process, "c", &transition.source),
                transition.span,
            )];
            if let Some(guard) = &transition.guard {
                body.push(ActionItem::Requires(
                    substitute_expr(guard.clone(), &replacements),
                    transition.span,
                ));
            }
            body.push(ActionItem::Statement(Statement::Assign {
                target: LValue::Index(
                    process_state(&process.process.name),
                    Expr::Var("c".to_owned()),
                ),
                value: Expr::Var(transition.target.clone()),
                span: transition.span,
            }));
            for (field, expression) in &transition.assignments {
                body.push(ActionItem::Statement(Statement::Assign {
                    target: LValue::Index(
                        process_field_state(&process.process.name, field),
                        Expr::Var("c".to_owned()),
                    ),
                    value: substitute_expr(expression.clone(), &replacements),
                    span: transition.span,
                }));
            }
            let metadata = transition.covers.as_ref().map_or_else(
                || meta(&transition.name, format!("by {}", transition.actor)),
                |(id, text)| meta(id, text),
            );
            let mut params = vec![Param::Typed(
                "c".to_owned(),
                qualified(&process.process.name),
            )];
            params.extend(transition.inputs.clone());
            items.push(SpecItem::Action {
                name: transition.name.clone(),
                params,
                items: body,
                span: transition.span,
                fair: true,
                meta: metadata,
                sync: false,
            });
        }
    }
    let mut deadlines = Vec::new();
    for requirement in requirement_items {
        let RequirementsItem::Requirement {
            id,
            text,
            items: declarations,
            ..
        } = requirement
        else {
            unreachable!();
        };
        let metadata = meta(id, text);
        for declaration in declarations {
            match declaration {
                RequirementBlockItem::Action(action) => {
                    items.extend(lower_requirement_action(action, metadata.clone()));
                }
                RequirementBlockItem::Property(property) => {
                    items.push(with_meta(property.clone(), metadata.clone()));
                }
                RequirementBlockItem::Deadline { name, bound, span } => {
                    deadlines.push((name.clone(), bound.clone(), *span, metadata.clone()));
                }
            }
        }
    }
    for action in standalone_actions {
        items.extend(lower_requirement_action(action, None));
    }
    if !deadlines.is_empty() && time_items.is_none() {
        return Err(core_error(
            "deadline requires a time block".to_owned(),
            deadlines[0].2,
        ));
    }
    if let Some((time_items, time_span)) = time_items {
        let mut ages = BTreeMap::new();
        let mut urgent = Vec::new();
        for item in time_items {
            match item {
                TimeItem::Urgent(names, _) => urgent.extend(names),
                TimeItem::Age {
                    name,
                    binder,
                    condition,
                    span,
                } => {
                    if ages
                        .insert(name.clone(), (binder, condition, span))
                        .is_some()
                    {
                        return Err(core_error(format!("duplicate age '{name}'"), span));
                    }
                }
            }
        }
        let mut constants = BTreeMap::new();
        for item in &items {
            if let SpecItem::Const { name, value } = item
                && let Some(value) = static_int(value, &constants)
            {
                constants.insert(name.clone(), value);
            }
        }
        let mut existing = items
            .iter()
            .filter_map(|item| match item {
                SpecItem::Type { name, .. }
                | SpecItem::Enum { name, .. }
                | SpecItem::Struct { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut age_info = BTreeMap::new();
        for (name, (binder, condition, span)) in ages {
            let matching = deadlines
                .iter()
                .filter(|(deadline_age, ..)| deadline_age == &name)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                return Err(core_error(format!("unused age '{name}'"), span));
            }
            let maximum = matching
                .iter()
                .filter_map(|(_, bound, ..)| static_int(bound, &constants))
                .max()
                .ok_or_else(|| core_error("deadline bound must be constant".to_owned(), span))?;
            if maximum < 0 {
                return Err(core_error(
                    "deadline bound must be non-negative".to_owned(),
                    span,
                ));
            }
            let cap = maximum + 1;
            let type_name = generated_age_type(&name, &mut existing);
            items.push(SpecItem::Type {
                name: type_name.clone(),
                lo: Box::new(Expr::Num(0)),
                hi: Box::new(Expr::Num(cap)),
                symmetric: false,
            });
            let (state_type, init_statement, reference, target) = match &binder {
                Some(binder) => {
                    let Binder::Typed {
                        name: binder_name,
                        type_name: binder_type,
                        ..
                    } = binder
                    else {
                        return Err(core_error(
                            "indexed age expects syntax `age m[x: T] while ...`".to_owned(),
                            span,
                        ));
                    };
                    let index = Expr::Var(binder_name.clone());
                    (
                        TypeExpr::Map(
                            Box::new(TypeExpr::Name(binder_type.name.clone())),
                            Box::new(TypeExpr::Name(type_name.clone())),
                        ),
                        Statement::ForAll {
                            binder: binder.clone(),
                            statements: vec![Statement::Assign {
                                target: LValue::Index(name.clone(), index.clone()),
                                value: Expr::Num(0),
                                span,
                            }],
                            span,
                        },
                        Expr::Index(Box::new(Expr::Var(name.clone())), Box::new(index.clone())),
                        LValue::Index(name.clone(), index),
                    )
                }
                None => (
                    TypeExpr::Name(type_name.clone()),
                    Statement::Assign {
                        target: LValue::Var(name.clone()),
                        value: Expr::Num(0),
                        span,
                    },
                    Expr::Var(name.clone()),
                    LValue::Var(name.clone()),
                ),
            };
            items.push(SpecItem::State(vec![StateField::generated(
                name.clone(),
                state_type,
                span,
            )]));
            items.push(SpecItem::Init {
                statements: vec![init_statement],
                meta: None,
            });
            age_info.insert(
                name,
                (binder, condition, span, cap, reference, target, type_name),
            );
        }
        let urgent_enabled = urgent
            .iter()
            .flat_map(|name| {
                let branch = format!("{name}__b");
                items.iter().filter_map(move |item| match item {
                    SpecItem::Action {
                        name: action_name, ..
                    } if action_name == name || action_name.starts_with(&branch) => {
                        action_enabled_expression(item)
                    }
                    _ => None,
                })
            })
            .collect::<Vec<_>>();
        let mut tick_items = Vec::new();
        if !urgent_enabled.is_empty() {
            tick_items.push(ActionItem::Requires(
                Expr::Not(Box::new(or_all(urgent_enabled))),
                time_span,
            ));
        }
        for (binder, condition, span, cap, reference, target, _) in age_info.values() {
            let increment = Statement::If {
                condition: Expr::Binary {
                    op: "<".to_owned(),
                    left: Box::new(reference.clone()),
                    right: Box::new(Expr::Num(*cap)),
                },
                then_statements: vec![Statement::Assign {
                    target: target.clone(),
                    value: Expr::Binary {
                        op: "+".to_owned(),
                        left: Box::new(reference.clone()),
                        right: Box::new(Expr::Num(1)),
                    },
                    span: *span,
                }],
                else_statements: Vec::new(),
                span: *span,
            };
            let update = Statement::If {
                condition: condition.clone(),
                then_statements: vec![increment],
                else_statements: vec![Statement::Assign {
                    target: target.clone(),
                    value: Expr::Num(0),
                    span: *span,
                }],
                span: *span,
            };
            tick_items.push(ActionItem::Statement(binder.as_ref().map_or(
                update.clone(),
                |binder| Statement::ForAll {
                    binder: binder.clone(),
                    statements: vec![update],
                    span: *span,
                },
            )));
        }
        items.push(SpecItem::Action {
            name: "tick".to_owned(),
            params: Vec::new(),
            items: tick_items,
            span: time_span,
            fair: false,
            meta: None,
            sync: false,
        });
        for (index, (name, bound, span, metadata)) in deadlines.iter().enumerate() {
            let Some((binder, _, _, _, reference, _, _)) = age_info.get(name) else {
                return Err(core_error(
                    format!("deadline references undeclared age '{name}'"),
                    *span,
                ));
            };
            let expression = Expr::Binary {
                op: "<=".to_owned(),
                left: Box::new(reference.clone()),
                right: Box::new(bound.clone()),
            };
            let expression =
                binder
                    .as_ref()
                    .map_or(expression.clone(), |binder| Expr::Quantified {
                        quantifier: "forall".to_owned(),
                        binder: binder.clone(),
                        body: Box::new(expression),
                    });
            let safe_id = metadata
                .as_ref()
                .map_or("deadline", |metadata| metadata.id.as_str())
                .chars()
                .map(|character| {
                    if character.is_alphanumeric() || character == '_' {
                        character
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            items.push(SpecItem::Invariant {
                name: format!("_deadline_{safe_id}_{name}_{}", index + 1),
                expr: Box::new(expression),
                span: *span,
                meta: metadata.clone(),
            });
        }
    }
    lower_direct_spec(SurfaceSpec {
        name: requirements.name,
        meta: None,
        items,
    })
}

/// Lower a governance catalog to its executable sentinel kernel.
///
/// # Errors
///
/// Returns [`CoreError`] if the generated kernel cannot be lowered.
pub fn lower_governance(governance: SurfaceGovernance) -> Result<KernelSpec, CoreError> {
    let span = zero_span();
    let metadata = meta("GOV", format!("governance catalog {}", governance.name));
    lower_direct_spec(SurfaceSpec {
        name: governance.name,
        meta: None,
        items: vec![
            SpecItem::Type {
                name: "_GovernanceUnit".to_owned(),
                lo: Box::new(Expr::Num(0)),
                hi: Box::new(Expr::Num(0)),
                symmetric: false,
            },
            SpecItem::State(vec![StateField::generated(
                "_governance_ok",
                TypeExpr::Bool,
                span,
            )]),
            SpecItem::Init {
                statements: vec![Statement::Assign {
                    target: LValue::Var("_governance_ok".to_owned()),
                    value: Expr::Bool(true),
                    span,
                }],
                meta: None,
            },
            SpecItem::Action {
                name: "_governance_noop".to_owned(),
                params: Vec::new(),
                items: vec![ActionItem::Requires(Expr::Bool(false), span)],
                span,
                fair: false,
                meta: metadata.clone(),
                sync: false,
            },
            SpecItem::Invariant {
                name: "_governance_catalog_ok".to_owned(),
                expr: Box::new(Expr::Binary {
                    op: "==".to_owned(),
                    left: Box::new(Expr::Var("_governance_ok".to_owned())),
                    right: Box::new(Expr::Bool(true)),
                }),
                span,
                meta: metadata,
            },
            SpecItem::Terminal {
                expr: Box::new(Expr::Bool(true)),
                span,
            },
        ],
    })
}

fn lower_catalog_sentinel(name: String, prefix: &str, id: &str) -> Result<KernelSpec, CoreError> {
    let span = zero_span();
    let state_name = format!("_{prefix}_ok");
    let action_name = format!("_{prefix}_noop");
    let invariant_name = format!("_{prefix}_catalog_ok");
    let metadata = meta(id, format!("{prefix} catalog {name}"));
    lower_direct_spec(SurfaceSpec {
        name,
        meta: None,
        items: vec![
            SpecItem::State(vec![StateField::generated(
                state_name.clone(),
                TypeExpr::Bool,
                span,
            )]),
            SpecItem::Init {
                statements: vec![Statement::Assign {
                    target: LValue::Var(state_name.clone()),
                    value: Expr::Bool(true),
                    span,
                }],
                meta: None,
            },
            SpecItem::Action {
                name: action_name,
                params: Vec::new(),
                items: vec![ActionItem::Requires(Expr::Bool(false), span)],
                span,
                fair: false,
                meta: metadata.clone(),
                sync: false,
            },
            SpecItem::Invariant {
                name: invariant_name,
                expr: Box::new(Expr::Var(state_name)),
                span,
                meta: metadata,
            },
            SpecItem::Terminal {
                expr: Box::new(Expr::Bool(true)),
                span,
            },
        ],
    })
}

/// Lower a database compatibility document to its executable catalog kernel.
///
/// # Errors
///
/// Returns [`CoreError`] if the generated kernel catalog is invalid.
pub fn lower_db(system: &fsl_syntax::DbSystem) -> Result<KernelSpec, CoreError> {
    let source = crate::db_kernel_source(system);
    let spec = fsl_syntax::parse_surface_spec(&source)?;
    lower_direct_spec(spec)
}

/// Lower a Functional-DDD document to its executable catalog kernel.
///
/// # Errors
///
/// Returns [`CoreError`] if the generated kernel catalog is invalid.
pub fn lower_domain(domain: &fsl_syntax::DomainSpec) -> Result<KernelSpec, CoreError> {
    let (surface, origins) = crate::domain_lowering::lower_domain_surface(domain)?;
    crate::lower_direct_spec_with_origins(surface, origins)
}

/// Lower an AI hard-contract document to its executable catalog kernel.
///
/// # Errors
///
/// Returns [`CoreError`] if the generated kernel catalog is invalid.
pub fn lower_ai_component(component: fsl_syntax::AiComponent) -> Result<KernelSpec, CoreError> {
    lower_catalog_sentinel(component.name, "ai", "AI")
}

/// Extract executable acceptance and must-forbid traces from requirements.
///
/// # Errors
///
/// Returns [`CoreError`] when the source cannot be parsed.
pub fn requirements_trace_contract(
    source: &str,
) -> Result<Option<RequirementsTraceContract>, CoreError> {
    let document = fsl_syntax::parse_surface_document(source)?;
    let SurfaceDocument::Requirements(requirements) = document else {
        return Ok(None);
    };
    let mut acceptance = Vec::new();
    let mut forbidden = Vec::new();
    for item in requirements.items {
        let convert_steps = |steps: Vec<fsl_syntax::AcceptanceStep>| {
            steps
                .into_iter()
                .map(|step| RequirementsTraceStep {
                    name: step.name,
                    args: step.args,
                    line: step.span.start.line,
                    column: step.span.start.column,
                })
                .collect()
        };
        match item {
            RequirementsItem::Acceptance {
                id,
                text,
                steps,
                expectation,
                span,
            } => {
                let expectation = match expectation {
                    fsl_syntax::AcceptanceExpectation::Expr(expr, _) => {
                        RequirementsTraceExpectation::Expr(expr)
                    }
                    fsl_syntax::AcceptanceExpectation::Stage {
                        entity,
                        instance,
                        stage,
                        ..
                    } => RequirementsTraceExpectation::Stage {
                        entity,
                        instance,
                        stage,
                    },
                };
                acceptance.push(RequirementsTraceCase {
                    id,
                    text,
                    steps: convert_steps(steps),
                    expectation: Some(expectation),
                    line: span.start.line,
                    column: span.start.column,
                });
            }
            RequirementsItem::Forbidden {
                id,
                text,
                steps,
                span,
            } => forbidden.push(RequirementsTraceCase {
                id,
                text,
                steps: convert_steps(steps),
                expectation: None,
                line: span.start.line,
                column: span.start.column,
            }),
            _ => {}
        }
    }
    Ok(Some(RequirementsTraceContract {
        acceptance,
        forbidden,
    }))
}

/// Extract governance catalog relationships needed by CLI reporting.
///
/// # Errors
///
/// Returns [`CoreError`] for malformed governance source or incomplete
/// preservation declarations.
pub fn governance_contract(source: &str) -> Result<Option<GovernanceContract>, CoreError> {
    let document = fsl_syntax::parse_surface_document(source)?;
    let SurfaceDocument::Governance(governance) = document else {
        return Ok(None);
    };
    let mut controls = Vec::new();
    let mut delegates = Vec::new();
    let mut preservations = Vec::new();
    for item in governance.items {
        match item {
            GovernanceItem::Control { id, .. } => controls.push(id),
            GovernanceItem::Delegates {
                business_name,
                items,
                ..
            } => {
                let mut required = Vec::new();
                let mut satisfied = BTreeMap::<String, Vec<(String, String)>>::new();
                for item in items {
                    match item {
                        GovernanceDelegateItem::Require(id, _) => required.push(id),
                        GovernanceDelegateItem::Satisfaction {
                            control_id,
                            artifacts,
                            ..
                        } => {
                            let projected = artifacts
                                .into_iter()
                                .map(|artifact| match artifact {
                                    GovernanceArtifactRef::Policy(id, _) => {
                                        ("policy".to_owned(), id)
                                    }
                                    GovernanceArtifactRef::Goal(id, _) => ("goal".to_owned(), id),
                                })
                                .collect();
                            satisfied.insert(control_id, projected);
                        }
                    }
                }
                delegates.push(GovernanceDelegate {
                    business: business_name,
                    required,
                    satisfied,
                });
            }
            GovernanceItem::Preservation { name, items, span } => {
                let mut before = None;
                let mut after = None;
                let mut preserve = Vec::new();
                let mut refinement = None;
                for item in items {
                    match item {
                        PreservationItem::Before {
                            spec_name, path, ..
                        } => before = Some((spec_name, path)),
                        PreservationItem::After {
                            spec_name, path, ..
                        } => after = Some((spec_name, path)),
                        PreservationItem::Preserve(id, _) => preserve.push(id),
                        PreservationItem::Refinement(path, _) => refinement = Some(path),
                    }
                }
                let (before_name, before_path) = before.ok_or_else(|| {
                    core_error("governance preservation missing before".to_owned(), span)
                })?;
                let (after_name, after_path) = after.ok_or_else(|| {
                    core_error("governance preservation missing after".to_owned(), span)
                })?;
                let refinement_path = refinement.ok_or_else(|| {
                    core_error(
                        "governance preservation missing refinement".to_owned(),
                        span,
                    )
                })?;
                preservations.push(GovernancePreservation {
                    name,
                    before_name,
                    before_path,
                    after_name,
                    after_path,
                    preserve,
                    refinement_path,
                });
            }
            GovernanceItem::Authority { .. } => {}
        }
    }
    Ok(Some(GovernanceContract {
        name: governance.name,
        controls,
        delegates,
        preservations,
    }))
}

fn zero_span() -> fsl_syntax::Span {
    let position = fsl_syntax::SourcePos {
        offset: 0,
        line: 0,
        column: 0,
    };
    fsl_syntax::Span {
        start: position,
        end: position,
    }
}
