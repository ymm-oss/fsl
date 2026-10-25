// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use fsl_syntax::{
    ActionItem, Annotations, Binder, ComposeItem, Expr, LValue, Param, ParseOptions, QualifiedName,
    SourceFile, SpecItem, Statement, SurfaceCompose, SurfaceDocument, SurfaceSpec, SyncAction,
    TypeExpr, parse_document_with,
};
use serde_json::{Value, json};

use crate::naming::NameMap;
use crate::{
    CoreError, CoreErrorKind, IndexedReplacements, KernelSpec, PredicateExpander,
    capture_avoiding_binding_with_names, collect_binder_names, collect_names, expand_spec_domains,
    free_vars, fresh_binding_name, substitute, substitute_binder,
};

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
        std::fs::read_to_string(self.base.join(path))
            .map_err(|error| CoreError::unlocated(error.to_string()))
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
    parse_kernel_source_with_options(source, resolver, &ParseOptions::stable())
}

/// [`parse_kernel_source`] under explicit [`ParseOptions`], which also read
/// every compose component.
///
/// # Errors
///
/// As [`parse_kernel_source`].
pub fn parse_kernel_source_with_options(
    source: &str,
    resolver: &dyn FileResolver,
    options: &ParseOptions,
) -> Result<KernelSpec, CoreError> {
    let parsed = parse_document_with(SourceFile::new(source), options)?;
    crate::reject_declared_separator(&parsed.surface, source)?;
    let mut kernel = match parsed.surface {
        SurfaceDocument::Spec(spec) => crate::lower_direct_spec(spec),
        SurfaceDocument::Business(business) => crate::lower_business(business),
        SurfaceDocument::Requirements(requirements) => crate::lower_requirements(requirements),
        SurfaceDocument::Governance(governance) => crate::lower_governance(governance),
        SurfaceDocument::Db(system) => crate::lower_db(&system),
        SurfaceDocument::Domain(domain) => crate::lower_domain(&domain),
        SurfaceDocument::AiComponent(component) => crate::lower_ai_component(component),
        SurfaceDocument::Compose(compose) => lower_compose_with_options(compose, resolver, options),
        SurfaceDocument::Refinement(_) | SurfaceDocument::Agent(_) => Err(CoreError::unlocated(
            "top-level document has not reached the kernel lowering gate",
        )
        .with_kind(CoreErrorKind::NotKernelLowerable)),
    }?;
    kernel
        .annotations
        .extend(crate::SPEC_TARGET, parsed.annotations);
    // The lexer's warnings (`fullwidth_space`, `non_nfc_identifier`) are
    // about the document as written, ahead of every lowering warning. They
    // only exist under the next edition's identifier rule.
    let lexical = fsl_syntax::lexical_warnings(source, options)
        .into_iter()
        .map(|warning| {
            json!({
                "kind": warning.code,
                "code": warning.code,
                "severity": "warning",
                "message": warning.message,
                "loc": warning.span.python_loc(),
                "replacement": warning.replacement,
            })
        })
        .collect::<Vec<_>>();
    if !lexical.is_empty() {
        kernel.diagnostics.splice(0..0, lexical);
    }
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
    parse_kernel_source_with_file_with_options(
        source,
        resolver,
        source_file,
        &ParseOptions::stable(),
    )
}

/// [`parse_kernel_source_with_file`] under explicit [`ParseOptions`].
///
/// # Errors
///
/// As [`parse_kernel_source_with_file`].
pub fn parse_kernel_source_with_file_with_options(
    source: &str,
    resolver: &dyn FileResolver,
    source_file: impl AsRef<str>,
    options: &ParseOptions,
) -> Result<KernelSpec, CoreError> {
    let source_file = source_file.as_ref();
    parse_kernel_source_with_options(source, resolver, options)
        .map(|kernel| kernel.with_source_file(source_file))
        .map_err(|error| error.with_source_file(source_file))
}

