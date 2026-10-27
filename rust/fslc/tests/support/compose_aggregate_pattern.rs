// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Check-only aggregate/filter scope product; no compose lowering is used to
//! decide the original source's admissibility.

use super::{extra, matrix};

pub const INAPPLICABLE_COUNT: usize = 24;
pub const INAPPLICABLE_REASON: &str = "inline initializer for ... does not allow a binder or aggregate expression; standalone rejection here does not test aggregate/filter scope";

#[derive(Clone, Copy, Debug)]
pub enum Owner {
    Component(&'static str),
    Arguments,
    Additional,
    Glue,
}

impl Owner {
    pub fn name(self) -> &'static str {
        match self {
            Self::Component(file) => file,
            Self::Arguments => "composition/header-arguments",
            Self::Additional => "composition/additional-items",
            Self::Glue => "composition/glue-items",
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Count,
    Sum,
    Forall,
    Unique,
}
impl Kind {
    const ALL: [Self; 4] = [Self::Count, Self::Sum, Self::Forall, Self::Unique];
    fn name(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Forall => "forall",
            Self::Unique => "unique",
        }
    }
    fn accepts(self) -> bool {
        matches!(self, Self::Count | Self::Sum)
    }
    fn expression(self, empty: bool, present: bool, negative: bool) -> String {
        let range = if empty { "1..0" } else { "0..1" };
        let expected = i32::from(!empty && present) * 2 + i32::from(negative);
        match self {
            Self::Count => format!("count(x in {range} where o is some(x)) == {expected}"),
            Self::Sum => format!("sum(x in {range} of 1 where o is some(x)) == {expected}"),
            Self::Forall => format!("forall x in {range} {{ o is some(x) }}"),
            Self::Unique => format!("unique(x in {range} where o is some(x))"),
        }
    }
}

#[derive(Clone)]
enum Place {
    Action(matrix::Slot, bool),
    Arguments,
    Additional(matrix::Slot),
    Glue(matrix::Slot),
    Root(extra::Cell),
}

const SLOTS: [(matrix::Slot, &str); 7] = [
    (matrix::Slot::Requires, "requires"),
    (matrix::Slot::LetValue, "let-value"),
    (matrix::Slot::Assignment, "assignment"),
    (matrix::Slot::If, "if"),
    (matrix::Slot::ForallWhere, "forall-where"),
    (matrix::Slot::Index, "index"),
    (matrix::Slot::Ensures, "ensures"),
];

pub struct Cell {
    pub id: String,
    kind: Kind,
    empty: bool,
    present: bool,
    negative: bool,
    place: Place,
}
impl Cell {
    pub fn accepts(&self) -> bool {
        self.kind.accepts()
    }
    pub fn owner(&self) -> Owner {
        match self.place {
            Place::Action(..) | Place::Root(_) => Owner::Component("b.fsl"),
            Place::Arguments => Owner::Arguments,
            Place::Additional(_) => Owner::Additional,
            Place::Glue(_) => Owner::Glue,
        }
    }
    pub fn render(&self) -> matrix::SourceBundle {
        let expression = self
            .kind
            .expression(self.empty, self.present, self.negative);
        let value = if self.present { "some(1)" } else { "none" };
        if let Place::Root(root) = &self.place {
            let mut bundle = root.render(false);
            let b = &mut bundle
                .files
                .iter_mut()
                .find(|(f, _)| f == "b.fsl")
                .expect("component")
                .1;
            let marker = "some(true) is some(x)";
            assert_eq!(b.matches(marker).count(), 1, "one aggregate root");
            *b = b
                .replace(marker, &format!("({expression})"))
                .replace("state { flag: Bool,", "state { o: Option<K>, flag: Bool,")
                .replace(
                    "init { flag = false",
                    &format!("init {{ o = {value} flag = false"),
                );
            let s = &mut bundle
                .files
                .iter_mut()
                .find(|(f, _)| f == "s.fsl")
                .expect("composition")
                .1;
            *s = s
                .replace("state { x: Bool }", "")
                .replace("init { x = true }", "");
            return bundle;
        }
        let mut params = String::new();
        let mut body = String::new();
        let mut extra = String::new();
        let mut observed = "flag";
        let mut argument = String::new();
        let mut mode = "sync";
        match self.place {
            Place::Action(slot, sync) => {
                (body, observed) = slot.render(&expression, "K");
                if !sync {
                    mode = "use";
                }
            }
            Place::Arguments => {
                params = "v: Bool".into();
                body = "requires v flag = true".into();
                argument = expression;
            }
            Place::Additional(slot) => {
                (extra, observed) = slot.render(&expression, "b.K");
            }
            Place::Glue(slot) => {
                (extra, observed) = slot.render(&expression, "b.K");
                mode = "glue";
            }
            Place::Root(_) => unreachable!(),
        }
        let b = format!(
            "spec B {{\n  type K = 0..1\n  state {{ o: Option<K>, flag: Bool, m: Map<K, Bool> }}\n  init {{ o = {value} flag = false forall k: K {{ m[k] = false }} }}\n  action go({params}) {{ {body} }}\n}}\n"
        );
        let component_owned = matches!(self.place, Place::Action(..));
        let prelude = if component_owned {
            String::new()
        } else {
            format!(
                "  state {{ o: Option<b__K>, flag: Bool, m: Map<b__K, Bool> }}\n  init {{ o = {value} flag = false forall k: b.K {{ m[k] = false }} }}\n"
            )
        };
        let action = match mode {
            "sync" => {
                format!("  internal b.go\n  action joined() = b.go({argument}) {{ {extra} }}\n")
            }
            "glue" => format!("  action glue() {{ {extra} }}\n"),
            "use" => String::new(),
            _ => unreachable!(),
        };
        let qualifier = if component_owned { "b." } else { "" };
        let s = format!(
            "compose S {{\n  use B as b from \"b.fsl\"\n{prelude}{action}  invariant Capture {{ not {qualifier}{observed} }}\n}}\n"
        );
        matrix::SourceBundle {
            files: vec![("b.fsl".into(), b), ("s.fsl".into(), s)],
            alpha_parameter: None,
        }
    }
}

pub fn cells() -> Vec<Cell> {
    product(false)
}

/// Include even intrinsic-refusal and metadata-only roots for namespace checks.
pub fn namespace_templates() -> Vec<Cell> {
    product(true).into_iter().filter(|c| !c.negative).collect()
}

fn product(all_roots: bool) -> Vec<Cell> {
    let mut places = Vec::new();
    for (slot, name) in SLOTS {
        for sync in [false, true] {
            places.push((
                format!(
                    "component-action/{}/{name}",
                    if sync { "sync" } else { "use" }
                ),
                Place::Action(slot, sync),
            ));
        }
        places.push((format!("additional/{name}"), Place::Additional(slot)));
        places.push((format!("glue/{name}"), Place::Glue(slot)));
    }
    places.push(("header-argument".into(), Place::Arguments));
    for root in extra::cells().into_iter().filter(|c| {
        c.family() == "ambient-root"
            && c.columns[1] == "compose-state"
            && c.columns[2] == "use-only"
            && (all_roots || c.columns[3] != "leads-helpful-argument")
            // All24 combinations, including8 refusal controls, hit the
            // intrinsic initializer restriction rather than filter scope.
            && (all_roots || c.columns[3] != "state-initializer")
    }) {
        places.push((
            format!("component-root/{}", root.columns[3]),
            Place::Root(root),
        ));
    }
    let mut cells = Vec::new();
    for kind in Kind::ALL {
        for empty in [false, true] {
            for present in [false, true] {
                for negative in [false, true] {
                    if negative && !kind.accepts() {
                        continue;
                    }
                    for (name, place) in &places {
                        cells.push(Cell {
                            id: format!(
                                "aggregate-pattern/{}/{}/{}/{}/{name}",
                                kind.name(),
                                if empty { "empty" } else { "full" },
                                if present { "some" } else { "none" },
                                if negative { "negative" } else { "positive" }
                            ),
                            kind,
                            empty,
                            present,
                            negative,
                            place: place.clone(),
                        });
                    }
                }
            }
        }
    }
    assert_eq!(places.len(), if all_roots { 53 } else { 51 });
    assert_eq!(cells.len(), if all_roots { 1272 } else { 1224 });
    cells
}
