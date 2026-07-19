// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for `fslc document generate` / `fslc document claims`,
//! issue #327. The RCIR v1 projector (#325) and the ja/en controlled-language
//! renderer (#326) are exercised as library code in `rust/fsl-tools/tests/`;
//! this file covers only the CLI's own contract: flag parsing, the
//! `-o`/no-`-o` output convention shared with `ledger`/`html`/`testgen`,
//! `--strict`/`--strict-rendering` diagnostics, and the JSON envelope shape.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

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
        "fslc-issue-327-{}-{nonce}-{}{suffix}",
        std::process::id(),
        NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed),
    ))
}

const CANCEL_SYSTEM: &str = "examples/pm/cancel_system.fsl";
const UNATTRIBUTED_FIXTURE: &str = "rust/fsl-tools/tests/fixtures/document_claims_fixture.fsl";
const UNSUPPORTED_FIXTURE: &str = "rust/fsl-tools/tests/fixtures/document_kpi_fixture.fsl";

// --- Acceptance criterion 1: deterministic Markdown -------------------------

#[test]
fn generate_is_byte_identical_across_repeated_runs() {
    let first = run(&["document", "generate", CANCEL_SYSTEM, "--lang", "ja"]);
    let second = run(&["document", "generate", CANCEL_SYSTEM, "--lang", "ja"]);
    assert!(first.status.success(), "{:?}", first.stderr);
    assert_eq!(first.stdout, second.stdout);
    let markdown = String::from_utf8_lossy(&first.stdout);
    assert!(
        markdown.starts_with("---\nfsl_document_schema:"),
        "no -o: raw markdown (frontmatter first, issue #329) goes straight to stdout, \
         matching ledger/html/testgen"
    );
    assert!(markdown.contains("# 要件仕様書:"));
}

#[test]
fn generate_lang_en_renders_english_markdown() {
    let output = run(&["document", "generate", CANCEL_SYSTEM, "--lang", "en"]);
    assert!(output.status.success());
    let markdown = String::from_utf8_lossy(&output.stdout);
    assert!(markdown.contains("\nlang: en\n"));
    assert!(markdown.contains("# Requirements Specification:"));
}

// --- Acceptance criterion 3: -o writes the file and a JSON envelope --------

#[test]
fn generate_with_output_writes_file_and_envelope_with_digests_and_coverage() {
    let out = temp_output(".md");
    let output = run(&[
        "document",
        "generate",
        CANCEL_SYSTEM,
        "--lang",
        "ja",
        "-o",
        out.to_str().expect("utf8 path"),
    ]);
    assert!(output.status.success(), "{:?}", output.stderr);
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "generated");
    assert_eq!(envelope["kind"], "requirements_document");
    assert_eq!(envelope["output"], out.to_str().expect("utf8 path"));
    for digest_field in ["spec_digest", "claim_set_digest", "artifact_digest"] {
        let digest = envelope[digest_field].as_str().unwrap_or_else(|| {
            panic!("{digest_field} missing from envelope: {envelope}");
        });
        assert!(
            digest.starts_with("sha256:"),
            "{digest_field} = {digest} is not sha256-framed"
        );
    }
    assert_eq!(envelope["coverage"]["unattributed_targets"], 0);
    assert!(
        envelope["coverage"]["rendered_targets"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );
    assert!(
        envelope.get("content").is_none(),
        "-o must not echo content"
    );

    let written = std::fs::read_to_string(&out).expect("read generated markdown");
    assert!(written.starts_with("---\nfsl_document_schema:"));
    assert!(written.contains("# 要件仕様書:"));
    let _ = std::fs::remove_file(&out);
}

// --- Acceptance criterion 2: --strict fails on unattributed/unsupported ----

#[test]
fn generate_default_mode_succeeds_despite_an_unattributed_target() {
    let output = run(&["document", "generate", UNATTRIBUTED_FIXTURE]);
    assert!(output.status.success(), "{:?}", output.stderr);
}

#[test]
fn generate_strict_fails_on_an_unattributed_target() {
    let output = run(&["document", "generate", UNATTRIBUTED_FIXTURE, "--strict"]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["code"], "FSL-DOC-UNTAGGED-TARGET");
}

#[test]
fn generate_default_mode_succeeds_despite_unsupported_targets() {
    let output = run(&["document", "generate", UNSUPPORTED_FIXTURE]);
    assert!(output.status.success(), "{:?}", output.stderr);
}

#[test]
fn generate_strict_fails_on_unsupported_targets() {
    let output = run(&["document", "generate", UNSUPPORTED_FIXTURE, "--strict"]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["code"], "FSL-DOC-UNSUPPORTED-TARGET");
}

