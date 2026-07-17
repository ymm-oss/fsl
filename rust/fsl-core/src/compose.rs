// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use fsl_syntax::{
    ActionItem, Annotations, Binder, ComposeItem, Expr, LValue, Param, QualifiedName, SourceFile,
    SpecItem, Statement, SurfaceCompose, SurfaceDocument, SurfaceSpec, SyncAction, TypeExpr,
    parse_document,
};

use crate::{CoreError, KernelSpec, PredicateExpander, expand_spec_domains, substitute};

pub trait FileResolver {
    /// Read one source-relative FSL dependency.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError`] when the resource is unavailable.
    fn read(&self, path: &str) -> Result<String, CoreError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FsResolver {
    base: PathBuf,
}

impl FsResolver {
    #[must_use]
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }
}

impl FileResolver for FsResolver {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        std::fs::read_to_string(self.base.join(path)).map_err(|error| CoreError {
            message: error.to_string(),
            line: 1,
            column: 1,
            origin: None,
        })
    }
}

/// Parse a direct spec or resolver-backed compose document into kernel AST.
///
/// # Errors
///
/// Returns [`CoreError`] for parsing, dependency resolution, name rewriting,
/// or unsupported top-level documents.
pub fn parse_kernel_source(
    source: &str,
    resolver: &dyn FileResolver,
) -> Result<KernelSpec, CoreError> {
    let parsed = parse_document(SourceFile::new(source))?;
    let mut kernel = match parsed.surface {
        SurfaceDocument::Spec(spec) => crate::lower_direct_spec(spec),
        SurfaceDocument::Business(business) => crate::lower_business(business),
        SurfaceDocument::Requirements(requirements) => crate::lower_requirements(requirements),
        SurfaceDocument::Governance(governance) => crate::lower_governance(governance),
        SurfaceDocument::Db(system) => crate::lower_db(&system),
        SurfaceDocument::Domain(domain) => crate::lower_domain(&domain),
        SurfaceDocument::AiComponent(component) => crate::lower_ai_component(component),
        SurfaceDocument::Compose(compose) => lower_compose(compose, resolver),
        SurfaceDocument::Refinement(_) | SurfaceDocument::Agent(_) => Err(CoreError {
            message: "top-level document has not reached the kernel lowering gate".to_owned(),
            line: 1,
            column: 1,
            origin: None,
        }),
    }?;
    kernel
        .annotations
        .extend(crate::SPEC_TARGET, parsed.annotations);
    Ok(kernel)
}

/// Parse and lower source while attaching the caller-known root file identity
/// to internal origins and diagnostics.
///
/// # Errors
///
/// Returns [`CoreError`] with the same language contract as
/// [`parse_kernel_source`], enriched with the supplied source identity.
pub fn parse_kernel_source_with_file(
    source: &str,
    resolver: &dyn FileResolver,
    source_file: impl AsRef<str>,
) -> Result<KernelSpec, CoreError> {
    let source_file = source_file.as_ref();
    parse_kernel_source(source, resolver)
        .map(|kernel| kernel.with_source_file(source_file))
        .map_err(|error| error.with_source_file(source_file))
}

#[derive(Clone)]
struct Component {
    alias: String,
    names: ComponentNames,
    spec: SurfaceSpec,
}

#[derive(Clone, Default)]
struct ComponentNames {
    consts: HashSet<String>,
    types: HashSet<String>,
    state: HashSet<String>,
    actions: HashSet<String>,
    properties: HashSet<String>,
}

impl ComponentNames {
    fn collect(spec: &SurfaceSpec) -> Self {
        let mut names = Self::default();
        for item in &spec.items {
            match item {
                SpecItem::Const { name, .. } => {
                    names.consts.insert(name.clone());
                }
                SpecItem::Type { name, .. }
                | SpecItem::Enum { name, .. }
                | SpecItem::Struct { name, .. } => {
                    names.types.insert(name.clone());
                }
                SpecItem::State(fields) => {
                    names
                        .state
                        .extend(fields.iter().map(|field| field.name.clone()));
                }
                SpecItem::Action { name, .. } => {
                    names.actions.insert(name.clone());
                }
                SpecItem::Invariant { name, .. }
                | SpecItem::Trans { name, .. }
                | SpecItem::Reachable { name, .. }
                | SpecItem::Until { name, .. }
                | SpecItem::Unless { name, .. }
                | SpecItem::LeadsTo { name, .. } => {
                    names.properties.insert(name.clone());
                }
                _ => {}
            }
        }
        names
    }
}

