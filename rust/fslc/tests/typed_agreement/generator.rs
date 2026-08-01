// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Deterministic structural generator for the C6 typed generative /
//! metamorphic agreement suite (#537 C6 slice 1, issue #648).
//!
//! Every model is produced by iterating fixed, named axes -- no randomness,
//! no wall-clock reads -- so a failing model is reproducible from its `id`
//! alone. Two families:
//!
//! - [`domain_sweep`]: domain kind x domain size x structural shape
//!   (state-variable count, action count, guard, fairness, property kind).
//!   Fifteen `(kind, size)` pairs from `docs/LANGUAGE.md` S2's four scalar
//!   domain kinds, each crossed with a structural variant selected by index
//!   so every one of the five checkable property kinds
//!   (invariant/reachable/leadsTo/trans/terminal) is exercised at least
//!   once (`PROPERTY_KINDS.len() == 5` divides the 15-entry axis evenly).
//! - [`operation_sweep`]: the six partial operations `docs/LANGUAGE.md` S6
//!   names (`head`/`pop`/`at`/index/`divide`/`remainder`), each placed at
//!   its documented boundary. See the module doc on `relations.rs` R6 for
//!   why `head`/`pop`/`at`/index are generated only in action context.
//! - [`expression_sweep`]: one checked model per executable `Expr` variant,
//!   with four models for the four `AggregateKind` values. `Call` and
//!   `Stage` are deliberately absent because direct/dialect lowering must
//!   eliminate them before a checked Kernel model reaches an evaluator.
//!
//! `type Domain = 1..0` is this codebase's existing idiom for an empty
//! finite domain (`rust/fsl-verifier/tests/expression_agreement.rs`,
//! `bounded_verification_stops_after_initial_state_without_action_instances`).
//! Size-0 and the +-1 domain boundary are R5's concern (`relations.rs`), not
//! this generic sweep: a size-0 *state-variable* domain has no valid initial
//! value, so the size axis here starts at 1 and R5 builds the empty-domain
//! shape itself, matching how the existing corpus already uses it.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainKind {
    Range,
    Entity,
    Number,
    Enum,
}

impl DomainKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            DomainKind::Range => "range",
            DomainKind::Entity => "entity",
            DomainKind::Number => "number",
            DomainKind::Enum => "enum",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyKind {
    Invariant,
    Reachable,
    LeadsTo,
    Trans,
    Terminal,
}

impl PropertyKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            PropertyKind::Invariant => "invariant",
            PropertyKind::Reachable => "reachable",
            PropertyKind::LeadsTo => "leadsTo",
            PropertyKind::Trans => "trans",
            PropertyKind::Terminal => "terminal",
        }
    }
}

const PROPERTY_KINDS: [PropertyKind; 5] = [
    PropertyKind::Invariant,
    PropertyKind::Reachable,
    PropertyKind::LeadsTo,
    PropertyKind::Trans,
    PropertyKind::Terminal,
];

/// One generated model plus the sweep coordinates that produced it, so a
/// failing agreement check can cite the exact axis combination without
/// re-deriving it from `source`.
#[derive(Clone, Debug)]
pub struct GeneratedModel {
    pub id: String,
    pub source: String,
    pub domain_kind: DomainKind,
    pub domain_size: i64,
    pub state_vars: usize,
    pub action_count: usize,
    pub guarded: bool,
    pub fair: bool,
    pub property_kind: PropertyKind,
    pub depth: usize,
}

fn domain_axis() -> Vec<(DomainKind, i64)> {
    let mut axis = Vec::new();
    for size in [1, 2, 3, 4] {
        axis.push((DomainKind::Range, size));
    }
    for size in [1, 2, 3] {
        axis.push((DomainKind::Entity, size));
    }
    for size in [1, 2, 3, 4] {
        axis.push((DomainKind::Number, size));
    }
    for size in [1, 2, 3, 4] {
        axis.push((DomainKind::Enum, size));
    }
    axis
}

/// The domain x structure grid described in the module doc.
#[must_use]
pub fn domain_sweep() -> Vec<GeneratedModel> {
    domain_axis()
        .into_iter()
        .enumerate()
        .map(|(index, (kind, size))| {
            build_domain_model(
                index,
                kind,
                size,
                PROPERTY_KINDS[index % PROPERTY_KINDS.len()],
            )
        })
        .collect()
}

