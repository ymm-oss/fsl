// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for `fslc document check`, issue #329: generated block
//! markers, frontmatter, and the structural drift checker. `fslc document
//! generate`'s own contract (digests, coverage, `--strict`) is covered by
//! `rust/fslc/tests/document_cli.rs`; this file exercises only drift
//! detection between a (possibly hand-edited) artifact and a fresh
//! re-projection + re-render of its spec.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

static NEXT_OUTPUT: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc")
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected JSON stdout: {error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn temp_output(suffix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!(
        "fslc-issue-329-{}-{nonce}-{}{suffix}",
        std::process::id(),
        NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed),
    ))
}

const CANCEL_SYSTEM: &str = "examples/pm/cancel_system.fsl";

/// Generates a fresh artifact into a unique temp file and returns its path.
fn generate(extra_args: &[&str]) -> PathBuf {
    let out = temp_output(".md");
    let mut args = vec!["document", "generate", CANCEL_SYSTEM];
    args.extend_from_slice(extra_args);
    args.push("-o");
    let out_str = out.to_str().expect("utf8 path").to_owned();
    args.push(&out_str);
    let output = run(&args);
    assert!(output.status.success(), "{:?}", output.stderr);
    out
}

fn check(spec: &str, artifact: &Path) -> Output {
    run(&[
        "document",
        "check",
        spec,
        artifact.to_str().expect("utf8 path"),
    ])
}

fn edit(artifact: &Path, from: &str, to: &str) -> PathBuf {
    let text = std::fs::read_to_string(artifact).expect("read artifact");
    assert!(
        text.contains(from),
        "fixture text {from:?} not found in artifact"
    );
    let edited = temp_output(".md");
    std::fs::write(&edited, text.replace(from, to)).expect("write edited artifact");
    edited
}

// --- Acceptance criteria -----------------------------------------------------

#[test]
fn a_freshly_generated_document_is_conformant() {
    let artifact = generate(&["--lang", "ja"]);
    let output = check(CANCEL_SYSTEM, &artifact);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(output.status.code(), Some(0));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "document_conformant");
    assert!(
        envelope["spec_digest"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:")
    );
    assert!(
        envelope["claim_set_digest"]
            .as_str()
            .unwrap_or("")
            .starts_with("sha256:")
    );
    let _ = std::fs::remove_file(&artifact);
}

#[test]
fn check_of_an_english_document_roundtrips_via_frontmatter_lang() {
    let artifact = generate(&["--lang", "en"]);
    let output = check(CANCEL_SYSTEM, &artifact);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json_stdout(&output)["result"], "document_conformant");
    let _ = std::fs::remove_file(&artifact);
}

#[test]
fn editing_the_background_slot_passes() {
    let artifact = generate(&["--lang", "ja"]);
    let edited = edit(
        &artifact,
        "（この節は自由に編集できる。規範的な効力はない。規範文はこの節の外の生成ブロックにのみ存在する。）",
        "このセクションにはプロジェクト固有の背景説明を記載する。",
    );
    let output = check(CANCEL_SYSTEM, &edited);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json_stdout(&output)["result"], "document_conformant");
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
}

#[test]
fn one_character_change_inside_a_claim_block_fails() {
    let artifact = generate(&["--lang", "ja"]);
    let edited = edit(&artifact, "CancelForm", "CancelFormX");
    let output = check(CANCEL_SYSTEM, &edited);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "document_drifted");
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "claim_changed"
                && reason["code"] == "FSL-DOC-BLOCK-DRIFT"
                && reason["detail"] == "artifact_edited")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
}

// --- Additional drift scenarios ----------------------------------------------

#[test]
fn deleting_a_claim_block_is_reported_as_missing() {
    let artifact = generate(&["--lang", "ja"]);
    let text = std::fs::read_to_string(&artifact).expect("read artifact");
    let start = text
        .find("<!-- fsl:claim begin id=\"action:tap_cancel#operation\"")
        .expect("tap_cancel claim marker present");
    let end = text[start..]
        .find("<!-- fsl:claim end -->")
        .map(|offset| start + offset + "<!-- fsl:claim end -->".len())
        .expect("closing marker present");
    let mut edited_text = text[..start].to_owned();
    edited_text.push_str(&text[end..]);
    let edited = temp_output(".md");
    std::fs::write(&edited, edited_text).expect("write edited artifact");

    let output = check(CANCEL_SYSTEM, &edited);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "claim_missing"
                && reason["code"] == "FSL-DOC-BLOCK-MISSING"
                && reason["claim"] == "action:tap_cancel#operation")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
}

