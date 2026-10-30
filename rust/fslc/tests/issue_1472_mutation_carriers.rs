// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

#[path = "support/mutate_progress.rs"]
mod mutation_progress;

fn run(directory: &Path, maps: &str) -> Value {
    std::fs::create_dir_all(directory).expect("create isolated fixtures");
    std::fs::write(
        directory.join("abs.fsl"),
        "spec Abs { type V = 0..2 state { seq: Seq<V, 2> } init { seq = Seq {} } \
         action push(v: V) { requires seq.size() < 2 seq = seq.push(v) } }",
    )
    .expect("write abstraction");
    std::fs::write(
        directory.join("req.fsl"),
        format!(
            "requirements Req {{ implements Abs from \"abs.fsl\" {{ {maps} }} \
             type V = 0..2 state {{ seq: Seq<V, 2> }} init {{ seq = Seq {{}} }} \
             action push(v: V) maps push(v) {{ requires v >= 0 requires v <= 1 \
             requires seq.size() < 2 seq = seq.push(v) }} }}"
        ),
    )
    .expect("write implementation");
    std::fs::write(
        directory.join("mutants.jsonl"),
        [
            json!({"id":"carrier", "target":"type V = 0..2", "replacement":"type V = 0..3"}),
            json!({"id":"control", "target":"requires v <= 1", "replacement":"requires v <= 0"}),
        ]
        .map(|entry| entry.to_string() + "\n")
        .concat(),
    )
    .expect("write external mutations");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .current_dir(directory)
        .env("FSLC_CACHE", "off")
        .args([
            "mutate",
            "req.fsl",
            "--depth",
            "3",
            "--max-mutants",
            "1",
            "--from",
            "mutants.jsonl",
            "--min-kill-rate",
            "0",
        ])
        .output()
        .expect("run mutation classification");
    assert_eq!(
        mutation_progress::progress(&output),
        [
            ("mutate baseline: proved", "explicit"),
            (
                "mutate [1/1] type_bound_lo_minus1 type V lo: invalid",
                "none"
            ),
            ("mutate external [1/2] carrier: invalid", "none"),
            ("mutate external [2/2] control: survived", "explicit"),
        ],
        "{output:?}"
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report = serde_json::from_slice(&output.stdout).expect("mutation JSON");
    mutation_progress::assert_report_progress(&output, &report);
    report
}

#[test]
fn same_named_domain_uses_type_invalid_handoff() {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("same-name-1599");
    let [auto, explicit] = ["maps auto", "map seq = seq"].map(|maps| run(&directory, maps));
    assert_eq!(auto["summary"], explicit["summary"]);
    assert_eq!(auto["gate"], explicit["gate"]);
    assert_eq!(auto["summary"]["invalid"], 2);
    let detail = &auto["mutants"][1]["invalid"];
    assert_eq!(detail, &explicit["mutants"][1]["invalid"]);
    assert_eq!(detail["kind"], "semantics");
}

#[test]
fn static_whole_map_mutants_are_invalid_not_oracle_failures() {
    for (label, maps) in [("auto", "maps auto"), ("explicit", "map seq = seq")] {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "collection-mutation-{}-{label}",
            std::process::id()
        ));
        let output = run(&directory, maps);
        let mutants = output["mutants"].as_array().expect("mutants");
        let invalid = mutants
            .iter()
            .find(|entry| entry["id"] == "carrier")
            .expect("external carrier mutation");
        assert_eq!(invalid["status"], "invalid", "{output}");
        assert_eq!(invalid["invalid"]["kind"], "semantics", "{output}");
        assert_eq!(
            invalid["invalid"]["message"],
            "invalid map expression for 'seq': expression of type Seq(Named(\"V\"), 2) is not assignable to Seq(Named(\"V\"), 2)",
            "{output}"
        );
        assert!(invalid.get("error").is_none(), "{output}");
        assert_eq!(output["summary"]["invalid"], 2, "{output}");
        assert_eq!(output["summary"]["survived"], 1, "{output}");
        assert_eq!(output["summary"]["kill_rate"], 0.0, "{output}");
        assert!(output["summary"].get("errored").is_none(), "{output}");
        assert_eq!(output["gate"]["judged"], 1, "{output}");
        assert_eq!(output["gate"]["violations"], json!([]), "{output}");
        assert_eq!(output["gate"]["passed"], true, "{output}");
    }
}

