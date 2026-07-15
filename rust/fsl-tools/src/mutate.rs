// SPDX-License-Identifier: Apache-2.0

//! Deterministic built-in mutations over the typed FSL surface AST.

use std::collections::BTreeMap;

use fsl_syntax::{
    ActionItem, Binder, Expr, LValue, MetaTag, Span, SpecItem, Statement, SurfaceSpec,
};

/// One fully materialized built-in mutant.
#[derive(Clone, Debug)]
pub struct BuiltinMutant {
    pub op: String,
    pub spec: SurfaceSpec,
    pub span: Option<Span>,
    pub target: String,
    pub requirement: Option<MetaTag>,
    pub action: Option<String>,
}

#[derive(Clone)]
struct ExprMutation {
    expr: Expr,
    op: &'static str,
    enum_swap: Option<(String, String)>,
}

#[derive(Clone)]
struct StatementMutation {
    statements: Vec<Statement>,
    op: &'static str,
    span: Span,
    target: String,
}

fn enum_siblings(spec: &SurfaceSpec) -> BTreeMap<String, Vec<String>> {
    let mut result = BTreeMap::new();
    for item in &spec.items {
        if let SpecItem::Enum { members, .. } = item {
            for member in members {
                result.insert(
                    member.clone(),
                    members
                        .iter()
                        .filter(|candidate| *candidate != member)
                        .cloned()
                        .collect(),
                );
            }
        }
    }
    result
}

fn rebuild_one<T: Clone>(values: &[T], index: usize, replacement: T) -> Vec<T> {
    let mut result = values.to_vec();
    result[index] = replacement;
    result
}

fn mutate_binder(
    binder: &Binder,
    enums: &BTreeMap<String, Vec<String>>,
) -> Vec<(Binder, ExprMutation)> {
    let mut output = Vec::new();
    match binder {
        Binder::Typed {
            name,
            type_name,
            where_expr,
        } => {
            if let Some(condition) = where_expr {
                for mutation in expr_mutations(condition, enums) {
                    output.push((
                        Binder::Typed {
                            name: name.clone(),
                            type_name: type_name.clone(),
                            where_expr: Some(Box::new(mutation.expr.clone())),
                        },
                        mutation,
                    ));
                }
            }
        }
        Binder::Range { name, lo, hi } => {
            for mutation in expr_mutations(lo, enums) {
                output.push((
                    Binder::Range {
                        name: name.clone(),
                        lo: Box::new(mutation.expr.clone()),
                        hi: hi.clone(),
                    },
                    mutation,
                ));
            }
            for mutation in expr_mutations(hi, enums) {
                output.push((
                    Binder::Range {
                        name: name.clone(),
                        lo: lo.clone(),
                        hi: Box::new(mutation.expr.clone()),
                    },
                    mutation,
                ));
            }
        }
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => {
            for mutation in expr_mutations(collection, enums) {
                output.push((
                    Binder::Collection {
                        name: name.clone(),
                        collection: Box::new(mutation.expr.clone()),
                        where_expr: where_expr.clone(),
                    },
                    mutation,
                ));
            }
            if let Some(condition) = where_expr {
                for mutation in expr_mutations(condition, enums) {
                    output.push((
                        Binder::Collection {
                            name: name.clone(),
                            collection: collection.clone(),
                            where_expr: Some(Box::new(mutation.expr.clone())),
                        },
                        mutation,
                    ));
                }
            }
        }
    }
    output
}