#[test]
fn duplicating_a_claim_block_is_reported_as_duplicate() {
    let artifact = generate(&["--lang", "ja"]);
    let text = std::fs::read_to_string(&artifact).expect("read artifact");
    let start = text
        .find("<!-- fsl:claim begin id=\"action:tap_cancel#operation\"")
        .expect("tap_cancel claim marker present");
    let end = text[start..]
        .find("<!-- fsl:claim end -->")
        .map(|offset| start + offset + "<!-- fsl:claim end -->".len())
        .expect("closing marker present");
    let block = text[start..end].to_owned();
    let mut edited_text = text[..end].to_owned();
    edited_text.push_str("\n\n");
    edited_text.push_str(&block);
    edited_text.push_str(&text[end..]);
    let edited = temp_output(".md");
    std::fs::write(&edited, edited_text).expect("write edited artifact");

    let output = check(CANCEL_SYSTEM, &edited);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "claim_duplicate"
                && reason["code"] == "FSL-DOC-BLOCK-DUPLICATE"
                && reason["claim"] == "action:tap_cancel#operation")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
}

#[test]
fn an_old_renderer_version_is_reported_as_renderer_changed() {
    let artifact = generate(&["--lang", "ja"]);
    let edited = edit(
        &artifact,
        "renderer_version: 1.2.0",
        "renderer_version: 0.9.0",
    );
    let output = check(CANCEL_SYSTEM, &edited);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "renderer_changed"
                && reason["code"] == "FSL-DOC-RENDERER-CHANGED")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
}

#[test]
fn editing_generated_prose_outside_any_slot_fails() {
    let artifact = generate(&["--lang", "ja"]);
    let edited = edit(&artifact, "全体の意味規約", "全体の意味規約（改変）");
    let output = check(CANCEL_SYSTEM, &edited);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "edit_outside_slot"
                && reason["code"] == "FSL-DOC-EDIT-OUTSIDE-SLOT")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
}

#[test]
fn a_marker_like_line_inside_the_background_slot_fails() {
    let artifact = generate(&["--lang", "ja"]);
    let edited = edit(
        &artifact,
        "（この節は自由に編集できる。規範的な効力はない。規範文はこの節の外の生成ブロックにのみ存在する。）",
        "（この節は自由に編集できる。）\n<!-- fsl:claim begin id=\"forged\" digest=\"sha256:0\" -->",
    );
    let output = check(CANCEL_SYSTEM, &edited);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "marker_malformed"
                && reason["code"] == "FSL-DOC-MARKER-MALFORMED")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
}

#[test]
fn a_requirement_text_change_drifts_the_claim_set_digest() {
    let artifact = generate(&["--lang", "ja"]);
    let spec = std::fs::read_to_string(root().join(CANCEL_SYSTEM)).expect("read spec");
    assert!(spec.contains("show the retention offer exactly once per subscription"));
    let mutated_spec = spec.replace(
        "show the retention offer exactly once per subscription",
        "show the retention offer exactly once per subscription, guaranteed",
    );
    let spec_copy = temp_output(".fsl");
    std::fs::write(&spec_copy, mutated_spec).expect("write mutated spec");
    // The absolute temp path never resolves under `import`, but this fixture
    // has no imports; `check` re-projects under the frontmatter's recorded
    // `source` label regardless of which physical path content was read
    // from, so the rendered 出典 text is unaffected by using a temp copy.
    let output = check(spec_copy.to_str().expect("utf8 path"), &artifact);
    assert_eq!(output.status.code(), Some(1));
    let envelope = json_stdout(&output);
    let reasons = envelope["reasons"].as_array().expect("reasons array");
    assert!(
        reasons
            .iter()
            .any(|reason| reason["kind"] == "claim_set_digest_mismatch"
                && reason["code"] == "FSL-DOC-SPEC-DRIFT")
    );
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&spec_copy);
}

// --- Hard errors (exit 2) ----------------------------------------------------

#[test]
fn a_document_with_no_frontmatter_exits_two() {
    let broken = temp_output(".md");
    std::fs::write(&broken, "# not a generated document\n").expect("write broken artifact");
    let output = check(CANCEL_SYSTEM, &broken);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["code"], "FSL-DOC-SCHEMA-UNSUPPORTED");
    let _ = std::fs::remove_file(&broken);
}

#[test]
fn an_unsupported_document_schema_exits_two() {
    let artifact = generate(&["--lang", "ja"]);
    let edited = edit(
        &artifact,
        "fsl_document_schema: fsl-requirements-document-v1",
        "fsl_document_schema: fsl-requirements-document-v2",
    );
    let output = check(CANCEL_SYSTEM, &edited);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["code"], "FSL-DOC-SCHEMA-UNSUPPORTED");
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_file(&edited);
}

#[test]
fn a_missing_artifact_file_exits_two() {
    let output = check(CANCEL_SYSTEM, Path::new("/nonexistent/requirements.md"));
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["kind"], "io");
}

#[test]
fn a_missing_spec_argument_is_a_usage_error() {
    let output = run(&["document", "check"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_stdout(&output)["kind"], "usage");
}
