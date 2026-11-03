// SPDX-License-Identifier: Apache-2.0

//! Limited #1480 acceptance, not authorization for dynamic/struct admission changes.
//! Literal semantic oracles are independent of production domain classification.
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);
const ROUTES: [&str; 4] = ["check", "bmc", "induction", "explicit"];

struct Fixture(PathBuf);
impl Fixture {
    fn new(source: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "issue-1480-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).expect("fresh owned fixture");
        std::fs::write(dir.join("model.fsl"), source).expect("write source");
        eprintln!("SOURCE {}\n{source}", dir.display());
        Self(dir)
    }
    fn run(&self, route: &str) -> (i32, Value) {
        let args = if route == "check" {
            vec!["check", "model.fsl"]
        } else {
            vec![
                "verify",
                "model.fsl",
                "--engine",
                route,
                "--depth",
                "3",
                "--no-cache",
            ]
        };
        let out = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(&args)
            .current_dir(&self.0)
            .env("FSLC_CACHE", "off")
            .env_remove("FSLC_UNSTABLE")
            .output()
            .expect("native CLI");
        eprintln!(
            "RAW bin={} cwd={} argv={args:?} FSLC_CACHE=off FSLC_UNSTABLE=unset exit={:?}\nstdout={}\nstderr={}",
            env!("CARGO_BIN_EXE_fslc"),
            self.0.display(),
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty(), "unexpected CLI stderr");
        (
            out.status.code().expect("exit"),
            serde_json::from_slice(&out.stdout).expect("JSON packet"),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned fixture");
    }
}

fn spec(property: &str) -> String {
    format!("spec B {{ state {{ d: Bool = false }} action flip() {{ d = not d }} {property} }}")
}

fn verdict(
    failures: &mut Vec<String>,
    label: &str,
    route: &str,
    out: &(i32, Value),
    negative: bool,
) {
    let expected = if route == "check" {
        (0, "ok")
    } else if negative {
        (1, "violated")
    } else if route == "bmc" {
        (0, "verified")
    } else {
        (0, "proved")
    };
    if (out.0, out.1["result"].as_str()) != (expected.0, Some(expected.1)) {
        failures.push(format!(
            "{label}/{route}: expected {expected:?}, produced {out:?}"
        ));
    } else if negative && route != "check" && out.1["invariant"] != "P" {
        failures.push(format!("{label}/{route}: wrong rejecting property {out:?}"));
    }
}

fn verify_cases(cases: Vec<(String, String, bool)>) {
    let mut failures = Vec::new();
    for (label, source, negative) in cases {
        let f = Fixture::new(&source);
        let admitted = f.run("check");
        verdict(&mut failures, &label, "check", &admitted, false);
        // Admission failures are setup/contract failures, never semantic RED credit.
        for route in &ROUTES[1..] {
            verdict(&mut failures, &label, route, &f.run(route), negative);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn bool_expression_consumers() {
    let mut cases = Vec::new();
    for (expr, negative) in [
        ("forall b: Bool { b or not b }", false),
        ("forall b: Bool { b }", true),
        ("forall b: Bool { not b }", true),
        ("exists b: Bool { b }", false),
        ("exists b: Bool { not b }", false),
        ("exists b: Bool { false }", true),
        ("unique(b: Bool where b)", false),
        ("unique(b: Bool where not b)", false),
        ("unique(b: Bool)", true),
        ("exactlyOne(b: Bool where b)", false),
        ("exactlyOne(b: Bool where not b)", false),
        ("exactlyOne(b: Bool where false)", true),
        ("exactlyOne(b: Bool)", true),
    ] {
        cases.push((
            expr.into(),
            spec(&format!("invariant P {{ {expr} }}")),
            negative,
        ));
    }
    verify_cases(cases);
}

#[test]
fn bool_symbolic_aggregate_preservation() {
    verify_cases(
        [
            ("count(b: Bool) == 2", false),
            ("count(b: Bool) == 1", true),
            ("sum(b: Bool of if b then 1 else 2) == 3", false),
            ("sum(b: Bool of if b then 1 else 2) == 2", true),
        ]
        .map(|(e, bad)| (e.into(), spec(&format!("invariant P {{ {e} }}")), bad))
        .to_vec(),
    );
}

#[test]
fn finite_named_domains_and_qualified_name_refusal_preservation() {
    for declaration in ["enum Key { A, B }", "type Key = 0..1"] {
        let source = format!(
            "spec F {{ {declaration} state {{ d: Bool = false, m: Map<Key, Bool> }} init {{ forall k: Key {{ m[k] = false }} }} action flip() {{ forall k: Key {{ m[k] = not m[k] }} d = not d }} invariant P {{ forall k: Key {{ m[k] == d }} and count(k: Key) == 2 }} }}"
        );
        verify_cases(vec![
            (declaration.into(), source.clone(), false),
            (
                format!("{declaration}/rejecting"),
                source.replace("m[k] == d", "m[k] != d"),
                true,
            ),
        ]);
    }
    for ty in ["Missing.Bool", "Missing.Key"] {
        let f = Fixture::new(&spec(&format!(
            "invariant P {{ forall k: {ty} {{ true }} }}"
        )));
        for route in ROUTES {
            let out = f.run(route);
            assert_eq!(
                (out.0, out.1["result"].as_str(), out.1["kind"].as_str()),
                (2, Some("error"), Some("semantics"))
            );
            assert!(
                out.1["message"]
                    .as_str()
                    .unwrap()
                    .contains("qualified types must be lowered")
            );
        }
    }
}

#[test]
fn bool_init_and_action_statement_consumers() {
    let mut cases = Vec::new();
    for position in ["init", "action"] {
        for bad in ["none", "false", "true"] {
            let init = if position == "init" {
                "forall b: Bool { m[b] = b }"
            } else {
                "forall k in Set { false, true } { m[k] = k }"
            };
            let action = if position == "action" {
                "forall b: Bool { m[b] = not m[b] } d = not d"
            } else {
                "d = not d"
            };
            let expected_false = if position == "action" { "d" } else { "false" };
            let expected_true = if position == "action" {
                "not d"
            } else {
                "true"
            };
            let body = match bad {
                "false" => format!("m[false] != ({expected_false})"),
                "true" => format!("m[true] != ({expected_true})"),
                _ => format!("m[false] == ({expected_false}) and m[true] == ({expected_true})"),
            };
            cases.push((format!("{position}/{bad}"), format!(
                "spec S {{ state {{ d: Bool = false, m: Map<Bool, Bool> }} init {{ {init} }} action flip() {{ {action} }} invariant P {{ {body} }} }}"
            ), bad != "none"));
        }
    }
    verify_cases(cases);
}

#[test]
fn bool_liveness_wrapper_and_bounded_assurance() {
    let mut failures = Vec::new();
    for response in ["true", "false", "b", "not b"] {
        let f = Fixture::new(&spec(&format!(
            "leadsTo P {{ forall b: Bool {{ true ~> within 0 {response} }} }}"
        )));
        verdict(&mut failures, response, "check", &f.run("check"), false);
        for route in ["bmc", "induction"] {
            let out = f.run(route);
            verdict(&mut failures, response, route, &out, response != "true");
            if response == "true" && out.0 == 0 && route == "induction" {
                bounded_liveness(&out.1, 1, 0);
            }
        }
        let out = f.run("explicit");
        assert_eq!(
            (out.0, out.1["result"].as_str()),
            (2, Some("error")),
            "{out:?}"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn deadline(expr: &str) -> String {
    format!(
        "spec D {{\n  const K = -1 const POS = 1\n  state {{ d: Bool = false }}\n  action flip() {{ d = not d }}\n  leadsTo P {{ true ~> within {expr} true }}\n}}\n"
    )
}

fn bounded_liveness(out: &Value, instances: usize, within: i64) {
    // Measured public packet: top-level unbounded applies to invariants only.
    assert_eq!(out["completeness"], "unbounded");
    assert_eq!(
        out["note"],
        "invariants proved for all depths; leadsTo checked to depth 3 only"
    );
    let properties = out["leads_to"]
        .as_object()
        .expect("bounded liveness metadata");
    assert_eq!(properties.len(), instances);
    for property in properties.values() {
        assert_eq!(property, &json!({"checked_to_depth":3,"within":within}));
    }
}

#[test]
fn bounded_liveness_rejects_corrupted_property_promotions() {
    let f = Fixture::new(&deadline("0"));
    let out = f.run("induction");
    assert_eq!((out.0, out.1["result"].as_str()), (0, Some("proved")));
    bounded_liveness(&out.1, 1, 0); // Genuine unranked packet; top unbounded is valid.
    for (field, value) in [
        ("completeness", json!("unbounded")),
        ("proof", json!("ranking")),
        ("proved", json!(true)),
        ("rank_proof", json!(true)),
    ] {
        // Synthetic envelope corruption, NOT a verifier run or source fault.
        let mut corrupted = out.1.clone();
        corrupted["leads_to"]["P"][field] = value;
        let rejected = std::panic::catch_unwind(|| bounded_liveness(&corrupted, 1, 0)).is_err();
        eprintln!(
            "CORRUPTED_ENVELOPE field={field} expected_rejected=true produced_rejected={rejected} packet={corrupted}"
        );
        assert!(rejected, "missed per-property promotion: {field}");
    }
}

#[test]
fn negative_deadline_is_a_located_frontend_refusal_before_engine_dispatch() {
    let mut failures = Vec::new();
    for expr in ["-1", "K", "(0 - 1)"] {
        let f = Fixture::new(&deadline(expr));
        let packets = ROUTES.map(|r| f.run(r));
        for (route, out) in ROUTES.iter().zip(&packets) {
            if out.0 != 2
                || out.1["result"] != "error"
                || out.1["kind"] != "semantics"
                || out.1["loc"] != json!({"line":5,"column":3})
            {
                failures.push(format!(
                    "{expr}/{route}: expected located semantics refusal at 5:3; {out:?}"
                ));
            }
            for key in [
                "assurance",
                "completeness",
                "properties",
                "trace",
                "violation_kind",
            ] {
                if out.1.get(key).is_some() {
                    failures.push(format!("{expr}/{route}: error carries {key}: {out:?}"));
                }
            }
        }
        // No excluded fields. An early checked-model refusal has one full envelope.
        if !packets.windows(2).all(|p| p[0] == p[1]) {
            failures.push(format!("{expr}: full envelope mismatch: {packets:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn deadline_preservation_and_existing_constant_refusals() {
    let mut failures = Vec::new();
    for (expr, within) in [
        ("0", 0),
        ("1", 1),
        ("POS", 1),
        ("(0 - K)", 1),
        ("(1 - 1)", 0),
    ] {
        let f = Fixture::new(&deadline(expr));
        for route in ["check", "bmc", "induction"] {
            let out = f.run(route);
            verdict(&mut failures, expr, route, &out, false);
            if route == "induction" && out.0 == 0 {
                bounded_liveness(&out.1, 1, within);
            }
        }
        let out = f.run("explicit");
        assert_eq!(
            (out.0, out.1["result"].as_str()),
            (2, Some("error")),
            "explicit unsupported is not a positive oracle: {out:?}"
        );
    }
    for expr in ["d", "true", "(9223372036854775807 + 1)"] {
        let f = Fixture::new(&deadline(expr));
        let packets = ROUTES.map(|r| f.run(r));
        for out in &packets {
            assert_eq!(
                (out.0, out.1["result"].as_str(), out.1["kind"].as_str()),
                (2, Some("error"), Some("semantics")),
                "{expr}: {out:?}"
            );
        }
        // Existing lowering errors attach loc only on check; engine errors lack
        // an origin. Observe that live difference, compare engine packets whole.
        assert_eq!(
            packets[0].1.get("loc"),
            (expr == "d")
                .then(|| json!({"line":3,"column":11}))
                .as_ref(),
            "{expr}: {packets:?}"
        );
        assert!(packets[1..].iter().all(|p| p.1.get("loc").is_none()));
        assert!(
            packets[1..].windows(2).all(|p| p[0] == p[1]),
            "{expr}: {packets:?}"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn arithmetic_measurements_by_position_not_a_universal_admission_policy() {
    // Independent inclusive sequences; no /, %, conditionals or state bounds.
    for (range, values) in [
        ("0..1", vec![0, 1]),
        ("0..N", vec![0, 1]),
        ("-1..0", vec![-1, 0]),
        ("0..(1 + 0)", vec![0, 1]),
        ("0..(3 - 2)", vec![0, 1]),
        ("0..(1 * 1)", vec![0, 1]),
        ("1..0", vec![]),
        ("0..(9223372036854775807 + 1)", vec![]),
        ("false..true", vec![]),
    ] {
        for position in ["ordinary", "aggregate", "init", "action", "leadsTo"] {
            let n = values.len();
            let property = match position {
                "ordinary" => {
                    format!("invariant P {{ forall i in {range} {{ i >= -1 and i <= 1 }} }}")
                }
                "aggregate" => format!("invariant P {{ count(i in {range}) == {n} }}"),
                "leadsTo" => {
                    format!("leadsTo P {{ forall i in {range} {{ true ~> within 0 true }} }}")
                }
                _ => "invariant P { true }".into(),
            };
            let loop_text = format!("forall i in {range} {{ m[i] = true }}");
            let initial = if position == "init" {
                loop_text.clone()
            } else {
                "forall k: Key { m[k] = false }".into()
            };
            let action = if position == "action" {
                loop_text
            } else {
                String::new()
            };
            let key = if range == "-1..0" { "-1..0" } else { "0..1" };
            let source = format!(
                "spec A {{ const N = 1 type Key = {key} state {{ d: Bool = false, m: Map<Key, Bool> }} init {{ {initial} }} action flip() {{ {action} d = not d }} {property} }}"
            );
            let f = Fixture::new(&source);
            for route in ROUTES {
                let out = f.run(route);
                eprintln!(
                    "MEASURE range={range} literal_values={values:?} position={position} route={route} exit={} result={}",
                    out.0, out.1["result"]
                );
                assert!(
                    matches!(out.0, 0..=2),
                    "non-contract process failure {out:?}"
                );
                // Inventory only: successful evaluation must not contradict its literal oracle.
                assert!(
                    position != "aggregate" || out.0 != 1,
                    "wrong literal aggregate cardinality: {range}: {out:?}"
                );
            }
        }
    }
}
