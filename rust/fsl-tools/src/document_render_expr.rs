// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Safe-pattern expression-to-controlled-language rendering (issue #326).
//!
//! Only a fixed whitelist of expression shapes is turned into a natural-
//! language sentence fragment. Everything else falls back to the canonical
//! FSL text `fsl_core::source_expr_text` already produces (the same text
//! `explain --readable` shows) inside a fenced `fsl`-tagged code block.
//! Falling back is never a failure: an unnatural paraphrase is a worse error
//! than a visible canonical expression.
//!
//! This module never rewrites boolean structure (no De Morgan, no
//! factoring, no reordering) and never invents an operator's meaning: it
//! only recognizes shapes and fills a fixed template per shape.

use std::fmt::Write as _;

use fsl_core::{
    KernelAggregateKind as AggregateKind, KernelBinder as Binder, KernelExpr as Expr, KernelModel,
    Pattern, source_binder_text,
};

use crate::document_render::Locale;

/// Longest canonical code-span text allowed inside a natural-language atom
/// before the whole enclosing proposition falls back (issue #326 §4.0).
const MAX_ATOM_CODE_SPAN: usize = 60;

fn code(model: &KernelModel, expr: &Expr) -> String {
    fsl_core::source_expr_text(model, expr)
}

fn cs(text: &str) -> String {
    format!("`{text}`")
}

fn too_long(text: &str) -> bool {
    text.chars().count() > MAX_ATOM_CODE_SPAN
}

/// One recognized, already-worded proposition fragment.
enum Shape {
    Inline(String),
    Chain { kind: ChainKind, items: Vec<String> },
}

#[derive(Clone, Copy)]
enum ChainKind {
    And,
    Or,
    NAnd,
}

/// The rendered condition block for one claim (a full section body, not a
/// bare sentence fragment), plus whether it used a canonical fallback.
pub(crate) struct Rendered {
    pub(crate) text: String,
    pub(crate) used_fallback: bool,
}

/// Render one top-level condition (an invariant/trans/reachable body, a
/// leadsTo before/after side, or an enablement guard) as a standalone
/// markdown block: either a fused natural-language sentence (ending in the
/// locale's full stop) or a fallback introduction plus a fenced canonical
/// block.
pub(crate) fn render_condition(expr: &Expr, model: &KernelModel, locale: Locale) -> Rendered {
    match render_shape(expr, model, locale, None) {
        Some(Shape::Inline(text)) => Rendered {
            text: format!("{text}{}", stop(locale)),
            used_fallback: false,
        },
        Some(Shape::Chain { kind, items }) => Rendered {
            text: chain_block(kind, None, &items, locale),
            used_fallback: false,
        },
        None => Rendered {
            text: fallback_block(expr, model, locale),
            used_fallback: true,
        },
    }
}

/// Render an inline proposition for embedding inside a larger sentence
/// (e.g. an acceptance/forbidden "Then" clause). Returns `None` when the
/// expression is not a recognized safe pattern; the caller falls back and
/// counts it.
pub(crate) fn render_inline(expr: &Expr, model: &KernelModel, locale: Locale) -> Option<String> {
    match render_shape(expr, model, locale, None)? {
        Shape::Inline(text) => Some(text),
        Shape::Chain { .. } => None,
    }
}

fn stop(locale: Locale) -> &'static str {
    match locale {
        Locale::Ja => "。",
        Locale::En => ".",
    }
}

