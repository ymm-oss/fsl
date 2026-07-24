// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::{Map, Value, json};

use crate::{
    AiComponent, Annotations, Binder, DbSystem, DomainSpec, Expr, QualifiedName, Span, SymbolPath,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetaTag {
    pub id: String,
    pub text: Option<String>,
    pub span: Option<Span>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeExpr {
    Int,
    Bool,
    Range(Expr, Expr),
    Map(Box<Self>, Box<Self>),
    Relation(Box<Self>, Box<Self>),
    Set(Box<Self>),
    Seq(Box<Self>, Expr),
    Option(Box<Self>),
    Name(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Param {
    Typed(String, QualifiedName),
    Range(String, Expr, Expr),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LValue {
    Var(String),
    Index(String, Expr),
    Field(Box<Self>, String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Statement {
    Assign {
        target: LValue,
        value: Expr,
        span: Span,
    },
    If {
        condition: Expr,
        then_statements: Vec<Self>,
        else_statements: Vec<Self>,
        span: Span,
    },
    ForAll {
        binder: Binder,
        statements: Vec<Self>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionItem {
    Requires(Expr, Span),
    Ensures(Expr, Span),
    Let(String, Expr, Span),
    Statement(Statement),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HelpfulAction {
    pub action: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

/// One Kernel state declaration, retaining optional inline-initializer syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateField {
    pub name: String,
    pub ty: TypeExpr,
    pub initializer: Option<Expr>,
    pub span: Span,
    pub initializer_span: Option<Span>,
}

impl StateField {
    #[must_use]
    pub fn generated(name: impl Into<String>, ty: TypeExpr, span: Span) -> Self {
        Self {
            name: name.into(),
            ty,
            initializer: None,
            span,
            initializer_span: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifyItem {
    Instances(String, i64, Span),
    Values(String, Box<Expr>, Box<Expr>, Span),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpecItem {
    Const {
        name: String,
        value: Box<Expr>,
    },
    Def {
        name: String,
        params: Vec<(String, QualifiedName)>,
        value: Box<Expr>,
        span: Span,
    },
    Type {
        name: String,
        lo: Box<Expr>,
        hi: Box<Expr>,
        symmetric: bool,
    },
    Enum {
        name: String,
        members: Vec<String>,
        symmetric: bool,
    },
    Struct {
        name: String,
        fields: Vec<(String, TypeExpr)>,
    },
    Entity(String, Span),
    Number(String, Span),
    State(Vec<StateField>),
    Init {
        statements: Vec<Statement>,
        meta: Option<MetaTag>,
        annotations: Annotations,
    },
    Action {
        name: String,
        params: Vec<Param>,
        items: Vec<ActionItem>,
        span: Span,
        fair: bool,
        meta: Option<MetaTag>,
        sync: bool,
        annotations: Annotations,
    },
    Invariant {
        name: String,
        expr: Box<Expr>,
        span: Span,
        meta: Option<MetaTag>,
        annotations: Annotations,
    },
    Trans {
        name: String,
        expr: Box<Expr>,
        span: Span,
        meta: Option<MetaTag>,
        annotations: Annotations,
    },
    Reachable {
        name: String,
        expr: Box<Expr>,
        span: Span,
        meta: Option<MetaTag>,
        annotations: Annotations,
    },
    Terminal {
        expr: Box<Expr>,
        span: Span,
    },
    Until {
        name: String,
        before: Box<Expr>,
        after: Box<Expr>,
        span: Span,
        meta: Option<MetaTag>,
        annotations: Annotations,
    },
    Unless {
        name: String,
        before: Box<Expr>,
        after: Box<Expr>,
        span: Span,
        meta: Option<MetaTag>,
        annotations: Annotations,
    },
    LeadsTo {
        name: String,
        binders: Vec<Binder>,
        before: Box<Expr>,
        after: Box<Expr>,
        span: Span,
        meta: Option<MetaTag>,
        decreases: Option<Box<Expr>>,
        within: Option<Box<Expr>>,
        helpful: Vec<HelpfulAction>,
        annotations: Annotations,
    },
    VerifyBounds {
        items: Vec<VerifyItem>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceSpec {
    pub name: String,
    pub meta: Option<MetaTag>,
    pub items: Vec<SpecItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementParam {
    pub name: String,
    pub ty: Option<TypeExpr>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionTarget {
    Stutter,
    Action(String, Vec<Expr>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrespondenceOrigin {
    InlineMapsClause,
    ImplementsBlock,
    RefinementFile,
    Auto,
}

impl CorrespondenceOrigin {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InlineMapsClause => "inline_maps_clause",
            Self::ImplementsBlock => "implements_block",
            Self::RefinementFile => "refinement_file",
            Self::Auto => "auto",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefinementItem {
    Impl(String),
    Abs(String),
    MapsAuto(Span),
    EnumConversion {
        name: String,
        source: String,
        target: String,
        members: Vec<(String, String, Span)>,
        span: Span,
    },
    EnumAbstraction {
        name: String,
        source: String,
        target: String,
        members: Vec<(String, String, Span)>,
        span: Span,
    },
    Map {
        name: String,
        binder: Option<Binder>,
        expr: Box<Expr>,
        span: Span,
    },
    Action {
        name: String,
        params: Vec<RefinementParam>,
        target: ActionTarget,
        origin: CorrespondenceOrigin,
        span: Span,
    },
    PreserveProgress {
        responses: Vec<(String, Vec<String>, Span)>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceRefinement {
    pub name: String,
    pub items: Vec<RefinementItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessField {
    pub name: String,
    pub type_name: QualifiedName,
    pub initial: Option<Expr>,
    pub span: Span,
    pub type_span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessFields {
    pub fields: Vec<ProcessField>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessTransition {
    pub name: String,
    pub source: String,
    pub target: String,
    pub actor: String,
    pub inputs: Vec<Param>,
    pub guard: Option<Expr>,
    pub assignments: Vec<(String, Expr)>,
    pub covers: Option<ProcessCover>,
    pub span: Span,
    pub annotations: Annotations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessCover {
    pub id: String,
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessItem {
    Stages(Vec<String>, Span),
    Initial(String, Span),
    Transition(Box<ProcessTransition>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlAttribute {
    Owner(String),
    Severity(String),
    AppliesTo(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BusinessPolicyBody {
    Invariant(Expr),
    Responds {
        binders: Vec<Binder>,
        before: Box<Expr>,
        after: Box<Expr>,
        within: Option<Box<Expr>>,
    },
    Eventually {
        case_name: String,
        source_stage: String,
        target_stages: Vec<String>,
    },
    Precedence {
        case_name: String,
        target_stages: Vec<String>,
        waypoints: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BusinessGoalBody {
    Expr(Expr),
    SomeStage {
        case_name: String,
        stage: String,
    },
    AllStage {
        case_name: String,
        stages: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BusinessItem {
    Actor(Vec<String>, Span),
    Entity(String, Span),
    Process {
        name: SymbolPath,
        fields: Option<ProcessFields>,
        items: Vec<ProcessItem>,
        span: Span,
    },
    Kpi {
        name: String,
        case_name: String,
        stage: String,
        span: Span,
    },
    Control {
        id: String,
        text: String,
        attributes: Vec<ControlAttribute>,
        span: Span,
    },
    Policy {
        id: String,
        text: String,
        body: Box<BusinessPolicyBody>,
        span: Span,
        satisfies: Vec<String>,
        annotations: Annotations,
    },
    Goal {
        id: String,
        text: String,
        body: BusinessGoalBody,
        span: Span,
        satisfies: Vec<String>,
        annotations: Annotations,
    },
    VerifyBounds {
        items: Vec<VerifyItem>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceBusiness {
    pub name: String,
    pub items: Vec<BusinessItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GovernanceArtifactRef {
    Policy(String, Span),
    Goal(String, Span),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GovernanceDelegateItem {
    Require(String, Span),
    Satisfaction {
        control_id: String,
        artifacts: Vec<GovernanceArtifactRef>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreservationItem {
    Before {
        spec_name: String,
        path: String,
        span: Span,
    },
    After {
        spec_name: String,
        path: String,
        span: Span,
    },
    Preserve(String, Span),
    Refinement(String, Span),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GovernanceItem {
    Authority {
        authority: String,
        control_ids: Vec<String>,
        span: Span,
    },
    Control {
        id: String,
        text: String,
        attributes: Vec<ControlAttribute>,
        span: Span,
    },
    Delegates {
        business_name: String,
        path: String,
        items: Vec<GovernanceDelegateItem>,
        span: Span,
    },
    Preservation {
        name: String,
        items: Vec<PreservationItem>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceGovernance {
    pub name: String,
    pub items: Vec<GovernanceItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapsClause {
    pub target: ActionTarget,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequirementBranch {
    pub condition: Expr,
    pub statements: Vec<Statement>,
    pub maps: MapsClause,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequirementActionItem {
    Action(Box<ActionItem>),
    Branches {
        branches: Vec<RequirementBranch>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequirementAction {
    pub name: String,
    pub params: Vec<Param>,
    pub items: Vec<RequirementActionItem>,
    pub span: Span,
    pub fair: bool,
    pub meta: Option<MetaTag>,
    pub maps: Option<MapsClause>,
    pub annotations: Annotations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequirementBlockItem {
    Action(RequirementAction),
    Property(SpecItem),
    Deadline {
        name: String,
        bound: Expr,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptanceStep {
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptanceExpectation {
    Expr(Expr, Span),
    Stage {
        entity: String,
        instance: i64,
        stage: String,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimeItem {
    Urgent(Vec<String>, Span),
    Age {
        name: String,
        binder: Option<Binder>,
        condition: Expr,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequirementsItem {
    Implements {
        name: String,
        path: String,
        items: Vec<RefinementItem>,
        span: Span,
    },
    Requirement {
        id: String,
        text: String,
        items: Vec<RequirementBlockItem>,
        span: Span,
        annotations: Annotations,
    },
    Acceptance {
        id: String,
        text: String,
        steps: Vec<AcceptanceStep>,
        expectation: AcceptanceExpectation,
        span: Span,
        annotations: Annotations,
    },
    Forbidden {
        id: String,
        text: String,
        steps: Vec<AcceptanceStep>,
        span: Span,
        annotations: Annotations,
    },
    Process(BusinessItem),
    Kpi(BusinessItem),
    Action(RequirementAction),
    Time {
        items: Vec<TimeItem>,
        span: Span,
    },
    Common(SpecItem),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceRequirements {
    pub name: String,
    pub items: Vec<RequirementsItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncRef {
    pub alias: String,
    pub action: String,
    pub args: Vec<Expr>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncAction {
    pub name: String,
    pub params: Vec<Param>,
    pub refs: Vec<SyncRef>,
    pub items: Vec<ActionItem>,
    pub span: Span,
    pub fair: bool,
    pub meta: Option<MetaTag>,
    pub annotations: Annotations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComposeItem {
    Use {
        spec_name: String,
        alias: String,
        path: String,
        span: Span,
    },
    Internal {
        alias: String,
        action: String,
        span: Span,
    },
    SyncAction(SyncAction),
    Common(SpecItem),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceCompose {
    pub name: String,
    pub items: Vec<ComposeItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceAgent {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SurfaceDocument {
    Spec(SurfaceSpec),
    Refinement(SurfaceRefinement),
    Business(SurfaceBusiness),
    Governance(SurfaceGovernance),
    Requirements(SurfaceRequirements),
    Compose(SurfaceCompose),
    Db(DbSystem),
    Domain(DomainSpec),
    AiComponent(AiComponent),
    Agent(SurfaceAgent),
}

impl MetaTag {
    #[must_use]
    pub fn parse(value: &str, span: Span) -> Self {
        value.split_once(':').map_or_else(
            || Self {
                id: value.trim().to_owned(),
                text: None,
                span: Some(span),
            },
            |(id, text)| Self {
                id: id.trim().to_owned(),
                text: Some(text.trim().to_owned()),
                span: Some(span),
            },
        )
    }

    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!({"id": self.id, "text": self.text})
    }
}

impl TypeExpr {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Int => json!(["int"]),
            Self::Bool => json!(["bool"]),
            Self::Range(lo, hi) => json!(["range", lo.python_ast(), hi.python_ast()]),
            Self::Map(key, value) => json!(["map", key.python_ast(), value.python_ast()]),
            Self::Relation(source, target) => {
                json!(["relation", source.python_ast(), target.python_ast()])
            }
            Self::Set(element) => json!(["set", element.python_ast()]),
            Self::Seq(element, cap) => json!(["seq", element.python_ast(), cap.python_ast()]),
            Self::Option(inner) => json!(["option", inner.python_ast()]),
            Self::Name(name) => json!(["name", name]),
        }
    }
}

impl Param {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Typed(name, type_name) => {
                json!(["param_typed", name, type_name.python_ast()])
            }
            Self::Range(name, lo, hi) => {
                json!(["param_range", name, lo.python_ast(), hi.python_ast()])
            }
        }
    }
}

impl LValue {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Var(name) => json!(["var", name]),
            Self::Index(name, index) => json!(["index", name, index.python_ast()]),
            Self::Field(base, field) => match base.as_ref() {
                Self::Var(name) => json!(["field_lv", ["var", name], field]),
                Self::Index(name, index) => {
                    json!(["field_lv", ["index", name, index.python_ast()], field])
                }
                Self::Field(_, _) => unreachable!("grammar permits one lvalue field suffix"),
            },
        }
    }
}

impl Statement {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Assign {
                target,
                value,
                span,
            } => json!([
                "assign",
                target.python_ast(),
                value.python_ast(),
                span.python_loc()
            ]),
            Self::If {
                condition,
                then_statements,
                else_statements,
                span,
            } => json!([
                "if",
                condition.python_ast(),
                statements_ast(then_statements),
                statements_ast(else_statements),
                span.python_loc()
            ]),
            Self::ForAll {
                binder,
                statements,
                span,
            } => json!([
                "forall_stmt",
                binder.python_ast(),
                statements_ast(statements),
                span.python_loc()
            ]),
        }
    }
}

impl ActionItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Requires(expr, span) => {
                json!(["requires", expr.python_ast(), span.python_loc()])
            }
            Self::Ensures(expr, span) => {
                json!(["ensures", expr.python_ast(), span.python_loc()])
            }
            Self::Let(name, expr, span) => {
                json!(["let", name, expr.python_ast(), span.python_loc()])
            }
            Self::Statement(statement) => statement.python_ast(),
        }
    }
}

impl VerifyItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Instances(name, value, span) => {
                json!(["verify_instances", name, value, span.python_loc()])
            }
            Self::Values(name, lo, hi, span) => json!([
                "verify_values",
                name,
                lo.python_ast(),
                hi.python_ast(),
                span.python_loc()
            ]),
        }
    }
}

impl SpecItem {
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Const { name, value } => json!(["const", name, value.python_ast()]),
            Self::Def {
                name,
                params,
                value,
                span,
            } => json!([
                "def",
                name,
                params
                    .iter()
                    .map(|(name, ty)| json!(["def_param", name, ty.python_ast()]))
                    .collect::<Vec<_>>(),
                value.python_ast(),
                span.python_loc()
            ]),
            Self::Type {
                name,
                lo,
                hi,
                symmetric,
            } => {
                let mut values = vec![json!("type"), json!(name), lo.python_ast(), hi.python_ast()];
                if *symmetric {
                    values.push(json!({"symmetric": true}));
                }
                Value::Array(values)
            }
            Self::Enum {
                name,
                members,
                symmetric,
            } => {
                let mut values = vec![json!("enum"), json!(name), json!(members)];
                if *symmetric {
                    values.push(json!({"symmetric": true}));
                }
                Value::Array(values)
            }
            Self::Struct { name, fields } => {
                let fields = fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), ty.python_ast()))
                    .collect::<Map<_, _>>();
                json!(["struct", name, fields])
            }
            Self::Entity(name, span) => json!(["entity", name, span.python_loc()]),
            Self::Number(name, span) => json!(["number", name, span.python_loc()]),
            Self::State(declarations) => json!([
                "state",
                declarations
                    .iter()
                    .map(|field| {
                        let mut declaration =
                            vec![json!("decl"), json!(field.name), field.ty.python_ast()];
                        if let Some(initializer) = &field.initializer {
                            declaration.push(initializer.python_ast());
                            declaration.push(json!(field.initializer_span.map(Span::python_loc)));
                        }
                        Value::Array(declaration)
                    })
                    .collect::<Vec<_>>()
            ]),
            Self::Init {
                statements, meta, ..
            } => {
                let mut values = vec![json!("init"), Value::Array(statements_ast(statements))];
                if let Some(meta) = meta {
                    values.push(meta.python_ast());
                }
                Value::Array(values)
            }
            Self::Action {
                name,
                params,
                items,
                span,
                fair,
                meta,
                sync,
                ..
            } => {
                let mut values = vec![
                    json!("action"),
                    json!(name),
                    json!(params.iter().map(Param::python_ast).collect::<Vec<_>>()),
                    json!(items.iter().map(ActionItem::python_ast).collect::<Vec<_>>()),
                    span.python_loc(),
                    json!(fair),
                    json!(meta.as_ref().map(MetaTag::python_ast)),
                ];
                if *sync {
                    values.push(json!(true));
                }
                Value::Array(values)
            }
            Self::Invariant {
                name,
                expr,
                span,
                meta,
                ..
            } => property_ast("invariant", name, expr, *span, meta.as_ref()),
            Self::Trans {
                name,
                expr,
                span,
                meta,
                ..
            } => property_ast("trans", name, expr, *span, meta.as_ref()),
            Self::Reachable {
                name,
                expr,
                span,
                meta,
                ..
            } => property_ast("reachable", name, expr, *span, meta.as_ref()),
            Self::Terminal { expr, span } => {
                json!(["terminal", expr.python_ast(), span.python_loc()])
            }
            Self::Until {
                name,
                before,
                after,
                span,
                meta,
                ..
            } => json!([
                "until",
                name,
                before.python_ast(),
                after.python_ast(),
                span.python_loc(),
                meta.as_ref().map(MetaTag::python_ast)
            ]),
            Self::Unless {
                name,
                before,
                after,
                span,
                meta,
                ..
            } => json!([
                "unless",
                name,
                before.python_ast(),
                after.python_ast(),
                span.python_loc(),
                meta.as_ref().map(MetaTag::python_ast)
            ]),
            Self::LeadsTo {
                name,
                binders,
                before,
                after,
                span,
                meta,
                decreases,
                within,
                helpful,
                ..
            } => json!([
                "leadsto",
                name,
                binders.iter().map(Binder::python_ast).collect::<Vec<_>>(),
                before.python_ast(),
                after.python_ast(),
                span.python_loc(),
                meta.as_ref().map(MetaTag::python_ast),
                decreases.as_ref().map(|expr| expr.python_ast()),
                within.as_ref().map(|expr| expr.python_ast()),
                helpful
                    .iter()
                    .map(|entry| json!({
                        "action": entry.action,
                        "args": entry.args.iter().map(Expr::python_ast).collect::<Vec<_>>(),
                        "loc": entry.span.python_loc()
                    }))
                    .collect::<Vec<_>>()
            ]),
            Self::VerifyBounds { items, span } => json!([
                "verify_bounds",
                items.iter().map(VerifyItem::python_ast).collect::<Vec<_>>(),
                span.python_loc()
            ]),
        }
    }
}

impl SurfaceSpec {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        let mut items = Vec::new();
        if let Some(meta) = &self.meta {
            items.push(json!(["__spec_meta", meta.python_ast()]));
        }
        items.extend(self.items.iter().map(SpecItem::python_ast));
        json!(["spec", self.name, items])
    }
}

impl RefinementParam {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "refinement_param",
            self.name,
            self.ty.as_ref().map(TypeExpr::python_ast)
        ])
    }
}

impl ActionTarget {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Stutter => json!(["stutter"]),
            Self::Action(name, args) => json!([
                "action",
                name,
                args.iter().map(Expr::python_ast).collect::<Vec<_>>()
            ]),
        }
    }
}

impl RefinementItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Impl(name) => json!(["impl", name]),
            Self::Abs(name) => json!(["abs", name]),
            Self::MapsAuto(span) => json!(["maps_auto", span.python_loc()]),
            Self::EnumConversion {
                name,
                source,
                target,
                members,
                span,
            } => json!([
                "enum_conversion",
                name,
                source,
                target,
                members
                    .iter()
                    .map(|(source, target, span)| { json!([source, target, span.python_loc()]) })
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::EnumAbstraction {
                name,
                source,
                target,
                members,
                span,
            } => json!([
                "enum_abstraction",
                name,
                source,
                target,
                members
                    .iter()
                    .map(|(source, target, span)| { json!([source, target, span.python_loc()]) })
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Map {
                name,
                binder,
                expr,
                span,
            } => json!([
                "map",
                name,
                binder.as_ref().map(Binder::python_ast),
                expr.python_ast(),
                span.python_loc()
            ]),
            Self::Action {
                name,
                params,
                target,
                origin: _,
                span,
            } => json!([
                "action_map",
                name,
                params
                    .iter()
                    .map(RefinementParam::python_ast)
                    .collect::<Vec<_>>(),
                target.python_ast(),
                span.python_loc()
            ]),
            Self::PreserveProgress { responses, span } => json!([
                "preserve_progress",
                responses
                    .iter()
                    .map(|(name, actions, span)| {
                        json!(["progress_respond", name, actions, span.python_loc()])
                    })
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
        }
    }
}

impl SurfaceRefinement {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "refinement",
            self.name,
            self.items
                .iter()
                .map(RefinementItem::python_ast)
                .collect::<Vec<_>>()
        ])
    }
}

impl ProcessField {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "proc_field",
            self.name,
            self.type_name.python_ast(),
            self.initial.as_ref().map(Expr::python_ast)
        ])
    }
}

impl ProcessFields {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "proc_fields",
            self.fields
                .iter()
                .map(ProcessField::python_ast)
                .collect::<Vec<_>>(),
            self.span.python_loc()
        ])
    }
}

impl ProcessItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Stages(names, span) => json!(["biz_stages", names, span.python_loc()]),
            Self::Initial(name, span) => json!(["biz_initial", name, span.python_loc()]),
            Self::Transition(transition) => {
                let mut extra = Map::new();
                let has_extra = !transition.inputs.is_empty()
                    || transition.guard.is_some()
                    || !transition.assignments.is_empty()
                    || transition.covers.is_some();
                if has_extra {
                    extra.insert(
                        "inputs".to_owned(),
                        Value::Array(transition.inputs.iter().map(Param::python_ast).collect()),
                    );
                    extra.insert(
                        "guard".to_owned(),
                        transition
                            .guard
                            .as_ref()
                            .map_or(Value::Null, Expr::python_ast),
                    );
                    extra.insert(
                        "sets".to_owned(),
                        Value::Array(
                            transition
                                .assignments
                                .iter()
                                .map(|(name, expr)| json!(["proc_assign", name, expr.python_ast()]))
                                .collect(),
                        ),
                    );
                    extra.insert(
                        "covers".to_owned(),
                        transition
                            .covers
                            .as_ref()
                            .map_or(Value::Null, |cover| json!([cover.id, cover.text])),
                    );
                }
                json!([
                    "biz_transition",
                    transition.name,
                    transition.source,
                    transition.target,
                    transition.actor,
                    extra,
                    transition.span.python_loc()
                ])
            }
        }
    }
}

impl ControlAttribute {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Owner(name) => json!(["control_owner", name]),
            Self::Severity(name) => json!(["control_severity", name]),
            Self::AppliesTo(name) => json!(["control_applies_to", name]),
        }
    }
}

impl BusinessPolicyBody {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Invariant(expr) => json!(["biz_policy_invariant", expr.python_ast()]),
            Self::Responds {
                binders,
                before,
                after,
                within,
            } => json!([
                "biz_policy_responds",
                binders.iter().map(Binder::python_ast).collect::<Vec<_>>(),
                before.python_ast(),
                after.python_ast(),
                within.as_deref().map(Expr::python_ast)
            ]),
            Self::Eventually {
                case_name,
                source_stage,
                target_stages,
            } => json!([
                "biz_policy_eventually",
                case_name,
                source_stage,
                target_stages
            ]),
            Self::Precedence {
                case_name,
                target_stages,
                waypoints,
            } => json!(["biz_policy_precedence", case_name, target_stages, waypoints]),
        }
    }
}

impl BusinessGoalBody {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Expr(expr) => json!(["biz_goal_expr", expr.python_ast()]),
            Self::SomeStage { case_name, stage } => {
                json!(["biz_goal_some_stage", case_name, stage])
            }
            Self::AllStage { case_name, stages } => {
                json!(["biz_goal_all_stage", case_name, stages])
            }
        }
    }
}

impl BusinessItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Actor(names, span) => json!(["biz_actor", names, span.python_loc()]),
            Self::Entity(name, span) => json!(["entity", name, span.python_loc()]),
            Self::Process {
                name,
                fields,
                items,
                span,
            } => json!([
                "biz_process",
                name.to_string(),
                fields.as_ref().map(ProcessFields::python_ast),
                items
                    .iter()
                    .map(ProcessItem::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Kpi {
                name,
                case_name,
                stage,
                span,
            } => json!(["biz_kpi", name, case_name, stage, span.python_loc()]),
            Self::Control {
                id,
                text,
                attributes,
                span,
            } => json!([
                "biz_control",
                id,
                text,
                attributes
                    .iter()
                    .map(ControlAttribute::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Policy {
                id,
                text,
                body,
                span,
                satisfies,
                ..
            } => json!([
                "biz_policy",
                id,
                text,
                body.python_ast(),
                span.python_loc(),
                satisfies
            ]),
            Self::Goal {
                id,
                text,
                body,
                span,
                satisfies,
                ..
            } => json!([
                "biz_goal",
                id,
                text,
                body.python_ast(),
                span.python_loc(),
                satisfies
            ]),
            Self::VerifyBounds { items, span } => json!([
                "verify_bounds",
                items.iter().map(VerifyItem::python_ast).collect::<Vec<_>>(),
                span.python_loc()
            ]),
        }
    }
}

impl SurfaceBusiness {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "business",
            self.name,
            self.items
                .iter()
                .map(BusinessItem::python_ast)
                .collect::<Vec<_>>()
        ])
    }
}

impl GovernanceArtifactRef {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Policy(id, span) => json!(["policy", id, span.python_loc()]),
            Self::Goal(id, span) => json!(["goal", id, span.python_loc()]),
        }
    }
}

impl GovernanceDelegateItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Require(id, span) => json!(["gov_require", id, span.python_loc()]),
            Self::Satisfaction {
                control_id,
                artifacts,
                span,
            } => json!([
                "gov_satisfaction",
                control_id,
                artifacts
                    .iter()
                    .map(GovernanceArtifactRef::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
        }
    }
}

impl PreservationItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Before {
                spec_name,
                path,
                span,
            } => json!(["preservation_before", spec_name, path, span.python_loc()]),
            Self::After {
                spec_name,
                path,
                span,
            } => json!(["preservation_after", spec_name, path, span.python_loc()]),
            Self::Preserve(id, span) => {
                json!(["preservation_preserve", id, span.python_loc()])
            }
            Self::Refinement(path, span) => {
                json!(["preservation_refinement", path, span.python_loc()])
            }
        }
    }
}

impl GovernanceItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Authority {
                authority,
                control_ids,
                span,
            } => json!(["gov_authority", authority, control_ids, span.python_loc()]),
            Self::Control {
                id,
                text,
                attributes,
                span,
            } => json!([
                "biz_control",
                id,
                text,
                attributes
                    .iter()
                    .map(ControlAttribute::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Delegates {
                business_name,
                path,
                items,
                span,
            } => json!([
                "gov_delegates",
                business_name,
                path,
                items
                    .iter()
                    .map(GovernanceDelegateItem::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Preservation { name, items, span } => json!([
                "gov_preservation",
                name,
                items
                    .iter()
                    .map(PreservationItem::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
        }
    }
}

impl SurfaceGovernance {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "governance",
            self.name,
            self.items
                .iter()
                .map(GovernanceItem::python_ast)
                .collect::<Vec<_>>()
        ])
    }
}

impl MapsClause {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!(["maps", self.target.python_ast(), self.span.python_loc()])
    }
}

impl RequirementBranch {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "branch",
            self.condition.python_ast(),
            statements_ast(&self.statements),
            self.maps.python_ast(),
            self.span.python_loc()
        ])
    }
}

impl RequirementActionItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Action(item) => item.python_ast(),
            Self::Branches { branches, span } => json!([
                "branches",
                branches
                    .iter()
                    .map(RequirementBranch::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
        }
    }
}

impl RequirementAction {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "req_action",
            self.name,
            self.params
                .iter()
                .map(Param::python_ast)
                .collect::<Vec<_>>(),
            self.items
                .iter()
                .map(RequirementActionItem::python_ast)
                .collect::<Vec<_>>(),
            self.span.python_loc(),
            self.fair,
            self.meta.as_ref().map(MetaTag::python_ast),
            self.maps.as_ref().map(MapsClause::python_ast)
        ])
    }
}

impl RequirementBlockItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Action(action) => action.python_ast(),
            Self::Property(item) => item.python_ast(),
            Self::Deadline { name, bound, span } => {
                json!(["deadline", name, bound.python_ast(), span.python_loc()])
            }
        }
    }
}

impl AcceptanceStep {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "acceptance_step",
            self.name,
            self.args.iter().map(Expr::python_ast).collect::<Vec<_>>(),
            self.span.python_loc()
        ])
    }
}

impl AcceptanceExpectation {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Expr(expr, span) => {
                json!(["acceptance_expect", expr.python_ast(), span.python_loc()])
            }
            Self::Stage {
                entity,
                instance,
                stage,
                span,
            } => json!([
                "acceptance_expect_stage",
                entity,
                instance,
                stage,
                span.python_loc()
            ]),
        }
    }
}

impl TimeItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Urgent(names, span) => json!(["time_urgent", names, span.python_loc()]),
            Self::Age {
                name,
                binder,
                condition,
                span,
            } => json!([
                "time_age",
                name,
                binder.as_ref().map(Binder::python_ast),
                condition.python_ast(),
                span.python_loc()
            ]),
        }
    }
}

impl RequirementsItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Implements {
                name,
                path,
                items,
                span,
            } => json!([
                "implements",
                name,
                path,
                items
                    .iter()
                    .map(RefinementItem::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Requirement {
                id,
                text,
                items,
                span,
                ..
            } => json!([
                "requirement",
                id,
                text,
                items
                    .iter()
                    .map(RequirementBlockItem::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Acceptance {
                id,
                text,
                steps,
                expectation,
                span,
                ..
            } => json!([
                "acceptance",
                id,
                text,
                steps
                    .iter()
                    .map(AcceptanceStep::python_ast)
                    .collect::<Vec<_>>(),
                expectation.python_ast(),
                span.python_loc()
            ]),
            Self::Forbidden {
                id,
                text,
                steps,
                span,
                ..
            } => json!([
                "forbidden",
                id,
                text,
                steps
                    .iter()
                    .map(AcceptanceStep::python_ast)
                    .collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Process(item) | Self::Kpi(item) => item.python_ast(),
            Self::Action(action) => action.python_ast(),
            Self::Time { items, span } => json!([
                "time",
                items.iter().map(TimeItem::python_ast).collect::<Vec<_>>(),
                span.python_loc()
            ]),
            Self::Common(item) => item.python_ast(),
        }
    }
}

impl SurfaceRequirements {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "requirements",
            self.name,
            self.items
                .iter()
                .map(RequirementsItem::python_ast)
                .collect::<Vec<_>>()
        ])
    }
}

impl SyncRef {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "sync_ref",
            self.alias,
            self.action,
            self.args.iter().map(Expr::python_ast).collect::<Vec<_>>()
        ])
    }
}

impl SyncAction {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "sync_action",
            self.name,
            self.params
                .iter()
                .map(Param::python_ast)
                .collect::<Vec<_>>(),
            self.refs
                .iter()
                .map(SyncRef::python_ast)
                .collect::<Vec<_>>(),
            self.items
                .iter()
                .map(ActionItem::python_ast)
                .collect::<Vec<_>>(),
            self.span.python_loc(),
            self.fair,
            self.meta.as_ref().map(MetaTag::python_ast)
        ])
    }
}

