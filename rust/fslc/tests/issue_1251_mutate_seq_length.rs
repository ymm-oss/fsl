// SPDX-License-Identifier: Apache-2.0

//! #1251 MF2: removing the init assignment of a `Seq` state leaves its length
//! unconstrained, so the BMC counterexample projection can fail with "model
//! sequence length is negative" (a likely projection gap of the #1283 kind).
//! That error is a finding about the mutant, not a solver failure: the mutant
//! must stay killed, never `error`.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const SPEC: &str = "spec Q {\n  type V = 0..2\n  state { q: Seq<V, 2>, n: Int }\n  \
     init { q = Seq {} n = 0 }\n  \
     action push(v: V) { requires q.size() < 2 q = q.push(v) n = 1 }\n  \
     action pop() { requires q.size() > 0 q = q.pop() }\n}\n";

#[test]
fn removing_a_seq_init_assignment_is_killed_not_an_oracle_error() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1251-seq-length-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    std::fs::write(dir.join("q.fsl"), SPEC).expect("write spec");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["mutate", "q.fsl", "--depth", "3", "--oracle-attribution"])
        .current_dir(&dir)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    let value: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let mutant = value["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .find(|mutant| {
            mutant["op"] == "assignment_remove"
                && mutant["loc"]["line"] == 4
                && mutant["loc"]["column"] == 10
        })
        .unwrap_or_else(|| panic!("q init removal mutant: {value}"));
    assert_eq!(mutant["status"], "killed", "{mutant}");
    assert!(mutant.get("error").is_none(), "{mutant}");
    assert!(
        value["summary"].get("errored").is_none(),
        "{}",
        value["summary"]
    );
}
