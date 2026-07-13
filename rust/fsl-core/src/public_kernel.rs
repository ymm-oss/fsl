// SPDX-License-Identifier: Apache-2.0

//! Versioned normalized Kernel JSON for external compilers.

use std::collections::BTreeMap;
use std::fmt;

use fsl_syntax::{Binder, Expr, LValue, MetaTag, Pattern, Span, SpecItem, Statement};
use serde_json::{Map, Value, json};

use crate::{ActionGuard, KernelModel, KernelSpec, ParamDef, TypeDef, TypeRef};

pub const KERNEL_SCHEMA_VERSION: &str = "1.0.0";
pub const KERNEL_SCHEMA_ID: &str = "https://fsl.dev/schemas/fslc/kernel/kernel.v1.schema.json";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicKernelError {
    pub message: String,
}

impl fmt::Display for PublicKernelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PublicKernelError {}

fn error(message: impl Into<String>) -> PublicKernelError {
    PublicKernelError {
        message: message.into(),
    }
}

fn span_json(path: &str, span: Span) -> Value {
    json!({
        "file": path,
        "line": span.start.line,
        "column": span.start.column,
        "end_line": span.end.line,
        "end_column": span.end.column,
    })
}

fn statement_span(statement: &Statement) -> Span {
    match statement {
        Statement::Assign { span, .. }
        | Statement::If { span, .. }
        | Statement::ForAll { span, .. } => *span,
    }
}

fn type_json(ty: &TypeRef) -> Value {
    match ty {
        TypeRef::Int => json!({"kind":"int"}),
        TypeRef::Bool => json!({"kind":"bool"}),
        TypeRef::Named(name) => json!({"kind":"named","name":name}),
        TypeRef::Range(lo, hi) => json!({"kind":"domain","lo":lo,"hi":hi}),
        TypeRef::Map(key, value) => {
            json!({"kind":"map","key":type_json(key),"value":type_json(value)})
        }
        TypeRef::Relation(source, target) => json!({
            "kind":"relation","source":type_json(source),"target":type_json(target)
        }),
        TypeRef::Set(item) => json!({"kind":"set","item":type_json(item)}),
        TypeRef::Seq(item, capacity) => {
            json!({"kind":"seq","item":type_json(item),"capacity":capacity})
        }
        TypeRef::Option(item) => json!({"kind":"option","item":type_json(item)}),
    }
}

fn resolve<'a>(model: &'a KernelModel, ty: &'a TypeRef) -> Result<TypeRef, PublicKernelError> {
    match ty {
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => Ok(TypeRef::Range(*lo, *hi)),
            Some(TypeDef::Enum { .. } | TypeDef::Struct { .. }) => Ok(ty.clone()),
            None => Err(error(format!("public Kernel cannot resolve type '{name}'"))),
        },
        _ => Ok(ty.clone()),
    }
}

type TypeEnv = BTreeMap<String, TypeRef>;

fn base_env(model: &KernelModel) -> TypeEnv {
    let mut env = model.state.iter().cloned().collect::<TypeEnv>();
    for (name, value) in &model.consts {
        let ty = match value {
            crate::FslValue::Bool(_) => TypeRef::Bool,
            _ => TypeRef::Int,
        };
        env.insert(name.clone(), ty);
    }
    for (type_name, definition) in &model.types {
        if let TypeDef::Enum { members, .. } = definition {
            for member in members {
                env.insert(member.clone(), TypeRef::Named(type_name.clone()));
            }
        }
    }
    env
}

fn qualified_name(name: &fsl_syntax::QualifiedName) -> Result<String, PublicKernelError> {
    if name.namespace.is_some() {
        return Err(error(
            "qualified types must be lowered before public Kernel export",
        ));
    }
    Ok(name.name.clone())
}