fn parse_component(
    source: &str,
    use_span: fsl_syntax::Span,
) -> Result<(SurfaceSpec, Annotations), CoreError> {
    let parsed = parse_document(SourceFile::new(source))?;
    let SurfaceDocument::Spec(spec) = parsed.surface else {
        return Err(error_at("compose use must reference a spec", use_span));
    };
    let spec = PredicateExpander::new(&spec)?.expand(spec)?;
    Ok((expand_spec_domains(spec)?, parsed.annotations))
}

fn composed_kernel(name: String, items: Vec<SpecItem>, annotations: Annotations) -> KernelSpec {
    let mut kernel = KernelSpec {
        spec: SurfaceSpec {
            name,
            meta: None,
            items,
        },
        origins: crate::OriginRegistry::default(),
        annotations: fsl_syntax::AnnotationRegistry::default(),
        projections: Vec::new(),
    };
    kernel.annotations.extend(crate::SPEC_TARGET, annotations);
    kernel
}

/// Lower one compose document using source-relative component resolution.
///
/// # Errors
///
/// Returns [`CoreError`] for missing/invalid components, unknown aliases or
/// actions, sync arity mismatches, and nested compose inputs.
#[allow(clippy::too_many_lines)]
pub fn lower_compose(
    compose: SurfaceCompose,
    resolver: &dyn FileResolver,
) -> Result<KernelSpec, CoreError> {
    let mut components = BTreeMap::new();
    let mut component_annotations = Annotations::default();
    let mut order = Vec::new();
    for item in &compose.items {
        let ComposeItem::Use {
            spec_name: _,
            alias,
            path,
            span,
        } = item
        else {
            continue;
        };
        if components.contains_key(alias) {
            return Err(error_at(format!("duplicate alias '{alias}'"), *span));
        }
        let source = resolver.read(path)?;
        let (spec, annotations) = parse_component(&source, *span)?;
        component_annotations.extend(annotations.source_order().iter().cloned());
        order.push(alias.clone());
        components.insert(
            alias.clone(),
            Component {
                alias: alias.clone(),
                names: ComponentNames::collect(&spec),
                spec,
            },
        );
    }
    let internal = compose
        .items
        .iter()
        .filter_map(|item| match item {
            ComposeItem::Internal { alias, action, .. } => Some((alias.clone(), action.clone())),
            _ => None,
        })
        .collect::<HashSet<_>>();

    let mut static_items = Vec::new();
    let mut init = Vec::new();
    let mut init_meta = None;
    let mut init_annotations = Annotations::default();
    let mut actions = Vec::new();
    for alias in &order {
        let component = &components[alias];
        for item in &component.spec.items {
            match item {
                SpecItem::Init {
                    statements,
                    meta,
                    annotations,
                } => {
                    init.extend(rewrite_statements(
                        statements.clone(),
                        component,
                        &HashSet::new(),
                    ));
                    if init_meta.is_none() {
                        init_meta.clone_from(meta);
                    }
                    init_annotations.extend(annotations.source_order().iter().cloned());
                }
                SpecItem::Action { name, .. } => {
                    if !internal.contains(&(alias.clone(), name.clone())) {
                        actions.push(rewrite_component_item(item.clone(), component));
                    }
                }
                _ => static_items.push(rewrite_component_item(item.clone(), component)),
            }
        }
    }

    for item in &compose.items {
        match item {
            ComposeItem::Common(SpecItem::State(fields)) => {
                static_items.push(SpecItem::State(fields.clone()));
            }
            ComposeItem::Common(SpecItem::Init {
                statements,
                meta,
                annotations,
            }) => {
                init.extend(rewrite_compose_statements(statements.clone(), &components));
                if init_meta.is_none() {
                    init_meta.clone_from(meta);
                }
                init_annotations.extend(annotations.source_order().iter().cloned());
            }
            ComposeItem::Common(item) => {
                static_items.push(rewrite_compose_item(item.clone(), &components)?);
            }
            ComposeItem::SyncAction(action) => {
                actions.push(sync_action(action, &components)?);
            }
            ComposeItem::Use { .. } | ComposeItem::Internal { .. } => {}
        }
    }
    static_items.push(SpecItem::Init {
        statements: init,
        meta: init_meta,
        annotations: init_annotations,
    });
    static_items.extend(actions);
    Ok(composed_kernel(
        compose.name,
        static_items,
        component_annotations,
    ))
}

fn prefix(alias: &str, name: &str) -> String {
    format!("{alias}__{name}")
}

