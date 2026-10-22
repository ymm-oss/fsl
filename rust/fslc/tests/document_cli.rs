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

// --- Requirements written with entities, events, rules and decision tables
// (fsl 5 plan §3 S10 document) ------------------------------------------------

const E2E_EN: &str = "rust/fslc/tests/fixtures/fsl5/e2e/fsl5_en.fsl";
const E2E_JA: &str = "rust/fslc/tests/fixtures/fsl5/e2e/fsl5_ja_full.fsl";
const REJECTION: &str = "rust/fslc/tests/fixtures/fsl5/rejection/fsl5_en.fsl";

/// `run` with the unstable gate set to `unstable` (or unset).
fn run_gated(args: &[&str], unstable: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(args)
        .current_dir(root())
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if let Some(features) = unstable {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, features);
    }
    command.output().expect("run native fslc")
}

fn gated_markdown(path: &str, lang: &str) -> String {
    let output = run_gated(
        &["document", "generate", path, "--lang", lang],
        Some("fsl5"),
    );
    assert!(
        output.status.success(),
        "document generate {path} --lang {lang}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 markdown")
}

/// The lines of every ```` ```mermaid ```` block.
fn mermaid_lines(markdown: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in markdown.lines() {
        match (inside, line) {
            (false, "```mermaid") => inside = true,
            (true, "```") => inside = false,
            (true, line) => lines.push(line.trim()),
            _ => {}
        }
    }
    lines
}

fn is_ascii_identifier(word: &str) -> bool {
    word.chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && word
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// Every state ID of the diagram: the `as ID` of a `state "…"` line and
/// both ends of a transition.
fn mermaid_ids<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut ids = Vec::new();
    for line in lines {
        if let Some((_, id)) = line
            .strip_prefix("state \"")
            .and_then(|rest| rest.rsplit_once(" as "))
        {
            ids.push(id);
        } else if let Some((source, rest)) = line.split_once(" --> ") {
            let target = rest.split_once(" : ").map_or(rest, |(target, _)| target);
            ids.extend([source, target].into_iter().filter(|id| *id != "[*]"));
        }
    }
    ids
}

/// S10 acceptance: the document has the diagram, table and glossary
/// sections; the Mermaid IDs are the ASCII canonical names; `--lang ja`
/// shows the glossary's aliases and the Japanese keywords.
#[test]
fn document_shows_lifecycle_diagram_tables_rules_and_glossary_in_japanese() {
    let markdown = gated_markdown(E2E_JA, "ja");
    for heading in [
        "## 要件の概観",
        "### ライフサイクル",
        "### 出来事",
        "### 決定表",
        "### 規則",
        "### 用語集",
    ] {
        assert!(
            markdown.lines().any(|line| line == heading),
            "missing {heading}:\n{markdown}"
        );
    }
    let lines = mermaid_lines(&markdown);
    assert_eq!(lines.first(), Some(&"stateDiagram-v2"), "{markdown}");
    let ids = mermaid_ids(&lines);
    assert!(!ids.is_empty());
    assert!(
        ids.iter().all(|id| is_ascii_identifier(id)),
        "non-ASCII Mermaid ID: {ids:?}"
    );
    // The stages display their aliases under their canonical IDs, and an edge
    // carries the event's actor and verb.
    for line in [
        "state \"提出済み\" as Submitted",
        "[*] --> Draft",
        "Submitted --> Rejected : 上長 が 却下する",
    ] {
        assert!(lines.contains(&line), "missing `{line}` in {lines:?}");
    }
    assert!(
        markdown.contains(
            "| `reject` | 上長 | 申請 | 却下する | 提出済み | 却下済み | — | REQ-2-3, REQ-3 |"
        ),
        "{markdown}"
    );
    assert!(markdown.contains("#### REQ-2-3: 提出された申請を誰が扱うか\n\n対象: 提出済み 申請"));
    assert!(markdown.contains(
        "| 2 | 高額申請 | 上長 が 承認する（`mgr_approve`）、上長 が 却下する（`reject`） |"
    ));
    assert!(markdown.contains("- 網羅性: 満たす（型の値域を列挙して確かめた）\n- 排他性: 満たす"));
    assert!(markdown.contains(
        "- **許可**（出来事 `submit`）: `許可 従業員 が 申請 を 提出する 条件 入力 申請金額 > 0`"
    ));
    assert!(markdown.contains("| `Claim.amount` | 申請金額 |"));
    // The event's phrase labels its action's claim heading.
    assert!(markdown.contains("#### 操作: 従業員 が 申請 を 提出する（`submit`）"));
}

/// `--lang en` shows the canonical names: no stage gets a label line, and the
/// same edge carries the canonical actor and verb.
#[test]
fn document_in_english_shows_canonical_names_for_a_japanese_source() {
    let markdown = gated_markdown(E2E_JA, "en");
    let lines = mermaid_lines(&markdown);
    assert!(
        !lines.iter().any(|line| line.starts_with("state ")),
        "{lines:?}"
    );
    assert!(lines.contains(&"Submitted --> Rejected : Manager rejects"));
    for heading in [
        "### Lifecycles",
        "### Events",
        "### Decision tables",
        "### Rules",
        "### Glossary",
    ] {
        assert!(
            markdown.lines().any(|line| line == heading),
            "missing {heading}"
        );
    }
    assert!(markdown.contains(
        "| `reject` | Manager | Claim | rejects | Submitted | Rejected | — | REQ-2-3, REQ-3 |"
    ));
    assert!(markdown.contains("- Exhaustive: yes (checked by enumerating the types' domains)"));
    // The English source declares no glossary, so it has no glossary table.
    let english = gated_markdown(E2E_EN, "en");
    assert!(english.contains("### Decision tables"));
    assert!(!english.lines().any(|line| line == "### Glossary"));
}