fn binder_type(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<TypeRef, PublicKernelError> {
    match binder {
        Binder::Typed { type_name, .. } => Ok(TypeRef::Named(qualified_name(type_name)?)),
        Binder::Range { lo, hi, .. } => {
            let Expr::Num(lo) = lo.as_ref() else {
                return Err(error(
                    "public Kernel requires lowered constant binder bounds",
                ));
            };
            let Expr::Num(hi) = hi.as_ref() else {
                return Err(error(
                    "public Kernel requires lowered constant binder bounds",
                ));
            };
            Ok(TypeRef::Range(*lo, *hi))
        }
        Binder::Collection { collection, .. } => {
            match resolve(model, &infer_type(collection, env, model, None)?)? {
                TypeRef::Set(item) | TypeRef::Seq(item, _) => Ok(*item),
                _ => Err(error("collection binder requires Set or Seq")),
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn infer_type(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    expected: Option<&TypeRef>,
) -> Result<TypeRef, PublicKernelError> {
    match expr {
        Expr::Num(_) => match expected.map(|ty| resolve(model, ty)).transpose()? {
            Some(ty @ (TypeRef::Int | TypeRef::Range(_, _))) => Ok(ty),
            _ => Ok(TypeRef::Int),
        },
        Expr::Bool(_) => Ok(TypeRef::Bool),
        Expr::None => match expected.map(|ty| resolve(model, ty)).transpose()? {
            Some(ty @ TypeRef::Option(_)) => Ok(ty),
            _ => Err(error("public Kernel cannot infer uncontextualized none")),
        },
        Expr::Some(item) => {
            if let Some(expected) = expected {
                if matches!(resolve(model, expected)?, TypeRef::Option(_)) {
                    return Ok(expected.clone());
                }
            }
            Ok(TypeRef::Option(Box::new(infer_type(
                item, env, model, None,
            )?)))
        }
        Expr::Set(items) => {
            if let Some(expected) = expected {
                if matches!(resolve(model, expected)?, TypeRef::Set(_)) {
                    return Ok(expected.clone());
                }
            }
            let first = items
                .first()
                .ok_or_else(|| error("public Kernel cannot infer empty Set"))?;
            Ok(TypeRef::Set(Box::new(infer_type(first, env, model, None)?)))
        }
        Expr::Seq(items) => {
            if let Some(expected) = expected {
                if matches!(resolve(model, expected)?, TypeRef::Seq(_, _)) {
                    return Ok(expected.clone());
                }
            }
            let first = items
                .first()
                .ok_or_else(|| error("public Kernel cannot infer empty Seq"))?;
            Ok(TypeRef::Seq(
                Box::new(infer_type(first, env, model, None)?),
                items.len(),
            ))
        }
        Expr::Struct { name, .. } => Ok(TypeRef::Named(name.clone())),
        Expr::Var(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| error(format!("public Kernel cannot type identifier '{name}'"))),
        Expr::Call { name, .. } => Err(error(format!(
            "unlowered predicate call '{name}' in public Kernel"
        ))),
        Expr::Index(base, _) => match resolve(model, &infer_type(base, env, model, None)?)? {
            TypeRef::Map(_, value) | TypeRef::Relation(_, value) => Ok(*value),
            TypeRef::Seq(item, _) => Ok(*item),
            _ => Err(error("public Kernel index requires Map, Relation, or Seq")),
        },
        Expr::Field(base, field) => {
            let TypeRef::Named(name) = infer_type(base, env, model, None)? else {
                return Err(error("public Kernel field access requires named struct"));
            };
            match model.types.get(&name) {
                Some(TypeDef::Struct { fields }) => fields
                    .iter()
                    .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
                    .ok_or_else(|| error(format!("unknown struct field '{name}.{field}'"))),
                _ => Err(error("public Kernel field access requires struct")),
            }
        }
        Expr::Method { receiver, name, .. } => {
            let receiver_ty = infer_type(receiver, env, model, None)?;
            let resolved = resolve(model, &receiver_ty)?;
            match name.as_str() {
                "contains" => Ok(TypeRef::Bool),
                "size" => Ok(TypeRef::Int),
                "add" | "remove" | "push" | "pop" => Ok(receiver_ty),
                "head" | "at" => match resolved {
                    TypeRef::Seq(item, _) => Ok(*item),
                    _ => Err(error(format!("public Kernel cannot type method '{name}'"))),
                },
                _ => Err(error(format!("public Kernel cannot type method '{name}'"))),
            }
        }
        Expr::Binary { op, .. } => {
            if matches!(
                op.as_str(),
                "and" | "or" | "=>" | "==" | "!=" | "<" | "<=" | ">" | ">="
            ) {
                Ok(TypeRef::Bool)
            } else {
                Ok(TypeRef::Int)
            }
        }
        Expr::Neg(_) | Expr::Count { .. } | Expr::Sum { .. } | Expr::BinaryNamed { .. } => {
            Ok(TypeRef::Int)
        }
        Expr::Not(_) | Expr::Is { .. } | Expr::Quantified { .. } | Expr::BinderNamed { .. } => {
            Ok(TypeRef::Bool)
        }
        Expr::IfThenElse { then_expr, .. } => infer_type(then_expr, env, model, expected),
        Expr::UnaryNamed { name, expr, .. } => match name.as_str() {
            "old" => infer_type(expr, env, model, expected),
            "abs" => Ok(TypeRef::Int),
            "rel_acyclic" | "rel_functional" | "rel_injective" => Ok(TypeRef::Bool),
            "rel_domain" | "rel_range" => Ok(TypeRef::Set(Box::new(TypeRef::Int))),
            _ => Err(error(format!("unsupported unary expression '{name}'"))),
        },
        Expr::TernaryNamed { name, .. } if name == "rel_reachable" => Ok(TypeRef::Bool),
        Expr::TernaryNamed { name, .. } => {
            Err(error(format!("unsupported ternary expression '{name}'")))
        }
    }
}

fn extend_pattern_binding(
    expression: &Expr,
    env: &mut TypeEnv,
    model: &KernelModel,
) -> Result<(), PublicKernelError> {
    match expression {
        Expr::Is {
            expr,
            pattern: Pattern::Some(binding),
        } => {
            let TypeRef::Option(inner) = resolve(model, &infer_type(expr, env, model, None)?)?
            else {
                return Err(error("some pattern requires an Option operand"));
            };
            env.insert(binding.clone(), *inner);
        }
        Expr::Binary { left, right, .. } => {
            extend_pattern_binding(left, env, model)?;
            extend_pattern_binding(right, env, model)?;
        }
        _ => {}
    }
    Ok(())
}

fn binder_json(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
) -> Result<Value, PublicKernelError> {
    let ty = binder_type(binder, env, model)?;
    let (kind, name, lo, hi, collection, where_expr) = match binder {
        Binder::Typed {
            name, where_expr, ..
        } => ("typed", name, None, None, None, where_expr.as_deref()),
        Binder::Range { name, lo, hi } => (
            "range",
            name,
            Some(lo.as_ref()),
            Some(hi.as_ref()),
            None,
            None,
        ),
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => (
            "collection",
            name,
            None,
            None,
            Some(collection.as_ref()),
            where_expr.as_deref(),
        ),
    };
    let mut output = json!({"name":name,"type":type_json(&ty),"kind":kind});
    let object = output.as_object_mut().expect("binder object");
    if let Some(lo) = lo {
        object.insert(
            "lo".to_owned(),
            expr_json(lo, env, model, path, span, Some(&ty))?,
        );
    }
    if let Some(hi) = hi {
        object.insert(
            "hi".to_owned(),
            expr_json(hi, env, model, path, span, Some(&ty))?,
        );
    }
    if let Some(collection) = collection {
        object.insert(
            "collection".to_owned(),
            expr_json(collection, env, model, path, span, None)?,
        );
    }
    if let Some(where_expr) = where_expr {
        let mut local = env.clone();
        local.insert(name.clone(), ty);
        object.insert(
            "where".to_owned(),
            expr_json(where_expr, &local, model, path, span, Some(&TypeRef::Bool))?,
        );
    }
    Ok(output)
}

#[allow(clippy::too_many_lines)]
fn expr_json(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
    expected: Option<&TypeRef>,
) -> Result<Value, PublicKernelError> {
    let ty = infer_type(expr, env, model, expected)?;
    let mut output = Map::from_iter([
        ("kind".to_owned(), Value::String("unknown".to_owned())),
        ("type".to_owned(), type_json(&ty)),
        ("span".to_owned(), span_json(path, span)),
    ]);
    match expr {
        Expr::Num(value) => {
            output.insert("kind".to_owned(), json!("num"));
            output.insert("value".to_owned(), json!(value));
        }
        Expr::Bool(value) => {
            output.insert("kind".to_owned(), json!("bool"));
            output.insert("value".to_owned(), json!(value));
        }
        Expr::None => {
            output.insert("kind".to_owned(), json!("none"));
        }
        Expr::Some(item) => {
            let TypeRef::Option(inner) = resolve(model, &ty)? else {
                return Err(error("some expression did not infer Option"));
            };
            output.insert("kind".to_owned(), json!("some"));
            output.insert(
                "operand".to_owned(),
                expr_json(item, env, model, path, span, Some(&inner))?,
            );
        }
        Expr::Set(items) | Expr::Seq(items) => {
            let (kind, item_ty) = match resolve(model, &ty)? {
                TypeRef::Set(item) => ("set_lit", item),
                TypeRef::Seq(item, _) => ("seq_lit", item),
                _ => return Err(error("collection literal type mismatch")),
            };
            output.insert("kind".to_owned(), json!(kind));
            output.insert(
                "items".to_owned(),
                Value::Array(
                    items
                        .iter()
                        .map(|item| expr_json(item, env, model, path, span, Some(&item_ty)))
                        .collect::<Result<_, _>>()?,
                ),
            );
        }
        Expr::Struct { name, fields } => {
            output.insert("kind".to_owned(), json!("struct_lit"));
            output.insert("name".to_owned(), json!(name));
            let Some(TypeDef::Struct {
                fields: definitions,
            }) = model.types.get(name)
            else {
                return Err(error(format!("unknown struct '{name}'")));
            };
            let definitions = definitions.iter().cloned().collect::<BTreeMap<_, _>>();
            output.insert(
                "fields".to_owned(),
                Value::Object(
                    fields
                        .iter()
                        .map(|(field, value)| {
                            let expected = definitions.get(field).ok_or_else(|| {
                                error(format!("unknown struct field '{name}.{field}'"))
                            })?;
                            Ok((
                                field.clone(),
                                expr_json(value, env, model, path, span, Some(expected))?,
                            ))
                        })
                        .collect::<Result<_, PublicKernelError>>()?,
                ),
            );
        }
        Expr::Var(name) => {
            output.insert("kind".to_owned(), json!("var"));
            output.insert("name".to_owned(), json!(name));
        }
        Expr::Call { name, .. } => {
            return Err(error(format!(
                "unlowered predicate call '{name}' in public Kernel"
            )));
        }
        Expr::Index(collection, index) => {
            let collection_ty = resolve(model, &infer_type(collection, env, model, None)?)?;
            let key_ty = match collection_ty {
                TypeRef::Map(key, _) | TypeRef::Relation(key, _) => *key,
                TypeRef::Seq(_, _) => TypeRef::Int,
                _ => return Err(error("index expression requires collection")),
            };
            output.insert("kind".to_owned(), json!("index"));
            output.insert(
                "collection".to_owned(),
                expr_json(collection, env, model, path, span, None)?,
            );
            output.insert(
                "index".to_owned(),
                expr_json(index, env, model, path, span, Some(&key_ty))?,
            );
        }
        Expr::Field(value, field) => {
            output.insert("kind".to_owned(), json!("field"));
            output.insert(
                "value".to_owned(),
                expr_json(value, env, model, path, span, None)?,
            );
            output.insert("field".to_owned(), json!(field));
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            output.insert("kind".to_owned(), json!("method"));
            output.insert(
                "receiver".to_owned(),
                expr_json(receiver, env, model, path, span, None)?,
            );
            output.insert("method".to_owned(), json!(name));
            output.insert(
                "arguments".to_owned(),
                Value::Array(
                    args.iter()
                        .map(|arg| expr_json(arg, env, model, path, span, None))
                        .collect::<Result<_, _>>()?,
                ),
            );
        }
        Expr::Binary { op, left, right } => {
            let (left_ty, right_ty) = if matches!(op.as_str(), "and" | "or" | "=>") {
                (TypeRef::Bool, TypeRef::Bool)
            } else if matches!(
                op.as_str(),
                "+" | "-" | "*" | "/" | "%" | "<" | "<=" | ">" | ">="
            ) {
                (TypeRef::Int, TypeRef::Int)
            } else {
                infer_type(left, env, model, None).map_or_else(
                    |_| infer_type(right, env, model, None).map(|ty| (ty.clone(), ty)),
                    |ty| Ok((ty.clone(), ty)),
                )?
            };
            output.insert("kind".to_owned(), json!("binary"));
            output.insert("operator".to_owned(), json!(op));
            let left_json = expr_json(left, env, model, path, span, Some(&left_ty))?;
            let mut right_env = env.clone();
            extend_pattern_binding(left, &mut right_env, model)?;
            output.insert("left".to_owned(), left_json);
            output.insert(
                "right".to_owned(),
                expr_json(right, &right_env, model, path, span, Some(&right_ty))?,
            );
        }
        Expr::Neg(operand) | Expr::Not(operand) => {
            let kind = if matches!(expr, Expr::Neg(_)) {
                "neg"
            } else {
                "not"
            };
            output.insert("kind".to_owned(), json!(kind));
            output.insert(
                "operand".to_owned(),
                expr_json(operand, env, model, path, span, None)?,
            );
        }
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => {
            output.insert("kind".to_owned(), json!("ite"));
            output.insert(
                "condition".to_owned(),
                expr_json(condition, env, model, path, span, Some(&TypeRef::Bool))?,
            );
            output.insert(
                "then".to_owned(),
                expr_json(then_expr, env, model, path, span, Some(&ty))?,
            );
            output.insert(
                "else".to_owned(),
                expr_json(else_expr, env, model, path, span, Some(&ty))?,
            );
        }
        Expr::Is { expr, pattern } => {
            output.insert("kind".to_owned(), json!("is"));
            output.insert(
                "operand".to_owned(),
                expr_json(expr, env, model, path, span, None)?,
            );
            output.insert(
                "pattern".to_owned(),
                match pattern {
                    Pattern::None => json!({"kind":"none","binding":Value::Null}),
                    Pattern::Some(binding) => json!({"kind":"some","binding":binding}),
                },
            );
        }
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => {
            let binder_ty = binder_type(binder, env, model)?;
            let name = match binder {
                Binder::Typed { name, .. }
                | Binder::Range { name, .. }
                | Binder::Collection { name, .. } => name,
            };
            let mut local = env.clone();
            local.insert(name.clone(), binder_ty);
            output.insert("kind".to_owned(), json!(quantifier));
            output.insert("quantifier".to_owned(), json!(quantifier));
            output.insert(
                "binder".to_owned(),
                binder_json(binder, env, model, path, span)?,
            );
            output.insert(
                "body".to_owned(),
                expr_json(body, &local, model, path, span, Some(&TypeRef::Bool))?,
            );
        }
        Expr::Count {
            name,
            type_name,
            condition,
        } => {
            let domain = qualified_name(type_name)?;
            let mut local = env.clone();
            local.insert(name.clone(), TypeRef::Named(domain.clone()));
            output.insert("kind".to_owned(), json!("count"));
            output.insert("binding".to_owned(), json!(name));
            output.insert("domain".to_owned(), json!(domain));
            output.insert(
                "condition".to_owned(),
                expr_json(condition, &local, model, path, span, Some(&TypeRef::Bool))?,
            );
        }
        Expr::Sum {
            name,
            type_name,
            body,
            condition,
        } => {
            let domain = qualified_name(type_name)?;
            let mut local = env.clone();
            local.insert(name.clone(), TypeRef::Named(domain.clone()));
            output.insert("kind".to_owned(), json!("sum"));
            output.insert("binding".to_owned(), json!(name));
            output.insert("domain".to_owned(), json!(domain));
            output.insert(
                "value".to_owned(),
                expr_json(body, &local, model, path, span, Some(&TypeRef::Int))?,
            );
            output.insert(
                "condition".to_owned(),
                condition.as_deref().map_or(Ok(Value::Null), |condition| {
                    expr_json(condition, &local, model, path, span, Some(&TypeRef::Bool))
                })?,
            );
        }
        Expr::UnaryNamed { name, expr, .. } => {
            output.insert("kind".to_owned(), json!(name));
            output.insert(
                "operand".to_owned(),
                expr_json(expr, env, model, path, span, None)?,
            );
        }
        Expr::BinaryNamed { name, left, right } => {
            output.insert("kind".to_owned(), json!(name));
            output.insert(
                "arguments".to_owned(),
                json!([
                    expr_json(left, env, model, path, span, Some(&TypeRef::Int))?,
                    expr_json(right, env, model, path, span, Some(&TypeRef::Int))?
                ]),
            );
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => {
            output.insert("kind".to_owned(), json!(name));
            output.insert(
                "relation".to_owned(),
                expr_json(first, env, model, path, span, None)?,
            );
            output.insert(
                "source".to_owned(),
                expr_json(second, env, model, path, span, None)?,
            );
            output.insert(
                "target".to_owned(),
                expr_json(third, env, model, path, span, None)?,
            );
        }
        Expr::BinderNamed { name, binder } => {
            output.insert("kind".to_owned(), json!(name));
            output.insert(
                "binder".to_owned(),
                binder_json(binder, env, model, path, span)?,
            );
        }
    }
    Ok(Value::Object(output))
}

fn lvalue_type(
    target: &LValue,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<TypeRef, PublicKernelError> {
    match target {
        LValue::Var(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| error(format!("unknown update target '{name}'"))),
        LValue::Index(name, _) => match resolve(
            model,
            env.get(name)
                .ok_or_else(|| error(format!("unknown update target '{name}'")))?,
        )? {
            TypeRef::Map(_, value) | TypeRef::Relation(_, value) => Ok(*value),
            _ => Err(error("indexed update target requires Map or Relation")),
        },
        LValue::Field(base, field) => {
            let TypeRef::Named(name) = lvalue_type(base, env, model)? else {
                return Err(error("field update target requires named struct"));
            };
            match model.types.get(&name) {
                Some(TypeDef::Struct { fields }) => fields
                    .iter()
                    .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
                    .ok_or_else(|| error(format!("unknown struct field '{name}.{field}'"))),
                _ => Err(error("field update target requires struct")),
            }
        }
    }
}

fn lvalue_json(
    target: &LValue,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
) -> Result<Value, PublicKernelError> {
    let ty = lvalue_type(target, env, model)?;
    Ok(match target {
        LValue::Var(name) => json!({
            "kind":"var","type":type_json(&ty),"span":span_json(path,span),"name":name
        }),
        LValue::Index(name, index) => {
            let (TypeRef::Map(key, _) | TypeRef::Relation(key, _)) = resolve(
                model,
                env.get(name)
                    .ok_or_else(|| error(format!("unknown update target '{name}'")))?,
            )?
            else {
                return Err(error("indexed update target requires Map or Relation"));
            };
            json!({
                "kind":"index","type":type_json(&ty),"span":span_json(path,span),
                "name":name,"index":expr_json(index,env,model,path,span,Some(&key))?
            })
        }
        LValue::Field(base, field) => json!({
            "kind":"field_lv","type":type_json(&ty),"span":span_json(path,span),
            "target":lvalue_json(base,env,model,path,span)?,"field":field
        }),
    })
}

fn statement_json(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
) -> Result<Value, PublicKernelError> {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => {
            let ty = lvalue_type(target, env, model)?;
            Ok(json!({
                "kind":"assign","type":{"kind":"statement"},"span":span_json(path,*span),
                "target":lvalue_json(target,env,model,path,*span)?,
                "value":expr_json(value,env,model,path,*span,Some(&ty))?
            }))
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => Ok(json!({
            "kind":"if","type":{"kind":"statement"},"span":span_json(path,*span),
            "condition":expr_json(condition,env,model,path,*span,Some(&TypeRef::Bool))?,
            "then":then_statements.iter().map(|item|statement_json(item,env,model,path)).collect::<Result<Vec<_>,_>>()?,
            "else":else_statements.iter().map(|item|statement_json(item,env,model,path)).collect::<Result<Vec<_>,_>>()?
        })),
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let ty = binder_type(binder, env, model)?;
            let name = match binder {
                Binder::Typed { name, .. }
                | Binder::Range { name, .. }
                | Binder::Collection { name, .. } => name,
            };
            let mut local = env.clone();
            local.insert(name.clone(), ty);
            Ok(json!({
                "kind":"forall","type":{"kind":"statement"},"span":span_json(path,*span),
                "binder":binder_json(binder,env,model,path,*span)?,
                "statements":statements.iter().map(|item|statement_json(item,&local,model,path)).collect::<Result<Vec<_>,_>>()?
            }))
        }
    }
}

fn requirement(meta: Option<&MetaTag>) -> Value {
    meta.map_or(Value::Null, |meta| json!({"id":meta.id,"text":meta.text}))
}

fn origin(meta: Option<&MetaTag>, dialect: &str, name: &str, generated: bool) -> Value {
    json!({
        "dialect":dialect,
        "declaration":meta.map_or(name,|meta|meta.id.as_str()),
        "lowered":dialect != "kernel",
        "generated":generated || name.starts_with('_'),
    })
}

#[allow(clippy::too_many_lines)]
fn walk_partial(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
    output: &mut Vec<Value>,
) -> Result<(), PublicKernelError> {
    if let Expr::Method {
        receiver,
        name,
        args,
    } = expr
        && matches!(name.as_str(), "head" | "pop" | "at")
    {
        let size = Expr::Method {
            receiver: receiver.clone(),
            name: "size".to_owned(),
            args: Vec::new(),
        };
        let failure = if name == "at" {
            Expr::Binary {
                op: "or".to_owned(),
                left: Box::new(Expr::Binary {
                    op: "<".to_owned(),
                    left: Box::new(args[0].clone()),
                    right: Box::new(Expr::Num(0)),
                }),
                right: Box::new(Expr::Binary {
                    op: ">=".to_owned(),
                    left: Box::new(args[0].clone()),
                    right: Box::new(size),
                }),
            }
        } else {
            Expr::Binary {
                op: "==".to_owned(),
                left: Box::new(size),
                right: Box::new(Expr::Num(0)),
            }
        };
        output.push(json!({
            "operation":name,
            "failure_condition":expr_json(&failure,env,model,path,span,Some(&TypeRef::Bool))?,
            "state_effect_on_failure":"rollback",
            "span":span_json(path,span),
        }));
    }
    if let Expr::Index(collection, index) = expr
        && matches!(
            resolve(model, &infer_type(collection, env, model, None)?)?,
            TypeRef::Seq(_, _)
        )
    {
        let size = Expr::Method {
            receiver: collection.clone(),
            name: "size".to_owned(),
            args: Vec::new(),
        };
        let failure = Expr::Binary {
            op: "or".to_owned(),
            left: Box::new(Expr::Binary {
                op: "<".to_owned(),
                left: index.clone(),
                right: Box::new(Expr::Num(0)),
            }),
            right: Box::new(Expr::Binary {
                op: ">=".to_owned(),
                left: index.clone(),
                right: Box::new(size),
            }),
        };
        output.push(json!({
            "operation":"index",
            "failure_condition":expr_json(&failure,env,model,path,span,Some(&TypeRef::Bool))?,
            "state_effect_on_failure":"rollback",
            "span":span_json(path,span),
        }));
    }
    if let Expr::Binary { op, right, .. } = expr
        && matches!(op.as_str(), "/" | "%")
    {
        let failure = Expr::Binary {
            op: "==".to_owned(),
            left: right.clone(),
            right: Box::new(Expr::Num(0)),
        };
        output.push(json!({
            "operation":if op == "/" {"divide"} else {"remainder"},
            "failure_condition":expr_json(&failure,env,model,path,span,Some(&TypeRef::Bool))?,
            "state_effect_on_failure":"rollback",
            "span":span_json(path,span),
        }));
    }
    let mut children: Vec<&Expr> = Vec::new();
    match expr {
        Expr::Some(item) | Expr::Neg(item) | Expr::Not(item) => children.push(item),
        Expr::Set(items) | Expr::Seq(items) => children.extend(items),
        Expr::Struct { fields, .. } => children.extend(fields.iter().map(|(_, item)| item)),
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => {
            children.push(left);
            children.push(right);
        }
        Expr::Field(value, _) | Expr::UnaryNamed { expr: value, .. } => children.push(value),
        Expr::Method { receiver, args, .. } => {
            children.push(receiver);
            children.extend(args);
        }
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => children.extend([condition.as_ref(), then_expr.as_ref(), else_expr.as_ref()]),
        Expr::Is { expr, .. } => children.push(expr),
        Expr::Quantified { body, .. } => children.push(body),
        Expr::Count { condition, .. } => children.push(condition),
        Expr::Sum {
            body, condition, ..
        } => {
            children.push(body);
            if let Some(condition) = condition {
                children.push(condition);
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => children.extend([first.as_ref(), second.as_ref(), third.as_ref()]),
        Expr::Num(_)
        | Expr::Bool(_)
        | Expr::None
        | Expr::Var(_)
        | Expr::Call { .. }
        | Expr::BinderNamed { .. } => {}
    }
    for child in children {
        walk_partial(child, env, model, path, span, output)?;
    }
    Ok(())
}

fn statement_partial(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    output: &mut Vec<Value>,
) -> Result<(), PublicKernelError> {
    match statement {
        Statement::Assign { value, span, .. } => {
            walk_partial(value, env, model, path, *span, output)?;
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            walk_partial(condition, env, model, path, *span, output)?;
            for item in then_statements.iter().chain(else_statements) {
                statement_partial(item, env, model, path, output)?;
            }
        }
        Statement::ForAll {
            statements, span, ..
        } => {
            for item in statements {
                statement_partial(item, env, model, path, output)?;
            }
            let _ = span;
        }
    }
    Ok(())
}

fn source_property_kinds(kernel: &KernelSpec) -> BTreeMap<String, String> {
    let mut kinds = BTreeMap::new();
    for item in &kernel.syntax().items {
        match item {
            SpecItem::Invariant { name, .. } => {
                kinds.insert(name.clone(), "invariant".to_owned());
            }
            SpecItem::Trans { name, .. } => {
                kinds.insert(name.clone(), "trans".to_owned());
            }
            SpecItem::Reachable { name, .. } => {
                kinds.insert(name.clone(), "reachable".to_owned());
            }
            SpecItem::LeadsTo { name, .. } => {
                kinds.insert(name.clone(), "leadsto".to_owned());
            }
            SpecItem::Unless { name, .. } => {
                kinds.insert(name.clone(), "unless".to_owned());
            }
            SpecItem::Until { name, .. } => {
                kinds.insert(name.clone(), "until".to_owned());
                kinds.insert(format!("{name}_until_safety"), "until".to_owned());
            }
            _ => {}
        }
    }
    kinds
}

/// Project a checked, lowered model into the stable public Kernel JSON value.
///
/// # Errors
///
/// Returns [`PublicKernelError`] instead of dropping any expression or type
/// that the public schema cannot represent.
#[allow(clippy::too_many_lines)]
pub fn public_kernel_contract(
    kernel: &KernelSpec,
    model: &KernelModel,
    source_path: &str,
    dialect: &str,
) -> Result<Value, PublicKernelError> {
    if dialect == "compose" {
        return Err(error(
            "public Kernel v1 cannot preserve component source filenames after compose lowering",
        ));
    }
    let env = base_env(model);
    let kinds = source_property_kinds(kernel);
    let constants = model
        .consts
        .iter()
        .map(|(name, value)| {
            let (ty, value) = match value {
                crate::FslValue::Int(value) => (type_json(&TypeRef::Int), json!(value)),
                crate::FslValue::Bool(value) => (type_json(&TypeRef::Bool), json!(value)),
                _ => return Err(error("public Kernel constants must be scalar")),
            };
            Ok(json!({
                "name":name,"type":ty,"value":value,"span":Value::Null,
                "origin":origin(None,dialect,name,false)
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let types = model
        .types
        .iter()
        .map(|(name, definition)| {
            let (symmetric, definition) = match definition {
                TypeDef::Domain { lo, hi, symmetric } => {
                    (*symmetric, json!({"kind":"domain","lo":lo,"hi":hi}))
                }
                TypeDef::Enum { members, symmetric } => {
                    (*symmetric, json!({"kind":"enum","members":members}))
                }
                TypeDef::Struct { fields } => {
                    let mut fields = fields.clone();
                    fields.sort_by(|left, right| left.0.cmp(&right.0));
                    (false, json!({
                        "kind":"struct",
                        "fields":fields.iter().map(|(field,ty)|json!({"name":field,"type":type_json(ty)})).collect::<Vec<_>>()
                    }))
                }
            };
            json!({
                "name":name,"span":Value::Null,"symmetric":symmetric,"definition":definition,
                "origin":origin(None,dialect,name,false)
            })
        })
        .collect::<Vec<_>>();
    let mut state = model.state.clone();
    state.sort_by(|left, right| left.0.cmp(&right.0));
    let state = state
        .iter()
        .map(|(name, ty)| {
            json!({
                "name":name,"type":type_json(ty),"span":Value::Null,
                "origin":origin(None,dialect,name,false)
            })
        })
        .collect::<Vec<_>>();
    let init = model
        .init
        .iter()
        .map(|statement| statement_json(statement, &env, model, source_path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut action_refs = model.actions.iter().collect::<Vec<_>>();
    action_refs.sort_by(|left, right| left.name.cmp(&right.name));
    let actions = action_refs
        .into_iter()
        .map(|action| {
            let mut local = env.clone();
            let parameters = action
                .params
                .iter()
                .map(|param| match param {
                    ParamDef::Typed { name, ty } => {
                        local.insert(name.clone(), ty.clone());
                        let (lo, hi) = model
                            .domain_values(ty)
                            .map_err(|err| error(err.message))?
                            .into_iter()
                            .enumerate()
                            .fold((i64::MAX, i64::MIN), |(lo, hi), (index, value)| {
                                let value = match value {
                                    crate::FslValue::Int(value) => value,
                                    crate::FslValue::Bool(value) => i64::from(value),
                                    crate::FslValue::Enum { .. } => i64::try_from(index).unwrap_or_default(),
                                    _ => 0,
                                };
                                (lo.min(value), hi.max(value))
                            });
                        Ok(json!({"name":name,"type":type_json(ty),"finite_domain":{"lo":lo,"hi":hi}}))
                    }
                    ParamDef::Range { name, lo, hi } => {
                        let ty = TypeRef::Range(*lo, *hi);
                        local.insert(name.clone(), ty.clone());
                        Ok(json!({"name":name,"type":type_json(&ty),"finite_domain":{"lo":lo,"hi":hi}}))
                    }
                })
                .collect::<Result<Vec<_>, PublicKernelError>>()?;
            let mut requires = Vec::new();
            let mut lets = Vec::new();
            let mut guards = Vec::new();
            let mut partial = Vec::new();
            let mut require_spans = action.require_spans.iter();
            for guard in &action.guards {
                match guard {
                    ActionGuard::Requires(expression) => {
                        let span = *require_spans.next().ok_or_else(|| {
                            error(format!("missing requires span for action '{}'", action.name))
                        })?;
                        let value = expr_json(
                            expression,
                            &local,
                            model,
                            source_path,
                            span,
                            Some(&TypeRef::Bool),
                        )?;
                        walk_partial(
                            expression,
                            &local,
                            model,
                            source_path,
                            span,
                            &mut partial,
                        )?;
                        requires.push(value.clone());
                        guards.push(json!({"kind":"requires","expression":value}));
                        extend_pattern_binding(expression, &mut local, model)?;
                    }
                    ActionGuard::Let(name, expression) => {
                        let value = expr_json(
                            expression,
                            &local,
                            model,
                            source_path,
                            action.span,
                            None,
                        )?;
                        walk_partial(
                            expression,
                            &local,
                            model,
                            source_path,
                            action.span,
                            &mut partial,
                        )?;
                        let ty = infer_type(expression, &local, model, None)?;
                        local.insert(name.clone(), ty);
                        lets.push(json!({"name":name,"value":value}));
                        guards.push(json!({"kind":"let","name":name,"value":value}));
                    }
                }
            }
            let updates = action
                .statements
                .iter()
                .map(|statement| statement_json(statement, &local, model, source_path))
                .collect::<Result<Vec<_>, _>>()?;
            let ensures = action
                .ensures
                .iter()
                .zip(&action.ensure_spans)
                .map(|(expression, span)| {
                    expr_json(
                        expression,
                        &local,
                        model,
                        source_path,
                        *span,
                        Some(&TypeRef::Bool),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            for expression in &action.ensures {
                walk_partial(
                    expression,
                    &local,
                    model,
                    source_path,
                    action.span,
                    &mut partial,
                )?;
            }
            for statement in &action.statements {
                statement_partial(
                    statement,
                    &local,
                    model,
                    source_path,
                    &mut partial,
                )?;
            }
            Ok(json!({
                "name":action.name,"parameters":parameters,"fair":action.fair,
                "guards":guards,"requires":requires,"lets":lets,"updates":updates,
                "update_semantics":"simultaneous","ensures":ensures,
                "partial_operations":partial,"requirement":requirement(action.meta.as_ref()),
                "origin":origin(action.meta.as_ref(),dialect,&action.name,false),
                "span":span_json(source_path,action.span),
            }))
        })
        .collect::<Result<Vec<_>, PublicKernelError>>()?;
    let property_list = |items: &[crate::PropertyDef], default_kind: &str| {
        let mut items = items.iter().collect::<Vec<_>>();
        items.sort_by(|left, right| left.name.cmp(&right.name));
        items
            .into_iter()
            .map(|item| {
                Ok(json!({
                    "name":item.name,
                    "source_kind":kinds.get(&item.name).map_or(default_kind,String::as_str),
                    "expression":expr_json(&item.expr,&env,model,source_path,item.span,Some(&TypeRef::Bool))?,
                    "requirement":requirement(item.meta.as_ref()),
                    "origin":origin(item.meta.as_ref(),dialect,&item.name,false),
                    "span":span_json(source_path,item.span),
                }))
            })
            .collect::<Result<Vec<_>, PublicKernelError>>()
    };
    let mut leads = model.leadstos.iter().collect::<Vec<_>>();
    leads.sort_by(|left, right| left.name.cmp(&right.name));
    let leads = leads
        .into_iter()
        .map(|item| {
            let mut local = env.clone();
            let mut binders = Vec::new();
            for binder in &item.binders {
                binders.push(binder_json(binder, &local, model, source_path, item.span)?);
                let name = match binder {
                    Binder::Typed { name, .. }
                    | Binder::Range { name, .. }
                    | Binder::Collection { name, .. } => name,
                };
                local.insert(name.clone(), binder_type(binder, &local, model)?);
            }
            Ok(json!({
                "name":item.name,
                "source_kind":kinds.get(&item.name).map_or("leadsTo",String::as_str),
                "binders":binders,
                "before":expr_json(&item.before,&local,model,source_path,item.span,Some(&TypeRef::Bool))?,
                "after":expr_json(&item.after,&local,model,source_path,item.span,Some(&TypeRef::Bool))?,
                "within":item.within,
                "decreases":item.decreases.as_ref().map_or(Ok(Value::Null),|expr|expr_json(expr,&local,model,source_path,item.span,Some(&TypeRef::Int)))?,
                "requirement":requirement(item.meta.as_ref()),
                "origin":origin(item.meta.as_ref(),dialect,&item.name,false),
                "span":span_json(source_path,item.span),
            }))
        })
        .collect::<Result<Vec<_>, PublicKernelError>>()?;
    let terminal_span = kernel.syntax().items.iter().find_map(|item| match item {
        SpecItem::Terminal { span, .. } => Some(*span),
        _ => None,
    });
    let terminal = match (&model.terminal, terminal_span) {
        (Some(expression), Some(span)) => json!({
            "source_kind":"terminal",
            "expression":expr_json(expression,&env,model,source_path,span,Some(&TypeRef::Bool))?,
            "span":span_json(source_path,span),
        }),
        _ => Value::Null,
    };
    Ok(json!({
        "$schema":KERNEL_SCHEMA_ID,
        "schema_version":KERNEL_SCHEMA_VERSION,
        "language_version":"1.0",
        "spec":{"name":model.name,"source":{"file":source_path,"dialect":dialect}},
        "semantics":{
            "assignment":"simultaneous","reads":"pre_state","requires_false":"not_enabled",
            "failure_state":"rollback","old":"pre_state","integer_division":"euclidean",
            "terminal_deadlock":"terminal_states_excluded","fairness":"weak"
        },
        "constants":constants,"types":types,"state":state,
        "init":{
            "statements":init,
            "requirement":requirement(model.init_meta.as_ref()),
            "origin":origin(model.init_meta.as_ref(),dialect,"init",false),
            "span":model.init.first().map_or(Value::Null,|statement|span_json(source_path,statement_span(statement)))
        },
        "actions":actions,
        "properties":{
            "invariants":property_list(&model.invariants,"invariant")?,
            "transitions":property_list(&model.transitions,"trans")?,
            "reachables":property_list(&model.reachables,"reachable")?,
            "leads_to":leads,"terminal":terminal,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_model, parse_direct_kernel_spec};

    #[test]
    fn unsupported_calls_fail_instead_of_disappearing() {
        let source = "spec S { state { x: Int } init { x = 0 } action a() { x = 0 } invariant I { x == 0 } }";
        let kernel = parse_direct_kernel_spec(source).expect("parse");
        let model = build_model(kernel.clone()).expect("model");
        let contract =
            public_kernel_contract(&kernel, &model, "s.fsl", "kernel").expect("contract");
        assert_eq!(contract["schema_version"], "1.0.0");
    }
}
