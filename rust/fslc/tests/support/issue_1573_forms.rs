// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Authored collision-set cases; name visibility is a separate axis.

#[derive(Clone, Copy, Debug)]
pub enum Expected {
    Reject(u32, u32),
    Check,
    Truth(bool),
}

pub struct Case {
    pub name: String,
    pub source: String,
    pub expected: Expected,
    pub params: serde_json::Value,
}

impl Case {
    fn new(name: impl Into<String>, source: String, expected: Expected) -> Self {
        Self {
            name: name.into(),
            source,
            expected,
            params: serde_json::json!({}),
        }
    }
}

fn spec(items: &str) -> String {
    format!(
        "spec A {{\n  type K = 1..2\n  state {{ o: Option<K>, p: Option<K>, flag: Bool }}\n  init {{ o = some(1) p = some(2) flag = true }}\n{items}\n}}\n"
    )
}

fn invariant(expression: &str) -> String {
    spec(&format!(
        "  action stay() {{}}\n  invariant P {{ {expression} }}"
    ))
}

pub fn issue_table() -> Vec<Case> {
    // The issue's eight table rows include a grouped row for three renamed
    // formulas: five underlying formulas, each with its own renamed control.
    let formulas = [
        ("not-false", "(not (o is some(v)))", "or", 1, false),
        (
            "if-false",
            "(if (o is some(v)) then true else true)",
            "and",
            1,
            false,
        ),
        ("not-true", "(not (o is some(v)))", "or", 2, true),
        (
            "not-short-circuit-true",
            "(not (o is none and (o is some(v))))",
            "and",
            2,
            true,
        ),
        (
            "if-true",
            "(if (o is some(v)) then true else true)",
            "and",
            2,
            true,
        ),
    ];
    let mut cases = Vec::new();
    for (tag, first, operator, payload, truth) in formulas {
        for renamed in [false, true] {
            let name = if renamed { "w" } else { "v" };
            let expression =
                format!("{first} {operator} ((p is some({name})) => {name} == {payload})");
            // Preserve the issue's direct spec verbatim, including unbounded Int.
            let source = format!(
                "spec A {{\n  state {{ o: Option<Int>, p: Option<Int>, x: Int }}\n  init {{ o = some(1) p = some(2) x = 0 }}\n  action stay() {{}}\n  invariant P {{ {expression} }}\n}}\n"
            );
            cases.push(Case::new(
                format!("{tag}/renamed={renamed}"),
                source,
                if renamed {
                    Expected::Truth(truth)
                } else {
                    Expected::Reject(5, 3)
                },
            ));
        }
    }
    cases
}

pub fn positions() -> Vec<Case> {
    let wrappers = [
        ("not-and-left", "not ((o is some(v)) and false)"),
        ("not-and-right", "not (true and (o is some(v)))"),
        ("nested-not", "not (not (o is some(v)))"),
        ("if-condition", "(if (o is some(v)) then true else true)"),
        ("if-then", "(if true then (o is some(v)) else true)"),
        ("if-else", "(if false then true else (o is some(v)))"),
    ];
    let mut cases = Vec::new();
    for (wrapper, first) in wrappers {
        for place in ["sibling", "requires", "body", "ensures"] {
            for renamed in [false, true] {
                let name = if renamed { "w" } else { "v" };
                let second = format!("(p is some({name})) and {name} == 2");
                let (source, line, column) = if place == "sibling" {
                    (invariant(&format!("({first}) and ({second})")), 6, 3)
                } else {
                    let later = match place {
                        "requires" => format!("requires {second}"),
                        "body" => format!("flag = {second}"),
                        "ensures" => format!("ensures {second}"),
                        _ => unreachable!(),
                    };
                    (
                        spec(&format!(
                            "  action stay() {{\n    requires {first}\n    {later}\n  }}\n  invariant P {{ flag }}"
                        )),
                        if place == "body" { 7 } else { 5 },
                        if place == "body" { 5 } else { 3 },
                    )
                };
                cases.push(Case::new(
                    format!("{wrapper}/{place}/renamed={renamed}"),
                    source,
                    if renamed {
                        // Some guards deliberately disable stay; this pins check
                        // acceptance, not a fictitious successful Monitor step.
                        Expected::Check
                    } else {
                        Expected::Reject(line, column)
                    },
                ));
            }
        }
    }
    cases
}

