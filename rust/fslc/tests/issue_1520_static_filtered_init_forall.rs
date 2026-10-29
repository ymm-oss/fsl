// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Static filtered map coverage also applies to refinement's assigned roots.
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command};

fn refinement(id: &str, free: &str, filter: bool) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fslc-1520-{id}-{}", std::process::id()));
    std::fs::create_dir(&dir).expect("fresh fixture directory");
    let source = format!(
        "spec RImpl {{ type K = 0..1 state {{ m: Map<K, Bool>{free} }} init {{ forall k: K where {filter} {{ m[k] = true }} }} action idle() {{}} }}"
    );
    let abstraction = "spec RAbs { state { a: Bool } init { a = true } action idle() {} }";
    let mapping = "refinement R { impl RImpl abs RAbs map a = m[0] action idle() -> idle() }";
    for (name, text) in [
        ("model.fsl", source.as_str()),
        ("abs.fsl", abstraction),
        ("map.fsl", mapping),
    ] {
        std::fs::write(dir.join(name), text).expect("write fixture");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["refine", "model.fsl", "abs.fsl", "map.fsl", "--depth", "4"])
        .current_dir(dir)
        .env_remove("FSLC_CACHE")
        .output()
        .expect("run native fslc");
    assert!(output.stderr.is_empty(), "{output:?}");
    let actual: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    let exit = output.status.code().expect("CLI exit code");
    eprintln!("OBS cell={id} consumer=refine exit={exit} output={actual}");
    let expected = if filter {
        json!([0, "refines", null])
    } else {
        json!([2, "error", "type"])
    };
    assert_eq!(
        json!([exit, actual["result"], actual["kind"]]),
        expected,
        "{actual:#}"
    );
    if !filter {
        assert!(
            actual["message"]
                .as_str()
                .expect("init diagnostic")
                .starts_with("init does not assign state variable(s): m"),
            "{actual:#}"
        );
    }
}

#[test]
fn r1_free_root() {
    refinement("r1_free_root", ", free: Bool", true);
}
#[test]
fn r1_no_free_root() {
    refinement("r1_no_free_root", "", true);
}
#[test]
fn r2_empty_free_root() {
    refinement("r2_empty_free_root", ", free: Bool", false);
}