fn render_shape(
    expr: &Expr,
    model: &KernelModel,
    locale: Locale,
    quantifier_prefix: Option<&str>,
) -> Option<Shape> {
    if let Expr::Quantified {
        quantifier,
        binder,
        body,
    } = expr
    {
        // A nested quantifier is never recognized; only the outer binder
        // reads naturally, and the inner body falls back on its own.
        if matches!(body.as_ref(), Expr::Quantified { .. }) {
            let prefix = quantifier_intro(quantifier, binder, model, locale);
            return Some(Shape::Inline(quantifier_fallback_sentence(
                &prefix, body, model, locale,
            )));
        }
        let prefix = quantifier_intro(quantifier, binder, model, locale);
        return render_shape(body, model, locale, Some(&prefix));
    }

    if let Some(items) = flatten_not_and(expr) {
        let atoms = render_atoms(&items, model, locale)?;
        return Some(with_prefix(
            Shape::Chain {
                kind: ChainKind::NAnd,
                items: atoms,
            },
            quantifier_prefix,
        ));
    }

    if let Expr::Binary { op, left, right } = expr
        && op == "=>"
    {
        let antecedent = render_operand_group(left, model, locale)?;
        let consequent = render_operand_group(right, model, locale)?;
        let text = match locale {
            Locale::Ja => format!("{antecedent}ならば、{consequent}"),
            Locale::En => format!("if {antecedent}, then {consequent}"),
        };
        return Some(with_prefix(Shape::Inline(text), quantifier_prefix));
    }

    if let Some((kind, items)) = flatten_chain(expr) {
        let atoms = render_atoms(&items, model, locale)?;
        return Some(with_prefix(
            if atoms.len() >= 3 {
                Shape::Chain { kind, items: atoms }
            } else {
                Shape::Inline(join_inline(kind, &atoms, locale))
            },
            quantifier_prefix,
        ));
    }

    let atom = render_atom(expr, model, locale)?;
    Some(with_prefix(Shape::Inline(atom), quantifier_prefix))
}

fn quantifier_fallback_sentence(
    prefix: &str,
    body: &Expr,
    model: &KernelModel,
    locale: Locale,
) -> String {
    let lead = match locale {
        Locale::Ja => format!("{prefix}、次が成立する。"),
        Locale::En => format!("{}, the following holds.", capitalize(prefix)),
    };
    format!("{lead}\n\n{}", fenced(&code(model, body)))
}

fn with_prefix(shape: Shape, quantifier_prefix: Option<&str>) -> Shape {
    let Some(prefix) = quantifier_prefix else {
        return shape;
    };
    match shape {
        Shape::Inline(text) => Shape::Inline(format!("{prefix}、{text}")),
        Shape::Chain { kind, items } => Shape::Chain { kind, items },
    }
}

fn quantifier_intro(
    quantifier: &str,
    binder: &Binder,
    model: &KernelModel,
    locale: Locale,
) -> String {
    let binder_text = source_binder_text(model, binder);
    match (quantifier, locale) {
        ("forall", Locale::Ja) => format!("すべての {} について", cs(&binder_text)),
        ("forall", Locale::En) => format!("for every {}", cs(&binder_text)),
        (_, Locale::Ja) => format!("ある {} が存在して", cs(&binder_text)),
        (_, Locale::En) => format!("there exists {} such that", cs(&binder_text)),
    }
}

/// A group usable as one side of `=>`: a bare atom, or a parenthesized
/// same-connective group of 2-3 atoms.
fn render_operand_group(expr: &Expr, model: &KernelModel, locale: Locale) -> Option<String> {
    if let Some((kind, items)) = flatten_chain(expr) {
        if items.len() > 3 {
            return None;
        }
        let atoms = render_atoms(&items, model, locale)?;
        return Some(paren_group(kind, &atoms, locale));
    }
    render_atom(expr, model, locale)
}

fn paren_group(kind: ChainKind, atoms: &[String], locale: Locale) -> String {
    let joined = match (kind, locale) {
        (ChainKind::Or, Locale::Ja) => atoms.join("、または、"),
        (ChainKind::Or, Locale::En) => join_en(atoms, "or"),
        (ChainKind::And | ChainKind::NAnd, Locale::Ja) => atoms.join("、かつ、"),
        (ChainKind::And | ChainKind::NAnd, Locale::En) => join_en(atoms, "and"),
    };
    match locale {
        Locale::Ja => format!("（{joined}）"),
        Locale::En => format!("({joined})"),
    }
}

fn join_en(atoms: &[String], connective: &str) -> String {
    match atoms {
        [] => String::new(),
        [only] => only.clone(),
        [a, b] => format!("{a}, {connective} {b}"),
        _ => {
            let (last, head) = atoms.split_last().expect("non-empty");
            format!("{}, {connective} {last}", head.join(", "))
        }
    }
}

