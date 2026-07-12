// SPDX-License-Identifier: Apache-2.0

//! Typestate derivation from the typed FSL kernel.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    KernelExpr as Expr, KernelLValue as LValue, KernelModel, KernelStatement as Statement,
    ParamDef, Pattern, TypeDef, TypeRef,
};
use serde_json::{Map, Value, json};

const EMPTY: &str = "Empty";
const FILLED: &str = "Filled";

type StateMap = BTreeMap<String, BTreeSet<String>>;

#[derive(Clone, Copy)]
enum EnumLocation<'a> {
    Field(&'a str),
    Var(&'a str),
}

#[derive(Clone)]
struct Assignment {
    entity: String,
    to: String,
    conditional: bool,
    branch_states: StateMap,
}

#[derive(Clone)]
struct Entity {
    kind: &'static str,
    type_name: String,
    field: Option<String>,
    var: Option<String>,
    enum_name: Option<String>,
    states: Vec<String>,
    data_fields: Vec<(String, TypeRef)>,
}

fn display_name(name: &str) -> String {
    name.replacen("__", ".", 1)
}

fn expr_source(expr: &Expr) -> String {
    match expr {
        Expr::Var(name) => display_name(name),
        Expr::Num(value) => value.to_string(),
        Expr::Bool(value) => value.to_string(),
        Expr::None => "none".to_owned(),
        Expr::Binary { op, left, right } => {
            format!("({} {op} {})", expr_source(left), expr_source(right))
        }
        Expr::Not(value) => format!("not {}", expr_source(value)),
        Expr::Neg(value) => format!("-{}", expr_source(value)),
        Expr::Field(base, field) => format!("{}.{field}", expr_source(base)),
        Expr::Index(base, index) => format!("{}[{}]", expr_source(base), expr_source(index)),
        Expr::Method { receiver, name, .. } => format!("{}.{name}(...)", expr_source(receiver)),
        Expr::Some(value) => format!("some({})", expr_source(value)),
        Expr::Is { expr, pattern } => format!(
            "{} is {}",
            expr_source(expr),
            match pattern {
                Pattern::None => "none",
                Pattern::Some(_) => "some(...)",
            }
        ),
        Expr::Set(_) => "set_lit".to_owned(),
        Expr::Seq(_) => "seq_lit".to_owned(),
        Expr::Struct { .. } => "struct_lit".to_owned(),
        Expr::Call { .. } => "call".to_owned(),
        Expr::IfThenElse { .. } => "if_expr".to_owned(),
        Expr::Quantified { .. } => "quant".to_owned(),
        Expr::Count { .. } => "count".to_owned(),
        Expr::Sum { .. } => "sum".to_owned(),
        Expr::UnaryNamed { name, .. }
        | Expr::BinaryNamed { name, .. }
        | Expr::TernaryNamed { name, .. }
        | Expr::BinderNamed { name, .. } => name.clone(),
    }
}

fn lvalue_source(value: &LValue) -> String {
    match value {
        LValue::Var(name) => display_name(name),
        LValue::Index(name, index) => format!("{}[{}]", display_name(name), expr_source(index)),
        LValue::Field(base, field) => format!("{}.{field}", lvalue_source(base)),
    }
}

fn expr_base_var(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Var(name) => Some(name),
        Expr::Index(base, _) | Expr::Field(base, _) => expr_base_var(base),
        _ => None,
    }
}

fn lvalue_base_var(value: &LValue) -> &str {
    match value {
        LValue::Var(name) | LValue::Index(name, _) => name,
        LValue::Field(base, _) => lvalue_base_var(base),
    }
}

fn enum_expr_location(expr: &Expr, location: EnumLocation<'_>) -> Option<String> {
    match location {
        EnumLocation::Field(expected) => match expr {
            Expr::Field(base, field) if field == expected => Some(expr_source(base)),
            _ => None,
        },
        EnumLocation::Var(expected) if expr_base_var(expr) == Some(expected) => match expr {
            Expr::Var(_) | Expr::Index(_, _) => Some(expr_source(expr)),
            _ => None,
        },
        EnumLocation::Var(_) => None,
    }
}

