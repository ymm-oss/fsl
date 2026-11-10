// SPDX-License-Identifier: Apache-2.0

use super::forms::Case;

pub fn assert_message(case: &Case, message: &str) {
    if case.id.starts_with("nested/") && case.rejection {
        assert!(
            message.contains(
                "local binding 'x' shadows a constant of the same name; rename the local binding"
            ),
            "{}: {message}",
            case.id
        );
    }
    if let Some(name) = case.public_name {
        assert!(message.contains(name), "{}: {message}", case.id);
        assert!(!message.contains("__"), "{}: {message}", case.id);
        assert!(
            message.contains("piece.fsl:"),
            "{}: component cause lost: {message}",
            case.id
        );
    }
    if case.id.starts_with("bound/component/") {
        assert!(message.contains("piece.fsl:6:3"), "{}: {message}", case.id);
    }
    if case.id.starts_with("sync/") {
        if case.public_name.is_some() {
            assert!(message.contains("piece.fsl:14:5"), "{}: {message}", case.id);
        } else {
            assert!(
                !message.contains("component"),
                "{}: root statement rebound: {message}",
                case.id
            );
        }
    }
}

#[allow(clippy::too_many_lines)] // Explicit nested-aggregate, bound and sync inventory.
pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for domain in ["range", "set"] {
        for empty in [false, true] {
            for shadow in [false, true] {
                let source = if domain == "set" {
                    "xs"
                } else if empty {
                    "1..0"
                } else {
                    "0..1"
                };
                for (mode, expression, full) in [
                    (
                        "value-count",
                        format!("sum(x in {source} of count(y in 0..x))"),
                        3,
                    ),
                    (
                        "value-sum",
                        format!("sum(x in {source} of sum(y in 0..x of 1))"),
                        3,
                    ),
                    (
                        "filter",
                        format!("count(x in {source} where count(y in 0..x) > 0)"),
                        2,
                    ),
                ] {
                    for negative in [false, true] {
                        let expected = if empty { 0 } else { full };
                        cases.push(Case {
                            id: format!("nested/{domain}/{empty}/{shadow}/{mode}/{negative}"),
                            source: format!("spec Nested {{\n  {}\n  type K = 0..1\n  state {{ xs: Set<K>, done: Bool }}\n  init {{ xs = Set {{ {} }} done = false }}\n  action go() {{}}\n  invariant Capture {{ {expression} == {} }}\n}}\n", if shadow { "const x = false" } else { "" }, if empty { "" } else { "0, 1" }, expected + i32::from(negative)),
                            // Const-shadow forms are now located refusal detectors;
                            // fresh forms retain their original execution oracle.
                            files: vec![], rejection: shadow, negative, public_name: None, marker: "invariant Capture",
                        });
                    }
                }
            }
        }
    }
    for negative in [false, true] {
        cases.push(Case {
            id: format!("nested/state-population/{negative}"),
            source: format!("spec P {{\n  type V = 0..2\n  state {{ s: Set<V>, n: 0..3 }}\n  init {{ s = Set {{}} n = 0 }}\n  action put(v: V) {{ requires not s.contains(v) s = s.add(v) n = n + 1 }}\n  invariant Capture {{ sum(x in s of count(y in 0..x)) <= {} }}\n}}\n", if negative { -1 } else { 6 }),
            files: vec![], rejection: false, negative, public_name: None, marker: "invariant Capture",
        });
    }
    for empty in [false, true] {
        for present in [false, true] {
            for kind in ["count", "sum"] {
                for negative in [false, true] {
                    let source = if empty { "1..0" } else { "0..1" };
                    let expression = if kind == "count" {
                        format!("count(x in {source} where o is some(x))")
                    } else {
                        format!("sum(x in {source} of 1 where o is some(x))")
                    };
                    let expected = if !empty && present { 2 } else { 0 };
                    cases.push(Case { id: format!("pattern/{empty}/{present}/{kind}/{negative}"),
                        source: format!("spec Pattern {{\n  type K = 0..1\n  state {{ o: Option<K>, done: Bool }}\n  init {{ o = {} done = false }}\n  action go() {{}}\n  invariant Capture {{ {expression} == {} }}\n}}\n", if present { "some(1)" } else { "none" }, expected + i32::from(negative)),
                        files: vec![], rejection: false, negative, public_name: None, marker: "invariant Capture" });
                }
            }
        }
    }
    for (kind, expression) in [
        ("forall", "forall x in 0..1 { o is some(x) }"),
        ("unique", "unique(x in 0..1 where o is some(x))"),
    ] {
        cases.push(Case { id: format!("scope-error/{kind}"), source: format!("spec Scope {{\n  type K = 0..1\n  state {{ o: Option<K> }}\n  init {{ o = none }}\n  action go() {{}}\n  invariant Capture {{ {expression} }}\n}}\n"), files: vec![], rejection: true, negative: false, public_name: None, marker: "invariant Capture" });
    }
    for aggregate in [false, true] {
        for hi in [false, true] {
            for component in [false, true] {
                let (lo, high) = if hi { ("0", "h[0]") } else { ("h[0]", "1") };
                let expression = if aggregate {
                    format!("count(z in {lo}..{high} where true) == 0")
                } else {
                    format!("forall z in {lo}..{high} {{ true }}")
                };
                let source = format!(
                    "spec Piece {{\n  type K = 0..1\n  state {{ h: relation K -> K }}\n  init {{ h = Set {{}} }}\n  action go() {{}}\n  invariant Capture {{ {expression} }}\n}}\n"
                );
                cases.push(Case {
                    id: format!(
                        "bound/{}/{aggregate}/{hi}",
                        if component { "component" } else { "root" }
                    ),
                    source: if component {
                        "compose Root {\n  use Piece as part from \"piece.fsl\"\n}\n".into()
                    } else {
                        source.clone()
                    },
                    files: if component {
                        vec![("piece.fsl".into(), source)]
                    } else {
                        vec![]
                    },
                    rejection: true,
                    negative: false,
                    public_name: if component {
                        Some("part.Capture")
                    } else {
                        None
                    },
                    marker: if component {
                        "use Piece"
                    } else {
                        "invariant Capture"
                    },
                });
            }
        }
    }
    for (mode, item) in [
        ("read", "done = h[a] == a"),
        ("write", "h[a] = a"),
        ("requires", "requires h[a] == a"),
        ("let", "let v = h[a]"),
        ("ensures", "ensures h[a] == a"),
        ("root", "done = true"),
    ] {
        for third in [false, true] {
            if mode == "root" && third {
                continue;
            }
            let (uses, internals, refs, public_name) = if third {
                (
                    "  use Good as part from \"good.fsl\"\n  use Piece as other from \"piece.fsl\"",
                    "  internal part.idle\n  internal other.go",
                    "part.idle() || other.go(0)",
                    Some("other.go"),
                )
            } else {
                (
                    "  use Piece as part from \"piece.fsl\"",
                    "  internal part.go",
                    "part.go(0)",
                    Some("part.go"),
                )
            };
            let piece = format!(
                "spec Piece {{\n  type K = 0..1\n  state {{\n    h: relation K -> K,\n    done: Bool\n  }}\n  init {{\n    h = Set {{}}\n    done = false\n  }}\n  action idle() {{}}\n  // source padding\n  action go(a: K) {{\n    {item}\n  }}\n}}\n"
            );
            cases.push(Case { id: format!("sync/{mode}/{third}"),
                source: format!("compose Root {{\n  use Good as first from \"good.fsl\"\n{uses}\n  internal first.idle\n{internals}\n  state {{ local: Bool }}\n  init {{ local = false }}\n  action joined() = first.idle() || {refs} {{\n{}\n  }}\n}}\n", if mode == "root" { "    local = part.h[0] == 0" } else { "" }),
                files: vec![("good.fsl".into(), "spec Good { state { seen: Bool } init { seen = false } action idle() { requires true let seed = 0 seen = seen ensures true } }".into()), ("piece.fsl".into(), piece)],
                rejection: true, negative: false, public_name: if mode == "root" { None } else { public_name }, marker: if mode == "root" { "local = part.h" } else { "use Piece" } });
        }
    }
    cases
}