fn invalid_before_kill(requirement: bool) {
    for (label, maps) in [("auto", "maps auto"), ("explicit", "map seq = seq")] {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "collection-priority-{}-{requirement}-{label}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let init = if requirement { "" } else { "2" };
        let body = format!(
            "type V = 0..2 state {{ seq: Seq<V, 2> }} \
            init {{ seq = Seq {{ {init} }} }} action push(v: V) {{ \
            requires seq.size() < 2 seq = seq.push(v) }}"
        );
        std::fs::write(directory.join("abs.fsl"), format!("spec Abs {{ {body} }}")).unwrap();
        let trace = if requirement {
            "acceptance AC-1 \"top\" { push(2) expect seq.contains(2) }"
        } else {
            ""
        };
        std::fs::write(
            directory.join("req.fsl"),
            format!(
                "requirements Req {{ implements Abs from \"abs.fsl\" {{ {maps} }} {body} {trace} }}"
            ),
        )
        .unwrap();
        std::fs::write(directory.join("mutants.jsonl"), [
            json!({"id":"carrier", "target":"type V = 0..2", "replacement":"type V = 0..1"}),
            json!({"id":"control", "target":"requires seq.size() < 2", "replacement":"requires seq.size() <= 1"}),
        ].map(|entry| entry.to_string() + "\n").concat()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .current_dir(&directory)
            .env("FSLC_CACHE", "off")
            .args([
                "mutate",
                "req.fsl",
                "--depth",
                "3",
                "--max-mutants",
                "3",
                "--from",
                "mutants.jsonl",
                "--min-kill-rate",
                "0",
            ])
            .output()
            .unwrap();
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        let mutants = report["mutants"].as_array().expect("mutants");
        for id in ["type_bound_hi_minus1", "carrier"] {
            let mutant = mutants
                .iter()
                .find(|entry| entry["op"] == id || entry["id"] == id)
                .expect(id);
            assert_eq!(
                mutant["status"], "invalid",
                "{label}/{requirement}: {report}"
            );
            assert_eq!(mutant["invalid"]["kind"], "semantics", "{report}");
            assert_eq!(
                mutant["invalid"]["message"],
                "invalid map expression for 'seq': expression of type Seq(Named(\"V\"), 2) is not assignable to Seq(Named(\"V\"), 2)",
                "{report}"
            );
            assert!(
                mutant["killed_by"].is_null() && mutant.get("error").is_none(),
                "{report}"
            );
        }
        assert_eq!(report["summary"]["invalid"], 4, "{report}");
        assert_eq!(report["summary"]["kill_rate"], 0.0, "{report}");
        assert!(report["summary"].get("errored").is_none(), "{report}");
        assert_eq!(report["gate"]["judged"], 1, "{report}");
        assert_eq!(report["gate"]["violations"], json!([]), "{report}");
        assert_eq!(output.status.code(), Some(0), "{output:?}");
    }
}

#[test]
fn static_map_invalid_precedes_main_kill() {
    invalid_before_kill(false);
}

#[test]
fn static_map_invalid_precedes_requirement_kill() {
    invalid_before_kill(true);
}

#[test]
fn nonempty_alias_retags_preserve_set_seq_and_map_contents() {
    let bodies = [
        "state { x: Set<N>, y: Set<M> } init { x = Set { 2 } y = Set {} } action snap() { y = x } invariant P { not y.contains(2) }",
        "state { x: Seq<N, 2>, y: Seq<M, 2> } init { x = Seq { 2 } y = Seq {} } action snap() { y = x } invariant P { not y.contains(2) }",
        "state { x: Map<N, Bool>, y: Map<M, Bool> } init { forall k: N { x[k] = true } forall k: M { y[k] = false } } action snap() { y = x } invariant P { not y[2] }",
        "state { x: Map<N, N>, y: Map<N, M> } init { forall k: N { x[k] = 2 y[k] = 0 } } action snap() { y = x } invariant P { y[2] != 2 }",
    ];
    for (index, body) in bodies.into_iter().enumerate() {
        for (label, body) in [
            ("alias", body.to_owned()),
            ("control", body.replace("<M", "<N").replace(", M>", ", N>")),
        ] {
            let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
                "collection-content-{}-{index}-{label}.fsl",
                std::process::id()
            ));
            std::fs::write(
                &path,
                format!("spec P {{ type N = 0..2 type M = 0..2 {body} }}"),
            )
            .unwrap();
            for mode in ["check", "bmc", "induction", "explicit"] {
                let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
                command.env("FSLC_CACHE", "off");
                if mode == "check" {
                    command.arg("check").arg(&path);
                } else {
                    command.arg("verify").arg(&path).args([
                        "--engine",
                        mode,
                        "--depth",
                        "3",
                        "--no-cache",
                    ]);
                }
                let output = command.output().unwrap();
                let report: Value = serde_json::from_slice(&output.stdout).unwrap();
                let checking = mode == "check";
                assert_eq!(
                    output.status.code(),
                    Some(i32::from(!checking)),
                    "{index}/{label}/{mode}: {report}"
                );
                assert_eq!(
                    report["result"],
                    if checking { "ok" } else { "violated" },
                    "{index}/{label}/{mode}: {report}"
                );
                if !checking {
                    assert_eq!(report["invariant"], "P", "{report}");
                    assert_eq!(report["violation_kind"], "invariant", "{report}");
                }
            }
        }
    }
}
