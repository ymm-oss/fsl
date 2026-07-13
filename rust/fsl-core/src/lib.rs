// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Typed kernel lowering and semantic model for the Rust FSL port.

use std::collections::{HashMap, HashSet};
use std::fmt;

use fsl_syntax::{
    ActionItem, Binder, BusinessItem, Expr, LValue, Param, ParseError, RequirementsItem, SpecItem,
    Statement, SurfaceDocument, SurfaceSpec, TypeExpr, VerifyItem, parse_surface_document,
};
use serde_json::Value;

pub use fsl_syntax::{
    Binder as KernelBinder, Expr as KernelExpr, LValue as KernelLValue, Pattern, QualifiedName,
    Statement as KernelStatement,
};

mod compose;
mod db;
mod dialect;
mod domain;
mod model;
mod refinement;
mod trace;

pub use compose::{FileResolver, FsResolver, lower_compose, parse_kernel_source};
pub use db::db_kernel_source;
pub use dialect::{
    GovernanceContract, GovernanceDelegate, GovernancePreservation, RequirementsTraceCase,
    RequirementsTraceContract, RequirementsTraceExpectation, RequirementsTraceStep,
    governance_contract, lower_ai_component, lower_business, lower_db, lower_domain,
    lower_governance, lower_requirements, requirements_trace_contract,
};
pub use domain::domain_kernel_source;
pub use model::{
    ActionDef, ActionGuard, KernelModel, LeadsToDef, ModelError, ParamDef, PropertyDef, TypeDef,
    TypeRef, Value as FslValue, build_model,
};
pub use refinement::{
    ActionMap, ActionMapTarget, ImplementsContract, ProgressMap, Refinement, RefinementError,
    StateMap, parse_refinement, requirements_implements,
};
pub use trace::{TraceAction, TraceChange, TraceStep};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreError {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}:{}",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for CoreError {}