fn error_at(message: impl Into<String>, span: fsl_syntax::Span) -> CoreError {
    CoreError {
        message: message.into(),
        line: span.start.line,
        column: span.start.column,
        origin: None,
    }
}

#[allow(clippy::too_many_lines)]
fn rewrite_component_item(item: SpecItem, component: &Component) -> SpecItem {
    let alias = &component.alias;
    match item {
        SpecItem::Const { name, value } => SpecItem::Const {
            name: prefix(alias, &name),
            value: Box::new(rewrite_expr(*value, component, &HashSet::new())),
        },
        SpecItem::Type {
            name,
            lo,
            hi,
            symmetric,
        } => SpecItem::Type {
            name: prefix(alias, &name),
            lo: Box::new(rewrite_expr(*lo, component, &HashSet::new())),
            hi: Box::new(rewrite_expr(*hi, component, &HashSet::new())),
            symmetric,
        },
        SpecItem::Enum {
            name,
            members,
            symmetric,
        } => SpecItem::Enum {
            name: prefix(alias, &name),
            members,
            symmetric,
        },
        SpecItem::Struct { name, fields } => SpecItem::Struct {
            name: prefix(alias, &name),
            fields: fields
                .into_iter()
                .map(|(name, ty)| (name, rewrite_type(ty, component)))
                .collect(),
        },
        SpecItem::State(fields) => SpecItem::State(
            fields
                .into_iter()
                .map(|field| fsl_syntax::StateField {
                    name: prefix(alias, &field.name),
                    ty: rewrite_type(field.ty, component),
                    initializer: field
                        .initializer
                        .map(|expr| rewrite_expr(expr, component, &HashSet::new())),
                    span: field.span,
                    initializer_span: field.initializer_span,
                })
                .collect(),
        ),
        SpecItem::Init {
            statements,
            meta,
            annotations,
        } => SpecItem::Init {
            statements: rewrite_statements(statements, component, &HashSet::new()),
            meta,
            annotations,
        },
        SpecItem::Action {
            name,
            params,
            items,
            span,
            fair,
            meta,
            sync,
            annotations,
        } => {
            let params = params
                .into_iter()
                .map(|param| rewrite_param(param, component))
                .collect::<Vec<_>>();
            let bound = params
                .iter()
                .map(|param| match param {
                    Param::Typed(name, _) | Param::Range(name, _, _) => name.clone(),
                })
                .collect();
            SpecItem::Action {
                name: prefix(alias, &name),
                params,
                items: rewrite_action_items(items, component, &bound),
                span,
                fair,
                meta,
                sync,
                annotations,
            }
        }
        SpecItem::Invariant {
            name,
            expr,
            span,
            meta,
            annotations,
        } => SpecItem::Invariant {
            name: prefix(alias, &name),
            expr: Box::new(rewrite_expr(*expr, component, &HashSet::new())),
            span,
            meta,
            annotations,
        },
        SpecItem::Trans {
            name,
            expr,
            span,
            meta,
            annotations,
        } => SpecItem::Trans {
            name: prefix(alias, &name),
            expr: Box::new(rewrite_expr(*expr, component, &HashSet::new())),
            span,
            meta,
            annotations,
        },
        SpecItem::Reachable {
            name,
            expr,
            span,
            meta,
            annotations,
        } => SpecItem::Reachable {
            name: prefix(alias, &name),
            expr: Box::new(rewrite_expr(*expr, component, &HashSet::new())),
            span,
            meta,
            annotations,
        },
        SpecItem::Terminal { expr, span } => SpecItem::Terminal {
            expr: Box::new(rewrite_expr(*expr, component, &HashSet::new())),
            span,
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
            annotations,
        } => {
            let mut bound = HashSet::new();
            let binders = binders
                .into_iter()
                .map(|binder| {
                    let binder = rewrite_binder(binder, component, &bound);
                    bound.insert(binder_name(&binder).to_owned());
                    binder
                })
                .collect();
            SpecItem::LeadsTo {
                name: prefix(alias, &name),
                binders,
                before: Box::new(rewrite_expr(*before, component, &bound)),
                after: Box::new(rewrite_expr(*after, component, &bound)),
                span,
                meta,
                decreases: decreases.map(|expr| Box::new(rewrite_expr(*expr, component, &bound))),
                within: within.map(|expr| Box::new(rewrite_expr(*expr, component, &bound))),
                helpful,
                annotations,
            }
        }
        item @ (SpecItem::Until { .. }
        | SpecItem::Unless { .. }
        | SpecItem::VerifyBounds { .. }
        | SpecItem::Entity(..)
        | SpecItem::Number(..)
        | SpecItem::Def { .. }) => item,
    }
}