fn branch_spec(expression: &str, o: u8, p: u8, condition: bool) -> String {
    format!(
        "spec Branch {{\n  type K = 1..2\n  state {{ o: Option<K>, p: Option<K>, c: Bool }}\n  init {{ o = some({o}) p = some({p}) c = {condition} }}\n  action stay() {{}}\n  invariant Shape {{ o == some({o}) and p == some({p}) and c == {condition} }}\n  invariant P {{ {expression} }}\n}}\n"
    )
}

pub fn branch_arms() -> Vec<Case> {
    let mut cases = Vec::new();
    for false_property in [false, true] {
        for renamed in [false, true] {
            let name = if renamed { "w" } else { "v" };
            let payload = if false_property { 1 } else { 2 };
            let expression = format!(
                "(if c then ((o is some(v)) and v == 1) else ((p is some({name})) and {name} == {payload}))"
            );
            // These four invariant sources are the supplied same/renamed and
            // same_false/renamed_false fixtures, not weakened approximations.
            let original = branch_spec(&expression, 1, 2, false);
            for place in ["invariant", "argument", "def"] {
                let (source, line, column) = match place {
                    "invariant" => (original.clone(), 7, 61),
                    "argument" => (
                        original
                            .replace("c: Bool", "c: Bool, flag: Bool")
                            .replace("c = false }", "c = false flag = true }")
                            .replace(
                                "action stay() {}",
                                &format!(
                                    "action stay(d: Bool) {{ requires d == c flag = {} }}",
                                    expression.replace("if c", "if d")
                                ),
                            )
                            .replace(
                                &format!("invariant P {{ {expression} }}"),
                                "invariant P { flag }",
                            ),
                        5,
                        93,
                    ),
                    "def" => (
                        original.replace(
                            &format!("  invariant P {{ {expression} }}"),
                            &format!(
                                "  def branch(d: Bool) = {}\n  invariant P {{ branch(c) }}",
                                expression.replace("if c", "if d")
                            ),
                        ),
                        7,
                        69,
                    ),
                    _ => unreachable!(),
                };
                let mut case = Case::new(
                    format!("arms/{place}/false={false_property}/renamed={renamed}"),
                    source,
                    if renamed {
                        Expected::Truth(!false_property)
                    } else {
                        Expected::Reject(line, column)
                    },
                );
                if place == "argument" {
                    case.params = serde_json::json!({"d": false});
                }
                cases.push(case);
            }
        }
    }
    cases
}

pub fn branch_variants() -> Vec<Case> {
    let mut cases = Vec::new();
    for condition in [false, true] {
        for (o, p) in [(1, 2), (2, 1)] {
            if !condition && o == 1 {
                // The supplied four fixtures already cover this combination.
                continue;
            }
            for truth in [false, true] {
                for renamed in [false, true] {
                    let name = if renamed { "w" } else { "v" };
                    let a = if truth { o } else { 3 - o };
                    let b = if truth { p } else { 3 - p };
                    let expression = format!(
                        "(if c then ((o is some(v)) and v == {a}) else ((p is some({name})) and {name} == {b}))"
                    );
                    cases.push(Case::new(
                        format!(
                            "arms-variant/c={condition}/o={o}/p={p}/truth={truth}/renamed={renamed}"
                        ),
                        branch_spec(&expression, o, p, condition),
                        if renamed {
                            Expected::Truth(truth)
                        } else {
                            Expected::Reject(7, 61)
                        },
                    ));
                }
            }
        }
    }
    cases
}

pub fn binders() -> Vec<Case> {
    let mut cases = Vec::new();
    for kind in ["forall", "exists", "sum", "count", "unique", "exactlyOne"] {
        for renamed in [false, true] {
            let name = if renamed { "w" } else { "v" };
            let filter = "(o is some(v)) and v == 1";
            let body = format!("(p is some({name})) and {name} == 2");
            let expression = match kind {
                "forall" | "exists" => {
                    format!("{kind} k: K where {filter} {{ {body} }}")
                }
                "sum" => format!("sum(k: K of (if {body} then 1 else 0) where {filter}) == 2"),
                // These aggregates have no body/of. Their two-match where
                // predicate is an existing rejecting collision control.
                "count" => format!("count(k: K where ({filter}) and ({body})) == 2"),
                _ => format!("{kind}(k: K where ({filter}) and ({body}))"),
            };
            cases.push(Case::new(
                format!("{kind}/renamed={renamed}"),
                invariant(&expression),
                if renamed {
                    Expected::Truth(!matches!(kind, "unique" | "exactlyOne"))
                } else {
                    // Sum's of expression contains a conditional: the existing
                    // error is located at its condition, not the declaration.
                    Expected::Reject(6, if kind == "sum" { 33 } else { 3 })
                },
            ));
            if renamed {
                cases.push(Case::new(
                    format!("{kind}/renamed-negative"),
                    invariant(&format!("not ({expression})")),
                    Expected::Truth(matches!(kind, "unique" | "exactlyOne")),
                ));
            }
        }
    }
    cases
}

