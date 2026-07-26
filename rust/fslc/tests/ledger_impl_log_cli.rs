// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for issue #499: `fslc ledger --impl-log` used to discard
//! every replay error (missing file, malformed JSON, wrong-spec trace,
//! schema-invalid trace) and still return `result: "generated"` at exit 0
//! with the implementation-log row silently missing. The ledger is an audit
//! artifact, so a silently-empty evidence chain is worse here than a crash
//! (AGENTS.md: "a confidently green false negative is more dangerous than a
//! crash"). Every `*_errors` test here fails if the fix is reverted; the
//! `*_still_generates` tests guard that legitimate replay evidence
//! (conformant and nonconformant, as opposed to a replay error) is not
//! collaterally broken by making the command fail-closed.
//!
//! `replay_trace.fsl`'s `partial(i: Id)` action is deliberately left
//! unguarded at `i == 0` -- `replay_trace_contract.rs` replays it there on
//! purpose to exercise the concrete Monitor's `partial_op` violation kind.
//! The same unguarded division makes `fslc verify` legitimately classify
//! this spec `result:"error"`/exit 2 (`kind:"semantics"`, "division by
//! zero"), which issue #592 now correctly propagates to `fslc ledger`'s own
//! exit code even though the ledger content still renders in full. That is
//! why the `*_still_generates` tests below assert exit 2, not exit 0.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root().join(format!(
        "rust/target/ledger-impl-log-cli-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native fslc")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

const SPEC: &str = "replay_trace.fsl";

fn ledger_with_impl_log(impl_log: &Path, output_md: &Path) -> Output {
    run(&[
        "ledger",
        fixture(SPEC).to_str().expect("spec path"),
        "--impl-log",
        impl_log.to_str().expect("impl-log path"),
        "-o",
        output_md.to_str().expect("output path"),
    ])
}

#[test]
fn ledger_impl_log_missing_file_errors() {
    let dir = scratch_dir("missing-file");
    let out = dir.join("ledger.md");
    let output = ledger_with_impl_log(Path::new("/no/such/impl-log.json"), &out);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    assert_eq!(value["kind"], "io");
    assert!(
        !out.exists(),
        "ledger must not be written on a replay error"
    );
}

#[test]
fn ledger_impl_log_malformed_json_errors() {
    let dir = scratch_dir("malformed-json");
    let log = dir.join("log.json");
    fs::write(&log, "not json").expect("write malformed log");
    let out = dir.join("ledger.md");
    let output = ledger_with_impl_log(&log, &out);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    assert!(
        !out.exists(),
        "ledger must not be written on a replay error"
    );
}

#[test]
fn ledger_impl_log_wrong_spec_errors() {
    let dir = scratch_dir("wrong-spec");
    let mut trace: Value = serde_json::from_str(
        &fs::read_to_string(fixture("replay_trace.valid.v1.json")).expect("read fixture"),
    )
    .expect("parse fixture JSON");
    trace["spec"] = Value::String("SomeOtherSpec".to_owned());
    let log = dir.join("log.json");
    fs::write(&log, trace.to_string()).expect("write wrong-spec log");
    let out = dir.join("ledger.md");
    let output = ledger_with_impl_log(&log, &out);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    assert!(
        value["message"]
            .as_str()
            .expect("error message")
            .contains("does not match checked spec"),
        "{value}"
    );
    assert!(
        !out.exists(),
        "ledger must not be written on a replay error"
    );
}

#[test]
fn ledger_impl_log_schema_invalid_trace_errors() {
    // `replay_trace.bad-tick.v1.json` declares tick 2 for its first event,
    // which the versioned replay-trace schema rejects (ticks must start at
    // 1 and increase by exactly one).
    let dir = scratch_dir("schema-invalid");
    let out = dir.join("ledger.md");
    let output = ledger_with_impl_log(&fixture("replay_trace.bad-tick.v1.json"), &out);

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output);
    assert_eq!(value["result"], "error");
    assert!(
        !out.exists(),
        "ledger must not be written on a replay error"
    );
}

#[test]
fn ledger_impl_log_conformant_trace_still_generates() {
    let dir = scratch_dir("conformant");
    let out = dir.join("ledger.md");
    let output = ledger_with_impl_log(&fixture("replay_trace.valid.v1.json"), &out);

    // Exit 2, not 0: `replay_trace.fsl` itself carries a genuine `verify`
    // error (see the module doc), and issue #592 makes `ledger` report that
    // rather than hide it -- but the ledger content still generates in full.
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let content = fs::read_to_string(&out).expect("ledger written");
    assert!(content.contains("実装ログ適合: 適合"), "{content}");
}

#[test]
fn ledger_impl_log_nonconformant_trace_still_generates() {
    // Nonconformance is legitimate replay evidence (the implementation log
    // disagrees with the spec), not a replay error, and must keep surfacing
    // in the ledger exactly as before this fix.
    let dir = scratch_dir("nonconformant");
    let out = dir.join("ledger.md");
    let output = ledger_with_impl_log(&fixture("replay_trace.state-mismatch.v1.json"), &out);

    // Same exit 2 as the conformant case above, for the same reason (the
    // spec's own `verify` baseline errors, independent of the impl-log
    // trace's conformance) -- the nonconformant row still renders.
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let content = fs::read_to_string(&out).expect("ledger written");
    assert!(content.contains("実装ログ適合"), "{content}");
    assert!(content.contains("非適合"), "{content}");
}
