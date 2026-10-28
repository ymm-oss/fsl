// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Finite scope products and alpha-control rendering, shared with future def tests.

use std::collections::BTreeSet;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Typed,
    Range,
    Let,
    Pattern,
    Quantifier,
    Aggregate,
    Forall,
}

impl Binding {
    const TOP: [Self; 2] = [Self::Let, Self::Pattern];
    const NESTED: [Self; 3] = [Self::Quantifier, Self::Aggregate, Self::Forall];

    fn name(self) -> &'static str {
        match self {
            Self::Typed => "param-typed",
            Self::Range => "param-range",
            Self::Let => "let",
            Self::Pattern => "requires-pattern",
            Self::Quantifier => "quantifier",
            Self::Aggregate => "aggregate",
            Self::Forall => "forall-statement",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    B,
    C,
    Additional,
    Glue,
    HeaderAdditional,
    HeaderComponent,
    Argument,
}

impl Owner {
    const PARTS: [Self; 3] = [Self::B, Self::C, Self::Additional];
    const LINKS: [(Self, Self); 3] = [
        (Self::B, Self::C),
        (Self::B, Self::Additional),
        (Self::Additional, Self::B),
    ];

    fn name(self) -> &'static str {
        match self {
            Self::B => "b",
            Self::C => "c",
            Self::Additional => "additional",
            Self::Glue => "glue",
            Self::HeaderAdditional => "header-additional",
            Self::HeaderComponent => "header-component",
            Self::Argument => "argument",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Requires,
    LetValue,
    Assignment,
    If,
    ForallWhere,
    Index,
    Ensures,
    Guard,
    Body,
    Argument,
}

impl Slot {
    const ALL: [Self; 7] = [
        Self::Requires,
        Self::LetValue,
        Self::Assignment,
        Self::If,
        Self::ForallWhere,
        Self::Index,
        Self::Ensures,
    ];
    const CONTAINERS: [Self; 3] = [Self::Guard, Self::Body, Self::Ensures];

    fn name(self) -> &'static str {
        match self {
            Self::Requires => "guard-requires",
            Self::LetValue => "guard-let-value",
            Self::Assignment => "body-assignment",
            Self::If => "body-if",
            Self::ForallWhere => "body-forall-where",
            Self::Index => "body-index",
            Self::Ensures => "ensures",
            Self::Guard => "guard",
            Self::Body => "body",
            Self::Argument => "argument-value",
        }
    }

