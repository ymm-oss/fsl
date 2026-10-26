// SPDX-License-Identifier: Apache-2.0

// One source inventory for CLI, editor and Worker acceptance. Baseline means
// 666acee8ad5f40a1ca5e205419091245f5a5d6de, not an older #1457 binary.

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Safe,
    Violated,
    Error,
}

#[derive(Debug)]
pub struct Case {
    pub id: String,
    pub source: String,
    pub baseline: [Outcome; 4],
    pub expected: [Outcome; 4],
    // Authored construct, not the first quoted state name in a message.
    pub rejection: Option<&'static str>,
    pub auto: Option<Outcome>,
    pub selectors: bool,
    pub files: Vec<(String, String)>,
}

pub const ROUTES: [&str; 4] = ["check", "bmc", "induction", "explicit"];

fn source(state: &str, init: &str, action: &str, property: &str) -> String {
    format!(
        "spec Support {{\n  type K = 0..1\n  state {{ {state} }}\n  init {{\n    {init}\n  }}\n  action go() {{ {action} }}\n  {property}\n}}\n"
    )
}

fn case(
    id: String,
    source: String,
    baseline: [Outcome; 4],
    rejection: Option<&'static str>,
) -> Case {
    Case {
        id,
        source,
        baseline,
        expected: if rejection.is_some() {
            [Outcome::Error; 4]
        } else {
            baseline
        },
        rejection,
        auto: None,
        selectors: false,
        files: Vec::new(),
    }
}

fn pair(cases: &mut Vec<Case>, id: &str, state: &str, init: &str) {
    for negative in [false, true] {
        let verdict = if negative {
            Outcome::Violated
        } else {
            Outcome::Safe
        };
        cases.push(case(
            format!("{id}/{negative}"),
            source(
                state,
                init,
                if negative {
                    "done = true"
                } else {
                    "done = false"
                },
                "invariant Capture { not done }",
            ),
            [Outcome::Ok, verdict, verdict, verdict],
            None,
        ));
    }
}