fn enum_lvalue_location(value: &LValue, location: EnumLocation<'_>) -> Option<String> {
    match location {
        EnumLocation::Field(expected) => match value {
            LValue::Field(base, field) if field == expected => Some(lvalue_source(base)),
            _ => None,
        },
        EnumLocation::Var(expected) if lvalue_base_var(value) == expected => match value {
            LValue::Var(_) | LValue::Index(_, _) => Some(lvalue_source(value)),
            LValue::Field(_, _) => None,
        },
        EnumLocation::Var(_) => None,
    }
}

fn merge_states(target: &mut StateMap, incoming: StateMap) {
    for (entity, states) in incoming {
        target.entry(entity).or_default().extend(states);
    }
}

fn and_states(base: &StateMap, constraint: &StateMap) -> StateMap {
    let mut result = base.clone();
    for (entity, states) in constraint {
        result
            .entry(entity.clone())
            .and_modify(|current| current.retain(|state| states.contains(state)))
            .or_insert_with(|| states.clone());
    }
    result
}

fn enum_guard_states(
    expr: &Expr,
    location: EnumLocation<'_>,
    members: &BTreeSet<String>,
) -> StateMap {
    let mut result = StateMap::new();
    match expr {
        Expr::Binary { op, left, right } if op == "or" || op == "and" => {
            merge_states(&mut result, enum_guard_states(left, location, members));
            merge_states(&mut result, enum_guard_states(right, location, members));
        }
        Expr::Binary { op, left, right } if op == "==" => {
            for (candidate, value) in [(left.as_ref(), right.as_ref()), (right, left)] {
                if let Some(entity) = enum_expr_location(candidate, location)
                    && let Expr::Var(member) = value
                    && members.contains(member)
                {
                    result.entry(entity).or_default().insert(member.clone());
                }
            }
        }
        _ => {}
    }
    result
}

fn enum_is_state_only(expr: &Expr, location: EnumLocation<'_>) -> bool {
    match expr {
        Expr::Binary { op, left, right } if op == "and" || op == "or" => {
            enum_is_state_only(left, location) && enum_is_state_only(right, location)
        }
        Expr::Binary { op, left, right } if op == "==" => {
            enum_expr_location(left, location).is_some()
                || enum_expr_location(right, location).is_some()
        }
        _ => false,
    }
}

fn enum_assignments(
    statements: &[Statement],
    location: EnumLocation<'_>,
    members: &BTreeSet<String>,
    field: Option<&str>,
) -> Vec<Assignment> {
    fn walk(
        statement: &Statement,
        location: EnumLocation<'_>,
        members: &BTreeSet<String>,
        field: Option<&str>,
        conditional: bool,
        branch_states: &StateMap,
        output: &mut Vec<Assignment>,
    ) {
        match statement {
            Statement::Assign { target, value, .. } => {
                if let Some(entity) = enum_lvalue_location(target, location)
                    && let Expr::Var(member) = value
                    && members.contains(member)
                {
                    output.push(Assignment {
                        entity,
                        to: member.clone(),
                        conditional,
                        branch_states: branch_states.clone(),
                    });
                } else if let Some(field) = field
                    && let Expr::Struct { fields, .. } = value
                    && matches!(target, LValue::Var(_) | LValue::Index(_, _))
                    && let Some((_, Expr::Var(member))) =
                        fields.iter().find(|(name, _)| name == field)
                    && members.contains(member)
                {
                    output.push(Assignment {
                        entity: lvalue_source(target),
                        to: member.clone(),
                        conditional,
                        branch_states: branch_states.clone(),
                    });
                }
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                let condition_states = enum_guard_states(condition, location, members);
                let then_states = and_states(branch_states, &condition_states);
                let mut else_condition = StateMap::new();
                if enum_is_state_only(condition, location) {
                    for (entity, states) in condition_states {
                        else_condition
                            .insert(entity, members.difference(&states).cloned().collect());
                    }
                }
                let else_states = and_states(branch_states, &else_condition);
                for child in then_statements {
                    walk(child, location, members, field, true, &then_states, output);
                }
                for child in else_statements {
                    walk(child, location, members, field, true, &else_states, output);
                }
            }
            Statement::ForAll { statements, .. } => {
                for child in statements {
                    walk(
                        child,
                        location,
                        members,
                        field,
                        conditional,
                        branch_states,
                        output,
                    );
                }
            }
        }
    }

    let mut output = Vec::new();
    for statement in statements {
        walk(
            statement,
            location,
            members,
            field,
            false,
            &StateMap::new(),
            &mut output,
        );
    }
    output
}

