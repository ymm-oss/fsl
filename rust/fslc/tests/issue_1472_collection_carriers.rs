// SPDX-License-Identifier: Apache-2.0

//! Whole finite values require equal carriers; scalar arguments and Option do not.
//! Refusals detect the missing frontend gate. Equal-range aliases detect missing
//! symbolic retagging. Every refusal runs a matched-carrier control first.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

#[path = "support/scratch_identity.rs"]
mod scratch_identity;

struct Case {
    name: &'static str,
    state: &'static str,
    init: &'static str,
    actions: &'static str,
    property: &'static str,
    actual: &'static str,
    expected: &'static str,
    loc: (u64, u64),
}

impl Case {
    fn source(&self) -> String {
        format!(
            "spec P {{\n  type N = 0..2\n  type M = 0..4\n  type K = 0..1\n  state {{ {} }}\n  init {{ {} }}\n  {}\n  invariant P {{ {} }}\n}}\n",
            self.state, self.init, self.actions, self.property
        )
    }

    fn control(&self) -> String {
        self.source()
            .replace("<M", "<N")
            .replace(", M>", ", N>")
            .replace("relation M -> M", "relation N -> N")
            .replace("forall m: M", "forall m: N")
            .replace("Seq<N, 1>", "Seq<N, 2>")
    }
}

const CASES: &[Case] = &[
    Case {
        name: "a01_assignment",
        state: "ys: Set<N>, xs: Set<M>",
        init: "ys = Set {} xs = Set {}",
        actions: "action addy(a: N) { requires a == 2 ys = ys.add(a) } action snap() { requires ys.contains(2) xs = ys }",
        property: "not xs.contains(2)",
        actual: "Set(Named(\"N\"))",
        expected: "Set(Named(\"M\"))",
        loc: (7, 96),
    },
    Case {
        name: "a02_equality",
        state: "ys: Set<N>, xs: Set<M>",
        init: "ys = Set {} xs = Set {}",
        actions: "action addy(a: N) { requires a == 2 ys = ys.add(a) }",
        property: "xs == ys",
        actual: "Set(Named(\"N\"))",
        expected: "Set(Named(\"M\"))",
        loc: (8, 3),
    },
    Case {
        name: "a03_range",
        state: "r: relation N -> N, xs: Set<M>",
        init: "r = Set {} xs = Set {}",
        actions: "action addr() { r = r.add(0, 2) } action snap() { requires r.contains(0, 2) xs = range(r) }",
        property: "not xs.contains(2)",
        actual: "Set(Named(\"N\"))",
        expected: "Set(Named(\"M\"))",
        loc: (7, 79),
    },
    Case {
        name: "a04_domain_frozen",
        state: "r: relation N -> N, q: relation M -> M",
        init: "r = Set {} q = Set {}",
        actions: "action addr() { r = r.add(0, 2) }",
        property: "domain(r) == domain(q)",
        actual: "Set(Named(\"M\"))",
        expected: "Set(Named(\"N\"))",
        loc: (8, 3),
    },
    Case {
        name: "a04_domain_live",
        state: "r: relation N -> N, q: relation M -> M",
        init: "r = Set {} q = Set {}",
        actions: "action addr() { r = r.add(0, 2) } action addrq() { requires r.contains(0, 2) q = q.add(0, 2) }",
        property: "domain(r) == domain(q)",
        actual: "Set(Named(\"M\"))",
        expected: "Set(Named(\"N\"))",
        loc: (8, 3),
    },
    Case {
        name: "issue_1627_add_result",
        state: "s: Set<N>, t: Set<M>, done: Bool",
        init: "s = Set {} t = Set {} done = false",
        actions: "action go() { requires not done s = s.add(1) t = s.add(1) done = true }",
        property: "not done or t.contains(1)",
        actual: "Set(Named(\"N\"))",
        expected: "Set(Named(\"M\"))",
        loc: (7, 48),
    },
    Case {
        name: "b01_seq_assignment",
        state: "ys: Seq<N, 2>, xs: Seq<M, 2>",
        init: "ys = Seq { 2 } xs = Seq {}",
        actions: "action snap() { xs = ys }",
        property: "not xs.contains(2)",
        actual: "Seq(Named(\"N\"), 2)",
        expected: "Seq(Named(\"M\"), 2)",
        loc: (7, 19),
    },
    Case {
        name: "b02_seq_equality",
        state: "ys: Seq<N, 2>, xs: Seq<M, 2>",
        init: "ys = Seq {} xs = Seq {}",
        actions: "action addy() { requires ys.size() < 2 ys = ys.push(2) }",
        property: "xs == ys",
        actual: "Seq(Named(\"N\"), 2)",
        expected: "Seq(Named(\"M\"), 2)",
        loc: (8, 3),
    },
    Case {
        name: "b04_map_value",
        state: "ys: Map<K, N>, xs: Map<K, M>",
        init: "forall k: K { ys[k] = 2 xs[k] = 0 }",
        actions: "action snap() { xs = ys }",
        property: "xs[0] != 2",
        actual: "Map(Named(\"K\"), Named(\"N\"))",
        expected: "Map(Named(\"K\"), Named(\"M\"))",
        loc: (7, 19),
    },
    Case {
        name: "b05_map_key",
        state: "ys: Map<N, Bool>, xs: Map<M, Bool>",
        init: "forall n: N { ys[n] = true } forall m: M { xs[m] = false }",
        actions: "action snap() { xs = ys }",
        property: "not xs[0]",
        actual: "Map(Named(\"N\"), Bool)",
        expected: "Map(Named(\"M\"), Bool)",
        loc: (7, 19),
    },
    Case {
        name: "b06_relation_endpoints",
        state: "ys: relation N -> N, xs: relation M -> M",
        init: "ys = Set {} xs = Set {}",
        actions: "action addy() { ys = ys.add(2, 2) } action snap() { requires ys.contains(2, 2) xs = ys }",
        property: "not xs.contains(2, 2)",
        actual: "Relation(N, N)",
        expected: "Relation(M, M)",
        loc: (7, 82),
    },
    Case {
        name: "b07_set_if",
        state: "ys: Set<N>, xs: Set<M>, flag: Bool",
        init: "ys = Set {} xs = Set {} flag = false",
        actions: "action go() { requires not flag ys = ys.add(2) flag = true }",
        property: "not (if flag then ys else xs).contains(2)",
        actual: "Set(Named(\"M\"))",
        expected: "Set(Named(\"N\"))",
        loc: (8, 43),
    },
    Case {
        name: "b08_relation_if",
        state: "r: relation N -> N, q: relation M -> M, flag: Bool",
        init: "r = Set {} q = Set {} flag = false",
        actions: "action go() { requires not flag r = r.add(1, 2) flag = true }",
        property: "not reachable(if flag then r else q, 1, 2)",
        actual: "Relation(M, M)",
        expected: "Relation(N, N)",
        loc: (8, 51),
    },
    Case {
        name: "b11_sum_binder",
        state: "ys: Set<N>, xs: Set<M>, flag: Bool",
        init: "ys = Set {} xs = Set {} flag = false",
        actions: "action go() { requires not flag ys = ys.add(2) flag = true }",
        property: "sum(x in (if flag then ys else xs) of x) == 0",
        actual: "Set(Named(\"M\"))",
        expected: "Set(Named(\"N\"))",
        loc: (8, 48),
    },
    Case {
        name: "b13_seq_capacity",
        state: "ys: Seq<N, 1>, xs: Seq<N, 2>",
        init: "ys = Seq { 2 } xs = Seq {}",
        actions: "action snap() { xs = ys }",
        property: "not xs.contains(2)",
        actual: "Seq(Named(\"N\"), 1)",
        expected: "Seq(Named(\"N\"), 2)",
        loc: (7, 19),
    },
    Case {
        name: "b15_map_equality",
        state: "ys: Map<K, N>, xs: Map<K, M>",
        init: "forall k: K { ys[k] = 0 xs[k] = 0 }",
        actions: "action addy() { ys[0] = 2 }",
        property: "xs == ys",
        actual: "Map(Named(\"K\"), Named(\"N\"))",
        expected: "Map(Named(\"K\"), Named(\"M\"))",
        loc: (8, 3),
    },
];