impl ComposeItem {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Use {
                spec_name,
                alias,
                path,
                span,
            } => json!(["use", spec_name, alias, path, span.python_loc()]),
            Self::Internal {
                alias,
                action,
                span,
            } => json!(["internal", alias, action, span.python_loc()]),
            Self::SyncAction(action) => action.python_ast(),
            Self::Common(item) => item.python_ast(),
        }
    }
}

impl SurfaceCompose {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        json!([
            "compose",
            self.name,
            self.items
                .iter()
                .map(ComposeItem::python_ast)
                .collect::<Vec<_>>()
        ])
    }
}

impl SurfaceDocument {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        match self {
            Self::Spec(spec) => spec.python_ast(),
            Self::Refinement(refinement) => refinement.python_ast(),
            Self::Business(business) => business.python_ast(),
            Self::Governance(governance) => governance.python_ast(),
            Self::Requirements(requirements) => requirements.python_ast(),
            Self::Compose(compose) => compose.python_ast(),
            Self::Db(system) => system.python_ast(),
            Self::Domain(domain) => domain.python_ast(),
            Self::AiComponent(component) => component.python_ast(),
            Self::Agent(agent) => json!({
                "$type": "Agent", "name": agent.name, "loc": agent.span.python_loc(),
            }),
        }
    }
}

fn statements_ast(statements: &[Statement]) -> Vec<Value> {
    statements.iter().map(Statement::python_ast).collect()
}

fn property_ast(kind: &str, name: &str, expr: &Expr, span: Span, meta: Option<&MetaTag>) -> Value {
    json!([
        kind,
        name,
        expr.python_ast(),
        span.python_loc(),
        meta.map(MetaTag::python_ast)
    ])
}