fn join_inline(kind: ChainKind, atoms: &[String], locale: Locale) -> String {
    match (kind, locale) {
        (ChainKind::Or, Locale::Ja) => atoms.join("、または、"),
        (ChainKind::Or, Locale::En) => join_en(atoms, "or"),
        (_, Locale::Ja) => atoms.join("、かつ、"),
        (_, Locale::En) => join_en(atoms, "and"),
    }
}

fn chain_block(
    kind: ChainKind,
    quantifier_prefix: Option<&str>,
    items: &[String],
    locale: Locale,
) -> String {
    let intro = chain_intro(kind, quantifier_prefix, locale);
    let mut text = intro;
    for (index, item) in items.iter().enumerate() {
        text.push('\n');
        text.push('\n');
        let _ = write!(text, "{}. {item}{}", index + 1, stop(locale));
    }
    text
}

fn chain_intro(kind: ChainKind, quantifier_prefix: Option<&str>, locale: Locale) -> String {
    let body = match (kind, locale) {
        (ChainKind::And, Locale::Ja) => "次のすべてが成立する。",
        (ChainKind::And, Locale::En) => "All of the following hold.",
        (ChainKind::Or, Locale::Ja) => "次の少なくとも一つが成立する。",
        (ChainKind::Or, Locale::En) => "At least one of the following holds.",
        (ChainKind::NAnd, Locale::Ja) => "次のすべてが同時に成立することはない。",
        (ChainKind::NAnd, Locale::En) => "The following do not all hold at the same time.",
    };
    match quantifier_prefix {
        None => body.to_owned(),
        Some(prefix) => match locale {
            Locale::Ja => format!("{prefix}、{body}"),
            Locale::En => format!("{}, {}", capitalize(prefix), lowercase_first(body)),
        },
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_lowercase(), chars.as_str()),
        None => String::new(),
    }
}

/// Flatten a left-associative `and`/`or` chain of two or more operands.
fn flatten_chain(expr: &Expr) -> Option<(ChainKind, Vec<&Expr>)> {
    let Expr::Binary { op, .. } = expr else {
        return None;
    };
    let kind = match op.as_str() {
        "and" => ChainKind::And,
        "or" => ChainKind::Or,
        _ => return None,
    };
    let mut items = Vec::new();
    flatten_into(expr, op, &mut items);
    (items.len() >= 2).then_some((kind, items))
}

fn flatten_into<'a>(expr: &'a Expr, op: &str, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        op: this_op,
        left,
        right,
    } = expr
        && this_op == op
    {
        flatten_into(left, op, out);
        flatten_into(right, op, out);
        return;
    }
    out.push(expr);
}

/// Recognize `not (A and B and ...)` where every conjunct is an atom.
fn flatten_not_and(expr: &Expr) -> Option<Vec<&Expr>> {
    let Expr::Not(inner) = expr else {
        return None;
    };
    let (ChainKind::And, items) = flatten_chain(inner)? else {
        return None;
    };
    Some(items)
}

fn render_atoms(exprs: &[&Expr], model: &KernelModel, locale: Locale) -> Option<Vec<String>> {
    exprs
        .iter()
        .map(|expr| render_atom(expr, model, locale))
        .collect()
}

/// A term used as an operand of a comparison, with `old(...)` unwrapped and
/// a `count`/`sum` aggregate rendered as a noun phrase when possible
/// (issue #326 §4.6). `too_long` never rejects a phrase-form operand: the
/// 60-character canonical-code-span limit applies to code spans, not to an
/// already-natural-language noun phrase.
struct Operand {
    code_text: String,
    is_old: bool,
    is_phrase: bool,
}

impl Operand {
    fn rejected(&self) -> bool {
        !self.is_phrase && too_long(&self.code_text)
    }
}

