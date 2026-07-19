// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{
    ActionItem, AggregateKind, Annotations, Binder, DomainAggregate, DomainDecide, DomainEffect,
    DomainField, DomainLoc, DomainSaga, DomainSagaStep, DomainSpec, DomainType,
    DomainTypeSourceForm, Expr, LValue, MetaTag, Param, Pattern, QualifiedName, Span, SpecItem,
    StateField, Statement, SurfaceSpec, SyntaxBinder, SyntaxExpr, SyntaxExprKind, SyntaxIdent,
    SyntaxLValue, SyntaxPattern, SyntaxQualifiedName, SyntaxTypeExpr, SyntaxTypeExprKind, TypeExpr,
};

use crate::CoreError;
use crate::{
    LoweringStep, OriginChain, OriginId, OriginRegistry, OriginSite, SPEC_TARGET, TERMINAL_TARGET,
    action_guard_target, action_statement_target, action_target, init_statement_target,
    property_target, state_target, type_target,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum LogicalType {
    Int,
    Bool,
    Named(String),
    Map(Box<Self>, Box<Self>),
    Set(Box<Self>),
    Seq(Box<Self>),
    Option(Box<Self>),
    Unknown,
}

#[derive(Clone, Debug)]
struct Symbol {
    kernel_name: String,
    ty: LogicalType,
}

type Scope = BTreeMap<String, Symbol>;

#[derive(Clone, Debug)]
struct ResolvedExpr {
    expr: Expr,
    ty: LogicalType,
}

fn error_at(message: impl Into<String>, span: Span) -> CoreError {
    CoreError {
        message: message.into(),
        line: span.start.line,
        column: span.start.column,
        origin: Some(Box::new(OriginChain {
            id: OriginId(format!(
                "domain:error:{}:{}",
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
                kind: "resolve_domain_expression".to_owned(),
                detail: None,
            }],
            generated: false,
        })),
    }
}

fn span_at(loc: DomainLoc) -> Span {
    loc.span()
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

pub(crate) fn effect_outcome_member(effect: &DomainEffect, event: &str) -> &'static str {
    if effect.success_event.as_deref() == Some(event) {
        return "Succeeded";
    }
    if effect.failure_event.as_deref() == Some(event) {
        return "Failed";
    }
    if effect.timeout_event.as_deref() == Some(event) {
        return "TimedOut";
    }
    let lowered = event.to_ascii_lowercase();
    if lowered.contains("timeout") || lowered.contains("timedout") {
        "TimedOut"
    } else if lowered.contains("fail") {
        "Failed"
    } else if lowered.contains("cancel") {
        "Cancelled"
    } else {
        "Succeeded"
    }
}

pub(crate) fn validate_effect_outcome_roles(domain: &DomainSpec) -> Result<(), CoreError> {
    for effect in &domain.effects {
        if let Some(message) = effect.outcome_role_conflict() {
            return Err(error_at(message, span_at(effect.loc)));
        }
        for event in effect.explicit_outcome_events() {
            if !domain
                .aggregates
                .iter()
                .flat_map(|aggregate| &aggregate.events)
                .any(|candidate| candidate.name == *event)
            {
                return Err(error_at(
                    format!("unknown domain event '{event}'"),
                    span_at(effect.loc),
                ));
            }
        }
    }
    Ok(())
}

fn and_all(mut values: Vec<Expr>) -> Expr {
    if values.is_empty() {
        return Expr::Bool(true);
    }
    let first = values.remove(0);
    values.into_iter().fold(first, |left, right| Expr::Binary {
        op: "and".to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn or_all(mut values: Vec<Expr>) -> Expr {
    if values.is_empty() {
        return Expr::Bool(false);
    }
    let first = values.remove(0);
    values.into_iter().fold(first, |left, right| Expr::Binary {
        op: "or".to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn qualified(name: &str) -> QualifiedName {
    QualifiedName {
        namespace: None,
        name: name.to_owned(),
    }
}

fn type_name(ty: &LogicalType) -> Option<&str> {
    match ty {
        LogicalType::Named(name) => Some(name),
        _ => None,
    }
}

fn logical_qualified_name(ty: &LogicalType, span: Span) -> Result<QualifiedName, CoreError> {
    match ty {
        LogicalType::Int => Ok(qualified("Int")),
        LogicalType::Bool => Ok(qualified("Bool")),
        LogicalType::Named(name) => Ok(qualified(name)),
        _ => Err(error_at("map keys require a scalar or named type", span)),
    }
}

fn needs_expected_type(expression: &SyntaxExpr) -> bool {
    match &expression.kind {
        SyntaxExprKind::Name(_) | SyntaxExprKind::None | SyntaxExprKind::Some(_) => true,
        SyntaxExprKind::Set(values) | SyntaxExprKind::Seq(values) => values.is_empty(),
        SyntaxExprKind::Group(value) => needs_expected_type(value),
        _ => false,
    }
}

struct Resolver<'a> {
    domain: &'a DomainSpec,
    types: Vec<DomainType>,
    enums: BTreeMap<String, BTreeSet<String>>,
    enum_candidates: BTreeMap<String, BTreeSet<String>>,
}

impl<'a> Resolver<'a> {
    fn compatible(&self, left: &LogicalType, right: &LogicalType) -> bool {
        if left == right
            || matches!(left, LogicalType::Unknown)
            || matches!(right, LogicalType::Unknown)
        {
            return true;
        }
        let numeric = |ty: &LogicalType| match ty {
            LogicalType::Int => true,
            LogicalType::Named(name) => self
                .types
                .iter()
                .find(|candidate| candidate.name == *name)
                .is_some_and(|definition| matches!(definition.kind.as_str(), "range" | "external")),
            _ => false,
        };
        if numeric(left) && numeric(right) {
            return true;
        }
        match (left, right) {
            (LogicalType::Map(left_key, left_value), LogicalType::Map(right_key, right_value)) => {
                self.compatible(left_key, right_key) && self.compatible(left_value, right_value)
            }
            (LogicalType::Set(left), LogicalType::Set(right))
            | (LogicalType::Seq(left), LogicalType::Seq(right))
            | (LogicalType::Option(left), LogicalType::Option(right)) => {
                self.compatible(left, right)
            }
            _ => false,
        }
    }

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
                .chain(aggregate.commands.iter().flat_map(|item| &item.inputs))
                .chain(aggregate.events.iter().flat_map(|item| &item.fields))
            {
                Self::collect_named_types(&field.type_name, &mut references);
            }
        }
        for name in references {
            if !declared.contains(&name) && !matches!(name.as_str(), "Int" | "Bool") {
                types.push(DomainType {
                    name,
                    kind: "external".to_owned(),
                    members: Vec::new(),
                    member_spans: Vec::new(),
                    lo: None,
                    hi: None,
                    fields: Vec::new(),
                    invariants: Vec::new(),
                    source_form: DomainTypeSourceForm::External,
                    span: domain.loc.span(),
                    loc: domain.loc,
                });
            }
        }
        let mut enums = BTreeMap::<String, BTreeSet<String>>::new();
        let mut enum_candidates = BTreeMap::<String, BTreeSet<String>>::new();
        for ty in &types {
            if ty.kind != "enum" {
                continue;
            }
            enums.insert(ty.name.clone(), ty.members.iter().cloned().collect());
            for member in &ty.members {
                enum_candidates
                    .entry(member.clone())
                    .or_default()
                    .insert(ty.name.clone());
            }
        }
        Self {
            domain,
            types,
            enums,
            enum_candidates,
        }
    }

    fn collect_named_types(ty: &SyntaxTypeExpr, output: &mut BTreeSet<String>) {
        match &ty.kind {
            SyntaxTypeExprKind::Name(name) => {
                output.insert(name.text.clone());
            }
            SyntaxTypeExprKind::Apply { arguments, .. } => {
                for argument in arguments {
                    Self::collect_named_types(argument, output);
                }
            }
        }
    }

    fn logical_type(&self, ty: &SyntaxTypeExpr) -> Result<LogicalType, CoreError> {
        match &ty.kind {
            SyntaxTypeExprKind::Name(name) => Ok(match name.text.as_str() {
                "Int" => LogicalType::Int,
                "Bool" => LogicalType::Bool,
                other if self.types.iter().any(|ty| ty.name == other) => {
                    LogicalType::Named(other.to_owned())
                }
                other => {
                    return Err(error_at(
                        format!("unknown domain type '{other}'"),
                        name.span,
                    ));
                }
            }),
            SyntaxTypeExprKind::Apply {
                constructor,
                arguments,
            } => match (constructor.text.as_str(), arguments.as_slice()) {
                ("Map", [key, value]) => Ok(LogicalType::Map(
                    Box::new(self.logical_type(key)?),
                    Box::new(self.logical_type(value)?),
                )),
                ("Set", [value]) => Ok(LogicalType::Set(Box::new(self.logical_type(value)?))),
                ("Seq", [value]) => Ok(LogicalType::Seq(Box::new(self.logical_type(value)?))),
                ("Option", [value]) => Ok(LogicalType::Option(Box::new(self.logical_type(value)?))),
                _ => Err(error_at(
                    format!(
                        "unsupported domain type constructor '{}'/{}",
                        constructor.text,
                        arguments.len()
                    ),
                    ty.span,
                )),
            },
        }
    }

    fn surface_type(&self, ty: &SyntaxTypeExpr) -> Result<TypeExpr, CoreError> {
        match &ty.kind {
            SyntaxTypeExprKind::Name(name) => Ok(match name.text.as_str() {
                "Int" => TypeExpr::Int,
                "Bool" => TypeExpr::Bool,
                other if self.types.iter().any(|ty| ty.name == other) => {
                    TypeExpr::Name(other.to_owned())
                }
                other => {
                    return Err(error_at(
                        format!("unknown domain type '{other}'"),
                        name.span,
                    ));
                }
            }),
            SyntaxTypeExprKind::Apply {
                constructor,
                arguments,
            } => match (constructor.text.as_str(), arguments.as_slice()) {
                ("Map", [key, value]) => Ok(TypeExpr::Map(
                    Box::new(self.surface_type(key)?),
                    Box::new(self.surface_type(value)?),
                )),
                ("Set", [value]) => Ok(TypeExpr::Set(Box::new(self.surface_type(value)?))),
                ("Option", [value]) => Ok(TypeExpr::Option(Box::new(self.surface_type(value)?))),
                _ => Err(error_at(
                    format!(
                        "unsupported domain type constructor '{}'/{}",
                        constructor.text,
                        arguments.len()
                    ),
                    ty.span,
                )),
            },
        }
    }

    fn enum_value(
        &self,
        name: &str,
        expected: Option<&LogicalType>,
        span: Span,
    ) -> Result<Option<ResolvedExpr>, CoreError> {
        if let Some(expected_name) = expected.and_then(type_name)
            && self
                .enums
                .get(expected_name)
                .is_some_and(|members| members.contains(name))
        {
            return Ok(Some(ResolvedExpr {
                expr: Expr::Var(format!("{expected_name}_{name}")),
                ty: LogicalType::Named(expected_name.to_owned()),
            }));
        }
        let Some(candidates) = self.enum_candidates.get(name) else {
            return Ok(None);
        };
        if candidates.len() != 1 {
            return Err(error_at(
                format!(
                    "ambiguous enum member '{name}'; expected one of {}",
                    candidates.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
                span,
            ));
        }
        let ty = candidates.iter().next().expect("one enum candidate");
        Ok(Some(ResolvedExpr {
            expr: Expr::Var(format!("{ty}_{name}")),
            ty: LogicalType::Named(ty.clone()),
        }))
    }

    fn scope_for_aggregate(&self, aggregate: &DomainAggregate) -> Result<Scope, CoreError> {
        aggregate
            .state
            .iter()
            .map(|field| {
                Ok((
                    field.name.text.clone(),
                    Symbol {
                        kernel_name: state_name(aggregate, &field.name),
                        ty: self.logical_type(&field.type_name)?,
                    },
                ))
            })
            .collect()
    }

    fn scope_for_fields(&self, fields: &[DomainField]) -> Result<Scope, CoreError> {
        fields
            .iter()
            .map(|field| {
                Ok((
                    field.name.text.clone(),
                    Symbol {
                        kernel_name: field.name.text.clone(),
                        ty: self.logical_type(&field.type_name)?,
                    },
                ))
            })
            .collect()
    }

    fn extend_fields(&self, scope: &mut Scope, fields: &[DomainField]) -> Result<(), CoreError> {
        for field in fields {
            scope.insert(
                field.name.text.clone(),
                Symbol {
                    kernel_name: field.name.text.clone(),
                    ty: self.logical_type(&field.type_name)?,
                },
            );
        }
        Ok(())
    }

    fn resolve_name(
        &self,
        name: &str,
        span: Span,
        expected: Option<&LogicalType>,
        scope: &Scope,
    ) -> Result<ResolvedExpr, CoreError> {
        if let Some(symbol) = scope.get(name) {
            return Ok(ResolvedExpr {
                expr: Expr::Var(symbol.kernel_name.clone()),
                ty: symbol.ty.clone(),
            });
        }
        if let Some(value) = self.enum_value(name, expected, span)? {
            return Ok(value);
        }
        Err(error_at(format!("unknown domain symbol '{name}'"), span))
    }

    #[allow(clippy::too_many_lines)]
    fn resolve_expr(
        &self,
        expression: &SyntaxExpr,
        expected: Option<&LogicalType>,
        scope: &Scope,
        aggregate: Option<&DomainAggregate>,
        expanding_can: &mut Vec<String>,
    ) -> Result<ResolvedExpr, CoreError> {
        let resolved = match &expression.kind {
            SyntaxExprKind::Num(value) => ResolvedExpr {
                expr: Expr::Num(*value),
                ty: LogicalType::Int,
            },
            SyntaxExprKind::Bool(value) => ResolvedExpr {
                expr: Expr::Bool(*value),
                ty: LogicalType::Bool,
            },
            SyntaxExprKind::None => ResolvedExpr {
                expr: Expr::None,
                ty: match expected {
                    Some(expected @ LogicalType::Option(_)) => expected.clone(),
                    Some(_) => {
                        return Err(error_at(
                            "none requires an Option expected type",
                            expression.span,
                        ));
                    }
                    None => LogicalType::Option(Box::new(LogicalType::Unknown)),
                },
            },
            SyntaxExprKind::Name(name) => {
                self.resolve_name(&name.text, name.span, expected, scope)?
            }
            SyntaxExprKind::Call { callee, args }
                if callee.text == "can" && matches!(args.as_slice(), [SyntaxExpr { .. }]) =>
            {
                let [argument] = args.as_slice() else {
                    unreachable!()
                };
                let SyntaxExprKind::Name(command) = &argument.kind else {
                    return Err(error_at("can() expects a command name", argument.span));
                };
                let Some(aggregate) = aggregate else {
                    return Err(error_at(
                        "can() is only valid in an aggregate expression",
                        expression.span,
                    ));
                };
                let Some(decide) = aggregate
                    .decides
                    .iter()
                    .find(|decide| decide.command == command.text)
                else {
                    let elsewhere = self.domain.aggregates.iter().any(|candidate| {
                        candidate.name != aggregate.name
                            && candidate
                                .commands
                                .iter()
                                .any(|item| item.name == command.text)
                    });
                    let detail = if elsewhere {
                        " belongs to another aggregate"
                    } else {
                        " is unknown"
                    };
                    return Err(error_at(
                        format!("command '{}'{}", command.text, detail),
                        expression.span,
                    ));
                };
                if expanding_can.contains(&command.text) {
                    let mut error = error_at(
                        format!("recursive can({}) expansion", command.text),
                        expression.span,
                    );
                    if let Some(origin) = &mut error.origin {
                        origin.secondary.push(OriginSite {
                            source_file: None,
                            span: Some(span_at(decide.loc)),
                            dialect: "domain".to_owned(),
                            declaration_path: vec![
                                self.domain.name.clone(),
                                "aggregate".to_owned(),
                                aggregate.name.clone(),
                                "decide".to_owned(),
                                decide.command.clone(),
                            ],
                        });
                    }
                    return Err(error);
                }
                expanding_can.push(command.text.clone());
                let result = self.resolve_can(decide, scope, aggregate, expanding_can);
                expanding_can.pop();
                ResolvedExpr {
                    expr: result?,
                    ty: LogicalType::Bool,
                }
            }
            SyntaxExprKind::Call { callee, .. } if callee.text == "can" => {
                return Err(error_at(
                    "can() expects exactly one command name",
                    expression.span,
                ));
            }
            SyntaxExprKind::Call { callee, args } => {
                let args = args
                    .iter()
                    .map(|arg| self.resolve_expr(arg, None, scope, aggregate, expanding_can))
                    .collect::<Result<Vec<_>, _>>()?;
                let (expr, ty) = match (callee.text.as_str(), args.as_slice()) {
                    ("old", [value]) => (
                        Expr::UnaryNamed {
                            name: callee.text.clone(),
                            expr: Box::new(value.expr.clone()),
                            span: callee.span,
                        },
                        value.ty.clone(),
                    ),
                    ("abs", [value]) if self.compatible(&value.ty, &LogicalType::Int) => (
                        Expr::UnaryNamed {
                            name: callee.text.clone(),
                            expr: Box::new(value.expr.clone()),
                            span: callee.span,
                        },
                        LogicalType::Int,
                    ),
                    ("min" | "max", [left, right])
                        if self.compatible(&left.ty, &LogicalType::Int)
                            && self.compatible(&right.ty, &LogicalType::Int) =>
                    {
                        (
                            Expr::BinaryNamed {
                                name: callee.text.clone(),
                                left: Box::new(left.expr.clone()),
                                right: Box::new(right.expr.clone()),
                            },
                            LogicalType::Int,
                        )
                    }
                    _ => {
                        return Err(error_at(
                            format!("unsupported domain call '{}'/{}", callee.text, args.len()),
                            expression.span,
                        ));
                    }
                };
                ResolvedExpr { expr, ty }
            }
            SyntaxExprKind::Membership { value, members } => {
                let value = self.resolve_expr(value, None, scope, aggregate, expanding_can)?;
                let comparisons = members
                    .iter()
                    .map(|member| {
                        let member_span = member.span;
                        let member = self.resolve_expr(
                            member,
                            Some(&value.ty),
                            scope,
                            aggregate,
                            expanding_can,
                        )?;
                        if !self.compatible(&value.ty, &member.ty) {
                            return Err(error_at("membership member type mismatch", member_span));
                        }
                        Ok(Expr::Binary {
                            op: "==".to_owned(),
                            left: Box::new(value.expr.clone()),
                            right: Box::new(member.expr),
                        })
                    })
                    .collect::<Result<Vec<_>, CoreError>>()?;
                ResolvedExpr {
                    expr: or_all(comparisons),
                    ty: LogicalType::Bool,
                }
            }
            SyntaxExprKind::Binary { op, left, right } => {
                let left_value = self.resolve_expr(left, None, scope, aggregate, expanding_can);
                let (left, right) = match left_value {
                    Ok(left_value) => {
                        let mut right_scope = scope.clone();
                        if matches!(op.canonical.as_str(), "and" | "=>") {
                            self.collect_pattern_bindings(
                                left,
                                scope,
                                &mut right_scope,
                                aggregate,
                                expanding_can,
                            )?;
                        }
                        let right_value = self.resolve_expr(
                            right,
                            needs_expected_type(right).then_some(&left_value.ty),
                            &right_scope,
                            aggregate,
                            expanding_can,
                        )?;
                        (left_value, right_value)
                    }
                    Err(left_error) => {
                        let Ok(right_value) =
                            self.resolve_expr(right, None, scope, aggregate, expanding_can)
                        else {
                            return Err(left_error);
                        };
                        let left_value = self
                            .resolve_expr(
                                left,
                                Some(&right_value.ty),
                                scope,
                                aggregate,
                                expanding_can,
                            )
                            .map_err(|_| left_error)?;
                        (left_value, right_value)
                    }
                };
                let result_type = match op.canonical.as_str() {
                    "and" | "or" | "=>" => {
                        if !self.compatible(&left.ty, &LogicalType::Bool)
                            || !self.compatible(&right.ty, &LogicalType::Bool)
                        {
                            return Err(error_at(
                                "logical operator requires Bool operands",
                                op.span,
                            ));
                        }
                        LogicalType::Bool
                    }
                    "==" | "!=" => {
                        if !self.compatible(&left.ty, &right.ty) {
                            return Err(error_at("comparison operand type mismatch", op.span));
                        }
                        LogicalType::Bool
                    }
                    "<" | "<=" | ">" | ">=" => {
                        if !self.compatible(&left.ty, &LogicalType::Int)
                            || !self.compatible(&right.ty, &LogicalType::Int)
                        {
                            return Err(error_at(
                                "ordering operator requires numeric operands",
                                op.span,
                            ));
                        }
                        LogicalType::Bool
                    }
                    "+" | "-" | "*" | "/" | "%" => {
                        if !self.compatible(&left.ty, &LogicalType::Int)
                            || !self.compatible(&right.ty, &LogicalType::Int)
                        {
                            return Err(error_at(
                                "arithmetic operator requires numeric operands",
                                op.span,
                            ));
                        }
                        LogicalType::Int
                    }
                    _ => {
                        return Err(error_at(
                            format!("unsupported domain operator '{}'", op.canonical),
                            op.span,
                        ));
                    }
                };
                ResolvedExpr {
                    expr: Expr::Binary {
                        op: op.canonical.clone(),
                        left: Box::new(left.expr),
                        right: Box::new(right.expr),
                    },
                    ty: result_type,
                }
            }
            SyntaxExprKind::Neg(value) => {
                let value = self.resolve_expr(
                    value,
                    Some(&LogicalType::Int),
                    scope,
                    aggregate,
                    expanding_can,
                )?;
                ResolvedExpr {
                    expr: Expr::Neg(Box::new(value.expr)),
                    ty: LogicalType::Int,
                }
            }
            SyntaxExprKind::Not(value) => {
                let value = self.resolve_expr(
                    value,
                    Some(&LogicalType::Bool),
                    scope,
                    aggregate,
                    expanding_can,
                )?;
                ResolvedExpr {
                    expr: Expr::Not(Box::new(value.expr)),
                    ty: LogicalType::Bool,
                }
            }
            SyntaxExprKind::Group(value) => {
                return self.resolve_expr(value, expected, scope, aggregate, expanding_can);
            }
            _ => {
                self.resolve_kernel_shape(expression, expected, scope, aggregate, expanding_can)?
            }
        };
        if let Some(expected) = expected
            && !self.compatible(&resolved.ty, expected)
        {
            return Err(error_at("domain expression type mismatch", expression.span));
        }
        Ok(resolved)
    }

    #[allow(clippy::too_many_lines)]
    fn resolve_kernel_shape(
        &self,
        expression: &SyntaxExpr,
        expected: Option<&LogicalType>,
        scope: &Scope,
        aggregate: Option<&DomainAggregate>,
        expanding_can: &mut Vec<String>,
    ) -> Result<ResolvedExpr, CoreError> {
        Ok(match &expression.kind {
            SyntaxExprKind::Some(value) => {
                let inner_expected = expected.and_then(|expected| match expected {
                    LogicalType::Option(inner) => Some(inner.as_ref()),
                    _ => None,
                });
                let value =
                    self.resolve_expr(value, inner_expected, scope, aggregate, expanding_can)?;
                ResolvedExpr {
                    expr: Expr::Some(Box::new(value.expr)),
                    ty: LogicalType::Option(Box::new(value.ty)),
                }
            }
            SyntaxExprKind::Set(values) => {
                let inner_expected = expected.and_then(|expected| match expected {
                    LogicalType::Set(inner) => Some(inner.as_ref()),
                    _ => None,
                });
                let values = values
                    .iter()
                    .map(|value| {
                        self.resolve_expr(value, inner_expected, scope, aggregate, expanding_can)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let item_type = values.first().map_or_else(
                    || inner_expected.cloned().unwrap_or(LogicalType::Unknown),
                    |value| value.ty.clone(),
                );
                if values
                    .iter()
                    .any(|value| !self.compatible(&item_type, &value.ty))
                {
                    return Err(error_at(
                        "Set literal element type mismatch",
                        expression.span,
                    ));
                }
                ResolvedExpr {
                    expr: Expr::Set(values.into_iter().map(|value| value.expr).collect()),
                    ty: LogicalType::Set(Box::new(item_type)),
                }
            }
            SyntaxExprKind::Seq(values) => {
                let inner_expected = expected.and_then(|expected| match expected {
                    LogicalType::Seq(inner) => Some(inner.as_ref()),
                    _ => None,
                });
                let values = values
                    .iter()
                    .map(|value| {
                        self.resolve_expr(value, inner_expected, scope, aggregate, expanding_can)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let item_type = values.first().map_or_else(
                    || inner_expected.cloned().unwrap_or(LogicalType::Unknown),
                    |value| value.ty.clone(),
                );
                if values
                    .iter()
                    .any(|value| !self.compatible(&item_type, &value.ty))
                {
                    return Err(error_at(
                        "Seq literal element type mismatch",
                        expression.span,
                    ));
                }
                ResolvedExpr {
                    expr: Expr::Seq(values.into_iter().map(|value| value.expr).collect()),
                    ty: LogicalType::Seq(Box::new(item_type)),
                }
            }
            SyntaxExprKind::Struct { name, fields } => {
                let Some(definition) = self
                    .types
                    .iter()
                    .find(|ty| ty.name == name.text && ty.kind == "value_object")
                else {
                    return Err(error_at(
                        format!("unknown domain value object '{}'", name.text),
                        name.span,
                    ));
                };
                let mut lowered = Vec::new();
                let mut seen = BTreeSet::new();
                for (field, value) in fields {
                    if !seen.insert(field.text.clone()) {
                        return Err(error_at(
                            format!("duplicate field '{}.{}'", name.text, field.text),
                            field.span,
                        ));
                    }
                    let Some(field_definition) = definition
                        .fields
                        .iter()
                        .find(|candidate| candidate.name.text == field.text)
                    else {
                        return Err(error_at(
                            format!("unknown field '{}.{}'", name.text, field.text),
                            field.span,
                        ));
                    };
                    let expected = self.logical_type(&field_definition.type_name)?;
                    lowered.push((
                        field.text.clone(),
                        self.resolve_expr(value, Some(&expected), scope, aggregate, expanding_can)?
                            .expr,
                    ));
                }
                if let Some(missing) = definition
                    .fields
                    .iter()
                    .find(|field| !seen.contains(&field.name.text))
                {
                    return Err(error_at(
                        format!("missing struct field '{}.{}'", name.text, missing.name.text),
                        expression.span,
                    ));
                }
                ResolvedExpr {
                    expr: Expr::Struct {
                        name: name.text.clone(),
                        fields: lowered,
                    },
                    ty: LogicalType::Named(name.text.clone()),
                }
            }
            SyntaxExprKind::Index { receiver, index } => {
                let receiver =
                    self.resolve_expr(receiver, None, scope, aggregate, expanding_can)?;
                let (index_type, value_type) = match &receiver.ty {
                    LogicalType::Map(key, value) => (key.as_ref().clone(), value.as_ref().clone()),
                    LogicalType::Seq(value) => (LogicalType::Int, value.as_ref().clone()),
                    _ => {
                        return Err(error_at(
                            "indexing requires a Map or Seq expression",
                            expression.span,
                        ));
                    }
                };
                let index =
                    self.resolve_expr(index, Some(&index_type), scope, aggregate, expanding_can)?;
                ResolvedExpr {
                    expr: Expr::Index(Box::new(receiver.expr), Box::new(index.expr)),
                    ty: value_type,
                }
            }
            SyntaxExprKind::Field { receiver, field } => {
                let receiver =
                    self.resolve_expr(receiver, None, scope, aggregate, expanding_can)?;
                let Some(type_name) = type_name(&receiver.ty) else {
                    return Err(error_at(
                        "field access requires a value object",
                        expression.span,
                    ));
                };
                let Some(definition) = self
                    .types
                    .iter()
                    .find(|ty| ty.name == type_name && ty.kind == "value_object")
                else {
                    return Err(error_at(
                        format!("type '{type_name}' has no fields"),
                        expression.span,
                    ));
                };
                let Some(field_definition) = definition
                    .fields
                    .iter()
                    .find(|candidate| candidate.name.text == field.text)
                else {
                    return Err(error_at(
                        format!("unknown field '{type_name}.{}'", field.text),
                        field.span,
                    ));
                };
                ResolvedExpr {
                    expr: Expr::Field(Box::new(receiver.expr), field.text.clone()),
                    ty: self.logical_type(&field_definition.type_name)?,
                }
            }
            SyntaxExprKind::Method {
                receiver,
                method,
                args,
            } => {
                let receiver =
                    self.resolve_expr(receiver, None, scope, aggregate, expanding_can)?;
                let args = args
                    .iter()
                    .map(|value| self.resolve_expr(value, None, scope, aggregate, expanding_can))
                    .collect::<Result<Vec<_>, _>>()?;
                let (result_type, expected_args): (LogicalType, Vec<LogicalType>) =
                    match (&receiver.ty, method.text.as_str(), args.len()) {
                        (LogicalType::Set(item) | LogicalType::Seq(item), "contains", 1) => {
                            (LogicalType::Bool, vec![item.as_ref().clone()])
                        }
                        (LogicalType::Set(item), "add" | "remove", 1)
                        | (LogicalType::Seq(item), "push", 1) => {
                            (receiver.ty.clone(), vec![item.as_ref().clone()])
                        }
                        (LogicalType::Set(_) | LogicalType::Seq(_), "size", 0) => {
                            (LogicalType::Int, Vec::new())
                        }
                        (LogicalType::Seq(item), "head", 0) => (item.as_ref().clone(), Vec::new()),
                        (LogicalType::Seq(item), "at", 1) => {
                            (item.as_ref().clone(), vec![LogicalType::Int])
                        }
                        (LogicalType::Set(_), name, _) => {
                            return Err(error_at(
                                format!("invalid Set method '{name}'/{}", args.len()),
                                method.span,
                            ));
                        }
                        (LogicalType::Seq(_), name, _) => {
                            return Err(error_at(
                                format!("invalid Seq method '{name}'/{}", args.len()),
                                method.span,
                            ));
                        }
                        _ => {
                            return Err(error_at(
                                "method receiver has no supported collection methods",
                                expression.span,
                            ));
                        }
                    };
                for (argument, expected) in args.iter().zip(&expected_args) {
                    if !self.compatible(&argument.ty, expected) {
                        return Err(error_at(
                            "collection method argument type mismatch",
                            expression.span,
                        ));
                    }
                }
                ResolvedExpr {
                    expr: Expr::Method {
                        receiver: Box::new(receiver.expr),
                        name: method.text.clone(),
                        args: args.into_iter().map(|value| value.expr).collect(),
                    },
                    ty: result_type,
                }
            }
            SyntaxExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let (condition_span, then_span, else_span) =
                    (condition.span, then_expr.span, else_expr.span);
                let condition = self.resolve_expr(
                    condition,
                    Some(&LogicalType::Bool),
                    scope,
                    aggregate,
                    expanding_can,
                )?;
                let then_expr =
                    self.resolve_expr(then_expr, expected, scope, aggregate, expanding_can)?;
                let else_expr = self.resolve_expr(
                    else_expr,
                    Some(&then_expr.ty),
                    scope,
                    aggregate,
                    expanding_can,
                )?;
                ResolvedExpr {
                    expr: Expr::Conditional {
                        spans: Box::new(fsl_syntax::ConditionalSpans {
                            condition: condition_span,
                            then_expr: then_span,
                            else_expr: else_span,
                        }),
                        condition: Box::new(condition.expr),
                        then_expr: Box::new(then_expr.expr),
                        else_expr: Box::new(else_expr.expr),
                    },
                    ty: then_expr.ty,
                }
            }
            SyntaxExprKind::Is { expr, pattern } => {
                let value = self.resolve_expr(expr, None, scope, aggregate, expanding_can)?;
                if !matches!(value.ty, LogicalType::Option(_)) {
                    return Err(error_at(
                        "'is none/some' requires an Option expression",
                        expression.span,
                    ));
                }
                ResolvedExpr {
                    expr: Expr::Is {
                        expr: Box::new(value.expr),
                        pattern: match pattern {
                            SyntaxPattern::None { .. } => Pattern::None,
                            SyntaxPattern::Some { name, .. } => Pattern::Some(name.text.clone()),
                        },
                    },
                    ty: LogicalType::Bool,
                }
            }
            SyntaxExprKind::Quantified {
                quantifier,
                binder,
                body,
                ..
            } => {
                let (binder, nested) =
                    self.resolve_binder(binder, scope, aggregate, expanding_can)?;
                let body = self.resolve_expr(
                    body,
                    Some(&LogicalType::Bool),
                    &nested,
                    aggregate,
                    expanding_can,
                )?;
                ResolvedExpr {
                    expr: Expr::Quantified {
                        quantifier: quantifier.text.clone(),
                        binder,
                        body: Box::new(body.expr),
                    },
                    ty: LogicalType::Bool,
                }
            }
            SyntaxExprKind::Aggregate {
                kind,
                binder,
                value,
            } => {
                let (binder, nested) =
                    self.resolve_binder(binder, scope, aggregate, expanding_can)?;
                let value = value
                    .as_deref()
                    .map(|value| {
                        self.resolve_expr(
                            value,
                            Some(&LogicalType::Int),
                            &nested,
                            aggregate,
                            expanding_can,
                        )
                        .map(|value| Box::new(value.expr))
                    })
                    .transpose()?;
                ResolvedExpr {
                    expr: Expr::Aggregate {
                        kind: *kind,
                        binder,
                        value,
                    },
                    ty: if matches!(kind, AggregateKind::Count | AggregateKind::Sum) {
                        LogicalType::Int
                    } else {
                        LogicalType::Bool
                    },
                }
            }
            SyntaxExprKind::Num(_)
            | SyntaxExprKind::Bool(_)
            | SyntaxExprKind::None
            | SyntaxExprKind::Name(_)
            | SyntaxExprKind::Call { .. }
            | SyntaxExprKind::Binary { .. }
            | SyntaxExprKind::Membership { .. }
            | SyntaxExprKind::Neg(_)
            | SyntaxExprKind::Not(_)
            | SyntaxExprKind::Group(_) => {
                return Err(error_at(
                    "internal domain lowering dispatch error",
                    expression.span,
                ));
            }
        })
    }

    fn qualified_name(name: &SyntaxQualifiedName) -> QualifiedName {
        let (namespace, name) = name.path.legacy_parts();
        QualifiedName { namespace, name }
    }

    fn qualified_logical_type(&self, name: &SyntaxQualifiedName) -> Result<LogicalType, CoreError> {
        if name.has_namespace() {
            return Err(error_at(
                "namespaced domain binder types are not supported",
                name.span(),
            ));
        }
        Ok(match name.name() {
            "Int" => LogicalType::Int,
            "Bool" => LogicalType::Bool,
            other if self.types.iter().any(|ty| ty.name == other) => {
                LogicalType::Named(other.to_owned())
            }
            other => {
                return Err(error_at(
                    format!("unknown domain type '{other}'"),
                    name.span(),
                ));
            }
        })
    }

    #[allow(clippy::too_many_lines)]
    fn resolve_binder(
        &self,
        binder: &SyntaxBinder,
        scope: &Scope,
        aggregate: Option<&DomainAggregate>,
        expanding_can: &mut Vec<String>,
    ) -> Result<(Binder, Scope), CoreError> {
        match binder {
            SyntaxBinder::Typed {
                name,
                type_name,
                where_expr,
                ..
            } => {
                let logical = self.qualified_logical_type(type_name)?;
                let mut nested = scope.clone();
                nested.insert(
                    name.text.clone(),
                    Symbol {
                        kernel_name: name.text.clone(),
                        ty: logical,
                    },
                );
                let where_expr = where_expr
                    .as_deref()
                    .map(|value| {
                        self.resolve_expr(
                            value,
                            Some(&LogicalType::Bool),
                            &nested,
                            aggregate,
                            expanding_can,
                        )
                        .map(|value| Box::new(value.expr))
                    })
                    .transpose()?;
                Ok((
                    Binder::Typed {
                        name: name.text.clone(),
                        type_name: Self::qualified_name(type_name),
                        where_expr,
                    },
                    nested,
                ))
            }
            SyntaxBinder::Range {
                name,
                lo,
                hi,
                where_expr,
                ..
            } => {
                let lo = self.resolve_expr(
                    lo,
                    Some(&LogicalType::Int),
                    scope,
                    aggregate,
                    expanding_can,
                )?;
                let hi = self.resolve_expr(
                    hi,
                    Some(&LogicalType::Int),
                    scope,
                    aggregate,
                    expanding_can,
                )?;
                let mut nested = scope.clone();
                nested.insert(
                    name.text.clone(),
                    Symbol {
                        kernel_name: name.text.clone(),
                        ty: LogicalType::Int,
                    },
                );
                let where_expr = where_expr
                    .as_deref()
                    .map(|value| {
                        self.resolve_expr(
                            value,
                            Some(&LogicalType::Bool),
                            &nested,
                            aggregate,
                            expanding_can,
                        )
                        .map(|value| Box::new(value.expr))
                    })
                    .transpose()?;
                Ok((
                    Binder::Range {
                        name: name.text.clone(),
                        lo: Box::new(lo.expr),
                        hi: Box::new(hi.expr),
                        where_expr,
                    },
                    nested,
                ))
            }
            SyntaxBinder::Collection {
                name,
                collection,
                where_expr,
                ..
            } => {
                let collection =
                    self.resolve_expr(collection, None, scope, aggregate, expanding_can)?;
                let item_type = match &collection.ty {
                    LogicalType::Set(value) | LogicalType::Seq(value) => value.as_ref().clone(),
                    _ => {
                        return Err(error_at(
                            "collection binder requires a Set or Seq expression",
                            name.span,
                        ));
                    }
                };
                let mut nested = scope.clone();
                nested.insert(
                    name.text.clone(),
                    Symbol {
                        kernel_name: name.text.clone(),
                        ty: item_type,
                    },
                );
                let where_expr = where_expr
                    .as_deref()
                    .map(|value| {
                        self.resolve_expr(
                            value,
                            Some(&LogicalType::Bool),
                            &nested,
                            aggregate,
                            expanding_can,
                        )
                        .map(|value| Box::new(value.expr))
                    })
                    .transpose()?;
                Ok((
                    Binder::Collection {
                        name: name.text.clone(),
                        collection: Box::new(collection.expr),
                        where_expr,
                    },
                    nested,
                ))
            }
        }
    }

    fn resolve_can(
        &self,
        decide: &DomainDecide,
        scope: &Scope,
        aggregate: &DomainAggregate,
        expanding_can: &mut Vec<String>,
    ) -> Result<Expr, CoreError> {
        let mut predicates = decide
            .requires
            .iter()
            .map(|value| {
                self.resolve_expr(
                    value,
                    Some(&LogicalType::Bool),
                    scope,
                    Some(aggregate),
                    expanding_can,
                )
                .map(|value| value.expr)
            })
            .collect::<Result<Vec<_>, _>>()?;
        predicates.extend(
            decide
                .rejects
                .iter()
                .map(|reject| {
                    self.resolve_expr(
                        &reject.condition,
                        Some(&LogicalType::Bool),
                        scope,
                        Some(aggregate),
                        expanding_can,
                    )
                    .map(|value| Expr::Not(Box::new(value.expr)))
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        Ok(and_all(predicates))
    }

    fn collect_pattern_bindings(
        &self,
        expression: &SyntaxExpr,
        source_scope: &Scope,
        target_scope: &mut Scope,
        aggregate: Option<&DomainAggregate>,
        expanding_can: &mut Vec<String>,
    ) -> Result<(), CoreError> {
        match &expression.kind {
            SyntaxExprKind::Group(value) => self.collect_pattern_bindings(
                value,
                source_scope,
                target_scope,
                aggregate,
                expanding_can,
            ),
            SyntaxExprKind::Is {
                expr,
                pattern: SyntaxPattern::Some { name, .. },
            } => {
                let value =
                    self.resolve_expr(expr, None, source_scope, aggregate, expanding_can)?;
                let LogicalType::Option(inner) = value.ty else {
                    return Err(error_at(
                        "'is some' requires an Option expression",
                        expression.span,
                    ));
                };
                target_scope.insert(
                    name.text.clone(),
                    Symbol {
                        kernel_name: name.text.clone(),
                        ty: *inner,
                    },
                );
                Ok(())
            }
            SyntaxExprKind::Binary { op, left, right } if op.canonical == "and" => {
                self.collect_pattern_bindings(
                    left,
                    source_scope,
                    target_scope,
                    aggregate,
                    expanding_can,
                )?;
                let after_left = target_scope.clone();
                self.collect_pattern_bindings(
                    right,
                    &after_left,
                    target_scope,
                    aggregate,
                    expanding_can,
                )
            }
            _ => Ok(()),
        }
    }

    fn resolve_lvalue(
        &self,
        target: &SyntaxLValue,
        scope: &Scope,
        aggregate: &DomainAggregate,
    ) -> Result<(LValue, LogicalType), CoreError> {
        match target {
            SyntaxLValue::Name(name) => {
                let Some(field) = aggregate
                    .state
                    .iter()
                    .find(|field| field.name.text == name.text)
                else {
                    return Err(error_at(
                        format!("unknown domain lvalue '{}'", name.text),
                        name.span,
                    ));
                };
                Ok((
                    LValue::Var(state_name(aggregate, &field.name)),
                    self.logical_type(&field.type_name)?,
                ))
            }
            SyntaxLValue::Index { base, index, span } => {
                let (base, base_type) = self.resolve_lvalue(base, scope, aggregate)?;
                let (name, key_type, value_type) = match (base, base_type) {
                    (LValue::Var(name), LogicalType::Map(key, value)) => {
                        (name, key.as_ref().clone(), value.as_ref().clone())
                    }
                    _ => {
                        return Err(error_at(
                            "Kernel lvalues only support indexing a root Map state",
                            *span,
                        ));
                    }
                };
                let index = self.resolve_expr(
                    index,
                    Some(&key_type),
                    scope,
                    Some(aggregate),
                    &mut Vec::new(),
                )?;
                Ok((LValue::Index(name, index.expr), value_type))
            }
            SyntaxLValue::Field { base, field, span } => {
                let (base, base_type) = self.resolve_lvalue(base, scope, aggregate)?;
                let Some(type_name) = type_name(&base_type) else {
                    return Err(error_at("field lvalue requires a value object", *span));
                };
                let Some(definition) = self
                    .types
                    .iter()
                    .find(|ty| ty.name == type_name && ty.kind == "value_object")
                else {
                    return Err(error_at(format!("type '{type_name}' has no fields"), *span));
                };
                let Some(field_definition) = definition
                    .fields
                    .iter()
                    .find(|candidate| candidate.name.text == field.text)
                else {
                    return Err(error_at(
                        format!("unknown field '{type_name}.{}'", field.text),
                        field.span,
                    ));
                };
                Ok((
                    LValue::Field(Box::new(base), field.text.clone()),
                    self.logical_type(&field_definition.type_name)?,
                ))
            }
        }
    }

    fn resolve_bool(
        &self,
        expression: &SyntaxExpr,
        scope: &Scope,
        aggregate: Option<&DomainAggregate>,
    ) -> Result<Expr, CoreError> {
        self.resolve_expr(
            expression,
            Some(&LogicalType::Bool),
            scope,
            aggregate,
            &mut Vec::new(),
        )
        .map(|value| value.expr)
    }

    fn default_value(
        &self,
        field: &DomainField,
        scope: &Scope,
        aggregate: Option<&DomainAggregate>,
    ) -> Result<Expr, CoreError> {
        let expected = self.logical_type(&field.type_name)?;
        if let Some(value) = &field.default {
            return self
                .resolve_expr(value, Some(&expected), scope, aggregate, &mut Vec::new())
                .map(|value| value.expr);
        }
        self.default_for_type(&expected, field.span, scope, aggregate)
    }

    fn default_for_type(
        &self,
        ty: &LogicalType,
        span: Span,
        scope: &Scope,
        aggregate: Option<&DomainAggregate>,
    ) -> Result<Expr, CoreError> {
        Ok(match ty {
            LogicalType::Bool => Expr::Bool(false),
            LogicalType::Int => Expr::Num(0),
            LogicalType::Named(name) => {
                let Some(definition) = self.types.iter().find(|ty| ty.name == *name) else {
                    return Err(error_at(format!("unknown domain type '{name}'"), span));
                };
                match definition.kind.as_str() {
                    "enum" => {
                        let Some(member) = definition.members.first() else {
                            return Err(error_at(format!("enum '{name}' has no members"), span));
                        };
                        Expr::Var(format!("{name}_{member}"))
                    }
                    "range" | "external" => definition.lo.as_ref().map_or_else(
                        || Ok(Expr::Num(0)),
                        |value| {
                            self.resolve_expr(
                                value,
                                Some(&LogicalType::Int),
                                scope,
                                aggregate,
                                &mut Vec::new(),
                            )
                            .map(|value| value.expr)
                        },
                    )?,
                    "value_object" => Expr::Struct {
                        name: name.clone(),
                        fields: definition
                            .fields
                            .iter()
                            .map(|field| {
                                Ok((
                                    field.name.text.clone(),
                                    self.default_value(field, scope, aggregate)?,
                                ))
                            })
                            .collect::<Result<Vec<_>, CoreError>>()?,
                    },
                    other => {
                        return Err(error_at(
                            format!("unsupported domain type kind '{other}'"),
                            span,
                        ));
                    }
                }
            }
            LogicalType::Option(_) => Expr::None,
            LogicalType::Set(_) => Expr::Set(Vec::new()),
            LogicalType::Seq(_) => Expr::Seq(Vec::new()),
            LogicalType::Map(_, _) => {
                return Err(error_at(
                    "Map state requires explicit initialization through supported semantics",
                    span,
                ));
            }
            LogicalType::Unknown => {
                return Err(error_at(
                    "cannot choose a default for an unknown type",
                    span,
                ));
            }
        })
    }

    fn event(
        &self,
        name: &str,
        loc: DomainLoc,
    ) -> Result<(&DomainAggregate, &fsl_syntax::DomainEvent), CoreError> {
        self.domain
            .aggregates
            .iter()
            .find_map(|aggregate| {
                aggregate
                    .events
                    .iter()
                    .find(|event| event.name == name)
                    .map(|event| (aggregate, event))
            })
            .ok_or_else(|| error_at(format!("unknown domain event '{name}'"), span_at(loc)))
    }

    fn correlation(&self, effect: &DomainEffect) -> Result<(String, SyntaxTypeExpr), CoreError> {
        let Some(expression) = &effect.correlation_id else {
            return Err(error_at(
                format!("effect '{}' requires correlation_id", effect.name),
                span_at(effect.loc),
            ));
        };
        let SyntaxExprKind::Field { receiver, field } = &expression.kind else {
            return Err(error_at(
                "effect correlation_id must be Event.field",
                expression.span,
            ));
        };
        let SyntaxExprKind::Name(event_name) = &receiver.kind else {
            return Err(error_at(
                "effect correlation_id must be Event.field",
                expression.span,
            ));
        };
        let (_, event) = self.event(&event_name.text, effect.loc)?;
        let Some(definition) = event
            .fields
            .iter()
            .find(|candidate| candidate.name.text == field.text)
        else {
            return Err(error_at(
                format!("unknown event field '{}.{}'", event_name.text, field.text),
                field.span,
            ));
        };
        Ok((field.text.clone(), definition.type_name.clone()))
    }

    #[allow(clippy::too_many_lines)]
    fn validate_domain_path(&self, expression: &SyntaxExpr) -> Result<(), CoreError> {
        fn collect<'a>(
            expression: &'a SyntaxExpr,
            output: &mut Vec<&'a fsl_syntax::SyntaxIdent>,
        ) -> bool {
            match &expression.kind {
                SyntaxExprKind::Name(name) => {
                    output.push(name);
                    true
                }
                SyntaxExprKind::Field { receiver, field } => {
                    if !collect(receiver, output) {
                        return false;
                    }
                    output.push(field);
                    true
                }
                _ => false,
            }
        }

        let mut path = Vec::new();
        if !collect(expression, &mut path) || path.len() < 2 {
            return Err(error_at(
                "domain effect path must be a dotted declaration path",
                expression.span,
            ));
        }
        let root = path.remove(0);
        let first = path.remove(0);
        let mut ty = if let Some(aggregate) = self
            .domain
            .aggregates
            .iter()
            .find(|aggregate| aggregate.name == root.text)
        {
            if first.text == "id" {
                aggregate
                    .id_type
                    .as_ref()
                    .map(|name| LogicalType::Named(name.clone()))
                    .ok_or_else(|| {
                        error_at(
                            format!("aggregate '{}' has no id", aggregate.name),
                            first.span,
                        )
                    })?
            } else {
                let field = aggregate
                    .state
                    .iter()
                    .find(|field| field.name.text == first.text)
                    .ok_or_else(|| {
                        error_at(
                            format!("unknown aggregate field '{}.{}'", root.text, first.text),
                            first.span,
                        )
                    })?;
                self.logical_type(&field.type_name)?
            }
        } else if let Some((_, event)) = self.domain.aggregates.iter().find_map(|aggregate| {
            aggregate
                .events
                .iter()
                .find(|event| event.name == root.text)
                .map(|event| (aggregate, event))
        }) {
            let field = event
                .fields
                .iter()
                .find(|field| field.name.text == first.text)
                .ok_or_else(|| {
                    error_at(
                        format!("unknown event field '{}.{}'", root.text, first.text),
                        first.span,
                    )
                })?;
            self.logical_type(&field.type_name)?
        } else {
            return Err(error_at(
                format!("unknown domain path root '{}'", root.text),
                root.span,
            ));
        };
        for component in path {
            let Some(name) = type_name(&ty) else {
                return Err(error_at(
                    "path component requires a value object",
                    component.span,
                ));
            };
            let definition = self
                .types
                .iter()
                .find(|candidate| candidate.name == name && candidate.kind == "value_object")
                .ok_or_else(|| error_at(format!("type '{name}' has no fields"), component.span))?;
            let field = definition
                .fields
                .iter()
                .find(|field| field.name.text == component.text)
                .ok_or_else(|| {
                    error_at(
                        format!("unknown field '{name}.{}'", component.text),
                        component.span,
                    )
                })?;
            ty = self.logical_type(&field.type_name)?;
        }
        Ok(())
    }

    fn validate_document_expressions(&self) -> Result<(), CoreError> {
        for ty in &self.types {
            if ty.kind != "enum" {
                continue;
            }
            if ty.members.is_empty() {
                return Err(error_at(
                    format!("enum '{}' has no members", ty.name),
                    ty.span,
                ));
            }
            let mut seen = BTreeSet::new();
            for (index, member) in ty.members.iter().enumerate() {
                if !seen.insert(member) {
                    return Err(error_at(
                        format!("duplicate enum member '{member}' in '{}'", ty.name),
                        ty.member_spans.get(index).copied().unwrap_or(ty.span),
                    ));
                }
            }
        }
        for ty in &self.types {
            if ty.kind != "value_object" {
                continue;
            }
            let scope = self.scope_for_fields(&ty.fields)?;
            for field in &ty.fields {
                if let Some(default) = &field.default {
                    let expected = self.logical_type(&field.type_name)?;
                    let _ =
                        self.resolve_expr(default, Some(&expected), &scope, None, &mut Vec::new())?;
                }
            }
            for invariant in &ty.invariants {
                self.resolve_bool(&invariant.expr, &scope, None)?;
            }
        }
        for aggregate in &self.domain.aggregates {
            let state_scope = self.scope_for_aggregate(aggregate)?;
            for fields in aggregate
                .commands
                .iter()
                .map(|command| command.inputs.as_slice())
                .chain(aggregate.events.iter().map(|event| event.fields.as_slice()))
            {
                let mut local_scope = state_scope.clone();
                self.extend_fields(&mut local_scope, fields)?;
                for field in fields {
                    if let Some(default) = &field.default {
                        let expected = self.logical_type(&field.type_name)?;
                        let _ = self.resolve_expr(
                            default,
                            Some(&expected),
                            &local_scope,
                            Some(aggregate),
                            &mut Vec::new(),
                        )?;
                    }
                }
            }
            for stale in &aggregate.stale_policies {
                self.event(&stale.event, stale.loc)?;
                for emitted in &stale.emits {
                    self.event(emitted, stale.loc)?;
                }
                self.resolve_bool(&stale.condition, &state_scope, Some(aggregate))?;
            }
            for evolve in &aggregate.evolves {
                let (_, event) = self.event(&evolve.event, evolve.loc)?;
                let mut scope = state_scope.clone();
                self.extend_fields(&mut scope, &event.fields)?;
                let _ = self.evolve_items(aggregate, &evolve.event, &scope)?;
            }
        }
        for effect in &self.domain.effects {
            if let Some(idempotency_key) = &effect.idempotency_key {
                self.validate_domain_path(idempotency_key)?;
            }
            let _ = self.correlation(effect)?;
        }
        Ok(())
    }

    fn event_assignments(
        &self,
        emitted: &[String],
        span: Span,
    ) -> Result<Vec<Statement>, CoreError> {
        for event in emitted {
            self.event(event, DomainLoc::from(span))?;
        }
        let emitted = emitted.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let mut names = self
            .domain
            .aggregates
            .iter()
            .flat_map(|aggregate| aggregate.events.iter().map(|event| event.name.as_str()))
            .collect::<Vec<_>>();
        names.sort_unstable();
        Ok(names
            .into_iter()
            .map(|name| Statement::Assign {
                target: LValue::Var(event_flag(name)),
                value: Expr::Bool(emitted.contains(name)),
                span,
            })
            .collect())
    }

    fn evolve_items(
        &self,
        aggregate: &DomainAggregate,
        event_name: &str,
        scope: &Scope,
    ) -> Result<Vec<ActionItem>, CoreError> {
        let Some(evolve) = aggregate
            .evolves
            .iter()
            .find(|evolve| evolve.event == event_name)
        else {
            return Ok(Vec::new());
        };
        let mut items = evolve
            .requires
            .iter()
            .map(|requirement| {
                Ok(ActionItem::Requires(
                    self.resolve_bool(requirement, scope, Some(aggregate))?,
                    requirement.span,
                ))
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        for assignment in &evolve.assignments {
            let (target, expected) = self.resolve_lvalue(&assignment.target, scope, aggregate)?;
            let value = self
                .resolve_expr(
                    &assignment.value,
                    Some(&expected),
                    scope,
                    Some(aggregate),
                    &mut Vec::new(),
                )?
                .expr;
            items.push(ActionItem::Statement(Statement::Assign {
                target,
                value,
                span: assignment.span,
            }));
        }
        Ok(items)
    }
}

fn metadata(id: impl Into<String>, text: impl Into<String>) -> MetaTag {
    MetaTag {
        id: id.into(),
        text: Some(text.into()),
        span: None,
    }
}

fn action(
    name: String,
    params: Vec<Param>,
    items: Vec<ActionItem>,
    span: Span,
    annotations: Annotations,
) -> SpecItem {
    SpecItem::Action {
        name,
        params,
        items,
        span,
        fair: false,
        meta: None,
        sync: false,
        annotations,
    }
}

fn evolve_annotations(aggregate: &DomainAggregate, event: &str) -> Annotations {
    aggregate
        .evolves
        .iter()
        .find(|evolve| evolve.event == event)
        .map(|evolve| evolve.annotations.clone())
        .unwrap_or_default()
}

fn field_params(resolver: &Resolver<'_>, fields: &[DomainField]) -> Result<Vec<Param>, CoreError> {
    fields
        .iter()
        .map(|field| match resolver.logical_type(&field.type_name)? {
            LogicalType::Named(name) => Ok(Param::Typed(field.name.text.clone(), qualified(&name))),
            LogicalType::Int => Ok(Param::Typed(field.name.text.clone(), qualified("Int"))),
            LogicalType::Bool => Ok(Param::Typed(field.name.text.clone(), qualified("Bool"))),
            _ => Err(error_at(
                "domain action parameters require scalar or named types",
                field.span,
            )),
        })
        .collect()
}

/// Resolve a parsed domain document and lower it directly into Kernel surface AST.
#[allow(clippy::too_many_lines)]
pub(crate) fn lower_domain_surface(
    domain: &DomainSpec,
) -> Result<(SurfaceSpec, OriginRegistry), CoreError> {
    validate_effect_outcome_roles(domain)?;
    let resolver = Resolver::new(domain);
    resolver.validate_document_expressions()?;
    let mut items = Vec::new();

    for ty in &resolver.types {
        match ty.kind.as_str() {
            "enum" => items.push(SpecItem::Enum {
                name: ty.name.clone(),
                members: ty
                    .members
                    .iter()
                    .map(|member| format!("{}_{}", ty.name, member))
                    .collect(),
                symmetric: false,
            }),
            "range" | "external" => {
                let scope = Scope::new();
                let lo = ty.lo.as_ref().map_or_else(
                    || Ok(Expr::Num(0)),
                    |value| {
                        resolver
                            .resolve_expr(
                                value,
                                Some(&LogicalType::Int),
                                &scope,
                                None,
                                &mut Vec::new(),
                            )
                            .map(|value| value.expr)
                    },
                )?;
                let hi = ty.hi.as_ref().map_or_else(
                    || Ok(Expr::Num(1)),
                    |value| {
                        resolver
                            .resolve_expr(
                                value,
                                Some(&LogicalType::Int),
                                &scope,
                                None,
                                &mut Vec::new(),
                            )
                            .map(|value| value.expr)
                    },
                )?;
                items.push(SpecItem::Type {
                    name: ty.name.clone(),
                    lo: Box::new(lo),
                    hi: Box::new(hi),
                    symmetric: false,
                });
            }
            "value_object" => items.push(SpecItem::Struct {
                name: ty.name.clone(),
                fields: ty
                    .fields
                    .iter()
                    .map(|field| {
                        Ok((
                            field.name.text.clone(),
                            resolver.surface_type(&field.type_name)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, CoreError>>()?,
            }),
            other => {
                return Err(error_at(
                    format!("unsupported domain type kind '{other}'"),
                    span_at(ty.loc),
                ));
            }
        }
    }

    for effect in &domain.effects {
        items.push(SpecItem::Enum {
            name: status_type(effect),
            members: [
                "NotStarted",
                "Pending",
                "Succeeded",
                "Failed",
                "TimedOut",
                "Cancelled",
                "Compensated",
            ]
            .iter()
            .map(|member| status_member(effect, member))
            .collect(),
            symmetric: false,
        });
        items.push(SpecItem::Type {
            name: attempt_type(effect),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Num(effect.retry.max_attempts.unwrap_or(1))),
            symmetric: false,
        });
    }

    let mut state = Vec::new();
    let mut init = Vec::new();
    for aggregate in &domain.aggregates {
        let scope = resolver.scope_for_aggregate(aggregate)?;
        for field in &aggregate.state {
            let logical_type = resolver.logical_type(&field.type_name)?;
            state.push(StateField::generated(
                state_name(aggregate, &field.name),
                resolver.surface_type(&field.type_name)?,
                field.span,
            ));
            if let LogicalType::Map(key, value) = &logical_type {
                if field.default.is_some() {
                    return Err(error_at(
                        "whole-Map domain defaults are not supported",
                        field.span,
                    ));
                }
                init.push(Statement::ForAll {
                    binder: Binder::Typed {
                        name: "k".to_owned(),
                        type_name: logical_qualified_name(key, field.span)?,
                        where_expr: None,
                    },
                    statements: vec![Statement::Assign {
                        target: LValue::Index(
                            state_name(aggregate, &field.name),
                            Expr::Var("k".to_owned()),
                        ),
                        value: resolver.default_for_type(
                            value,
                            field.span,
                            &scope,
                            Some(aggregate),
                        )?,
                        span: field.span,
                    }],
                    span: field.span,
                });
            } else {
                init.push(Statement::Assign {
                    target: LValue::Var(state_name(aggregate, &field.name)),
                    value: resolver.default_value(field, &scope, Some(aggregate))?,
                    span: field.span,
                });
            }
        }
    }
    let mut event_names = domain
        .aggregates
        .iter()
        .flat_map(|aggregate| aggregate.events.iter().map(|event| event.name.as_str()))
        .collect::<Vec<_>>();
    event_names.sort_unstable();
    for event in event_names {
        state.push(StateField::generated(
            event_flag(event),
            TypeExpr::Bool,
            span_at(domain.loc),
        ));
        init.push(Statement::Assign {
            target: LValue::Var(event_flag(event)),
            value: Expr::Bool(false),
            span: span_at(domain.loc),
        });
    }
    for effect in &domain.effects {
        let (_, correlation_type) = resolver.correlation(effect)?;
        let key_type = resolver.surface_type(&correlation_type)?;
        state.push(StateField::generated(
            status_var(effect),
            TypeExpr::Map(
                Box::new(key_type.clone()),
                Box::new(TypeExpr::Name(status_type(effect))),
            ),
            span_at(effect.loc),
        ));
        state.push(StateField::generated(
            attempt_var(effect),
            TypeExpr::Map(
                Box::new(key_type),
                Box::new(TypeExpr::Name(attempt_type(effect))),
            ),
            span_at(effect.loc),
        ));
        let span = span_at(effect.loc);
        init.push(Statement::ForAll {
            binder: Binder::Typed {
                name: "k".to_owned(),
                type_name: qualified(&correlation_type.render_source()),
                where_expr: None,
            },
            statements: vec![Statement::Assign {
                target: LValue::Index(status_var(effect), Expr::Var("k".to_owned())),
                value: Expr::Var(status_member(effect, "NotStarted")),
                span,
            }],
            span,
        });
        init.push(Statement::ForAll {
            binder: Binder::Typed {
                name: "k".to_owned(),
                type_name: qualified(&correlation_type.render_source()),
                where_expr: None,
            },
            statements: vec![Statement::Assign {
                target: LValue::Index(attempt_var(effect), Expr::Var("k".to_owned())),
                value: Expr::Num(0),
                span,
            }],
            span,
        });
    }
    items.push(SpecItem::State(state));
    items.push(SpecItem::Init {
        statements: init,
        meta: None,
        annotations: Annotations::default(),
    });

    lower_aggregate_actions(&resolver, domain, &mut items)?;
    lower_effect_actions(&resolver, domain, &mut items)?;
    lower_saga_actions(&resolver, domain, &mut items)?;
    lower_properties(&resolver, domain, &mut items)?;
    items.push(SpecItem::Terminal {
        expr: Box::new(Expr::Bool(false)),
        span: span_at(domain.loc),
    });

    let surface = SurfaceSpec {
        name: domain.name.clone(),
        meta: Some(metadata(
            "DOMAIN",
            "domain: generated from fsl-domain/fsl-effect",
        )),
        items,
    };
    let origins = domain_origin_registry(domain, &surface);
    Ok((surface, origins))
}

#[allow(clippy::too_many_lines)]
fn lower_aggregate_actions(
    resolver: &Resolver<'_>,
    domain: &DomainSpec,
    items: &mut Vec<SpecItem>,
) -> Result<(), CoreError> {
    let effects_by_request = domain
        .effects
        .iter()
        .filter_map(|effect| {
            effect
                .handles
                .as_deref()
                .or(effect.request_event.as_deref())
                .map(|event| (event, effect))
        })
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
                .find(|command| command.name == decide.command)
            else {
                return Err(error_at(
                    format!(
                        "decide references unknown command '{}.{}'",
                        aggregate.name, decide.command
                    ),
                    span_at(decide.loc),
                ));
            };
            let mut scope = resolver.scope_for_aggregate(aggregate)?;
            resolver.extend_fields(&mut scope, &command.inputs)?;
            let span = span_at(decide.loc);
            let mut action_items = decide
                .requires
                .iter()
                .map(|requirement| {
                    Ok(ActionItem::Requires(
                        resolver.resolve_bool(requirement, &scope, Some(aggregate))?,
                        requirement.span,
                    ))
                })
                .collect::<Result<Vec<_>, CoreError>>()?;
            action_items.extend(
                decide
                    .rejects
                    .iter()
                    .map(|reject| {
                        Ok(ActionItem::Requires(
                            Expr::Not(Box::new(resolver.resolve_bool(
                                &reject.condition,
                                &scope,
                                Some(aggregate),
                            )?)),
                            reject.condition.span,
                        ))
                    })
                    .collect::<Result<Vec<_>, CoreError>>()?,
            );
            for event in &decide.emits {
                resolver.event(event, decide.loc)?;
                for effect in effects_by_request.get(event.as_str()).into_iter().flatten() {
                    let (correlation, _) = resolver.correlation(effect)?;
                    let Some(symbol) = scope.get(&correlation) else {
                        return Err(error_at(
                            format!(
                                "effect '{}' correlation '{}' is not in command '{}' scope",
                                effect.name, correlation, command.name
                            ),
                            span,
                        ));
                    };
                    let current = Expr::Index(
                        Box::new(Expr::Var(status_var(effect))),
                        Box::new(Expr::Var(symbol.kernel_name.clone())),
                    );
                    for member in ["Pending", "Succeeded"] {
                        action_items.push(ActionItem::Requires(
                            Expr::Binary {
                                op: "!=".to_owned(),
                                left: Box::new(current.clone()),
                                right: Box::new(Expr::Var(status_member(effect, member))),
                            },
                            span,
                        ));
                    }
                }
            }
            action_items.extend(
                resolver
                    .event_assignments(&decide.emits, span)?
                    .into_iter()
                    .map(ActionItem::Statement),
            );
            for event in &decide.emits {
                action_items.extend(resolver.evolve_items(aggregate, event, &scope)?);
                for effect in effects_by_request.get(event.as_str()).into_iter().flatten() {
                    let (correlation, _) = resolver.correlation(effect)?;
                    let symbol = scope
                        .get(&correlation)
                        .expect("validated correlation symbol");
                    let index = Expr::Var(symbol.kernel_name.clone());
                    action_items.push(ActionItem::Statement(Statement::Assign {
                        target: LValue::Index(status_var(effect), index.clone()),
                        value: Expr::Var(status_member(effect, "Pending")),
                        span,
                    }));
                    action_items.push(ActionItem::Statement(Statement::Assign {
                        target: LValue::Index(attempt_var(effect), index),
                        value: Expr::Num(1),
                        span,
                    }));
                }
            }
            let mut annotations = command.annotations.clone();
            annotations.extend(decide.annotations.source_order().iter().cloned());
            for event in &decide.emits {
                annotations.extend(
                    evolve_annotations(aggregate, event)
                        .source_order()
                        .iter()
                        .cloned(),
                );
            }
            items.push(action(
                format!(
                    "{}_{}",
                    lower_name(&aggregate.name),
                    lower_name(&command.name)
                ),
                field_params(resolver, &command.inputs)?,
                action_items,
                span,
                annotations,
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn lower_effect_actions(
    resolver: &Resolver<'_>,
    domain: &DomainSpec,
    items: &mut Vec<SpecItem>,
) -> Result<(), CoreError> {
    for effect in &domain.effects {
        let (correlation, correlation_type) = resolver.correlation(effect)?;
        for event_name in effect.outcome_events() {
            let (aggregate, event) = resolver.event(event_name, effect.loc)?;
            let span = span_at(event.loc);
            let mut scope = resolver.scope_for_aggregate(aggregate)?;
            resolver.extend_fields(&mut scope, &event.fields)?;
            if !scope.contains_key(&correlation) {
                scope.insert(
                    correlation.clone(),
                    Symbol {
                        kernel_name: correlation.clone(),
                        ty: resolver.logical_type(&correlation_type)?,
                    },
                );
            }
            let mut params = event.fields.clone();
            if !params.iter().any(|field| field.name.text == correlation) {
                params.insert(
                    0,
                    DomainField {
                        name: SyntaxIdent {
                            text: correlation.clone(),
                            span,
                        },
                        type_name: correlation_type.clone(),
                        default: None,
                        span,
                        loc: event.loc,
                    },
                );
            }
            let current = Expr::Index(
                Box::new(Expr::Var(status_var(effect))),
                Box::new(Expr::Var(correlation.clone())),
            );
            let mut action_items = vec![ActionItem::Requires(
                Expr::Binary {
                    op: "==".to_owned(),
                    left: Box::new(current),
                    right: Box::new(Expr::Var(status_member(effect, "Pending"))),
                },
                span,
            )];
            action_items.extend(
                resolver
                    .event_assignments(std::slice::from_ref(event_name), span)?
                    .into_iter()
                    .map(ActionItem::Statement),
            );
            action_items.push(ActionItem::Statement(Statement::Assign {
                target: LValue::Index(status_var(effect), Expr::Var(correlation.clone())),
                value: Expr::Var(status_member(
                    effect,
                    effect_outcome_member(effect, event_name),
                )),
                span,
            }));
            action_items.extend(resolver.evolve_items(aggregate, event_name, &scope)?);
            let mut annotations = effect.annotations.clone();
            annotations.extend(
                evolve_annotations(aggregate, event_name)
                    .source_order()
                    .iter()
                    .cloned(),
            );
            items.push(action(
                format!(
                    "{}_complete_{}",
                    lower_name(&effect.name),
                    lower_name(event_name)
                ),
                field_params(resolver, &params)?,
                action_items,
                span,
                annotations,
            ));
        }
        if let Some(maximum) = effect.retry.max_attempts {
            let span = span_at(effect.loc);
            let current_status = Expr::Index(
                Box::new(Expr::Var(status_var(effect))),
                Box::new(Expr::Var(correlation.clone())),
            );
            let status_guard = or_all(
                ["Failed", "TimedOut"]
                    .iter()
                    .map(|member| Expr::Binary {
                        op: "==".to_owned(),
                        left: Box::new(current_status.clone()),
                        right: Box::new(Expr::Var(status_member(effect, member))),
                    })
                    .collect(),
            );
            let current_attempts = Expr::Index(
                Box::new(Expr::Var(attempt_var(effect))),
                Box::new(Expr::Var(correlation.clone())),
            );
            let mut action_items = vec![
                ActionItem::Requires(status_guard, span),
                ActionItem::Requires(
                    Expr::Binary {
                        op: "<".to_owned(),
                        left: Box::new(current_attempts.clone()),
                        right: Box::new(Expr::Num(maximum)),
                    },
                    span,
                ),
            ];
            action_items.extend(
                resolver
                    .event_assignments(&[], span)?
                    .into_iter()
                    .map(ActionItem::Statement),
            );
            action_items.push(ActionItem::Statement(Statement::Assign {
                target: LValue::Index(status_var(effect), Expr::Var(correlation.clone())),
                value: Expr::Var(status_member(effect, "Pending")),
                span,
            }));
            action_items.push(ActionItem::Statement(Statement::Assign {
                target: LValue::Index(attempt_var(effect), Expr::Var(correlation.clone())),
                value: Expr::Binary {
                    op: "+".to_owned(),
                    left: Box::new(current_attempts),
                    right: Box::new(Expr::Num(1)),
                },
                span,
            }));
            items.push(action(
                format!("{}_retry", lower_name(&effect.name)),
                vec![Param::Typed(
                    correlation.clone(),
                    qualified(&correlation_type.render_source()),
                )],
                action_items,
                span,
                effect.annotations.clone(),
            ));
        }
    }
    Ok(())
}

fn saga_scope(resolver: &Resolver<'_>) -> Scope {
    resolver
        .domain
        .aggregates
        .iter()
        .flat_map(|aggregate| aggregate.events.iter())
        .map(|event| {
            (
                event.name.clone(),
                Symbol {
                    kernel_name: event_flag(&event.name),
                    ty: LogicalType::Bool,
                },
            )
        })
        .collect()
}

fn saga_guards(
    resolver: &Resolver<'_>,
    saga: &DomainSaga,
    step: &DomainSagaStep,
    first: bool,
) -> Result<Vec<Expr>, CoreError> {
    let scope = saga_scope(resolver);
    let mut guards = Vec::new();
    if first && let Some(event) = &saga.starts_on {
        resolver.event(event, saga.loc)?;
        guards.push(Expr::Var(event_flag(event)));
    }
    guards.extend(
        step.requires
            .iter()
            .map(|value| resolver.resolve_bool(value, &scope, None))
            .collect::<Result<Vec<_>, _>>()?,
    );
    if step.emits.is_empty() && !step.awaits.is_empty() {
        for event in &step.awaits {
            resolver.event(event, step.loc)?;
        }
        let events = step
            .awaits
            .iter()
            .map(|event| Expr::Var(event_flag(event)))
            .collect::<Vec<_>>();
        guards.push(if step.awaits_mode == "all" {
            and_all(events)
        } else {
            or_all(events)
        });
    }
    Ok(guards)
}

#[allow(clippy::too_many_lines)]
fn lower_saga_actions(
    resolver: &Resolver<'_>,
    domain: &DomainSpec,
    items: &mut Vec<SpecItem>,
) -> Result<(), CoreError> {
    for saga in &domain.sagas {
        let mut observed = BTreeSet::new();
        for step in &saga.steps {
            observed.extend(step.awaits.iter().cloned());
        }
        for compensation in &saga.compensations {
            observed.insert(compensation.trigger_event.clone());
            observed.insert(compensation.after_event.clone());
        }
        for event_name in observed {
            let (aggregate, event) = resolver.event(&event_name, saga.loc)?;
            let span = span_at(event.loc);
            let mut scope = resolver.scope_for_aggregate(aggregate)?;
            resolver.extend_fields(&mut scope, &event.fields)?;
            let mut action_items = resolver
                .event_assignments(std::slice::from_ref(&event_name), span)?
                .into_iter()
                .map(ActionItem::Statement)
                .collect::<Vec<_>>();
            action_items.extend(resolver.evolve_items(aggregate, &event_name, &scope)?);
            let annotations = evolve_annotations(aggregate, &event_name);
            items.push(action(
                format!(
                    "saga_{}_observe_{}",
                    lower_name(&saga.name),
                    lower_name(&event_name)
                ),
                field_params(resolver, &event.fields)?,
                action_items,
                span,
                annotations,
            ));
        }
        for (index, step) in saga.steps.iter().enumerate() {
            let span = span_at(step.loc);
            let guards = saga_guards(resolver, saga, step, index == 0)?;
            let mut action_items = guards
                .iter()
                .cloned()
                .map(|guard| ActionItem::Requires(guard, span))
                .collect::<Vec<_>>();
            action_items.extend(
                resolver
                    .event_assignments(&step.emits, span)?
                    .into_iter()
                    .map(ActionItem::Statement),
            );
            let action_name = format!("saga_{}_{}", lower_name(&saga.name), lower_name(&step.name));
            items.push(action(
                action_name.clone(),
                Vec::new(),
                action_items,
                span,
                step.annotations.clone(),
            ));
            if let Some(timeout) = &step.timeout_event {
                let mut timeout_items = guards
                    .into_iter()
                    .map(|guard| ActionItem::Requires(guard, span))
                    .collect::<Vec<_>>();
                timeout_items.extend(
                    resolver
                        .event_assignments(std::slice::from_ref(timeout), span)?
                        .into_iter()
                        .map(ActionItem::Statement),
                );
                items.push(action(
                    format!("{action_name}_timeout"),
                    Vec::new(),
                    timeout_items,
                    span,
                    step.annotations.clone(),
                ));
            }
        }
        for compensation in &saga.compensations {
            resolver.event(&compensation.trigger_event, compensation.loc)?;
            resolver.event(&compensation.after_event, compensation.loc)?;
            let span = span_at(compensation.loc);
            let mut action_items = vec![ActionItem::Requires(
                Expr::Var(event_flag(&compensation.trigger_event)),
                span,
            )];
            action_items.extend(
                resolver
                    .event_assignments(&compensation.emits, span)?
                    .into_iter()
                    .map(ActionItem::Statement),
            );
            items.push(action(
                format!(
                    "saga_{}_compensate_{}_after_{}",
                    lower_name(&saga.name),
                    lower_name(&compensation.trigger_event),
                    lower_name(&compensation.after_event)
                ),
                Vec::new(),
                action_items,
                span,
                Annotations::default(),
            ));
        }
    }
    Ok(())
}

fn lower_properties(
    resolver: &Resolver<'_>,
    domain: &DomainSpec,
    items: &mut Vec<SpecItem>,
) -> Result<(), CoreError> {
    for aggregate in &domain.aggregates {
        let scope = resolver.scope_for_aggregate(aggregate)?;
        for invariant in &aggregate.invariants {
            items.push(SpecItem::Invariant {
                name: format!("{}_{}", safe(&aggregate.name), safe(&invariant.name)),
                expr: Box::new(resolver.resolve_bool(&invariant.expr, &scope, Some(aggregate))?),
                span: invariant.span,
                meta: Some(metadata(
                    "DOMAIN-INVARIANT",
                    format!("{}.{}", aggregate.name, invariant.name),
                )),
                annotations: invariant.annotations.clone(),
            });
        }
    }
    let scope = saga_scope(resolver);
    for saga in &domain.sagas {
        for invariant in &saga.invariants {
            items.push(SpecItem::Invariant {
                name: format!("{}_{}", safe(&saga.name), safe(&invariant.name)),
                expr: Box::new(resolver.resolve_bool(&invariant.expr, &scope, None)?),
                span: invariant.span,
                meta: Some(metadata(
                    "DOMAIN-SAGA",
                    format!("{}.{}", saga.name, invariant.name),
                )),
                annotations: invariant.annotations.clone(),
            });
        }
    }
    for effect in &domain.effects {
        let (_, correlation_type) = resolver.correlation(effect)?;
        let span = span_at(effect.loc);
        let status = Expr::Index(
            Box::new(Expr::Var(status_var(effect))),
            Box::new(Expr::Var("k".to_owned())),
        );
        let succeeded = Expr::Var(status_member(effect, "Succeeded"));
        items.push(SpecItem::Trans {
            name: format!("{}_SuccessSticky", safe(&effect.name)),
            expr: Box::new(Expr::Quantified {
                quantifier: "forall".to_owned(),
                binder: Binder::Typed {
                    name: "k".to_owned(),
                    type_name: qualified(&correlation_type.render_source()),
                    where_expr: None,
                },
                body: Box::new(Expr::Binary {
                    op: "=>".to_owned(),
                    left: Box::new(Expr::Binary {
                        op: "==".to_owned(),
                        left: Box::new(Expr::UnaryNamed {
                            name: "old".to_owned(),
                            expr: Box::new(status.clone()),
                            span,
                        }),
                        right: Box::new(succeeded.clone()),
                    }),
                    right: Box::new(Expr::Binary {
                        op: "==".to_owned(),
                        left: Box::new(status),
                        right: Box::new(succeeded),
                    }),
                }),
            }),
            span,
            meta: Some(metadata(
                "DOMAIN-EFFECT",
                format!("{} success is sticky", effect.name),
            )),
            annotations: effect.annotations.clone(),
        });
    }
    Ok(())
}

#[derive(Clone)]
struct DomainSourceRecord {
    span: Span,
    path: Vec<String>,
    steps: Vec<LoweringStep>,
    secondary: Vec<OriginSite>,
}

fn source_site(path: Vec<String>, span: Span) -> OriginSite {
    OriginSite {
        source_file: None,
        span: Some(span),
        dialect: "domain".to_owned(),
        declaration_path: path,
    }
}

fn expression_lowering_steps(expression: &SyntaxExpr) -> Vec<LoweringStep> {
    fn visit(expression: &SyntaxExpr, output: &mut Vec<LoweringStep>) {
        match &expression.kind {
            SyntaxExprKind::Call { callee, args } => {
                if callee.text == "can" {
                    output.push(LoweringStep {
                        kind: "expand_can".to_owned(),
                        detail: Some("command preconditions and rejections".to_owned()),
                    });
                }
                for argument in args {
                    visit(argument, output);
                }
            }
            SyntaxExprKind::Membership { value, members } => {
                output.push(LoweringStep {
                    kind: "expand_membership".to_owned(),
                    detail: Some(format!("{} equality predicate(s)", members.len())),
                });
                visit(value, output);
                for member in members {
                    visit(member, output);
                }
            }
            SyntaxExprKind::Binary { op, left, right } => {
                if op.spelling != op.canonical {
                    output.push(LoweringStep {
                        kind: "normalize_legacy_operator".to_owned(),
                        detail: Some(format!("{} -> {}", op.spelling, op.canonical)),
                    });
                }
                visit(left, output);
                visit(right, output);
            }
            SyntaxExprKind::Some(value)
            | SyntaxExprKind::Neg(value)
            | SyntaxExprKind::Not(value)
            | SyntaxExprKind::Group(value) => visit(value, output),
            SyntaxExprKind::Set(values) | SyntaxExprKind::Seq(values) => {
                for value in values {
                    visit(value, output);
                }
            }
            SyntaxExprKind::Struct { fields, .. } => {
                for (_, value) in fields {
                    visit(value, output);
                }
            }
            SyntaxExprKind::Index { receiver, index } => {
                visit(receiver, output);
                visit(index, output);
            }
            SyntaxExprKind::Field { receiver, .. } => visit(receiver, output),
            SyntaxExprKind::Method { receiver, args, .. } => {
                visit(receiver, output);
                for argument in args {
                    visit(argument, output);
                }
            }
            SyntaxExprKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                visit(condition, output);
                visit(then_expr, output);
                visit(else_expr, output);
            }
            SyntaxExprKind::Is { expr, .. } => visit(expr, output),
            SyntaxExprKind::Quantified { body, .. } => visit(body, output),
            SyntaxExprKind::Aggregate { value, .. } => {
                if let Some(value) = value {
                    visit(value, output);
                }
            }
            SyntaxExprKind::Num(_)
            | SyntaxExprKind::Bool(_)
            | SyntaxExprKind::None
            | SyntaxExprKind::Name(_) => {}
        }
    }

    let mut output = vec![LoweringStep {
        kind: "resolve_domain_expression".to_owned(),
        detail: None,
    }];
    visit(expression, &mut output);
    output
}

#[allow(clippy::too_many_lines)]
fn domain_source_records(domain: &DomainSpec) -> Vec<DomainSourceRecord> {
    let mut records = vec![DomainSourceRecord {
        span: span_at(domain.loc),
        path: vec![domain.name.clone()],
        steps: Vec::new(),
        secondary: Vec::new(),
    }];
    for ty in &domain.types {
        records.push(DomainSourceRecord {
            span: span_at(ty.loc),
            path: vec![domain.name.clone(), "type".to_owned(), ty.name.clone()],
            steps: Vec::new(),
            secondary: Vec::new(),
        });
    }
    for aggregate in &domain.aggregates {
        let aggregate_path = vec![
            domain.name.clone(),
            "aggregate".to_owned(),
            aggregate.name.clone(),
        ];
        records.push(DomainSourceRecord {
            span: span_at(aggregate.loc),
            path: aggregate_path.clone(),
            steps: Vec::new(),
            secondary: Vec::new(),
        });
        for field in &aggregate.state {
            records.push(DomainSourceRecord {
                span: field.span,
                path: [
                    aggregate_path.clone(),
                    vec!["state".to_owned(), field.name.text.clone()],
                ]
                .concat(),
                steps: Vec::new(),
                secondary: Vec::new(),
            });
        }
        for decide in &aggregate.decides {
            let command = aggregate
                .commands
                .iter()
                .find(|command| command.name == decide.command);
            records.push(DomainSourceRecord {
                span: span_at(decide.loc),
                path: [
                    aggregate_path.clone(),
                    vec!["decide".to_owned(), decide.command.clone()],
                ]
                .concat(),
                steps: vec![LoweringStep {
                    kind: "lower_decision_to_action".to_owned(),
                    detail: None,
                }],
                secondary: command
                    .into_iter()
                    .map(|command| {
                        source_site(
                            [
                                aggregate_path.clone(),
                                vec!["command".to_owned(), command.name.clone()],
                            ]
                            .concat(),
                            span_at(command.loc),
                        )
                    })
                    .collect(),
            });
            for (index, requirement) in decide.requires.iter().enumerate() {
                records.push(DomainSourceRecord {
                    span: requirement.span,
                    path: [
                        aggregate_path.clone(),
                        vec![
                            "decide".to_owned(),
                            decide.command.clone(),
                            "requires".to_owned(),
                            index.to_string(),
                        ],
                    ]
                    .concat(),
                    steps: expression_lowering_steps(requirement),
                    secondary: Vec::new(),
                });
            }
            for reject in &decide.rejects {
                records.push(DomainSourceRecord {
                    span: reject.condition.span,
                    path: [
                        aggregate_path.clone(),
                        vec![
                            "decide".to_owned(),
                            decide.command.clone(),
                            "reject".to_owned(),
                            reject.error.clone(),
                        ],
                    ]
                    .concat(),
                    steps: expression_lowering_steps(&reject.condition),
                    secondary: Vec::new(),
                });
            }
        }
        for evolve in &aggregate.evolves {
            for (index, requirement) in evolve.requires.iter().enumerate() {
                records.push(DomainSourceRecord {
                    span: requirement.span,
                    path: [
                        aggregate_path.clone(),
                        vec![
                            "evolve".to_owned(),
                            evolve.event.clone(),
                            "requires".to_owned(),
                            index.to_string(),
                        ],
                    ]
                    .concat(),
                    steps: expression_lowering_steps(requirement),
                    secondary: Vec::new(),
                });
            }
            for assignment in &evolve.assignments {
                records.push(DomainSourceRecord {
                    span: assignment.span,
                    path: [
                        aggregate_path.clone(),
                        vec![
                            "evolve".to_owned(),
                            evolve.event.clone(),
                            assignment.target.render_source(),
                        ],
                    ]
                    .concat(),
                    steps: expression_lowering_steps(&assignment.value),
                    secondary: Vec::new(),
                });
            }
        }
        for invariant in &aggregate.invariants {
            let invariant_path = [
                aggregate_path.clone(),
                vec!["invariant".to_owned(), invariant.name.text.clone()],
            ]
            .concat();
            records.push(DomainSourceRecord {
                span: invariant.span,
                path: invariant_path.clone(),
                steps: Vec::new(),
                secondary: Vec::new(),
            });
            records.push(DomainSourceRecord {
                span: invariant.expr.span,
                path: invariant_path.clone(),
                steps: expression_lowering_steps(&invariant.expr),
                secondary: vec![source_site(invariant_path, invariant.span)],
            });
        }
    }
    for saga in &domain.sagas {
        for invariant in &saga.invariants {
            let invariant_path = vec![
                domain.name.clone(),
                "saga".to_owned(),
                saga.name.clone(),
                "invariant".to_owned(),
                invariant.name.text.clone(),
            ];
            records.push(DomainSourceRecord {
                span: invariant.span,
                path: invariant_path.clone(),
                steps: Vec::new(),
                secondary: Vec::new(),
            });
            records.push(DomainSourceRecord {
                span: invariant.expr.span,
                path: invariant_path.clone(),
                steps: expression_lowering_steps(&invariant.expr),
                secondary: vec![source_site(invariant_path, invariant.span)],
            });
        }
    }
    for effect in &domain.effects {
        records.push(DomainSourceRecord {
            span: span_at(effect.loc),
            path: vec![
                domain.name.clone(),
                "effect".to_owned(),
                effect.name.clone(),
            ],
            steps: vec![LoweringStep {
                kind: "lower_effect_lifecycle".to_owned(),
                detail: None,
            }],
            secondary: Vec::new(),
        });
    }
    records
}

fn same_origin_position(left: Span, right: Span) -> bool {
    left.start.line == right.start.line && left.start.column == right.start.column
}

fn origin_chain_for_span(
    target: &str,
    span: Span,
    records: &[DomainSourceRecord],
    generated: bool,
) -> OriginChain {
    let matches = records
        .iter()
        .filter(|record| same_origin_position(record.span, span))
        .collect::<Vec<_>>();
    let Some(primary) = matches.first() else {
        return OriginChain::generated_only(format!("domain:generated:{target}"), "domain");
    };
    let id = format!(
        "domain:{}:{}:{}:{}:{}",
        primary.path.join("/"),
        primary.span.start.offset,
        primary.span.end.offset,
        primary.span.start.line,
        primary.span.start.column
    );
    let mut secondary = primary.secondary.clone();
    secondary.extend(
        matches
            .iter()
            .skip(1)
            .map(|record| source_site(record.path.clone(), record.span)),
    );
    OriginChain {
        id: OriginId(id),
        dialect: "domain".to_owned(),
        primary: Some(source_site(primary.path.clone(), primary.span)),
        secondary,
        lowering_steps: primary.steps.clone(),
        generated,
    }
}

fn statement_span(statement: &Statement) -> Span {
    match statement {
        Statement::Assign { span, .. }
        | Statement::If { span, .. }
        | Statement::ForAll { span, .. } => *span,
    }
}

fn action_item_span(item: &ActionItem) -> Span {
    match item {
        ActionItem::Requires(_, span)
        | ActionItem::Ensures(_, span)
        | ActionItem::Let(_, _, span) => *span,
        ActionItem::Statement(statement) => statement_span(statement),
    }
}

fn bind_expression_tree(
    registry: &mut OriginRegistry,
    target: &str,
    expression: &Expr,
    origin: &OriginChain,
) {
    registry.bind(target, origin.clone());
    let child = |suffix: &str| format!("{target}:expr:{suffix}");
    match expression {
        Expr::Some(value) | Expr::Neg(value) | Expr::Not(value) => {
            bind_expression_tree(registry, &child("operand"), value, origin);
        }
        Expr::Set(values) | Expr::Seq(values) => {
            for (index, value) in values.iter().enumerate() {
                bind_expression_tree(registry, &child(&index.to_string()), value, origin);
            }
        }
        Expr::Struct { fields, .. } => {
            for (name, value) in fields {
                bind_expression_tree(registry, &child(name), value, origin);
            }
        }
        Expr::Call { args, .. } => {
            for (index, value) in args.iter().enumerate() {
                bind_expression_tree(registry, &child(&index.to_string()), value, origin);
            }
        }
        Expr::Index(base, index) => {
            bind_expression_tree(registry, &child("base"), base, origin);
            bind_expression_tree(registry, &child("index"), index, origin);
        }
        Expr::Field(base, _) => {
            bind_expression_tree(registry, &child("base"), base, origin);
        }
        Expr::Method { receiver, args, .. } => {
            bind_expression_tree(registry, &child("receiver"), receiver, origin);
            for (index, value) in args.iter().enumerate() {
                bind_expression_tree(registry, &child(&index.to_string()), value, origin);
            }
        }
        Expr::Binary { left, right, .. } | Expr::BinaryNamed { left, right, .. } => {
            bind_expression_tree(registry, &child("left"), left, origin);
            bind_expression_tree(registry, &child("right"), right, origin);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            bind_expression_tree(registry, &child("condition"), condition, origin);
            bind_expression_tree(registry, &child("then"), then_expr, origin);
            bind_expression_tree(registry, &child("else"), else_expr, origin);
        }
        Expr::Is { expr, .. }
        | Expr::Stage { entity: expr, .. }
        | Expr::UnaryNamed { expr, .. } => {
            bind_expression_tree(registry, &child("operand"), expr, origin);
        }
        Expr::Quantified { body, .. } => {
            bind_expression_tree(registry, &child("body"), body, origin);
        }
        Expr::Aggregate { value, .. } => {
            if let Some(value) = value {
                bind_expression_tree(registry, &child("value"), value, origin);
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            bind_expression_tree(registry, &child("first"), first, origin);
            bind_expression_tree(registry, &child("second"), second, origin);
            bind_expression_tree(registry, &child("third"), third, origin);
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) => {}
    }
}

fn domain_property_expression_span(domain: &DomainSpec, kind: &str, name: &str) -> Option<Span> {
    if kind != "invariant" {
        return None;
    }
    domain
        .aggregates
        .iter()
        .find_map(|aggregate| {
            aggregate.invariants.iter().find_map(|invariant| {
                (format!("{}_{}", safe(&aggregate.name), safe(&invariant.name)) == name)
                    .then_some(invariant.expr.span)
            })
        })
        .or_else(|| {
            domain.sagas.iter().find_map(|saga| {
                saga.invariants.iter().find_map(|invariant| {
                    (format!("{}_{}", safe(&saga.name), safe(&invariant.name)) == name)
                        .then_some(invariant.expr.span)
                })
            })
        })
}

#[allow(clippy::too_many_lines)]
fn domain_origin_registry(domain: &DomainSpec, surface: &SurfaceSpec) -> OriginRegistry {
    let records = domain_source_records(domain);
    let mut registry = OriginRegistry::default();

    registry.bind(
        SPEC_TARGET,
        origin_chain_for_span(SPEC_TARGET, span_at(domain.loc), &records, false),
    );
    for item in &surface.items {
        let (SpecItem::Type { name, .. }
        | SpecItem::Enum { name, .. }
        | SpecItem::Struct { name, .. }) = item
        else {
            continue;
        };
        let target = type_target(name);
        let matches = domain
            .types
            .iter()
            .filter(|ty| ty.name == *name)
            .collect::<Vec<_>>();
        if matches.is_empty() {
            registry.bind(
                target,
                OriginChain::generated_only(format!("domain:generated:type:{name}"), "domain"),
            );
        } else {
            for ty in matches {
                registry.bind(
                    target.clone(),
                    origin_chain_for_span(&target, span_at(ty.loc), &records, false),
                );
            }
        }
    }

    for aggregate in &domain.aggregates {
        for field in &aggregate.state {
            let target = state_target(&state_name(aggregate, &field.name));
            registry.bind(
                target.clone(),
                origin_chain_for_span(&target, field.span, &records, false),
            );
        }
    }
    for effect in &domain.effects {
        for name in [status_var(effect), attempt_var(effect)] {
            let target = state_target(&name);
            registry.bind(
                target.clone(),
                origin_chain_for_span(&target, span_at(effect.loc), &records, true),
            );
        }
    }
    for aggregate in &domain.aggregates {
        for event in &aggregate.events {
            let target = state_target(&event_flag(&event.name));
            registry.bind(
                target.clone(),
                OriginChain::generated_only(
                    format!("domain:generated:event-flag:{}", event.name),
                    "domain",
                ),
            );
        }
    }

    let mut init_index = 0;
    for item in &surface.items {
        match item {
            SpecItem::Init { statements, .. } => {
                for statement in statements {
                    let target = init_statement_target(init_index);
                    registry.bind(
                        target.clone(),
                        origin_chain_for_span(&target, statement_span(statement), &records, true),
                    );
                    init_index += 1;
                }
            }
            SpecItem::Action {
                name, items, span, ..
            } => {
                let target = action_target(name);
                registry.bind(
                    target.clone(),
                    origin_chain_for_span(&target, *span, &records, true),
                );
                let mut guard_index = 0;
                let mut statement_index = 0;
                for item in items {
                    let target = match item {
                        ActionItem::Requires(..) | ActionItem::Let(..) => {
                            let target = action_guard_target(name, guard_index);
                            guard_index += 1;
                            target
                        }
                        ActionItem::Ensures(..) => {
                            let target = format!("action:{name}:ensure:{guard_index}");
                            guard_index += 1;
                            target
                        }
                        ActionItem::Statement(..) => {
                            let target = action_statement_target(name, statement_index);
                            statement_index += 1;
                            target
                        }
                    };
                    let origin =
                        origin_chain_for_span(&target, action_item_span(item), &records, true);
                    registry.bind(target.clone(), origin.clone());
                    match item {
                        ActionItem::Requires(expression, _)
                        | ActionItem::Ensures(expression, _)
                        | ActionItem::Let(_, expression, _) => bind_expression_tree(
                            &mut registry,
                            &format!("{target}:expr:root"),
                            expression,
                            &origin,
                        ),
                        ActionItem::Statement(..) => {}
                    }
                }
            }
            SpecItem::Invariant {
                name, expr, span, ..
            } => {
                let target = property_target("invariant", name);
                let source_span =
                    domain_property_expression_span(domain, "invariant", name).unwrap_or(*span);
                let origin = origin_chain_for_span(&target, source_span, &records, false);
                registry.bind(target.clone(), origin.clone());
                bind_expression_tree(&mut registry, &format!("{target}:expr:root"), expr, &origin);
            }
            SpecItem::Trans {
                name, expr, span, ..
            } => {
                let target = property_target("trans", name);
                let origin = origin_chain_for_span(&target, *span, &records, true);
                registry.bind(target.clone(), origin.clone());
                bind_expression_tree(&mut registry, &format!("{target}:expr:root"), expr, &origin);
            }
            SpecItem::Reachable {
                name, expr, span, ..
            } => {
                let target = property_target("reachable", name);
                let origin = origin_chain_for_span(&target, *span, &records, false);
                registry.bind(target.clone(), origin.clone());
                bind_expression_tree(&mut registry, &format!("{target}:expr:root"), expr, &origin);
            }
            SpecItem::Terminal { .. } => registry.bind(
                TERMINAL_TARGET,
                OriginChain::generated_only(
                    format!("domain:generated:terminal:{}", domain.name),
                    "domain",
                ),
            ),
            _ => {}
        }
    }
    registry
}
