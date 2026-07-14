// SPDX-License-Identifier: Apache-2.0

//! Deterministic Typed Semantic Graph projection for checked kernel models.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    Annotations, KernelBinder as Binder, KernelExpr as Expr, KernelLValue as LValue, KernelModel,
    KernelStatement as Statement, TypeDef, TypeRef,
};
use fsl_syntax::MetaTag;
use serde_json::{Map, Value, json};

use crate::analysis_graph;

fn display(name: &str) -> String {
    name.replacen("__", ".", 1)
}

#[allow(clippy::needless_pass_by_value)]
fn node(id: String, kind: &str, name: Option<String>, loc: Option<Value>) -> Map<String, Value> {
    let mut result = Map::new();
    result.insert("id".to_owned(), json!(id));
    result.insert("kind".to_owned(), json!(kind));
    if let Some(name) = name {
        result.insert("label".to_owned(), json!(display(&name)));
        result.insert("name".to_owned(), json!(name));
    }
    if let Some(loc) = loc {
        result.insert("loc".to_owned(), loc);
    }
    result
}

fn edge(from: &str, kind: &str, to: &str) -> Value {
    json!({
        "id":format!("edge:{from}:{kind}:{to}"),
        "kind":kind,
        "from":from,
        "to":to,
    })
}

fn requirement_metadata(annotations: &Annotations, legacy: Option<&MetaTag>) -> Vec<Value> {
    let requirements = annotations
        .requirements()
        .expect("checked model annotations are valid")
        .into_iter()
        .map(|requirement| json!({"id":requirement.id,"text":requirement.text}))
        .collect::<Vec<_>>();
    if requirements.is_empty() {
        legacy
            .filter(|meta| !meta.id.eq_ignore_ascii_case("undecided"))
            .map_or_else(Vec::new, |meta| {
                vec![json!({"id":meta.id,"text":meta.text})]
            })
    } else {
        requirements
    }
}

fn add_requirement_metadata(
    node: &mut Map<String, Value>,
    annotations: &Annotations,
    legacy: Option<&MetaTag>,
) {
    let requirements = requirement_metadata(annotations, legacy);
    if let Some(first) = requirements.first() {
        node.insert("meta".to_owned(), first.clone());
        node.insert("requirements".to_owned(), Value::Array(requirements));
    }
}

fn public_type(model: &KernelModel, ty: &TypeRef) -> Value {
    match ty {
        TypeRef::Int => json!(["int"]),
        TypeRef::Bool => json!(["bool"]),
        TypeRef::Range(lo, hi) => json!(["domain", lo, hi]),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => json!(["domain", lo, hi]),
            Some(TypeDef::Enum { .. }) => json!(["enum", display(name)]),
            Some(TypeDef::Struct { .. }) => json!(["struct", display(name)]),
            None => json!(["named", display(name)]),
        },
        TypeRef::Map(key, value) => {
            json!(["map", public_type(model, key), public_type(model, value)])
        }
        TypeRef::Relation(left, right) => {
            json!([
                "relation",
                public_type(model, left),
                public_type(model, right)
            ])
        }
        TypeRef::Set(value) => json!(["set", public_type(model, value)]),
        TypeRef::Seq(value, cap) => json!(["seq", public_type(model, value), cap]),
        TypeRef::Option(value) => json!(["option", public_type(model, value)]),
    }
}

