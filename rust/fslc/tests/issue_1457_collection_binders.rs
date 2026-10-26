// SPDX-License-Identifier: Apache-2.0

//! #1457: one inventory generates accepting/rejecting collection-binder pairs.
//! Keep every detector, including Set value ordering, when reducing run time.
//! Stateful init forall has a shared verify refusal; `check` acceptance and
//! bounded leadsTo remain separately tracked siblings.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use serde_json::Value;

const OPS: [&str; 6] = ["exists", "forall", "count", "sum", "unique", "exactlyOne"];
const SITES: [&str; 5] = ["rhs", "requires", "init", "invariant", "trans"];
const KINDS: [&str; 2] = ["Set", "Seq"];

struct Case {
    name: String,
    kind: &'static str,
    site: &'static str,
    values: String,
    expression: String,
    setup: String,
    body: String,
    enumeration: bool,
    expected: bool,
    partial: bool,
}

impl Case {
    fn new(name: String, kind: &'static str, expression: String) -> Self {
        Self {
            name,
            kind,
            site: "rhs",
            values: "1".into(),
            expression,
            setup: String::new(),
            body: String::new(),
            enumeration: false,
            expected: true,
            partial: false,
        }
    }

    fn source(&self, negative: bool) -> String {
        let ty = if self.kind == "Set" {
            "Set<K>"
        } else {
            "Seq<K, 3>"
        };
        let declaration = if self.enumeration {
            "enum K { One, Other }"
        } else {
            "type K = 0..1"
        };
        let literal = format!("{} {{ {} }}", self.kind, self.values);
        let mut init = format!("xs = {literal} flag = false done = false");
        let parameter = if self.setup.contains("requires p ==") {
            "p: K"
        } else {
            ""
        };
        let mut action = format!(
            "action go({parameter}) {{ {} flag = {} done = true }}",
            self.setup, self.expression
        );
        let relation = if negative { "!=" } else { "==" };
        let mut property = format!(
            "invariant Capture {{ not done or flag {relation} {} }}",
            self.expected
        );
        match self.site {
            "rhs" => {}
            "requires" => {
                action = format!(
                    "action go() {{ {} requires {} flag = true done = true }}",
                    self.setup, self.expression
                );
            }
            "init" => {
                init = format!("xs = {literal} flag = {} done = true", self.expression);
                action = "action go() {}".into();
            }
            "invariant" | "trans" => {
                action = "action go() {}".into();
                property = format!(
                    "{} Capture {{ {}({}) }}",
                    self.site,
                    if negative { "not " } else { "" },
                    self.expression
                );
            }
            "statement" => {
                action = format!(
                    "action go() {{ {} forall z in {} {{ {} }} flag = true done = true }}",
                    self.setup, self.expression, self.body
                );
            }
            "init-statement" | "init-state" => {
                init = format!("{init} forall z in {} {{ {} }}", self.expression, self.body);
                action = "action go() { flag = true done = true }".into();
            }
            _ => panic!("unknown site: {}", self.site),
        }
        let source = format!(
            "spec Collection {{\n {declaration}\n state {{ xs: {ty}, flag: Bool, done: Bool }}\n init {{ {init} }}\n {action}\n invariant Domain {{ xs == {literal} }}\n {property}\n}}\n"
        );
        if self.name.starts_with("order-enum/") {
            source.replace("One", "Z").replace("Other", "A")
        } else {
            source
        }
    }
}

fn expression(op: &str, domain: &str, enumeration: bool) -> String {
    let one = if enumeration { "One" } else { "1" };
    let binder = format!("z in {domain}");
    match op {
        "exists" | "forall" => format!("{op} {binder} {{ z == {one} }}"),
        "count" => format!("count({binder} where z == {one}) == 1"),
        "sum" => format!(
            "sum({binder} of {}) == 1",
            if enumeration { "1" } else { "z" }
        ),
        "unique" | "exactlyOne" => format!("{op}({binder} where z == {one})"),
        _ => panic!("unknown aggregate: {op}"),
    }
}

