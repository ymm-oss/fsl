// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::claim::{
    AgreementEdges, Calibration, EvidenceRef, EvidenceState, ObservationEvidence, ObservationKind,
    ObserverEvidence, ScopeEvidence, TriangulatedClaim,
};
use crate::matrix_claim::Citation;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

const fn executable(path: &'static str, anchor: &'static str) -> EvidenceRef {
    EvidenceRef {
        by: Citation { path, anchor },
        state: EvidenceState::Executable,
    }
}

const REJECTING: &[EvidenceRef] = &[
    executable(
        "rust/fslc/tests/triangulated/p3_dialect_dispatch.rs",
        "fn legacy_raw_prefix_and_corrupt_unknown_span_cut_p3_edges()",
    ),
    executable(
        "rust/fsl-syntax/src/dispatch.rs",
        "fn registry_rejects_duplicate_keywords()",
    ),
];

pub fn claims() -> Vec<TriangulatedClaim> {
    vec![TriangulatedClaim {
        id: "p3.token_dialect_dispatch",
        contract: Citation {
            path: "docs/DESIGN-triangulated-assurance.md",
            anchor: "## P3 — token-based dialect dispatch",
        },
        common_observation: ObservationEvidence {
            observed_by: executable(
                "rust/fslc/tests/triangulated/p3_dialect_dispatch.rs",
                "fn raw_sources_agree_across_model_manifest_library_and_cli()",
            ),
            kind: ObservationKind::RawSource,
            fields: &["source_bytes", "source_revision"],
        },
        model_observer: ObserverEvidence {
            observed_by: executable("examples/self/dialect_dispatch.fsl", "spec DialectDispatch"),
            semantic_owner: "P3 finite-state self-spec",
            semantic_lineage: &["significant-token state order", "native replay semantics"],
        },
        independent_observer: ObserverEvidence {
            observed_by: executable(
                "rust/fslc/tests/fixtures/triangulated_dialect_dispatch.json",
                "\"bom_trivia_annotation_domain\"",
            ),
            semantic_owner: "P3 hand-written fixture manifest",
            semantic_lineage: &["manifest expected keyword/span", "manual prefix scanner"],
        },
        edges: AgreementEdges {
            model_world: executable(
                "rust/fslc/tests/triangulated/p3_dialect_dispatch.rs",
                "fn raw_sources_agree_across_model_manifest_library_and_cli()",
            ),
            oracle_world: executable(
                "rust/fsl-lsp/tests/triangulated_dialect_dispatch.rs",
                "fn lsp_consumes_the_same_raw_dispatch_manifest()",
            ),
            model_oracle: executable(
                "rust/fslc/tests/triangulated/p3_dialect_dispatch.rs",
                "fn raw_sources_agree_across_model_manifest_library_and_cli()",
            ),
        },
        calibration: Calibration {
            accepting: executable(
                "rust/fsl-syntax/src/dispatch.rs",
                "fn every_registered_dialect_uses_the_significant_keyword_rule()",
            ),
            rejecting: REJECTING,
            common_mode: None,
        },
        scope: ScopeEvidence {
            declared_by: Citation {
                path: "docs/DESIGN-triangulated-assurance.md",
                anchor: "## P3 — token-based dialect dispatch",
            },
            commands: &[
                "check",
                "library parse_document",
                "LSP DocumentIndex::build",
            ],
            feature: "BOM/trivia/annotation significant-token dialect dispatch",
            domain: "registered dialect keywords and unknown diagnostic",
            backend: "native lexer/parser with independent manifest oracle",
            platform: "native Rust product gate platforms",
            corpus_revision: "triangulated_dialect_dispatch.json plus DIALECT_KEYWORDS",
        },
    }]
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn manifest() -> Vec<Value> {
    serde_json::from_str(
        &std::fs::read_to_string(
            repository_root().join("rust/fslc/tests/fixtures/triangulated_dialect_dispatch.json"),
        )
        .expect("read P3 manifest"),
    )
    .expect("parse P3 manifest")
}

fn run(arguments: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc")
}

fn write_source(id: &str, source: &str) -> PathBuf {
    let path = repository_root().join("rust/target").join(format!(
        "triangulated-dispatch-{id}-{}-{}.fsl",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, source.as_bytes()).expect("write P3 source");
    path
}

fn model_trace(source: &str, accepted: bool) -> Vec<Value> {
    let mut trace = Vec::new();
    if source.starts_with('\u{feff}') {
        trace.push(json!({"action":"consume_bom"}));
    }
    if source.contains("//") {
        trace.push(json!({"action":"consume_trivia"}));
    }
    if source.contains('@') {
        trace.push(json!({"action":"consume_annotation"}));
    }
    trace.push(json!({"action":"declaration_keyword"}));
    trace.push(json!({"action":if accepted { "dispatch" } else { "reject_unknown" }}));
    trace
}

fn replay_model(id: &str, trace: &[Value]) {
    let path = repository_root().join("rust/target").join(format!(
        "triangulated-dispatch-trace-{id}-{}-{}.json",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(
        &path,
        serde_json::to_vec(trace).expect("serialize P3 trace"),
    )
    .expect("write P3 trace");
    let output = run(&[
        "replay".to_owned(),
        "examples/self/dialect_dispatch.fsl".to_owned(),
        "--trace".to_owned(),
        path.to_string_lossy().into_owned(),
    ]);
    std::fs::remove_file(path).expect("remove P3 trace");
    let value: Value = serde_json::from_slice(&output.stdout).expect("P3 replay JSON");
    assert_eq!(output.status.code(), Some(0), "{id}: {value:#}");
    assert_eq!(value["result"], "conformant", "{id}: {value:#}");
}

fn manual_significant_identifier(source: &str) -> Option<(String, usize, usize)> {
    let bytes = source.as_bytes();
    let mut offset = usize::from(source.starts_with('\u{feff}')) * '\u{feff}'.len_utf8();
    loop {
        while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
            offset += 1;
        }
        if bytes.get(offset..offset + 2) == Some(b"//") {
            offset += 2;
            while bytes.get(offset).is_some_and(|byte| *byte != b'\n') {
                offset += 1;
            }
            continue;
        }
        if bytes.get(offset) == Some(&b'@') {
            let mut depth = 0_i32;
            while let Some(byte) = bytes.get(offset) {
                offset += 1;
                if *byte == b'(' {
                    depth += 1;
                } else if *byte == b')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            continue;
        }
        break;
    }
    let start = offset;
    while bytes
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        offset += 1;
    }
    (offset > start).then(|| {
        let prefix = &source[..start];
        let line = prefix
            .chars()
            .filter(|character| *character == '\n')
            .count()
            + 1;
        let column = prefix.rsplit_once('\n').map_or_else(
            || prefix.chars().count() + 1,
            |(_, tail)| tail.chars().count() + 1,
        );
        (source[start..offset].to_owned(), line, column)
    })
}

fn check_production_edges(
    case: &Value,
    expected_keyword: &str,
    expected_span: (usize, usize),
) -> Result<(), String> {
    let id = case["id"].as_str().ok_or("case id")?;
    let source = case["source"].as_str().ok_or("case source")?;
    let accepted = case["accepted"].as_bool().ok_or("accepted flag")?;
    let library = fsl_syntax::dialect_keyword(source);

    let path = write_source(id, source);
    let output = run(&["check".to_owned(), path.to_string_lossy().into_owned()]);
    std::fs::remove_file(path).map_err(|error| format!("remove P3 source: {error}"))?;
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("{id}: P3 CLI JSON: {error}"))?;

    if accepted {
        let keyword = library.map_err(|error| format!("{id}: library dispatch: {error}"))?;
        if keyword != expected_keyword {
            return Err(format!(
                "{id}: oracle↔library keyword mismatch: expected={expected_keyword:?} actual={keyword:?}"
            ));
        }
        if output.status.code() != Some(0) || value["result"] != "ok" {
            return Err(format!("{id}: oracle↔CLI acceptance mismatch: {value:#}"));
        }
    } else {
        let error = library.map_or_else(Ok, |keyword| {
            Err(format!("{id}: unknown source dispatched as {keyword:?}"))
        })?;
        let expected_code = case["error_code"].as_str().ok_or("error code")?;
        if error.code() != expected_code
            || (
                error.span.start.line as usize,
                error.span.start.column as usize,
            ) != expected_span
        {
            return Err(format!(
                "{id}: oracle↔library diagnostic mismatch: expected={expected_code}@{expected_span:?} actual={}@{:?}",
                error.code(),
                (error.span.start.line, error.span.start.column)
            ));
        }
        if output.status.code() != Some(2)
            || value["diagnostic_code"] != expected_code
            || value["loc"]["line"].as_u64() != Some(expected_span.0 as u64)
            || value["loc"]["column"].as_u64() != Some(expected_span.1 as u64)
            || !error.to_string().contains(expected_keyword)
        {
            return Err(format!("{id}: oracle↔CLI diagnostic mismatch: {value:#}"));
        }
    }
    Ok(())
}

#[test]
fn raw_sources_agree_across_model_manifest_library_and_cli() {
    for case in manifest() {
        let id = case["id"].as_str().expect("case id");
        let source = case["source"].as_str().expect("case source");
        let expected = case["keyword"].as_str().expect("expected keyword");
        let accepted = case["accepted"].as_bool().expect("accepted flag");
        let manual = manual_significant_identifier(source).expect("manual oracle keyword");
        assert_eq!(manual.0, expected, "{id}: independent manifest mismatch");
        check_production_edges(&case, &manual.0, (manual.1, manual.2))
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        replay_model(id, &model_trace(source, accepted));
    }
}

#[test]
fn legacy_raw_prefix_and_corrupt_unknown_span_cut_p3_edges() {
    let annotated = manifest()
        .into_iter()
        .find(|case| case["id"] == "bom_trivia_annotation_domain")
        .expect("annotated P3 case");
    let source = annotated["source"].as_str().expect("annotated source");
    let legacy_prefix = source
        .trim_start_matches(['\u{feff}', ' ', '\n', '\r', '\t'])
        .split_ascii_whitespace()
        .next()
        .expect("legacy prefix");
    assert!(
        check_production_edges(&annotated, legacy_prefix, (1, 1)).is_err(),
        "legacy raw-prefix oracle must cut the accepted keyword edge"
    );

    let unknown = manifest()
        .into_iter()
        .find(|case| case["id"] == "unknown_after_trivia")
        .expect("unknown P3 case");
    let (_, line, column) =
        manual_significant_identifier(unknown["source"].as_str().expect("unknown source"))
            .expect("unknown identifier");
    assert!(
        check_production_edges(
            &unknown,
            unknown["keyword"].as_str().expect("unknown keyword"),
            (line + 1, column),
        )
        .is_err(),
        "corrupt unknown span must cut the diagnostic edge"
    );

    fsl_syntax::validate_frontend_registry().expect("positive registry control");
}