    pub fn render(self, expression: &str, ty: &str) -> (String, &'static str) {
        match self {
            Self::Requires | Self::Guard => (format!("requires {expression}\nflag = true"), "flag"),
            Self::LetValue => (
                format!("let observed = {expression}\nflag = observed"),
                "flag",
            ),
            Self::Assignment | Self::Body | Self::Argument => {
                (format!("flag = {expression}"), "flag")
            }
            Self::If => (
                format!("if {expression} {{ flag = true }} else {{ flag = false }}"),
                "flag",
            ),
            Self::ForallWhere => (
                format!("forall k: {ty} where k == 1 and ({expression}) {{ m[k] = true }}"),
                "m[1]",
            ),
            Self::Index => (format!("m[if {expression} then 1 else 0] = true"), "m[1]"),
            Self::Ensures => (format!("flag = true\nensures {expression}"), "flag"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    Typed,
    Range,
    Collection,
}

impl Domain {
    const ALL: [Self; 3] = [Self::Typed, Self::Range, Self::Collection];

    fn name(self) -> &'static str {
        match self {
            Self::Typed => "typed",
            Self::Range => "range",
            Self::Collection => "collection",
        }
    }

    fn render(self, name: &str, ty: &str, collection: &str, selected: u8) -> String {
        match self {
            Self::Typed => format!("{name}: {ty} where {name} == {selected}"),
            Self::Range => format!("{name} in {selected}..{selected}"),
            Self::Collection => format!("{name} in {collection}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternSite {
    Export,
    RequiresQuantified,
    LetValue,
    Is,
    And,
    Implies,
    If,
    Conditional,
    Ensures,
    ForallWhere,
    QuantifierWhere,
    AggregateWhere,
}

impl PatternSite {
    const ALL: [Self; 12] = [
        Self::Export,
        Self::RequiresQuantified,
        Self::LetValue,
        Self::Is,
        Self::And,
        Self::Implies,
        Self::If,
        Self::Conditional,
        Self::Ensures,
        Self::ForallWhere,
        Self::QuantifierWhere,
        Self::AggregateWhere,
    ];
    const LOCAL: [Self; 4] = [Self::And, Self::Implies, Self::If, Self::Conditional];

    fn logical_continuation(self) -> bool {
        matches!(self, Self::And | Self::Implies)
    }

    fn name(self) -> &'static str {
        match self {
            Self::Export => "requires-export",
            Self::RequiresQuantified => "requires-quantified",
            Self::LetValue => "let-value",
            Self::Is => "assignment-is",
            Self::And => "assignment-and",
            Self::Implies => "assignment-implies",
            Self::If => "if-condition",
            Self::Conditional => "conditional-condition",
            Self::Ensures => "ensures",
            Self::ForallWhere => "forall-where",
            Self::QuantifierWhere => "quantifier-where",
            Self::AggregateWhere => "aggregate-where",
        }
    }

    fn local_name(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Implies => "implies",
            Self::If => "if-condition",
            Self::Conditional => "conditional-condition",
            _ => panic!("not a local continuation facet"),
        }
    }

    fn expression(self, name: &str, continuation: &str) -> String {
        let pattern = format!("opt is some({name})");
        match self {
            Self::And => format!("({pattern}) and ({continuation})"),
            Self::Implies => format!("({pattern}) => ({continuation})"),
            Self::If | Self::Conditional => {
                format!("if {pattern} then ({continuation}) else false")
            }
            _ => panic!("not an expression continuation facet"),
        }
    }

    fn items(self, name: &str, ty: &str) -> (String, &'static str) {
        let pattern = format!("opt is some({name})");
        let code = match self {
            Self::Export => format!("requires {pattern}\nflag = {name}"),
            Self::RequiresQuantified => {
                format!("requires forall q: {ty} {{ {pattern} }}\nflag = true")
            }
            Self::LetValue => format!("let observed = {pattern}\nflag = observed"),
            Self::Is => format!("flag = {pattern}"),
            Self::And | Self::Implies => format!("flag = {}", self.expression(name, name)),
            Self::If => format!("if {pattern} {{ flag = true }} else {{ flag = false }}"),
            Self::Conditional => format!("flag = if {pattern} then true else false"),
            Self::Ensures => format!("flag = true\nensures {pattern}"),
            Self::ForallWhere => {
                format!("forall k: {ty} where k == 1 and ({pattern}) {{ m[k] = true }}")
            }
            Self::QuantifierWhere => format!("flag = exists q: {ty} where {pattern} {{ q == 1 }}"),
            Self::AggregateWhere => format!("flag = count(q: {ty} where {pattern}) == 2"),
        };
        (
            code,
            if self == Self::ForallWhere {
                "m[1]"
            } else {
                "flag"
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    State,
    Const,
    Member,
    ComposeState,
    Header,
    Alias,
    ForeignMember,
}

impl NameKind {
    const GLOBALS: [Self; 3] = [Self::State, Self::Const, Self::Member];
    const ACTUALS: [Self; 3] = [Self::ComposeState, Self::Header, Self::Alias];

    fn name(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Const => "const",
            Self::Member => "enum-member",
            Self::ComposeState => "compose-state",
            Self::Header => "header-param",
            Self::Alias => "alias-root",
            Self::ForeignMember => "other-component-enum-member",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ArgumentForm {
    Pattern(PatternSite),
    Nested(Binding, Domain),
}

impl ArgumentForm {
    fn all() -> Vec<Self> {
        let mut forms: Vec<_> = PatternSite::LOCAL.into_iter().map(Self::Pattern).collect();
        for kind in [Binding::Quantifier, Binding::Aggregate] {
            for domain in Domain::ALL {
                forms.push(Self::Nested(kind, domain));
            }
        }
        forms
    }

    fn name(self) -> String {
        match self {
            Self::Pattern(site) => format!("pattern-{}", site.local_name()),
            Self::Nested(kind, domain) => format!("{}-{}", kind.name(), domain.name()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Recipe {
    Prefix(Binding, NameKind, Slot, bool),
    PrefixNested(Binding, Domain, NameKind, Slot),
    AliasTop(Owner, Binding, Slot, bool),
    AliasNested(Owner, Binding, Domain, Slot),
    AliasPattern(Owner, PatternSite, Slot),
    Merge(Owner, Owner, Binding, Binding, Slot, bool, bool),
    MergePattern(Owner, Owner, Binding, PatternSite, bool, bool),
    MergeArgument(Owner, Binding, NameKind, Slot, bool),
    Substitute(Binding, NameKind, Slot, bool),
    SubstitutePattern(PatternSite, NameKind, Slot),
    SubstituteNested(Binding, Domain, NameKind, Slot),
    ArgumentScope(Owner, Binding, ArgumentForm, Slot, bool),
    Ambient(PatternSite, NameKind, bool),
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub columns: [String; 5],
    recipe: Recipe,
}

impl Cell {
    pub fn id(&self) -> String {
        self.columns.join("|")
    }

    pub fn family(&self) -> &str {
        &self.columns[0]
    }

    /// The same-name Const factor is now refused at its authored AST owner.
    pub fn constant_shadow_location(&self) -> Option<(u64, u64)> {
        match self.recipe {
            Recipe::Prefix(_, NameKind::Const, ..) => Some((7, 3)),
            Recipe::PrefixNested(kind, _, NameKind::Const, slot) => {
                Some(if kind == Binding::Forall || slot == Slot::Body {
                    (8, 1)
                } else {
                    (7, 3)
                })
            }
            _ => None,
        }
    }

    /// Known base-verifier limitation, gated by syntax factors, never cell IDs.
    /// A future implemented BMC result uses the normal violation oracle instead.
    pub fn collection_statement(&self) -> bool {
        matches!(
            self.recipe,
            Recipe::PrefixNested(Binding::Forall, Domain::Collection, ..)
                | Recipe::AliasNested(_, Binding::Forall, Domain::Collection, _)
                | Recipe::SubstituteNested(Binding::Forall, Domain::Collection, ..)
        )
    }
}

fn containers(kind: Binding) -> Vec<Slot> {
    if kind == Binding::Forall {
        vec![Slot::Body]
    } else {
        Slot::CONTAINERS.to_vec()
    }
}

fn layout(late: bool) -> &'static str {
    if late { "late" } else { "early" }
}

fn with_guard(items: &str, binding: &str, late: bool) -> String {
    if !late {
        return format!("{binding}\n{items}");
    }
    // Move the generated statements/ensures before the declaration, NOT a
    // reading guard before its binder. Each generated item is one line.
    let (guards, body): (Vec<_>, Vec<_>) = items
        .lines()
        .partition(|line| line.starts_with("requires ") || line.starts_with("let "));
    format!("{}\n{binding}\n{}", body.join("\n"), guards.join("\n"))
}

/// This is the executable product, not a list of hand-written input fixtures.
// Keep the finite factors together for comparison with the accepted table.
#[allow(clippy::too_many_lines)]
pub fn cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    let mut add = |family: &str,
                   origin: String,
                   position: String,
                   collision: String,
                   shape: String,
                   recipe| {
        cells.push(Cell {
            columns: [family.to_owned(), origin, position, collision, shape],
            recipe,
        });
    };
    for binder in [Binding::Typed, Binding::Range, Binding::Let] {
        for global in NameKind::GLOBALS {
            for slot in Slot::ALL {
                for late in [false, true] {
                    add(
                        "prefix-top",
                        format!("component-{}", binder.name()),
                        format!("component-{}", slot.name()),
                        global.name().into(),
                        layout(late).into(),
                        Recipe::Prefix(binder, global, slot, late),
                    );
                }
            }
        }
    }
    for binder in Binding::NESTED {
        for domain in Domain::ALL {
            for global in NameKind::GLOBALS {
                for slot in containers(binder) {
                    add(
                        "prefix-nested",
                        format!("component-{}", binder.name()),
                        format!("component-{}", slot.name()),
                        global.name().into(),
                        domain.name().into(),
                        Recipe::PrefixNested(binder, domain, global, slot),
                    );
                }
            }
        }
    }
    for (owner, kinds) in [
        (
            Owner::Glue,
            vec![
                Binding::Typed,
                Binding::Range,
                Binding::Let,
                Binding::Pattern,
            ],
        ),
        (Owner::Additional, Binding::TOP.to_vec()),
        (
            Owner::HeaderAdditional,
            vec![Binding::Typed, Binding::Range],
        ),
        (Owner::HeaderComponent, vec![Binding::Typed, Binding::Range]),
    ] {
        for binder in kinds {
            for slot in Slot::ALL {
                for late in [false, true] {
                    add(
                        "alias-top",
                        format!("{}-{}", owner.name(), binder.name()),
                        format!("{}-{}", owner.name(), slot.name()),
                        if binder == Binding::Let {
                            "alias-root"
                        } else {
                            "alias-token"
                        }
                        .into(),
                        layout(late).into(),
                        Recipe::AliasTop(owner, binder, slot, late),
                    );
                }
            }
        }
    }
    for owner in [Owner::Glue, Owner::Additional, Owner::Argument] {
        for binder in Binding::NESTED {
            if owner == Owner::Argument && binder == Binding::Forall {
                continue;
            }
            for domain in Domain::ALL {
                let slots = if owner == Owner::Argument {
                    vec![Slot::Argument]
                } else {
                    containers(binder)
                };
                for slot in slots {
                    add(
                        "alias-nested",
                        format!("{}-{}", owner.name(), binder.name()),
                        format!("{}-{}", owner.name(), slot.name()),
                        "alias-token".into(),
                        domain.name().into(),
                        Recipe::AliasNested(owner, binder, domain, slot),
                    );
                }
            }
        }
    }
    for owner in [Owner::Glue, Owner::Additional, Owner::Argument] {
        for pattern in PatternSite::LOCAL {
            let slots = if owner == Owner::Argument {
                vec![Slot::Argument]
            } else {
                Slot::CONTAINERS.to_vec()
            };
            for slot in slots {
                let token = pattern.logical_continuation();
                add(
                    "alias-local-pattern",
                    format!("{}-pattern-{}", owner.name(), pattern.local_name()),
                    format!("{}-{}", owner.name(), slot.name()),
                    if token { "alias-token" } else { "alias-root" }.into(),
                    "local".into(),
                    Recipe::AliasPattern(owner, pattern, slot),
                );
            }
        }
    }
    for source in Owner::PARTS {
        for target in Owner::PARTS {
            if source == target {
                continue;
            }
            for sb in Binding::TOP {
                for tb in Binding::TOP {
                    for slot in Slot::ALL {
                        for sl in [false, true] {
                            for tl in [false, true] {
                                add(
                                    "merge-guard",
                                    format!("{}-{}", source.name(), sb.name()),
                                    format!("{}-{}", target.name(), slot.name()),
                                    format!("{}-{}", target.name(), tb.name()),
                                    format!("{}-{}", layout(sl), layout(tl)),
                                    Recipe::Merge(source, target, sb, tb, slot, sl, tl),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    for (source, target) in Owner::LINKS {
        for binder in Binding::TOP {
            for pattern in PatternSite::ALL {
                for late in [false, true] {
                    for reverse in [false, true] {
                        let origin = if reverse {
                            format!("{}-local-pattern-{}", target.name(), pattern.name())
                        } else {
                            format!("{}-{}", source.name(), binder.name())
                        };
                        let collision = if reverse {
                            format!("{}-{}", source.name(), binder.name())
                        } else {
                            "pattern-label".into()
                        };
                        add(
                            "merge-local-pattern",
                            origin,
                            format!("{}-{}", target.name(), pattern.name()),
                            collision,
                            format!(
                                "{}-{}",
                                layout(late),
                                if reverse {
                                    "rename-pattern"
                                } else {
                                    "rename-outer"
                                }
                            ),
                            Recipe::MergePattern(source, target, binder, pattern, late, reverse),
                        );
                    }
                }
            }
        }
    }
    for owner in [Owner::B, Owner::Additional] {
        for binder in Binding::TOP {
            for name in NameKind::ACTUALS {
                if owner == Owner::Additional
                    && binder == Binding::Pattern
                    && name != NameKind::Alias
                {
                    continue;
                }
                for slot in Slot::ALL {
                    for late in [false, true] {
                        add(
                            "merge-argument",
                            format!("{}-{}", owner.name(), binder.name()),
                            format!("other-component-{}", slot.name()),
                            name.name().into(),
                            layout(late).into(),
                            Recipe::MergeArgument(owner, binder, name, slot, late),
                        );
                    }
                }
            }
        }
    }
    for binder in Binding::TOP {
        for name in NameKind::ACTUALS {
            for slot in Slot::ALL {
                for late in [false, true] {
                    add(
                        "substitution",
                        format!("component-{}", binder.name()),
                        format!("component-{}", slot.name()),
                        name.name().into(),
                        layout(late).into(),
                        Recipe::Substitute(binder, name, slot, late),
                    );
                }
            }
        }
    }
    for pattern in PatternSite::LOCAL {
        for name in NameKind::ACTUALS {
            for slot in Slot::CONTAINERS {
                add(
                    "substitution",
                    format!("component-local-pattern-{}", pattern.local_name()),
                    format!("component-{}", slot.name()),
                    name.name().into(),
                    "local".into(),
                    Recipe::SubstitutePattern(pattern, name, slot),
                );
            }
        }
    }
    for binder in Binding::NESTED {
        for domain in Domain::ALL {
            for name in NameKind::ACTUALS {
                for slot in containers(binder) {
                    add(
                        "substitution",
                        format!("component-{}", binder.name()),
                        format!("component-{}", slot.name()),
                        name.name().into(),
                        domain.name().into(),
                        Recipe::SubstituteNested(binder, domain, name, slot),
                    );
                }
            }
        }
    }
    for owner in [Owner::B, Owner::Additional] {
        for binding in Binding::TOP {
            for form in ArgumentForm::all() {
                for slot in Slot::CONTAINERS {
                    for reverse in [false, true] {
                        let origin = if reverse {
                            format!("argument-{}", form.name())
                        } else {
                            format!("{}-{}", owner.name(), binding.name())
                        };
                        let collision = if reverse {
                            format!("{}-{}", owner.name(), binding.name())
                        } else {
                            form.name()
                        };
                        add(
                            "argument-scope",
                            origin,
                            format!("c-{}", slot.name()),
                            collision,
                            if reverse {
                                "rename-argument"
                            } else {
                                "rename-outer"
                            }
                            .into(),
                            Recipe::ArgumentScope(owner, binding, form, slot, reverse),
                        );
                    }
                }
            }
        }
    }
    for pattern in PatternSite::ALL {
        for ambient in [
            NameKind::ComposeState,
            NameKind::Header,
            NameKind::ForeignMember,
        ] {
            for sync in [false, true] {
                if ambient == NameKind::Header && !sync {
                    continue;
                }
                add(
                    "ambient-pattern",
                    format!("component-pattern-{}", pattern.name()),
                    format!("component-{}", pattern.name()),
                    ambient.name().into(),
                    if sync { "sync" } else { "use-only" }.into(),
                    Recipe::Ambient(pattern, ambient, sync),
                );
            }
        }
    }
    assert_eq!(cells.len(), 1988, "accepted matrix cardinality");
    assert_eq!(
        cells.iter().map(Cell::id).collect::<BTreeSet<_>>().len(),
        cells.len(),
        "duplicate matrix cells"
    );
    cells
}

#[derive(Clone, Debug, Default)]
struct Part {
    params: String,
    items: String,
    extra: String,
    extra_fields: String,
    extra_init: String,
    payload: bool,
    selected: u8,
}

impl Part {
    fn source(&self, name: &str) -> String {
        format!(
            "spec {name} {{\n  type K = 0..1\n  struct R {{ on: Bool }}\n  {}\n  state {{ flag: Bool, m: Map<K, Bool>, opt: Option<Bool>, r: R, ks: Set<K>{} }}\n  init {{ flag = false forall k: K {{ m[k] = false }} opt = some({}) r = R {{ on: true }} ks = Set {{ {} }} {} }}\n  action go({}) {{\n{}\n  }}\n}}\n",
            self.extra,
            self.extra_fields,
            self.payload,
            self.selected,
            self.extra_init,
            self.params,
            self.items
        )
    }

    fn global(&mut self, kind: NameKind, numeric: bool) {
        let (ty, value) = if numeric {
            ("K", "0")
        } else {
            ("Bool", "false")
        };
        match kind {
            NameKind::State => {
                self.extra_fields = format!(", x: {ty}");
                self.extra_init = format!("x = {value}");
            }
            NameKind::Const => self.extra = format!("const x = {value}"),
            NameKind::Member | NameKind::ForeignMember => self.extra = "enum E { x, Other }".into(),
            _ => panic!("not a component global kind"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceBundle {
    pub files: Vec<(String, String)>,
    /// The sole changed public parameter belongs to this action, if any.
    pub alpha_parameter: Option<(String, String)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionMode {
    UseOnly,
    Sync,
    Glue,
}

struct Program {
    b: Part,
    c: Part,
    additional: Part,
    header: String,
    b_arg: String,
    c_arg: String,
    mode: ActionMode,
    alias_on: bool,
    compose_x: bool,
    property: String,
    alpha_parameter: Option<(String, String)>,
}

impl Program {
    fn new() -> Self {
        let part = Part {
            payload: true,
            selected: 1,
            ..Part::default()
        };
        Self {
            b: part.clone(),
            c: part.clone(),
            additional: part,
            header: String::new(),
            b_arg: String::new(),
            c_arg: String::new(),
            mode: ActionMode::Sync,
            alias_on: true,
            compose_x: false,
            property: "not b.flag".into(),
            alpha_parameter: None,
        }
    }

    fn part(&mut self, owner: Owner) -> &mut Part {
        match owner {
            Owner::B => &mut self.b,
            Owner::C => &mut self.c,
            Owner::Additional | Owner::Glue | Owner::HeaderAdditional => &mut self.additional,
            _ => panic!("not an item owner"),
        }
    }

    fn ty(owner: Owner) -> &'static str {
        if matches!(owner, Owner::B | Owner::C) {
            "K"
        } else {
            "b.K"
        }
    }

    fn collection(owner: Owner) -> &'static str {
        if matches!(owner, Owner::B | Owner::C) {
            "ks"
        } else {
            "b.ks"
        }
    }

    fn observe(&mut self, owner: Owner, observed: &str) {
        self.property = format!(
            "not {}{observed}",
            match owner {
                Owner::B => "b.",
                Owner::C => "c.",
                _ => "",
            }
        );
    }

    fn receiver(&mut self, owner: Owner, slot: Slot, expression: &str) {
        let (code, observed) = slot.render(expression, Self::ty(owner));
        self.part(owner).items = code;
        self.observe(owner, observed);
    }

    fn guard(&mut self, owner: Owner, kind: Binding, name: &str, value: bool, late: bool) {
        let binding = match kind {
            Binding::Let => format!("let {name} = {value}"),
            Binding::Pattern => {
                self.part(owner).payload = value;
                format!("requires opt is some({name})")
            }
            _ => panic!("not a top-level guard binder"),
        };
        let part = self.part(owner);
        part.items = with_guard(&part.items, &binding, late);
    }

    fn actual(&mut self, kind: NameKind) -> (&'static str, &'static str) {
        match kind {
            NameKind::ComposeState => {
                self.compose_x = true;
                ("x", "x")
            }
            NameKind::Header => {
                self.header = "x: Bool".into();
                ("x", "x")
            }
            NameKind::Alias => ("a", "a.on"),
            _ => panic!("not an actual's free-name kind"),
        }
    }

    fn formal_receiver(&mut self, owner: Owner, slot: Slot, argument: &str) {
        self.receiver(owner, slot, "v");
        let part = self.part(owner);
        part.params = "v: Bool".into();
        match owner {
            Owner::B => self.b_arg = argument.into(),
            Owner::C => self.c_arg = argument.into(),
            _ => panic!("formal receiver is a component"),
        }
    }

    fn constrain_actual(&mut self, owner: Owner) {
        let part = self.part(owner);
        part.items = format!("requires v\n{}", part.items);
    }

    fn finish(self) -> SourceBundle {
        let a = format!(
            "spec A {{\n  state {{ on: Bool }}\n  init {{ on = {} }}\n  action stay() {{ }}\n}}\n",
            self.alias_on
        );
        let extra = if self.compose_x { ", x: Bool" } else { "" };
        let init = if self.compose_x { "x = true" } else { "" };
        let action = if self.mode == ActionMode::Glue {
            format!(
                "action g({}) {{\n{}\n  }}",
                self.header, self.additional.items
            )
        } else if self.mode == ActionMode::Sync {
            format!(
                "internal b.go\n  internal c.go\n  action s({}) = b.go({}) || c.go({}) {{\n{}\n  }}",
                self.header, self.b_arg, self.c_arg, self.additional.items
            )
        } else {
            String::new()
        };
        let system = format!(
            "compose S {{\n  use A as a from \"a.fsl\"\n  use B as b from \"b.fsl\"\n  use C as c from \"c.fsl\"\n  state {{ flag: Bool, m: Map<b__K, Bool>, opt: Option<Bool>{extra} }}\n  init {{ flag = false forall k: b.K {{ m[k] = false }} opt = some({}) {init} }}\n  {action}\n  invariant Capture {{ {} }}\n}}\n",
            self.additional.payload, self.property
        );
        SourceBundle {
            files: vec![
                ("a.fsl".into(), a),
                ("b.fsl".into(), self.b.source("B")),
                ("c.fsl".into(), self.c.source("C")),
                ("s.fsl".into(), system),
            ],
            alpha_parameter: self.alpha_parameter,
        }
    }
}

fn alpha(fresh: bool, original: &str) -> &str {
    if fresh { "z" } else { original }
}

fn nested(kind: Binding, binder: &str, expression: &str) -> String {
    match kind {
        Binding::Quantifier => format!("exists {binder} {{ {expression} }}"),
        Binding::Aggregate => format!("sum({binder} of (if {expression} then 1 else 0)) == 1"),
        _ => panic!("not a nested expression binder"),
    }
}

fn nested_receiver(
    program: &mut Program,
    owner: Owner,
    form: (Binding, Domain),
    slot: Slot,
    name: &str,
    expression: &str,
    selected: u8,
) {
    let (kind, domain) = form;
    program.part(owner).selected = selected;
    // Compose-side collection controls use the typed component collection.
    if !matches!(owner, Owner::B | Owner::C) {
        program.b.selected = selected;
    }
    let binder = domain.render(
        name,
        Program::ty(owner),
        Program::collection(owner),
        selected,
    );
    if kind == Binding::Forall {
        if domain == Domain::Collection {
            // Kernel rejects collection-forall writes as non-injective even
            // for Set state. Observe this scope through partiality instead:
            // the correct read is true/safe; capturing a false root divides
            // by zero before the violating flag update. No vacuous loop read.
            program.part(owner).items = format!(
                "forall {binder} {{ if 1 / (if {expression} then 1 else 0) == 1 {{ }} }}\nflag = true"
            );
            program.observe(owner, "flag");
        } else {
            program.part(owner).items = format!("forall {binder} {{ m[{name}] = {expression} }}");
            program.observe(owner, &format!("m[{selected}]"));
        }
    } else {
        program.receiver(owner, slot, &nested(kind, &binder, expression));
    }
}

impl Cell {
    /// Alpha conversion is performed while rendering the owning scope, never
    /// by replacing text in an already merged input.
    // One total arm per finite family keeps source ownership reviewable.
    #[allow(clippy::too_many_lines)]
    pub fn render(&self, fresh: bool) -> SourceBundle {
        let mut p = Program::new();
        match self.recipe {
            Recipe::Prefix(kind, global, slot, late) => {
                p.mode = ActionMode::UseOnly;
                p.b.global(global, false);
                let name = alpha(fresh, "x");
                let expression = if kind == Binding::Range {
                    format!("{name} == 1")
                } else {
                    name.into()
                };
                p.receiver(Owner::B, slot, &expression);
                if kind == Binding::Let {
                    p.guard(Owner::B, kind, name, true, late);
                } else {
                    p.b.params = if kind == Binding::Typed {
                        format!("{name}: Bool")
                    } else {
                        format!("{name} in 1..1")
                    };
                    p.b.items = format!("requires {expression}\n{}", p.b.items);
                    p.alpha_parameter = Some(("b.go".into(), name.into()));
                }
            }
            Recipe::PrefixNested(kind, domain, global, slot) => {
                p.mode = ActionMode::UseOnly;
                p.b.global(global, true);
                let name = alpha(fresh, "x");
                nested_receiver(
                    &mut p,
                    Owner::B,
                    (kind, domain),
                    slot,
                    name,
                    &format!("{name} == 1"),
                    1,
                );
            }
            Recipe::AliasTop(owner, kind, slot, late) => {
                let name = alpha(fresh, "a");
                p.alias_on = false;
                p.mode = if owner == Owner::Glue {
                    ActionMode::Glue
                } else {
                    ActionMode::Sync
                };
                let target = if owner == Owner::HeaderComponent {
                    Owner::B
                } else {
                    Owner::Additional
                };
                let expression = match kind {
                    Binding::Let => format!("{name}.on"),
                    Binding::Range => format!("{name} == 1"),
                    _ => name.into(),
                };
                if owner == Owner::HeaderComponent {
                    p.formal_receiver(target, slot, &expression);
                    p.constrain_actual(target);
                } else {
                    p.receiver(target, slot, &expression);
                }
                match kind {
                    Binding::Typed | Binding::Range => {
                        p.header = if kind == Binding::Typed {
                            format!("{name}: Bool")
                        } else {
                            format!("{name} in 1..1")
                        };
                        if owner != Owner::HeaderComponent {
                            p.additional.items =
                                format!("requires {expression}\n{}", p.additional.items);
                        }
                        p.alpha_parameter = Some((
                            if p.mode == ActionMode::Glue { "g" } else { "s" }.into(),
                            name.into(),
                        ));
                    }
                    Binding::Let => {
                        let items = &mut p.additional.items;
                        *items = with_guard(items, &format!("let {name} = b.r"), late);
                    }
                    Binding::Pattern => p.guard(target, kind, name, true, late),
                    _ => unreachable!(),
                }
            }
            Recipe::AliasNested(owner, kind, domain, slot) => {
                p.mode = if owner == Owner::Glue {
                    ActionMode::Glue
                } else {
                    ActionMode::Sync
                };
                let name = alpha(fresh, "a");
                let expression = format!("{name} == 1");
                if owner == Owner::Argument {
                    let binder = domain.render(name, "b.K", "b.ks", 1);
                    p.formal_receiver(Owner::C, Slot::Body, &nested(kind, &binder, &expression));
                } else {
                    nested_receiver(
                        &mut p,
                        Owner::Additional,
                        (kind, domain),
                        slot,
                        name,
                        &expression,
                        1,
                    );
                }
            }
            Recipe::AliasPattern(owner, site, slot) => {
                p.mode = if owner == Owner::Glue {
                    ActionMode::Glue
                } else {
                    ActionMode::Sync
                };
                let name = alpha(fresh, "a");
                let continuation = if site.logical_continuation() {
                    name
                } else {
                    "a.on"
                };
                let expression = site.expression(name, continuation);
                if owner == Owner::Argument {
                    p.formal_receiver(Owner::C, Slot::Body, &expression);
                } else if site == PatternSite::If && slot == Slot::Body {
                    p.additional.items =
                        format!("if opt is some({name}) {{ flag = a.on }} else {{ flag = false }}");
                    p.observe(Owner::Additional, "flag");
                } else {
                    p.receiver(Owner::Additional, slot, &expression);
                }
            }
            Recipe::Merge(source, target, sb, tb, slot, sl, tl) => {
                let name = alpha(fresh, "x");
                p.part(source).items = format!("flag = {name}");
                p.guard(source, sb, name, false, sl);
                p.receiver(target, slot, "x");
                p.guard(target, tb, "x", true, tl);
            }
            Recipe::MergePattern(source, target, binding, site, late, reverse) => {
                let outer = alpha(fresh && !reverse, "x");
                let pattern = alpha(fresh && reverse, "x");
                p.part(source).items = format!("flag = {outer}");
                p.guard(source, binding, outer, false, late);
                let (items, observed) = site.items(pattern, Program::ty(target));
                p.part(target).items = items;
                p.observe(target, observed);
            }
            Recipe::MergeArgument(owner, binding, name_kind, slot, late) => {
                let (original, argument) = p.actual(name_kind);
                let name = alpha(fresh, original);
                p.part(owner).items = format!("flag = {name}");
                p.guard(owner, binding, name, false, late);
                let receiver = if owner == Owner::B {
                    Owner::C
                } else {
                    Owner::B
                };
                p.formal_receiver(receiver, slot, argument);
                p.constrain_actual(receiver);
            }
            Recipe::Substitute(binding, name_kind, slot, late) => {
                let (original, argument) = p.actual(name_kind);
                let name = alpha(fresh, original);
                p.formal_receiver(Owner::B, slot, argument);
                p.guard(Owner::B, binding, name, false, late);
                // Only the first guard constrains the actual, before the local
                // declaration; statements/ensures still see final guards.
                p.constrain_actual(Owner::B);
            }
            Recipe::SubstitutePattern(site, name_kind, slot) => {
                let (original, argument) = p.actual(name_kind);
                let name = alpha(fresh, original);
                p.b.payload = false;
                let expression = site.expression(name, "v");
                p.receiver(Owner::B, slot, &expression);
                p.b.params = "v: Bool".into();
                p.constrain_actual(Owner::B);
                p.b_arg = argument.into();
            }
            Recipe::SubstituteNested(kind, domain, name_kind, slot) => {
                let (original, argument) = p.actual(name_kind);
                let name = alpha(fresh, original);
                nested_receiver(&mut p, Owner::B, (kind, domain), slot, name, "v", 0);
                p.b.params = "v: Bool".into();
                p.constrain_actual(Owner::B);
                p.b_arg = argument.into();
            }
            Recipe::ArgumentScope(owner, binding, form, slot, reverse) => {
                let outer = alpha(fresh && !reverse, "x");
                let name = alpha(fresh && reverse, "x");
                // Addition and actual share opt. A matching payload must stay
                // true for the actual's and/=> read; freshness (not polarity)
                // is the detector when the competing guard is also a pattern.
                let shared_pattern = owner == Owner::Additional && binding == Binding::Pattern;
                p.part(owner).items = if shared_pattern {
                    format!("flag = not {outer}")
                } else {
                    format!("flag = {outer}")
                };
                p.guard(owner, binding, outer, shared_pattern, false);
                let expression = match form {
                    ArgumentForm::Pattern(site) => {
                        let continuation = if site.logical_continuation() {
                            name
                        } else {
                            "true"
                        };
                        site.expression(name, continuation)
                    }
                    ArgumentForm::Nested(kind, domain) => {
                        let binder = domain.render(name, "b.K", "b.ks", 1);
                        nested(kind, &binder, &format!("{name} == 1"))
                    }
                };
                p.formal_receiver(Owner::C, slot, &expression);
            }
            Recipe::Ambient(site, ambient, sync) => {
                p.mode = if sync {
                    ActionMode::Sync
                } else {
                    ActionMode::UseOnly
                };
                let (items, observed) = site.items(alpha(fresh, "x"), "K");
                p.b.items = items;
                p.observe(Owner::B, observed);
                match ambient {
                    NameKind::ComposeState => p.compose_x = true,
                    NameKind::Header => p.header = "x in 1..1".into(),
                    NameKind::ForeignMember => p.c.global(ambient, false),
                    _ => unreachable!(),
                }
            }
        }
        p.finish()
    }
}

// compose_scratch exposes only scratch/check/run/verify, not resource masking.
// The older1442 test's mask_cost is private to that integration-test crate.
// Keep this reusable comparison here; do not modify either existing file.
fn mask_measurement(value: &mut Value, pointer: &str) -> Result<(), String> {
    let measurement = value
        .pointer_mut(pointer)
        .ok_or_else(|| format!("missing live resource field {pointer}"))?;
    if !(measurement.is_null()
        || measurement
            .as_f64()
            .is_some_and(|n| n.is_finite() && n >= 0.0))
    {
        return Err(format!(
            "invalid resource measurement {pointer}: {measurement}"
        ));
    }
    *measurement = Value::Null;
    Ok(())
}

pub fn normalize(
    value: &mut Value,
    parameter: Option<&(String, String)>,
    cost: bool,
) -> Result<(), String> {
    if cost {
        for field in [
            "elapsed_s",
            "solver/checks",
            "solver/check_elapsed_s",
            "solver/conflicts",
            "solver/decisions",
            "solver/propagations",
            "solver/memory_mb",
        ] {
            mask_measurement(value, &format!("/cost/{field}"))?;
        }
        let count = value
            .pointer("/cost/properties")
            .and_then(Value::as_array)
            .ok_or("missing live cost properties")?
            .len();
        for i in 0..count {
            for field in ["checks", "elapsed_s"] {
                mask_measurement(value, &format!("/cost/properties/{i}/{field}"))?;
            }
        }
    }
    if let Some((action, name)) = parameter {
        let count = normalize_parameter(value, action, name)?;
        if count == 0 {
            return Err(format!("missing live alpha parameter {action}.{name}"));
        }
    }
    Ok(())
}

fn normalize_parameter(value: &mut Value, action: &str, parameter: &str) -> Result<usize, String> {
    let mut count = 0;
    match value {
        Value::Object(fields) => {
            if fields.get("name").and_then(Value::as_str) == Some(action)
                && let Some(params) = fields.get_mut("params")
            {
                let params = params
                    .as_object_mut()
                    .ok_or("action params must be object")?;
                let binding = params
                    .remove(parameter)
                    .ok_or_else(|| format!("missing {parameter} in {action} params"))?;
                if params.insert("alpha_binding".into(), binding).is_some() {
                    return Err("alpha normalization merged parameter keys".into());
                }
                count += 1;
            }
            for child in fields.values_mut() {
                count += normalize_parameter(child, action, parameter)?;
            }
        }
        Value::Array(children) => {
            for child in children {
                count += normalize_parameter(child, action, parameter)?;
            }
        }
        _ => {}
    }
    Ok(count)
}
