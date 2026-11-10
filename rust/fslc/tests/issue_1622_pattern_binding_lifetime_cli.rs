// SPDX-License-Identifier: Apache-2.0

//! Source-level preservation and init truth. Collision-reservation bypasses
//! are exercised directly in fsl-verifier, never by relaxing CLI admission.
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../.codex/tmp/issue-1622-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("fixture directory");
        for (name, text) in files {
            std::fs::write(dir.join(name), text).expect("fixture");
        }
        Self(dir)
    }

    fn run(&self, args: &[&str], tag: &str) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .output()
            .expect("fslc");
        let exit = output.status.code().expect("exit");
        for (suffix, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            std::fs::write(self.0.join(format!("{tag}.{suffix}")), bytes).expect("raw evidence");
        }
        std::fs::write(self.0.join(format!("{tag}.exit")), exit.to_string())
            .expect("exit evidence");
        assert!(output.stderr.is_empty(), "{tag}: {output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).expect("JSON envelope");
        eprintln!(
            "OBS {tag} exit={exit} result={} kind={} path={}",
            report["result"],
            report["kind"],
            self.0.display()
        );
        (report, exit)
    }

    fn verify(&self, file: &str, engine: &str, tag: &str) -> (Value, i32) {
        self.run(
            &[
                "verify",
                file,
                "--engine",
                engine,
                "--depth",
                "4",
                "--no-cache",
            ],
            tag,
        )
    }
}

fn verdict(report: &Value, exit: i32, result: &str, code: i32) {
    assert_eq!(exit, code, "{report:#}");
    assert_eq!(report["result"], result, "{report:#}");
}

