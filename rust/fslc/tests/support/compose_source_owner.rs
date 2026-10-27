// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! One source-holder construction policy for every composition-owned check.
//! Values keep their original names. Imported types are copied bijectively to
//! legal standalone type identifiers; aliases are NEVER faked as value records.

use std::collections::BTreeMap;

use fsl_syntax::{ComposeItem, Expr, Param, SourceFile, SpecItem, SurfaceDocument, TypeExpr};

use super::{aggregate::Owner, matrix::SourceBundle};

pub struct Standalone {
    pub source: String,
    pub description: String,
    pub type_names: BTreeMap<String, String>,
}

fn parsed(source: &str) -> SurfaceDocument {
    fsl_syntax::parse_document(SourceFile::new(source))
        .expect("generated source parses")
        .surface
}
fn source<'a>(bundle: &'a SourceBundle, file: &str) -> &'a str {
    &bundle
        .files
        .iter()
        .find(|(f, _)| f == file)
        .expect("source file")
        .1
}

struct Types {
    names: BTreeMap<String, String>,
    definitions: BTreeMap<String, (String, SpecItem)>,
}
impl Types {
    fn collect(bundle: &SourceBundle, items: &[ComposeItem]) -> Self {
        let mut names = BTreeMap::new();
        let mut definitions = BTreeMap::new();
        for item in items {
            if let ComposeItem::Use { alias, path, .. } = item {
                let SurfaceDocument::Spec(spec) = parsed(source(bundle, path)) else {
                    panic!("component spec")
                };
                for declaration in spec.items {
                    let (SpecItem::Type { name, .. }
                    | SpecItem::Enum { name, .. }
                    | SpecItem::Struct { name, .. }) = &declaration
                    else {
                        continue;
                    };
                    let key = format!("{alias}.{name}");
                    let fresh = format!("OwnerType{}", definitions.len());
                    names.insert(key.clone(), fresh.clone());
                    names.insert(format!("{alias}__{name}"), fresh);
                    definitions.insert(key, (alias.clone(), declaration));
                }
            }
        }
        Self { names, definitions }
    }
    fn name(&self, name: &str, owner: Option<&str>) -> Result<String, String> {
        let key = owner.map_or_else(|| name.to_owned(), |alias| format!("{alias}.{name}"));
        self.names
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("unrepresentable type identity {key}"))
    }
    fn ty(&self, ty: &TypeExpr, owner: Option<&str>) -> Result<String, String> {
        Ok(match ty {
            TypeExpr::Int => "Int".into(),
            TypeExpr::Bool => "Bool".into(),
            TypeExpr::Name(n) => self.name(n, owner)?,
            TypeExpr::Range(a, b) => format!("{}..{}", integer(a)?, integer(b)?),
            TypeExpr::Map(a, b) => format!("Map<{}, {}>", self.ty(a, owner)?, self.ty(b, owner)?),
            TypeExpr::Relation(a, b) => {
                format!("relation {} -> {}", self.ty(a, owner)?, self.ty(b, owner)?)
            }
            TypeExpr::Set(t) => format!("Set<{}>", self.ty(t, owner)?),
            TypeExpr::Option(t) => format!("Option<{}>", self.ty(t, owner)?),
            TypeExpr::Seq(t, n) => format!("Seq<{}, {}>", self.ty(t, owner)?, integer(n)?),
        })
    }
    fn declarations(&self) -> Result<String, String> {
        let mut declarations = Vec::new();
        for (key, (alias, item)) in &self.definitions {
            let name = &self.names[key];
            declarations.push(match item {
                SpecItem::Type { lo, hi, .. } => {
                    format!("type {name} = {}..{}", integer(lo)?, integer(hi)?)
                }
                SpecItem::Enum { members, .. } => {
                    format!("enum {name} {{ {} }}", members.join(", "))
                }
                SpecItem::Struct { fields, .. } => format!(
                    "struct {name} {{ {} }}",
                    fields
                        .iter()
                        .map(|(n, t)| Ok(format!("{n}: {}", self.ty(t, Some(alias))?)))
                        .collect::<Result<Vec<_>, String>>()?
                        .join(", ")
                ),
                _ => unreachable!(),
            });
        }
        Ok(declarations.join("\n"))
    }
    fn rewrite(&self, text: &str, aliases: &[String]) -> Result<String, String> {
        let tokens = fsl_syntax::lex(text).expect("source fragment tokens");
        let mut out = String::new();
        let mut cursor = 0;
        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];
            let word = &text[token.span.start.offset..token.span.end.offset];
            let previous = if i == 0 {
                ""
            } else {
                &text[tokens[i - 1].span.start.offset..tokens[i - 1].span.end.offset]
            };
            let next = tokens
                .get(i + 1)
                .map(|t| &text[t.span.start.offset..t.span.end.offset]);
            if aliases.iter().any(|a| a == word) && next == Some(".") {
                let member = tokens.get(i + 2).ok_or("incomplete qualified name")?;
                let member_text = &text[member.span.start.offset..member.span.end.offset];
                if previous != ":" {
                    return Err(format!(
                        "namespace value access {word}.{member_text} has no same-named plain-spec namespace"
                    ));
                }
                let replacement = self.name(&format!("{word}.{member_text}"), None)?;
                out.push_str(&text[cursor..token.span.start.offset]);
                out.push_str(&replacement);
                cursor = member.span.end.offset;
                i += 3;
                continue;
            }
            if word.contains("__") {
                if previous != ":" && next != Some("{") {
                    return Err(format!(
                        "generated value reference {word} cannot be declared unchanged in a plain spec"
                    ));
                }
                let replacement = self.name(word, None)?;
                out.push_str(&text[cursor..token.span.start.offset]);
                out.push_str(&replacement);
                cursor = token.span.end.offset;
            }
            i += 1;
        }
        out.push_str(&text[cursor..]);
        Ok(out)
    }
    fn params(&self, params: &[Param], aliases: &[String]) -> Result<String, String> {
        params
            .iter()
            .map(|p| {
                Ok(match p {
                    Param::Typed(name, q) => {
                        let ty = if let Some(alias) = &q.namespace {
                            self.name(&format!("{alias}.{}", q.name), None)?
                        } else if matches!(q.name.as_str(), "Bool" | "Int") {
                            q.name.clone()
                        } else {
                            self.name(&q.name, None)?
                        };
                        format!("{name}: {ty}")
                    }
                    Param::Range(name, lo, hi) => format!(
                        "{name} in {}..{}",
                        self.rewrite(&fsl_core::expr_text(lo), aliases)?,
                        self.rewrite(&fsl_core::expr_text(hi), aliases)?
                    ),
                })
            })
            .collect::<Result<Vec<_>, String>>()
            .map(|v| v.join(", "))
    }
}