const MULTI_ENTITY_JA: &str = "rust/fslc/tests/fixtures/fsl5/multi_entity/fsl5_ja_full.fsl";
const RELATIONS: &str = "rust/fslc/tests/fixtures/fsl5/relations/fsl5_en.fsl";

/// The relations stage's declarations are in the overview: the relation,
/// the resource, the measures and the goal of the multi-entity example under
/// their aliases (with the keyword table's column words), and the same
/// lines under canonical names in English.
#[test]
fn document_shows_relations_resources_measures_and_goals() {
    let markdown = gated_markdown(MULTI_ENTITY_JA, "ja");
    for line in [
        "### 関係",
        "| エンティティ | 所属先 | 役名 | 契機 |",
        "| 注文 | 利用者 | 購入者 | `place` |",
        "### 資源",
        "| 在庫（`stock`） | 数量型 | `2` |",
        "### 指標",
        "| 支払件数（`paid_count`） | 注文 | 決済済み |",
        "| 返金件数（`refund_count`） | 注文 | 返金済み |",
        "### 目標",
        "- **返金できる**（`RefundedOne`）: 注文は返金まで進める — いずれかの 注文 が 返金済み に到達できる",
        "| `place` | 利用者 | 注文 | 注文する | 空 | 受注 | u（利用者）, 数量 | REQ-PLACE |",
    ] {
        assert!(
            markdown.lines().any(|candidate| candidate == line),
            "missing `{line}`:\n{markdown}"
        );
    }
    let ids = mermaid_ids(&mermaid_lines(&markdown));
    assert!(ids.contains(&"ReturnRequested"));
    assert!(ids.iter().all(|id| is_ascii_identifier(id)), "{ids:?}");

    let english = gated_markdown(MULTI_ENTITY_JA, "en");
    for line in [
        "| Order | User | buyer | `place` |",
        "| `stock` | Qty | `2` |",
        "| `paid_count` | Order | Settled |",
        "- **RefundedOne**: 注文は返金まで進める — some Order can reach Refunded",
    ] {
        assert!(
            english.lines().any(|candidate| candidate == line),
            "missing `{line}`:\n{english}"
        );
    }
}

/// A policy reads as its sentence: every instance in the source stage must
/// eventually reach one of the targets.
#[test]
fn document_shows_policies_in_either_language() {
    let english = gated_markdown(RELATIONS, "en");
    assert!(english.contains(
        "### Policies\n\n- **Resolved**: Every assigned ticket is eventually closed or escalated — \
         every Ticket in Assigned must eventually be Closed or Escalated"
    ));
    assert!(english.contains("| Ticket | Agent | assignee | `take` |"));
    let japanese = gated_markdown(RELATIONS, "ja");
    assert!(japanese.contains(
        "### 方針\n\n- **Resolved**: Every assigned ticket is eventually closed or escalated — \
         すべての Ticket は Assigned から いずれ必ず Closed または Escalated になる"
    ));
}

/// The projection reads the document under the gate's options, so the
/// forbidden traces of a document written with rules are claims (the
/// published-language reading rejected the whole document).
#[test]
fn document_claims_keep_the_forbidden_traces_of_a_gated_document() {
    let output = run_gated(&["document", "claims", REJECTION], Some("fsl5"));
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let claims = json_stdout(&output);
    let mut forbidden = claims["trace_cases"]
        .as_array()
        .expect("trace cases")
        .iter()
        .filter(|case| case["kind"] == "forbidden")
        .map(|case| case["id"].as_str().expect("id").to_owned())
        .collect::<Vec<_>>();
    forbidden.sort();
    assert_eq!(
        forbidden,
        [
            "FB-EXECUTE-BEFORE-APPROVAL",
            "FB-EXECUTE-WITH-DOWNGRADED-APPROVAL",
            "FB-EXECUTE-WITH-PUBLIC-ROLE",
            "FB-PUBLIC-APPROVE",
        ]
    );
}

/// R0: a requirements document in the published form renders the same bytes
/// with the gate on, without the new sections.
#[test]
fn a_published_form_document_renders_the_same_with_the_gate_on() {
    let original = "rust/fslc/tests/fixtures/fsl5/e2e/original.fsl";
    let args = ["document", "generate", original, "--lang", "ja"];
    let off = run_gated(&args, None);
    let on = run_gated(&args, Some("fsl5"));
    assert!(
        off.status.success(),
        "{}",
        String::from_utf8_lossy(&off.stdout)
    );
    assert_eq!(off.stdout, on.stdout);
    assert!(!String::from_utf8_lossy(&on.stdout).contains("## 要件の概観"));
}