// Keep the complete fixture inventory together rather than hide matrix axes
// behind unrelated test functions (the repository uses this for long fixtures).
#[allow(clippy::too_many_lines)]
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for site in SITES {
        for op in OPS {
            for kind in KINDS {
                for origin in ["state", "literal", "let"] {
                    if (origin == "let" && !["rhs", "requires"].contains(&site))
                        || (origin == "literal" && kind == "Set" && ["count", "sum"].contains(&op))
                    {
                        continue;
                    }
                    let domain = match origin {
                        "state" => "xs".into(),
                        "let" => "ys".into(),
                        _ => format!("{kind} {{ 1 }}"),
                    };
                    let mut case = Case::new(
                        format!("{site}/{op}/{kind}/{origin}"),
                        kind,
                        expression(op, &domain, false),
                    );
                    case.site = site;
                    if origin == "let" {
                        case.setup = "let ys = xs".into();
                    }
                    cases.push(case);
                }
                let mut case = Case::new(
                    format!("typed-literal/{site}/{op}/{kind}"),
                    kind,
                    expression(op, &format!("{kind} {{ One }}"), true),
                );
                case.site = site;
                case.enumeration = true;
                case.values = "One".into();
                cases.push(case);
            }
        }
    }
    for kind in KINDS {
        for origin in ["state", "literal", "let"] {
            let domain = match origin {
                "state" => "xs".into(),
                "let" => "ys".into(),
                _ => format!("{kind} {{ 1 }}"),
            };
            let mut case = Case::new(format!("stmt-empty/forall/{kind}/{origin}"), kind, domain);
            case.site = "statement";
            if origin == "let" {
                case.setup = "let ys = xs".into();
            }
            cases.push(case);
        }
        for origin in ["state", "literal"] {
            for op in ["count", "sum", "unique", "exactlyOne"] {
                let domain = if origin == "state" {
                    "xs".into()
                } else {
                    format!("{kind} {{ One, One }}")
                };
                let expr = match op {
                    "count" => format!("count(z in {domain}) == 1"),
                    "sum" => format!("sum(z in {domain} of 1) == 1"),
                    _ => format!("{op}(z in {domain})"),
                };
                let mut case = Case::new(format!("duplicates/{op}/{kind}/{origin}"), kind, expr);
                case.enumeration = true;
                case.values = "One, One".into();
                case.expected = kind == "Set";
                cases.push(case);
            }
        }
        for op in OPS {
            let mut case = Case::new(
                format!("empty-state/{op}/{kind}"),
                kind,
                expression(op, "xs", false),
            );
            case.values.clear();
            case.expected = ["forall", "unique"].contains(&op);
            cases.push(case);
        }
        for origin in ["state", "literal"] {
            for empty in [false, true] {
                if empty && origin == "literal" {
                    continue;
                }
                let domain = if origin == "state" {
                    "xs".into()
                } else {
                    format!("{kind} {{ 1 }}")
                };
                let mut case = Case::new(
                    format!(
                        "action-stmt/{}/{kind}/{origin}",
                        if empty { "empty-domain" } else { "empty-body" }
                    ),
                    kind,
                    domain,
                );
                case.site = "statement";
                if empty {
                    case.values.clear();
                }
                cases.push(case);
            }
        }
        let mut case = Case::new(
            format!("init-stmt/empty-body/{kind}/literal"),
            kind,
            format!("{kind} {{ 1 }}"),
        );
        case.site = "init-statement";
        cases.push(case);
        for (name, values) in [("empty-body", "1"), ("empty-domain", "")] {
            let mut case = Case::new(format!("init-stmt/{name}/{kind}/state"), kind, "xs".into());
            case.site = "init-state";
            case.values = values.into();
            cases.push(case);
        }
        for values in ["1", "", "0"] {
            for position in ["body", "where"] {
                let domain = if position == "where" {
                    "xs where 1 / z == 1"
                } else {
                    "xs"
                };
                let mut case = Case::new(
                    format!(
                        "membership/{position}/{kind}/{}",
                        match values {
                            "" => "empty",
                            "0" => "live-zero",
                            _ => "live-one",
                        }
                    ),
                    kind,
                    domain.into(),
                );
                case.site = "statement";
                case.values = values.into();
                case.partial = values == "0";
                if position == "body" {
                    case.body = "if 1 / z == 1 {}".into();
                }
                cases.push(case);
            }
        }
        for op in OPS {
            for origin in ["argument-derived", "let-literal"] {
                if origin == "let-literal" && kind == "Set" && ["count", "sum"].contains(&op) {
                    continue;
                }
                let domain = if origin == "argument-derived" {
                    format!("{kind} {{ p }}")
                } else {
                    "ys".into()
                };
                let mut case = Case::new(
                    format!("{origin}/{op}/{kind}"),
                    kind,
                    expression(op, &domain, false),
                );
                case.setup = if origin == "argument-derived" {
                    "requires p == 1".into()
                } else {
                    format!("let ys = {kind} {{ 1 }}")
                };
                cases.push(case);
            }
        }
    }
    for kind in KINDS {
        for origin in ["state", "literal"] {
            for values in ["1, 0", "0, 1"] {
                let domain = if origin == "state" {
                    "xs".into()
                } else {
                    format!("{kind} {{ {values} }}")
                };
                let name = if kind == "Seq" {
                    format!(
                        "order/exists/Seq/{origin}/{}",
                        if values == "1, 0" { "safe" } else { "partial" }
                    )
                } else {
                    format!("order-Set/{origin}/{}", values.replace(", ", "-"))
                };
                let mut case = Case::new(
                    name,
                    kind,
                    format!("exists z in {domain} {{ z == 1 or 1 / z == 1 }}"),
                );
                case.values = values.into();
                case.partial = kind == "Set" || values == "0, 1";
                cases.push(case);
            }
            let domain = if origin == "state" {
                "xs"
            } else {
                "Set { One, Other }"
            };
            if kind == "Set" {
                let mut case = Case::new(
                    format!("order-enum/{origin}/declaration-vs-value"),
                    kind,
                    format!(
                        "exists z in {domain} {{ z == One or 1 / (if z == Other then 0 else 1) == 1 }}"
                    ),
                );
                case.enumeration = true;
                case.values = "One, Other".into();
                case.partial = true;
                cases.push(case);
            }
        }
    }
    let mut case = Case::new(
        "live-prefix/exists-padding-zero/Seq/state".into(),
        "Seq",
        "exists z in xs { z == 0 }".into(),
    );
    case.expected = false;
    cases.push(case);
    for (name, setup, expr, partial) in [
        (
            "dynamic-set/duplicate-p1",
            "requires p == 1",
            "count(z in Set { p, 1 }) == 1",
            false,
        ),
        (
            "dynamic-set/order-p0",
            "requires p == 0",
            "exists z in Set { p, 1 } { z == 1 or 1 / z == 1 }",
            true,
        ),
    ] {
        let mut case = Case::new(name.into(), "Set", expr.into());
        case.setup = setup.into();
        case.partial = partial;
        cases.push(case);
    }
    cases
}

