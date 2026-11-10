// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

fn run(source: &str, engine: &str) -> (i32, Value) {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("literal-bridge-{}.fsl", std::process::id()));
    std::fs::write(&path, source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.env("FSLC_CACHE", "off");
    if engine == "check" {
        command.arg("check").arg(&path);
    } else {
        command
            .arg("verify")
            .arg(&path)
            .args(["--engine", engine, "--depth", "3", "--no-cache"]);
    }
    let output = command.output().unwrap();
    assert!(output.stderr.is_empty(), "{output:?}");
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

#[test]
fn finite_carrier_survives_literal_conditional_in_both_orders() {
    let mut observations = Vec::new();
    for (kind, actual, expected) in [
        ("Set", "Set<N>", "Set<M>"),
        ("Seq", "Seq<N, 2>", "Seq<M, 2>"),
        ("Seq", "Seq<N, 2>", "Seq<N, 3>"),
    ] {
        for literal_first in [true, false] {
            let literal = format!("{kind} {{ 0 }}");
            let conditional = if literal_first {
                format!("if c then {literal} else xs")
            } else {
                format!("if c then xs else {literal}")
            };
            let source = format!(
                "spec P {{\n type N = 0..2\n type M = 0..4\n state {{ xs: {actual}, ys: {expected}, c: Bool }}\n init {{ xs = {kind} {{ 2 }} ys = {kind} {{}} c = false }}\n action go() {{\n  let bridge = {conditional}\n  ys = bridge\n }}\n}}"
            );
            // Standalone conditional inference remains accepted, as does the
            // same-carrier copy; no literal materialization is requested.
            for control in [
                source.replace("  ys = bridge\n", ""),
                source.replace(expected, actual),
            ] {
                let (exit, report) = run(&control, "check");
                assert_eq!(
                    (exit, &report["result"]),
                    (0, &json!("ok")),
                    "{control}: {report}"
                );
            }
            let outputs =
                ["check", "bmc", "induction", "explicit"].map(|engine| run(&source, engine));
            observations.push((source, outputs));
        }
    }
    for (source, outputs) in observations {
        for (exit, report) in &outputs {
            assert_eq!(
                (*exit, &report["result"], &report["kind"]),
                (2, &json!("error"), &json!("semantics")),
                "{source}: {outputs:#?}"
            );
            assert_eq!(
                report["loc"],
                json!({"line":8,"column":3}),
                "{source}: {report}"
            );
            assert!(
                report["message"]
                    .as_str()
                    .unwrap()
                    .contains("is not assignable to"),
                "{report}"
            );
            assert_eq!(report, &outputs[0].1, "full frontend envelope: {source}");
        }
    }
}