#[test]
fn fresh_progress_patterns_preserve_impossible_and_reached_responses() {
    let implementation = "spec Impl {type V=0..5 state {x:Int=0,p:Option<V> = some(3)} fair action go(){requires x==0 x=1}}";
    let mapping = "refinement R {impl Impl abs Abs map y=x map o=p action go()->step() preserve progress {respond L by go}}";
    for (name, q, reached) in [
        ("b18", "((o is some(w)) or false) and y==3", false),
        ("b15", "not (o is some(w)) or y==3", false),
        ("b17", "((o is some(w)) => false) or y==3", false),
        ("and-false", "((o is some(w)) and false) or y==3", false),
        ("b04-reverse", "not (o is some(w)) or y==1", true),
    ] {
        let abstraction = format!(
            "spec Abs {{type V=0..5 state {{y:Int=0,o:Option<V> = some(3)}} fair action step(){{requires y==0 y=1}} leadsTo L {{y==0 ~> ({q})}}}}"
        );
        let fixture = Fixture::new(&[
            ("impl.fsl", implementation),
            ("abs.fsl", &abstraction),
            ("map.fsl", mapping),
        ]);
        for repeat in 1..=2 {
            let (report, exit) = fixture.run(
                &["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "4"],
                &format!("{name}-refine-{repeat}"),
            );
            verdict(
                &report,
                exit,
                if reached {
                    "refines"
                } else {
                    "refinement_failed"
                },
                i32::from(!reached),
            );
            if !reached {
                assert_eq!(report["kind"], "progress_lost", "{report:#}");
                assert_eq!(report["violation_kind"], "leadsTo", "{report:#}");
                assert_eq!(
                    report["progress_failure"], "deadlock_or_stall_blocks_progress",
                    "{report:#}"
                );
            }
            for engine in ["bmc", "induction"] {
                for (file, holds) in [("impl.fsl", true), ("abs.fsl", reached)] {
                    let (report, exit) =
                        fixture.verify(file, engine, &format!("{name}-{file}-{engine}-{repeat}"));
                    verdict(
                        &report,
                        exit,
                        if !holds {
                            "violated"
                        } else if engine == "bmc" {
                            "verified"
                        } else {
                            "proved"
                        },
                        i32::from(!holds),
                    );
                    if !holds {
                        assert_eq!(report["violation_kind"], "leadsTo", "{report:#}");
                        assert_eq!(report["invariant"], "L", "{report:#}");
                    }
                }
            }
        }
    }
}

#[test]
fn compose_initializers_use_later_payload_not_prior_source_payload() {
    let a = "spec A {state {flag:Bool} init {flag=if (some(1) is some(v)) then true else false} action stay(){} invariant P {flag}}";
    for binder in ["v", "w"] {
        let b = format!(
            "spec B {{state {{flag:Bool}} init {{flag=(some(2) is some({binder})) and {binder}==2}} action stay(){{}} invariant P {{flag}}}}"
        );
        let fixture = Fixture::new(&[
            ("a.fsl", a),
            ("b.fsl", &b),
            (
                "s.fsl",
                "compose C {use A as a from \"a.fsl\" use B as b from \"b.fsl\"}",
            ),
        ]);
        for repeat in 1..=2 {
            for engine in ["bmc", "induction", "explicit"] {
                for file in ["a.fsl", "b.fsl", "s.fsl"] {
                    let (report, exit) =
                        fixture.verify(file, engine, &format!("{binder}-{file}-{engine}-{repeat}"));
                    verdict(
                        &report,
                        exit,
                        if engine == "bmc" {
                            "verified"
                        } else {
                            "proved"
                        },
                        0,
                    );
                }
            }
        }
    }
}

#[test]
fn composed_init_fragments_are_false_with_payload_two_compared_to_one() {
    let use_only = "compose C {use A as a from \"a.fsl\"}";
    for (name, a, s) in [
        (
            "blocks",
            "spec A {state {first:Bool,flag:Bool} init {if (some(1) is some(v)) {first=true} else {first=false}} init {flag=(some(2) is some(v)) and v == 1} action stay(){} invariant P {flag}}",
            use_only,
        ),
        (
            "inline",
            "spec A {state {first:Bool=if (some(1) is some(v)) then true else false,flag:Bool} init {flag=(some(2) is some(v)) and v == 1} action stay(){} invariant P {flag}}",
            use_only,
        ),
        (
            "own",
            "spec A {state {o:Option<Int>,n:Int} init {o=some(3) n=0} action go(){requires o is some(v) and v==3 n=1} invariant P {n<=1}}",
            "compose C {use A as a from \"a.fsl\" state {flag:Bool} init {if (some(1) is some(v)) {flag=(some(2) is some(v)) and v == 1} else {flag=false}} action g()=a.go(){} invariant F {flag}}",
        ),
    ] {
        for fresh in [false, true] {
            let rename = |text: &str| {
                if fresh {
                    text.replace("some(v)) and v == 1", "some(w)) and w == 1")
                } else {
                    text.to_owned()
                }
            };
            let fixture = Fixture::new(&[("a.fsl", &rename(a)), ("s.fsl", &rename(s))]);
            for repeat in 1..=2 {
                let tag = format!("{name}-fresh-{fresh}-{repeat}");
                let (report, exit) = fixture.run(&["check", "s.fsl"], &format!("{tag}-check"));
                // Preserve the existing composed-fragment admission gap; do not expand it.
                verdict(&report, exit, "ok", 0);
                for engine in ["bmc", "induction", "explicit"] {
                    let (report, exit) =
                        fixture.verify("s.fsl", engine, &format!("{tag}-{engine}"));
                    verdict(&report, exit, "violated", 1);
                    assert_eq!(report["violation_kind"], "invariant", "{report:#}");
                    let property = if name == "own" { "F" } else { "a.P" };
                    let state = if name == "own" { "flag" } else { "a.flag" };
                    assert_eq!(report["invariant"], property, "{report:#}");
                    assert_eq!(report["trace"][0]["state"][state], false, "{report:#}");
                }
            }
        }
    }
}

#[test]
fn quantified_diagnostic_keeps_matched_payload_and_full_failure_contract() {
    for compared in [1, 2] {
        let source = format!(
            "spec W {{ state {{ o: Option<Int> = some(1) }} action tick() {{}} invariant Q {{ forall k in 0..0 {{ o is some(v) and v == {compared} }} }} }}\n"
        );
        let fixture = Fixture::new(&[("w.fsl", &source)]);
        for engine in ["bmc", "explicit"] {
            for repeat in 1..=2 {
                let (report, exit) =
                    fixture.verify("w.fsl", engine, &format!("w-{compared}-{engine}-{repeat}"));
                if compared == 2 {
                    assert_w_failure(
                        &report,
                        exit,
                        "forall k in 0..0 { o is some(v) and v == 2 }",
                        &json!([{"k":0,"v":1}]),
                        &json!(1),
                        62,
                    );
                } else {
                    verdict(
                        &report,
                        exit,
                        if engine == "bmc" {
                            "verified"
                        } else {
                            "proved"
                        },
                        0,
                    );
                    assert!(report.get("violating_bindings").is_none());
                    assert!(report.get("blame").is_none());
                    assert_eq!(
                        report["completeness"],
                        if engine == "bmc" {
                            "bounded"
                        } else {
                            "unbounded"
                        }
                    );
                }
            }
        }
    }
}

fn assert_w_failure(
    report: &Value,
    exit: i32,
    text: &str,
    bindings: &Value,
    option: &Value,
    column: u32,
) {
    verdict(report, exit, "violated", 1);
    assert_eq!(&report["violating_bindings"], bindings);
    assert_eq!(
        report["blame"],
        json!({"conjuncts":[{
            "index":0,"text":text,"holds":false,"violating_bindings":bindings
        }]})
    );
    assert_eq!(report["loc"], json!({"line":1,"column":column}));
    assert_eq!(report["trace"], json!([{"step":0,"state":{"o":option}}]));
    assert_eq!(report["violation_kind"], "invariant");
    assert_eq!(report["invariant"], "Q");
    assert_eq!(report["violated_at_step"], 0);
    assert_eq!(report["checked_to_depth"], 0);
    assert_eq!(report["completeness"], "bounded");
    assert_eq!(report["last_action"], Value::Null);
}

#[test]
fn nested_conjunction_witness_packets_keep_reached_payloads() {
    for (name, frame, rendered) in [
        ("left", "($) and true", "$ and true"),
        ("right", "true and ($)", "true and ($)"),
        (
            "deeper",
            "((true and ($)) and true) and true",
            "true and ($) and true and true",
        ),
        (
            "deeper-right",
            "true and (true and (($) and true))",
            "true and (true and ($ and true))",
        ),
    ] {
        for (kind, option, matched, hi, compare, expected) in [
            ("exact", "some(1)", "o", 0, 2, json!([{"k":0,"v":1}])),
            (
                "varied",
                "some(1)",
                "(if k == 0 then o else some(2))",
                1,
                3,
                json!([{"k":0,"v":1},{"k":1,"v":2}]),
            ),
            (
                "absent",
                "none",
                "(if k == 0 then o else some(2))",
                1,
                3,
                json!([{"k":0},{"k":1,"v":2}]),
            ),
        ] {
            let matched = format!("{matched} is some(v) and v == {compare}");
            let body = frame.replace('$', &matched);
            let text = format!(
                "forall k in 0..{hi} {{ {} }}",
                rendered.replace('$', &matched)
            );
            let source = format!(
                "spec W {{ state {{ o: Option<Int> = {option} }} action tick() {{}} invariant Q {{ forall k in 0..{hi} {{ {body} }} }} }}\n"
            );
            let fixture = Fixture::new(&[("w.fsl", &source)]);
            for engine in ["bmc", "explicit"] {
                for repeat in 1..=2 {
                    let (report, exit) = fixture.verify(
                        "w.fsl",
                        engine,
                        &format!("nested-{name}-{kind}-{engine}-{repeat}"),
                    );
                    let (state, column) = if option == "none" {
                        (Value::Null, 59)
                    } else {
                        (json!(1), 62)
                    };
                    assert_w_failure(&report, exit, &text, &expected, &state, column);
                }
            }
        }
    }
}