pub fn isolation() -> Vec<Case> {
    let mut cases = Vec::new();
    for expression in [
        "forall k: K { (some(k) is some(v)) and v == k }",
        "exists k: K { (some(k) is some(v)) and v == k }",
        "sum(k: K of (if (some(k) is some(v)) then k else 0)) == 3",
        "count(k: K where (some(k) is some(v)) and v == k) == 2",
        "unique(k: K where (some(k) is some(v)) and v == 1)",
        "exactlyOne(k: K where (some(k) is some(v)) and v == 1)",
    ] {
        for truth in [false, true] {
            let payload = if truth { 2 } else { 1 };
            cases.push(Case::new(
                format!("isolated/{}/truth={truth}", cases.len()),
                invariant(&format!(
                    "({expression}) and ((p is some(v)) and v == {payload})"
                )),
                Expected::Truth(truth),
            ));
        }
    }
    cases
}

pub fn diagnostics() -> Vec<Case> {
    let mut cases = Vec::new();
    for operator in ["and", "=>"] {
        cases.push(Case::new(
            format!("existing/{operator}"),
            invariant(&format!("(o is some(v)) {operator} (p is some(v))")),
            Expected::Reject(6, 3),
        ));
    }
    for (place, later) in [
        ("requires", "requires p is some(v)"),
        ("body", "flag = p is some(v)"),
        ("ensures", "ensures p is some(v)"),
    ] {
        cases.push(Case::new(
            format!("existing/{place}"),
            spec(&format!(
                "  action stay() {{\n    requires o is some(v)\n    {later}\n  }}\n  invariant P {{ flag }}"
            )),
            if place == "body" {
                Expected::Reject(7, 5)
            } else {
                Expected::Reject(5, 3)
            },
        ));
    }
    cases
}

pub fn statements() -> Vec<Case> {
    let mut cases = Vec::new();
    for route in [
        "condition-then",
        "condition-else",
        "condition-later",
        "then-later",
        "else-later",
    ] {
        for present in [false, true] {
            for renamed in [false, true] {
                let name = if renamed { "w" } else { "v" };
                let second = format!("(p is some({name})) and {name} == 2");
                let first = "(o is some(v)) and v == 1";
                let (condition, then_body, else_body, later, line, column) = match route {
                    "condition-then" => (
                        "o is some(v)",
                        format!("flag = {second}"),
                        "flag = true".into(),
                        String::new(),
                        7,
                        7,
                    ),
                    "condition-else" => (
                        "o is some(v)",
                        "flag = true".into(),
                        format!("flag = {second}"),
                        String::new(),
                        9,
                        7,
                    ),
                    "condition-later" => (
                        "o is some(v)",
                        "flag = true".into(),
                        "flag = true".into(),
                        format!("    seen = {second}\n"),
                        11,
                        5,
                    ),
                    "then-later" => (
                        "true",
                        format!("flag = {first}"),
                        "flag = true".into(),
                        format!("    seen = {second}\n"),
                        11,
                        5,
                    ),
                    "else-later" => (
                        "false",
                        "flag = true".into(),
                        format!("flag = {first}"),
                        format!("    seen = {second}\n"),
                        11,
                        5,
                    ),
                    _ => unreachable!(),
                };
                let option = if present { "some(1)" } else { "none" };
                let source = format!(
                    "spec S {{\n  type K = 1..2\n  state {{ o: Option<K>, p: Option<K>, flag: Bool, seen: Bool }}\n  init {{ o = {option} p = some(2) flag = true seen = true }}\n  action stay() {{\n    if {condition} {{\n      {then_body}\n    }} else {{\n      {else_body}\n    }}\n{later}  }}\n  invariant Shape {{ o == {option} and p == some(2) }}\n  invariant P {{ flag and seen }}\n}}\n"
                );
                cases.push(Case::new(
                    format!("statement/{route}/present={present}/renamed={renamed}"),
                    source,
                    if renamed {
                        Expected::Truth(present || !matches!(route, "then-later" | "else-later"))
                    } else {
                        Expected::Reject(line, column)
                    },
                ));
            }
        }
    }
    cases
}