fn operand_term(expr: &Expr, model: &KernelModel, locale: Locale) -> Operand {
    if let Expr::UnaryNamed {
        name, expr: inner, ..
    } = expr
        && name == "old"
    {
        return Operand {
            code_text: code(model, inner),
            is_old: true,
            is_phrase: false,
        };
    }
    if let Some(phrase) = render_aggregate_term(expr, model, locale) {
        return Operand {
            code_text: phrase,
            is_old: false,
            is_phrase: true,
        };
    }
    Operand {
        code_text: code(model, expr),
        is_old: false,
        is_phrase: false,
    }
}

fn operand_phrase(operand: &Operand, locale: Locale) -> String {
    if operand.is_phrase {
        return operand.code_text.clone();
    }
    let span = cs(&operand.code_text);
    if !operand.is_old {
        return span;
    }
    match locale {
        Locale::Ja => format!("遷移前の {span}"),
        Locale::En => format!("the pre-transition value of {span}"),
    }
}

fn is_enum_member(model: &KernelModel, name: &str) -> bool {
    model.enum_members.contains_key(name)
}

/// Render one atomic (non-connective) boolean expression, or `None` when it
/// is not a recognized pattern.
fn render_atom(expr: &Expr, model: &KernelModel, locale: Locale) -> Option<String> {
    match expr {
        Expr::Bool(true) => Some(match locale {
            Locale::Ja => "常に成立する".to_owned(),
            Locale::En => "always holds".to_owned(),
        }),
        Expr::Bool(false) => Some(match locale {
            Locale::Ja => "決して成立しない".to_owned(),
            Locale::En => "never holds".to_owned(),
        }),
        Expr::Not(inner) => render_negated_atom(inner, model, locale),
        Expr::Is {
            expr: inner,
            pattern,
        } => render_is_atom(inner, pattern, model, locale, false),
        Expr::Binary { op, left, right } => render_binary_atom(op, left, right, model, locale),
        Expr::Method {
            receiver,
            name,
            args,
        } if name == "contains" && args.len() == 1 => {
            render_membership_atom(receiver, &args[0], model, locale, false)
        }
        Expr::Aggregate {
            kind: AggregateKind::Unique,
            binder,
            ..
        } => render_unique(binder, model, locale, false),
        Expr::Aggregate {
            kind: AggregateKind::ExactlyOne,
            binder,
            ..
        } => render_exactly_one(binder, model, locale, false),
        Expr::Var(name) if !is_enum_member(model, name) => bool_var_atom(expr, model, locale, true),
        Expr::Field(..) | Expr::Index(..) => bool_var_atom(expr, model, locale, true),
        _ => None,
    }
}

fn render_negated_atom(inner: &Expr, model: &KernelModel, locale: Locale) -> Option<String> {
    match inner {
        Expr::Var(name) if !is_enum_member(model, name) => {
            bool_var_atom(inner, model, locale, false)
        }
        Expr::Field(..) | Expr::Index(..) => bool_var_atom(inner, model, locale, false),
        Expr::Binary { op, left, right } if matches!(op.as_str(), "==" | "!=") => {
            render_equality_atom(!matches!(op.as_str(), "!="), left, right, model, locale)
        }
        Expr::Is {
            expr: subject,
            pattern: Pattern::None,
        } => render_is_atom(subject, &Pattern::None, model, locale, true),
        Expr::Method {
            receiver,
            name,
            args,
        } if name == "contains" && args.len() == 1 => {
            render_membership_atom(receiver, &args[0], model, locale, true)
        }
        _ => None,
    }
}

fn bool_var_atom(
    expr: &Expr,
    model: &KernelModel,
    locale: Locale,
    positive: bool,
) -> Option<String> {
    let text = code(model, expr);
    if too_long(&text) {
        return None;
    }
    Some(match (locale, positive) {
        (Locale::Ja, true) => format!("{} が `true` である", cs(&text)),
        (Locale::Ja, false) => format!("{} が `false` である", cs(&text)),
        (Locale::En, true) => format!("{} is `true`", cs(&text)),
        (Locale::En, false) => format!("{} is `false`", cs(&text)),
    })
}