#[allow(clippy::too_many_lines)]
fn expr_mutations(expr: &Expr, enums: &BTreeMap<String, Vec<String>>) -> Vec<ExprMutation> {
    let mut output = Vec::new();
    match expr {
        Expr::Num(value) => {
            output.push(ExprMutation {
                expr: Expr::Num(value - 1),
                op: "integer_literal_minus1",
                enum_swap: None,
            });
            output.push(ExprMutation {
                expr: Expr::Num(value + 1),
                op: "integer_literal_plus1",
                enum_swap: None,
            });
        }
        Expr::Var(name) if enums.contains_key(name) => {
            for sibling in &enums[name] {
                output.push(ExprMutation {
                    expr: Expr::Var(sibling.clone()),
                    op: "enum_constant_swap",
                    enum_swap: Some((name.clone(), sibling.clone())),
                });
            }
        }
        Expr::Some(value) => {
            for mutation in expr_mutations(value, enums) {
                output.push(ExprMutation {
                    expr: Expr::Some(Box::new(mutation.expr.clone())),
                    ..mutation
                });
            }
        }
        Expr::Set(values) => {
            for (index, value) in values.iter().enumerate() {
                for mutation in expr_mutations(value, enums) {
                    output.push(ExprMutation {
                        expr: Expr::Set(rebuild_one(values, index, mutation.expr.clone())),
                        ..mutation
                    });
                }
            }
        }
        Expr::Seq(values) => {
            for (index, value) in values.iter().enumerate() {
                for mutation in expr_mutations(value, enums) {
                    output.push(ExprMutation {
                        expr: Expr::Seq(rebuild_one(values, index, mutation.expr.clone())),
                        ..mutation
                    });
                }
            }
        }
        Expr::Struct { name, fields } => {
            let mut indices = (0..fields.len()).collect::<Vec<_>>();
            indices.sort_by_key(|index| &fields[*index].0);
            for index in indices {
                let value = &fields[index].1;
                for mutation in expr_mutations(value, enums) {
                    let mut replacement = fields.clone();
                    replacement[index].1 = mutation.expr.clone();
                    output.push(ExprMutation {
                        expr: Expr::Struct {
                            name: name.clone(),
                            fields: replacement,
                        },
                        ..mutation
                    });
                }
            }
        }
        Expr::Call { name, args, span } => {
            for (index, value) in args.iter().enumerate() {
                for mutation in expr_mutations(value, enums) {
                    output.push(ExprMutation {
                        expr: Expr::Call {
                            name: name.clone(),
                            args: rebuild_one(args, index, mutation.expr.clone()),
                            span: *span,
                        },
                        ..mutation
                    });
                }
            }
        }
        Expr::Index(base, index) => {
            for mutation in expr_mutations(base, enums) {
                output.push(ExprMutation {
                    expr: Expr::Index(Box::new(mutation.expr.clone()), index.clone()),
                    ..mutation
                });
            }
            for mutation in expr_mutations(index, enums) {
                output.push(ExprMutation {
                    expr: Expr::Index(base.clone(), Box::new(mutation.expr.clone())),
                    ..mutation
                });
            }
        }
        Expr::Field(base, name) => {
            for mutation in expr_mutations(base, enums) {
                output.push(ExprMutation {
                    expr: Expr::Field(Box::new(mutation.expr.clone()), name.clone()),
                    ..mutation
                });
            }
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            for mutation in expr_mutations(receiver, enums) {
                output.push(ExprMutation {
                    expr: Expr::Method {
                        receiver: Box::new(mutation.expr.clone()),
                        name: name.clone(),
                        args: args.clone(),
                    },
                    ..mutation
                });
            }
            for (index, value) in args.iter().enumerate() {
                for mutation in expr_mutations(value, enums) {
                    output.push(ExprMutation {
                        expr: Expr::Method {
                            receiver: receiver.clone(),
                            name: name.clone(),
                            args: rebuild_one(args, index, mutation.expr.clone()),
                        },
                        ..mutation
                    });
                }
            }
        }
        Expr::Binary { op, left, right } => {
            if matches!(op.as_str(), "==" | "!=") {
                output.push(ExprMutation {
                    expr: Expr::Binary {
                        op: if op == "==" { "!=" } else { "==" }.to_owned(),
                        left: left.clone(),
                        right: right.clone(),
                    },
                    op: "equality_operator_flip",
                    enum_swap: None,
                });
            }
            for mutation in expr_mutations(left, enums) {
                output.push(ExprMutation {
                    expr: Expr::Binary {
                        op: op.clone(),
                        left: Box::new(mutation.expr.clone()),
                        right: right.clone(),
                    },
                    ..mutation
                });
            }
            for mutation in expr_mutations(right, enums) {
                output.push(ExprMutation {
                    expr: Expr::Binary {
                        op: op.clone(),
                        left: left.clone(),
                        right: Box::new(mutation.expr.clone()),
                    },
                    ..mutation
                });
            }
        }
        Expr::Neg(value) => {
            for mutation in expr_mutations(value, enums) {
                output.push(ExprMutation {
                    expr: Expr::Neg(Box::new(mutation.expr.clone())),
                    ..mutation
                });
            }
        }
        Expr::Not(value) => {
            for mutation in expr_mutations(value, enums) {
                output.push(ExprMutation {
                    expr: Expr::Not(Box::new(mutation.expr.clone())),
                    ..mutation
                });
            }
        }
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => {
            for mutation in expr_mutations(condition, enums) {
                output.push(ExprMutation {
                    expr: Expr::IfThenElse {
                        condition: Box::new(mutation.expr.clone()),
                        then_expr: then_expr.clone(),
                        else_expr: else_expr.clone(),
                    },
                    ..mutation
                });
            }
            for mutation in expr_mutations(then_expr, enums) {
                output.push(ExprMutation {
                    expr: Expr::IfThenElse {
                        condition: condition.clone(),
                        then_expr: Box::new(mutation.expr.clone()),
                        else_expr: else_expr.clone(),
                    },
                    ..mutation
                });
            }
            for mutation in expr_mutations(else_expr, enums) {
                output.push(ExprMutation {
                    expr: Expr::IfThenElse {
                        condition: condition.clone(),
                        then_expr: then_expr.clone(),
                        else_expr: Box::new(mutation.expr.clone()),
                    },
                    ..mutation
                });
            }
        }
        Expr::Is {
            expr: value,
            pattern,
        } => {
            for mutation in expr_mutations(value, enums) {
                output.push(ExprMutation {
                    expr: Expr::Is {
                        expr: Box::new(mutation.expr.clone()),
                        pattern: pattern.clone(),
                    },
                    ..mutation
                });
            }
        }
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => {
            for (replacement, mutation) in mutate_binder(binder, enums) {
                output.push(ExprMutation {
                    expr: Expr::Quantified {
                        quantifier: quantifier.clone(),
                        binder: replacement,
                        body: body.clone(),
                    },
                    ..mutation
                });
            }
            for mutation in expr_mutations(body, enums) {
                output.push(ExprMutation {
                    expr: Expr::Quantified {
                        quantifier: quantifier.clone(),
                        binder: binder.clone(),
                        body: Box::new(mutation.expr.clone()),
                    },
                    ..mutation
                });
            }
        }
        Expr::Count {
            name,
            type_name,
            condition,
        } => {
            for mutation in expr_mutations(condition, enums) {
                output.push(ExprMutation {
                    expr: Expr::Count {
                        name: name.clone(),
                        type_name: type_name.clone(),
                        condition: Box::new(mutation.expr.clone()),
                    },
                    ..mutation
                });
            }
        }
        Expr::Sum {
            name,
            type_name,
            body,
            condition,
        } => {
            for mutation in expr_mutations(body, enums) {
                output.push(ExprMutation {
                    expr: Expr::Sum {
                        name: name.clone(),
                        type_name: type_name.clone(),
                        body: Box::new(mutation.expr.clone()),
                        condition: condition.clone(),
                    },
                    ..mutation
                });
            }
            if let Some(condition) = condition {
                for mutation in expr_mutations(condition, enums) {
                    output.push(ExprMutation {
                        expr: Expr::Sum {
                            name: name.clone(),
                            type_name: type_name.clone(),
                            body: body.clone(),
                            condition: Some(Box::new(mutation.expr.clone())),
                        },
                        ..mutation
                    });
                }
            }
        }
        Expr::UnaryNamed {
            name,
            expr: value,
            span,
        } => {
            for mutation in expr_mutations(value, enums) {
                output.push(ExprMutation {
                    expr: Expr::UnaryNamed {
                        name: name.clone(),
                        expr: Box::new(mutation.expr.clone()),
                        span: *span,
                    },
                    ..mutation
                });
            }
        }
        Expr::BinaryNamed { name, left, right } => {
            for mutation in expr_mutations(left, enums) {
                output.push(ExprMutation {
                    expr: Expr::BinaryNamed {
                        name: name.clone(),
                        left: Box::new(mutation.expr.clone()),
                        right: right.clone(),
                    },
                    ..mutation
                });
            }
            for mutation in expr_mutations(right, enums) {
                output.push(ExprMutation {
                    expr: Expr::BinaryNamed {
                        name: name.clone(),
                        left: left.clone(),
                        right: Box::new(mutation.expr.clone()),
                    },
                    ..mutation
                });
            }
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => {
            for mutation in expr_mutations(first, enums) {
                output.push(ExprMutation {
                    expr: Expr::TernaryNamed {
                        name: name.clone(),
                        first: Box::new(mutation.expr.clone()),
                        second: second.clone(),
                        third: third.clone(),
                    },
                    ..mutation
                });
            }
            for mutation in expr_mutations(second, enums) {
                output.push(ExprMutation {
                    expr: Expr::TernaryNamed {
                        name: name.clone(),
                        first: first.clone(),
                        second: Box::new(mutation.expr.clone()),
                        third: third.clone(),
                    },
                    ..mutation
                });
            }
            for mutation in expr_mutations(third, enums) {
                output.push(ExprMutation {
                    expr: Expr::TernaryNamed {
                        name: name.clone(),
                        first: first.clone(),
                        second: second.clone(),
                        third: Box::new(mutation.expr.clone()),
                    },
                    ..mutation
                });
            }
        }
        Expr::BinderNamed { name, binder } => {
            for (replacement, mutation) in mutate_binder(binder, enums) {
                output.push(ExprMutation {
                    expr: Expr::BinderNamed {
                        name: name.clone(),
                        binder: replacement,
                    },
                    ..mutation
                });
            }
        }
        Expr::Bool(_) | Expr::None | Expr::Var(_) => {}
    }
    output
}

fn lvalue_mutations(
    value: &LValue,
    enums: &BTreeMap<String, Vec<String>>,
) -> Vec<(LValue, ExprMutation)> {
    match value {
        LValue::Index(name, index) => expr_mutations(index, enums)
            .into_iter()
            .map(|mutation| (LValue::Index(name.clone(), mutation.expr.clone()), mutation))
            .collect(),
        LValue::Field(base, field) => lvalue_mutations(base, enums)
            .into_iter()
            .map(|(base, mutation)| (LValue::Field(Box::new(base), field.clone()), mutation))
            .collect(),
        LValue::Var(_) => Vec::new(),
    }
}

fn mutation_target(base: &str, mutation: &ExprMutation) -> String {
    mutation.enum_swap.as_ref().map_or_else(
        || base.to_owned(),
        |(from, to)| format!("{base} {from}->{to}"),
    )
}

#[allow(clippy::too_many_lines)]
fn statement_mutations(
    statements: &[Statement],
    enums: &BTreeMap<String, Vec<String>>,
    action: &str,
) -> Vec<StatementMutation> {
    let mut output = Vec::new();
    for (index, statement) in statements.iter().enumerate() {
        match statement {
            Statement::Assign {
                target,
                value,
                span,
            } => {
                let mut removed = statements.to_vec();
                removed.remove(index);
                output.push(StatementMutation {
                    statements: removed,
                    op: "assignment_remove",
                    span: *span,
                    target: format!("{action} assignment"),
                });
                for (replacement, mutation) in lvalue_mutations(target, enums) {
                    let statement = Statement::Assign {
                        target: replacement,
                        value: value.clone(),
                        span: *span,
                    };
                    output.push(StatementMutation {
                        statements: rebuild_one(statements, index, statement),
                        op: mutation.op,
                        span: *span,
                        target: mutation_target(&format!("{action} assignment target"), &mutation),
                    });
                }
                for mutation in expr_mutations(value, enums) {
                    let statement = Statement::Assign {
                        target: target.clone(),
                        value: mutation.expr.clone(),
                        span: *span,
                    };
                    output.push(StatementMutation {
                        statements: rebuild_one(statements, index, statement),
                        op: mutation.op,
                        span: *span,
                        target: mutation_target(&format!("{action} assignment"), &mutation),
                    });
                }
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                span,
            } => {
                if !then_statements.is_empty() && !else_statements.is_empty() {
                    let statement = Statement::If {
                        condition: condition.clone(),
                        then_statements: else_statements.clone(),
                        else_statements: then_statements.clone(),
                        span: *span,
                    };
                    output.push(StatementMutation {
                        statements: rebuild_one(statements, index, statement),
                        op: "then_else_swap",
                        span: *span,
                        target: format!("{action} if"),
                    });
                }
                for mutation in expr_mutations(condition, enums) {
                    let statement = Statement::If {
                        condition: mutation.expr.clone(),
                        then_statements: then_statements.clone(),
                        else_statements: else_statements.clone(),
                        span: *span,
                    };
                    output.push(StatementMutation {
                        statements: rebuild_one(statements, index, statement),
                        op: mutation.op,
                        span: *span,
                        target: mutation_target(&format!("{action} if condition"), &mutation),
                    });
                }
                for nested in statement_mutations(then_statements, enums, action) {
                    let statement = Statement::If {
                        condition: condition.clone(),
                        then_statements: nested.statements,
                        else_statements: else_statements.clone(),
                        span: *span,
                    };
                    output.push(StatementMutation {
                        statements: rebuild_one(statements, index, statement),
                        ..nested
                    });
                }
                for nested in statement_mutations(else_statements, enums, action) {
                    let statement = Statement::If {
                        condition: condition.clone(),
                        then_statements: then_statements.clone(),
                        else_statements: nested.statements,
                        span: *span,
                    };
                    output.push(StatementMutation {
                        statements: rebuild_one(statements, index, statement),
                        ..nested
                    });
                }
            }
            Statement::ForAll {
                binder,
                statements: body,
                span,
            } => {
                for (replacement, mutation) in mutate_binder(binder, enums) {
                    let statement = Statement::ForAll {
                        binder: replacement,
                        statements: body.clone(),
                        span: *span,
                    };
                    let suffix = match binder {
                        Binder::Range { .. } => "bound",
                        Binder::Typed { .. } | Binder::Collection { .. } => "where",
                    };
                    output.push(StatementMutation {
                        statements: rebuild_one(statements, index, statement),
                        op: mutation.op,
                        span: *span,
                        target: mutation_target(&format!("{action} forall {suffix}"), &mutation),
                    });
                }
                for nested in statement_mutations(body, enums, action) {
                    let statement = Statement::ForAll {
                        binder: binder.clone(),
                        statements: nested.statements,
                        span: *span,
                    };
                    output.push(StatementMutation {
                        statements: rebuild_one(statements, index, statement),
                        ..nested
                    });
                }
            }
        }
    }
    output
}

/// Enumerate Python-compatible built-in mutants in deterministic source order.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn enumerate_builtin_mutants(spec: &SurfaceSpec) -> Vec<BuiltinMutant> {
    let enums = enum_siblings(spec);
    let mut output = Vec::new();
    for (item_index, item) in spec.items.iter().enumerate() {
        match item {
            SpecItem::Type {
                name,
                lo,
                hi,
                symmetric,
            } => {
                for (bound, value) in [("lo", lo.as_ref()), ("hi", hi.as_ref())] {
                    let Expr::Num(value) = value else {
                        continue;
                    };
                    for (delta, suffix) in [(-1, "minus1"), (1, "plus1")] {
                        let mut mutated = spec.clone();
                        mutated.items[item_index] = if bound == "lo" {
                            SpecItem::Type {
                                name: name.clone(),
                                lo: Box::new(Expr::Num(value + delta)),
                                hi: hi.clone(),
                                symmetric: *symmetric,
                            }
                        } else {
                            SpecItem::Type {
                                name: name.clone(),
                                lo: lo.clone(),
                                hi: Box::new(Expr::Num(value + delta)),
                                symmetric: *symmetric,
                            }
                        };
                        output.push(BuiltinMutant {
                            op: format!("type_bound_{bound}_{suffix}"),
                            spec: mutated,
                            span: None,
                            target: format!("type {name} {bound}"),
                            requirement: None,
                            action: None,
                        });
                    }
                }
            }
            SpecItem::Const { name, value } => {
                for mutation in expr_mutations(value, &enums) {
                    let mut mutated = spec.clone();
                    mutated.items[item_index] = SpecItem::Const {
                        name: name.clone(),
                        value: Box::new(mutation.expr.clone()),
                    };
                    output.push(BuiltinMutant {
                        op: mutation.op.to_owned(),
                        spec: mutated,
                        span: None,
                        target: mutation_target(&format!("const {name}"), &mutation),
                        requirement: None,
                        action: None,
                    });
                }
            }
            SpecItem::Init {
                statements,
                meta,
                annotations,
            } => {
                for mutation in statement_mutations(statements, &enums, "init") {
                    let mut mutated = spec.clone();
                    mutated.items[item_index] = SpecItem::Init {
                        statements: mutation.statements,
                        meta: meta.clone(),
                        annotations: annotations.clone(),
                    };
                    output.push(BuiltinMutant {
                        op: mutation.op.to_owned(),
                        spec: mutated,
                        span: Some(mutation.span),
                        target: mutation.target,
                        requirement: None,
                        action: Some("init".to_owned()),
                    });
                }
            }
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
                let label = name.clone();
                let mut require_number = 0;
                for (part_index, part) in items.iter().enumerate() {
                    match part {
                        ActionItem::Requires(expr, item_span) => {
                            require_number += 1;
                            let target = format!("{label} requires #{require_number}");
                            let mut removed = spec.clone();
                            if let SpecItem::Action { items, .. } = &mut removed.items[item_index] {
                                items.remove(part_index);
                            }
                            output.push(BuiltinMutant {
                                op: "requires_remove".to_owned(),
                                spec: removed,
                                span: Some(*item_span),
                                target: target.clone(),
                                requirement: meta.clone(),
                                action: Some(label.clone()),
                            });
                            let mut negated = spec.clone();
                            if let SpecItem::Action { items, .. } = &mut negated.items[item_index] {
                                items[part_index] = ActionItem::Requires(
                                    Expr::Not(Box::new(expr.clone())),
                                    *item_span,
                                );
                            }
                            output.push(BuiltinMutant {
                                op: "requires_negate".to_owned(),
                                spec: negated,
                                span: Some(*item_span),
                                target: target.clone(),
                                requirement: meta.clone(),
                                action: Some(label.clone()),
                            });
                            for mutation in expr_mutations(expr, &enums) {
                                let mut mutated = spec.clone();
                                if let SpecItem::Action { items, .. } =
                                    &mut mutated.items[item_index]
                                {
                                    items[part_index] =
                                        ActionItem::Requires(mutation.expr.clone(), *item_span);
                                }
                                output.push(BuiltinMutant {
                                    op: mutation.op.to_owned(),
                                    spec: mutated,
                                    span: Some(*item_span),
                                    target: mutation_target(&target, &mutation),
                                    requirement: meta.clone(),
                                    action: Some(label.clone()),
                                });
                            }
                        }
                        ActionItem::Let(let_name, expr, item_span) => {
                            for mutation in expr_mutations(expr, &enums) {
                                let mut mutated = spec.clone();
                                if let SpecItem::Action { items, .. } =
                                    &mut mutated.items[item_index]
                                {
                                    items[part_index] = ActionItem::Let(
                                        let_name.clone(),
                                        mutation.expr.clone(),
                                        *item_span,
                                    );
                                }
                                output.push(BuiltinMutant {
                                    op: mutation.op.to_owned(),
                                    spec: mutated,
                                    span: Some(*item_span),
                                    target: mutation_target(
                                        &format!("{label} let {let_name}"),
                                        &mutation,
                                    ),
                                    requirement: meta.clone(),
                                    action: Some(label.clone()),
                                });
                            }
                        }
                        ActionItem::Ensures(..) | ActionItem::Statement(..) => {}
                    }
                }
                for (part_index, part) in items.iter().enumerate() {
                    let ActionItem::Statement(statement) = part else {
                        continue;
                    };
                    for mutation in
                        statement_mutations(std::slice::from_ref(statement), &enums, &label)
                    {
                        let mut mutated = spec.clone();
                        if let SpecItem::Action { items, .. } = &mut mutated.items[item_index] {
                            if mutation.statements.is_empty() {
                                items.remove(part_index);
                            } else {
                                items[part_index] =
                                    ActionItem::Statement(mutation.statements[0].clone());
                            }
                        }
                        output.push(BuiltinMutant {
                            op: mutation.op.to_owned(),
                            spec: mutated,
                            span: Some(mutation.span),
                            target: mutation.target,
                            requirement: meta.clone(),
                            action: Some(label.clone()),
                        });
                    }
                }
                if *fair {
                    let mut mutated = spec.clone();
                    mutated.items[item_index] = SpecItem::Action {
                        name: name.clone(),
                        params: params.clone(),
                        items: items.clone(),
                        span: *span,
                        fair: false,
                        meta: meta.clone(),
                        sync: *sync,
                        annotations: annotations.clone(),
                    };
                    output.push(BuiltinMutant {
                        op: "fair_remove".to_owned(),
                        spec: mutated,
                        span: Some(*span),
                        target: format!("{label} fair"),
                        requirement: meta.clone(),
                        action: Some(label),
                    });
                }
            }
            _ => {}
        }
    }
    output
}
