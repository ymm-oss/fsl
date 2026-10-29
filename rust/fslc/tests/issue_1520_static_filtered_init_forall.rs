// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Static filtered map coverage also applies to refinement's assigned roots.
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command};

const REFINE: &[&str] = &["refine", "model.fsl", "abs.fsl", "map.fsl", "--depth", "4"];
const FREE: &str = "spec RImpl { type K = 0..1 state { m: Map<K, Bool>, free: Bool } init { forall k: K where true { m[k] = true } } action idle() {} }";
const NO_FREE: &str = "spec RImpl { type K = 0..1 state { m: Map<K, Bool> } init { forall k: K where true { m[k] = true } } action idle() {} }";
const EMPTY: &str = "spec RImpl { type K = 0..1 state { m: Map<K, Bool>, free: Bool } init { forall k: K where false { m[k] = true } } action idle() {} }";
const SHADOW: &str = "spec RImpl { const outer = 1 type K = 0..1 state { m: Map<K, Bool>, free: Bool } init { forall outer in 0..0 { forall k: K where outer == 1 { m[k] = true } } } action idle() {} }";
const ENUM_SHADOW: &str = "spec RImpl { enum E { P, Q } type K = 0..1 state { m: Map<K, Bool>, free: Bool } init { forall P: E where P == Q { forall k: K where P != Q { m[k] = true } } } action idle() {} }";
const OUTER: &str = "spec RImpl { type K = 0..1 state { m: Map<K, Bool>, free: Bool } init { forall o in 0..1 { forall k: K where k == o { m[k] = true } } } action idle() {} }";
const FIXED: &str = "spec RImpl { type K = 0..1 const i = 0 state { x: Bool, free: Bool } init { forall i: K where i == 1 { forall j: K where i == 0 { x = true } } } action idle() {} invariant NoX { not x } }";

fn observe(id: &str, source: &str, args: &[&str], filter: Option<bool>, mapped: &str) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fslc-1520-{id}-{}", std::process::id()));
    std::fs::create_dir(&dir).expect("fresh fixture directory");
    let init = filter.is_some();
    let abstraction = format!("spec RAbs{{state{{a:Bool}}init{{a={init}}}action idle(){{}}}}");
    let mapping =
        format!("refinement R {{ impl RImpl abs RAbs map a = {mapped} action idle() -> idle() }}");
    for (name, text) in [
        ("model.fsl", source),
        ("abs.fsl", abstraction.as_str()),
        ("map.fsl", mapping.as_str()),
        ("empty.json", "[]"),
    ] {
        std::fs::write(dir.join(name), text).expect("write fixture");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env_remove("FSLC_CACHE")
        .output()
        .expect("run native fslc");
    assert!(output.stderr.is_empty(), "{output:?}");
    let actual: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    let exit = output.status.code().expect("CLI exit code");
    eprintln!("OBS cell={id} args={args:?} exit={exit} output={actual}");
    let Some(filter) = filter else {
        assert_eq!(exit, 2, "{actual:#}");
        return;
    };
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
        let message = actual["message"].as_str().expect("init diagnostic");
        assert!(
            message.starts_with("init does not assign state variable(s): m"),
            "{actual:#}"
        );
    }
}

#[test]
fn r1_free_root() {
    observe("r1_free_root", FREE, REFINE, Some(true), "m[0]");
}
#[test]
fn r1_no_free_root() {
    observe("r1_no_free_root", NO_FREE, REFINE, Some(true), "m[0]");
}
#[test]
fn r2_empty_free_root() {
    observe("r2_empty_free_root", EMPTY, REFINE, Some(false), "m[0]");
}

#[test]
fn outer_binder_domains_stay_unresolved() {
    for (id, source) in [("shadow", SHADOW), ("enum", ENUM_SHADOW), ("outer", OUTER)] {
        for args in [
            REFINE,
            &["verify", "model.fsl", "--engine", "explicit", "--no-cache"],
            &["replay", "model.fsl", "--trace", "empty.json"],
        ] {
            observe(&format!("{id}-{}", args[0]), source, args, None, "m[0]");
        }
    }
}

#[test]
fn outer_binder_fixed_target_stays_unresolved() {
    for args in [
        REFINE,
        &["verify", "model.fsl", "--engine", "explicit", "--no-cache"],
        &["replay", "model.fsl", "--trace", "empty.json"],
    ] {
        observe(&format!("fixed-{}", args[0]), FIXED, args, None, "x");
    }
}