#[test]
fn generate_strict_rendering_fails_on_a_formula_fallback() {
    let output = run(&[
        "document",
        "generate",
        UNATTRIBUTED_FIXTURE,
        "--strict-rendering",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["code"], "FSL-DOC-FORMULA-FALLBACK");
}

// --- Usage errors -----------------------------------------------------------

#[test]
fn generate_rejects_an_unknown_view() {
    let output = run(&["document", "generate", CANCEL_SYSTEM, "--view", "business"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_stdout(&output)["kind"], "usage");
}

#[test]
fn generate_rejects_an_unknown_lang() {
    let output = run(&["document", "generate", CANCEL_SYSTEM, "--lang", "fr"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_stdout(&output)["kind"], "usage");
}

#[test]
fn unknown_document_subcommand_is_rejected() {
    let output = run(&["document", "frobnicate", CANCEL_SYSTEM]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_stdout(&output)["kind"], "usage");
}

// --- `fslc document claims` --------------------------------------------------

fn compiled_rcir_schema() -> jsonschema::Validator {
    let schema_text = std::fs::read_to_string(
        root().join("schemas/fslc/document/requirement-claims.v1.schema.json"),
    )
    .expect("read RCIR v1 schema");
    let schema_value: Value = serde_json::from_str(&schema_text).expect("schema is valid JSON");
    let kernel_text =
        std::fs::read_to_string(root().join("schemas/fslc/kernel/kernel.v2.schema.json"))
            .expect("read Public Kernel v2 schema");
    let kernel_value: Value = serde_json::from_str(&kernel_text).expect("kernel schema is JSON");
    let registry = jsonschema::Registry::new()
        .add(
            "https://fsl.dev/schemas/fslc/kernel/kernel.v2.schema.json",
            &kernel_value,
        )
        .expect("kernel schema resource")
        .prepare()
        .expect("schema registry");
    jsonschema::options()
        .with_registry(&registry)
        .build(&schema_value)
        .expect("schema compiles")
}

#[test]
fn claims_output_validates_against_the_rcir_v1_schema() {
    let output = run(&["document", "claims", CANCEL_SYSTEM]);
    assert!(output.status.success(), "{:?}", output.stderr);
    let claims = json_stdout(&output);
    let validator = compiled_rcir_schema();
    let errors: Vec<String> = validator
        .iter_errors(&claims)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "schema validation errors: {errors:?}");
}

#[test]
fn claims_and_generate_agree_on_spec_and_claim_set_digests() {
    let claims_output = run(&["document", "claims", CANCEL_SYSTEM]);
    let claims = json_stdout(&claims_output);
    let out = temp_output(".md");
    let generate_output = run(&[
        "document",
        "generate",
        CANCEL_SYSTEM,
        "-o",
        out.to_str().expect("utf8 path"),
    ]);
    let envelope = json_stdout(&generate_output);
    assert_eq!(claims["spec"]["spec_digest"], envelope["spec_digest"]);
    assert_eq!(
        claims["spec"]["claim_set_digest"],
        envelope["claim_set_digest"]
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn claims_with_output_writes_the_raw_rcir_json_and_a_small_envelope() {
    let out = temp_output(".claims.json");
    let output = run(&[
        "document",
        "claims",
        CANCEL_SYSTEM,
        "-o",
        out.to_str().expect("utf8 path"),
    ]);
    assert!(output.status.success(), "{:?}", output.stderr);
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "generated");
    assert_eq!(envelope["kind"], "requirement_claims");
    assert!(envelope.get("content").is_none());
    assert!(envelope.get("coverage").is_none());

    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(&out).expect("read written claims file"))
            .expect("written file is JSON");
    assert_eq!(written["result"], "requirement_claims");
    assert_eq!(written["spec"]["name"], "CancelSystemReq");
    let _ = std::fs::remove_file(&out);
}

// --- Dialect boundary (issue #334) -------------------------------------------
//
// The RCIR v1 projector itself (library-level, every non-`spec`/`requirements`
// dialect) is exercised in `rust/fsl-tools/tests/document.rs`'s
// `rejects_every_unsupported_dialect_fail_closed`; these two tests only pin
// the CLI's own envelope contract end-to-end for `generate`/`claims`.

#[test]
fn generate_rejects_an_unsupported_dialect_with_a_coded_error() {
    let output = run(&[
        "document",
        "generate",
        "examples/annotations/annotated_domain.fsl",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["kind"], "document");
    assert_eq!(envelope["code"], "FSL-DOC-DIALECT-UNSUPPORTED");
    assert_eq!(envelope["dialect"], "domain");
    assert_eq!(
        envelope["supported_dialects"],
        json!(["requirements", "spec"])
    );
    assert_eq!(
        envelope["message"],
        "document projection does not support dialect 'domain' in RCIR v1"
    );
    // No apparent/partial document ever reaches stdout alongside the error.
    assert!(envelope.get("content").is_none());
}

#[test]
fn claims_rejects_an_unsupported_dialect_with_a_coded_error() {
    let output = run(&[
        "document",
        "claims",
        "examples/annotations/annotated_dbsystem.fsl",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = json_stdout(&output);
    assert_eq!(envelope["result"], "error");
    assert_eq!(envelope["kind"], "document");
    assert_eq!(envelope["code"], "FSL-DOC-DIALECT-UNSUPPORTED");
    assert_eq!(envelope["dialect"], "dbsystem");
    assert_eq!(
        envelope["supported_dialects"],
        json!(["requirements", "spec"])
    );
    assert!(envelope.get("content").is_none());
    assert!(envelope.get("claims").is_none());
}