fn rewrite_type(ty: TypeExpr, component: &Component) -> TypeExpr {
    match ty {
        TypeExpr::Name(name) if component.names.types.contains(&name) => {
            TypeExpr::Name(prefix(&component.alias, &name))
        }
        TypeExpr::Range(lo, hi) => TypeExpr::Range(
            rewrite_expr(lo, component, &HashSet::new()),
            rewrite_expr(hi, component, &HashSet::new()),
        ),
        TypeExpr::Map(key, value) => TypeExpr::Map(
            Box::new(rewrite_type(*key, component)),
            Box::new(rewrite_type(*value, component)),
        ),
        TypeExpr::Relation(source, target) => TypeExpr::Relation(
            Box::new(rewrite_type(*source, component)),
            Box::new(rewrite_type(*target, component)),
        ),
        TypeExpr::Set(inner) => TypeExpr::Set(Box::new(rewrite_type(*inner, component))),
        TypeExpr::Seq(inner, cap) => TypeExpr::Seq(
            Box::new(rewrite_type(*inner, component)),
            rewrite_expr(cap, component, &HashSet::new()),
        ),
        TypeExpr::Option(inner) => TypeExpr::Option(Box::new(rewrite_type(*inner, component))),
        other => other,
    }
}

fn rewrite_param(param: Param, component: &Component) -> Param {
    match param {
        Param::Typed(name, mut qualified) => {
            if qualified.namespace.is_none() && component.names.types.contains(&qualified.name) {
                qualified.name = prefix(&component.alias, &qualified.name);
            }
            Param::Typed(name, qualified)
        }
        Param::Range(name, lo, hi) => Param::Range(
            name,
            rewrite_expr(lo, component, &HashSet::new()),
            rewrite_expr(hi, component, &HashSet::new()),
        ),
    }
}

fn rewrite_action_items(
    items: Vec<ActionItem>,
    component: &Component,
    bound: &HashSet<String>,
) -> Vec<ActionItem> {
    items
        .into_iter()
        .map(|item| match item {
            ActionItem::Requires(expr, span) => {
                ActionItem::Requires(rewrite_expr(expr, component, bound), span)
            }
            ActionItem::Ensures(expr, span) => {
                ActionItem::Ensures(rewrite_expr(expr, component, bound), span)
            }
            ActionItem::Let(name, expr, span) => {
                ActionItem::Let(name, rewrite_expr(expr, component, bound), span)
            }
            ActionItem::Statement(statement) => {
                ActionItem::Statement(rewrite_statement(statement, component, bound))
            }
        })
        .collect()
}

fn rewrite_statements(
    statements: Vec<Statement>,
    component: &Component,
    bound: &HashSet<String>,
) -> Vec<Statement> {
    statements
        .into_iter()
        .map(|statement| rewrite_statement(statement, component, bound))
        .collect()
}

fn rewrite_statement(
    statement: Statement,
    component: &Component,
    bound: &HashSet<String>,
) -> Statement {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => Statement::Assign {
            target: rewrite_lvalue(target, component, bound),
            value: rewrite_expr(value, component, bound),
            span,
        },
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => Statement::If {
            condition: rewrite_expr(condition, component, bound),
            then_statements: rewrite_statements(then_statements, component, bound),
            else_statements: rewrite_statements(else_statements, component, bound),
            span,
        },
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let binder = rewrite_binder(binder, component, bound);
            let mut nested = bound.clone();
            nested.insert(binder_name(&binder).to_owned());
            Statement::ForAll {
                binder,
                statements: rewrite_statements(statements, component, &nested),
                span,
            }
        }
    }
}

fn rewrite_lvalue(lvalue: LValue, component: &Component, bound: &HashSet<String>) -> LValue {
    match lvalue {
        LValue::Var(name) => LValue::Var(rewrite_state_name(name, component)),
        LValue::Index(name, expr) => LValue::Index(
            rewrite_state_name(name, component),
            rewrite_expr(expr, component, bound),
        ),
        LValue::Field(base, field) => {
            LValue::Field(Box::new(rewrite_lvalue(*base, component, bound)), field)
        }
    }
}

fn rewrite_state_name(name: String, component: &Component) -> String {
    if component.names.state.contains(&name) {
        prefix(&component.alias, &name)
    } else {
        name
    }
}

