// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Additive round4 factors. The legacy generator and its 1988 inputs stay intact.

use super::matrix::{Slot, SourceBundle};

pub const EXPLICIT_TEMPORAL: &str = "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    Requires,
    LetValue,
    Assignment,
    If,
    ForallWhere,
    Index,
    Ensures,
    ForwardRequires,
    ForwardLet,
    UpdateVar,
    UpdateIndex,
    UpdateField,
}

impl Position {
    const READ: [Self; 7] = [
        Self::Requires,
        Self::LetValue,
        Self::Assignment,
        Self::If,
        Self::ForallWhere,
        Self::Index,
        Self::Ensures,
    ];
    const FORWARD: [Self; 2] = [Self::ForwardRequires, Self::ForwardLet];
    const UPDATE: [Self; 3] = [Self::UpdateVar, Self::UpdateIndex, Self::UpdateField];
    fn name(self) -> &'static str {
        match self {
            Self::Requires => "requires",
            Self::LetValue => "let-value",
            Self::Assignment => "assignment",
            Self::If => "if",
            Self::ForallWhere => "forall-where",
            Self::Index => "index",
            Self::Ensures => "ensures",
            Self::ForwardRequires => "forward-requires",
            Self::ForwardLet => "forward-let-value",
            Self::UpdateVar => "update-var",
            Self::UpdateIndex => "update-index",
            Self::UpdateField => "update-field",
        }
    }
    fn slot(self) -> Slot {
        match self {
            Self::Requires => Slot::Requires,
            Self::LetValue => Slot::LetValue,
            Self::Assignment => Slot::Assignment,
            Self::If => Slot::If,
            Self::ForallWhere => Slot::ForallWhere,
            Self::Index => Slot::Index,
            Self::Ensures => Slot::Ensures,
            _ => panic!("not a read slot"),
        }
    }
    fn read(self, expression: &str) -> (String, &'static str) {
        self.slot().render(expression, "K")
    }
    fn rejected(self, name: &str, expression: &str) -> String {
        match self {
            Self::ForwardRequires => {
                format!("requires {expression}\n    let u = true\n    flag = u")
            }
            Self::ForwardLet => "let captured = u\n    let u = true\n    flag = u".into(),
            Self::UpdateVar => format!("{name} = true"),
            Self::UpdateIndex => format!("{name}[0] = true"),
            Self::UpdateField => format!("{name}.on = true"),
            _ => self.read(expression).0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Capturer {
    HeaderTyped,
    HeaderRange,
    StateSync,
    StateUse,
    RefLetBefore,
    RefLetAfter,
    RefPatternBefore,
    RefPatternAfter,
    AdditionalLet,
    AdditionalPattern,
}
impl Capturer {
    const ALL: [Self; 10] = [
        Self::HeaderTyped,
        Self::HeaderRange,
        Self::StateSync,
        Self::StateUse,
        Self::RefLetBefore,
        Self::RefLetAfter,
        Self::RefPatternBefore,
        Self::RefPatternAfter,
        Self::AdditionalLet,
        Self::AdditionalPattern,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::HeaderTyped => "header-typed",
            Self::HeaderRange => "header-range",
            Self::StateSync => "state-sync",
            Self::StateUse => "state-use",
            Self::RefLetBefore => "ref-let-before",
            Self::RefLetAfter => "ref-let-after",
            Self::RefPatternBefore => "ref-pattern-before",
            Self::RefPatternAfter => "ref-pattern-after",
            Self::AdditionalLet => "additional-let",
            Self::AdditionalPattern => "additional-pattern",
        }
    }
    fn state(self) -> bool {
        matches!(self, Self::StateSync | Self::StateUse)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Sync,
    Use,
}
impl Mode {
    const ALL: [Self; 2] = [Self::Sync, Self::Use];
    fn name(self) -> &'static str {
        if self == Self::Sync { "sync" } else { "use" }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Root {
    Invariant,
    Trans,
    Reachable,
    Terminal,
    UntilBefore,
    UntilAfter,
    UnlessBefore,
    UnlessAfter,
    LeadsBefore,
    LeadsAfter,
    Decreases,
    TypedWhere,
    RangeWhere,
    CollectionWhere,
    RangeLo,
    RangeHi,
    CollectionDomain,
    Helpful,
    InitIf,
    InitConditional,
    InitRhs,
    InitWhere,
    InitIndex,
    Initializer,
}
impl Root {
    const ALL: [Self; 24] = [
        Self::Invariant,
        Self::Trans,
        Self::Reachable,
        Self::Terminal,
        Self::UntilBefore,
        Self::UntilAfter,
        Self::UnlessBefore,
        Self::UnlessAfter,
        Self::LeadsBefore,
        Self::LeadsAfter,
        Self::Decreases,
        Self::TypedWhere,
        Self::RangeWhere,
        Self::CollectionWhere,
        Self::RangeLo,
        Self::RangeHi,
        Self::CollectionDomain,
        Self::Helpful,
        Self::InitIf,
        Self::InitConditional,
        Self::InitRhs,
        Self::InitWhere,
        Self::InitIndex,
        Self::Initializer,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Invariant => "invariant",
            Self::Trans => "trans",
            Self::Reachable => "reachable",
            Self::Terminal => "terminal",
            Self::UntilBefore => "until-before",
            Self::UntilAfter => "until-after",
            Self::UnlessBefore => "unless-before",
            Self::UnlessAfter => "unless-after",
            Self::LeadsBefore => "leads-before",
            Self::LeadsAfter => "leads-after",
            Self::Decreases => "leads-decreases",
            Self::TypedWhere => "leads-typed-where",
            Self::RangeWhere => "leads-range-where",
            Self::CollectionWhere => "leads-collection-where",
            Self::RangeLo => "leads-range-lo",
            Self::RangeHi => "leads-range-hi",
            Self::CollectionDomain => "leads-collection-domain",
            Self::Helpful => "leads-helpful-argument",
            Self::InitIf => "init-if",
            Self::InitConditional => "init-conditional",
            Self::InitRhs => "init-rhs",
            Self::InitWhere => "init-forall-where",
            Self::InitIndex => "init-index",
            Self::Initializer => "state-initializer",
        }
    }
    fn temporal(self) -> bool {
        matches!(
            self,
            Self::UntilBefore
                | Self::UntilAfter
                | Self::LeadsBefore
                | Self::LeadsAfter
                | Self::Decreases
                | Self::TypedWhere
                | Self::RangeWhere
                | Self::CollectionWhere
                | Self::RangeLo
                | Self::RangeHi
                | Self::CollectionDomain
                | Self::Helpful
        )
    }
    fn unsupported(self, engine: usize) -> Option<&'static str> {
        if engine == 2 && self.temporal() {
            return Some(EXPLICIT_TEMPORAL);
        }
        match (engine, self) {
            (1, Self::TypedWhere | Self::RangeWhere) => {
                Some("where filters are not implemented in bounded leadsTo")
            }
            (1, Self::CollectionWhere | Self::CollectionDomain) => {
                Some("collection binders are not implemented in the current verifier slice")
            }
            (1, Self::RangeLo | Self::RangeHi) => Some("binder bound is not a static integer"),
            (2, Self::InitIndex) => Some("init does not assign state variable(s): b__aux"),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Argument {
    Label,
    And,
    Implies,
    Conditional,
    Quantified,
}
impl Argument {
    const ALL: [Self; 5] = [
        Self::Label,
        Self::And,
        Self::Implies,
        Self::Conditional,
        Self::Quantified,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Label => "label-only",
            Self::And => "and",
            Self::Implies => "implies",
            Self::Conditional => "conditional-condition",
            Self::Quantified => "quantified-pattern",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Recipe {
    Enum(Capturer, Position),
    EnumRoot(Mode, bool),
    Rejected(Capturer, Position),
    ForeignState(Mode, Position),
    OwnState(Mode, Position),
    Const(bool, Mode, Position),
    ForeignEnum(Mode, Position),
    Ambient(bool, Mode, Root),
    Argument(Argument, Position),
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub columns: Vec<String>,
    recipe: Recipe,
}
impl Cell {
    pub fn id(&self) -> String {
        self.columns.join("|")
    }
    pub fn family(&self) -> &str {
        &self.columns[0]
    }
    pub fn rejection(&self) -> bool {
        matches!(
            self.recipe,
            Recipe::Rejected(..)
                | Recipe::ForeignState(..)
                | Recipe::OwnState(..)
                | Recipe::Const(..)
                | Recipe::ForeignEnum(..)
        )
    }
    pub fn explain(&self) -> bool {
        matches!(self.recipe, Recipe::Argument(..))
    }
    pub fn invariant(&self) -> &'static str {
        if matches!(self.recipe, Recipe::EnumRoot(_, true)) {
            "b.Capture"
        } else {
            "Capture"
        }
    }
    pub fn unsupported(&self, engine: usize) -> Option<&'static str> {
        if let Recipe::Ambient(_, _, root) = self.recipe {
            root.unsupported(engine)
        } else {
            None
        }
    }
    pub fn state_alpha(&self, fresh: bool) -> Option<&'static str> {
        if fresh
            && (matches!(self.recipe,Recipe::Enum(c,_) if c.state())
                || matches!(self.recipe, Recipe::EnumRoot(..)))
        {
            Some("z")
        } else {
            None
        }
    }
    pub fn rejected_name(&self) -> String {
        match self.recipe {
            Recipe::ForeignState(_, p) => format!("c.{}", state_name(p)),
            Recipe::OwnState(_, p) => format!("b.{}", state_name(p)),
            Recipe::Const(own, _, _) => format!("{}.N", if own { "b" } else { "c" }),
            _ => "u".into(),
        }
    }
    pub fn rejected_update(&self) -> bool {
        matches!(
            self.recipe,
            Recipe::Rejected(
                _,
                Position::UpdateVar | Position::UpdateIndex | Position::UpdateField
            ) | Recipe::ForeignState(
                _,
                Position::UpdateVar | Position::UpdateIndex | Position::UpdateField
            ) | Recipe::OwnState(
                _,
                Position::UpdateVar | Position::UpdateIndex | Position::UpdateField
            )
        )
    }
}

// Keep all additive Cartesian factors together for the frozen manifest check.
#[allow(clippy::too_many_lines)]
pub fn cells() -> Vec<Cell> {
    let mut out = Vec::new();
    let mut add = |parts: Vec<String>, recipe| {
        out.push(Cell {
            columns: parts,
            recipe,
        });
    };
    for c in Capturer::ALL {
        for p in Position::READ {
            if c != Capturer::AdditionalPattern {
                add(
                    vec!["bare-enum-read".into(), c.name().into(), p.name().into()],
                    Recipe::Enum(c, p),
                );
            }
        }
    }
    for c in Capturer::ALL {
        for p in Position::READ
            .into_iter()
            .chain(Position::FORWARD)
            .chain(Position::UPDATE)
        {
            if !Position::UPDATE.contains(&p) || c.state() {
                add(
                    vec!["source-rejection".into(), c.name().into(), p.name().into()],
                    Recipe::Rejected(c, p),
                );
            }
        }
    }
    for mode in Mode::ALL {
        for p in Position::READ.into_iter().chain(Position::UPDATE) {
            add(
                vec![
                    "source-rejection-physical".into(),
                    format!("foreign-state-{}", mode.name()),
                    p.name().into(),
                ],
                Recipe::ForeignState(mode, p),
            );
            add(
                vec![
                    "source-rejection-generated-self".into(),
                    mode.name().into(),
                    p.name().into(),
                ],
                Recipe::OwnState(mode, p),
            );
        }
        for own in [true, false] {
            for p in Position::READ {
                add(
                    vec![
                        "source-rejection-generated-const".into(),
                        if own { "self" } else { "foreign" }.into(),
                        mode.name().into(),
                        p.name().into(),
                    ],
                    Recipe::Const(own, mode, p),
                );
            }
        }
        for p in Position::READ.into_iter().chain(Position::FORWARD) {
            add(
                vec![
                    "source-rejection-foreign-enum".into(),
                    mode.name().into(),
                    p.name().into(),
                ],
                Recipe::ForeignEnum(mode, p),
            );
        }
        for invariant in [true, false] {
            add(
                vec![
                    "bare-enum-root".into(),
                    format!("state-{}", mode.name()),
                    if invariant {
                        "invariant"
                    } else {
                        "initializer"
                    }
                    .into(),
                ],
                Recipe::EnumRoot(mode, invariant),
            );
        }
    }
    for foreign in [false, true] {
        for mode in Mode::ALL {
            for root in Root::ALL {
                add(
                    vec![
                        "ambient-root".into(),
                        if foreign {
                            "foreign-enum"
                        } else {
                            "compose-state"
                        }
                        .into(),
                        if mode == Mode::Sync {
                            "sync"
                        } else {
                            "use-only"
                        }
                        .into(),
                        root.name().into(),
                    ],
                    Recipe::Ambient(foreign, mode, root),
                );
            }
        }
    }
    for a in Argument::ALL {
        for p in Position::READ {
            add(
                vec![
                    "argument-no-interference".into(),
                    a.name().into(),
                    p.name().into(),
                ],
                Recipe::Argument(a, p),
            );
        }
    }
    assert_eq!(out.len(), 380);
    let ids: std::collections::BTreeSet<_> = out.iter().map(Cell::id).collect();
    assert_eq!(ids.len(), out.len());
    out
}

fn alpha(fresh: bool, original: &str) -> &str {
    if fresh { "z" } else { original }
}
fn state_name(p: Position) -> &'static str {
    match p {
        Position::UpdateIndex => "m",
        Position::UpdateField => "r",
        _ => "flag",
    }
}

#[derive(Default)]
struct Component {
    items: String,
    params: String,
    extra: String,
    fields: String,
    init: String,
    col: Option<&'static str>,
}
impl Component {
    fn source(&self) -> String {
        let mut lines = vec![
            "spec B {".into(),
            "  type K = 0..1".into(),
            "  struct R { on: Bool }".into(),
            "  enum Color { Red, Blue }".into(),
            format!("  {}", self.extra),
            format!(
                "  state {{ flag: Bool, col: Color, m: Map<K, Bool>, ks: Set<K>{} }}",
                self.fields
            ),
            format!(
                "  init {{ flag = false col = {} forall k: K {{ m[k] = false }} ks = Set {{ 1 }} {} }}",
                self.col.unwrap_or("Blue"),
                self.init
            ),
        ];
        lines.resize(11, String::new());
        lines.extend([
            format!("  action go({}) {{", self.params),
            format!("    {}", self.items),
            "  }".into(),
            "}".into(),
        ]);
        lines.join("\n") + "\n"
    }
}

struct World {
    b: Component,
    c: Option<String>,
    header: String,
    extra: String,
    prelude: Vec<String>,
    refs: Vec<String>,
    internal: Vec<String>,
    mode: Mode,
    observed: String,
    parent_capture: bool,
    parameter: Option<(String, String)>,
}
impl World {
    fn new(items: String) -> Self {
        Self {
            b: Component {
                items,
                ..Component::default()
            },
            c: None,
            header: String::new(),
            extra: String::new(),
            prelude: Vec::new(),
            refs: vec!["b.go()".into()],
            internal: vec!["internal b.go".into()],
            mode: Mode::Sync,
            observed: "flag".into(),
            parent_capture: true,
            parameter: None,
        }
    }
    fn provider(&mut self, c: Capturer, name: &str, numeric: bool, p: Position) {
        match c {
            Capturer::HeaderTyped => {
                self.header = format!("{name}: {}", if numeric { "b.Color" } else { "Bool" });
                self.extra = if numeric {
                    format!("requires {name} == Blue")
                } else {
                    format!("requires {name}")
                };
                self.parameter = Some(("s".into(), name.into()));
            }
            Capturer::HeaderRange => {
                self.header = format!("{name} in 1..1");
                self.extra = format!("requires {name} == 1");
                self.parameter = Some(("s".into(), name.into()));
            }
            Capturer::StateUse | Capturer::StateSync => {
                self.mode = if c == Capturer::StateUse {
                    Mode::Use
                } else {
                    Mode::Sync
                };
                let (ty, init): (String, String) = if numeric {
                    ("b__Color".into(), format!("{name} = Blue"))
                } else {
                    match p {
                        Position::UpdateIndex => (
                            "Map<b__K, Bool>".into(),
                            format!("forall k: b.K {{ {name}[k] = false }}"),
                        ),
                        Position::UpdateField => {
                            ("b__R".into(), format!("{name} = b__R {{ on: false }}"))
                        }
                        _ => ("Bool".into(), format!("{name} = true")),
                    }
                };
                self.prelude.extend([
                    format!("state {{ {name}: {ty} }}"),
                    format!("init {{ {init} }}"),
                ]);
            }
            Capturer::RefLetBefore
            | Capturer::RefLetAfter
            | Capturer::RefPatternBefore
            | Capturer::RefPatternAfter => {
                let pattern = matches!(c, Capturer::RefPatternBefore | Capturer::RefPatternAfter);
                let value = if numeric { "1" } else { "true" };
                let ty = if numeric { "K" } else { "Bool" };
                let binding = if pattern {
                    format!("requires opt is some({name})")
                } else {
                    format!("let {name} = {value}")
                };
                let read = if numeric {
                    format!("{name} == 0")
                } else {
                    format!("not {name}")
                };
                self.c = Some(format!(
                    "spec C {{ type K = 0..1 struct R {{ on: Bool }} state {{ flag: Bool, m: Map<K, Bool>, r: R, opt: Option<{ty}> }} init {{ flag = false forall k: K {{ m[k] = false }} r = R {{ on: false }} opt = some({value}) }} action go() {{ {binding} flag = {read} }} }}\n"
                ));
                if matches!(c, Capturer::RefLetBefore | Capturer::RefPatternBefore) {
                    self.refs.insert(0, "c.go()".into());
                } else {
                    self.refs.push("c.go()".into());
                }
                self.internal.push("internal c.go".into());
            }
            Capturer::AdditionalLet => {
                self.extra = format!("let {name} = {}", if numeric { "Blue" } else { "true" });
            }
            Capturer::AdditionalPattern => {
                self.prelude.extend([
                    "state { opt: Option<Bool> }".into(),
                    "init { opt = some(true) }".into(),
                ]);
                self.extra = format!("requires opt is some({name})");
            }
        }
    }
    fn finish(self) -> SourceBundle {
        let mut lines = vec!["compose S {".into(), "  use B as b from \"b.fsl\"".into()];
        if self.c.is_some() {
            lines.push("  use C as c from \"c.fsl\"".into());
        }
        lines.extend(self.prelude.iter().map(|s| format!("  {s}")));
        lines.resize(11, String::new());
        if self.mode == Mode::Sync {
            lines.push(format!(
                "  action s({}) = {} {{ {} }}",
                self.header,
                self.refs.join(" || "),
                self.extra
            ));
            lines.extend(self.internal.iter().map(|s| format!("  {s}")));
        } else {
            lines.push("  // use-only component".into());
        }
        if self.parent_capture {
            lines.push(format!("  invariant Capture {{ not b.{} }}", self.observed));
        }
        lines.push("}".into());
        let mut files = vec![
            ("b.fsl".into(), self.b.source()),
            ("s.fsl".into(), lines.join("\n") + "\n"),
        ];
        if let Some(c) = self.c {
            files.push(("c.fsl".into(), c));
        }
        SourceBundle {
            files,
            alpha_parameter: self.parameter,
        }
    }
}

fn rename_identifier(source: &str, old: &str, new: &str) -> String {
    // This is a generated source's declared-token alpha operation. Underscores
    // are identifier characters, so an unresolved b__flag is NOT a flag use.
    let mut out = String::new();
    let mut token = String::new();
    for ch in source.chars().chain(std::iter::once('\0')) {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if !token.is_empty() {
                out.push_str(if token == old { new } else { &token });
                token.clear();
            }
            if ch != '\0' {
                out.push(ch);
            }
        }
    }
    out
}

impl Cell {
    // All finite arms share source/scope constructors; no fixture-per-cell list.
    #[allow(clippy::too_many_lines)]
    pub fn render(&self, fresh: bool) -> SourceBundle {
        match self.recipe {
            Recipe::Enum(c, p) => {
                let col = if c.state() { "Blue" } else { "Red" };
                let expr = if c.state() {
                    "col != Red"
                } else {
                    "col == Red"
                };
                let (items, observed) = p.read(expr);
                let mut w = World::new(items);
                w.b.col = Some(col);
                w.observed = observed.into();
                w.provider(c, alpha(fresh, "Red"), true, p);
                w.finish()
            }
            Recipe::EnumRoot(mode, invariant) => {
                let mut w = World::new(
                    if invariant {
                        "flag = true"
                    } else {
                        "flag = col != Blue"
                    }
                    .into(),
                );
                if invariant {
                    w.b.extra = "invariant Capture { (col != Red) == (not flag) }".into();
                    w.parent_capture = false;
                } else {
                    w.b.col = Some("Red");
                }
                w.provider(
                    if mode == Mode::Use {
                        Capturer::StateUse
                    } else {
                        Capturer::StateSync
                    },
                    alpha(fresh, "Red"),
                    true,
                    Position::Assignment,
                );
                w.finish()
            }
            Recipe::Rejected(c, p) => {
                let expr = if c == Capturer::HeaderRange {
                    "u == 1"
                } else {
                    "u"
                };
                let mut w = World::new(p.rejected("u", expr));
                if Position::UPDATE.contains(&p) {
                    w.b.items.push_str("\n    flag = true");
                }
                w.provider(c, alpha(fresh, "u"), false, p);
                w.finish()
            }
            Recipe::ForeignState(mode, p) => {
                let raw = state_name(p);
                let spelling = format!("c__{raw}");
                let mut w = World::new(p.rejected(&spelling, &spelling));
                if Position::UPDATE.contains(&p) {
                    w.b.items.push_str("\n    flag = true");
                }
                w.mode = mode;
                let c = "spec C { type K = 0..1 struct R { on: Bool } state { flag: Bool, m: Map<K, Bool>, r: R } init { flag = true forall k: K { m[k] = false } r = R { on: false } } action go() {} }";
                w.c = Some(if fresh {
                    rename_identifier(c, raw, "z")
                } else {
                    c.into()
                });
                w.finish()
            }
            Recipe::OwnState(mode, p) => {
                let raw = state_name(p);
                let spelling = format!("b__{raw}");
                let mut w = World::new(p.rejected(&spelling, &spelling));
                w.b.fields = ", r: R".into();
                w.b.init = "r = R { on: false }".into();
                w.mode = mode;
                let mut bundle = w.finish();
                if fresh {
                    let b = &mut bundle
                        .files
                        .iter_mut()
                        .find(|(f, _)| f == "b.fsl")
                        .expect("B")
                        .1;
                    *b = rename_identifier(b, raw, "z");
                    if raw == "flag" {
                        let s = &mut bundle
                            .files
                            .iter_mut()
                            .find(|(f, _)| f == "s.fsl")
                            .expect("S")
                            .1;
                        *s = s.replace("b.flag", "b.z");
                    }
                }
                bundle
            }
            Recipe::Const(own, mode, p) => {
                let spelling = if own { "b__N" } else { "c__N" };
                let mut w = World::new(p.read(spelling).0);
                w.mode = mode;
                let n = if fresh { "Z" } else { "N" };
                if own {
                    w.b.extra = format!("const {n} = true");
                } else {
                    w.c = Some(format!(
                        "spec C {{ const {n} = true state {{ spare: Bool }} init {{ spare = false }} action go() {{}} }}"
                    ));
                }
                w.finish()
            }
            Recipe::ForeignEnum(mode, p) => {
                let expr = "u == Blue";
                let mut w = World::new(p.rejected("u", expr));
                w.mode = mode;
                w.c = Some(format!(
                    "spec C {{ enum Ambient {{ {}, Other }} state {{ spare: Bool }} init {{ spare = false }} action go() {{}} }}",
                    alpha(fresh, "u")
                ));
                w.finish()
            }
            Recipe::Ambient(foreign, mode, root) => {
                let mut w = World::new("flag = true".into());
                w.mode = mode;
                let name = alpha(fresh, "x");
                let p = format!("some(true) is some({name})");
                let one = format!("(if {p} then 1 else 1)");
                w.b.extra = match root {
                    Root::Invariant => format!("invariant Match {{ {p} }}"),
                    Root::Trans => format!("trans Match {{ {p} }}"),
                    Root::Reachable => format!("reachable Match {{ {p} }}"),
                    Root::Terminal => format!("terminal {{ flag and ({p}) }}"),
                    Root::UntilBefore => format!("until Match {{ ({p}) until true }}"),
                    Root::UntilAfter => format!("until Match {{ true until ({p}) }}"),
                    Root::UnlessBefore => format!("unless Match {{ ({p}) unless true }}"),
                    Root::UnlessAfter => format!("unless Match {{ true unless ({p}) }}"),
                    Root::LeadsBefore => format!("leadsTo Match {{ ({p}) ~> true }}"),
                    Root::LeadsAfter => format!("leadsTo Match {{ true ~> ({p}) }}"),
                    Root::Decreases => format!("leadsTo Match {{ true ~> true decreases {one} }}"),
                    Root::TypedWhere => {
                        format!("leadsTo Match {{ forall q: K where {p} {{ true ~> true }} }}")
                    }
                    Root::RangeWhere => {
                        format!("leadsTo Match {{ forall q in 0..1 where {p} {{ true ~> true }} }}")
                    }
                    Root::CollectionWhere => {
                        format!("leadsTo Match {{ forall q in ks where {p} {{ true ~> true }} }}")
                    }
                    Root::RangeLo => format!(
                        "leadsTo Match {{ forall q in (if {p} then 0 else 0)..1 {{ true ~> true }} }}"
                    ),
                    Root::RangeHi => {
                        format!("leadsTo Match {{ forall q in 0..{one} {{ true ~> true }} }}")
                    }
                    Root::CollectionDomain => format!(
                        "leadsTo Match {{ forall q in (if {p} then ks else ks) {{ true ~> true }} }}"
                    ),
                    Root::Helpful => format!(
                        "action help(t: Bool) {{ requires t }} leadsTo Match {{ true ~> true decreases 1 helpful help(if {p} then true else false) }}"
                    ),
                    _ => String::new(),
                };
                match root {
                    Root::InitIf => {
                        w.b.fields = ", aux: Bool".into();
                        w.b.init = format!("if {p} {{ aux = true }} else {{ aux = false }}");
                    }
                    Root::InitConditional => {
                        w.b.fields = ", aux: Bool".into();
                        w.b.init = format!("aux = if {p} then true else false");
                    }
                    Root::InitRhs => {
                        w.b.fields = ", aux: Bool".into();
                        w.b.init = format!("aux = {p}");
                    }
                    Root::InitWhere => w.b.init = format!("forall q: K where {p} {{ }}"),
                    Root::InitIndex => {
                        w.b.fields = ", aux: Map<0..0, Bool>".into();
                        w.b.init = format!("aux[if {p} then 0 else 0] = false");
                    }
                    Root::Initializer => w.b.fields = format!(", aux: Bool = {p}"),
                    _ => {}
                }
                if foreign {
                    w.c=Some("spec C { enum Ambient { x, Other } state { spare: Bool } init { spare = false } action go() {} }".into());
                } else {
                    w.prelude
                        .extend(["state { x: Bool }".into(), "init { x = true }".into()]);
                }
                w.finish()
            }
            Recipe::Argument(a, p) => {
                let name = alpha(fresh, "u");
                let pattern = format!("(some(true) is some({name}))");
                let argument = match a {
                    Argument::Label => pattern,
                    Argument::And => format!("{pattern} and {name}"),
                    Argument::Implies => format!("{pattern} => {name}"),
                    Argument::Conditional => format!("if {pattern} then true else false"),
                    Argument::Quantified => format!("forall q: b.K {{ {pattern} }}"),
                };
                let (items, observed) = p.read("v");
                let mut w = World::new(items);
                w.b.params = "v: Bool".into();
                w.refs = vec![format!("b.go({argument})")];
                w.observed = observed.into();
                w.finish()
            }
        }
    }
}