fn render_is_atom(
    subject: &Expr,
    pattern: &Pattern,
    model: &KernelModel,
    locale: Locale,
    negated: bool,
) -> Option<String> {
    let operand = operand_term(subject, model, locale);
    if operand.rejected() {
        return None;
    }
    let phrase = operand_phrase(&operand, locale);
    match (pattern, negated) {
        (Pattern::None, false) => Some(match locale {
            Locale::Ja => format!("{phrase} が `none` である"),
            Locale::En => format!("{phrase} is `none`"),
        }),
        (Pattern::None, true) => Some(match locale {
            Locale::Ja => format!("{phrase} が `none` でない"),
            Locale::En => format!("{phrase} is not `none`"),
        }),
        (Pattern::Some(name), false) => Some(match locale {
            Locale::Ja => format!("{phrase} が `some` である（その値を `{name}` と呼ぶ）"),
            Locale::En => format!("{phrase} is `some` (its value is referred to as `{name}`)"),
        }),
        (Pattern::Some(_), true) => None,
    }
}

fn render_membership_atom(
    collection: &Expr,
    element: &Expr,
    model: &KernelModel,
    locale: Locale,
    negated: bool,
) -> Option<String> {
    let collection_text = code(model, collection);
    let element_text = code(model, element);
    if too_long(&collection_text) || too_long(&element_text) {
        return None;
    }
    Some(match (locale, negated) {
        (Locale::Ja, false) => {
            format!(
                "{} が {} に含まれる",
                cs(&element_text),
                cs(&collection_text)
            )
        }
        (Locale::Ja, true) => format!(
            "{} が {} に含まれない",
            cs(&element_text),
            cs(&collection_text)
        ),
        (Locale::En, false) => format!(
            "{} is contained in {}",
            cs(&element_text),
            cs(&collection_text)
        ),
        (Locale::En, true) => format!(
            "{} is not contained in {}",
            cs(&element_text),
            cs(&collection_text)
        ),
    })
}

fn render_binary_atom(
    op: &str,
    left: &Expr,
    right: &Expr,
    model: &KernelModel,
    locale: Locale,
) -> Option<String> {
    match op {
        "==" => render_equality_atom(false, left, right, model, locale),
        "!=" => render_equality_atom(true, left, right, model, locale),
        "<" | "<=" | ">" | ">=" => render_ordering_atom(op, left, right, model, locale),
        _ => None,
    }
}

fn render_ordering_atom(
    op: &str,
    left: &Expr,
    right: &Expr,
    model: &KernelModel,
    locale: Locale,
) -> Option<String> {
    let (ja_suffix, en_phrase) = match op {
        "<" => ("より小さい", "is less than"),
        "<=" => ("以下である", "is at most"),
        ">" => ("より大きい", "is greater than"),
        _ => ("以上である", "is at least"),
    };
    let left_operand = operand_term(left, model, locale);
    let right_operand = operand_term(right, model, locale);
    if left_operand.rejected() || right_operand.rejected() {
        return None;
    }
    let left_phrase = operand_phrase(&left_operand, locale);
    let right_phrase = operand_phrase(&right_operand, locale);
    Some(match locale {
        Locale::Ja => format!("{left_phrase} が {right_phrase} {ja_suffix}"),
        Locale::En => format!("{left_phrase} {en_phrase} {right_phrase}"),
    })
}

/// Equality/inequality, dispatching to the enum-member / stage template
/// when recognized, otherwise the generic equality template.
fn render_equality_atom(
    negated: bool,
    left: &Expr,
    right: &Expr,
    model: &KernelModel,
    locale: Locale,
) -> Option<String> {
    if let Some(text) = render_enum_or_stage_equality(negated, left, right, model, locale) {
        return Some(text);
    }
    let left_operand = operand_term(left, model, locale);
    let right_operand = operand_term(right, model, locale);
    if left_operand.rejected() || right_operand.rejected() {
        return None;
    }
    let left_phrase = operand_phrase(&left_operand, locale);
    let right_phrase = operand_phrase(&right_operand, locale);
    Some(match (locale, negated) {
        (Locale::Ja, false) => format!("{left_phrase} が {right_phrase} に等しい"),
        (Locale::Ja, true) => format!("{left_phrase} が {right_phrase} に等しくない"),
        (Locale::En, false) => format!("{left_phrase} is equal to {right_phrase}"),
        (Locale::En, true) => format!("{left_phrase} is not equal to {right_phrase}"),
    })
}