fn binder_reads(
    binder: &Binder,
    state: &BTreeSet<String>,
    bound: &BTreeSet<String>,
) -> BTreeSet<String> {
    match binder {
        Binder::Typed { where_expr, .. } => where_expr
            .as_deref()
            .map_or_else(BTreeSet::new, |expr| expr_reads_bound(expr, state, bound)),
        Binder::Range { lo, hi, .. } => {
            let mut reads = expr_reads_bound(lo, state, bound);
            reads.extend(expr_reads_bound(hi, state, bound));
            reads
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            let mut reads = expr_reads_bound(collection, state, bound);
            if let Some(expr) = where_expr {
                reads.extend(expr_reads_bound(expr, state, bound));
            }
            reads
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

fn expr_reads_bound(
    expr: &Expr,
    state: &BTreeSet<String>,
    bound: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut reads = BTreeSet::new();
    match expr {
        Expr::Var(name) => {
            if state.contains(name) && !bound.contains(name) {
                reads.insert(name.clone());
            }
        }
        Expr::Some(value) | Expr::Neg(value) | Expr::Not(value) | Expr::Field(value, _) => {
            reads.extend(expr_reads_bound(value, state, bound));
        }
        Expr::Index(base, index)
        | Expr::BinaryNamed {
            left: base,
            right: index,
            ..
        } => {
            reads.extend(expr_reads_bound(base, state, bound));
            reads.extend(expr_reads_bound(index, state, bound));
        }
        Expr::Method { receiver, args, .. } => {
            reads.extend(expr_reads_bound(receiver, state, bound));
            for arg in args {
                reads.extend(expr_reads_bound(arg, state, bound));
            }
        }
        Expr::Binary { left, right, .. } => {
            reads.extend(expr_reads_bound(left, state, bound));
            reads.extend(expr_reads_bound(right, state, bound));
        }
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => {
            reads.extend(expr_reads_bound(condition, state, bound));
            reads.extend(expr_reads_bound(then_expr, state, bound));
            reads.extend(expr_reads_bound(else_expr, state, bound));
        }
        Expr::Is { expr, .. } | Expr::UnaryNamed { expr, .. } => {
            reads.extend(expr_reads_bound(expr, state, bound));
        }
        Expr::Set(values) | Expr::Seq(values) => {
            for value in values {
                reads.extend(expr_reads_bound(value, state, bound));
            }
        }
        Expr::Struct { fields, .. } => {
            for (_, value) in fields {
                reads.extend(expr_reads_bound(value, state, bound));
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                reads.extend(expr_reads_bound(arg, state, bound));
            }
        }
        Expr::Quantified { binder, body, .. } => {
            let mut next = bound.clone();
            next.insert(binder_name(binder).to_owned());
            reads.extend(binder_reads(binder, state, &next));
            reads.extend(expr_reads_bound(body, state, &next));
        }
        Expr::Count {
            name, condition, ..
        } => {
            let mut next = bound.clone();
            next.insert(name.clone());
            reads.extend(expr_reads_bound(condition, state, &next));
        }
        Expr::Sum {
            name,
            body,
            condition,
            ..
        } => {
            let mut next = bound.clone();
            next.insert(name.clone());
            reads.extend(expr_reads_bound(body, state, &next));
            if let Some(condition) = condition {
                reads.extend(expr_reads_bound(condition, state, &next));
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            reads.extend(expr_reads_bound(first, state, bound));
            reads.extend(expr_reads_bound(second, state, bound));
            reads.extend(expr_reads_bound(third, state, bound));
        }
        Expr::BinderNamed { binder, .. } => reads.extend(binder_reads(binder, state, bound)),
        Expr::Num(_) | Expr::Bool(_) | Expr::None => {}
    }
    reads
}

fn expr_reads(expr: &Expr, state: &BTreeSet<String>) -> BTreeSet<String> {
    expr_reads_bound(expr, state, &BTreeSet::new())
}

fn lvalue_root(value: &LValue) -> &str {
    match value {
        LValue::Var(name) | LValue::Index(name, _) => name,
        LValue::Field(base, _) => lvalue_root(base),
    }
}

fn lvalue_reads(value: &LValue, state: &BTreeSet<String>) -> BTreeSet<String> {
    match value {
        LValue::Index(_, index) => expr_reads(index, state),
        LValue::Field(base, _) => lvalue_reads(base, state),
        LValue::Var(_) => BTreeSet::new(),
    }
}

fn statement_reads(statements: &[Statement], state: &BTreeSet<String>) -> BTreeSet<String> {
    let mut reads = BTreeSet::new();
    for statement in statements {
        match statement {
            Statement::Assign { target, value, .. } => {
                reads.extend(lvalue_reads(target, state));
                reads.extend(expr_reads(value, state));
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                reads.extend(expr_reads(condition, state));
                reads.extend(statement_reads(then_statements, state));
                reads.extend(statement_reads(else_statements, state));
            }
            Statement::ForAll {
                binder, statements, ..
            } => {
                reads.extend(binder_reads(
                    binder,
                    state,
                    &BTreeSet::from([binder_name(binder).to_owned()]),
                ));
                reads.extend(statement_reads(statements, state));
            }
        }
    }
    reads
}

fn assignments<'a>(statements: &'a [Statement], output: &mut Vec<(&'a LValue, &'a Expr, Value)>) {
    for statement in statements {
        match statement {
            Statement::Assign {
                target,
                value,
                span,
            } => output.push((target, value, span.python_loc())),
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                assignments(then_statements, output);
                assignments(else_statements, output);
            }
            Statement::ForAll { statements, .. } => assignments(statements, output),
        }
    }
}

fn physical_names(model: &KernelModel, logical: &str, ty: &TypeRef) -> Vec<String> {
    fn struct_fields(model: &KernelModel, logical: &str, name: &str) -> Vec<String> {
        match model.types.get(name) {
            Some(TypeDef::Struct { fields }) => fields
                .iter()
                .flat_map(|(field, ty)| match ty {
                    TypeRef::Option(_) => vec![
                        format!("{logical}__{field}__present"),
                        format!("{logical}__{field}__value"),
                    ],
                    _ => vec![format!("{logical}__{field}")],
                })
                .collect(),
            _ => vec![logical.to_owned()],
        }
    }
    match ty {
        TypeRef::Option(_) => vec![format!("{logical}__present"), format!("{logical}__value")],
        TypeRef::Seq(_, _) => vec![format!("{logical}__data"), format!("{logical}__len")],
        TypeRef::Named(name) if matches!(model.types.get(name), Some(TypeDef::Struct { .. })) => {
            struct_fields(model, logical, name)
        }
        TypeRef::Map(_, value) => match value.as_ref() {
            TypeRef::Option(_) => vec![format!("{logical}__present"), format!("{logical}__value")],
            TypeRef::Named(name)
                if matches!(model.types.get(name), Some(TypeDef::Struct { .. })) =>
            {
                struct_fields(model, logical, name)
            }
            _ => vec![logical.to_owned()],
        },
        _ => vec![logical.to_owned()],
    }
}

struct Builder<'a> {
    model: &'a KernelModel,
    state: BTreeSet<String>,
    nodes: BTreeMap<String, Value>,
    edges: BTreeMap<String, Value>,
}

impl<'a> Builder<'a> {
    fn new(model: &'a KernelModel) -> Self {
        Self {
            model,
            state: model.state.iter().map(|(name, _)| name.clone()).collect(),
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
        }
    }
    fn add_node(&mut self, value: Map<String, Value>, declared: bool) {
        let id = value["id"].as_str().unwrap_or_default().to_owned();
        self.nodes.insert(id.clone(), Value::Object(value));
        if declared {
            self.add_edge(edge(&format!("spec:{}", self.model.name), "declares", &id));
        }
    }
    fn add_edge(&mut self, value: Value) {
        if let Some(id) = value["id"].as_str() {
            self.edges.insert(id.to_owned(), value);
        }
    }
    fn add_reads(&mut self, source: &str, reads: &BTreeSet<String>) {
        for name in reads {
            self.add_edge(edge(source, "reads", &format!("state:{name}")));
        }
    }
    fn add_checks(&mut self, source: &str, reads: &BTreeSet<String>) {
        for name in reads {
            self.add_edge(edge(source, "checks", &format!("state:{name}")));
        }
    }
    fn add_writes(&mut self, source: &str, root: &str) {
        if self.state.contains(root) {
            self.add_edge(edge(source, "writes", &format!("state:{root}")));
        }
    }

    #[allow(clippy::too_many_lines)]
    fn build(mut self) -> Value {
        self.add_node(
            node(
                format!("spec:{}", self.model.name),
                "spec",
                Some(self.model.name.clone()),
                None,
            ),
            false,
        );
        for (name, ty) in &self.model.state {
            let mut value = node(format!("state:{name}"), "state", Some(name.clone()), None);
            value.insert("type".to_owned(), public_type(self.model, ty));
            self.add_node(value, true);
            for physical in physical_names(self.model, name, ty) {
                let mut value = node(
                    format!("phys_state:{physical}"),
                    "phys_state",
                    Some(physical.clone()),
                    None,
                );
                value.insert("label".to_owned(), json!(physical));
                value.insert("logical".to_owned(), json!(name));
                self.add_node(value, true);
                self.add_edge(edge(
                    &format!("state:{name}"),
                    "expands_to",
                    &format!("phys_state:{physical}"),
                ));
            }
        }
        for action in &self.model.actions {
            let name = &action.name;
            let action_id = format!("action:{name}");
            let mut value = node(
                action_id.clone(),
                "action",
                Some(name.clone()),
                Some(action.span.python_loc()),
            );
            value.insert("fair".to_owned(), json!(action.fair));
            value.insert("sync".to_owned(), json!(false));
            add_requirement_metadata(&mut value, &action.annotations, action.meta.as_ref());
            self.add_node(value, true);
            let mut action_reads = BTreeSet::new();
            for (index, requirement) in action.requires.iter().enumerate() {
                let id = format!("guard:{name}:{index}");
                let mut value = node(
                    id.clone(),
                    "guard",
                    Some(format!("{name}:{index}")),
                    action
                        .require_spans
                        .get(index)
                        .map(|span| span.python_loc()),
                );
                value.insert(
                    "label".to_owned(),
                    json!(format!("{} requires {index}", display(name))),
                );
                value.insert("expr".to_owned(), requirement.python_ast());
                value.insert("action".to_owned(), json!(action_id));
                self.add_node(value, false);
                self.add_edge(edge(&action_id, "has_guard", &id));
                let reads = expr_reads(requirement, &self.state);
                action_reads.extend(reads.clone());
                self.add_reads(&id, &reads);
            }
            let mut effects = Vec::new();
            assignments(&action.statements, &mut effects);
            for (index, (target, expr, loc)) in effects.into_iter().enumerate() {
                let id = format!("effect:{name}:{index}");
                let root = lvalue_root(target);
                let mut value = node(
                    id.clone(),
                    "effect",
                    Some(format!("{name}:{index}")),
                    Some(loc),
                );
                value.insert(
                    "label".to_owned(),
                    json!(format!("{} effect {index}", display(name))),
                );
                value.insert("expr".to_owned(), expr.python_ast());
                value.insert("action".to_owned(), json!(action_id));
                value.insert("target".to_owned(), json!(root));
                self.add_node(value, false);
                self.add_edge(edge(&action_id, "has_effect", &id));
                self.add_writes(&action_id, root);
                self.add_writes(&id, root);
                self.add_reads(&id, &expr_reads(expr, &self.state));
            }
            action_reads.extend(statement_reads(&action.statements, &self.state));
            for (index, ensures) in action.ensures.iter().enumerate() {
                let id = format!("ensures:{name}:{index}");
                let mut value = node(
                    id.clone(),
                    "ensures",
                    Some(format!("{name}:{index}")),
                    action.ensure_spans.get(index).map(|span| span.python_loc()),
                );
                value.insert(
                    "label".to_owned(),
                    json!(format!("{} ensures {index}", display(name))),
                );
                value.insert("expr".to_owned(), ensures.python_ast());
                value.insert("action".to_owned(), json!(action_id));
                self.add_node(value, false);
                self.add_edge(edge(&action_id, "has_ensures", &id));
                let reads = expr_reads(ensures, &self.state);
                action_reads.extend(reads.clone());
                self.add_reads(&id, &reads);
            }
            self.add_reads(&action_id, &action_reads);
        }
        for (kind, items) in [
            ("invariant", &self.model.invariants),
            ("trans", &self.model.transitions),
            ("reachable", &self.model.reachables),
        ] {
            for property in items {
                let id = format!("{kind}:{}", property.name);
                let mut value = node(
                    id.clone(),
                    kind,
                    Some(property.name.clone()),
                    Some(property.span.python_loc()),
                );
                value.insert("expr".to_owned(), property.expr.python_ast());
                add_requirement_metadata(&mut value, &property.annotations, property.meta.as_ref());
                self.add_node(value, true);
                let reads = expr_reads(&property.expr, &self.state);
                self.add_reads(&id, &reads);
                self.add_checks(&id, &reads);
            }
        }
        for property in &self.model.leadstos {
            let id = format!("leadsTo:{}", property.name);
            let mut value = node(
                id.clone(),
                "leadsTo",
                Some(property.name.clone()),
                Some(property.span.python_loc()),
            );
            value.insert("P".to_owned(), property.before.python_ast());
            value.insert("Q".to_owned(), property.after.python_ast());
            add_requirement_metadata(&mut value, &property.annotations, property.meta.as_ref());
            if let Some(within) = property.within {
                value.insert("within".to_owned(), json!(within));
            }
            if let Some(decreases) = &property.decreases {
                value.insert("decreases".to_owned(), decreases.python_ast());
            }
            self.add_node(value, true);
            let mut reads = expr_reads(&property.before, &self.state);
            reads.extend(expr_reads(&property.after, &self.state));
            if let Some(decreases) = &property.decreases {
                reads.extend(expr_reads(decreases, &self.state));
            }
            self.add_reads(&id, &reads);
            self.add_checks(&id, &reads);
        }
        json!({"analysis":"structure","projection":"tsg","schema_version":"tsg.v0","nodes":self.nodes.into_values().collect::<Vec<_>>(),"edges":self.edges.into_values().collect::<Vec<_>>()})
    }
}

/// Build the deterministic `tsg.v0` structural projection.
#[must_use]
pub fn build_tsg(model: &KernelModel) -> Value {
    Builder::new(model).build()
}

/// Build either the raw TSG or one of its deterministic graph projections.
///
/// # Errors
///
/// Returns an error for unsupported projections, invalid focus usage, or an
/// unknown impact-graph focus node.
pub fn analyze_model(
    model: &KernelModel,
    projection: &str,
    focus: Option<&str>,
) -> Result<Value, String> {
    let tsg = build_tsg(model);
    if projection == "tsg" {
        if focus.is_some() {
            return Err("--focus is supported only with --projection impact_graph".to_owned());
        }
        Ok(tsg)
    } else {
        analysis_graph::project(&tsg, projection, focus)
    }
}
