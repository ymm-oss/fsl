// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;

use fsl_syntax::{
    ActionTarget, Binder, Expr, QualifiedName, RefinementItem, RefinementParam, RequirementAction,
    RequirementActionItem, RequirementBlockItem, RequirementsItem, Span, SurfaceDocument,
    SurfaceRefinement,
};

use crate::{FileResolver, KernelModel, ParamDef, TypeRef, build_model, parse_kernel_source};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateMap {
    pub name: String,
    pub binder: Option<Binder>,
    pub expr: Expr,
    pub span: Option<Span>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionMapTarget {
    Stutter,
    Action { name: String, args: Vec<Expr> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionMap {
    pub name: String,
    pub params: Vec<String>,
    pub target: ActionMapTarget,
    pub span: Option<Span>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressMap {
    pub leads_to: String,
    pub actions: Vec<String>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refinement {
    pub name: String,
    pub impl_name: String,
    pub abs_name: String,
    pub state_maps: BTreeMap<String, StateMap>,
    pub action_maps: BTreeMap<String, ActionMap>,
    pub progress: Vec<ProgressMap>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementsContract {
    pub abstraction: KernelModel,
    pub refinement: Refinement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementError {
    pub message: String,
    pub span: Option<Span>,
}

impl fmt::Display for RefinementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RefinementError {}

/// Parse and validate a refinement mapping against its implementation and
/// abstract kernel models.
///
/// # Errors
///
/// Returns [`RefinementError`] for syntax errors, mismatched model names,
/// duplicate/missing maps, or unknown action correspondences.
pub fn parse_refinement(
    source: &str,
    implementation: &KernelModel,
    abstraction: &KernelModel,
) -> Result<Refinement, RefinementError> {
    let document = fsl_syntax::parse_surface_document(source).map_err(|error| RefinementError {
        message: error.message,
        span: Some(error.span),
    })?;
    let SurfaceDocument::Refinement(surface) = document else {
        return Err(refinement_error("expected refinement document", None));
    };
    build_refinement(surface, implementation, abstraction)
}

#[allow(clippy::too_many_lines)]
fn build_refinement(
    surface: SurfaceRefinement,
    implementation: &KernelModel,
    abstraction: &KernelModel,
) -> Result<Refinement, RefinementError> {
    let mut impl_name = None;
    let mut abs_name = None;
    let mut maps_auto = false;
    let mut state_maps = BTreeMap::new();
    let mut action_maps = BTreeMap::new();
    let mut progress = Vec::new();
    for item in surface.items {
        match item {
            RefinementItem::Impl(name) => impl_name = Some(name),
            RefinementItem::Abs(name) => abs_name = Some(name),
            RefinementItem::MapsAuto(_) => maps_auto = true,
            RefinementItem::Map {
                name,
                binder,
                expr,
                span,
            } => {
                if abstraction.state_type(&name).is_none() {
                    return Err(refinement_error(
                        format!("unknown abstract state variable '{name}'"),
                        Some(span),
                    ));
                }
                if state_maps
                    .insert(
                        name.clone(),
                        StateMap {
                            name: name.clone(),
                            binder,
                            expr: *expr,
                            span: Some(span),
                        },
                    )
                    .is_some()
                {
                    return Err(refinement_error(
                        format!("duplicate map for '{name}'"),
                        Some(span),
                    ));
                }
            }
            RefinementItem::Action {
                name,
                params,
                target,
                span,
            } => {
                let Some(action) = implementation
                    .actions
                    .iter()
                    .find(|action| action.name == name)
                else {
                    return Err(refinement_error(
                        format!("unknown impl action '{name}'"),
                        Some(span),
                    ));
                };
                let param_names = params
                    .iter()
                    .map(|param| param.name.clone())
                    .collect::<Vec<_>>();
                let expected = action
                    .params
                    .iter()
                    .map(|param| param.name().to_owned())
                    .collect::<Vec<_>>();
                if param_names != expected {
                    return Err(refinement_error(
                        format!(
                            "action '{name}' parameter names/order must match impl ({expected:?})"
                        ),
                        Some(span),
                    ));
                }
                let target = match target {
                    ActionTarget::Stutter => ActionMapTarget::Stutter,
                    ActionTarget::Action(target, args) => {
                        let Some(abs_action) = abstraction
                            .actions
                            .iter()
                            .find(|action| action.name == target)
                        else {
                            return Err(refinement_error(
                                format!("unknown abstract action '{target}'"),
                                Some(span),
                            ));
                        };
                        if args.len() != abs_action.params.len() {
                            return Err(refinement_error(
                                format!(
                                    "action '{name}' -> '{target}' expects {} arguments",
                                    abs_action.params.len()
                                ),
                                Some(span),
                            ));
                        }
                        ActionMapTarget::Action { name: target, args }
                    }
                };
                if action_maps
                    .insert(
                        name.clone(),
                        ActionMap {
                            name: name.clone(),
                            params: param_names,
                            target,
                            span: Some(span),
                        },
                    )
                    .is_some()
                {
                    return Err(refinement_error(
                        format!("duplicate action map for '{name}'"),
                        Some(span),
                    ));
                }
            }
            RefinementItem::PreserveProgress { responses, .. } => {
                progress.extend(responses.into_iter().map(|(leads_to, actions, span)| {
                    ProgressMap {
                        leads_to,
                        actions,
                        span,
                    }
                }));
            }
        }
    }
    let impl_name =
        impl_name.ok_or_else(|| refinement_error("refinement missing impl spec name", None))?;
    let abs_name =
        abs_name.ok_or_else(|| refinement_error("refinement missing abs spec name", None))?;
    if impl_name != implementation.name {
        return Err(refinement_error(
            format!(
                "impl name '{impl_name}' does not match impl spec '{}'",
                implementation.name
            ),
            None,
        ));
    }
    if abs_name != abstraction.name {
        return Err(refinement_error(
            format!(
                "abs name '{abs_name}' does not match abs spec '{}'",
                abstraction.name
            ),
            None,
        ));
    }
    if maps_auto {
        apply_auto_maps(
            implementation,
            abstraction,
            &mut state_maps,
            &mut action_maps,
        )?;
    }
    validate_refinement_expressions(implementation, abstraction, &state_maps, &action_maps)?;
    for (name, _) in &abstraction.state {
        if !state_maps.contains_key(name) {
            return Err(refinement_error(
                format!("missing map for abstract state variable '{name}'"),
                None,
            ));
        }
    }
    for action in &implementation.actions {
        if !action_maps.contains_key(&action.name) {
            return Err(refinement_error(
                format!(
                    "missing action correspondence for impl action '{}'",
                    action.name
                ),
                None,
            ));
        }
    }
    for declaration in &progress {
        if !abstraction
            .leadstos
            .iter()
            .any(|property| property.name == declaration.leads_to)
        {
            return Err(refinement_error(
                format!("unknown abstract leadsTo '{}'", declaration.leads_to),
                Some(declaration.span),
            ));
        }
        for action in &declaration.actions {
            if !implementation
                .actions
                .iter()
                .any(|candidate| candidate.name == *action)
            {
                return Err(refinement_error(
                    format!("unknown impl progress action '{action}'"),
                    Some(declaration.span),
                ));
            }
        }
    }
    Ok(Refinement {
        name: surface.name,
        impl_name,
        abs_name,
        state_maps,
        action_maps,
        progress,
    })
}

fn validate_refinement_expressions(
    implementation: &KernelModel,
    abstraction: &KernelModel,
    state_maps: &BTreeMap<String, StateMap>,
    action_maps: &BTreeMap<String, ActionMap>,
) -> Result<(), RefinementError> {
    let context = refinement_type_context(implementation, abstraction);
    for state_map in state_maps.values() {
        validate_state_map(state_map, abstraction, &context)?;
    }
    for action_map in action_maps.values() {
        validate_action_map(action_map, implementation, abstraction, &context)?;
    }
    Ok(())
}

fn refinement_type_context(implementation: &KernelModel, abstraction: &KernelModel) -> KernelModel {
    let mut context = implementation.clone();
    for (name, value) in &abstraction.consts {
        context.consts.entry(name.clone()).or_insert(value.clone());
    }
    for (name, definition) in &abstraction.types {
        context
            .types
            .entry(name.clone())
            .or_insert(definition.clone());
    }
    for (name, value) in &abstraction.enum_members {
        context
            .enum_members
            .entry(name.clone())
            .or_insert(value.clone());
    }
    context
}

fn validate_state_map(
    state_map: &StateMap,
    abstraction: &KernelModel,
    context: &KernelModel,
) -> Result<(), RefinementError> {
    let mut expected = abstraction
        .state_type(&state_map.name)
        .expect("state maps were checked against abstraction state")
        .clone();
    let mut bindings = Vec::new();
    if let Some(binder) = &state_map.binder {
        let binder_ty = crate::public_kernel::expression_binder_type(binder, context)
            .map_err(|error| invalid_state_map_at_map(state_map, "binder", &error.message))?;
        let TypeRef::Map(key, value) = expected else {
            return Err(refinement_error(
                format!(
                    "map '{}' has a binder but its abstract state is not a Map",
                    state_map.name
                ),
                state_map.span,
            ));
        };
        let binder_name = match binder {
            Binder::Typed { name, .. }
            | Binder::Range { name, .. }
            | Binder::Collection { name, .. } => name.clone(),
        };
        crate::public_kernel::validate_expression_type(
            &Expr::Var(binder_name.clone()),
            &key,
            &[(binder_name.clone(), binder_ty.clone())],
            context,
        )
        .map_err(|error| invalid_state_map_at_map(state_map, "binder", &error.message))?;
        bindings.push((binder_name, binder_ty));
        expected = *value;
    }
    crate::public_kernel::validate_expression_type(&state_map.expr, &expected, &bindings, context)
        .map_err(|error| {
            invalid_state_map(
                state_map,
                "expression",
                &error.message,
                error
                    .span
                    .expect("mapped expression type errors carry source spans"),
            )
        })
}

fn invalid_state_map_at_map(state_map: &StateMap, part: &str, message: &str) -> RefinementError {
    refinement_error(
        format!("invalid map {part} for '{}': {message}", state_map.name),
        state_map.span,
    )
}

fn invalid_state_map(
    state_map: &StateMap,
    part: &str,
    message: &str,
    span: Span,
) -> RefinementError {
    refinement_error(
        format!("invalid map {part} for '{}': {message}", state_map.name),
        Some(span),
    )
}

fn validate_action_map(
    action_map: &ActionMap,
    implementation: &KernelModel,
    abstraction: &KernelModel,
    context: &KernelModel,
) -> Result<(), RefinementError> {
    let ActionMapTarget::Action { name, args } = &action_map.target else {
        return Ok(());
    };
    let implementation_action = implementation
        .actions
        .iter()
        .find(|action| action.name == action_map.name)
        .expect("action maps were checked against implementation actions");
    let abstraction_action = abstraction
        .actions
        .iter()
        .find(|action| action.name == *name)
        .expect("action maps were checked against abstraction actions");
    let bindings = implementation_action
        .params
        .iter()
        .map(|param| (param.name().to_owned(), parameter_type(param)))
        .collect::<Vec<_>>();
    for (index, (argument, parameter)) in args.iter().zip(&abstraction_action.params).enumerate() {
        crate::public_kernel::validate_expression_type(
            argument,
            &parameter_type(parameter),
            &bindings,
            context,
        )
        .map_err(|error| {
            refinement_error(
                format!(
                    "invalid argument {} for action '{}' -> '{}': {}",
                    index + 1,
                    action_map.name,
                    name,
                    error.message
                ),
                Some(
                    error
                        .span
                        .expect("mapped argument type errors carry source spans"),
                ),
            )
        })?;
    }
    Ok(())
}

fn parameter_type(parameter: &ParamDef) -> TypeRef {
    match parameter {
        ParamDef::Typed { ty, .. } => ty.clone(),
        ParamDef::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
    }
}

fn apply_auto_maps(
    implementation: &KernelModel,
    abstraction: &KernelModel,
    state_maps: &mut BTreeMap<String, StateMap>,
    action_maps: &mut BTreeMap<String, ActionMap>,
) -> Result<(), RefinementError> {
    for (name, abs_ty) in &abstraction.state {
        if state_maps.contains_key(name) || implementation.state_type(name).is_none() {
            continue;
        }
        let (binder, expr) = match abs_ty {
            TypeRef::Map(key, _) => {
                let TypeRef::Named(type_name) = key.as_ref() else {
                    return Err(refinement_error(
                        "Map keys need a named bounded type for per-key identity mapping",
                        None,
                    ));
                };
                let binder = Binder::Typed {
                    name: "_k".to_owned(),
                    type_name: QualifiedName {
                        namespace: None,
                        name: type_name.clone(),
                    },
                    where_expr: None,
                };
                (
                    Some(binder),
                    Expr::Index(
                        Box::new(Expr::Var(name.clone())),
                        Box::new(Expr::Var("_k".to_owned())),
                    ),
                )
            }
            _ => (None, Expr::Var(name.clone())),
        };
        state_maps.insert(
            name.clone(),
            StateMap {
                name: name.clone(),
                binder,
                expr,
                span: None,
            },
        );
    }
    for impl_action in &implementation.actions {
        if action_maps.contains_key(&impl_action.name) {
            continue;
        }
        let Some(abs_action) = abstraction
            .actions
            .iter()
            .find(|action| action.name == impl_action.name)
        else {
            continue;
        };
        if impl_action.params.len() != abs_action.params.len() {
            continue;
        }
        let params = impl_action
            .params
            .iter()
            .map(|param| param.name().to_owned())
            .collect::<Vec<_>>();
        action_maps.insert(
            impl_action.name.clone(),
            ActionMap {
                name: impl_action.name.clone(),
                params: params.clone(),
                target: ActionMapTarget::Action {
                    name: impl_action.name.clone(),
                    args: params.into_iter().map(Expr::Var).collect(),
                },
                span: None,
            },
        );
    }
    Ok(())
}

fn refinement_error(message: impl Into<String>, span: Option<Span>) -> RefinementError {
    RefinementError {
        message: message.into(),
        span,
    }
}

fn append_requirement_action_maps(action: &RequirementAction, items: &mut Vec<RefinementItem>) {
    let params = action
        .params
        .iter()
        .map(|param| RefinementParam {
            name: match param {
                fsl_syntax::Param::Typed(name, _) | fsl_syntax::Param::Range(name, ..) => {
                    name.clone()
                }
            },
            ty: None,
        })
        .collect::<Vec<_>>();
    if let Some(branches) = action.items.iter().find_map(|item| match item {
        RequirementActionItem::Branches { branches, .. } => Some(branches),
        RequirementActionItem::Action(_) => None,
    }) {
        for (index, branch) in branches.iter().enumerate() {
            items.push(RefinementItem::Action {
                name: format!("{}__b{}", action.name, index + 1),
                params: params.clone(),
                target: branch.maps.target.clone(),
                span: branch.maps.span,
            });
        }
    } else if let Some(mapping) = &action.maps {
        items.push(RefinementItem::Action {
            name: action.name.clone(),
            params,
            target: mapping.target.clone(),
            span: mapping.span,
        });
    }
}

/// Resolve a requirements-layer `implements` declaration into the abstract
/// model and its generated refinement mapping.
///
/// # Errors
///
/// Returns [`RefinementError`] for dependency, lowering, or mapping failures.
#[allow(clippy::too_many_lines)]
pub fn requirements_implements(
    source: &str,
    resolver: &dyn FileResolver,
    implementation: &KernelModel,
) -> Result<Option<ImplementsContract>, RefinementError> {
    let document = fsl_syntax::parse_surface_document(source).map_err(|error| RefinementError {
        message: error.message,
        span: Some(error.span),
    })?;
    let SurfaceDocument::Requirements(requirements) = document else {
        return Ok(None);
    };
    let implements = requirements.items.iter().find_map(|item| match item {
        RequirementsItem::Implements {
            name,
            path,
            items,
            span,
        } => Some((name, path, items, *span)),
        _ => None,
    });
    let Some((abs_name, path, explicit, span)) = implements else {
        return Ok(None);
    };
    let abs_source = resolver.read(path).map_err(|error| RefinementError {
        message: error.message,
        span: Some(span),
    })?;
    let kernel = parse_kernel_source(&abs_source, resolver).map_err(|error| RefinementError {
        message: error.message,
        span: Some(span),
    })?;
    let abstraction = build_model(kernel).map_err(|error| RefinementError {
        message: error.message,
        span: Some(span),
    })?;
    if abstraction.name != *abs_name {
        return Err(refinement_error(
            format!(
                "spec name mismatch: expected '{abs_name}', got '{}'",
                abstraction.name
            ),
            Some(span),
        ));
    }
    let mut items = vec![
        RefinementItem::Impl(implementation.name.clone()),
        RefinementItem::Abs(abstraction.name.clone()),
    ];
    items.extend(explicit.iter().cloned());
    for item in &requirements.items {
        match item {
            RequirementsItem::Requirement {
                items: declarations,
                ..
            } => {
                for declaration in declarations {
                    if let RequirementBlockItem::Action(action) = declaration {
                        append_requirement_action_maps(action, &mut items);
                    }
                }
            }
            RequirementsItem::Action(action) => {
                append_requirement_action_maps(action, &mut items);
            }
            _ => {}
        }
    }
    let explicit_maps = explicit
        .iter()
        .filter_map(|item| match item {
            RefinementItem::Map { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    for (name, ty) in &abstraction.state {
        if explicit_maps.contains(name.as_str()) || implementation.state_type(name).is_none() {
            continue;
        }
        let (binder, expr) = match ty {
            TypeRef::Map(key, _) => {
                let TypeRef::Named(type_name) = key.as_ref() else {
                    return Err(refinement_error(
                        "auto state map requires a named Map key type",
                        Some(span),
                    ));
                };
                (
                    Some(Binder::Typed {
                        name: "c".to_owned(),
                        type_name: QualifiedName {
                            namespace: None,
                            name: type_name.clone(),
                        },
                        where_expr: None,
                    }),
                    Expr::Index(
                        Box::new(Expr::Var(name.clone())),
                        Box::new(Expr::Var("c".to_owned())),
                    ),
                )
            }
            _ => (None, Expr::Var(name.clone())),
        };
        items.push(RefinementItem::Map {
            name: name.clone(),
            binder,
            expr: Box::new(expr),
            span,
        });
    }
    let explicit_actions = items
        .iter()
        .filter_map(|item| match item {
            RefinementItem::Action { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    for action in &implementation.actions {
        if explicit_actions.contains(&action.name) {
            continue;
        }
        let params = action
            .params
            .iter()
            .map(|param| RefinementParam {
                name: param.name().to_owned(),
                ty: None,
            })
            .collect::<Vec<_>>();
        let target = abstraction
            .actions
            .iter()
            .find(|candidate| candidate.name == action.name)
            .map_or(ActionTarget::Stutter, |abstract_action| {
                let args = abstract_action
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, param)| {
                        let name = action
                            .params
                            .iter()
                            .find(|candidate| candidate.name() == param.name())
                            .map_or_else(
                                || action.params[index].name().to_owned(),
                                |candidate| candidate.name().to_owned(),
                            );
                        Expr::Var(name)
                    })
                    .collect();
                ActionTarget::Action(abstract_action.name.clone(), args)
            });
        items.push(RefinementItem::Action {
            name: action.name.clone(),
            params,
            target,
            span: action.span,
        });
    }
    let refinement = build_refinement(
        SurfaceRefinement {
            name: format!("{}Implements{}", implementation.name, abstraction.name),
            items,
        },
        implementation,
        &abstraction,
    )?;
    Ok(Some(ImplementsContract {
        abstraction,
        refinement,
    }))
}