/// `l == Member` where one side is a bare `Stage` expression, or a `Var`
/// resolving to an enum member (the other side is the subject term).
fn render_enum_or_stage_equality(
    negated: bool,
    left: &Expr,
    right: &Expr,
    model: &KernelModel,
    locale: Locale,
) -> Option<String> {
    let (subject, member, is_stage) = if matches!(left, Expr::Stage { .. }) {
        (left, right, true)
    } else if matches!(right, Expr::Stage { .. }) {
        (right, left, true)
    } else if matches!(right, Expr::Var(name) if is_enum_member(model, name)) {
        (left, right, false)
    } else if matches!(left, Expr::Var(name) if is_enum_member(model, name)) {
        (right, left, false)
    } else {
        return None;
    };
    let Expr::Var(member_name) = member else {
        return None;
    };
    let subject_operand = operand_term(subject, model, locale);
    if subject_operand.rejected() {
        return None;
    }
    let subject_phrase = operand_phrase(&subject_operand, locale);
    let member_span = cs(member_name);
    Some(if is_stage {
        match (locale, negated) {
            (Locale::Ja, false) => format!("{subject_phrase} が段階 {member_span} にある"),
            (Locale::Ja, true) => format!("{subject_phrase} が段階 {member_span} にない"),
            (Locale::En, false) => format!("{subject_phrase} is in stage {member_span}"),
            (Locale::En, true) => format!("{subject_phrase} is not in stage {member_span}"),
        }
    } else {
        match (locale, negated) {
            (Locale::Ja, false) => format!("{subject_phrase} が {member_span} である"),
            (Locale::Ja, true) => format!("{subject_phrase} が {member_span} でない"),
            (Locale::En, false) => format!("{subject_phrase} is {member_span}"),
            (Locale::En, true) => format!("{subject_phrase} is not {member_span}"),
        }
    })
}

fn render_unique(
    binder: &Binder,
    model: &KernelModel,
    locale: Locale,
    _negated: bool,
) -> Option<String> {
    let where_expr = binder_where(binder)?;
    let binder_base = source_binder_text(model, &strip_where(binder));
    let condition = render_inline(where_expr, model, locale)?;
    Some(match locale {
        Locale::Ja => format!(
            "{quote}を満たす各 {binder} の値は互いにすべて異なる",
            quote = quote(locale, &condition),
            binder = cs(&binder_base)
        ),
        Locale::En => format!(
            "the values are pairwise distinct across all {binder} satisfying {quote}",
            binder = cs(&binder_base),
            quote = quote(locale, &condition)
        ),
    })
}

fn render_exactly_one(
    binder: &Binder,
    model: &KernelModel,
    locale: Locale,
    _negated: bool,
) -> Option<String> {
    let binder_base = source_binder_text(model, &strip_where(binder));
    match binder_where(binder) {
        Some(where_expr) => {
            let condition = render_inline(where_expr, model, locale)?;
            Some(match locale {
                Locale::Ja => format!(
                    "{quote}を満たす {binder} がちょうど一つ存在する",
                    quote = quote(locale, &condition),
                    binder = cs(&binder_base)
                ),
                Locale::En => format!(
                    "exactly one {binder} satisfies {quote}",
                    binder = cs(&binder_base),
                    quote = quote(locale, &condition)
                ),
            })
        }
        None => Some(match locale {
            Locale::Ja => format!("{} がちょうど一つ存在する", cs(&binder_base)),
            Locale::En => format!("there is exactly one {}", cs(&binder_base)),
        }),
    }
}

fn quote(locale: Locale, text: &str) -> String {
    match locale {
        Locale::Ja => format!("「{text}」"),
        Locale::En => format!("\"{text}\""),
    }
}

fn binder_where(binder: &Binder) -> Option<&Expr> {
    match binder {
        Binder::Typed { where_expr, .. }
        | Binder::Range { where_expr, .. }
        | Binder::Collection { where_expr, .. } => where_expr.as_deref(),
    }
}