pub fn independent_statement_arms() -> Vec<Case> {
    let mut cases = Vec::new();
    for condition in [false, true] {
        for truth in [false, true] {
            let then_payload = if truth { 1 } else { 2 };
            let else_payload = if truth { 2 } else { 1 };
            let source = spec(&format!(
                "  action stay() {{\n    if {condition} {{ flag = (o is some(v)) and v == {then_payload} }}\n    else {{ flag = (p is some(v)) and v == {else_payload} }}\n  }}\n  invariant Shape {{ o == some(1) and p == some(2) }}\n  invariant P {{ flag }}"
            ));
            cases.push(Case::new(
                format!("independent-statement-arms/condition={condition}/truth={truth}"),
                source,
                Expected::Truth(truth),
            ));
        }
    }
    cases
}

pub fn nested_statement_arm_exports() -> Vec<Case> {
    let mut cases = Vec::new();
    for outer in [false, true] {
        for inner in [false, true] {
            for truth in [false, true] {
                for renamed in [false, true] {
                    let arms =
                        [(true, true), (true, false), (false, true), (false, false)].map(|route| {
                            if route == (outer, inner) {
                                "(o is some(v)) and v == 1"
                            } else {
                                "true"
                            }
                        });
                    let name = if renamed { "w" } else { "v" };
                    let payload = if truth { 2 } else { 1 };
                    let source = format!(
                        "spec S {{\n  type K = 1..2\n  state {{ o: Option<K>, p: Option<K>, flag: Bool, seen: Bool }}\n  init {{ o = some(1) p = some(2) flag = true seen = true }}\n  action stay() {{\n    if {outer} {{\n      if {inner} {{ flag = {} }}\n      else {{ flag = {} }}\n    }} else {{\n      if {inner} {{ flag = {} }}\n      else {{ flag = {} }}\n    }}\n    seen = (p is some({name})) and {name} == {payload}\n  }}\n  invariant Shape {{ o == some(1) and p == some(2) }}\n  invariant P {{ flag and seen }}\n}}\n",
                        arms[0], arms[1], arms[2], arms[3]
                    );
                    cases.push(Case::new(
                        format!(
                            "nested-arm-export/outer={outer}/inner={inner}/truth={truth}/renamed={renamed}"
                        ),
                        source,
                        if renamed {
                            Expected::Truth(truth)
                        } else {
                            Expected::Reject(13, 5)
                        },
                    ));
                }
            }
        }
    }
    cases
}

pub fn normalized_aggregate_controls() -> Vec<Case> {
    let mut cases = Vec::new();
    for source in ["0..2", "Seq { 0, 1, 2 }"] {
        for kind in ["count", "sum"] {
            let clause = if kind == "count" {
                "where (o is some(v)) and v == 1"
            } else {
                "of (if (o is some(v)) and v == 1 then 1 else 0)"
            };
            cases.push(Case::new(
                format!("normalization/{source}/{kind}"),
                invariant(&format!("{kind}(i in {source} {clause}) == 3")),
                Expected::Check,
            ));
        }
    }
    cases
}

pub fn sync_sources(negative: bool) -> (Case, Case) {
    let first = Case::new(
        "sync-source",
        spec(
            "  action stay() { flag = (o is some(v)) and v == 1 }\n  invariant Shape { o == some(1) }\n  invariant P { flag }",
        ),
        Expected::Check,
    );
    let mut second = Case::new("sync-source/B", first.source.clone(), Expected::Check);
    second.source = second
        .source
        .replace("spec A", "spec B")
        .replace("some(1)", "some(2)")
        .replace("v == 1", if negative { "v < 1" } else { "v == 2" });
    (first, second)
}

