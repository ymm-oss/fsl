// SPDX-License-Identifier: Apache-2.0

//! Inputs and literal classes independently pinned after explicit calibration.

#[derive(Clone, Copy)]
pub enum Expected {
    Before(&'static str),
    FreeRead { root: &'static str, violated: bool },
    Accepted,
    Free { root: &'static str, partial: bool },
    StaticRange,
    ExistingKeyRefusal,
}
impl Expected {
    pub fn name(self) -> &'static str {
        match self {
            Self::Before(_) => "read-before-assigned",
            Self::FreeRead { .. } => "free-root-read",
            Self::Accepted => "explicit-accepted",
            Self::Free { .. } => "symbolic-free-init",
            Self::StaticRange => "existing-static-range-refusal",
            Self::ExistingKeyRefusal => "existing-dynamic-key-refusal",
        }
    }
}

pub struct Case {
    pub id: String,
    pub family: &'static str,
    pub expected: Expected,
    pub source: String,
    pub marker: Option<String>,
}

fn spec(fields: &str, init: &str, invariant: &str) -> String {
    format!(
        "spec A {{\n  type K = 0..1\n  struct R {{ on: Bool, other: Bool }}\n  state {{ {fields} }}\n  init {{\n    {init}\n  }}\n  action stay() {{}}\n  invariant P {{ {invariant} }}\n}}\n"
    )
}

fn case(
    id: &str,
    family: &'static str,
    expected: Expected,
    fields: &str,
    init: &str,
    invariant: &str,
    marker: Option<&str>,
) -> Case {
    Case {
        id: id.into(),
        family,
        expected,
        source: spec(fields, init, invariant),
        marker: marker.map(str::to_owned),
    }
}

fn ordered(after: bool, read: &str, write: &str) -> String {
    if after {
        format!("{write}\n    {read}")
    } else {
        format!("{read}\n    {write}")
    }
}

pub const ISSUE: &str = "spec A {\n  state { on: Bool, g: Bool }\n  init { g = on }\n  action stay() { }\n  invariant Same { g == on }\n}\n";
pub const CHOOSE: &str = "spec A {\n  state { choose: Bool, x: Int }\n  init { if choose { x = 1 } else { x = 0 } }\n  action stay() { }\n  invariant X { x == 1 }\n}\n";

#[allow(clippy::too_many_lines)] // Keep the finite calibrated input table together.
pub fn cases() -> Vec<Case> {
    use Expected::{Accepted, Before, ExistingKeyRefusal, Free, FreeRead, StaticRange};
    // #493: roots never assigned anywhere in init are free values, unlike
    // roots read before a later assignment. Explicit still cannot run these.
    let mut rows = vec![
        Case {
            id: "issue-exact".into(),
            family: "root",
            expected: FreeRead {
                root: "on",
                violated: false,
            },
            source: ISSUE.into(),
            marker: Some("g = on".into()),
        },
        Case {
            id: "free-choose".into(),
            family: "if-condition",
            expected: FreeRead {
                root: "choose",
                violated: true,
            },
            source: CHOOSE.into(),
            marker: Some("if choose".into()),
        },
    ];
    for after in [false, true] {
        let suffix = if after { "after" } else { "before" };
        let on = if after { Accepted } else { Before("on") };
        rows.push(case(
            &format!("root-value-{suffix}"),
            "root",
            on,
            "on: Bool, g: Bool",
            &ordered(after, "g = on", "on = false"),
            "g == on",
            Some("g = on"),
        ));
        rows.push(case(
            &format!("root-unary-{suffix}"),
            "rhs-unary",
            on,
            "on: Bool, g: Bool",
            &ordered(after, "g = not on", "on = false"),
            "g == not on",
            Some("g = not on"),
        ));
        rows.push(case(
            &format!("if-condition-{suffix}"),
            "if-condition",
            on,
            "on: Bool, g: Bool",
            &ordered(after, "if on { g = true } else { g = false }", "on = false"),
            "g == on",
            Some("if on"),
        ));
        rows.push(case(
            &format!("forall-body-{suffix}"),
            "forall-body",
            on,
            "on: Bool, m: Map<K, Bool>",
            &ordered(after, "forall k in 0..1 { m[k] = on }", "on = false"),
            "not on and not m[0] and not m[1]",
            Some("m[k] = on"),
        ));
        rows.push(case(
            &format!("forall-bound-{suffix}"),
            "forall-range",
            StaticRange,
            "n: K, m: Map<K, Bool>",
            &ordered(after, "forall k in 0..n { m[k] = false }", "n = 1"),
            "n == 1 and not m[0] and not m[1]",
            Some("forall k in 0..n"),
        ));
        rows.push(case(
            &format!("map-index-read-{suffix}"),
            "map-key-rhs",
            if after { Accepted } else { Before("k") },
            "k: K, m: Map<K, Bool>, g: Bool",
            &format!(
                "m[0] = false m[1] = true\n    {}",
                ordered(after, "g = m[k]", "k = 0")
            ),
            "g == m[k]",
            Some("g = m[k]"),
        ));
        rows.push(case(
            &format!("map-key-write-{suffix}"),
            "map-key-lhs",
            ExistingKeyRefusal,
            "k: K, m: Map<K, Bool>, g: Bool",
            &format!(
                "{}\n    m[1] = true g = false",
                ordered(after, "m[k] = false", "k = 0")
            ),
            "not g",
            Some("m[k] = false"),
        ));
        rows.push(case(
            &format!("struct-root-{suffix}"),
            "struct-root",
            if after { Accepted } else { Before("r") },
            "r: R, g: Bool",
            &ordered(after, "g = r.on", "r = R { on: false, other: true }"),
            "not r.on and r.other and not g",
            Some("g = r.on"),
        ));
        rows.push(case(
            &format!("conditional-value-{suffix}"),
            "rhs-conditional",
            on,
            "flag: Bool, on: Bool, g: Bool",
            &format!(
                "flag = true\n    {}",
                ordered(after, "g = if flag then on else false", "on = false")
            ),
            "flag and not on and not g",
            Some("g = if flag"),
        ));
        rows.push(case(
            &format!("dead-rhs-{suffix}"),
            "rhs-short-circuit",
            on,
            "on: Bool, g: Bool",
            &ordered(after, "g = false and on", "on = false"),
            "not g and not on",
            Some("g = false and on"),
        ));
    }
    for key in [0, 1] {
        for full in [false, true] {
            let suffix = if full { "full" } else { "partial" };
            let init = if full {
                format!("m[0] = false\n    m[1] = true\n    g = m[{key}]")
            } else {
                format!("m[0] = false\n    g = m[{key}]\n    m[1] = true")
            };
            rows.push(case(
                &format!("map-key{key}-{suffix}"),
                if key == 0 {
                    "map-assigned-key"
                } else {
                    "map-unassigned-key"
                },
                if full { Accepted } else { Before("m") },
                "m: Map<K, Bool>, g: Bool",
                &init,
                &format!("g == m[{key}]"),
                Some(&format!("g = m[{key}]")),
            ));
        }
    }
    for field in ["on", "other"] {
        for full in [false, true] {
            let suffix = if full { "full" } else { "partial" };
            let init = if full {
                format!("r = R {{ on: false, other: true }}\n    g = r.{field}")
            } else {
                format!("r.on = false\n    g = r.{field}")
            };
            rows.push(case(
                &format!("struct-{field}-{suffix}"),
                if field == "on" {
                    "struct-assigned-field"
                } else {
                    "struct-unassigned-field"
                },
                if full { Accepted } else { Before("r") },
                "r: R, g: Bool",
                &init,
                &format!("g == r.{field}"),
                Some(&format!("g = r.{field}")),
            ));
        }
    }
    rows.push(case(
        "if-one-branch-read",
        "if-partial-coverage",
        Before("on"),
        "flag: Bool, on: Bool, g: Bool",
        "flag = false\n    if flag { on = true }\n    g = on",
        "g == on",
        Some("g = on"),
    ));
    rows.push(case(
        "if-both-branches-read",
        "if-complete-coverage",
        Accepted,
        "flag: Bool, on: Bool, g: Bool",
        "flag = false\n    if flag { on = true } else { on = false }\n    g = on",
        "g == on",
        Some("g = on"),
    ));
    rows.push(case(
        "bound-key-is-not-state-read",
        "forall-bound-name",
        Accepted,
        "k: K, m: Map<K, Bool>",
        "forall k in 0..1 { m[k] = false }\n    k = 0",
        "k == 0 and not m[0] and not m[1]",
        Some("m[k] = false"),
    ));
    rows.push(case(
        "omitted-root-no-read",
        "nondeterministic-root",
        Free {
            root: "on",
            partial: false,
        },
        "on: Bool, g: Bool",
        "g = false",
        "not g",
        None,
    ));
    rows.push(case(
        "partial-map-no-read",
        "nondeterministic-map",
        Free {
            root: "m",
            partial: true,
        },
        "m: Map<K, Bool>",
        "m[0] = false",
        "not m[0]",
        None,
    ));
    rows.push(case(
        "partial-struct-no-read",
        "nondeterministic-struct",
        Free {
            root: "r",
            partial: true,
        },
        "r: R",
        "r.on = false",
        "not r.on",
        None,
    ));
    assert_eq!(rows.len(), 36);
    rows
}