impl From<ParseError> for CoreError {
    fn from(error: ParseError) -> Self {
        Self {
            message: error.message,
            line: error.span.start.line,
            column: error.span.start.column,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelSpec {
    spec: SurfaceSpec,
}

/// Build a checked kernel model from an already parsed surface specification.
///
/// This is used by native structural tools that clone and mutate the typed
/// surface tree before re-running the ordinary semantic lowering gate.
///
/// # Errors
///
/// Returns [`ModelError`] when the mutated surface is not semantically valid.
pub fn build_surface_model(spec: SurfaceSpec) -> Result<KernelModel, ModelError> {
    build_model(KernelSpec { spec })
}

impl KernelSpec {
    #[must_use]
    pub fn python_ast(&self) -> Value {
        self.spec.python_ast()
    }

    #[must_use]
    pub fn syntax(&self) -> &SurfaceSpec {
        &self.spec
    }

    #[must_use]
    pub fn into_syntax(self) -> SurfaceSpec {
        self.spec
    }
}

/// Parse and lower one direct kernel `spec` source.
///
/// This first Phase-1 slice performs the same named-predicate validation and
/// capture-safe inlining as the Python reference. Dialect and file-based
/// lowering are separate resolver-backed slices.
///
/// # Errors
///
/// Returns [`CoreError`] for parse failures, non-`spec` documents, invalid
/// predicate definitions, recursion, arity mismatches, or variable capture.
pub fn parse_direct_kernel_spec(source: &str) -> Result<KernelSpec, CoreError> {
    let SurfaceDocument::Spec(spec) = parse_surface_document(source)? else {
        return Err(CoreError {
            message: "expected direct kernel spec".to_owned(),
            line: 1,
            column: 1,
        });
    };
    lower_direct_spec(spec)
}

fn validate_direct_scope_overrides(
    spec: &SurfaceSpec,
    instances: &std::collections::BTreeMap<String, i64>,
    values: &std::collections::BTreeMap<String, (i64, i64)>,
) -> Result<(), CoreError> {
    let entities = spec
        .items
        .iter()
        .filter_map(|item| match item {
            SpecItem::Entity(name, _) => Some(name.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let numbers = spec
        .items
        .iter()
        .filter_map(|item| match item {
            SpecItem::Number(name, _) => Some(name.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let error = |message| CoreError {
        message,
        line: 1,
        column: 1,
    };
    if (!instances.is_empty() || !values.is_empty()) && entities.is_empty() && numbers.is_empty() {
        return Err(error(
            "--instances/--values only apply to specs with entity/number declarations; this spec declares neither".to_owned(),
        ));
    }
    if let Some(name) = instances
        .keys()
        .find(|name| !entities.contains(name.as_str()))
    {
        return Err(error(format!(
            "verify instances references undeclared entity '{name}'"
        )));
    }
    if let Some(name) = values.keys().find(|name| !numbers.contains(name.as_str())) {
        return Err(error(format!(
            "verify values references undeclared number '{name}'"
        )));
    }
    Ok(())
}

/// Parse a direct spec/business/requirements document with temporary verify-bound overrides.
///
/// This is the semantic seam used by the `sweep` driver; it does not mutate
/// source files or the ordinary parser result.
///
/// # Errors
///
/// Returns [`CoreError`] for parse, lowering, or unsupported top-level input.
pub fn parse_kernel_source_with_bounds(
    source: &str,
    instances: &std::collections::BTreeMap<String, i64>,
    values: &std::collections::BTreeMap<String, (i64, i64)>,
) -> Result<KernelSpec, CoreError> {
    fn update_bounds(
        items: &mut [VerifyItem],
        instances: &std::collections::BTreeMap<String, i64>,
        values: &std::collections::BTreeMap<String, (i64, i64)>,
    ) {
        for bound in items {
            match bound {
                VerifyItem::Instances(name, count, _) => {
                    if let Some(value) = instances.get(name) {
                        *count = *value;
                    }
                }
                VerifyItem::Values(name, lo, hi, _) => {
                    if let Some((new_lo, new_hi)) = values.get(name) {
                        **lo = Expr::Num(*new_lo);
                        **hi = Expr::Num(*new_hi);
                    }
                }
            }
        }
    }
    fn update(
        item: &mut SpecItem,
        instances: &std::collections::BTreeMap<String, i64>,
        values: &std::collections::BTreeMap<String, (i64, i64)>,
    ) {
        if let SpecItem::VerifyBounds { items, .. } = item {
            update_bounds(items, instances, values);
        }
    }

    match parse_surface_document(source)? {
        SurfaceDocument::Spec(mut spec) => {
            validate_direct_scope_overrides(&spec, instances, values)?;
            for item in &mut spec.items {
                update(item, instances, values);
            }
            lower_direct_spec(spec)
        }
        SurfaceDocument::Business(mut business) => {
            for item in &mut business.items {
                if let BusinessItem::VerifyBounds { items, .. } = item {
                    update_bounds(items, instances, values);
                }
            }
            lower_business(business)
        }
        SurfaceDocument::Requirements(mut requirements) => {
            for item in &mut requirements.items {
                if let RequirementsItem::Common(item) = item {
                    update(item, instances, values);
                }
            }
            lower_requirements(requirements)
        }
        _ => Err(CoreError {
            message: "scope overrides only support spec, business, or requirements".to_owned(),
            line: 1,
            column: 1,
        }),
    }
}

/// Lower a parsed direct `spec` into the kernel representation.
///
/// # Errors
///
/// Returns [`CoreError`] when named predicate validation or expansion fails.
pub fn lower_direct_spec(spec: SurfaceSpec) -> Result<KernelSpec, CoreError> {
    let spec = PredicateExpander::new(&spec)?.expand(spec)?;
    let spec = expand_spec_domains(spec)?;
    Ok(KernelSpec { spec })
}

#[allow(clippy::too_many_lines)]
fn expand_spec_domains(mut spec: SurfaceSpec) -> Result<SurfaceSpec, CoreError> {
    if !spec
        .items
        .iter()
        .any(|item| matches!(item, SpecItem::Entity(..) | SpecItem::Number(..)))
    {
        return Ok(spec);
    }
    let mut instances = HashMap::new();
    let mut values = HashMap::new();
    for item in &spec.items {
        if let SpecItem::VerifyBounds { items, .. } = item {
            for bound in items {
                match bound {
                    VerifyItem::Instances(name, value, span) => {
                        if instances.insert(name.clone(), (*value, *span)).is_some() {
                            return Err(core_error(
                                format!("duplicate instances bound for '{name}'"),
                                *span,
                            ));
                        }
                    }
                    VerifyItem::Values(name, lo, hi, span) => {
                        if values
                            .insert(
                                name.clone(),
                                (lo.as_ref().clone(), hi.as_ref().clone(), *span),
                            )
                            .is_some()
                        {
                            return Err(core_error(
                                format!("duplicate values bound for '{name}'"),
                                *span,
                            ));
                        }
                    }
                }
            }
        }
    }
    let mut entities = Vec::new();
    let mut numbers = Vec::new();
    for item in &spec.items {
        match item {
            SpecItem::Entity(name, span) => entities.push((name.clone(), *span)),
            SpecItem::Number(name, span) => numbers.push((name.clone(), *span)),
            _ => {}
        }
    }
    let mut types = Vec::new();
    for (name, span) in &entities {
        let Some((count, bound_span)) = instances.remove(name) else {
            return Err(core_error(
                format!("entity '{name}' has no 'instances' bound in verify block"),
                *span,
            ));
        };
        if count < 1 {
            return Err(core_error(
                format!("entity '{name}' instances bound must be >= 1"),
                bound_span,
            ));
        }
        types.push(SpecItem::Type {
            name: name.clone(),
            lo: Box::new(Expr::Num(0)),
            hi: Box::new(Expr::Num(count - 1)),
            symmetric: false,
        });
    }
    for (name, span) in &numbers {
        let Some((lo, hi, _)) = values.remove(name) else {
            return Err(core_error(
                format!("number '{name}' has no 'values' bound in verify block"),
                *span,
            ));
        };
        types.push(SpecItem::Type {
            name: name.clone(),
            lo: Box::new(lo),
            hi: Box::new(hi),
            symmetric: false,
        });
    }
    if let Some((name, (_, span))) = instances.into_iter().next() {
        return Err(core_error(
            format!("verify instances for undeclared entity '{name}'"),
            span,
        ));
    }
    if let Some((name, (_, _, span))) = values.into_iter().next() {
        return Err(core_error(
            format!("verify values for undeclared number '{name}'"),
            span,
        ));
    }
    types.extend(spec.items.into_iter().filter(|item| {
        !matches!(
            item,
            SpecItem::Entity(..) | SpecItem::Number(..) | SpecItem::VerifyBounds { .. }
        )
    }));
    spec.items = types;
    Ok(spec)
}

#[derive(Clone)]
struct Definition {
    params: Vec<String>,
    body: Expr,
    line: u32,
    column: u32,
}

struct PredicateExpander {
    definitions: HashMap<String, Definition>,
}

impl PredicateExpander {
    fn new(spec: &SurfaceSpec) -> Result<Self, CoreError> {
        let mut definitions = HashMap::new();
        for item in &spec.items {
            let SpecItem::Def {
                name,
                params,
                value,
                span,
            } = item
            else {
                continue;
            };
            if definitions.contains_key(name) {
                return Err(core_error(format!("duplicate def '{name}'"), *span));
            }
            let names = params
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            if names.iter().collect::<HashSet<_>>().len() != names.len() {
                return Err(core_error(
                    format!("duplicate parameter in def '{name}'"),
                    *span,
                ));
            }
            let bound = bound_vars(value);
            if let Some(shadowed) = names.iter().filter(|name| bound.contains(*name)).min() {
                return Err(core_error(
                    format!("def '{name}' parameter is shadowed by binder '{shadowed}'"),
                    *span,
                ));
            }
            definitions.insert(
                name.clone(),
                Definition {
                    params: names,
                    body: value.as_ref().clone(),
                    line: span.start.line,
                    column: span.start.column,
                },
            );
        }
        let expander = Self { definitions };
        for name in expander.definitions.keys() {
            expander.validate_definition(name, &mut Vec::new())?;
        }
        Ok(expander)
    }

    fn validate_definition(&self, name: &str, stack: &mut Vec<String>) -> Result<(), CoreError> {
        if stack.iter().any(|entry| entry == name) {
            let mut cycle = stack.clone();
            cycle.push(name.to_owned());
            let definition = &self.definitions[name];
            return Err(CoreError {
                message: format!(
                    "recursive predicate definition is not allowed: {}",
                    cycle.join(" -> ")
                ),
                line: definition.line,
                column: definition.column,
            });
        }
        stack.push(name.to_owned());
        self.validate_calls(&self.definitions[name].body, stack)?;
        stack.pop();
        Ok(())
    }

    fn validate_calls(&self, expr: &Expr, stack: &mut Vec<String>) -> Result<(), CoreError> {
        if let Expr::Call {
            name, args, span, ..
        } = expr
        {
            let Some(definition) = self.definitions.get(name) else {
                return Err(core_error(format!("undefined predicate '{name}'"), *span));
            };
            if args.len() != definition.params.len() {
                return Err(core_error(
                    format!(
                        "predicate '{name}' expects {} argument(s), got {}",
                        definition.params.len(),
                        args.len()
                    ),
                    *span,
                ));
            }
            self.validate_definition(name, stack)?;
        }
        visit_expr_children(expr, &mut |child| self.validate_calls(child, stack))
    }

    fn expand(&self, mut spec: SurfaceSpec) -> Result<SurfaceSpec, CoreError> {
        let mut items = Vec::new();
        for item in spec.items {
            if matches!(item, SpecItem::Def { .. }) {
                continue;
            }
            items.push(self.expand_item(item)?);
        }
        spec.items = items;
        Ok(spec)
    }

    #[allow(clippy::too_many_lines)]
    fn expand_item(&self, item: SpecItem) -> Result<SpecItem, CoreError> {
        Ok(match item {
            SpecItem::Const { name, value } => SpecItem::Const {
                name,
                value: Box::new(self.expand_expr(*value, &mut Vec::new())?),
            },
            SpecItem::Type {
                name,
                lo,
                hi,
                symmetric,
            } => SpecItem::Type {
                name,
                lo: Box::new(self.expand_expr(*lo, &mut Vec::new())?),
                hi: Box::new(self.expand_expr(*hi, &mut Vec::new())?),
                symmetric,
            },
            SpecItem::Struct { name, fields } => SpecItem::Struct {
                name,
                fields: fields
                    .into_iter()
                    .map(|(name, ty)| Ok((name, self.expand_type(ty)?)))
                    .collect::<Result<_, CoreError>>()?,
            },
            SpecItem::State(fields) => SpecItem::State(
                fields
                    .into_iter()
                    .map(|(name, ty)| Ok((name, self.expand_type(ty)?)))
                    .collect::<Result<_, CoreError>>()?,
            ),
            SpecItem::Init(statements) => {
                SpecItem::Init(self.expand_statements(statements, &mut Vec::new())?)
            }
            SpecItem::Action {
                name,
                params,
                items,
                span,
                fair,
                meta,
                sync,
            } => SpecItem::Action {
                name,
                params: params
                    .into_iter()
                    .map(|param| self.expand_param(param))
                    .collect::<Result<_, _>>()?,
                items: items
                    .into_iter()
                    .map(|item| self.expand_action_item(item))
                    .collect::<Result<_, _>>()?,
                span,
                fair,
                meta,
                sync,
            },
            SpecItem::Invariant {
                name,
                expr,
                span,
                meta,
            } => SpecItem::Invariant {
                name,
                expr: Box::new(self.expand_expr(*expr, &mut Vec::new())?),
                span,
                meta,
            },
            SpecItem::Trans {
                name,
                expr,
                span,
                meta,
            } => SpecItem::Trans {
                name,
                expr: Box::new(self.expand_expr(*expr, &mut Vec::new())?),
                span,
                meta,
            },
            SpecItem::Reachable {
                name,
                expr,
                span,
                meta,
            } => SpecItem::Reachable {
                name,
                expr: Box::new(self.expand_expr(*expr, &mut Vec::new())?),
                span,
                meta,
            },
            SpecItem::Terminal { expr, span } => SpecItem::Terminal {
                expr: Box::new(self.expand_expr(*expr, &mut Vec::new())?),
                span,
            },
            SpecItem::Until {
                name,
                before,
                after,
                span,
                meta,
            } => SpecItem::Until {
                name,
                before: Box::new(self.expand_expr(*before, &mut Vec::new())?),
                after: Box::new(self.expand_expr(*after, &mut Vec::new())?),
                span,
                meta,
            },
            SpecItem::Unless {
                name,
                before,
                after,
                span,
                meta,
            } => SpecItem::Unless {
                name,
                before: Box::new(self.expand_expr(*before, &mut Vec::new())?),
                after: Box::new(self.expand_expr(*after, &mut Vec::new())?),
                span,
                meta,
            },
            SpecItem::LeadsTo {
                name,
                binders,
                before,
                after,
                span,
                meta,
                decreases,
                within,
                helpful,
            } => SpecItem::LeadsTo {
                name,
                binders: binders
                    .into_iter()
                    .map(|binder| self.expand_binder(binder, &mut Vec::new()))
                    .collect::<Result<_, _>>()?,
                before: Box::new(self.expand_expr(*before, &mut Vec::new())?),
                after: Box::new(self.expand_expr(*after, &mut Vec::new())?),
                span,
                meta,
                decreases: decreases
                    .map(|expr| self.expand_expr(*expr, &mut Vec::new()).map(Box::new))
                    .transpose()?,
                within: within
                    .map(|expr| self.expand_expr(*expr, &mut Vec::new()).map(Box::new))
                    .transpose()?,
                helpful: helpful
                    .into_iter()
                    .map(|mut entry| {
                        entry.args = entry
                            .args
                            .into_iter()
                            .map(|arg| self.expand_expr(arg, &mut Vec::new()))
                            .collect::<Result<_, _>>()?;
                        Ok(entry)
                    })
                    .collect::<Result<_, CoreError>>()?,
            },
            SpecItem::VerifyBounds { items, span } => SpecItem::VerifyBounds {
                items: items
                    .into_iter()
                    .map(|item| match item {
                        VerifyItem::Instances(..) => Ok(item),
                        VerifyItem::Values(name, lo, hi, span) => Ok(VerifyItem::Values(
                            name,
                            Box::new(self.expand_expr(*lo, &mut Vec::new())?),
                            Box::new(self.expand_expr(*hi, &mut Vec::new())?),
                            span,
                        )),
                    })
                    .collect::<Result<_, CoreError>>()?,
                span,
            },
            item @ (SpecItem::Enum { .. } | SpecItem::Entity(..) | SpecItem::Number(..)) => item,
            SpecItem::Def { .. } => unreachable!("definitions are removed before item expansion"),
        })
    }

    fn expand_action_item(&self, item: ActionItem) -> Result<ActionItem, CoreError> {
        Ok(match item {
            ActionItem::Requires(expr, span) => {
                ActionItem::Requires(self.expand_expr(expr, &mut Vec::new())?, span)
            }
            ActionItem::Ensures(expr, span) => {
                ActionItem::Ensures(self.expand_expr(expr, &mut Vec::new())?, span)
            }
            ActionItem::Let(name, expr, span) => {
                ActionItem::Let(name, self.expand_expr(expr, &mut Vec::new())?, span)
            }
            ActionItem::Statement(statement) => {
                ActionItem::Statement(self.expand_statement(statement, &mut Vec::new())?)
            }
        })
    }

    fn expand_param(&self, param: Param) -> Result<Param, CoreError> {
        Ok(match param {
            Param::Typed(..) => param,
            Param::Range(name, lo, hi) => Param::Range(
                name,
                self.expand_expr(lo, &mut Vec::new())?,
                self.expand_expr(hi, &mut Vec::new())?,
            ),
        })
    }

    fn expand_type(&self, ty: TypeExpr) -> Result<TypeExpr, CoreError> {
        Ok(match ty {
            TypeExpr::Range(lo, hi) => TypeExpr::Range(
                self.expand_expr(lo, &mut Vec::new())?,
                self.expand_expr(hi, &mut Vec::new())?,
            ),
            TypeExpr::Map(key, value) => TypeExpr::Map(
                Box::new(self.expand_type(*key)?),
                Box::new(self.expand_type(*value)?),
            ),
            TypeExpr::Relation(source, target) => TypeExpr::Relation(
                Box::new(self.expand_type(*source)?),
                Box::new(self.expand_type(*target)?),
            ),
            TypeExpr::Set(inner) => TypeExpr::Set(Box::new(self.expand_type(*inner)?)),
            TypeExpr::Seq(inner, cap) => TypeExpr::Seq(
                Box::new(self.expand_type(*inner)?),
                self.expand_expr(cap, &mut Vec::new())?,
            ),
            TypeExpr::Option(inner) => TypeExpr::Option(Box::new(self.expand_type(*inner)?)),
            TypeExpr::Int => TypeExpr::Int,
            TypeExpr::Bool => TypeExpr::Bool,
            TypeExpr::Name(name) => TypeExpr::Name(name),
        })
    }

    fn expand_statements(
        &self,
        statements: Vec<Statement>,
        stack: &mut Vec<String>,
    ) -> Result<Vec<Statement>, CoreError> {
        statements
            .into_iter()
            .map(|statement| self.expand_statement(statement, stack))
            .collect()
    }

    fn expand_statement(
        &self,
        statement: Statement,
        stack: &mut Vec<String>,
    ) -> Result<Statement, CoreError> {
        Ok(match statement {
            Statement::Assign {
                target,
                value,
                span,
            } => Statement::Assign {
                target: self.expand_lvalue(target, stack)?,
                value: self.expand_expr(value, stack)?,
                span,
            },
            Statement::If {
                condition,
                then_statements,
                else_statements,
                span,
            } => Statement::If {
                condition: self.expand_expr(condition, stack)?,
                then_statements: self.expand_statements(then_statements, stack)?,
                else_statements: self.expand_statements(else_statements, stack)?,
                span,
            },
            Statement::ForAll {
                binder,
                statements,
                span,
            } => Statement::ForAll {
                binder: self.expand_binder(binder, stack)?,
                statements: self.expand_statements(statements, stack)?,
                span,
            },
        })
    }

    fn expand_lvalue(&self, lvalue: LValue, stack: &mut Vec<String>) -> Result<LValue, CoreError> {
        Ok(match lvalue {
            LValue::Index(name, expr) => LValue::Index(name, self.expand_expr(expr, stack)?),
            LValue::Field(base, field) => {
                LValue::Field(Box::new(self.expand_lvalue(*base, stack)?), field)
            }
            lvalue @ LValue::Var(_) => lvalue,
        })
    }

    fn expand_binder(&self, binder: Binder, stack: &mut Vec<String>) -> Result<Binder, CoreError> {
        Ok(match binder {
            Binder::Typed {
                name,
                type_name,
                where_expr,
            } => Binder::Typed {
                name,
                type_name,
                where_expr: where_expr
                    .map(|expr| self.expand_expr(*expr, stack).map(Box::new))
                    .transpose()?,
            },
            Binder::Range { name, lo, hi } => Binder::Range {
                name,
                lo: Box::new(self.expand_expr(*lo, stack)?),
                hi: Box::new(self.expand_expr(*hi, stack)?),
            },
            Binder::Collection {
                name,
                collection,
                where_expr,
            } => Binder::Collection {
                name,
                collection: Box::new(self.expand_expr(*collection, stack)?),
                where_expr: where_expr
                    .map(|expr| self.expand_expr(*expr, stack).map(Box::new))
                    .transpose()?,
            },
        })
    }

    #[allow(clippy::too_many_lines)]
    fn expand_expr(&self, expr: Expr, stack: &mut Vec<String>) -> Result<Expr, CoreError> {
        if let Expr::Call { name, args, span } = expr {
            let Some(definition) = self.definitions.get(&name) else {
                return Err(core_error(format!("undefined predicate '{name}'"), span));
            };
            if stack.contains(&name) {
                let mut cycle = stack.clone();
                cycle.push(name.clone());
                return Err(core_error(
                    format!(
                        "recursive predicate definition is not allowed: {}",
                        cycle.join(" -> ")
                    ),
                    span,
                ));
            }
            let args = args
                .into_iter()
                .map(|arg| self.expand_expr(arg, stack))
                .collect::<Result<Vec<_>, _>>()?;
            if args.len() != definition.params.len() {
                return Err(core_error(
                    format!(
                        "predicate '{name}' expects {} argument(s), got {}",
                        definition.params.len(),
                        args.len()
                    ),
                    span,
                ));
            }
            let collisions = bound_vars(&definition.body);
            let free = args.iter().flat_map(free_vars).collect::<HashSet<_>>();
            if let Some(captured) = collisions.intersection(&free).min() {
                return Err(core_error(
                    format!(
                        "predicate '{name}' call would capture variable '{captured}'; rename the binder in the def"
                    ),
                    span,
                ));
            }
            stack.push(name);
            let body = self.expand_expr(definition.body.clone(), stack)?;
            stack.pop();
            let replacements = definition
                .params
                .iter()
                .cloned()
                .zip(args)
                .collect::<HashMap<_, _>>();
            return Ok(substitute(body, &replacements));
        }
        Ok(match expr {
            Expr::Some(expr) => Expr::Some(Box::new(self.expand_expr(*expr, stack)?)),
            Expr::Set(items) => Expr::Set(
                items
                    .into_iter()
                    .map(|item| self.expand_expr(item, stack))
                    .collect::<Result<_, _>>()?,
            ),
            Expr::Seq(items) => Expr::Seq(
                items
                    .into_iter()
                    .map(|item| self.expand_expr(item, stack))
                    .collect::<Result<_, _>>()?,
            ),
            Expr::Struct { name, fields } => Expr::Struct {
                name,
                fields: fields
                    .into_iter()
                    .map(|(name, expr)| Ok((name, self.expand_expr(expr, stack)?)))
                    .collect::<Result<_, CoreError>>()?,
            },
            Expr::Index(base, index) => Expr::Index(
                Box::new(self.expand_expr(*base, stack)?),
                Box::new(self.expand_expr(*index, stack)?),
            ),
            Expr::Field(base, name) => Expr::Field(Box::new(self.expand_expr(*base, stack)?), name),
            Expr::Method {
                receiver,
                name,
                args,
            } => Expr::Method {
                receiver: Box::new(self.expand_expr(*receiver, stack)?),
                name,
                args: args
                    .into_iter()
                    .map(|arg| self.expand_expr(arg, stack))
                    .collect::<Result<_, _>>()?,
            },
            Expr::Binary { op, left, right } => Expr::Binary {
                op,
                left: Box::new(self.expand_expr(*left, stack)?),
                right: Box::new(self.expand_expr(*right, stack)?),
            },
            Expr::Neg(expr) => Expr::Neg(Box::new(self.expand_expr(*expr, stack)?)),
            Expr::Not(expr) => Expr::Not(Box::new(self.expand_expr(*expr, stack)?)),
            Expr::IfThenElse {
                condition,
                then_expr,
                else_expr,
            } => Expr::IfThenElse {
                condition: Box::new(self.expand_expr(*condition, stack)?),
                then_expr: Box::new(self.expand_expr(*then_expr, stack)?),
                else_expr: Box::new(self.expand_expr(*else_expr, stack)?),
            },
            Expr::Is { expr, pattern } => Expr::Is {
                expr: Box::new(self.expand_expr(*expr, stack)?),
                pattern,
            },
            Expr::Quantified {
                quantifier,
                binder,
                body,
            } => Expr::Quantified {
                quantifier,
                binder: self.expand_binder(binder, stack)?,
                body: Box::new(self.expand_expr(*body, stack)?),
            },
            Expr::Count {
                name,
                type_name,
                condition,
            } => Expr::Count {
                name,
                type_name,
                condition: Box::new(self.expand_expr(*condition, stack)?),
            },
            Expr::Sum {
                name,
                type_name,
                body,
                condition,
            } => Expr::Sum {
                name,
                type_name,
                body: Box::new(self.expand_expr(*body, stack)?),
                condition: condition
                    .map(|expr| self.expand_expr(*expr, stack).map(Box::new))
                    .transpose()?,
            },
            Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
                name,
                expr: Box::new(self.expand_expr(*expr, stack)?),
                span,
            },
            Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
                name,
                left: Box::new(self.expand_expr(*left, stack)?),
                right: Box::new(self.expand_expr(*right, stack)?),
            },
            Expr::TernaryNamed {
                name,
                first,
                second,
                third,
            } => Expr::TernaryNamed {
                name,
                first: Box::new(self.expand_expr(*first, stack)?),
                second: Box::new(self.expand_expr(*second, stack)?),
                third: Box::new(self.expand_expr(*third, stack)?),
            },
            Expr::BinderNamed { name, binder } => Expr::BinderNamed {
                name,
                binder: self.expand_binder(binder, stack)?,
            },
            other => other,
        })
    }
}

fn core_error(message: String, span: fsl_syntax::Span) -> CoreError {
    CoreError {
        message,
        line: span.start.line,
        column: span.start.column,
    }
}

fn visit_expr_children(
    expr: &Expr,
    visitor: &mut impl FnMut(&Expr) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
    match expr {
        Expr::Some(expr)
        | Expr::Neg(expr)
        | Expr::Not(expr)
        | Expr::UnaryNamed { expr, .. }
        | Expr::Is { expr, .. } => visitor(expr)?,
        Expr::Set(items) | Expr::Seq(items) => {
            for item in items {
                visitor(item)?;
            }
        }
        Expr::Struct { fields, .. } => {
            for (_, expr) in fields {
                visitor(expr)?;
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                visitor(arg)?;
            }
        }
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => {
            visitor(left)?;
            visitor(right)?;
        }
        Expr::Field(base, _) => visitor(base)?,
        Expr::Method { receiver, args, .. } => {
            visitor(receiver)?;
            for arg in args {
                visitor(arg)?;
            }
        }
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => {
            visitor(condition)?;
            visitor(then_expr)?;
            visitor(else_expr)?;
        }
        Expr::Quantified { binder, body, .. } => {
            visit_binder_exprs(binder, visitor)?;
            visitor(body)?;
        }
        Expr::Count { condition, .. } => visitor(condition)?,
        Expr::Sum {
            body, condition, ..
        } => {
            visitor(body)?;
            if let Some(condition) = condition {
                visitor(condition)?;
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            visitor(first)?;
            visitor(second)?;
            visitor(third)?;
        }
        Expr::BinderNamed { binder, .. } => visit_binder_exprs(binder, visitor)?,
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) => {}
    }
    Ok(())
}

fn visit_binder_exprs(
    binder: &Binder,
    visitor: &mut impl FnMut(&Expr) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
    match binder {
        Binder::Typed { where_expr, .. } => {
            if let Some(expr) = where_expr {
                visitor(expr)?;
            }
        }
        Binder::Range { lo, hi, .. } => {
            visitor(lo)?;
            visitor(hi)?;
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            visitor(collection)?;
            if let Some(expr) = where_expr {
                visitor(expr)?;
            }
        }
    }
    Ok(())
}

fn bound_vars(expr: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    collect_bound_vars(expr, &mut out);
    out
}

fn collect_bound_vars(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Quantified { binder, .. } | Expr::BinderNamed { binder, .. } => {
            out.insert(binder_name(binder).to_owned());
        }
        Expr::Count { name, .. } | Expr::Sum { name, .. } => {
            out.insert(name.clone());
        }
        _ => {}
    }
    let _ = visit_expr_children(expr, &mut |child| {
        collect_bound_vars(child, out);
        Ok(())
    });
}

fn free_vars(expr: &Expr) -> HashSet<String> {
    free_vars_bound(expr, &HashSet::new())
}

fn free_vars_bound(expr: &Expr, bound: &HashSet<String>) -> HashSet<String> {
    if let Expr::Var(name) = expr {
        return if bound.contains(name) {
            HashSet::new()
        } else {
            HashSet::from([name.clone()])
        };
    }
    let mut child_bound = bound.clone();
    match expr {
        Expr::Quantified { binder, .. } | Expr::BinderNamed { binder, .. } => {
            child_bound.insert(binder_name(binder).to_owned());
        }
        Expr::Count { name, .. } | Expr::Sum { name, .. } => {
            child_bound.insert(name.clone());
        }
        _ => {}
    }
    let mut out = HashSet::new();
    let _ = visit_expr_children(expr, &mut |child| {
        out.extend(free_vars_bound(child, &child_bound));
        Ok(())
    });
    out
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn substitute<S: std::hash::BuildHasher>(
    expr: Expr,
    replacements: &HashMap<String, Expr, S>,
) -> Expr {
    if let Expr::Var(name) = &expr {
        if let Some(replacement) = replacements.get(name) {
            return replacement.clone();
        }
    }
    match expr {
        Expr::Some(expr) => Expr::Some(Box::new(substitute(*expr, replacements))),
        Expr::Set(items) => Expr::Set(
            items
                .into_iter()
                .map(|item| substitute(item, replacements))
                .collect(),
        ),
        Expr::Seq(items) => Expr::Seq(
            items
                .into_iter()
                .map(|item| substitute(item, replacements))
                .collect(),
        ),
        Expr::Struct { name, fields } => Expr::Struct {
            name,
            fields: fields
                .into_iter()
                .map(|(name, expr)| (name, substitute(expr, replacements)))
                .collect(),
        },
        Expr::Call { name, args, span } => Expr::Call {
            name,
            args: args
                .into_iter()
                .map(|arg| substitute(arg, replacements))
                .collect(),
            span,
        },
        Expr::Index(base, index) => Expr::Index(
            Box::new(substitute(*base, replacements)),
            Box::new(substitute(*index, replacements)),
        ),
        Expr::Field(base, name) => Expr::Field(Box::new(substitute(*base, replacements)), name),
        Expr::Method {
            receiver,
            name,
            args,
        } => Expr::Method {
            receiver: Box::new(substitute(*receiver, replacements)),
            name,
            args: args
                .into_iter()
                .map(|arg| substitute(arg, replacements))
                .collect(),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: Box::new(substitute(*left, replacements)),
            right: Box::new(substitute(*right, replacements)),
        },
        Expr::Neg(expr) => Expr::Neg(Box::new(substitute(*expr, replacements))),
        Expr::Not(expr) => Expr::Not(Box::new(substitute(*expr, replacements))),
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => Expr::IfThenElse {
            condition: Box::new(substitute(*condition, replacements)),
            then_expr: Box::new(substitute(*then_expr, replacements)),
            else_expr: Box::new(substitute(*else_expr, replacements)),
        },
        Expr::Is { expr, pattern } => Expr::Is {
            expr: Box::new(substitute(*expr, replacements)),
            pattern,
        },
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => Expr::Quantified {
            quantifier,
            binder: substitute_binder(binder, replacements),
            body: Box::new(substitute(*body, replacements)),
        },
        Expr::Count {
            name,
            type_name,
            condition,
        } => Expr::Count {
            name,
            type_name,
            condition: Box::new(substitute(*condition, replacements)),
        },
        Expr::Sum {
            name,
            type_name,
            body,
            condition,
        } => Expr::Sum {
            name,
            type_name,
            body: Box::new(substitute(*body, replacements)),
            condition: condition.map(|expr| Box::new(substitute(*expr, replacements))),
        },
        Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
            name,
            expr: Box::new(substitute(*expr, replacements)),
            span,
        },
        Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
            name,
            left: Box::new(substitute(*left, replacements)),
            right: Box::new(substitute(*right, replacements)),
        },
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => Expr::TernaryNamed {
            name,
            first: Box::new(substitute(*first, replacements)),
            second: Box::new(substitute(*second, replacements)),
            third: Box::new(substitute(*third, replacements)),
        },
        Expr::BinderNamed { name, binder } => Expr::BinderNamed {
            name,
            binder: substitute_binder(binder, replacements),
        },
        other => other,
    }
}

/// Substitute free variable references in an expression.
///
/// Refinement uses this to pull abstract properties back through scalar state
/// maps before bounded progress checking.
#[must_use]
pub fn substitute_expr<S: std::hash::BuildHasher>(
    expr: Expr,
    replacements: &HashMap<String, Expr, S>,
) -> Expr {
    substitute(expr, replacements)
}

fn substitute_binder<S: std::hash::BuildHasher>(
    binder: Binder,
    replacements: &HashMap<String, Expr, S>,
) -> Binder {
    match binder {
        Binder::Typed {
            name,
            type_name,
            where_expr,
        } => Binder::Typed {
            name,
            type_name,
            where_expr: where_expr.map(|expr| Box::new(substitute(*expr, replacements))),
        },
        Binder::Range { name, lo, hi } => Binder::Range {
            name,
            lo: Box::new(substitute(*lo, replacements)),
            hi: Box::new(substitute(*hi, replacements)),
        },
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => Binder::Collection {
            name,
            collection: Box::new(substitute(*collection, replacements)),
            where_expr: where_expr.map(|expr| Box::new(substitute(*expr, replacements))),
        },
    }
}