#[derive(Clone)]
struct Component {
    alias: String,
    path: String,
    use_span: fsl_syntax::Span,
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

/// Re-anchor a failure that happened *inside* a component onto the parent's
/// `use ... from` declaration (issue #567).
///
/// `loc` is `{line, column}` with no `file` (`docs/design/DESIGN-v1.md`), so it can
/// only ever mean "a position in the file the envelope names". A component's
/// line and column reported against the parent's path therefore points at
/// whatever happens to sit there — for the compose error gallery fixture, a
/// comment. The position now belongs to the `use` declaration, which really is
/// in the parent and really is the line to look at, and the component's own
/// path and position move into the message so nothing is lost.
pub(crate) fn component_error(
    path: &str,
    use_span: fsl_syntax::Span,
    error: &CoreError,
    verb: &str,
) -> CoreError {
    CoreError {
        message: if error.is_located() {
            format!(
                "component \"{path}\" {verb} ({} at {path}:{}:{})",
                error.message, error.line, error.column
            )
        } else {
            format!("component \"{path}\" {verb} ({})", error.message)
        },
        line: use_span.start.line,
        column: use_span.start.column,
        // Not adopted from `error`. What this value reports is a compose-level
        // failure to load a component, anchored at the parent's `use`
        // declaration — it is not itself a name-resolution failure, and
        // `docs/design/DESIGN-v1.md` §8's repair branch for `name` ("add a declaration
        // or change a type") would be wrong advice at that location. The
        // component's own classification is kept as the cause, and its message,
        // path and position survive inside this message (issue 565's flag,
        // issue 567's anchoring, issue 1167's typed kind).
        kind: CoreErrorKind::ComponentLoad(Box::new(error.kind.clone())),
        diagnostic_code: None,
        // The origin's span is the `use` declaration, which really is in the
        // parent, so `with_source_file` stamps the parent path onto a position
        // that exists there. Carrying the component's span here is what let the
        // rendered message pair the parent's path with the component's line.
        origin: Some(Box::new(component_use_origin(path, use_span))),
    }
}

fn component_use_origin(path: &str, use_span: fsl_syntax::Span) -> crate::OriginChain {
    crate::OriginChain {
        id: crate::OriginId(format!(
            "compose:use:{}:{}",
            use_span.start.offset, use_span.end.offset
        )),
        dialect: "compose".to_owned(),
        primary: Some(crate::OriginSite {
            source_file: None,
            span: Some(use_span),
            dialect: "compose".to_owned(),
            declaration_path: vec!["use".to_owned(), path.to_owned()],
        }),
        secondary: Vec::new(),
        lowering_steps: Vec::new(),
        generated: false,
    }
}

fn component_origin(component: &Component, declaration: &str) -> crate::OriginChain {
    let mut origin = component_use_origin(&component.path, component.use_span);
    origin
        .primary
        .as_mut()
        .expect("use site")
        .declaration_path
        .push(format!("{}.{}", component.alias, declaration));
    origin
}

fn component_item_origin(
    component: &Component,
    action: &str,
    item: &ActionItem,
) -> crate::OriginChain {
    let span = match item {
        ActionItem::Requires(_, span)
        | ActionItem::Ensures(_, span)
        | ActionItem::Let(_, _, span)
        | ActionItem::Statement(
            Statement::Assign { span, .. }
            | Statement::If { span, .. }
            | Statement::ForAll { span, .. },
        ) => *span,
    };
    let mut origin = component_origin(component, action);
    origin.secondary.push(crate::OriginSite {
        source_file: Some(component.path.clone()),
        span: Some(span),
        dialect: "kernel".to_owned(),
        declaration_path: vec![action.to_owned()],
    });
    origin
}

fn record_sync_item_origins(
    origins: &mut BTreeMap<String, crate::OriginChain>,
    name: &str,
    items: &[ActionItem],
    item_origins: Vec<crate::OriginChain>,
) {
    let (mut guards, mut statements, mut ensures) = (0, 0, 0);
    for (item, origin) in items.iter().zip(item_origins) {
        let target = match item {
            ActionItem::Requires(..) | ActionItem::Let(..) => {
                let target = crate::action_guard_target(name, guards);
                guards += 1;
                target
            }
            ActionItem::Statement(..) => {
                let target = crate::action_statement_target(name, statements);
                statements += 1;
                target
            }
            ActionItem::Ensures(..) => {
                let target = crate::origin::action_ensure_target(name, ensures);
                ensures += 1;
                target
            }
        };
        origins.insert(target, origin);
    }
}

fn record_component_origins(
    origins: &mut BTreeMap<String, crate::OriginChain>,
    component: &Component,
    item: &SpecItem,
) {
    let (name, targets) = match item {
        SpecItem::Action { name, .. } => (
            name.as_str(),
            vec![crate::action_target(&prefix(&component.alias, name))],
        ),
        SpecItem::Invariant { name, .. } => (
            name.as_str(),
            vec![crate::property_target(
                "invariant",
                &prefix(&component.alias, name),
            )],
        ),
        SpecItem::Trans { name, .. } | SpecItem::Unless { name, .. } => (
            name.as_str(),
            vec![crate::property_target(
                "trans",
                &prefix(&component.alias, name),
            )],
        ),
        SpecItem::Reachable { name, .. } => (
            name.as_str(),
            vec![crate::property_target(
                "reachable",
                &prefix(&component.alias, name),
            )],
        ),
        SpecItem::LeadsTo { name, .. } => (
            name.as_str(),
            vec![crate::property_target(
                "leadsTo",
                &prefix(&component.alias, name),
            )],
        ),
        SpecItem::Until { name, .. } => (
            name.as_str(),
            vec![
                crate::property_target("leadsTo", &prefix(&component.alias, name)),
                crate::property_target(
                    "trans",
                    &format!("{}_until_safety", prefix(&component.alias, name)),
                ),
            ],
        ),
        SpecItem::Terminal { .. } => ("terminal", vec![crate::TERMINAL_TARGET.to_owned()]),
        _ => return,
    };
    for target in targets {
        origins.insert(target, component_origin(component, name));
    }
}

fn parse_component(
    source: &str,
    path: &str,
    use_span: fsl_syntax::Span,
    options: ParseOptions,
) -> Result<(SurfaceSpec, Annotations), CoreError> {
    let parsed = parse_document_with(SourceFile::new(source), &options).map_err(|error| {
        component_error(path, use_span, &CoreError::from(error), "failed to parse")
    })?;
    crate::reject_declared_separator(&parsed.surface, source)
        .map_err(|error| component_error(path, use_span, &error, "failed to lower"))?;
    let SurfaceDocument::Spec(spec) = parsed.surface else {
        return Err(error_at("compose use must reference a spec", use_span));
    };
    let spec = PredicateExpander::new(&spec)
        .and_then(|expander| expander.expand(spec))
        .map_err(|error| component_error(path, use_span, &error, "failed to lower"))?;
    let spec = expand_spec_domains(spec)
        .map_err(|error| component_error(path, use_span, &error, "failed to lower"))?;
    Ok((spec, parsed.annotations))
}

fn composed_kernel(
    name: String,
    items: Vec<SpecItem>,
    annotations: Annotations,
    diagnostics: Vec<Value>,
) -> KernelSpec {
    let mut kernel = KernelSpec {
        spec: SurfaceSpec {
            name,
            meta: None,
            items,
        },
        origins: crate::OriginRegistry::default(),
        annotations: fsl_syntax::AnnotationRegistry::default(),
        projections: Vec::new(),
        diagnostics,
        names: NameMap::default(),
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
pub fn lower_compose(
    compose: SurfaceCompose,
    resolver: &dyn FileResolver,
) -> Result<KernelSpec, CoreError> {
    lower_compose_with_options(compose, resolver, &ParseOptions::stable())
}

/// [`lower_compose`] reading its components under explicit [`ParseOptions`].
///
/// # Errors
///
/// As [`lower_compose`].
#[allow(clippy::too_many_lines)]
pub fn lower_compose_with_options(
    compose: SurfaceCompose,
    resolver: &dyn FileResolver,
    options: &ParseOptions,
) -> Result<KernelSpec, CoreError> {
    let compose = expand_compose_predicates(compose)?;
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
        let source = resolver
            .read(path)
            .map_err(|error| component_error(path, *span, &error, "could not be read"))?;
        let (spec, annotations) = parse_component(&source, path, *span, *options)?;
        component_annotations.extend(annotations.source_order().iter().cloned());
        order.push(alias.clone());
        components.insert(
            alias.clone(),
            Component {
                alias: alias.clone(),
                path: path.clone(),
                use_span: *span,
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

    let mut warnings = Vec::new();
    let mut static_items = Vec::new();
    let mut init = Vec::new();
    let mut init_meta = None;
    let mut init_annotations = Annotations::default();
    let mut actions = Vec::new();
    let mut names = NameMap::default();
    let mut origins = BTreeMap::new();
    let mut init_origins = Vec::new();
    for alias in &order {
        let component = &components[alias];
        for state in &component.names.state {
            names.insert_state(alias, state);
        }
        for property in &component.names.properties {
            names.insert_property(alias, property);
        }
        for item in &component.spec.items {
            record_component_origins(&mut origins, component, item);
            match item {
                SpecItem::Init {
                    statements,
                    meta,
                    annotations,
                } => {
                    init_origins.extend(std::iter::repeat_n(
                        Some(component_origin(component, "init")),
                        statements.len(),
                    ));
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
                        names.insert_action(alias, name);
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
                init_origins.extend(std::iter::repeat_n(None, statements.len()));
                init.extend(rewrite_compose_statements(
                    statements.clone(),
                    &components,
                    false,
                )?);
                if init_meta.is_none() {
                    init_meta.clone_from(meta);
                }
                init_annotations.extend(annotations.source_order().iter().cloned());
            }
            ComposeItem::Common(item) => {
                if matches!(item, SpecItem::Terminal { .. }) {
                    origins.remove(crate::TERMINAL_TARGET);
                }
                static_items.push(rewrite_compose_item(item.clone(), &components)?);
            }
            ComposeItem::SyncAction(action) => {
                actions.push(sync_action(
                    action,
                    &components,
                    &mut warnings,
                    &mut origins,
                )?);
            }
            ComposeItem::Use { .. } | ComposeItem::Internal { .. } => {}
        }
    }
    let inline_count = static_items
        .iter()
        .filter_map(|item| match item {
            SpecItem::State(fields) => Some(
                fields
                    .iter()
                    .filter(|field| field.initializer.is_some())
                    .count(),
            ),
            _ => None,
        })
        .sum::<usize>();
    for (index, origin) in init_origins.into_iter().enumerate() {
        if let Some(origin) = origin {
            origins.insert(crate::init_statement_target(inline_count + index), origin);
        }
    }
    static_items.push(SpecItem::Init {
        statements: init,
        meta: init_meta,
        annotations: init_annotations,
    });
    static_items.extend(actions);
    let mut kernel = composed_kernel(compose.name, static_items, component_annotations, warnings);
    kernel.names = names;
    for (target, origin) in origins {
        kernel
            .origins
            .bind(crate::origin::component_check_target(&target), origin);
    }
    Ok(kernel)
}

/// Expand source-local defs before aliases or sync substitution can change scope.
fn expand_compose_predicates(mut compose: SurfaceCompose) -> Result<SurfaceCompose, CoreError> {
    let definitions = SurfaceSpec {
        name: compose.name.clone(),
        meta: None,
        items: compose
            .items
            .iter()
            .filter_map(|item| match item {
                ComposeItem::Common(item @ SpecItem::Def { .. }) => Some(item.clone()),
                _ => None,
            })
            .collect(),
    };
    let expander = PredicateExpander::new(&definitions)?;
    let mut items = Vec::new();
    for item in compose.items {
        items.push(match item {
            ComposeItem::Common(SpecItem::Def { .. }) => continue,
            ComposeItem::Common(item) => ComposeItem::Common(expander.expand_item(item)?),
            ComposeItem::SyncAction(mut action) => {
                action.params = action
                    .params
                    .into_iter()
                    .map(|param| expander.expand_param(param))
                    .collect::<Result<_, _>>()?;
                for reference in &mut action.refs {
                    reference.args = std::mem::take(&mut reference.args)
                        .into_iter()
                        .map(|arg| expander.expand_expr(arg, &mut Vec::new()))
                        .collect::<Result<_, _>>()?;
                }
                action.items = action
                    .items
                    .into_iter()
                    .map(|item| expander.expand_action_item(item))
                    .collect::<Result<_, _>>()?;
                ComposeItem::SyncAction(action)
            }
            item @ (ComposeItem::Use { .. } | ComposeItem::Internal { .. }) => item,
        });
    }
    compose.items = items;
    Ok(compose)
}

fn prefix(alias: &str, name: &str) -> String {
    crate::naming::compose_internal(alias, name)
}

fn error_at(message: impl Into<String>, span: fsl_syntax::Span) -> CoreError {
    CoreError {
        message: message.into(),
        line: span.start.line,
        column: span.start.column,
        origin: Some(Box::new(crate::OriginChain {
            id: crate::OriginId(format!(
                "compose:error:{}:{}",
                span.start.offset, span.end.offset
            )),
            dialect: "compose".to_owned(),
            primary: Some(crate::OriginSite {
                source_file: None,
                span: Some(span),
                dialect: "compose".to_owned(),
                declaration_path: Vec::new(),
            }),
            secondary: Vec::new(),
            lowering_steps: Vec::new(),
            generated: false,
        })),
        kind: CoreErrorKind::Semantics,
        diagnostic_code: None,
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
            member_spans,
            symmetric,
        } => SpecItem::Enum {
            name: prefix(alias, &name),
            members,
            member_spans,
            symmetric,
        },
        SpecItem::Struct { name, fields, span } => SpecItem::Struct {
            name: prefix(alias, &name),
            span,
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
                helpful: helpful
                    .into_iter()
                    .map(|mut helper| {
                        helper.action = prefix(alias, &helper.action);
                        helper.args = helper
                            .args
                            .into_iter()
                            .map(|arg| rewrite_expr(arg, component, &bound))
                            .collect();
                        helper
                    })
                    .collect(),
                annotations,
            }
        }
        // A component's `until` / `unless` is a property like the others: its
        // name gets the alias prefix and its operands read the component's
        // own (prefixed) state. Passing it through unchanged left `held` to
        // fail typing, or, when the compose declares a same-named state, to
        // silently check the compose's variable instead (#1197).
        SpecItem::Until {
            name,
            before,
            after,
            span,
            meta,
            annotations,
        } => SpecItem::Until {
            name: prefix(alias, &name),
            before: Box::new(rewrite_expr(*before, component, &HashSet::new())),
            after: Box::new(rewrite_expr(*after, component, &HashSet::new())),
            span,
            meta,
            annotations,
        },
        SpecItem::Unless {
            name,
            before,
            after,
            span,
            meta,
            annotations,
        } => SpecItem::Unless {
            name: prefix(alias, &name),
            before: Box::new(rewrite_expr(*before, component, &HashSet::new())),
            after: Box::new(rewrite_expr(*after, component, &HashSet::new())),
            span,
            meta,
            annotations,
        },
        item @ (SpecItem::VerifyBounds { .. }
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
            expr: Box::new(resolve_alias_expr(*expr, components, span)?),
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
            expr: Box::new(resolve_alias_expr(*expr, components, span)?),
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
            expr: Box::new(resolve_alias_expr(*expr, components, span)?),
            span,
            meta,
            annotations,
        },
        // Compose-level temporal properties read component state through
        // `alias.var` exactly like invariants do; leaving them unresolved
        // made `a.on` fail typing with exit 2 (#1297).
        SpecItem::Until {
            name,
            before,
            after,
            span,
            meta,
            annotations,
        } => SpecItem::Until {
            name,
            before: Box::new(resolve_alias_expr(*before, components, span)?),
            after: Box::new(resolve_alias_expr(*after, components, span)?),
            span,
            meta,
            annotations,
        },
        SpecItem::Unless {
            name,
            before,
            after,
            span,
            meta,
            annotations,
        } => SpecItem::Unless {
            name,
            before: Box::new(resolve_alias_expr(*before, components, span)?),
            after: Box::new(resolve_alias_expr(*after, components, span)?),
            span,
            meta,
            annotations,
        },
        item @ SpecItem::LeadsTo { .. } => resolve_alias_leads_to(item, components)?,
        item @ SpecItem::Action { .. } => resolve_alias_action(item, components)?,
        other => other,
    })
}

/// Resolve `alias.var` throughout a compose-level `leadsTo` (#1297).
fn resolve_alias_leads_to(
    item: SpecItem,
    components: &BTreeMap<String, Component>,
) -> Result<SpecItem, CoreError> {
    let SpecItem::LeadsTo {
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
    } = item
    else {
        return Ok(item);
    };
    let resolve = |expr: Box<Expr>| resolve_alias_expr(*expr, components, span).map(Box::new);
    Ok(SpecItem::LeadsTo {
        name,
        binders: binders
            .into_iter()
            .map(|binder| resolve_alias_binder(binder, components, span))
            .collect::<Result<_, _>>()?,
        before: resolve(before)?,
        after: resolve(after)?,
        span,
        meta,
        decreases: decreases.map(resolve).transpose()?,
        within: within.map(resolve).transpose()?,
        helpful: helpful
            .into_iter()
            .map(|mut helper| {
                helper.args = helper
                    .args
                    .into_iter()
                    .map(|arg| resolve_alias_expr(arg, components, span))
                    .collect::<Result<_, _>>()?;
                Ok(helper)
            })
            .collect::<Result<_, CoreError>>()?,
        annotations,
    })
}

fn resolve_alias_action(
    item: SpecItem,
    components: &BTreeMap<String, Component>,
) -> Result<SpecItem, CoreError> {
    let SpecItem::Action {
        name,
        params,
        items,
        span,
        fair,
        meta,
        sync,
        annotations,
    } = item
    else {
        unreachable!("only glue actions are routed here")
    };
    Ok(SpecItem::Action {
        name,
        params: params
            .into_iter()
            .map(|param| resolve_alias_param(param, components, span))
            .collect::<Result<_, _>>()?,
        items: items
            .into_iter()
            .map(|item| resolve_alias_action_item(item, components, true))
            .collect::<Result<_, _>>()?,
        span,
        fair,
        meta,
        sync,
        annotations,
    })
}

fn rewrite_compose_statements(
    statements: Vec<Statement>,
    components: &BTreeMap<String, Component>,
    allow_component_writes: bool,
) -> Result<Vec<Statement>, CoreError> {
    statements
        .into_iter()
        .map(|statement| resolve_alias_statement(statement, components, allow_component_writes))
        .collect()
}

fn resolve_alias_statement(
    statement: Statement,
    components: &BTreeMap<String, Component>,
    allow_component_writes: bool,
) -> Result<Statement, CoreError> {
    Ok(match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => Statement::Assign {
            target: resolve_alias_lvalue(target, components, span, allow_component_writes)?,
            value: resolve_alias_expr(value, components, span)?,
            span,
        },
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => Statement::If {
            condition: resolve_alias_expr(condition, components, span)?,
            then_statements: rewrite_compose_statements(
                then_statements,
                components,
                allow_component_writes,
            )?,
            else_statements: rewrite_compose_statements(
                else_statements,
                components,
                allow_component_writes,
            )?,
            span,
        },
        Statement::ForAll {
            binder,
            statements,
            span,
        } => Statement::ForAll {
            binder: resolve_alias_binder(binder, components, span)?,
            statements: rewrite_compose_statements(statements, components, allow_component_writes)?,
            span,
        },
    })
}

/// All compose-side assignment paths share alias/index normalization and write policy.
fn resolve_alias_lvalue(
    target: LValue,
    components: &BTreeMap<String, Component>,
    span: fsl_syntax::Span,
    allow_component_writes: bool,
) -> Result<LValue, CoreError> {
    let target = match target {
        LValue::Var(name) => LValue::Var(name),
        LValue::Index(name, index) => {
            LValue::Index(name, resolve_alias_expr(index, components, span)?)
        }
        LValue::Field(base, name) => match *base {
            LValue::Var(alias) if components.contains_key(&alias) => {
                LValue::Var(prefix(&alias, &name))
            }
            base => LValue::Field(
                Box::new(resolve_alias_lvalue(
                    base,
                    components,
                    span,
                    allow_component_writes,
                )?),
                name,
            ),
        },
    };
    let mut base = &target;
    while let LValue::Field(parent, _) = base {
        base = parent;
    }
    let (LValue::Var(name) | LValue::Index(name, _)) = base else {
        unreachable!("field bases were unwrapped")
    };
    if !allow_component_writes
        && components.values().any(|component| {
            component
                .names
                .state
                .iter()
                .any(|state| prefix(&component.alias, state) == *name)
        })
    {
        return Err(error_at(
            "only glue actions may write component state; sync additions and compose init may only write composition-side state",
            span,
        ));
    }
    Ok(target)
}

fn sync_action(
    action: &SyncAction,
    components: &BTreeMap<String, Component>,
    warnings: &mut Vec<Value>,
    origins: &mut BTreeMap<String, crate::OriginChain>,
) -> Result<SpecItem, CoreError> {
    let params = action
        .params
        .iter()
        .cloned()
        .map(|param| resolve_alias_param(param, components, action.span))
        .collect::<Result<Vec<_>, _>>()?;
    let mut items = Vec::new();
    let mut item_origins = Vec::new();
    let mut fair_constituents = Vec::new();
    let mut avoid = sync_avoid_names(action, &params, components)?;
    for reference in &action.refs {
        let component = components
            .get(&reference.alias)
            .ok_or_else(|| error_at(format!("unknown alias '{}'", reference.alias), action.span))?;
        let source = component
            .spec
            .items
            .iter()
            .find(|item| matches!(item, SpecItem::Action { name, .. } if name == &reference.action))
            .cloned()
            .ok_or_else(|| {
                error_at(
                    format!("unknown action '{}.{}'", reference.alias, reference.action),
                    action.span,
                )
            })?;
        // Fairness is not inherited through synchronization (docs/manual/LANGUAGE.md
        // "Compose"): capture the constituent's own `fair` marker here, before
        // `rewrite_component_item` and the composite's single `fair: action.fair`
        // below discard it, so a non-fair composite that references a fair
        // constituent can be reported.
        if matches!(&source, SpecItem::Action { fair: true, .. }) {
            fair_constituents.push(format!("{}.{}", reference.alias, reference.action));
        }
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
                Ok((
                    name,
                    resolve_alias_expr(arg.clone(), components, action.span)?,
                ))
            })
            .collect::<Result<HashMap<_, _>, CoreError>>()?;
        let source_items = substitute_action_items(source_items, &replacements, &mut avoid);
        item_origins.extend(
            source_items
                .iter()
                .map(|item| component_item_origin(component, &reference.action, item)),
        );
        items.extend(source_items);
    }
    record_sync_item_origins(origins, &action.name, &items, item_origins);
    items.extend(
        action
            .items
            .iter()
            .cloned()
            .map(|item| resolve_alias_action_item(item, components, false))
            .collect::<Result<Vec<_>, _>>()?,
    );
    if !action.fair && !fair_constituents.is_empty() {
        let refs = fair_constituents.join(", ");
        warnings.push(json!({
            "kind": "fair_not_inherited",
            "message": format!(
                "synchronized action '{}' is not fair; fair constituent action(s) {refs} will not contribute fairness unless the composite action is declared fair",
                action.name,
            ),
            "loc": action.span.python_loc(),
        }));
    }
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

/// Names a component `let` must not take over in a sync action (see
/// `substitute_action_items`): the sync action's parameters and every
/// argument's free names, across all references (#1254).
fn sync_avoid_names(
    action: &SyncAction,
    params: &[Param],
    components: &BTreeMap<String, Component>,
) -> Result<HashSet<String>, CoreError> {
    let mut avoid = params
        .iter()
        .map(|param| match param {
            Param::Typed(name, _) | Param::Range(name, _, _) => name.clone(),
        })
        .collect::<HashSet<_>>();
    for reference in &action.refs {
        for arg in &reference.args {
            avoid.extend(free_vars(&resolve_alias_expr(
                arg.clone(),
                components,
                action.span,
            )?));
        }
    }
    Ok(avoid)
}

fn resolve_alias_param(
    param: Param,
    components: &BTreeMap<String, Component>,
    span: fsl_syntax::Span,
) -> Result<Param, CoreError> {
    Ok(match param {
        Param::Typed(name, qualified) => Param::Typed(
            name,
            resolve_alias_qualified_name(qualified, components, span)?,
        ),
        Param::Range(name, lo, hi) => Param::Range(
            name,
            resolve_alias_expr(lo, components, span)?,
            resolve_alias_expr(hi, components, span)?,
        ),
    })
}

fn resolve_alias_qualified_name(
    qualified: QualifiedName,
    components: &BTreeMap<String, Component>,
    span: fsl_syntax::Span,
) -> Result<QualifiedName, CoreError> {
    if let Some(alias) = qualified.namespace {
        let component = components
            .get(&alias)
            .ok_or_else(|| error_at(format!("unknown alias '{alias}'"), span))?;
        if !component.names.types.contains(&qualified.name) {
            let message = format!("unknown type '{alias}.{}'", qualified.name);
            return Err(error_at(message, span).with_kind(CoreErrorKind::UnknownType));
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
    span: fsl_syntax::Span,
) -> Result<Binder, CoreError> {
    Ok(match binder {
        Binder::Typed {
            name,
            type_name,
            where_expr,
        } => Binder::Typed {
            name,
            type_name: resolve_alias_qualified_name(type_name, components, span)?,
            where_expr: where_expr
                .map(|expr| resolve_alias_expr(*expr, components, span).map(Box::new))
                .transpose()?,
        },
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => Binder::Range {
            name,
            lo: Box::new(resolve_alias_expr(*lo, components, span)?),
            hi: Box::new(resolve_alias_expr(*hi, components, span)?),
            where_expr: where_expr
                .map(|expr| resolve_alias_expr(*expr, components, span).map(Box::new))
                .transpose()?,
        },
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => Binder::Collection {
            name,
            collection: Box::new(resolve_alias_expr(*collection, components, span)?),
            where_expr: where_expr
                .map(|expr| resolve_alias_expr(*expr, components, span).map(Box::new))
                .transpose()?,
        },
    })
}

#[allow(clippy::too_many_lines)]
fn resolve_alias_expr(
    expr: Expr,
    components: &BTreeMap<String, Component>,
    diagnostic_span: fsl_syntax::Span,
) -> Result<Expr, CoreError> {
    Ok(match expr {
        Expr::Field(base, name) => {
            if let Expr::Var(alias) = base.as_ref() {
                if components.contains_key(alias) {
                    Expr::Var(prefix(alias, &name))
                } else {
                    Expr::Field(
                        Box::new(resolve_alias_expr(*base, components, diagnostic_span)?),
                        name,
                    )
                }
            } else {
                Expr::Field(
                    Box::new(resolve_alias_expr(*base, components, diagnostic_span)?),
                    name,
                )
            }
        }
        Expr::Some(expr) => Expr::Some(Box::new(resolve_alias_expr(
            *expr,
            components,
            diagnostic_span,
        )?)),
        Expr::Set(items) => Expr::Set(
            items
                .into_iter()
                .map(|item| resolve_alias_expr(item, components, diagnostic_span))
                .collect::<Result<_, _>>()?,
        ),
        Expr::Seq(items) => Expr::Seq(
            items
                .into_iter()
                .map(|item| resolve_alias_expr(item, components, diagnostic_span))
                .collect::<Result<_, _>>()?,
        ),
        Expr::Struct { name, fields } => Expr::Struct {
            name,
            fields: fields
                .into_iter()
                .map(|(name, expr)| {
                    Ok((name, resolve_alias_expr(expr, components, diagnostic_span)?))
                })
                .collect::<Result<_, CoreError>>()?,
        },
        Expr::Call { name, args, span } => Expr::Call {
            name,
            args: args
                .into_iter()
                .map(|arg| resolve_alias_expr(arg, components, diagnostic_span))
                .collect::<Result<_, _>>()?,
            span,
        },
        Expr::Index(base, index) => Expr::Index(
            Box::new(resolve_alias_expr(*base, components, diagnostic_span)?),
            Box::new(resolve_alias_expr(*index, components, diagnostic_span)?),
        ),
        Expr::Method {
            receiver,
            name,
            args,
        } => Expr::Method {
            receiver: Box::new(resolve_alias_expr(*receiver, components, diagnostic_span)?),
            name,
            args: args
                .into_iter()
                .map(|arg| resolve_alias_expr(arg, components, diagnostic_span))
                .collect::<Result<_, _>>()?,
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: Box::new(resolve_alias_expr(*left, components, diagnostic_span)?),
            right: Box::new(resolve_alias_expr(*right, components, diagnostic_span)?),
        },
        Expr::Neg(expr) => Expr::Neg(Box::new(resolve_alias_expr(
            *expr,
            components,
            diagnostic_span,
        )?)),
        Expr::Not(expr) => Expr::Not(Box::new(resolve_alias_expr(
            *expr,
            components,
            diagnostic_span,
        )?)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => Expr::Conditional {
            spans,
            condition: Box::new(resolve_alias_expr(*condition, components, diagnostic_span)?),
            then_expr: Box::new(resolve_alias_expr(*then_expr, components, diagnostic_span)?),
            else_expr: Box::new(resolve_alias_expr(*else_expr, components, diagnostic_span)?),
        },
        Expr::Is { expr, pattern } => Expr::Is {
            expr: Box::new(resolve_alias_expr(*expr, components, diagnostic_span)?),
            pattern,
        },
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => Expr::Quantified {
            quantifier,
            binder: resolve_alias_binder(binder, components, diagnostic_span)?,
            body: Box::new(resolve_alias_expr(*body, components, diagnostic_span)?),
        },
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => Expr::Aggregate {
            kind,
            binder: resolve_alias_binder(binder, components, diagnostic_span)?,
            value: value
                .map(|expr| resolve_alias_expr(*expr, components, diagnostic_span).map(Box::new))
                .transpose()?,
        },
        Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
            name,
            expr: Box::new(resolve_alias_expr(*expr, components, diagnostic_span)?),
            span,
        },
        Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
            name,
            left: Box::new(resolve_alias_expr(*left, components, diagnostic_span)?),
            right: Box::new(resolve_alias_expr(*right, components, diagnostic_span)?),
        },
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => Expr::TernaryNamed {
            name,
            first: Box::new(resolve_alias_expr(*first, components, diagnostic_span)?),
            second: Box::new(resolve_alias_expr(*second, components, diagnostic_span)?),
            third: Box::new(resolve_alias_expr(*third, components, diagnostic_span)?),
        },
        other => other,
    })
}

fn resolve_alias_action_item(
    item: ActionItem,
    components: &BTreeMap<String, Component>,
    allow_component_writes: bool,
) -> Result<ActionItem, CoreError> {
    Ok(match item {
        ActionItem::Requires(expr, span) => {
            ActionItem::Requires(resolve_alias_expr(expr, components, span)?, span)
        }
        ActionItem::Ensures(expr, span) => {
            ActionItem::Ensures(resolve_alias_expr(expr, components, span)?, span)
        }
        ActionItem::Let(name, expr, span) => {
            ActionItem::Let(name, resolve_alias_expr(expr, components, span)?, span)
        }
        ActionItem::Statement(statement) => ActionItem::Statement(resolve_alias_statement(
            statement,
            components,
            allow_component_writes,
        )?),
    })
}

/// Substitute a component action's parameters by a sync reference's arguments
/// throughout the action's items, respecting every binder in scope (#1254).
///
/// `let` bindings scope over the items after them, so they are processed in
/// order: a `let` shadows a parameter of the same name for the rest of the
/// action, and a `let` whose name occurs free in an argument (`go(y) =
/// c.poke(y)` against `let y = 0`) or in `avoid` is renamed to a fresh name,
/// so neither the argument nor a later sync part reading that name is
/// captured by it. `avoid` collects the names the composed action's other
/// parts read: the sync action's own parameters, every argument's free
/// names, and the `let`s already emitted by earlier references.
fn substitute_action_items(
    items: Vec<ActionItem>,
    replacements: &HashMap<String, Expr>,
    avoid: &mut HashSet<String>,
) -> Vec<ActionItem> {
    let indexed = IndexedReplacements::new();
    let mut names = avoid.clone();
    names.extend(replacements.keys().cloned());
    for replacement in replacements.values() {
        collect_names(replacement, &mut names);
    }
    for item in &items {
        collect_action_item_names(item, &mut names);
    }
    let mut current = replacements.clone();
    items
        .into_iter()
        .map(|item| match item {
            ActionItem::Requires(expr, span) => {
                ActionItem::Requires(substitute(expr, &current, &indexed), span)
            }
            ActionItem::Ensures(expr, span) => {
                ActionItem::Ensures(substitute(expr, &current, &indexed), span)
            }
            ActionItem::Let(name, expr, span) => {
                let value = substitute(expr, &current, &indexed);
                current.remove(&name);
                let captures = avoid.contains(&name)
                    || current
                        .values()
                        .any(|replacement| free_vars(replacement).contains(&name));
                let name = if captures {
                    let fresh = fresh_binding_name(&name, &names);
                    names.insert(fresh.clone());
                    current.insert(name, Expr::Var(fresh.clone()));
                    fresh
                } else {
                    name
                };
                avoid.insert(name.clone());
                ActionItem::Let(name, value, span)
            }
            ActionItem::Statement(statement) => {
                ActionItem::Statement(substitute_statement(statement, &current))
            }
        })
        .collect()
}

fn collect_action_item_names(item: &ActionItem, names: &mut HashSet<String>) {
    match item {
        ActionItem::Requires(expr, _) | ActionItem::Ensures(expr, _) => collect_names(expr, names),
        ActionItem::Let(name, expr, _) => {
            names.insert(name.clone());
            collect_names(expr, names);
        }
        ActionItem::Statement(statement) => collect_statement_names(statement, names),
    }
}

fn collect_statement_names(statement: &Statement, names: &mut HashSet<String>) {
    match statement {
        Statement::Assign { target, value, .. } => {
            collect_lvalue_names(target, names);
            collect_names(value, names);
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            collect_names(condition, names);
            for statement in then_statements.iter().chain(else_statements) {
                collect_statement_names(statement, names);
            }
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            collect_binder_names(binder, names);
            for statement in statements {
                collect_statement_names(statement, names);
            }
        }
    }
}

fn collect_lvalue_names(lvalue: &LValue, names: &mut HashSet<String>) {
    match lvalue {
        LValue::Var(name) => {
            names.insert(name.clone());
        }
        LValue::Index(name, expr) => {
            names.insert(name.clone());
            collect_names(expr, names);
        }
        LValue::Field(base, _) => collect_lvalue_names(base, names),
    }
}

fn substitute_statement(statement: Statement, replacements: &HashMap<String, Expr>) -> Statement {
    let indexed = IndexedReplacements::new();
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => Statement::Assign {
            target: substitute_lvalue(target, replacements),
            value: substitute(value, replacements, &indexed),
            span,
        },
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => Statement::If {
            condition: substitute(condition, replacements, &indexed),
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
        // The statement `forall` follows the expression quantifier's rule
        // (`substitute` in lib.rs): the binder's where / range / collection
        // are substituted, the binding shadows a same-named parameter in its
        // body, and a binding that an argument mentions free is renamed
        // first so the argument is not captured (#1254).
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let original = binder_name(&binder).to_owned();
            let mut body_names = HashSet::new();
            for statement in &statements {
                collect_statement_names(statement, &mut body_names);
            }
            let (binder, _, mut scoped, _) = capture_avoiding_binding_with_names(
                binder,
                Vec::new(),
                replacements,
                &indexed,
                &body_names,
            );
            let renamed = binder_name(&binder).to_owned();
            if renamed != original {
                scoped.insert(original, Expr::Var(renamed));
            }
            Statement::ForAll {
                binder: substitute_binder(binder, replacements, &indexed),
                statements: statements
                    .into_iter()
                    .map(|statement| substitute_statement(statement, &scoped))
                    .collect(),
                span,
            }
        }
    }
}

fn substitute_lvalue(lvalue: LValue, replacements: &HashMap<String, Expr>) -> LValue {
    match lvalue {
        LValue::Index(name, expr) => LValue::Index(
            name,
            substitute(expr, replacements, &IndexedReplacements::new()),
        ),
        LValue::Field(base, field) => {
            LValue::Field(Box::new(substitute_lvalue(*base, replacements)), field)
        }
        lvalue @ LValue::Var(_) => lvalue,
    }
}