fn integer(expr: &Expr) -> Result<String, String> {
    match expr {
        Expr::Num(n) => Ok(n.to_string()),
        Expr::Neg(n) => Ok(format!("-{}", integer(n)?)),
        _ => Err(
            "imported type has a nonliteral bound; no scope-preserving standalone type constructed"
                .into(),
        ),
    }
}

fn action_body(text: &str, span: fsl_syntax::Span) -> String {
    body_after(text, span.start.offset)
}

fn body_after(text: &str, offset: usize) -> String {
    // Surface item spans identify the leading token, not the whole body.
    let text = &text[offset..];
    let tokens = fsl_syntax::lex(text).expect("action tokens");
    let mut parens = 0;
    let mut opening = None;
    for token in &tokens {
        let value = &text[token.span.start.offset..token.span.end.offset];
        match value {
            "(" => parens += 1,
            ")" => parens -= 1,
            "{" if parens == 0 => {
                opening = Some(token.span.end.offset);
                break;
            }
            _ => {}
        }
    }
    let start = opening.expect("action body");
    let mut depth = 1;
    for token in tokens.iter().filter(|t| t.span.start.offset >= start) {
        match &text[token.span.start.offset..token.span.end.offset] {
            "{" => depth += 1,
            "}" => {
                depth -= 1;
                if depth == 0 {
                    return text[start..token.span.start.offset].into();
                }
            }
            _ => {}
        }
    }
    panic!("generated body closes")
}

fn initializers(text: &str) -> Vec<String> {
    let mut depth = 0;
    let mut result = Vec::new();
    for token in fsl_syntax::lex(text).expect("composition tokens") {
        let word = &text[token.span.start.offset..token.span.end.offset];
        if depth == 1 && word == "init" {
            result.push(body_after(text, token.span.start.offset));
        }
        match word {
            "{" => depth += 1,
            "}" => depth -= 1,
            _ => {}
        }
    }
    result
}

fn actual_arguments(text: &str, span: fsl_syntax::Span) -> Vec<String> {
    let text = &text[span.start.offset..];
    let tokens = fsl_syntax::lex(text).expect("sync tokens");
    let mut in_refs = false;
    let mut parens = 0;
    let mut braces = 0;
    let mut start = None;
    let mut result = Vec::new();
    for token in tokens {
        let word = &text[token.span.start.offset..token.span.end.offset];
        if !in_refs {
            if word == "=" && parens == 0 {
                in_refs = true;
            } else if word == "(" {
                parens += 1;
            } else if word == ")" {
                parens -= 1;
            }
            continue;
        }
        match word {
            "(" => {
                if parens == 0 {
                    start = Some(token.span.end.offset);
                }
                parens += 1;
            }
            ")" => {
                parens -= 1;
                if parens == 0 {
                    let a =
                        text[start.take().expect("argument start")..token.span.start.offset].trim();
                    if !a.is_empty() {
                        result.push(a.into());
                    }
                }
            }
            "{" => {
                if parens == 0 {
                    break;
                }
                braces += 1;
            }
            "}" => braces -= 1,
            "," if parens == 1 && braces == 0 => {
                result.push(
                    text[start.expect("argument")..token.span.start.offset]
                        .trim()
                        .into(),
                );
                start = Some(token.span.end.offset);
            }
            _ => {}
        }
    }
    result
}

