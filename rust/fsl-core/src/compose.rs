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

use crate::model::ExpressionErrorContext;
use crate::naming::NameMap;
use crate::typecheck::TypecheckError;
use crate::{
    CoreError, CoreErrorKind, KernelSpec, PredicateExpander, collect_binder_names, collect_names,
    expand_spec_domains,
};

mod hygiene;
pub(crate) use hygiene::map_children;

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
    ambient: HashSet<String>,
    public_bindings: HashSet<String>,
}

#[derive(Clone, Default)]
struct ComponentNames {
    pattern_scopes: std::rc::Rc<std::cell::RefCell<crate::typecheck::PatternScopes>>,
    consts: HashSet<String>,
    enum_members: HashMap<String, Vec<String>>,
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
                SpecItem::Type { name, .. } | SpecItem::Struct { name, .. } => {
                    names.types.insert(name.clone());
                }
                SpecItem::Enum { name, members, .. } => {
                    names.types.insert(name.clone());
                    for member in members {
                        names
                            .enum_members
                            .entry(member.clone())
                            .or_default()
                            .push(name.clone());
                    }
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
        for owners in names.enum_members.values_mut() {
            owners.sort();
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
        SpecItem::Enum { name, members, .. } if members.is_empty() => {
            let mut origin = component_use_origin(&component.path, component.use_span);
            origin
                .primary
                .as_mut()
                .expect("use site")
                .declaration_path
                .push(name.clone());
            origins.insert(crate::type_target(&prefix(&component.alias, name)), origin);
            return;
        }
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
                ambient: HashSet::new(),
                public_bindings: HashSet::new(),
            },
        );
    }
    let compose = expand_compose_predicates(compose, &components)?;
    let internal = compose
        .items
        .iter()
        .filter_map(|item| match item {
            ComposeItem::Internal { alias, action, .. } => Some((alias.clone(), action.clone())),
            _ => None,
        })
        .collect::<HashSet<_>>();

    let mut composition_names = ComponentNames::default();
    for item in &compose.items {
        if let ComposeItem::Common(SpecItem::Const { name, .. }) = item {
            composition_names.consts.insert(name.clone());
        }
        if let ComposeItem::Common(SpecItem::State(fields)) = item {
            composition_names
                .state
                .extend(fields.iter().map(|field| field.name.clone()));
        }
    }
    for component in components.values() {
        for (member, owners) in &component.names.enum_members {
            composition_names
                .enum_members
                .entry(member.clone())
                .or_default()
                .extend(owners.iter().map(|owner| prefix(&component.alias, owner)));
        }
    }
    let mut ambient = composition_names
        .state
        .iter()
        .chain(composition_names.consts.iter())
        .chain(composition_names.enum_members.keys())
        .cloned()
        .collect::<HashSet<_>>();
    ambient.extend(components.values().flat_map(|component| {
        component
            .names
            .state
            .iter()
            .chain(&component.names.consts)
            .map(|name| prefix(&component.alias, name))
    }));
    for component in components.values_mut() {
        component.names.pattern_scopes = composition_names.pattern_scopes.clone();
        component.ambient.clone_from(&ambient);
        component
            .public_bindings
            .clone_from(&composition_names.state);
        component.public_bindings.extend(
            component
                .names
                .enum_members
                .iter()
                .filter(|(member, owners)| {
                    composition_names.enum_members[*member].len() > owners.len()
                })
                .map(|(member, _)| member.clone()),
        );
    }

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
                    init.extend(
                        rewrite_statements(statements.clone(), component, &HashSet::new())
                            .and_then(|statements| {
                                hygiene::root_statements(statements, &component.names, &ambient)
                            })
                            .map_err(|error| {
                                component_rewrite_error(
                                    error,
                                    ExpressionErrorContext::Init,
                                    Some(component),
                                )
                            })?,
                    );
                    if init_meta.is_none() {
                        init_meta.clone_from(meta);
                    }
                    init_annotations.extend(annotations.source_order().iter().cloned());
                }
                SpecItem::Action { name, .. } => {
                    if !internal.contains(&(alias.clone(), name.clone())) {
                        names.insert_action(alias, name);
                        actions.push(hygienic_component_action(
                            rewrite_component_item(item.clone(), component)?,
                            &component.names,
                            &ambient,
                        )?);
                    }
                }
                _ => static_items.push(rewrite_component_item(item.clone(), component)?),
            }
        }
    }

    for item in &compose.items {
        match item {
            ComposeItem::Common(SpecItem::Init {
                statements,
                meta,
                annotations,
            }) => {
                init_origins.extend(std::iter::repeat_n(None, statements.len()));
                init.extend(rewrite_compose_statements(
                    statements.clone(),
                    &components,
                    &HashSet::new(),
                    ComposeWriteContext::Init,
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
                    &composition_names,
                    &ambient,
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
    kernel.names.pattern_scopes = composition_names.pattern_scopes.borrow().clone();
    for (target, origin) in origins {
        kernel
            .origins
            .bind(crate::origin::component_check_target(&target), origin);
    }
    Ok(kernel)
}

/// Expand source-local defs before aliases or sync substitution can change scope.
/// Loaded component enum members supplement the source's own global inventory.
fn expand_compose_predicates(
    mut compose: SurfaceCompose,
    components: &BTreeMap<String, Component>,
) -> Result<SurfaceCompose, CoreError> {
    let definitions = SurfaceSpec {
        name: compose.name.clone(),
        meta: None,
        items: compose
            .items
            .iter()
            .filter_map(|item| match item {
                ComposeItem::Common(item) => Some(item.clone()),
                _ => None,
            })
            .collect(),
    };
    let mut expander = PredicateExpander::new(&definitions)?;
    expander.globals.extend(
        components
            .values()
            .flat_map(|component| component.names.enum_members.keys().cloned()),
    );
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
                        .map(|arg| {
                            expander.scoped(param_names(&action.params), || {
                                expander.expand_expr(arg, &mut Vec::new())
                            })
                        })
                        .collect::<Result<_, _>>()?;
                }
                action.items = expander.expand_action_items(action.items, &action.params)?;
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

fn component_rewrite_error(
    error: TypecheckError,
    context: ExpressionErrorContext<'_>,
    component: Option<&Component>,
) -> CoreError {
    if error.constant_shadow
        && let Some(component) = component
    {
        let source = component_rewrite_error(error, context, None);
        return component_error(
            &component.path,
            component.use_span,
            &source,
            "failed to lower",
        );
    }
    let TypecheckError { message, span, .. } = error;
    let message = context.message(message);
    if let Some(span) = span {
        return error_at(message, span);
    }
    CoreError {
        message,
        line: 0,
        column: 0,
        kind: CoreErrorKind::Semantics,
        origin: None,
        diagnostic_code: None,
    }
}

fn rewrite_component_item(item: SpecItem, component: &Component) -> Result<SpecItem, CoreError> {
    let span = match &item {
        SpecItem::Action { span, .. }
        | SpecItem::Invariant { span, .. }
        | SpecItem::Trans { span, .. }
        | SpecItem::Reachable { span, .. }
        | SpecItem::Terminal { span, .. }
        | SpecItem::Until { span, .. }
        | SpecItem::Unless { span, .. }
        | SpecItem::LeadsTo { span, .. }
        | SpecItem::Struct { span, .. }
        | SpecItem::VerifyBounds { span, .. } => Some(*span),
        _ => None,
    };
    let initializers = match &item {
        SpecItem::State(fields) => fields
            .iter()
            .filter_map(|field| {
                field
                    .initializer_span
                    .map(|span| (span, field.name.clone()))
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    rewrite_component_item_inner(item, component).map_err(|error| {
        let error = match span {
            Some(span) => error.with_span(span),
            None => error,
        };
        let owner = initializers.iter().find(|(initializer, _)| {
            error.span.is_some_and(|span| {
                span.start.offset >= initializer.start.offset
                    && span.end.offset <= initializer.end.offset
            })
        });
        match owner {
            Some((_, name)) => component_rewrite_error(
                error,
                ExpressionErrorContext::InlineInitializer(name),
                Some(component),
            ),
            None => component_rewrite_error(error, ExpressionErrorContext::Model, Some(component)),
        }
    })
}

#[allow(clippy::too_many_lines)]
fn rewrite_component_item_inner(
    item: SpecItem,
    component: &Component,
) -> Result<SpecItem, TypecheckError> {
    let alias = &component.alias;
    Ok(match item {
        SpecItem::Const { name, value } => SpecItem::Const {
            name: prefix(alias, &name),
            value: Box::new(rewrite_expr(*value, component, &HashSet::new())?),
        },
        SpecItem::Type {
            name,
            lo,
            hi,
            symmetric,
        } => SpecItem::Type {
            name: prefix(alias, &name),
            lo: Box::new(rewrite_expr(*lo, component, &HashSet::new())?),
            hi: Box::new(rewrite_expr(*hi, component, &HashSet::new())?),
            symmetric,
        },
        SpecItem::Enum {
            name,
            members,
            member_spans,
            span,
            symmetric,
        } => SpecItem::Enum {
            name: prefix(alias, &name),
            members,
            member_spans,
            span,
            symmetric,
        },
        SpecItem::Struct { name, fields, span } => SpecItem::Struct {
            name: prefix(alias, &name),
            span,
            fields: fields
                .into_iter()
                .map(|(name, ty)| {
                    Ok((
                        name,
                        rewrite_type(ty, component).map_err(|error| error.with_span(span))?,
                    ))
                })
                .collect::<Result<_, TypecheckError>>()?,
        },
        SpecItem::State(fields) => SpecItem::State(
            fields
                .into_iter()
                .map(|field| {
                    Ok(fsl_syntax::StateField {
                        name: prefix(alias, &field.name),
                        ty: rewrite_type(field.ty, component)
                            .map_err(|error| error.with_span(field.span))?,
                        initializer: field
                            .initializer
                            .map(|expr| {
                                if crate::model::unsupported_inline_form(&expr).is_some() {
                                    return Ok(expr);
                                }
                                rewrite_root_expr_at(
                                    expr,
                                    component,
                                    &HashSet::new(),
                                    field.initializer_span.unwrap_or(field.span),
                                )
                            })
                            .transpose()?,
                        span: field.span,
                        initializer_span: field.initializer_span,
                    })
                })
                .collect::<Result<_, TypecheckError>>()?,
        ),
        SpecItem::Init {
            statements,
            meta,
            annotations,
        } => SpecItem::Init {
            statements: hygiene::root_statements(
                rewrite_statements(statements, component, &HashSet::new())?,
                &component.names,
                &component.ambient,
            )?,
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
                .collect::<Result<Vec<_>, TypecheckError>>()
                .map_err(|error| error.with_span(span))?;
            let bound = param_names(&params);
            SpecItem::Action {
                name: prefix(alias, &name),
                params,
                items: rewrite_action_items(items, component, &bound, span)?,
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
            expr: Box::new(rewrite_root_expr_at(
                *expr,
                component,
                &HashSet::new(),
                span,
            )?),
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
            expr: Box::new(rewrite_root_expr_at(
                *expr,
                component,
                &HashSet::new(),
                span,
            )?),
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
            expr: Box::new(rewrite_root_expr_at(
                *expr,
                component,
                &HashSet::new(),
                span,
            )?),
            span,
            meta,
            annotations,
        },
        SpecItem::Terminal { expr, span } => SpecItem::Terminal {
            expr: Box::new(rewrite_root_expr_at(
                *expr,
                component,
                &HashSet::new(),
                span,
            )?),
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
                    let binder = rewrite_binder(binder, component, &bound)?;
                    let binder =
                        hygiene::root_binder(binder, &component.names, &bound, &component.ambient)?;
                    bound.insert(binder_name(&binder).to_owned());
                    Ok(binder)
                })
                .collect::<Result<_, TypecheckError>>()?;
            SpecItem::LeadsTo {
                name: prefix(alias, &name),
                binders,
                before: Box::new(rewrite_root_expr_at(*before, component, &bound, span)?),
                after: Box::new(rewrite_root_expr_at(*after, component, &bound, span)?),
                span,
                meta,
                decreases: decreases
                    .map(|expr| rewrite_root_expr_at(*expr, component, &bound, span).map(Box::new))
                    .transpose()?,
                within: within
                    .map(|expr| rewrite_root_expr_at(*expr, component, &bound, span).map(Box::new))
                    .transpose()?,
                helpful: helpful
                    .into_iter()
                    .map(|mut helper| {
                        helper.action = prefix(alias, &helper.action);
                        helper.args = helper
                            .args
                            .into_iter()
                            .map(|arg| rewrite_root_expr_at(arg, component, &bound, helper.span))
                            .collect::<Result<_, TypecheckError>>()?;
                        Ok(helper)
                    })
                    .collect::<Result<_, TypecheckError>>()?,
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
            before: Box::new(rewrite_root_expr_at(
                *before,
                component,
                &HashSet::new(),
                span,
            )?),
            after: Box::new(rewrite_root_expr_at(
                *after,
                component,
                &HashSet::new(),
                span,
            )?),
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
            before: Box::new(rewrite_root_expr_at(
                *before,
                component,
                &HashSet::new(),
                span,
            )?),
            after: Box::new(rewrite_root_expr_at(
                *after,
                component,
                &HashSet::new(),
                span,
            )?),
            span,
            meta,
            annotations,
        },
        item @ (SpecItem::VerifyBounds { .. }
        | SpecItem::Entity(..)
        | SpecItem::Number(..)
        | SpecItem::Def { .. }) => item,
    })
}

fn rewrite_type(ty: TypeExpr, component: &Component) -> Result<TypeExpr, TypecheckError> {
    Ok(match ty {
        TypeExpr::Name(name) if component.names.types.contains(&name) => {
            TypeExpr::Name(prefix(&component.alias, &name))
        }
        TypeExpr::Range(lo, hi) => TypeExpr::Range(
            rewrite_expr(lo, component, &HashSet::new())?,
            rewrite_expr(hi, component, &HashSet::new())?,
        ),
        TypeExpr::Map(key, value) => TypeExpr::Map(
            Box::new(rewrite_type(*key, component)?),
            Box::new(rewrite_type(*value, component)?),
        ),
        TypeExpr::Relation(source, target) => TypeExpr::Relation(
            Box::new(rewrite_type(*source, component)?),
            Box::new(rewrite_type(*target, component)?),
        ),
        TypeExpr::Set(inner) => TypeExpr::Set(Box::new(rewrite_type(*inner, component)?)),
        TypeExpr::Seq(inner, cap) => TypeExpr::Seq(
            Box::new(rewrite_type(*inner, component)?),
            rewrite_expr(cap, component, &HashSet::new())?,
        ),
        TypeExpr::Option(inner) => TypeExpr::Option(Box::new(rewrite_type(*inner, component)?)),
        other => other,
    })
}

fn rewrite_param(param: Param, component: &Component) -> Result<Param, TypecheckError> {
    let (Param::Typed(name, _) | Param::Range(name, ..)) = &param;
    crate::typecheck::reject_constant_shadow(name, &component.names.consts)?;
    Ok(match param {
        Param::Typed(name, mut qualified) => {
            if qualified.namespace.is_none() && component.names.types.contains(&qualified.name) {
                qualified.name = prefix(&component.alias, &qualified.name);
            }
            Param::Typed(name, qualified)
        }
        Param::Range(name, lo, hi) => Param::Range(
            name,
            rewrite_expr(lo, component, &HashSet::new())?,
            rewrite_expr(hi, component, &HashSet::new())?,
        ),
    })
}

pub(super) fn param_names(params: &[Param]) -> HashSet<String> {
    params
        .iter()
        .map(|param| match param {
            Param::Typed(name, _) | Param::Range(name, _, _) => name.clone(),
        })
        .collect()
}

fn hygienic_component_action(
    mut item: SpecItem,
    globals: &ComponentNames,
    ambient: &HashSet<String>,
) -> Result<SpecItem, CoreError> {
    let SpecItem::Action {
        items,
        params,
        span,
        ..
    } = &mut item
    else {
        unreachable!("only component actions enter source hygiene")
    };
    let bound = param_names(params);
    let mut used = ambient.clone();
    used.extend(bound.iter().cloned());
    for item in items.iter() {
        collect_action_item_names(item, &mut used, None);
    }
    *items = hygiene::rewrite(
        std::mem::take(items),
        hygiene::Source {
            globals,
            params: &bound,
            span: *span,
            arguments: None,
        },
        &HashSet::new(),
        &HashSet::new(),
        ambient,
        &mut used,
    )?;
    Ok(item)
}

/// Match model normalization without changing surface item order: guards see
/// preceding guards; every statement and ensures sees the final guard scope.
pub(super) fn map_normalized_action_items<S, E>(
    mut items: Vec<ActionItem>,
    scope: &mut S,
    mut rewrite: impl FnMut(ActionItem, &mut S) -> Result<ActionItem, E>,
) -> Result<Vec<ActionItem>, E> {
    for guards in [true, false] {
        for item in &mut items {
            if matches!(item, ActionItem::Requires(..) | ActionItem::Let(..)) == guards {
                *item = rewrite(item.clone(), scope)?;
            }
        }
    }
    Ok(items)
}

pub(super) fn map_scoped_action_items<E>(
    items: Vec<ActionItem>,
    bound: &mut HashSet<String>,
    globals: &HashSet<String>,
    mut rewrite: impl FnMut(ActionItem, &HashSet<String>) -> Result<ActionItem, E>,
) -> Result<Vec<ActionItem>, E> {
    map_normalized_action_items(items, bound, |item, bound| {
        let item = rewrite(item, bound)?;
        match &item {
            ActionItem::Requires(expr, _) => *bound = pattern_scope(expr, bound, globals, None),
            ActionItem::Let(name, ..) => {
                bound.insert(name.clone());
            }
            ActionItem::Ensures(..) | ActionItem::Statement(_) => {}
        }
        Ok(item)
    })
}

fn rewrite_action_items(
    items: Vec<ActionItem>,
    component: &Component,
    bound: &HashSet<String>,
    action_span: fsl_syntax::Span,
) -> Result<Vec<ActionItem>, TypecheckError> {
    let mut bound = bound.clone();
    map_scoped_action_items(items, &mut bound, &HashSet::new(), |item, bound| {
        Ok(match item {
            ActionItem::Requires(expr, span) => {
                let expr = rewrite_expr_at(expr, component, bound, action_span)?;
                ActionItem::Requires(expr, span)
            }
            ActionItem::Ensures(expr, span) => {
                ActionItem::Ensures(rewrite_expr_at(expr, component, bound, action_span)?, span)
            }
            ActionItem::Let(name, expr, span) => {
                crate::typecheck::reject_constant_shadow(&name, &component.names.consts)?;
                let expr = rewrite_expr_at(expr, component, bound, action_span)?;
                ActionItem::Let(name, expr, span)
            }
            ActionItem::Statement(statement) => {
                ActionItem::Statement(rewrite_statement(statement, component, bound)?)
            }
        })
    })
}

fn rewrite_statements(
    statements: Vec<Statement>,
    component: &Component,
    bound: &HashSet<String>,
) -> Result<Vec<Statement>, TypecheckError> {
    statements
        .into_iter()
        .map(|statement| rewrite_statement(statement, component, bound))
        .collect()
}

fn rewrite_statement(
    statement: Statement,
    component: &Component,
    bound: &HashSet<String>,
) -> Result<Statement, TypecheckError> {
    Ok(match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => Statement::Assign {
            target: rewrite_lvalue(target, component, bound)
                .map_err(|error| error.with_span(span))?,
            value: rewrite_expr_at(value, component, bound, span)?,
            span,
        },
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            let condition = rewrite_expr_at(condition, component, bound, span)?;
            Statement::If {
                condition,
                then_statements: rewrite_statements(then_statements, component, bound)?,
                else_statements: rewrite_statements(else_statements, component, bound)?,
                span,
            }
        }
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let binder =
                rewrite_binder(binder, component, bound).map_err(|error| error.with_span(span))?;
            let mut nested = bound.clone();
            nested.insert(binder_name(&binder).to_owned());
            Statement::ForAll {
                binder,
                statements: rewrite_statements(statements, component, &nested)?,
                span,
            }
        }
    })
}

fn rewrite_lvalue(
    lvalue: LValue,
    component: &Component,
    bound: &HashSet<String>,
) -> Result<LValue, TypecheckError> {
    Ok(match lvalue {
        LValue::Var(name) => LValue::Var(rewrite_state_name(&name, component)?),
        LValue::Index(name, expr) => LValue::Index(
            rewrite_state_name(&name, component)?,
            rewrite_expr(expr, component, bound)?,
        ),
        LValue::Field(base, field) => {
            LValue::Field(Box::new(rewrite_lvalue(*base, component, bound)?), field)
        }
    })
}

fn rewrite_state_name(name: &str, component: &Component) -> Result<String, TypecheckError> {
    if component.names.state.contains(name) {
        Ok(prefix(&component.alias, name))
    } else {
        Err(crate::typecheck::error(
            crate::model::unknown_update_target(name).message,
        ))
    }
}

fn rewrite_root_expr_at(
    expr: Expr,
    component: &Component,
    bound: &HashSet<String>,
    span: fsl_syntax::Span,
) -> Result<Expr, TypecheckError> {
    let expr = rewrite_expr_at(expr, component, bound, span)?;
    hygiene::root_expression(expr, &component.names, bound, &component.ambient)
        .map_err(|error| error.with_span(span))
}

fn rewrite_expr_at(
    expr: Expr,
    component: &Component,
    bound: &HashSet<String>,
    span: fsl_syntax::Span,
) -> Result<Expr, TypecheckError> {
    rewrite_expr(expr, component, bound).map_err(|error| error.with_span(span))
}

fn rewrite_expr(
    expr: Expr,
    component: &Component,
    bound: &HashSet<String>,
) -> Result<Expr, TypecheckError> {
    rewrite_expr_in_phase(
        expr,
        component,
        bound,
        &crate::typecheck::ValidationPhase::normalized(),
    )
}

fn rewrite_expr_in_phase(
    expr: Expr,
    component: &Component,
    bound: &HashSet<String>,
    phase: &crate::typecheck::ValidationPhase,
) -> Result<Expr, TypecheckError> {
    crate::recursion::guard(|| {
        let rewrite = |expr| rewrite_expr_in_phase(expr, component, bound, phase);
        Ok(match expr {
            Expr::Var(name) => rewrite_component_read(name, component, bound)?,
            Expr::Struct { name, fields } => hygiene::map_children(
                Expr::Struct {
                    name: if component.names.types.contains(&name) {
                        prefix(&component.alias, &name)
                    } else {
                        name
                    },
                    fields,
                },
                |child, _| rewrite(child),
            )?,
            Expr::Binary { op, left, right } => {
                let left = rewrite(*left)?;
                let nested = pattern_scope(&left, bound, &HashSet::new(), Some(&op));
                let right_phase = phase.after_patterns(&left);
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(rewrite_expr_in_phase(
                        *right,
                        component,
                        &nested,
                        &right_phase,
                    )?),
                }
            }
            Expr::Is { expr, pattern } => {
                if let fsl_syntax::Pattern::Some(name) = &pattern {
                    crate::typecheck::reject_pattern_binding_name(
                        phase,
                        None,
                        name,
                        component.names.state.iter().map(String::as_str),
                        component.names.consts.iter().map(String::as_str),
                        component.names.enum_members.keys().map(String::as_str),
                        bound.iter().chain(&phase.collisions).map(String::as_str),
                    )?;
                }
                Expr::Is {
                    expr: Box::new(rewrite(*expr)?),
                    pattern,
                }
            }
            Expr::Quantified {
                quantifier,
                binder,
                body,
            } => {
                let child_phase = phase.under_binder(&binder, None);
                let binder =
                    rewrite_binder_in_phase(binder, component, bound, phase, &child_phase)?;
                let mut nested = bound.clone();
                nested.insert(binder_name(&binder).to_owned());
                Expr::Quantified {
                    quantifier,
                    binder,
                    body: Box::new(rewrite_expr_in_phase(
                        *body,
                        component,
                        &nested,
                        &child_phase,
                    )?),
                }
            }
            Expr::Aggregate {
                kind,
                binder,
                value,
            } => {
                let child_phase = phase.under_binder(&binder, Some(kind));
                let binder =
                    rewrite_binder_in_phase(binder, component, bound, phase, &child_phase)?;
                let mut nested = bound.clone();
                nested.insert(binder_name(&binder).to_owned());
                Expr::Aggregate {
                    kind,
                    binder,
                    value: value
                        .map(|expr| {
                            rewrite_expr_in_phase(*expr, component, &nested, &child_phase)
                                .map(Box::new)
                        })
                        .transpose()?,
                }
            }
            expr @ Expr::Stage { .. } => expr,
            expr => hygiene::map_children(expr, |child, span| match span {
                Some(span) => rewrite(child).map_err(|error| error.with_span(span)),
                None => rewrite(child),
            })?,
        })
    })
}

fn rewrite_component_read(
    name: String,
    component: &Component,
    bound: &HashSet<String>,
) -> Result<Expr, TypecheckError> {
    if bound.contains(&name) {
        return Ok(Expr::Var(name));
    }
    if component.names.state.contains(&name) || component.names.consts.contains(&name) {
        return Ok(Expr::Var(prefix(&component.alias, &name)));
    }
    let owners = component
        .names
        .enum_members
        .get(&name)
        .map_or(&[][..], Vec::as_slice);
    let [owner] = owners else {
        return Err(crate::typecheck::unresolved_identifier(&name, owners));
    };
    // Only an unrenamed public value requires reifying the original bare read.
    if component.public_bindings.contains(&name) {
        return Ok(Expr::EnumMember {
            type_name: prefix(&component.alias, owner),
            member: name,
            source_bare: true,
        });
    }
    Ok(Expr::Var(name))
}

fn rewrite_binder(
    binder: Binder,
    component: &Component,
    bound: &HashSet<String>,
) -> Result<Binder, TypecheckError> {
    let phase = crate::typecheck::ValidationPhase::normalized();
    let child_phase = phase.under_binder(&binder, None);
    rewrite_binder_in_phase(binder, component, bound, &phase, &child_phase)
}

fn rewrite_binder_in_phase(
    binder: Binder,
    component: &Component,
    bound: &HashSet<String>,
    phase: &crate::typecheck::ValidationPhase,
    child_phase: &crate::typecheck::ValidationPhase,
) -> Result<Binder, TypecheckError> {
    crate::typecheck::reject_constant_shadow(binder_name(&binder), &component.names.consts)?;
    let mut nested = bound.clone();
    nested.insert(binder_name(&binder).to_owned());
    let filter = |expr: Option<Box<Expr>>| {
        expr.map(|expr| rewrite_expr_in_phase(*expr, component, &nested, child_phase).map(Box::new))
            .transpose()
    };
    Ok(match binder {
        Binder::Typed {
            name,
            mut type_name,
            where_expr,
        } => {
            if type_name.namespace.is_none() && component.names.types.contains(&type_name.name) {
                type_name.name = prefix(&component.alias, &type_name.name);
            }
            Binder::Typed {
                name,
                type_name,
                where_expr: filter(where_expr)?,
            }
        }
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => Binder::Range {
            name,
            lo: Box::new(rewrite_expr_in_phase(*lo, component, bound, phase)?),
            hi: Box::new(rewrite_expr_in_phase(*hi, component, bound, phase)?),
            where_expr: filter(where_expr)?,
        },
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => Binder::Collection {
            name,
            collection: Box::new(rewrite_expr_in_phase(*collection, component, bound, phase)?),
            where_expr: filter(where_expr)?,
        },
    })
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
        SpecItem::State(fields) => SpecItem::State(
            fields
                .into_iter()
                .map(|mut field| {
                    field.ty = resolve_alias_type(field.ty, components, field.span)?;
                    field.initializer = field
                        .initializer
                        .map(|expr| {
                            resolve_alias_expr(
                                expr,
                                components,
                                field.initializer_span.unwrap_or(field.span),
                            )
                        })
                        .transpose()?;
                    Ok(field)
                })
                .collect::<Result<_, CoreError>>()?,
        ),
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

fn resolve_alias_type(
    ty: TypeExpr,
    components: &BTreeMap<String, Component>,
    span: fsl_syntax::Span,
) -> Result<TypeExpr, CoreError> {
    let resolve_type = |ty| resolve_alias_type(ty, components, span).map(Box::new);
    let resolve_expr = |expr| resolve_alias_expr(expr, components, span);
    Ok(match ty {
        TypeExpr::Range(lo, hi) => TypeExpr::Range(resolve_expr(lo)?, resolve_expr(hi)?),
        TypeExpr::Map(key, value) => TypeExpr::Map(resolve_type(*key)?, resolve_type(*value)?),
        TypeExpr::Relation(source, target) => {
            TypeExpr::Relation(resolve_type(*source)?, resolve_type(*target)?)
        }
        TypeExpr::Set(inner) => TypeExpr::Set(resolve_type(*inner)?),
        TypeExpr::Seq(inner, cap) => TypeExpr::Seq(resolve_type(*inner)?, resolve_expr(cap)?),
        TypeExpr::Option(inner) => TypeExpr::Option(resolve_type(*inner)?),
        TypeExpr::Int | TypeExpr::Bool | TypeExpr::Name(_) => ty,
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
    let mut bound = HashSet::new();
    let binders = binders
        .into_iter()
        .map(|binder| {
            let (binder, nested) = resolve_alias_binder(binder, components, span, &bound)?;
            bound = nested;
            Ok(binder)
        })
        .collect::<Result<_, CoreError>>()?;
    let resolve =
        |expr: Box<Expr>| resolve_alias_expr_scoped(*expr, components, span, &bound).map(Box::new);
    Ok(SpecItem::LeadsTo {
        name,
        binders,
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
                    .map(|arg| resolve_alias_expr_scoped(arg, components, span, &bound))
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
    let (params, bound) = resolve_alias_params(params, components, span)?;
    Ok(SpecItem::Action {
        name,
        params,
        items: resolve_alias_action_items(items, components, &bound, ComposeWriteContext::Glue)?,
        span,
        fair,
        meta,
        sync,
        annotations,
    })
}

#[derive(Clone, Copy)]
enum ComposeWriteContext {
    Glue,
    Sync,
    Init,
}

impl ComposeWriteContext {
    fn permits_component_write(self) -> bool {
        match self {
            Self::Glue | Self::Init => true,
            Self::Sync => false,
        }
    }
}

fn rewrite_compose_statements(
    statements: Vec<Statement>,
    components: &BTreeMap<String, Component>,
    bound: &HashSet<String>,
    context: ComposeWriteContext,
) -> Result<Vec<Statement>, CoreError> {
    statements
        .into_iter()
        .map(|statement| resolve_alias_statement(statement, components, bound, context))
        .collect()
}

fn resolve_alias_statement(
    statement: Statement,
    components: &BTreeMap<String, Component>,
    bound: &HashSet<String>,
    context: ComposeWriteContext,
) -> Result<Statement, CoreError> {
    Ok(match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => Statement::Assign {
            target: resolve_alias_lvalue(target, components, span, bound, context)?,
            value: resolve_alias_expr_scoped(value, components, span, bound)?,
            span,
        },
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            let condition = resolve_alias_expr_scoped(condition, components, span, bound)?;
            Statement::If {
                condition,
                then_statements: rewrite_compose_statements(
                    then_statements,
                    components,
                    bound,
                    context,
                )?,
                else_statements: rewrite_compose_statements(
                    else_statements,
                    components,
                    bound,
                    context,
                )?,
                span,
            }
        }
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let (binder, nested) = resolve_alias_binder(binder, components, span, bound)?;
            Statement::ForAll {
                binder,
                statements: rewrite_compose_statements(statements, components, &nested, context)?,
                span,
            }
        }
    })
}

/// All compose-side assignment paths share alias/index normalization and write policy.
fn resolve_alias_lvalue(
    target: LValue,
    components: &BTreeMap<String, Component>,
    span: fsl_syntax::Span,
    bound: &HashSet<String>,
    context: ComposeWriteContext,
) -> Result<LValue, CoreError> {
    let via_alias = matches!(&target, LValue::Field(base, _)
        if matches!(base.as_ref(), LValue::Var(alias)
            if !bound.contains(alias) && components.contains_key(alias)));
    let target = match target {
        LValue::Var(name) => LValue::Var(name),
        LValue::Index(name, index) => LValue::Index(
            name,
            resolve_alias_expr_scoped(index, components, span, bound)?,
        ),
        LValue::Field(base, name) => match *base {
            LValue::Var(alias) if via_alias => LValue::Var(prefix(&alias, &name)),
            base => LValue::Field(
                Box::new(resolve_alias_lvalue(
                    base, components, span, bound, context,
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
    if !context.permits_component_write()
        && components.values().any(|component| {
            component
                .names
                .state
                .iter()
                .any(|state| prefix(&component.alias, state) == *name)
        })
    {
        return Err(error_at(
            "sync additional statements may only write composition-side state",
            span,
        ));
    }
    Ok(target)
}

struct SyncSource {
    items: Vec<ActionItem>,
    item_origins: Vec<crate::OriginChain>,
    params: HashSet<String>,
    replacements: HashMap<String, Expr>,
    globals: ComponentNames,
    span: fsl_syntax::Span,
}

struct SourceNames {
    all: HashSet<String>,
    capture: HashSet<String>,
    guards: HashSet<String>,
    arguments: HashSet<String>,
}

fn source_names(source: &SyncSource) -> Result<SourceNames, CoreError> {
    let mut names = SourceNames {
        all: source.params.clone(),
        capture: HashSet::new(),
        guards: HashSet::new(),
        arguments: source
            .replacements
            .values()
            .flat_map(|value| crate::capture_names(value, &HashSet::new()))
            .collect(),
    };
    for item in &source.items {
        collect_action_item_names(item, &mut names.all, None);
    }
    // Formals disappear, while public header params read by additions remain
    // foreign values. Only this source's replaced formals start bound here.
    let mut bound = source.replacements.keys().cloned().collect::<HashSet<_>>();
    let items = source.items.clone();
    map_scoped_action_items(items, &mut bound, &HashSet::new(), |item, bound| {
        collect_action_item_names(&item, &mut names.capture, Some(bound));
        match &item {
            ActionItem::Let(name, ..) => {
                names.capture.insert(name.clone());
                names.guards.insert(name.clone());
            }
            ActionItem::Requires(expr, _) => {
                extend_pattern_names(expr, &mut names.guards, true);
            }
            ActionItem::Statement(_) | ActionItem::Ensures(..) => {}
        }
        Ok::<_, CoreError>(item)
    })?;
    Ok(names)
}

fn sync_component_sources(
    action: &SyncAction,
    components: &BTreeMap<String, Component>,
    bound: &HashSet<String>,
) -> Result<(Vec<SyncSource>, Vec<String>), CoreError> {
    let mut sources = Vec::new();
    let mut fair_constituents = Vec::new();
    for reference in &action.refs {
        let mut component = components
            .get(&reference.alias)
            .ok_or_else(|| error_at(format!("unknown alias '{}'", reference.alias), action.span))?
            .clone();
        component.public_bindings.extend(bound.iter().cloned());
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
            span: source_span,
            ..
        } = rewrite_component_item(source, &component)?
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
                    resolve_alias_expr_scoped(arg.clone(), components, action.span, bound)?,
                ))
            })
            .collect::<Result<HashMap<_, _>, CoreError>>()?;
        let item_origins = source_items
            .iter()
            .map(|item| component_item_origin(&component, &reference.action, item))
            .collect();
        sources.push(SyncSource {
            items: source_items,
            item_origins,
            params: param_names(&source_params),
            replacements,
            globals: component.names.clone(),
            span: source_span,
        });
    }
    Ok((sources, fair_constituents))
}

fn sync_action(
    action: &SyncAction,
    components: &BTreeMap<String, Component>,
    composition_names: &ComponentNames,
    ambient: &HashSet<String>,
    warnings: &mut Vec<Value>,
    origins: &mut BTreeMap<String, crate::OriginChain>,
) -> Result<SpecItem, CoreError> {
    let (params, bound) = resolve_alias_params(action.params.clone(), components, action.span)?;
    let additional_source = SyncSource {
        items: resolve_alias_action_items(
            action.items.clone(),
            components,
            &bound,
            ComposeWriteContext::Sync,
        )?,
        item_origins: Vec::new(),
        params: bound.clone(),
        replacements: HashMap::new(),
        globals: composition_names.clone(),
        span: action.span,
    };
    let (mut sources, fair_constituents) = sync_component_sources(action, components, &bound)?;
    sources.push(additional_source);
    let inventories = sources
        .iter()
        .map(source_names)
        .collect::<Result<Vec<_>, _>>()?;
    let protected: HashSet<_> = inventories
        .iter()
        .flat_map(|names| names.arguments.iter().cloned())
        .collect();
    let mut used = ambient.clone();
    used.extend(bound.iter().cloned());
    for actual in sources
        .iter()
        .flat_map(|source| source.replacements.values())
    {
        collect_names(actual, &mut used);
    }
    for names in &inventories {
        used.extend(names.all.iter().cloned());
    }
    let mut pattern_ambient = ambient.clone();
    pattern_ambient.extend(bound.iter().cloned());
    let mut items = Vec::new();
    let mut item_origins = Vec::new();
    for (index, source) in sources.into_iter().enumerate() {
        let mut avoid = protected.clone();
        let mut pattern_avoid = HashSet::new();
        let mut argument_avoid = HashSet::new();
        for (other, names) in inventories.iter().enumerate() {
            argument_avoid.extend(names.guards.iter().cloned());
            if other != index {
                avoid.extend(names.capture.iter().cloned());
                pattern_avoid.extend(names.guards.iter().cloned());
                argument_avoid.extend(names.arguments.iter().cloned());
            }
        }
        item_origins.extend(source.item_origins);
        items.extend(hygiene::rewrite(
            source.items,
            hygiene::Source {
                globals: &source.globals,
                params: &source.params,
                span: source.span,
                arguments: Some(hygiene::Arguments {
                    values: &source.replacements,
                    globals: composition_names,
                    params: &bound,
                    span: action.span,
                    avoid: &argument_avoid,
                }),
            },
            &avoid,
            &pattern_avoid,
            &pattern_ambient,
            &mut used,
        )?);
    }
    record_sync_item_origins(origins, &action.name, &items, item_origins);
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

fn resolve_alias_params(
    params: Vec<Param>,
    components: &BTreeMap<String, Component>,
    span: fsl_syntax::Span,
) -> Result<(Vec<Param>, HashSet<String>), CoreError> {
    let mut bound = HashSet::new();
    let params = params
        .into_iter()
        .map(|param| {
            let param = match param {
                Param::Typed(name, qualified) => Param::Typed(
                    name,
                    resolve_alias_qualified_name(qualified, components, span)?,
                ),
                Param::Range(name, lo, hi) => Param::Range(
                    name,
                    resolve_alias_expr_scoped(lo, components, span, &bound)?,
                    resolve_alias_expr_scoped(hi, components, span, &bound)?,
                ),
            };
            let (Param::Typed(name, _) | Param::Range(name, _, _)) = &param;
            bound.insert(name.clone());
            Ok(param)
        })
        .collect::<Result<_, CoreError>>()?;
    Ok((params, bound))
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
    bound: &HashSet<String>,
) -> Result<(Binder, HashSet<String>), CoreError> {
    let mut nested = bound.clone();
    nested.insert(binder_name(&binder).to_owned());
    let binder = match binder {
        Binder::Typed {
            name,
            type_name,
            where_expr,
        } => Binder::Typed {
            name,
            type_name: resolve_alias_qualified_name(type_name, components, span)?,
            where_expr: where_expr
                .map(|expr| {
                    resolve_alias_expr_scoped(*expr, components, span, &nested).map(Box::new)
                })
                .transpose()?,
        },
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => Binder::Range {
            name,
            lo: Box::new(resolve_alias_expr_scoped(*lo, components, span, bound)?),
            hi: Box::new(resolve_alias_expr_scoped(*hi, components, span, bound)?),
            where_expr: where_expr
                .map(|expr| {
                    resolve_alias_expr_scoped(*expr, components, span, &nested).map(Box::new)
                })
                .transpose()?,
        },
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => Binder::Collection {
            name,
            collection: Box::new(resolve_alias_expr_scoped(
                *collection,
                components,
                span,
                bound,
            )?),
            where_expr: where_expr
                .map(|expr| {
                    resolve_alias_expr_scoped(*expr, components, span, &nested).map(Box::new)
                })
                .transpose()?,
        },
    };
    Ok((binder, nested))
}

fn resolve_alias_expr(
    expr: Expr,
    components: &BTreeMap<String, Component>,
    diagnostic_span: fsl_syntax::Span,
) -> Result<Expr, CoreError> {
    resolve_alias_expr_scoped(expr, components, diagnostic_span, &HashSet::new())
}

fn resolve_alias_expr_scoped(
    expr: Expr,
    components: &BTreeMap<String, Component>,
    diagnostic_span: fsl_syntax::Span,
    bound: &HashSet<String>,
) -> Result<Expr, CoreError> {
    crate::recursion::guard(|| {
        let resolve = |expr| resolve_alias_expr_scoped(expr, components, diagnostic_span, bound);
        Ok(match expr {
            Expr::Field(base, name) => {
                if let Expr::Var(alias) = base.as_ref()
                    && !bound.contains(alias)
                    && components.contains_key(alias)
                {
                    Expr::Var(prefix(alias, &name))
                } else {
                    Expr::Field(Box::new(resolve(*base)?), name)
                }
            }
            Expr::Binary { op, left, right } => {
                let left = resolve(*left)?;
                let nested = pattern_scope(&left, bound, &HashSet::new(), Some(&op));
                Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(resolve_alias_expr_scoped(
                        *right,
                        components,
                        diagnostic_span,
                        &nested,
                    )?),
                }
            }
            Expr::Is { expr, pattern } => Expr::Is {
                expr: Box::new(resolve(*expr)?),
                pattern,
            },
            Expr::Quantified {
                quantifier,
                binder,
                body,
            } => {
                let (binder, nested) =
                    resolve_alias_binder(binder, components, diagnostic_span, bound)?;
                Expr::Quantified {
                    quantifier,
                    binder,
                    body: Box::new(resolve_alias_expr_scoped(
                        *body,
                        components,
                        diagnostic_span,
                        &nested,
                    )?),
                }
            }
            Expr::Aggregate {
                kind,
                binder,
                value,
            } => {
                let (binder, nested) =
                    resolve_alias_binder(binder, components, diagnostic_span, bound)?;
                Expr::Aggregate {
                    kind,
                    binder,
                    value: value
                        .map(|expr| {
                            resolve_alias_expr_scoped(*expr, components, diagnostic_span, &nested)
                                .map(Box::new)
                        })
                        .transpose()?,
                }
            }
            expr @ (Expr::Var(_) | Expr::Stage { .. }) => expr,
            expr => hygiene::map_children(expr, |child, _| resolve(child))?,
        })
    })
}

/// Match the public Kernel's Is/Binary pattern continuation scope.
/// `None` denotes continuation after a true requires guard, not a Binary RHS.
pub(super) fn pattern_scope(
    expr: &Expr,
    bound: &HashSet<String>,
    globals: &HashSet<String>,
    op: Option<&str>,
) -> HashSet<String> {
    let active = op.is_none_or(|op| crate::pattern_binding_flows(op, true));
    let mut patterns = HashSet::new();
    extend_pattern_names(expr, &mut patterns, false);
    let mut nested = bound.clone();
    nested.extend(patterns.difference(globals).filter(|_| active).cloned());
    nested
}

/// Visible exports use true continuations; collision exports retain all Binary names.
/// Extend collision traversal here when additional constructors retain bindings (#1573).
pub(super) fn extend_pattern_names(expr: &Expr, bound: &mut HashSet<String>, collisions: bool) {
    crate::recursion::guard(|| match expr {
        Expr::Is {
            pattern: fsl_syntax::Pattern::Some(name),
            ..
        } => {
            bound.insert(name.clone());
        }
        Expr::Binary { op, left, right }
            if collisions || crate::pattern_binding_flows(op, false) =>
        {
            extend_pattern_names(left, bound, collisions);
            extend_pattern_names(right, bound, collisions);
        }
        _ => {}
    });
}

fn resolve_alias_action_items(
    items: Vec<ActionItem>,
    components: &BTreeMap<String, Component>,
    bound: &HashSet<String>,
    context: ComposeWriteContext,
) -> Result<Vec<ActionItem>, CoreError> {
    let mut bound = bound.clone();
    map_scoped_action_items(items, &mut bound, &HashSet::new(), |item, bound| {
        Ok(match item {
            ActionItem::Requires(expr, span) => {
                let expr = resolve_alias_expr_scoped(expr, components, span, bound)?;
                ActionItem::Requires(expr, span)
            }
            ActionItem::Ensures(expr, span) => ActionItem::Ensures(
                resolve_alias_expr_scoped(expr, components, span, bound)?,
                span,
            ),
            ActionItem::Let(name, expr, span) => {
                let expr = resolve_alias_expr_scoped(expr, components, span, bound)?;
                ActionItem::Let(name, expr, span)
            }
            ActionItem::Statement(statement) => ActionItem::Statement(resolve_alias_statement(
                statement, components, bound, context,
            )?),
        })
    })
}

fn collect_expression_names(
    expr: &Expr,
    names: &mut HashSet<String>,
    bound: Option<&HashSet<String>>,
) {
    match bound {
        Some(bound) => names.extend(crate::capture_names(expr, bound)),
        None => collect_names(expr, names),
    }
}

fn collect_action_item_names(
    item: &ActionItem,
    names: &mut HashSet<String>,
    bound: Option<&HashSet<String>>,
) {
    match item {
        ActionItem::Requires(expr, _) | ActionItem::Ensures(expr, _) => {
            collect_expression_names(expr, names, bound);
        }
        ActionItem::Let(name, expr, _) => {
            if bound.is_none() {
                names.insert(name.clone());
            }
            collect_expression_names(expr, names, bound);
        }
        ActionItem::Statement(statement) => collect_statement_names(statement, names, bound),
    }
}

fn collect_statement_names(
    statement: &Statement,
    names: &mut HashSet<String>,
    bound: Option<&HashSet<String>>,
) {
    match statement {
        Statement::Assign { target, value, .. } => {
            collect_lvalue_reads(target, names, bound);
            collect_expression_names(value, names, bound);
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            collect_expression_names(condition, names, bound);
            for statement in then_statements.iter().chain(else_statements) {
                collect_statement_names(statement, names, bound);
            }
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            let nested = bound.map(|bound| {
                let (free, nested) = crate::capture_binder_names(binder, bound);
                names.extend(free);
                nested
            });
            if bound.is_none() {
                collect_binder_names(binder, names);
            }
            for statement in statements {
                collect_statement_names(statement, names, nested.as_ref());
            }
        }
    }
}

fn collect_lvalue_reads(
    lvalue: &LValue,
    names: &mut HashSet<String>,
    bound: Option<&HashSet<String>>,
) {
    match lvalue {
        LValue::Var(_) => {}
        LValue::Index(_, expr) => {
            collect_expression_names(expr, names, bound);
        }
        LValue::Field(base, _) => collect_lvalue_reads(base, names, bound),
    }
}
