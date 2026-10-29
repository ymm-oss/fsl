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
                "explicit"
            ),
            ("mutate external [1/2] carrier: invalid", "explicit"),
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