fn option_guard_states(expr: &Expr, var: &str) -> StateMap {
    let mut result = StateMap::new();
    match expr {
        Expr::Binary { op, left, right } if op == "or" || op == "and" => {
            merge_states(&mut result, option_guard_states(left, var));
            merge_states(&mut result, option_guard_states(right, var));
        }
        Expr::Binary { op, left, right } if op == "==" || op == "!=" => {
            for (candidate, value) in [(left.as_ref(), right.as_ref()), (right, left)] {
                if expr_base_var(candidate) == Some(var) && matches!(value, Expr::None) {
                    let state = if op == "==" { EMPTY } else { FILLED };
                    result
                        .entry(expr_source(candidate))
                        .or_default()
                        .insert(state.to_owned());
                }
            }
        }
        Expr::Is { expr, pattern } if expr_base_var(expr) == Some(var) => {
            let state = if matches!(pattern, Pattern::None) {
                EMPTY
            } else {
                FILLED
            };
            result
                .entry(expr_source(expr))
                .or_default()
                .insert(state.to_owned());
        }
        _ => {}
    }
    result
}

fn option_is_state_only(expr: &Expr, var: &str) -> bool {
    match expr {
        Expr::Binary { op, left, right } if op == "and" || op == "or" => {
            option_is_state_only(left, var) && option_is_state_only(right, var)
        }
        Expr::Binary { op, left, right } if op == "==" || op == "!=" => {
            (expr_base_var(left) == Some(var) && matches!(right.as_ref(), Expr::None))
                || (expr_base_var(right) == Some(var) && matches!(left.as_ref(), Expr::None))
        }
        Expr::Is { expr, .. } => expr_base_var(expr) == Some(var),
        _ => false,
    }
}

fn option_assignments(statements: &[Statement], var: &str) -> Vec<Assignment> {
    fn walk(
        statement: &Statement,
        var: &str,
        conditional: bool,
        branch_states: &StateMap,
        output: &mut Vec<Assignment>,
    ) {
        match statement {
            Statement::Assign { target, value, .. }
                if lvalue_base_var(target) == var
                    && matches!(target, LValue::Var(_) | LValue::Index(_, _)) =>
            {
                let to = match value {
                    Expr::None => Some(EMPTY),
                    Expr::Some(_) => Some(FILLED),
                    _ => None,
                };
                if let Some(to) = to {
                    output.push(Assignment {
                        entity: lvalue_source(target),
                        to: to.to_owned(),
                        conditional,
                        branch_states: branch_states.clone(),
                    });
                }
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                let condition_states = option_guard_states(condition, var);
                let then_states = and_states(branch_states, &condition_states);
                let mut else_condition = StateMap::new();
                if option_is_state_only(condition, var) {
                    let universe = BTreeSet::from([EMPTY.to_owned(), FILLED.to_owned()]);
                    for (entity, states) in condition_states {
                        else_condition
                            .insert(entity, universe.difference(&states).cloned().collect());
                    }
                }
                let else_states = and_states(branch_states, &else_condition);
                for child in then_statements {
                    walk(child, var, true, &then_states, output);
                }
                for child in else_statements {
                    walk(child, var, true, &else_states, output);
                }
            }
            Statement::ForAll { statements, .. } => {
                for child in statements {
                    walk(child, var, conditional, branch_states, output);
                }
            }
            Statement::Assign { .. } => {}
        }
    }

    let mut output = Vec::new();
    for statement in statements {
        walk(statement, var, false, &StateMap::new(), &mut output);
    }
    output
}

fn requirement(action: &fsl_core::ActionDef) -> Option<Value> {
    action
        .meta
        .as_ref()
        .map(|meta| json!({"id": meta.id, "text": meta.text}))
}