#[allow(clippy::too_many_lines)]
fn rewrite_expr(expr: Expr, component: &Component, bound: &HashSet<String>) -> Expr {
    match expr {
        Expr::Var(name)
            if !bound.contains(&name)
                && (component.names.state.contains(&name)
                    || component.names.consts.contains(&name)) =>
        {
            Expr::Var(prefix(&component.alias, &name))
        }
        Expr::Some(expr) => Expr::Some(Box::new(rewrite_expr(*expr, component, bound))),
        Expr::Set(items) => Expr::Set(
            items
                .into_iter()
                .map(|item| rewrite_expr(item, component, bound))
                .collect(),
        ),
        Expr::Seq(items) => Expr::Seq(
            items
                .into_iter()
                .map(|item| rewrite_expr(item, component, bound))
                .collect(),
        ),
        Expr::Struct { name, fields } => Expr::Struct {
            name: if component.names.types.contains(&name) {
                prefix(&component.alias, &name)
            } else {
                name
            },
            fields: fields
                .into_iter()
                .map(|(name, expr)| (name, rewrite_expr(expr, component, bound)))
                .collect(),
        },
        Expr::Call { name, args, span } => Expr::Call {
            name,
            args: args
                .into_iter()
                .map(|arg| rewrite_expr(arg, component, bound))
                .collect(),
            span,
        },
        Expr::Index(base, index) => Expr::Index(
            Box::new(rewrite_expr(*base, component, bound)),
            Box::new(rewrite_expr(*index, component, bound)),
        ),
        Expr::Field(base, name) => {
            Expr::Field(Box::new(rewrite_expr(*base, component, bound)), name)
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => Expr::Method {
            receiver: Box::new(rewrite_expr(*receiver, component, bound)),
            name,
            args: args
                .into_iter()
                .map(|arg| rewrite_expr(arg, component, bound))
                .collect(),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: Box::new(rewrite_expr(*left, component, bound)),
            right: Box::new(rewrite_expr(*right, component, bound)),
        },
        Expr::Neg(expr) => Expr::Neg(Box::new(rewrite_expr(*expr, component, bound))),
        Expr::Not(expr) => Expr::Not(Box::new(rewrite_expr(*expr, component, bound))),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => Expr::Conditional {
            spans,
            condition: Box::new(rewrite_expr(*condition, component, bound)),
            then_expr: Box::new(rewrite_expr(*then_expr, component, bound)),
            else_expr: Box::new(rewrite_expr(*else_expr, component, bound)),
        },
        Expr::Is { expr, pattern } => Expr::Is {
            expr: Box::new(rewrite_expr(*expr, component, bound)),
            pattern,
        },
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => {
            let binder = rewrite_binder(binder, component, bound);
            let mut nested = bound.clone();
            nested.insert(binder_name(&binder).to_owned());
            Expr::Quantified {
                quantifier,
                binder,
                body: Box::new(rewrite_expr(*body, component, &nested)),
            }
        }
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => {
            let binder = rewrite_binder(binder, component, bound);
            let mut nested = bound.clone();
            nested.insert(binder_name(&binder).to_owned());
            Expr::Aggregate {
                kind,
                binder,
                value: value.map(|expr| Box::new(rewrite_expr(*expr, component, &nested))),
            }
        }
        Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
            name,
            expr: Box::new(rewrite_expr(*expr, component, bound)),
            span,
        },
        Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
            name,
            left: Box::new(rewrite_expr(*left, component, bound)),
            right: Box::new(rewrite_expr(*right, component, bound)),
        },
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => Expr::TernaryNamed {
            name,
            first: Box::new(rewrite_expr(*first, component, bound)),
            second: Box::new(rewrite_expr(*second, component, bound)),
            third: Box::new(rewrite_expr(*third, component, bound)),
        },
        other => other,
    }
}