/// One replayable FSL Logic Test case. Coordinates are retained separately
/// from the source so failures can be shrunk and regenerated without parsing
/// panic text.
#[derive(Clone, Debug)]
pub struct LogicCase {
    pub case_id: String,
    pub seed: u64,
    pub index: usize,
    pub domain_kind: DomainKind,
    pub domain_size: i64,
    pub property_kind: PropertyKind,
    pub state_vars: usize,
    pub action_count: usize,
    pub guarded: bool,
    pub fair: bool,
    pub expected_violation: bool,
    pub expected_violation_step: Option<usize>,
    pub source: String,
    pub depth: usize,
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

/// Regenerate one case from its stable `(seed, index)` coordinates.
#[must_use]
pub fn logic_case(seed: u64, index: usize) -> LogicCase {
    const KINDS: [DomainKind; 4] = [
        DomainKind::Range,
        DomainKind::Entity,
        DomainKind::Number,
        DomainKind::Enum,
    ];
    // leadsTo is intentionally absent: it is BMC-only and the inventory owns
    // that reviewed exclusion from common agreement.
    const COMMON_PROPERTIES: [PropertyKind; 4] = [
        PropertyKind::Invariant,
        PropertyKind::Reachable,
        PropertyKind::Trans,
        PropertyKind::Terminal,
    ];
    let bits = splitmix64(seed ^ u64::try_from(index).expect("case index fits u64"));
    let kind = KINDS[usize::try_from(bits & 3).expect("two bits fit usize")];
    let size = 1 + i64::try_from((bits >> 2) & 3).expect("two bits fit i64");
    let property_kind =
        COMMON_PROPERTIES[usize::try_from((bits >> 4) & 3).expect("two bits fit usize")];
    let render_index = 10_000 + index;
    let generated = build_domain_model(render_index, kind, size, property_kind);
    let expected_violation_step = if index.is_multiple_of(8) {
        Some(0)
    } else if index % 8 == 4 {
        Some(1)
    } else {
        None
    };
    let expected_violation = expected_violation_step.is_some();
    let mut source = generated.source;
    if let Some(step) = expected_violation_step {
        let closing = source
            .rfind("\n}\n")
            .expect("generated direct spec has a top-level closing brace");
        let control = if step == 0 {
            "  invariant LogicControl { false }\n"
        } else {
            "  action LogicTrip() { x = x  ensures false }\n"
        };
        source.insert_str(closing + 1, control);
    }
    LogicCase {
        case_id: format!("fsl-logic-v1-s{seed}-c{index}-d6"),
        seed,
        index,
        domain_kind: kind,
        domain_size: size,
        property_kind,
        state_vars: generated.state_vars,
        action_count: generated.action_count,
        guarded: generated.guarded,
        fair: generated.fair,
        expected_violation,
        expected_violation_step,
        source,
        depth: generated.depth,
    }
}

#[must_use]
pub fn logic_case_at_depth(seed: u64, index: usize, depth: usize) -> LogicCase {
    let mut case = logic_case(seed, index);
    case.depth = depth;
    case.case_id = format!("fsl-logic-v1-s{seed}-c{index}-d{depth}");
    case
}

#[must_use]
pub fn logic_cases_at_depth(seed: u64, count: usize, depth: usize) -> Vec<LogicCase> {
    (0..count)
        .map(|index| logic_case_at_depth(seed, index, depth))
        .collect()
}

fn rebuild_logic_case(mut case: LogicCase) -> LogicCase {
    case.case_id = format!(
        "fsl-logic-v1-s{}-c{}-d{}",
        case.seed, case.index, case.depth
    );
    let generated = build_domain_model_with_shape(
        10_000 + case.index,
        case.domain_kind,
        case.domain_size,
        case.property_kind,
        case.state_vars,
        case.action_count,
        case.guarded,
        case.fair,
        case.depth,
    );
    let mut source = generated.source;
    if let Some(step) = case.expected_violation_step {
        let closing = source
            .rfind("\n}\n")
            .expect("generated direct spec has a top-level closing brace");
        let control = if step == 0 {
            "  invariant LogicControl { false }\n"
        } else {
            "  action LogicTrip() { x = x  ensures false }\n"
        };
        source.insert_str(closing + 1, control);
    }
    case.source = source;
    case
}

/// Produce strictly simpler, same-coordinate structural candidates. Unlike
/// seed/index search, every candidate removes one explicit model dimension;
/// the shrinker can greedily retain only reductions that preserve the named
/// semantic failure.
#[must_use]
pub fn structural_shrink_candidates(case: &LogicCase) -> Vec<LogicCase> {
    let mut candidates = Vec::new();
    let mut push = |mut candidate: LogicCase| {
        candidate = rebuild_logic_case(candidate);
        candidates.push(candidate);
    };
    if case.expected_violation {
        let mut candidate = case.clone();
        candidate.expected_violation = false;
        candidate.expected_violation_step = None;
        push(candidate);
    }
    if case.depth > 1 {
        let mut candidate = case.clone();
        candidate.depth = 1;
        push(candidate);
    }
    if case.fair {
        let mut candidate = case.clone();
        candidate.fair = false;
        push(candidate);
    }
    if case.guarded {
        let mut candidate = case.clone();
        candidate.guarded = false;
        push(candidate);
    }
    if case.action_count > 1 {
        let mut candidate = case.clone();
        candidate.action_count = 1;
        push(candidate);
    }
    if case.state_vars > 1 {
        let mut candidate = case.clone();
        candidate.state_vars = 1;
        push(candidate);
    }
    if case.property_kind != PropertyKind::Invariant {
        let mut candidate = case.clone();
        candidate.property_kind = PropertyKind::Invariant;
        push(candidate);
    }
    if case.domain_kind != DomainKind::Range {
        let mut candidate = case.clone();
        candidate.domain_kind = DomainKind::Range;
        push(candidate);
    }
    if case.domain_size > 1 {
        let mut candidate = case.clone();
        candidate.domain_size = 1;
        push(candidate);
    }
    candidates
}

struct IntDomain {
    type_name: String,
    decl: String,
    trailer: Option<String>,
    lo: i64,
    hi: i64,
}

fn int_domain(kind: DomainKind, index: usize, size: i64) -> IntDomain {
    let hi = size - 1;
    match kind {
        DomainKind::Range => {
            let type_name = format!("D{index}");
            IntDomain {
                decl: format!("type {type_name} = 0..{hi}"),
                trailer: None,
                type_name,
                lo: 0,
                hi,
            }
        }
        DomainKind::Entity => {
            let type_name = format!("E{index}");
            IntDomain {
                decl: format!("entity {type_name}"),
                trailer: Some(format!("verify {{ instances {type_name} = {size} }}")),
                type_name,
                lo: 0,
                hi,
            }
        }
        DomainKind::Number => {
            let type_name = format!("N{index}");
            IntDomain {
                decl: format!("number {type_name}"),
                trailer: Some(format!("verify {{ values {type_name} = 0..{hi} }}")),
                type_name,
                lo: 0,
                hi,
            }
        }
        DomainKind::Enum => unreachable!("enum uses build_enum_model"),
    }
}

fn build_domain_model(
    index: usize,
    kind: DomainKind,
    size: i64,
    property_kind: PropertyKind,
) -> GeneratedModel {
    let state_vars = if index.is_multiple_of(2) { 1 } else { 2 };
    let action_count = 1 + index % 3;
    let guarded = index.is_multiple_of(2);
    let fair = index.is_multiple_of(3);
    build_domain_model_with_shape(
        index,
        kind,
        size,
        property_kind,
        state_vars,
        action_count,
        guarded,
        fair,
        6,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_domain_model_with_shape(
    index: usize,
    kind: DomainKind,
    size: i64,
    property_kind: PropertyKind,
    state_vars: usize,
    action_count: usize,
    guarded: bool,
    fair: bool,
    depth: usize,
) -> GeneratedModel {
    let id = format!(
        "domain_{}_{size}_sv{state_vars}_ac{action_count}_{}",
        kind.label(),
        property_kind.label()
    );
    let source = if matches!(kind, DomainKind::Enum) {
        render_enum_model(
            index,
            size,
            state_vars,
            action_count,
            guarded,
            fair,
            property_kind,
        )
    } else {
        render_int_model(
            int_domain(kind, index, size),
            index,
            state_vars,
            action_count,
            guarded,
            fair,
            property_kind,
        )
    };
    GeneratedModel {
        id,
        source,
        domain_kind: kind,
        domain_size: size,
        state_vars,
        action_count,
        guarded,
        fair,
        property_kind,
        depth,
    }
}

fn render_int_model(
    domain: IntDomain,
    index: usize,
    state_vars: usize,
    action_count: usize,
    guarded: bool,
    fair: bool,
    property_kind: PropertyKind,
) -> String {
    let IntDomain {
        type_name,
        decl,
        trailer,
        lo,
        hi,
    } = domain;
    let size = hi - lo + 1;
    let mut source = format!("spec IntDomain{index} {{\n  {decl}\n  state {{\n    x: {type_name}");
    if state_vars == 2 {
        source.push_str(",\n    flag: Bool");
    }
    source.push_str("\n  }\n  init {\n");
    let _ = writeln!(source, "    x = {lo}");
    if state_vars == 2 {
        source.push_str("    flag = false\n");
    }
    source.push_str("  }\n");

    let advance_prefix = if fair { "fair action" } else { "action" };
    let advance = if guarded {
        format!("{advance_prefix} advance() {{\n    requires x < {hi}\n    x = x + 1\n  }}\n")
    } else {
        format!("{advance_prefix} advance() {{\n    x = (x + 1) % {size}\n  }}\n")
    };
    let reset = format!("action reset() {{\n    requires x == {hi}\n    x = {lo}\n  }}\n");
    let extra = if state_vars == 2 {
        "action toggle() {\n    flag = not flag\n  }\n".to_owned()
    } else {
        "action hold() {\n    x = x\n  }\n".to_owned()
    };
    for action in [advance, reset, extra].into_iter().take(action_count) {
        source.push_str("  ");
        source.push_str(&action);
    }

    match property_kind {
        PropertyKind::Invariant => {
            let _ = writeln!(source, "  invariant Bounded {{ x >= {lo} and x <= {hi} }}");
        }
        PropertyKind::Reachable => {
            let _ = writeln!(source, "  reachable ReachedHi {{ x == {hi} }}");
        }
        PropertyKind::LeadsTo => {
            let _ = writeln!(source, "  leadsTo Advances {{ x == {lo} ~> x == {hi} }}");
        }
        PropertyKind::Trans => {
            let _ = writeln!(source, "  trans Monotone {{ x >= old(x) or x == {lo} }}");
        }
        PropertyKind::Terminal => {
            let _ = writeln!(source, "  invariant Bounded {{ x >= {lo} and x <= {hi} }}");
            let _ = writeln!(source, "  terminal {{ x == {hi} }}");
        }
    }
    source.push_str("}\n");
    if let Some(trailer) = trailer {
        source.push_str(&trailer);
        source.push('\n');
    }
    source
}

fn render_enum_model(
    index: usize,
    size: i64,
    state_vars: usize,
    action_count: usize,
    guarded: bool,
    fair: bool,
    property_kind: PropertyKind,
) -> String {
    let type_name = format!("Enm{index}");
    let members: Vec<String> = (0..size).map(|k| format!("M{k}")).collect();
    let lo = members
        .first()
        .expect("enum has at least one member")
        .clone();
    let hi = members
        .last()
        .expect("enum has at least one member")
        .clone();
    let mut source = format!(
        "spec EnumDomain{index} {{\n  enum {type_name} {{ {} }}\n  state {{\n    x: {type_name}",
        members.join(", ")
    );
    if state_vars == 2 {
        source.push_str(",\n    flag: Bool");
    }
    let _ = writeln!(source, "\n  }}\n  init {{\n    x = {lo}");
    if state_vars == 2 {
        source.push_str("    flag = false\n");
    }
    source.push_str("  }\n");

    let first_action_prefix = if fair { "fair action" } else { "action" };
    let mut actions = Vec::new();
    if guarded {
        for window in members.windows(2) {
            let prefix = if actions.is_empty() {
                first_action_prefix
            } else {
                "action"
            };
            actions.push(format!(
                "{prefix} advance_{}() {{\n    requires x == {}\n    x = {}\n  }}\n",
                actions.len(),
                window[0],
                window[1]
            ));
        }
        actions.push(format!(
            "action reset() {{\n    requires x == {hi}\n    x = {lo}\n  }}\n"
        ));
    } else {
        let mut chain = lo.clone();
        for window in members.windows(2).rev() {
            chain = format!(
                "if x == {} then {}\n           else {chain}",
                window[0], window[1]
            );
        }
        actions.push(format!(
            "{first_action_prefix} advance() {{\n    x = {chain}\n  }}\n"
        ));
    }
    if state_vars == 2 {
        actions.push("action toggle() {\n    flag = not flag\n  }\n".to_owned());
    } else {
        actions.push("action hold() {\n    x = x\n  }\n".to_owned());
    }
    for action in actions.into_iter().take(action_count.max(1)) {
        source.push_str("  ");
        source.push_str(&action);
    }

    match property_kind {
        PropertyKind::Invariant => {
            let _ = writeln!(source, "  invariant Known {{ x == {lo} or x != {lo} }}");
        }
        PropertyKind::Reachable => {
            let _ = writeln!(source, "  reachable ReachedHi {{ x == {hi} }}");
        }
        PropertyKind::LeadsTo => {
            let _ = writeln!(source, "  leadsTo Advances {{ x == {lo} ~> x == {hi} }}");
        }
        PropertyKind::Trans => {
            let _ = writeln!(source, "  trans StaysKnown {{ old(x) == old(x) }}");
        }
        PropertyKind::Terminal => {
            let _ = writeln!(source, "  invariant Known {{ x == {lo} or x != {lo} }}");
            let _ = writeln!(source, "  terminal {{ x == {hi} }}");
        }
    }
    source.push_str("}\n");
    source
}

/// How an expression model enters the checked Kernel.
///
/// Ordinary enum tokens remain `Expr::Var` in parsed direct specs.
/// `Expr::EnumMember` is synthesized by typed lowering paths such as enum
/// conversion and aggregate normalization, so its deterministic C6 model
/// mutates the already-parsed typed surface tree and then re-runs
/// `build_surface_model` (the public semantic gate for typed AST mutations).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpressionBuild {
    ParsedSource,
    EnumMemberTypedAst,
}

/// One deterministic expression-variant model and the matrix row(s) it
/// exercises. `aggregate_kind` is populated only for the four aggregate
/// models; all other models exercise one `Expr` row.
#[derive(Clone, Debug)]
pub struct ExpressionModel {
    pub id: String,
    pub source: String,
    pub expr_variant: &'static str,
    pub aggregate_kind: Option<&'static str>,
    pub build: ExpressionBuild,
    pub depth: usize,
}

fn expression_model(
    index: usize,
    expr_variant: &'static str,
    aggregate_kind: Option<&'static str>,
    expression: &str,
    build: ExpressionBuild,
) -> ExpressionModel {
    let source = format!(
        r"
spec ExpressionVariant{index} {{
  type Item = 0..2
  enum Status {{ Pending, Done }}
  struct Payload {{ value: Item, ready: Bool }}
  state {{
    raw: Int,
    bounded: 0..1,
    x: Item,
    flag: Bool,
    optional: Option<Item>,
    selected: Set<Item>,
    queue: Seq<Item, 3>,
    payload: Payload,
    lookup: Map<Item, Item>,
    edges: relation Item -> Item,
    status: Status
  }}
  init {{
    raw = 0
    bounded = 0
    x = 0
    flag = true
    optional = none
    selected = Set {{}}
    queue = Seq {{}}
    payload = Payload {{ value: 0, ready: true }}
    forall i: Item {{ lookup[i] = 0 }}
    edges = Set {{}}
    status = Pending
  }}
  action stay() {{ x = x }}
  invariant Variant {{ {expression} }}
}}
"
    );
    ExpressionModel {
        id: format!("expr_variant_{index}_{expr_variant}"),
        source,
        expr_variant,
        aggregate_kind,
        build,
        depth: 2,
    }
}

/// Deterministic C6 family for every checked-kernel `Expr` variant.
///
/// There are 25 models: 21 non-aggregate executable variants and four
/// aggregate-kind models. The `Expr::Aggregate` row is therefore observed
/// four times. The two remaining live syntax variants (`Call`, `Stage`) are
/// exercised by fail-closed controls in `assurance/expr.rs`, not by an
/// evaluator agreement model, because allowing either into a checked
/// Kernel would itself violate the lowering contract.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn expression_sweep() -> Vec<ExpressionModel> {
    [
        ("Expr::Num", None, "0 == 0", ExpressionBuild::ParsedSource),
        ("Expr::Bool", None, "true", ExpressionBuild::ParsedSource),
        (
            "Expr::None",
            None,
            "optional == none",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Some",
            None,
            "some(0) != optional",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Set",
            None,
            "selected == Set {}",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Seq",
            None,
            "queue == Seq {}",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Struct",
            None,
            "payload == Payload { value: 0, ready: true }",
            ExpressionBuild::ParsedSource,
        ),
        ("Expr::Var", None, "x == x", ExpressionBuild::ParsedSource),
        (
            "Expr::EnumMember",
            None,
            "status == Pending",
            ExpressionBuild::EnumMemberTypedAst,
        ),
        (
            "Expr::Index",
            None,
            "lookup[0] == 0",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Field",
            None,
            "payload.value == 0",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Method",
            None,
            "selected.size() == 0",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Binary",
            None,
            "x + 1 >= 1 and flag",
            ExpressionBuild::ParsedSource,
        ),
        ("Expr::Neg", None, "-x <= 0", ExpressionBuild::ParsedSource),
        (
            "Expr::Not",
            None,
            "not false",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Conditional",
            None,
            "if flag then x == 0 else x != 0",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Is",
            None,
            "not (optional is some(value))",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Quantified",
            None,
            "forall i: Item { lookup[i] >= 0 }",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Aggregate",
            Some("AggregateKind::Count"),
            "count(i: Item where i >= 0) == 3",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Aggregate",
            Some("AggregateKind::Sum"),
            "sum(i in 0..2 of i) == 3",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Aggregate",
            Some("AggregateKind::Unique"),
            "unique(i: Item where false) and unique(i: Item where i == 0) and not unique(i: Item where true)",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::Aggregate",
            Some("AggregateKind::ExactlyOne"),
            "not exactlyOne(i: Item where false) and exactlyOne(i: Item where i == 0) and not exactlyOne(i: Item where true)",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::UnaryNamed",
            None,
            "abs(raw) == 0",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::BinaryNamed",
            None,
            "min(raw, 1) == 0",
            ExpressionBuild::ParsedSource,
        ),
        (
            "Expr::TernaryNamed",
            None,
            "not reachable(edges, 0, 0)",
            ExpressionBuild::ParsedSource,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (expr_variant, aggregate_kind, expression, build))| {
            expression_model(index, expr_variant, aggregate_kind, expression, build)
        },
    )
    .collect()
}

/// `divide`/`remainder`, each in action context (guarded, so no partial-op
/// boundary is ever crossed here -- that boundary is exercised by the
/// dedicated, unguarded action-context tests in `relations.rs`'s R6 section
/// instead, where all four native engines must agree) and in property context,
/// where S3:561-563 guarantees totalization. `head`/`pop`/`at`/index are
/// exercised only in `relations.rs`, which owns the full automatic-boundary
/// matrix rather than duplicating it in this successful-transition sweep.
#[derive(Clone, Debug)]
pub struct OperationModel {
    pub id: String,
    pub source: String,
    pub operation: &'static str,
    pub context: &'static str,
    pub depth: usize,
}

#[must_use]
pub fn operation_sweep() -> Vec<OperationModel> {
    vec![
        OperationModel {
            id: "operation_divide_action".to_owned(),
            source: r"
spec OperationDivideAction {
  type Small = -3..3
  state { x: Small, y: Small, q: Small }
  init { x = -3 y = 0 q = 0 }
  action step() {
    requires y != 0
    q = x / y
    y = y + 1
  }
  invariant Euclid { y == 0 or x == y * (x / y) + (x % y) }
}
"
            .to_owned(),
            operation: "divide",
            context: "action",
            depth: 4,
        },
        OperationModel {
            id: "operation_remainder_action".to_owned(),
            source: r"
spec OperationRemainderAction {
  type Small = -3..3
  state { x: Small, y: Small, r: Small }
  init { x = -3 y = 0 r = 0 }
  action step() {
    requires y != 0
    r = x % y
    y = y + 1
  }
  invariant BoundedRemainder { y == 0 or (0 <= r and r < 3) or (-3 < r and r <= 0) }
}
"
            .to_owned(),
            operation: "remainder",
            context: "action",
            depth: 4,
        },
        OperationModel {
            id: "operation_divide_property".to_owned(),
            source: r"
spec OperationDivideProperty {
  type Small = -3..3
  state { x: Small }
  init { x = 0 }
  action stay() { x = x }
  invariant ZeroDivTotal { 5 / 0 == 0 and x / 0 == 0 }
}
"
            .to_owned(),
            operation: "divide",
            context: "property",
            depth: 2,
        },
        OperationModel {
            id: "operation_remainder_property".to_owned(),
            source: r"
spec OperationRemainderProperty {
  type Small = -3..3
  state { x: Small }
  init { x = 0 }
  action stay() { x = x }
  invariant ZeroModTotal { 5 % 0 == 0 and x % 0 == 0 }
}
"
            .to_owned(),
            operation: "remainder",
            context: "property",
            depth: 2,
        },
    ]
}

use std::fmt::Write as _;