fn classify_action(
    action: &fsl_core::ActionDef,
    guard_states: impl Fn(&Expr) -> StateMap,
    assignments: Vec<Assignment>,
    is_state_only: impl Fn(&Expr) -> bool,
) -> Option<Value> {
    if assignments.is_empty() {
        return None;
    }

    let mut guards = StateMap::new();
    for expr in &action.requires {
        merge_states(&mut guards, guard_states(expr));
    }

    let mut transitions = Vec::new();
    let mut verdict = "derivable";
    let mut diagnostics = Vec::new();
    for assignment in assignments {
        let require_from = guards.get(&assignment.entity).cloned().unwrap_or_default();
        let branch_from = assignment
            .branch_states
            .get(&assignment.entity)
            .cloned()
            .unwrap_or_default();
        let from: BTreeSet<_> = if !require_from.is_empty() && !branch_from.is_empty() {
            require_from.intersection(&branch_from).cloned().collect()
        } else {
            require_from.union(&branch_from).cloned().collect()
        };
        if from.is_empty() {
            verdict = "relational";
            diagnostics.push(format!(
                "assigns `{} → {}` but no local `requires` pins its from-state; the precondition is relational (it lives outside the entity), so it cannot be carried by a phantom type and remains a runtime/verification obligation.",
                assignment.entity, assignment.to
            ));
        } else if assignment.conditional && verdict != "relational" {
            verdict = "branching";
            diagnostics.push(format!(
                "`{} → {}` is inside an `if` (data-dependent target).",
                assignment.entity, assignment.to
            ));
        }
        transitions.push(json!({
            "entity": assignment.entity,
            "from": from,
            "to": assignment.to,
            "conditional": assignment.conditional,
        }));
    }

    let mut output = Map::new();
    output.insert("action".to_owned(), json!(display_name(&action.name)));
    output.insert("verdict".to_owned(), json!(verdict));
    output.insert(
        "params".to_owned(),
        json!(action.params.iter().map(ParamDef::name).collect::<Vec<_>>()),
    );
    output.insert("transitions".to_owned(), Value::Array(transitions));
    output.insert(
        "value_preconditions".to_owned(),
        json!(
            action
                .requires
                .iter()
                .filter(|expr| !is_state_only(expr))
                .map(expr_source)
                .collect::<Vec<_>>()
        ),
    );
    if let Some(meta) = requirement(action) {
        output.insert("requirement".to_owned(), meta);
    }
    if !diagnostics.is_empty() {
        output.insert("diagnostics".to_owned(), json!(diagnostics));
    }
    Some(Value::Object(output))
}

fn ts_type(model: &KernelModel, ty: &TypeRef) -> String {
    match ty {
        TypeRef::Int | TypeRef::Range(_, _) => "number".to_owned(),
        TypeRef::Bool => "boolean".to_owned(),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { .. }) => "number".to_owned(),
            Some(TypeDef::Enum { .. } | TypeDef::Struct { .. }) => display_name(name),
            None => "unknown".to_owned(),
        },
        TypeRef::Option(inner) => format!("{} | null", ts_type(model, inner)),
        TypeRef::Set(inner) => format!("Set<{}>", ts_type(model, inner)),
        _ => "unknown".to_owned(),
    }
}

