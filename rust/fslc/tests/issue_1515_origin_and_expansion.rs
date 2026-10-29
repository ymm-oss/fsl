// SPDX-License-Identifier: Apache-2.0
//! #1515: source-visible names and pre-expansion refusals keep their owner.
use serde_json::{Value, json};
use std::{path::Path, process::Command};
#[allow(dead_code)]
#[path = "support/compose_scratch.rs"]
mod support;
const ROUTES: [&str; 4] = ["check", "bmc", "induction", "explicit"];
const NEXT: &str = "fsl 5\nrequirements Probe {\n  const r = 2\n  actor System\n  entity Request {\n    lifecycle Idle -> Done\n  }\n  event finish = System finishes Request Idle -> Done\n}\nverify { instances Request = 1 }\n";
fn run(dir: &Path, source: &str, composed: bool) -> [(i32, Value); 4] {
    let root = "compose System {\n  use Shadow as part from \"component.fsl\"\n}\n";
    std::fs::write(dir.join("case.fsl"), if composed { root } else { source }).unwrap();
    std::fs::write(dir.join("component.fsl"), source).unwrap();
    ROUTES.map(|route| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fslc"));
        cmd.args([
            if route == "check" { "check" } else { "verify" },
            "case.fsl",
        ]);
        if route != "check" {
            cmd.args(["--depth", "3", "--no-cache", "--engine", route]);
        }
        cmd.current_dir(dir)
            .env("FSLC_CACHE", "off")
            .env_remove("FSLC_UNSTABLE");
        if source.starts_with("fsl 5\n") {
            cmd.env("FSLC_UNSTABLE", "fsl5");
        }
        let output = cmd.output().unwrap();
        assert!(output.stderr.is_empty(), "{output:?}");
        (
            output.status.code().unwrap(),
            serde_json::from_slice(&output.stdout).unwrap(),
        )
    })
}
fn refused(out: &[(i32, Value); 4], name: &str, line: u64, column: u64) -> bool {
    let (exit, report) = &out[0];
    *exit == 2
        && report["result"] == "error"
        && report["kind"] == "semantics"
        && report["loc"] == json!({"line": line, "column": column})
        && report["message"]
            .as_str()
            .is_some_and(|m| m.contains("shadows a constant") && m.contains(&format!("'{name}'")))
        && out.iter().all(|value| value == &out[0])
}
fn accepted(out: &[(i32, Value); 4]) -> bool {
    out.iter()
        .zip(["ok", "verified", "proved", "proved"])
        .all(|((exit, report), result)| *exit == 0 && report["result"] == result)
}
#[test]
fn visible_implicit_names_pin_unwritten_owner_locations() {
    let dir = support::scratch("issue_1515_visible", &[]);
    for (name, line, source) in [
        (
            "c",
            3,
            "requirements InitOnly {\n  const c = 2\n  process Case {\n    stages Idle, Done\n    initial Idle\n  }\n  action idle() { }\n  invariant P { true }\n}\nverify { instances Case = 1 }\n",
        ),
        (
            "c",
            4,
            "requirements Shadow {\n  const c = 2\n  number Amount\n  process Case with value: Amount = 0 {\n    stages Idle, Done\n    initial Idle\n    transition act Idle -> Done by System with amount: Amount set value = amount\n  }\n  invariant P { true }\n}\nverify { instances Case = 1 values Amount = 0..1 }\n",
        ),
        ("r", 5, NEXT),
        (
            "a",
            10,
            "fsl 5\nrequirements RelationProbe {\n  const a = 2\n  actor Agent\n  entity Agent\n  entity Ticket {\n    lifecycle Open -> Assigned\n  }\n  Ticket belongs to Agent as assignee via take\n  event take = Agent takes Ticket Open -> Assigned\n}\nverify { instances Ticket = 1 instances Agent = 1 }\n",
        ),
    ] {
        let out = run(&dir, source, false);
        assert!(refused(&out, name, line, 3), "{source}\n{out:#?}");
        let control = source.replace(&format!("const {name} ="), "const LIMIT =");
        assert!(accepted(&run(&dir, &control, false)), "{control}");
    }
    let bound = "requirements BoundUse {\n  const c = 2\n  process Case {\n    stages Idle, Done\n    initial Idle\n    transition act Idle -> Done by System when forall z in 0..c { z <= 1 }\n  }\n  action idle() { }\n  invariant P { forall k: Case { stage(k) == Idle } }\n}\nverify { instances Case = 2 }\n";
    assert!(refused(&run(&dir, bound, false), "c", 3, 3));
    let nested = "spec RImpl {\n  type K = 0..1\n  const i = 1\n  state { m: Map<K, Bool>, free: Bool }\n  init { forall i in 0..0 { forall j in 0..i { m[j] = true } } }\n  action idle() { }\n  invariant NoM1 { not m[1] }\n}\n";
    let out = run(&dir, nested, false);
    assert!(refused(&out, "i", 5, 10), "{out:#?}");
    assert_eq!(
        out[0].1["message"],
        "invalid init statement: local binding 'i' shadows a constant of the same name; rename the local binding at 5:10"
    );
    assert_eq!(
        run(&dir, &nested.replace("const i =", "const c ="), false)[0].0,
        0
    );
}
fn spec(definition: &str, expression: &str) -> String {
    format!(
        "spec Shadow {{\n  const N = 2\n  {definition}\n  state {{ y: Int = 0 }}\n  action tick() {{ y = y }}\n  invariant P {{ {expression} }}\n}}\n"
    )
}
fn arguments(kind: &str) {
    let dir = support::scratch("issue_1515_arguments", &[]);
    let mut failures = Vec::new();
    for expression in [
        "forall N in 0..1 { N <= 1 }",
        "exists N in 0..1 { N == 1 }",
        "count(N in 0..1) == 2",
        "sum(N in 0..1 of N) == 1",
        "unique(N in 0..1 where N == 0)",
        "exactlyOne(N in 0..1 where N == 0)",
    ] {
        for composed in [false, true] {
            let source = |body: &str| {
                let definition = format!(
                    "def pass(p: Bool) = p def ignore(p: Bool) = true def target(p: Bool) = {body}"
                );
                let source = spec(&definition, &format!("target({expression})"));
                if kind == "control" {
                    source.replace("const N =", "const LIMIT =")
                } else {
                    source
                }
            };
            let reference = run(&dir, &source("p"), composed);
            for body in ["p", "p and p", "pass(p)", "true", "ignore(p)"] {
                if (kind == "retained" && matches!(body, "true" | "ignore(p)"))
                    || (kind == "erased" && !matches!(body, "true" | "ignore(p)"))
                {
                    continue;
                }
                let out = run(&dir, &source(body), composed);
                let valid = if kind == "control" {
                    accepted(&out)
                } else {
                    refused(&out, "N", if composed { 2 } else { 6 }, 3)
                        && out == reference
                        && (!composed
                            || out[0].1["message"]
                                .as_str()
                                .unwrap()
                                .contains("component.fsl:6:3"))
                };
                if !valid {
                    failures.push(format!("{kind} body={body} composed={composed} {expression}: expected same located refusal or successful control; produced {out:#?}; reference {reference:#?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn retained_arguments_keep_owner_and_message() {
    arguments("retained");
}
#[test]
fn erased_arguments_keep_owner_and_message() {
    arguments("erased");
}
#[test]
fn renamed_argument_controls() {
    arguments("control");
}
#[test]
fn unused_definition_body_and_parameter_still_reject() {
    let dir = support::scratch("issue_1515_definitions", &[]);
    for definition in [
        "def unused(p: Bool) = forall N in 0..1 { N <= 1 }",
        "def unused(N: Bool) = true",
    ] {
        for composed in [false, true] {
            let source = spec(definition, "true");
            let out = run(&dir, &source, composed);
            assert!(
                refused(&out, "N", if composed { 2 } else { 3 }, 3),
                "{definition} {out:#?}"
            );
            assert!(accepted(&run(
                &dir,
                &source.replace("const N =", "const LIMIT ="),
                composed
            )));
        }
    }
}
#[test]
fn hidden_kpi_name_preserves_values_and_verdicts() {
    let dir = support::scratch("issue_1515_kpi", &[]);
    let source = NEXT.replace("const r =", "const c =").replace(
        "}\nverify",
        "  kpi waiting = count Request in Idle\n}\nverify",
    );
    let control = source.replace("const c =", "const LIMIT =");
    let options = fsl_syntax::ParseOptions::with_unstable(
        fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
    );
    for text in [&control, &source] {
        let out = run(&dir, text, false);
        assert!(
            accepted(&out),
            "hidden projection binder must not reject: {out:#?}"
        );
        let kernel = fsl_core::parse_kernel_source_with_options(
            text,
            &fsl_core::FsResolver::new(&dir),
            &options,
        )
        .unwrap();
        let model = fsl_core::build_model(kernel).unwrap();
        assert_eq!(model.projections[0].name, "waiting");
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).unwrap();
        for expected in [1, 0] {
            let value = fsl_runtime::eval(
                &model.projections[0].expr,
                &monitor.state,
                &mut fsl_runtime::Bindings::new(),
                &model,
                None,
            )
            .unwrap();
            assert_eq!(value, fsl_core::FslValue::Int(expected));
            if expected == 1 {
                let step = monitor.step(&monitor.enabled().unwrap()[0]).unwrap();
                assert!(step.violation.is_none());
            }
        }
    }
}