fn rewrite_binder(binder: Binder, component: &Component, bound: &HashSet<String>) -> Binder {
    match binder {
        Binder::Typed {
            name,
            mut type_name,
            where_expr,
        } => {
            if type_name.namespace.is_none() && component.names.types.contains(&type_name.name) {
                type_name.name = prefix(&component.alias, &type_name.name);
            }
            let mut nested = bound.clone();
            nested.insert(name.clone());
            Binder::Typed {
                name,
                type_name,
                where_expr: where_expr
                    .map(|expr| Box::new(rewrite_expr(*expr, component, &nested))),
            }
        }
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => {
            let mut nested = bound.clone();
            nested.insert(name.clone());
            Binder::Range {
                name,
                lo: Box::new(rewrite_expr(*lo, component, bound)),
                hi: Box::new(rewrite_expr(*hi, component, bound)),
                where_expr: where_expr
                    .map(|expr| Box::new(rewrite_expr(*expr, component, &nested))),
            }
        }
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => {
            let mut nested = bound.clone();
            nested.insert(name.clone());
            Binder::Collection {
                name,
                collection: Box::new(rewrite_expr(*collection, component, bound)),
                where_expr: where_expr
                    .map(|expr| Box::new(rewrite_expr(*expr, component, &nested))),
            }
        }
    }
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}

fn rewrite_compose_item(
    item: SpecItem,
    components: &BTreeMap<String, Component>,
) -> Result<SpecItem, CoreError> {
    Ok(match item {
        SpecItem::Invariant {
            name,
            expr,
            span,
            meta,
            annotations,
        } => SpecItem::Invariant {
            name,
            expr: Box::new(resolve_alias_expr(*expr, components)?),
            span,
            meta,
            annotations,
        },
        SpecItem::Trans {
            name,
            expr,
            span,
            meta,
            annotations,
        } => SpecItem::Trans {
            name,
            expr: Box::new(resolve_alias_expr(*expr, components)?),
            span,
            meta,
            annotations,
        },
        SpecItem::Reachable {
            name,
            expr,
            span,
            meta,
            annotations,
        } => SpecItem::Reachable {
            name,
            expr: Box::new(resolve_alias_expr(*expr, components)?),
            span,
            meta,
            annotations,
        },
        other => other,
    })
}

fn rewrite_compose_statements(
    statements: Vec<Statement>,
    components: &BTreeMap<String, Component>,
) -> Vec<Statement> {
    statements
        .into_iter()
        .map(|statement| resolve_alias_statement(statement, components))
        .collect::<Result<_, _>>()
        .expect("compose alias validation occurs during lowering")
}

fn resolve_alias_statement(
    statement: Statement,
    components: &BTreeMap<String, Component>,
) -> Result<Statement, CoreError> {
    Ok(match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => Statement::Assign {
            target,
            value: resolve_alias_expr(value, components)?,
            span,
        },
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => Statement::If {
            condition: resolve_alias_expr(condition, components)?,
            then_statements: then_statements
                .into_iter()
                .map(|statement| resolve_alias_statement(statement, components))
                .collect::<Result<_, _>>()?,
            else_statements: else_statements
                .into_iter()
                .map(|statement| resolve_alias_statement(statement, components))
                .collect::<Result<_, _>>()?,
            span,
        },
        Statement::ForAll {
            binder,
            statements,
            span,
        } => Statement::ForAll {
            binder: resolve_alias_binder(binder, components)?,
            statements: statements
                .into_iter()
                .map(|statement| resolve_alias_statement(statement, components))
                .collect::<Result<_, _>>()?,
            span,
        },
    })
}