#[allow(clippy::too_many_lines)]
fn emit_typescript(model: &KernelModel, entity: &Entity, actions: &[Value], note: &str) -> String {
    let state_type = format!("{}State", entity.type_name);
    let mut lines = vec![
        format!(
            "// Typestate skeleton for `{}` from spec `{}`.",
            entity.type_name,
            display_name(&model.name)
        ),
        format!("// {note}"),
        "// Only transitions with a LOCAL from-state guard are typed; the rest stay dynamic."
            .to_owned(),
        String::new(),
        format!(
            "export type {state_type} = {};",
            entity
                .states
                .iter()
                .map(|state| format!("\"{state}\""))
                .collect::<Vec<_>>()
                .join(" | ")
        ),
        String::new(),
        "declare const __state: unique symbol;".to_owned(),
        format!(
            "export interface {}<S extends {state_type}> {{",
            entity.type_name
        ),
    ];
    for (name, ty) in &entity.data_fields {
        lines.push(format!("  {name}: {};", ts_type(model, ty)));
    }
    lines.extend([
        "  readonly [__state]: S;".to_owned(),
        "}".to_owned(),
        String::new(),
    ]);

    for action in actions {
        let verdict = action["verdict"].as_str().unwrap_or_default();
        if verdict != "derivable" && verdict != "branching" {
            continue;
        }
        let mut from = BTreeSet::new();
        let mut to = BTreeSet::new();
        for transition in action["transitions"].as_array().into_iter().flatten() {
            from.extend(
                transition["from"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            );
            if let Some(state) = transition["to"].as_str() {
                to.insert(state);
            }
        }
        let from_type = if from.is_empty() {
            state_type.clone()
        } else {
            from.into_iter()
                .map(|state| format!("\"{state}\""))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let to_type = if to.is_empty() {
            from_type.clone()
        } else {
            to.into_iter()
                .map(|state| format!("\"{state}\""))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let value_preconditions = action["value_preconditions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        if !value_preconditions.is_empty() {
            lines.push(format!(
                "  // runtime precondition (not in type): {}",
                value_preconditions.join("; ")
            ));
        }
        if verdict == "branching" {
            lines.push(
                "  // branching: to-state is data-dependent; verify exhaustiveness at the impl"
                    .to_owned(),
            );
        }
        let mut function_name = action["action"].as_str().unwrap_or_default().to_owned();
        if matches!(
            function_name.as_str(),
            "void"
                | "delete"
                | "new"
                | "default"
                | "function"
                | "return"
                | "switch"
                | "case"
                | "class"
                | "enum"
                | "interface"
                | "type"
        ) {
            function_name.push('_');
        }
        let params = action["params"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|param| format!("{param}: number"))
            .collect::<Vec<_>>();
        let extra = if params.is_empty() {
            String::new()
        } else {
            format!(", {}", params.join(", "))
        };
        lines.push(format!(
            "export function {function_name}(self: {}<{from_type}>{extra}): {}<{to_type}>;",
            entity.type_name, entity.type_name
        ));
    }
    lines.join("\n")
}

fn camel(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect()
}

fn discover_entities(model: &KernelModel) -> Vec<Entity> {
    let mut entities = Vec::new();
    for (type_name, definition) in &model.types {
        let TypeDef::Struct { fields } = definition else {
            continue;
        };
        for (field, ty) in fields {
            let TypeRef::Named(enum_name) = ty else {
                continue;
            };
            let Some(TypeDef::Enum { members, .. }) = model.types.get(enum_name) else {
                continue;
            };
            entities.push(Entity {
                kind: "enum",
                type_name: display_name(type_name),
                field: Some(field.clone()),
                var: None,
                enum_name: Some(display_name(enum_name)),
                states: members.clone(),
                data_fields: fields
                    .iter()
                    .filter(|(name, _)| name != field)
                    .cloned()
                    .collect(),
            });
        }
    }

    for (var, ty) in &model.state {
        let enum_name = match ty {
            TypeRef::Named(name) if matches!(model.types.get(name), Some(TypeDef::Enum { .. })) => {
                Some(name)
            }
            TypeRef::Map(_, value) => match value.as_ref() {
                TypeRef::Named(name)
                    if matches!(model.types.get(name), Some(TypeDef::Enum { .. })) =>
                {
                    Some(name)
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(enum_name) = enum_name
            && let Some(TypeDef::Enum { members, .. }) = model.types.get(enum_name)
        {
            entities.push(Entity {
                kind: "enum",
                type_name: camel(&display_name(var)),
                field: None,
                var: Some(var.clone()),
                enum_name: Some(display_name(enum_name)),
                states: members.clone(),
                data_fields: Vec::new(),
            });
        }
    }

    for (var, ty) in &model.state {
        let inner = match ty {
            TypeRef::Option(inner) => Some(inner.as_ref()),
            TypeRef::Map(_, value) => match value.as_ref() {
                TypeRef::Option(inner) => Some(inner.as_ref()),
                _ => None,
            },
            _ => None,
        };
        if let Some(inner) = inner {
            let public_var = display_name(var);
            let mut chars = public_var.chars();
            let type_name = chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            });
            entities.push(Entity {
                kind: "option",
                type_name,
                field: None,
                var: Some(var.clone()),
                enum_name: None,
                states: vec![EMPTY.to_owned(), FILLED.to_owned()],
                data_fields: vec![("value".to_owned(), TypeRef::Option(Box::new(inner.clone())))],
            });
        }
    }
    entities
}

/// Derive sound host-language typestate slices from a checked FSL model.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn analyze_typestate(model: &KernelModel) -> Value {
    let mut report_entities = Vec::new();
    for entity in discover_entities(model) {
        let (actions, key, note) = if entity.kind == "enum" {
            let location = entity.field.as_deref().map_or_else(
                || EnumLocation::Var(entity.var.as_deref().unwrap_or_default()),
                EnumLocation::Field,
            );
            let members: BTreeSet<_> = entity.states.iter().cloned().collect();
            let actions = model
                .actions
                .iter()
                .filter_map(|action| {
                    classify_action(
                        action,
                        |expr| enum_guard_states(expr, location, &members),
                        enum_assignments(
                            &action.statements,
                            location,
                            &members,
                            entity.field.as_deref(),
                        ),
                        |expr| enum_is_state_only(expr, location),
                    )
                })
                .collect::<Vec<_>>();
            let key = entity.field.as_ref().map_or_else(
                || {
                    format!(
                        "{} ({})",
                        display_name(entity.var.as_deref().unwrap_or_default()),
                        entity.enum_name.as_deref().unwrap_or_default()
                    )
                },
                |field| format!("{}.{field}", entity.type_name),
            );
            let note = "FSL holds these in a collection; phantom types track one entity, so each becomes an independently-typed handle.".to_owned();
            (actions, key, note)
        } else {
            let var = entity.var.as_deref().unwrap_or_default();
            let actions = model
                .actions
                .iter()
                .filter_map(|action| {
                    classify_action(
                        action,
                        |expr| option_guard_states(expr, var),
                        option_assignments(&action.statements, var),
                        |expr| option_is_state_only(expr, var),
                    )
                })
                .collect::<Vec<_>>();
            (
                actions,
                format!("{} (Option)", display_name(var)),
                format!(
                    "`{}` is an Option slot; states are Empty (none) / Filled (some).",
                    display_name(var)
                ),
            )
        };
        if actions.is_empty() {
            continue;
        }
        let derivable = actions
            .iter()
            .filter(|action| action["verdict"] == "derivable")
            .count();
        let relational = actions
            .iter()
            .filter(|action| action["verdict"] == "relational")
            .count();
        let applicability = if relational == 0 {
            "full"
        } else if derivable == 0
            && actions
                .iter()
                .all(|action| action["verdict"] == "relational")
        {
            "none"
        } else {
            "partial"
        };
        let typescript = emit_typescript(model, &entity, &actions, &note);
        report_entities.push(json!({
            "entity": key,
            "kind": entity.kind,
            "enum": entity.enum_name,
            "states": entity.states,
            "applicability": applicability,
            "actions": actions,
            "typescript": typescript,
        }));
    }

    let full = report_entities
        .iter()
        .filter(|entity| entity["applicability"] == "full")
        .count();
    let partial = report_entities
        .iter()
        .filter(|entity| entity["applicability"] == "partial")
        .count();
    let none = report_entities
        .iter()
        .filter(|entity| entity["applicability"] == "none")
        .count();
    let mut report = Map::new();
    report.insert("result".to_owned(), json!("typestate"));
    report.insert("spec".to_owned(), json!(display_name(&model.name)));
    report.insert("entities".to_owned(), Value::Array(report_entities));
    report.insert(
        "summary".to_owned(),
        json!({"entities": full + partial + none, "full": full, "partial": partial, "none": none}),
    );
    if full + partial + none == 0 {
        report.insert(
            "note".to_owned(),
            json!("no enum-field or Option state machine found — nothing to derive."),
        );
    }
    Value::Object(report)
}