fn run(label: &str, source: &str) -> [(&'static str, i32, Value); 4] {
    let directory = scratch_identity::unique(
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("collection-carriers-{label}")),
    );
    std::fs::create_dir_all(&directory).expect("create carrier fixture directory");
    std::fs::write(directory.join("input.fsl"), source).expect("write carrier fixture");
    ["check", "bmc", "induction", "explicit"].map(|mode| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args([
            if mode == "check" { "check" } else { "verify" },
            "input.fsl",
        ]);
        if mode != "check" {
            command.args(["--engine", mode, "--depth", "4", "--no-cache"]);
        }
        let output = command
            .current_dir(&directory)
            .env("FSLC_CACHE", "off")
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .output()
            .expect("run carrier fixture");
        assert!(
            output.stderr.is_empty(),
            "{label} {mode}: {:?}",
            output.stderr
        );
        (
            mode,
            output.status.code().expect("exit code"),
            serde_json::from_slice(&output.stdout).expect("CLI JSON"),
        )
    })
}

fn accepted(label: &str, source: &str, results: [&str; 3]) {
    let outputs = run(label, source);
    for ((mode, code, output), result) in outputs.iter().zip(["ok"].into_iter().chain(results)) {
        let expected_code = i32::from(matches!(result, "violated" | "unknown_cti"));
        assert_eq!(
            (*code, output["result"].as_str()),
            (expected_code, Some(result)),
            "{label} {mode}: {outputs:#?}"
        );
        assert!(
            output.get("kind").is_none() && output.get("message").is_none(),
            "{label} {mode}: {output}"
        );
        if result == "violated" {
            let bound = match label {
                "b09_set_argument" => Some("_bounds_s"),
                "b10_relation_arguments" => Some("_bounds_r"),
                _ => None,
            };
            assert_eq!(
                output["invariant"],
                bound.unwrap_or("P"),
                "{label}: {output}"
            );
            assert_eq!(
                output["violation_kind"],
                if bound.is_some() {
                    "type_bound"
                } else {
                    "invariant"
                },
                "{label}: {output}"
            );
            assert_eq!(
                output["loc"],
                if bound.is_some() {
                    Value::Null
                } else {
                    json!({"line": 8, "column": 3})
                },
                "{label}: {output}"
            );
        }
    }
}