fn fixture(source: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../target/issue-1457-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("fixture directory inside worktree");
    let path = directory.join(format!("{}.fsl", NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&path, source).expect("fixture source");
    path
}

fn run(path: &Path, engine: &str) -> (i32, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    if engine == "check" {
        command.arg("check").arg(path);
    } else {
        command
            .arg("verify")
            .arg(path)
            .args(["--engine", engine, "--depth", "4", "--no-cache"]);
    }
    let output = command.output().expect("CLI output");
    assert!(
        output.stderr.is_empty(),
        "CLI stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    (output.status.code().expect("CLI exit"), report)
}

fn diagnostic(status: i32, report: &Value) -> String {
    format!(
        "{status} result={} kind={} message={} violation={}/{}",
        report["result"],
        report["kind"],
        report["message"],
        report["violation_kind"],
        report["invariant"]
    )
}

fn exercise(case: &Case, negative: bool) -> Vec<String> {
    let mut failures = Vec::new();
    let path = fixture(&case.source(negative));
    let checked = run(&path, "check");
    if checked.0 != 0 || checked.1["result"] != "ok" {
        failures.push(format!(
            "{} negative={negative} check: expected 0 ok; produced {}",
            case.name,
            diagnostic(checked.0, &checked.1)
        ));
    }
    let mut partial_evidence = None;
    let observations = ["bmc", "induction", "explicit"].map(|engine| (engine, run(&path, engine)));
    let refusal = (
        observations[2].1.1["kind"].clone(),
        observations[2].1.1["message"].clone(),
    );
    for (engine, (status, report)) in observations {
        if case.site == "init-state" {
            // Compare the complete kind/message values with the existing
            // explicit refusal. Do not duplicate the product diagnostic.
            let actual = (report["kind"].clone(), report["message"].clone());
            if status != 2
                || report["result"] != "error"
                || refusal.0.as_str().is_none_or(str::is_empty)
                || refusal.1.as_str().is_none_or(str::is_empty)
                || actual != refusal
            {
                failures.push(format!("{} negative={negative} {engine}: expected 2 error with explicit kind/message {refusal:?}; produced {}", case.name, diagnostic(status, &report)));
            }
            continue;
        }
        let expected = if negative || case.partial {
            "violated"
        } else if engine == "bmc" {
            "verified"
        } else {
            "proved"
        };
        let expected_status = i32::from(negative || case.partial);
        let expected_name = if case.partial {
            "_partial_go"
        } else {
            "Capture"
        };
        let expected_kind = if case.partial {
            "partial_op"
        } else if case.site == "trans" {
            "trans"
        } else {
            "invariant"
        };
        let valid = status == expected_status
            && report["result"] == expected
            && (!(negative || case.partial)
                || (report["invariant"] == expected_name
                    && report["violation_kind"] == expected_kind));
        if !valid {
            failures.push(format!("{} negative={negative} {engine}: expected {expected_status} {expected} {expected_kind}/{expected_name}; produced {}", case.name, diagnostic(status, &report)));
        }
        if case.partial && report["result"] == "violated" {
            // The comparison scope is the entire location and replay trace,
            // not a projection/shape of selected state or action fields.
            let evidence = (report["loc"].clone(), report["trace"].clone());
            if evidence.0.is_null() || evidence.1.as_array().is_none_or(Vec::is_empty) {
                failures.push(format!(
                    "{} {engine}: missing partial location/trace",
                    case.name
                ));
            }
            if let Some(previous) = &partial_evidence {
                if previous != &evidence {
                    failures.push(format!("{} {engine}: partial location/trace mismatch: {previous:?} != {evidence:?}", case.name));
                }
            } else {
                partial_evidence = Some(evidence);
            }
        }
    }
    failures
}

#[test]
fn generated_collection_binder_matrix() {
    let started = Instant::now();
    let inventory = cases();
    assert_eq!(inventory.len(), 287, "do not lose matrix cells");
    assert_eq!(
        inventory
            .iter()
            .map(|case| &case.name)
            .collect::<BTreeSet<_>>()
            .len(),
        inventory.len()
    );
    let mut failures = Vec::new();
    let mut red = Vec::new();
    for case in inventory {
        let before = failures.len();
        for negative in [false, true] {
            failures.extend(exercise(&case, negative));
        }
        if failures.len() == before {
            println!("PRESERVED {}", case.name);
        } else {
            println!("RED {}", case.name);
            red.push(case.name);
        }
    }
    println!(
        "MATRIX elapsed={:?} red={} preserved={}",
        started.elapsed(),
        red.len(),
        287 - red.len()
    );
    assert!(
        started.elapsed().as_secs() < 180,
        "report a runtime budget failure without reducing the inventory"
    );
    assert!(
        failures.is_empty(),
        "red cells (all): {red:#?}\nproduced versus expected:\n{}",
        failures.join("\n")
    );
}

#[test]
fn rejected_collection_boundaries_stay_rejected() {
    let prefix = "spec Rejected { type K = 0..1 state { xs: Set<K>, flag: Bool, m: Map<K, Bool> } init { xs = Set { 1 } flag = false forall k: K { m[k] = false } }";
    for (name, action, property, message) in [
        (
            "empty",
            "action go() { flag = exists z in Set {} { true } }",
            "",
            "cannot infer empty Set",
        ),
        (
            "nonfinite",
            "action go() { flag = count(z in Set { 1 }) == 1 }",
            "",
            "not a finite scalar",
        ),
        (
            "map",
            "action go() { forall z in m {} }",
            "",
            "collection binder requires Set or Seq",
        ),
        ("parameter", "action go(p: Set<K>) {}", "", "expected ','"),
        (
            "scope",
            "action go() { let ys = xs }",
            "invariant Capture { exists z in ys { true } }",
            "cannot type identifier 'ys'",
        ),
        (
            "write",
            "action go() { forall z in xs { m[z] = (z == 1) } }",
            "",
            "cannot prove write-index distinctness",
        ),
    ] {
        let path = fixture(&format!("{prefix} {action} {property} }}"));
        let (status, report) = run(&path, "check");
        assert_eq!(status, 2, "{name}: {report}");
        assert_eq!(report["result"], "error", "{name}: {report}");
        assert!(
            report["message"]
                .as_str()
                .is_some_and(|actual| actual.contains(message)),
            "{name}: expected {message}; produced {report}"
        );
    }
}
