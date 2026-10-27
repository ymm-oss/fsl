// SPDX-License-Identifier: Apache-2.0

//! Finite caller-scope/definition-global product; no expansion code is used.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Global {
    State,
    Const,
    Enum,
}
impl Global {
    pub fn name(self) -> &'static str {
        match self {
            Self::State => "state-root",
            Self::Const => "const",
            Self::Enum => "enum-value",
        }
    }
    fn variable(self) -> &'static str {
        if self == Self::Enum { "Cold" } else { "on" }
    }
    fn ty(self) -> &'static str {
        if self == Self::Enum { "Tone" } else { "K" }
    }
    fn value(self) -> &'static str {
        if self == Self::Enum { "Hot" } else { "1" }
    }
    fn predicate(self) -> &'static str {
        if self == Self::Enum {
            "(tag == Cold) or z"
        } else {
            "(on == 0) or z"
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Param,
    Let,
    Forall,
    Exists,
    Count,
    Sum,
    Pattern,
    StatementForall,
    Header,
    AdditionalLet,
}
impl Binding {
    pub fn name(self) -> &'static str {
        match self {
            Self::Param => "param",
            Self::Let => "let",
            Self::Forall => "forall",
            Self::Exists => "exists",
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Pattern => "pattern",
            Self::StatementForall => "statement-forall",
            Self::Header => "sync-header-param",
            Self::AdditionalLet => "sync-additional-let",
        }
    }
    fn synchronized(self) -> bool {
        matches!(self, Self::Header | Self::AdditionalLet)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Rhs,
    Requires,
    Invariant,
    LeadsTo,
    Init,
}
impl Position {
    pub fn name(self) -> &'static str {
        match self {
            Self::Rhs => "rhs",
            Self::Requires => "requires",
            Self::Invariant => "invariant",
            Self::LeadsTo => "leadsTo",
            Self::Init => "init",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Component,
    Compose,
}
impl Owner {
    pub fn name(self) -> &'static str {
        if self == Self::Component {
            "component"
        } else {
            "compose"
        }
    }
}

pub struct Case {
    pub global: Global,
    pub binding: Binding,
    pub position: Position,
    pub owner: Owner,
}
pub struct Sources {
    pub direct: String,
    pub component: String,
    pub composed: String,
}

fn binder(global: Global, name: &str, ty: &str) -> String {
    if global == Global::Enum {
        format!("{name}: {ty}")
    } else {
        format!("{name} in 0..1")
    }
}