fn control(case: &Case) {
    let source = if case.name == "issue_1627_add_result" {
        case.control()
            .replace("or t.contains(1)", "or not t.contains(1)")
    } else {
        case.control()
    };
    accepted(case.name, &source, ["violated"; 3]);
}

fn rejected(index: usize) {
    let case = &CASES[index];
    control(case);
    let source = if case.name == "b08_relation_if" {
        case.source().replace("type M = 0..4", "type M = 1..3")
    } else {
        case.source()
    };
    let outputs = run(case.name, &source);
    let (line, column) = case.loc;
    let message = format!(
        "invalid model expression: expression of type {} is not assignable to {} at {line}:{column}",
        case.actual, case.expected
    );
    for (mode, code, output) in &outputs {
        assert_eq!(
            (
                *code,
                &output["result"],
                &output["kind"],
                &output["message"],
                &output["loc"]
            ),
            (
                2,
                &json!("error"),
                &json!("semantics"),
                &json!(message),
                &json!({"line": line, "column": column})
            ),
            "{} {mode}: {outputs:#?}",
            case.name
        );
        // Located frontend failures must be identical in full, not just exits.
        assert_eq!(output, &outputs[0].2, "{} {mode}", case.name);
    }
}

macro_rules! detectors {
    ($($name:ident: $index:expr),* $(,)?) => { $(#[test] fn $name() { rejected($index); })* };
}

detectors! {
    a01_assignment: 0, a02_equality: 1, a03_range: 2,
    a04_domain_frozen: 3, a04_domain_live: 4, issue_1627_add_result: 5,
    b01_seq_assignment: 6, b02_seq_equality: 7, b04_map_value: 8,
    b05_map_key: 9, b06_relation_endpoints: 10, b07_set_if: 11,
    b08_relation_if: 12, b11_sum_binder: 13, b13_seq_capacity: 14, b15_map_equality: 15,
}

#[test]
fn matched_carrier_controls() {
    for case in CASES {
        control(case);
    }
}

#[test]
fn equal_range_alias_retags_without_changing_carrier() {
    let source = "spec P {\n  type N = 0..2\n  type M = 0..2\n  state { ys: Set<N>, xs: Set<M> }\n  init { ys = Set {} xs = Set {} }\n  action snap() { xs = ys }\n  invariant P { xs == Set {} }\n}\n";
    // ys is frozen empty, but the induction step can consider nonempty ys.
    accepted(
        "alias-control",
        &source.replace("Set<M>", "Set<N>"),
        ["verified", "unknown_cti", "proved"],
    );
    accepted(
        "same-range-alias",
        source,
        ["verified", "unknown_cti", "proved"],
    );
}

#[test]
fn option_and_scalar_arguments_preserve_numeric_compatibility() {
    let option = Case {
        name: "b03_option_assignment",
        state: "ys: Option<N>, xs: Option<M>",
        init: "ys = some(2) xs = none",
        actions: "action snap() { xs = ys }",
        property: "xs != some(2)",
        actual: "",
        expected: "",
        loc: (0, 0),
    };
    let option_if = Case {
        name: "b14_option_if",
        state: "ys: Option<N>, xs: Option<M>, flag: Bool",
        init: "ys = some(2) xs = none flag = false",
        actions: "action flip() { flag = not flag }",
        property: "(if flag then ys else xs) != some(2)",
        actual: "",
        expected: "",
        loc: (0, 0),
    };
    let set_arg = Case {
        name: "b09_set_argument",
        state: "s: Set<N>, m: M",
        init: "s = Set {} m = 4",
        actions: "action addm() { s = s.add(m) }",
        property: "not s.contains(m)",
        actual: "",
        expected: "",
        loc: (0, 0),
    };
    let rel_arg = Case {
        name: "b10_relation_arguments",
        state: "r: relation N -> N, m: M",
        init: "r = Set {} m = 4",
        actions: "action addm() { r = r.add(m, m) }",
        property: "not r.contains(m, m)",
        actual: "",
        expected: "",
        loc: (0, 0),
    };
    for case in [option, option_if, set_arg, rel_arg] {
        accepted(case.name, &case.source(), ["violated"; 3]);
    }
}