fn sync_action(
    action: &SyncAction,
    components: &BTreeMap<String, Component>,
) -> Result<SpecItem, CoreError> {
    let params = action
        .params
        .iter()
        .cloned()
        .map(|param| resolve_alias_param(param, components))
        .collect::<Result<Vec<_>, _>>()?;
    let mut items = Vec::new();
    for reference in &action.refs {
        let component = components.get(&reference.alias).ok_or_else(|| CoreError {
            message: format!("unknown alias '{}'", reference.alias),
            line: action.span.start.line,
            column: action.span.start.column,
            origin: None,
        })?;
        let source = component
            .spec
            .items
            .iter()
            .find(|item| matches!(item, SpecItem::Action { name, .. } if name == &reference.action))
            .cloned()
            .ok_or_else(|| CoreError {
                message: format!("unknown action '{}.{}'", reference.alias, reference.action),
                line: action.span.start.line,
                column: action.span.start.column,
                origin: None,
            })?;
        let SpecItem::Action {
            params: source_params,
            items: source_items,
            ..
        } = rewrite_component_item(source, component)
        else {
            unreachable!()
        };
        if source_params.len() != reference.args.len() {
            return Err(error_at(
                format!(
                    "sync reference '{}.{}' expects {} argument(s), got {}",
                    reference.alias,
                    reference.action,
                    source_params.len(),
                    reference.args.len()
                ),
                action.span,
            ));
        }
        let replacements = source_params
            .iter()
            .zip(&reference.args)
            .map(|(param, arg)| {
                let name = match param {
                    Param::Typed(name, _) | Param::Range(name, _, _) => name.clone(),
                };
                Ok((name, resolve_alias_expr(arg.clone(), components)?))
            })
            .collect::<Result<HashMap<_, _>, CoreError>>()?;
        items.extend(
            source_items
                .into_iter()
                .map(|item| substitute_action_item(item, &replacements)),
        );
    }
    items.extend(
        action
            .items
            .iter()
            .cloned()
            .map(|item| resolve_alias_action_item(item, components))
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(SpecItem::Action {
        name: action.name.clone(),
        params,
        items,
        span: action.span,
        fair: action.fair,
        meta: action.meta.clone(),
        sync: true,
        annotations: action.annotations.clone(),
    })
}

fn resolve_alias_param(
    param: Param,
    components: &BTreeMap<String, Component>,
) -> Result<Param, CoreError> {
    Ok(match param {
        Param::Typed(name, qualified) => {
            Param::Typed(name, resolve_alias_qualified_name(qualified, components)?)
        }
        Param::Range(name, lo, hi) => Param::Range(
            name,
            resolve_alias_expr(lo, components)?,
            resolve_alias_expr(hi, components)?,
        ),
    })
}

fn resolve_alias_qualified_name(
    qualified: QualifiedName,
    components: &BTreeMap<String, Component>,
) -> Result<QualifiedName, CoreError> {
    if let Some(alias) = qualified.namespace {
        if !components.contains_key(&alias) {
            return Err(CoreError {
                message: format!("unknown alias '{alias}'"),
                line: 1,
                column: 1,
                origin: None,
            });
        }
        Ok(QualifiedName {
            namespace: None,
            name: prefix(&alias, &qualified.name),
        })
    } else {
        Ok(qualified)
    }
}

fn resolve_alias_binder(
    binder: Binder,
    components: &BTreeMap<String, Component>,
) -> Result<Binder, CoreError> {
    Ok(match binder {
        Binder::Typed {
            name,
            type_name,
            where_expr,
        } => Binder::Typed {
            name,
            type_name: resolve_alias_qualified_name(type_name, components)?,
            where_expr: where_expr
                .map(|expr| resolve_alias_expr(*expr, components).map(Box::new))
                .transpose()?,
        },
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => Binder::Range {
            name,
            lo: Box::new(resolve_alias_expr(*lo, components)?),
            hi: Box::new(resolve_alias_expr(*hi, components)?),
            where_expr: where_expr
                .map(|expr| resolve_alias_expr(*expr, components).map(Box::new))
                .transpose()?,
        },
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => Binder::Collection {
            name,
            collection: Box::new(resolve_alias_expr(*collection, components)?),
            where_expr: where_expr
                .map(|expr| resolve_alias_expr(*expr, components).map(Box::new))
                .transpose()?,
        },
    })
}

#[allow(clippy::too_many_lines)]
fn resolve_alias_expr(
    expr: Expr,
    components: &BTreeMap<String, Component>,
) -> Result<Expr, CoreError> {
    Ok(match expr {
        Expr::Field(base, name) => {
            if let Expr::Var(alias) = base.as_ref() {
                if components.contains_key(alias) {
                    Expr::Var(prefix(alias, &name))
                } else {
                    Expr::Field(Box::new(resolve_alias_expr(*base, components)?), name)
                }
            } else {
                Expr::Field(Box::new(resolve_alias_expr(*base, components)?), name)
            }
        }
        Expr::Some(expr) => Expr::Some(Box::new(resolve_alias_expr(*expr, components)?)),
        Expr::Set(items) => Expr::Set(
            items
                .into_iter()
                .map(|item| resolve_alias_expr(item, components))
                .collect::<Result<_, _>>()?,
        ),
        Expr::Seq(items) => Expr::Seq(
            items
                .into_iter()
                .map(|item| resolve_alias_expr(item, components))
                .collect::<Result<_, _>>()?,
        ),
        Expr::Struct { name, fields } => Expr::Struct {
            name,
            fields: fields
                .into_iter()
                .map(|(name, expr)| Ok((name, resolve_alias_expr(expr, components)?)))
                .collect::<Result<_, CoreError>>()?,
        },
        Expr::Call { name, args, span } => Expr::Call {
            name,
            args: args
                .into_iter()
                .map(|arg| resolve_alias_expr(arg, components))
                .collect::<Result<_, _>>()?,
            span,
        },
        Expr::Index(base, index) => Expr::Index(
            Box::new(resolve_alias_expr(*base, components)?),
            Box::new(resolve_alias_expr(*index, components)?),
        ),
        Expr::Method {
            receiver,
            name,
            args,
        } => Expr::Method {
            receiver: Box::new(resolve_alias_expr(*receiver, components)?),
            name,
            args: args
                .into_iter()
                .map(|arg| resolve_alias_expr(arg, components))
                .collect::<Result<_, _>>()?,
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: Box::new(resolve_alias_expr(*left, components)?),
            right: Box::new(resolve_alias_expr(*right, components)?),
        },
        Expr::Neg(expr) => Expr::Neg(Box::new(resolve_alias_expr(*expr, components)?)),
        Expr::Not(expr) => Expr::Not(Box::new(resolve_alias_expr(*expr, components)?)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => Expr::Conditional {
            spans,
            condition: Box::new(resolve_alias_expr(*condition, components)?),
            then_expr: Box::new(resolve_alias_expr(*then_expr, components)?),
            else_expr: Box::new(resolve_alias_expr(*else_expr, components)?),
        },
        Expr::Is { expr, pattern } => Expr::Is {
            expr: Box::new(resolve_alias_expr(*expr, components)?),
            pattern,
        },
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => Expr::Quantified {
            quantifier,
            binder: resolve_alias_binder(binder, components)?,
            body: Box::new(resolve_alias_expr(*body, components)?),
        },
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => Expr::Aggregate {
            kind,
            binder: resolve_alias_binder(binder, components)?,
            value: value
                .map(|expr| resolve_alias_expr(*expr, components).map(Box::new))
                .transpose()?,
        },
        Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
            name,
            expr: Box::new(resolve_alias_expr(*expr, components)?),
            span,
        },
        Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
            name,
            left: Box::new(resolve_alias_expr(*left, components)?),
            right: Box::new(resolve_alias_expr(*right, components)?),
        },
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => Expr::TernaryNamed {
            name,
            first: Box::new(resolve_alias_expr(*first, components)?),
            second: Box::new(resolve_alias_expr(*second, components)?),
            third: Box::new(resolve_alias_expr(*third, components)?),
        },
        other => other,
    })
}