fn strip_where(binder: &Binder) -> Binder {
    match binder {
        Binder::Typed {
            name, type_name, ..
        } => Binder::Typed {
            name: name.clone(),
            type_name: type_name.clone(),
            where_expr: None,
        },
        Binder::Range { name, lo, hi, .. } => Binder::Range {
            name: name.clone(),
            lo: lo.clone(),
            hi: hi.clone(),
            where_expr: None,
        },
        Binder::Collection {
            name, collection, ..
        } => Binder::Collection {
            name: name.clone(),
            collection: collection.clone(),
            where_expr: None,
        },
    }
}

/// `count`/`sum` render as a noun phrase (a term), not a proposition; used
/// wherever an operand of a comparison is an aggregate (issue #326 §4.6).
pub(crate) fn render_aggregate_term(
    expr: &Expr,
    model: &KernelModel,
    locale: Locale,
) -> Option<String> {
    let Expr::Aggregate {
        kind,
        binder,
        value,
    } = expr
    else {
        return None;
    };
    let binder_base = source_binder_text(model, &strip_where(binder));
    let condition = match binder_where(binder) {
        Some(where_expr) => Some(render_inline(where_expr, model, locale)?),
        None => None,
    };
    match kind {
        AggregateKind::Count => Some(match (locale, &condition) {
            (Locale::Ja, Some(condition)) => format!(
                "{quote}を満たす {binder} の個数",
                quote = quote(locale, condition),
                binder = cs(&binder_base)
            ),
            (Locale::Ja, None) => format!("{} の総数", cs(&binder_base)),
            (Locale::En, Some(condition)) => format!(
                "the number of {binder} satisfying {quote}",
                binder = cs(&binder_base),
                quote = quote(locale, condition)
            ),
            (Locale::En, None) => format!("the total number of {}", cs(&binder_base)),
        }),
        AggregateKind::Sum => {
            let value_text = value.as_deref().map(|value| code(model, value))?;
            if too_long(&value_text) {
                return None;
            }
            Some(match (locale, &condition) {
                (Locale::Ja, Some(condition)) => format!(
                    "{quote}を満たす各 {binder} にわたる {value} の合計",
                    quote = quote(locale, condition),
                    binder = cs(&binder_base),
                    value = cs(&value_text)
                ),
                (Locale::Ja, None) => format!(
                    "各 {binder} にわたる {value} の合計",
                    binder = cs(&binder_base),
                    value = cs(&value_text)
                ),
                (Locale::En, Some(condition)) => format!(
                    "the sum of {value} over all {binder} satisfying {quote}",
                    value = cs(&value_text),
                    binder = cs(&binder_base),
                    quote = quote(locale, condition)
                ),
                (Locale::En, None) => format!(
                    "the sum of {value} over all {binder}",
                    value = cs(&value_text),
                    binder = cs(&binder_base)
                ),
            })
        }
        AggregateKind::Unique | AggregateKind::ExactlyOne => None,
    }
}

fn fenced(canonical: &str) -> String {
    format!("```fsl\n{canonical}\n```")
}

fn fallback_block(expr: &Expr, model: &KernelModel, locale: Locale) -> String {
    let lead = match locale {
        Locale::Ja => "次の条件が成立する（FSL canonical 形式で示す）。",
        Locale::En => "The following condition holds (shown in canonical FSL form).",
    };
    format!("{lead}\n\n{}", fenced(&code(model, expr)))
}

/// Same as [`fallback_block`] but for embedding as one numbered list item
/// (issue #326 §5's list-item fallback form).
pub(crate) fn fallback_list_item(
    expr: &Expr,
    model: &KernelModel,
    locale: Locale,
    index: usize,
) -> String {
    let lead = match locale {
        Locale::Ja => "次の条件（FSL canonical 形式で示す）:",
        Locale::En => "The following condition (shown in canonical FSL form):",
    };
    format!(
        "{index}. {lead}\n\n   ```fsl\n   {}\n   ```",
        code(model, expr)
    )
}