// This is intentionally a single inventory: adding a source automatically adds
// its check/engine cells and, for rejections, its Worker/editor cells.
#[allow(clippy::too_many_lines)]
pub fn cases() -> Vec<Case> {
    use Outcome::{Error, Ok, Safe, Violated};
    let mut cases = Vec::new();
    for kind in ["Set", "Seq"] {
        let ty = if kind == "Set" { "Set<K>" } else { "Seq<K, 2>" };
        for origin in ["state", "literal"] {
            for filtered in [false, true] {
                let domain = if origin == "state" {
                    "xs".into()
                } else {
                    format!("{kind} {{ 1 }}")
                };
                let filter = if filtered { " where z == 1" } else { "" };
                cases.push(case(
                    format!("L1/{kind}/{origin}/{filtered}"),
                    source(&format!("xs: {ty}, done: Bool"), &format!("xs = {kind} {{ 1 }} done = false"), "done = true", &format!("leadsTo Capture {{ forall z in {domain}{filter} {{ z == 1 ~> within 1 true }} }}")),
                    [Ok, Error, Error, Error], Some("leadsTo Capture"),
                ));
            }
        }
        for empty in [false, true] {
            for wrapper in ["plain", "conditional", "unreachable", "nested"] {
                let domain = match wrapper {
                    "conditional" => format!("if done then {kind} {{ 1 }} else xs"),
                    _ => "xs".into(),
                };
                let loop_text = format!("forall z in {domain} {{ }}");
                let loop_text = match wrapper {
                    "unreachable" => format!("if false {{\n      {loop_text}\n    }}"),
                    "nested" => format!("forall a: K {{\n      {loop_text}\n    }}"),
                    _ => loop_text,
                };
                let mut c = case(
                    format!("I1/{kind}/{empty}/{wrapper}"),
                    source(
                        &format!("xs: {ty}, done: Bool"),
                        &format!(
                            "xs = {kind} {{ {} }} done = false\n    {loop_text}",
                            if empty { "" } else { "1" }
                        ),
                        "done = true",
                        "invariant Capture { not done }",
                    ),
                    [Ok, Error, Error, Error],
                    Some("forall z in"),
                );
                c.auto = Some(Error);
                cases.push(c);
            }
        }
        pair(
            &mut cases,
            &format!("I2/{kind}/assigned-filter-body"),
            "n: K, done: Bool",
            &format!(
                "n = 1 done = false\n    forall z in {kind} {{ 1 }} where n == 1 {{ if n == 1 {{ }} }}"
            ),
        );
    }
    for binder in ["z: K", "z in 0..1"] {
        for filter in ["z == 1", "not done", "true", "false"] {
            cases.push(case(
                format!("L2/{binder}/{filter}"),
                source("done: Bool", "done = false", "done = true", &format!("leadsTo Capture {{ forall {binder} where {filter} {{ not done ~> within 1 done }} }}")),
                [Ok, Error, Error, Error], Some("leadsTo Capture"),
            ));
        }
    }
    for binder in ["z in Set { 1 }", "z: K where z == 1"] {
        let mut c = case(
            format!("L3/{binder}"),
            source(
                "done: Bool",
                "done = false",
                "done = true",
                &format!(
                    "leadsTo Capture {{ forall a: K {{ forall {binder} {{ not done ~> within 1 true }} }} }}"
                ),
            ),
            [Ok, Error, Error, Error],
            Some("leadsTo Capture"),
        );
        c.selectors = true;
        cases.push(c);
    }
    let unicode_source = source(
        "done: Bool",
        "done = false",
        "done = true",
        "leadsTo Capture { forall z in Seq { 1 } { true ~> within 1 true } }",
    )
    .replace(
        "  leadsTo Capture",
        "  invariant Prefix \"日本😀\" { not done or done } leadsTo Capture",
    );
    cases.push(case(
        "L1/utf16-prefix".into(),
        unicode_source,
        [Ok, Error, Error, Error],
        Some("leadsTo Capture"),
    ));
    let mut resolved = case("L1/resolved-component".into(), "compose Resolved {\n  use Support as part from \"component.fsl\"\n  leadsTo Capture { forall z in Seq { 1 } { true ~> within 1 true } }\n}\n".into(), [Ok, Error, Error, Error], Some("leadsTo Capture"));
    resolved.files.push((
        "component.fsl".into(),
        source(
            "done: Bool",
            "done = false",
            "done = true",
            "invariant Safe { not done or done }",
        ),
    ));
    cases.push(resolved);
    for binder in ["z: K", "z in 0..1"] {
        for negative in [false, true] {
            let verdict = if negative { Violated } else { Safe };
            let mut c = case(
                format!("L4/{binder}/{negative}"),
                source(
                    "done: Bool",
                    "done = false",
                    "done = true",
                    &format!(
                        "leadsTo Capture {{ forall {binder} {{ not done ~> within 1 {} }} }}",
                        !negative
                    ),
                ),
                [Ok, verdict, verdict, Error],
                None,
            );
            c.auto = Some(verdict);
            c.selectors = true;
            cases.push(c);
        }
    }
    // Expression quantifiers in P/Q are not leadsTo wrapper binders.
    let mut c = case(
        "L3/expression-not-wrapper".into(),
        source(
            "xs: Set<K>, done: Bool",
            "xs = Set { 1 } done = false",
            "done = true",
            "leadsTo Capture { (exists z in xs where z == 1 { z == 1 }) ~> within 1 (forall z in xs { z == 1 }) }",
        ),
        [Ok, Safe, Safe, Error],
        None,
    );
    c.auto = Some(Safe);
    cases.push(c);
    for expr in [
        "forall n in Seq { 1 } { n == 1 }",
        "exists n in Seq { 1 } { n == 1 }",
        "count(n in Seq { 1 } where n == 1) == 1",
        "sum(n in Seq { 1 } of n) == 1",
        "unique(n in Seq { 1 } where n == 1)",
        "exactlyOne(n in Seq { 1 } where n == 1)",
    ] {
        let start = cases.len();
        pair(
            &mut cases,
            &format!("S1/{expr}"),
            "n: K, done: Bool",
            &format!("forall z in Seq {{ {expr} }} {{ }}\n    n = 1 done = false"),
        );
        for c in &mut cases[start..] {
            c.baseline = [Ok, Error, Error, Error];
        }
    }
    for (site, init, state) in [
        (
            "body",
            "forall n in Seq { 1 } { if n == 1 { } } n = 1 done = false",
            "n: K, done: Bool",
        ),
        (
            "filter",
            "forall n in Seq { 1 } where n == 1 { } n = 1 done = false",
            "n: K, done: Bool",
        ),
        (
            "rhs-index",
            "forall n: K { m[n] = n == 1 } n = 1 done = false",
            "n: K, done: Bool, m: Map<K, Bool>",
        ),
    ] {
        let start = cases.len();
        pair(&mut cases, &format!("S2/{site}"), state, init);
        for c in &mut cases[start..] {
            c.baseline[3] = Error;
        }
    }
    let start = cases.len();
    pair(
        &mut cases,
        "S3/outer-local",
        "n: K, done: Bool",
        "n = 1 done = false forall n in Seq { 1 } { forall a in Seq { n } { } }",
    );
    for c in &mut cases[start..] {
        c.baseline[1..].fill(Error);
        c.expected[3] = Error;
        c.auto = Some(c.expected[1]);
    }
    for (name, expr) in [
        ("own-source", "exists n in Seq { n } { n == 1 }"),
        (
            "sibling-restored",
            "(exists n in Seq { 1 } { n == 1 }) and n == 1",
        ),
        ("aggregate-source", "sum(n in Seq { n } of n) == 1"),
    ] {
        cases.push(case(
            format!("S4/{name}"),
            source(
                "n: K, done: Bool",
                &format!("n = 1 done = false\n    forall z in Seq {{ {expr} }} {{ }}"),
                "done = true",
                "invariant Capture { not done }",
            ),
            [Ok, Error, Error, Error],
            Some("forall z in"),
        ));
    }
    for position in [
        "init",
        "action",
        "invariant",
        "trans",
        "leadsTo",
        "reachable",
    ] {
        for bound in [false, true] {
            let expr = if bound {
                "forall x in 0..1 { h[x] != Red }"
            } else {
                "h[0] != Red"
            };
            let mut init = "h = Set {} done = false".to_owned();
            let mut action = "done = true".to_owned();
            let mut property = "invariant Capture { not done }".to_owned();
            let marker = match position {
                "init" => {
                    init = format!("h = Set {{}}\n    done = {expr}");
                    "done = h[0]"
                }
                "action" => {
                    action = format!("done = {expr}");
                    "done = h[0]"
                }
                "leadsTo" => {
                    property = format!("leadsTo Capture {{ ({expr}) ~> within 1 done }}");
                    "leadsTo Capture"
                }
                _ => {
                    property = format!("{position} Capture {{ {expr} }}");
                    match position {
                        "trans" => "trans Capture",
                        "reachable" => "reachable Capture",
                        _ => "invariant Capture",
                    }
                }
            };
            let marker = if bound && ["init", "action"].contains(&position) {
                "done = forall"
            } else {
                marker
            };
            let text = source(
                "h: relation K -> Color, done: Bool",
                &init,
                &action,
                &property,
            )
            .replace(
                "  type K = 0..1",
                "  type K = 0..1\n  enum Color { Red, Blue }",
            );
            cases.push(case(
                format!("R1/read/{position}/{bound}"),
                text,
                [Ok, Error, Error, Error],
                Some(marker),
            ));
        }
    }
    for position in ["init", "action"] {
        let (init, action) = if position == "init" {
            ("done = false\n    h[0] = Red", "done = true")
        } else {
            ("h = Set {} done = false", "h[0] = Red done = true")
        };
        cases.push(case(
            format!("R1/write/{position}"),
            source(
                "h: relation K -> Color, done: Bool",
                init,
                action,
                "invariant Capture { not done }",
            )
            .replace(
                "  type K = 0..1",
                "  type K = 0..1\n  enum Color { Red, Blue }",
            ),
            [Ok, Error, Error, Error],
            Some("h[0] = Red"),
        ));
    }
    for (name, expr) in [
        ("old", "old(h)[0] != Red"),
        ("conditional", "(if done then h else h)[0] != Red"),
        ("domain-binder", "forall x in domain(h) { h[x] != Red }"),
    ] {
        cases.push(case(
            format!("R1/receiver/{name}"),
            source(
                "h: relation K -> Color, done: Bool",
                "h = Set {} done = false",
                "done = true",
                &format!("trans Capture {{ {expr} }}"),
            )
            .replace(
                "  type K = 0..1",
                "  type K = 0..1\n  enum Color { Red, Blue }",
            ),
            if name == "domain-binder" {
                [Ok, Error, Error, Safe]
            } else {
                [Ok, Error, Error, Error]
            },
            Some("trans Capture"),
        ));
    }
    pair(
        &mut cases,
        "R2/map-index",
        "m: Map<K, Bool>, done: Bool",
        "forall k: K { m[k] = false } done = m[0]",
    );
    let start = cases.len();
    pair(
        &mut cases,
        "R2/map-struct-field",
        "m: Map<K, Cell>, done: Bool",
        "forall k: K { m[k] = Cell { flag: false } } done = m[0].flag",
    );
    for c in &mut cases[start..] {
        c.source = c.source.replace(
            "  type K = 0..1",
            "  type K = 0..1\n  struct Cell { flag: Bool }",
        );
    }
    for negative in [false, true] {
        let verdict = if negative { Violated } else { Safe };
        cases.push(case(format!("R2/relation-methods/{negative}"), source("h: relation K -> K, done: Bool", "h = Set {} done = false", if negative { "h = h.add(0, 1)" } else { "h = h.add(0, 1).remove(0, 1)" }, "invariant Capture { not h.contains(0, 1) and domain(h).size() == 0 and range(h).size() == 0 }"), [Ok, verdict, verdict, verdict], None));
    }
    let c = case(
        "R2/seq-write-remains-rejected".into(),
        source(
            "xs: Seq<K, 2>, done: Bool",
            "xs = Seq { 1 } done = false",
            "xs[0] = 0",
            "invariant Capture { not done }",
        ),
        [Error; 4],
        None,
    );
    cases.push(c);
    let binders = ["a: K", "a in 0..1", "a in Set { 1 }", "a in Seq { 1 }"];
    for outer in binders {
        for inner in binders {
            let start = cases.len();
            pair(
                &mut cases,
                &format!("E1/{outer}/{inner}"),
                "done: Bool",
                &format!(
                    "done = false forall {outer} {{ forall {} {{ }} }}",
                    inner.replace('a', "b")
                ),
            );
            for c in &mut cases[start..] {
                c.baseline[3] = Error;
                c.expected[3] = Error;
                c.auto = Some(c.expected[1]);
            }
        }
    }
    pair(
        &mut cases,
        "E2/ordinary-assigned-read",
        "n: K, done: Bool",
        "n = 1 done = n != 1",
    );
    let mut c = case(
        "E2/nondeterministic-init".into(),
        source(
            "done: Bool",
            "",
            "done = true",
            "invariant Capture { not done }",
        ),
        [Ok, Violated, Violated, Error],
        None,
    );
    c.auto = Some(Violated);
    cases.push(c);
    for (name, text, outcomes) in [
        (
            "static-range",
            source(
                "done: Bool",
                "done = false",
                "done = true",
                "leadsTo Capture { forall z in 0..(1 + 0) { not done ~> within 1 true } }",
            ),
            [Ok, Error, Error, Error],
        ),
        (
            "dynamic-range",
            source(
                "n: K, done: Bool",
                "n = 1 done = false forall z in 0..n { }",
                "done = true",
                "invariant Capture { not done }",
            ),
            [Ok, Error, Error, Error],
        ),
        (
            "negative-within",
            source(
                "done: Bool",
                "done = false",
                "done = true",
                "leadsTo Capture { not done ~> within -1 done }",
            ),
            [Ok, Error, Error, Error],
        ),
        (
            "typed-domain",
            source(
                "done: Bool",
                "done = false",
                "done = true",
                "invariant Capture { forall b: Bool { b or not b } }",
            ),
            [Ok, Error, Error, Error],
        ),
        (
            "operand-method-types",
            source(
                "done: Bool",
                "done = false",
                "done = true",
                "invariant Capture { acyclic(done) }",
            ),
            [Ok, Error, Error, Error],
        ),
        (
            "zero-capacity-head",
            source(
                "xs: Seq<K, 0>, done: Bool",
                "xs = Seq {} done = false",
                "done = true",
                "invariant Capture { xs.head() == 1 }",
            ),
            [Ok, Violated, Violated, Error],
        ),
    ] {
        cases.push(case(format!("X1/{name}"), text, outcomes, None));
    }
    for expr in [
        "Seq { 1 }[0] == 1",
        "some(Set { 1 }) == some(Set { 1 })",
        "Seq { 1 }.head() == 1",
        "Set { 1 } == Set { 1 }",
    ] {
        cases.push(case(
            format!("X1/1467/{expr}"),
            source(
                "done: Bool",
                "done = false",
                "done = true",
                &format!("invariant Capture {{ {expr} }}"),
            ),
            [Ok, Error, Error, Safe],
            None,
        ));
    }
    cases
}