pub fn init_positions() -> Vec<Case> {
    let mut cases = Vec::new();
    for route in [
        "condition-then",
        "condition-else",
        "condition-later",
        "then-later",
        "else-later",
        "where-body",
        "review-false-proof",
    ] {
        for renamed in [false, true] {
            let name = if renamed { "w" } else { "v" };
            let later = format!("(some(2) is some({name})) and {name} == 2");
            let (body, line, column) = match route {
                "condition-then" => (
                    format!(
                        "    if (some(1) is some(v)) {{\n      flag = {later}\n    }} else {{\n      flag = true\n    }}\n    seen = true"
                    ),
                    6,
                    7,
                ),
                "condition-else" => (
                    format!(
                        "    if (some(1) is some(v)) and false {{\n      flag = true\n    }} else {{\n      flag = {later}\n    }}\n    seen = true"
                    ),
                    8,
                    7,
                ),
                "condition-later" => (
                    format!(
                        "    if (some(1) is some(v)) {{ flag = true }} else {{ flag = true }}\n    seen = {later}"
                    ),
                    6,
                    5,
                ),
                "then-later" | "else-later" => {
                    let first = "(some(1) is some(v)) and v == 1";
                    let then_body = if route == "then-later" { first } else { "true" };
                    let else_body = if route == "else-later" { first } else { "true" };
                    let condition = route == "then-later";
                    (
                        format!(
                            "    if {condition} {{ flag = {then_body} }} else {{ flag = {else_body} }}\n    seen = {later}"
                        ),
                        6,
                        5,
                    )
                }
                "where-body" => (
                    format!(
                        "    forall k: K where (some(1) is some(v)) and v == 1 {{\n      flag = {later}\n    }}\n    seen = true"
                    ),
                    6,
                    7,
                ),
                "review-false-proof" => (String::new(), 5, 7),
                _ => unreachable!(),
            };
            let source = if route == "review-false-proof" {
                format!(
                    "spec InitReuse {{\n  state {{ flag: Bool }}\n  init {{\n    if (some(1) is some(v)) {{\n      flag = (some(2) is some({name})) and {name} == 1\n    }} else {{\n      flag = false\n    }}\n  }}\n  action stay() {{}}\n  invariant P {{ flag }}\n}}\n"
                )
            } else {
                format!(
                    "spec A {{\n  type K = 0..0\n  state {{ flag: Bool, seen: Bool }}\n  init {{\n{body}\n  }}\n  action stay() {{}}\n  invariant P {{ flag and seen }}\n}}\n"
                )
            };
            cases.push(Case::new(
                format!("init/{route}/renamed={renamed}"),
                source,
                if renamed {
                    Expected::Truth(route != "review-false-proof")
                } else {
                    Expected::Reject(line, column)
                },
            ));
        }
    }
    cases
}

pub fn init_source_pair(renamed: bool) -> (Case, String) {
    let first = Case::new(
        format!("init-source-pair/renamed={renamed}"),
        "spec A {\n  state { flag: Bool }\n  init { flag = if (some(1) is some(v)) then true else false }\n  action stay() {}\n  invariant P { flag }\n}\n".into(),
        if renamed { Expected::Truth(true) } else { Expected::Check },
    );
    let name = if renamed { "w" } else { "v" };
    (
        first,
        format!(
            "spec B {{\n  state {{ flag: Bool }}\n  init {{ flag = (some(2) is some({name})) and {name} == 2 }}\n  action stay() {{}}\n  invariant P {{ flag }}\n}}\n"
        ),
    )
}

pub fn direct_init_fragments() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, source, line, column) in [
        (
            "blocks",
            "spec A {\n  state { first: Bool, flag: Bool }\n  init {\n    if (some(1) is some(v)) { first = true }\n    else { first = false }\n  }\n  init {\n    flag = (some(2) is some(v)) and v == 1\n  }\n  action stay() {}\n  invariant P { flag }\n}\n",
            8,
            5,
        ),
        (
            "inline-then-block",
            "spec A {\n  state {\n    first: Bool = if (some(1) is some(v)) then true else false,\n    flag: Bool\n  }\n  init { flag = (some(2) is some(v)) and v == 1 }\n  action stay() {}\n  invariant P { flag }\n}\n",
            6,
            10,
        ),
    ] {
        for renamed in [false, true] {
            let later = "(some(2) is some(v)) and v == 1";
            assert_eq!(source.matches(later).count(), 1);
            cases.push(Case::new(
                format!("direct-init-fragments/{name}/renamed={renamed}"),
                if renamed {
                    source.replacen(later, "(some(2) is some(w)) and w == 1", 1)
                } else {
                    source.into()
                },
                if renamed {
                    Expected::Truth(false)
                } else {
                    Expected::Reject(line, column)
                },
            ));
        }
    }
    cases
}

pub fn refinement_priority() -> (Case, &'static str, &'static str) {
    (
        Case::new(
            "refinement-priority",
            "spec Impl {type V=0..5 state {p:Option<V> = some(1),q:Option<V> = some(3)} fair action go(){p=some(1) q=some(3)}}\n".into(),
            Expected::Check,
        ),
        "spec Abs {state {b:Bool=false} fair action step(){b=false} leadsTo L {forall z in 0..3 {true ~> (b or z!=3)}}}\n",
        "refinement R {impl Impl abs Abs map b=((not (p is some(z))) or ((q is some(z)) and z==3)) action go()->step() preserve progress {respond L by go}}\n",
    )
}