impl Case {
    pub fn id(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.global.name(),
            self.binding.name(),
            self.position.name(),
            self.owner.name()
        )
    }
    pub fn intrinsic(&self) -> bool {
        self.binding == Binding::Pattern
    }
    pub fn pattern_name(&self) -> &'static str {
        self.global.variable()
    }
    pub fn pattern_rejection(&self) -> &'static str {
        match self.global {
            Global::State => "shadows a state variable",
            Global::Const => "shadows a constant",
            Global::Enum => "shadows an enum member",
        }
    }
    fn expression(&self, name: &str, ty: &str) -> String {
        let domain = binder(self.global, name, ty);
        match self.binding {
            Binding::Forall => format!("forall {domain} {{ p(false) }}"),
            Binding::Exists => format!("exists {domain} {{ p(false) }}"),
            Binding::Count => format!("count({domain} where p(false)) == 2"),
            Binding::Sum => format!("sum({domain} of (if p(false) then 1 else 0)) == 2"),
            Binding::Pattern => format!("(o is some({name})) and p(false)"),
            _ => "p(false)".into(),
        }
    }
    fn source(&self, fresh: bool, composed: bool) -> String {
        let name = if fresh { "x" } else { self.global.variable() };
        let ty = if composed {
            format!("part.{}", self.global.ty())
        } else {
            self.global.ty().into()
        };
        let field_ty = if composed {
            format!("part__{}", self.global.ty())
        } else {
            self.global.ty().into()
        };
        let expression = self.expression(name, &ty);
        let params = if matches!(self.binding, Binding::Param | Binding::Header) {
            format!("{name}: {ty}")
        } else {
            String::new()
        };
        let local = if matches!(self.binding, Binding::Let | Binding::AdditionalLet) {
            format!("let {name} = {}\n", self.global.value())
        } else {
            String::new()
        };
        let mut body = match self.position {
            Position::Rhs => format!("{local}seen = ({expression})"),
            Position::Requires => format!("{local}requires ({expression})\nseen = true"),
            _ => "seen = true".into(),
        };
        let mut map_init = format!(
            "forall k {} {{ m[k] = false }}",
            if self.global == Global::Enum {
                "in Set { Hot, Cold }"
            } else {
                "in 0..1"
            }
        );
        let mut seen_init = "seen = false".to_owned();
        if self.binding == Binding::StatementForall {
            let domain = binder(self.global, name, &ty);
            if self.position == Position::Init {
                map_init = format!("forall {domain} {{ m[{name}] = p(false) }}");
            } else {
                body = format!("forall {domain} {{ m[{name}] = p(false) }}\nseen = true");
            }
        } else if self.binding == Binding::Pattern
            && matches!(self.position, Position::Rhs | Position::Init)
        {
            let branch =
                format!("if o is some({name}) {{ seen = p(false) }} else {{ seen = false }}");
            if self.position == Position::Init {
                seen_init = branch;
            } else {
                body = branch;
            }
        } else if self.position == Position::Init {
            seen_init = format!("seen = ({expression})");
        }
        let (global_declaration, global_field, global_init) = match self.global {
            Global::State => ("", format!("on: {field_ty},"), "on = 0"),
            Global::Const => ("const on = 0", String::new(), ""),
            Global::Enum => ("", format!("tag: {field_ty},"), "tag = Cold"),
        };
        let head = if composed {
            "compose C {\n  use T as part from \"a.fsl\"\n"
        } else {
            "spec S {\n  type K = 0..1\n  enum Tone { Hot, Cold }\n"
        };
        let sync = if composed && self.binding.synchronized() {
            "  internal part.step\n"
        } else {
            ""
        };
        let equals = if composed && self.binding.synchronized() {
            " = part.step()"
        } else {
            ""
        };
        let invariant = if self.position == Position::Invariant {
            format!("not seen and ({expression})")
        } else {
            "not seen".into()
        };
        let temporal = if self.position == Position::LeadsTo {
            format!("  leadsTo L {{ ({expression}) ~> true }}\n")
        } else {
            String::new()
        };
        format!(
            "{head}  {global_declaration}\n  state {{ {global_field} o: Option<{field_ty}>, seen: Bool, m: Map<{field_ty}, Bool> }}\n  init {{ {global_init} o = some({}) {map_init} {seen_init} }}\n  def p(z: Bool) = {}\n{sync}  action g({params}){equals} {{ {body} }}\n  invariant P {{ {invariant} }}\n{temporal}}}\n",
            self.global.value(),
            self.global.predicate()
        )
    }
    pub fn render(&self, fresh: bool) -> Sources {
        let direct = self.source(fresh, false);
        if self.owner == Owner::Component {
            Sources {
                component: direct.clone(),
                direct,
                composed: "compose C {\n  use S as part from \"a.fsl\"\n}\n".into(),
            }
        } else {
            Sources { direct, component: "spec T { type K = 0..1 enum Tone { Hot, Cold } state { idle: Bool } init { idle = false } action step() {} }".into(), composed: self.source(fresh, true) }
        }
    }
    pub fn intrinsic_markers(&self) -> Vec<&'static str> {
        match self.position {
            Position::Rhs => vec!["action g", "if o", "o is some("],
            Position::Requires => vec!["action g", "o is some("],
            Position::Invariant => vec!["invariant P", "o is some("],
            Position::LeadsTo => vec!["leadsTo L", "o is some("],
            Position::Init => vec!["init", "if o", "o is some("],
        }
    }
}

fn inapplicable(
    global: Global,
    binding: Binding,
    position: Position,
    owner: Owner,
) -> Option<&'static str> {
    if binding.synchronized() && owner == Owner::Component {
        return Some(
            "synchronized header/additional bindings belong to the compose source, not its component",
        );
    }
    if matches!(
        binding,
        Binding::Param | Binding::Let | Binding::Header | Binding::AdditionalLet
    ) && !matches!(position, Position::Rhs | Position::Requires)
    {
        return Some("action parameters/lets do not bind property or init expressions");
    }
    if binding == Binding::StatementForall && !matches!(position, Position::Rhs | Position::Init) {
        return Some("statement forall is not an expression and cannot contain requires");
    }
    if owner == Owner::Compose && global == Global::Const {
        return Some(
            "compose grammar has no bare const declaration (parser.rs:410-424); no fake global constructed",
        );
    }
    None
}

pub fn cases() -> (Vec<Case>, Vec<String>) {
    let mut cases = Vec::new();
    let mut excluded = Vec::new();
    for global in [Global::State, Global::Const, Global::Enum] {
        for binding in [
            Binding::Param,
            Binding::Let,
            Binding::Forall,
            Binding::Exists,
            Binding::Count,
            Binding::Sum,
            Binding::Pattern,
            Binding::StatementForall,
            Binding::Header,
            Binding::AdditionalLet,
        ] {
            for position in [
                Position::Rhs,
                Position::Requires,
                Position::Invariant,
                Position::LeadsTo,
                Position::Init,
            ] {
                for owner in [Owner::Component, Owner::Compose] {
                    let case = Case {
                        global,
                        binding,
                        position,
                        owner,
                    };
                    if let Some(reason) = inapplicable(global, binding, position, owner) {
                        excluded.push(format!("{}\t{reason}", case.id()));
                    } else {
                        cases.push(case);
                    }
                }
            }
        }
    }
    assert_eq!((cases.len(), excluded.len()), (163, 137));
    (cases, excluded)
}

pub const ISSUE: &str = "spec D {\n  struct R { on: Bool }\n  state { on: Bool, r: R, seen: Bool }\n  init { on = false r = R { on: true } seen = false }\n  def p(z: Bool) = (not on) or z\n  action g() { let on = r.on  seen = p(false) }\n  invariant P { not seen }\n}\n";