pub fn location(c: &Case) -> (usize, usize) {
    let marker = c.rejection.expect("rejection marker");
    let offset = c.source.find(marker).expect("authored rejection site");
    let prefix = &c.source[..offset];
    (
        prefix.bytes().filter(|b| *b == b'\n').count() + 1,
        prefix.rsplit('\n').next().unwrap().chars().count() + 1,
    )
}

#[allow(dead_code)] // Used by the LSP consumer of this shared inventory.
pub fn utf16_location(c: &Case) -> (usize, usize) {
    let (line, _) = location(c);
    let offset = c.source.find(c.rejection.unwrap()).unwrap();
    let column = c.source[..offset]
        .rsplit('\n')
        .next()
        .unwrap()
        .encode_utf16()
        .count();
    (line - 1, column)
}

pub fn assert_rejection(c: &Case, report: &Value) {
    let (line, column) = location(c);
    assert_eq!(report["result"], "error", "{}: {report}", c.id);
    assert_eq!(report["kind"], "semantics", "{}: {report}", c.id);
    // Default semantic codes are omitted by CLI/Worker, not replaced with an
    // invented rule-specific code. LSP exposes FSL-SEMANTIC for the same rule.
    assert!(
        report.get("diagnostic_code").is_none(),
        "{}: {report}",
        c.id
    );
    assert_eq!(
        report["loc"],
        json!({"line": line, "column": column}),
        "{}: {report}",
        c.id
    );
    assert!(
        report["message"].as_str().is_some_and(|s| !s.is_empty()),
        "{}: {report}",
        c.id
    );
}