/// The ONLY composition-owner holder policy. Imports provide type identities
/// and enum labels, not fake alias-valued variables. Header actuals see no
/// additional lets; additional/glue bodies retain their original guard order.
#[allow(clippy::too_many_lines)] // Single audited policy for each lexical source owner.
pub fn standalone(bundle: &SourceBundle, owner: Owner) -> Result<Standalone, String> {
    if let Owner::Component(file) = owner {
        return Ok(Standalone {
            source: source(bundle, file).into(),
            description: format!("component:{file}"),
            type_names: BTreeMap::new(),
        });
    }
    let text = source(bundle, "s.fsl");
    let SurfaceDocument::Compose(compose) = parsed(text) else {
        panic!("composition")
    };
    if !compose.items.iter().any(
        |item| matches!(item,ComposeItem::Common(SpecItem::State(fields)) if !fields.is_empty()),
    ) {
        return Err("source has no owned state; KernelModel requires nonempty state (model.rs:1095-1097); no dummy state added".into());
    }
    let types = Types::collect(bundle, &compose.items);
    let aliases: Vec<_> = compose
        .items
        .iter()
        .filter_map(|i| {
            if let ComposeItem::Use { alias, .. } = i {
                Some(alias.clone())
            } else {
                None
            }
        })
        .collect();
    let mut declarations = vec![types.declarations()?];
    let mut initialization = Vec::new();
    for item in &compose.items {
        match item {
            ComposeItem::Common(SpecItem::State(fields)) => {
                let mut rows = Vec::new();
                for field in fields {
                    let init = field
                        .initializer
                        .as_ref()
                        .map(|e| {
                            types
                                .rewrite(&fsl_core::expr_text(e), &aliases)
                                .map(|s| format!(" = {s}"))
                        })
                        .transpose()?
                        .unwrap_or_default();
                    rows.push(format!(
                        "{}: {}{init}",
                        field.name,
                        types.ty(&field.ty, None)?
                    ));
                }
                declarations.push(format!("state {{ {} }}", rows.join(", ")));
            }
            ComposeItem::Common(SpecItem::Const { name, value }) => declarations.push(format!(
                "const {name} = {}",
                types.rewrite(&fsl_core::expr_text(value), &aliases)?
            )),
            ComposeItem::Common(SpecItem::Def { .. }) => {
                return Err(
                    "composition definitions need an independent lexical expansion reference"
                        .into(),
                );
            }
            _ => {}
        }
    }
    for body in initializers(text) {
        initialization.push(types.rewrite(&body, &aliases)?);
    }
    let (name, params, body) = match owner {
        Owner::Arguments | Owner::Additional => {
            let action = compose
                .items
                .iter()
                .find_map(|i| {
                    if let ComposeItem::SyncAction(a) = i {
                        Some(a)
                    } else {
                        None
                    }
                })
                .ok_or("no synchronized source action")?;
            let body = if matches!(owner, Owner::Arguments) {
                let args = actual_arguments(text, action.span);
                if args.is_empty() {
                    return Err("no actual expression in this source".into());
                }
                let mut lines = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let mut probe = format!("sourceArgument{i}");
                    while text.contains(&probe) {
                        probe.push('X');
                    }
                    lines.push(format!("let {probe} = {}", types.rewrite(arg, &aliases)?));
                }
                lines.join("\n")
            } else {
                types.rewrite(&action_body(text, action.span), &aliases)?
            };
            (
                action.name.clone(),
                types.params(&action.params, &aliases)?,
                body,
            )
        }
        Owner::Glue => {
            let (name, params, span) = compose
                .items
                .iter()
                .find_map(|i| {
                    if let ComposeItem::Common(SpecItem::Action {
                        name, params, span, ..
                    }) = i
                    {
                        Some((name, params, *span))
                    } else {
                        None
                    }
                })
                .ok_or("no ordinary source action")?;
            (
                name.clone(),
                types.params(params, &aliases)?,
                types.rewrite(&action_body(text, span), &aliases)?,
            )
        }
        Owner::Component(_) => unreachable!(),
    };
    Ok(Standalone {
        source: format!(
            "spec Owner {{\n{}\ninit {{ {} }}\naction {name}({params}) {{\n{body}\n}}\n}}\n",
            declarations.join("\n"),
            initialization.join("\n")
        ),
        description: owner.name().into(),
        type_names: types.names,
    })
}