fn resolve_alias_action_item(
    item: ActionItem,
    components: &BTreeMap<String, Component>,
) -> Result<ActionItem, CoreError> {
    Ok(match item {
        ActionItem::Requires(expr, span) => {
            ActionItem::Requires(resolve_alias_expr(expr, components)?, span)
        }
        ActionItem::Ensures(expr, span) => {
            ActionItem::Ensures(resolve_alias_expr(expr, components)?, span)
        }
        ActionItem::Let(name, expr, span) => {
            ActionItem::Let(name, resolve_alias_expr(expr, components)?, span)
        }
        ActionItem::Statement(statement) => {
            ActionItem::Statement(resolve_alias_statement(statement, components)?)
        }
    })
}

fn substitute_action_item(item: ActionItem, replacements: &HashMap<String, Expr>) -> ActionItem {
    match item {
        ActionItem::Requires(expr, span) => {
            ActionItem::Requires(substitute(expr, replacements), span)
        }
        ActionItem::Ensures(expr, span) => {
            ActionItem::Ensures(substitute(expr, replacements), span)
        }
        ActionItem::Let(name, expr, span) => {
            ActionItem::Let(name, substitute(expr, replacements), span)
        }
        ActionItem::Statement(statement) => {
            ActionItem::Statement(substitute_statement(statement, replacements))
        }
    }
}

fn substitute_statement(statement: Statement, replacements: &HashMap<String, Expr>) -> Statement {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => Statement::Assign {
            target: substitute_lvalue(target, replacements),
            value: substitute(value, replacements),
            span,
        },
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => Statement::If {
            condition: substitute(condition, replacements),
            then_statements: then_statements
                .into_iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
            else_statements: else_statements
                .into_iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
            span,
        },
        Statement::ForAll {
            binder,
            statements,
            span,
        } => Statement::ForAll {
            binder,
            statements: statements
                .into_iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
            span,
        },
    }
}

fn substitute_lvalue(lvalue: LValue, replacements: &HashMap<String, Expr>) -> LValue {
    match lvalue {
        LValue::Index(name, expr) => LValue::Index(name, substitute(expr, replacements)),
        LValue::Field(base, field) => {
            LValue::Field(Box::new(substitute_lvalue(*base, replacements)), field)
        }
        lvalue @ LValue::Var(_) => lvalue,
    }
}
