// SPDX-License-Identifier: Apache-2.0

//! Decision A for #1351: init is a traceability anchor, not an orphan.
//! The genuine-orphan and full-output controls preserve the detector's scope.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const REQUIREMENTS: &str = r#"requirements CounterReq {
  type N = 0..3
  state { x: N }

  @requirement("MODEL-CNT-001", "検証用の初期値として 0 から始める")
  init { x = 0 }

  requirement REQ-CNT-001 "2 を超えて数えない" {
    action inc() {
      requires x < 2
      x = x + 1
    }

    invariant AtMostTwo { x <= 2 }
  }
}
"#;

const KERNEL: &str = r#"spec CounterKernel {
  type N = 0..3
  state { x: N }

  @requirement("MODEL-CNT-001", "検証用の初期値として 0 から始める")
  init { x = 0 }

  @requirement("REQ-CNT-001", "2 を超えて数えない")
  action inc() {
    requires x < 2
    x = x + 1
  }

  @requirement("REQ-CNT-001", "2 を超えて数えない")
  invariant AtMostTwo { x <= 2 }
}
"#;

const PLAIN: &str = "spec Plain {\n  state { ready: Bool }\n  init { ready = false }\n}\n";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

fn scratch(label: &str) -> PathBuf {
    let path = root().join(".claude/work").join(format!(
        "tests-1351-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&path).expect("create task-local scratch directory");
    path
}

fn fixture(directory: &Path, name: &str, source: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, source).expect("write acceptance input");
    path
}

fn invoke(command: &str, path: &Path, suffix: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(command)
        .arg(path)
        .args(suffix)
        .current_dir(root())
        .output()
        .expect("run native fslc")
}

fn run(command: &str, path: &Path, suffix: &[&str]) -> Value {
    let output = invoke(command, path, suffix);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{command} {} {suffix:?}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("native JSON envelope")
}

fn assert_no_dangling_edges(graph: &Value) {
    let ids = graph["nodes"]
        .as_array()
        .expect("graph nodes")
        .iter()
        .map(|node| node["id"].as_str().expect("node id"))
        .collect::<BTreeSet<_>>();
    for edge in graph["edges"].as_array().expect("graph edges") {
        assert!(
            ids.contains(edge["from"].as_str().expect("edge from")),
            "{edge:#}"
        );
        assert!(
            ids.contains(edge["to"].as_str().expect("edge to")),
            "{edge:#}"
        );
    }
}

fn assert_init_anchor(graph: &Value) {
    let init = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "init")
        .unwrap_or_else(|| panic!("missing init traceability node: {graph:#}"));
    for key in ["id", "kind", "name", "label"] {
        assert_eq!(init[key], "init", "init {key}: {graph:#}");
    }
    assert!(
        graph["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .any(|edge| {
                edge["id"] == "edge:requirement:MODEL-CNT-001:covers:init"
                    && edge["kind"] == "covers"
                    && edge["from"] == "requirement:MODEL-CNT-001"
                    && edge["to"] == "init"
            }),
        "missing requirement-to-init covers edge: {graph:#}"
    );
    assert_no_dangling_edges(graph);
}

fn assert_init_contract(source: &str, spec: &str, label: &str) {
    let directory = scratch(label);
    let path = fixture(&directory, "init.fsl", source);
    let checked = run("check", &path, &["--strict-tags"]);
    assert_eq!(checked["result"], "ok", "{checked:#}");
    assert_eq!(checked["warnings"], json!([]), "{checked:#}");
    let lint = run("lint", &path, &[]);
    assert_eq!(lint["finding_count"], 0, "{lint:#}");
    assert_eq!(lint["files"][0]["findings"], json!([]), "{lint:#}");
    let review = run("analyze", &path, &["--profile", "ai-review"]);
    assert!(
        !review["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| {
                finding["finding_type"] == "disconnected_requirement"
                    && finding["involved_nodes"] == json!(["requirement:MODEL-CNT-001"])
            }),
        "init-only requirement is still reported as disconnected: {review:#}"
    );
    let graph = run("analyze", &path, &[]);
    assert_eq!(graph["result"], "analyzed", "{graph:#}");
    assert_init_anchor(&graph);
    assert!(
        graph["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .any(|edge| {
                edge["id"] == format!("edge:spec:{spec}:declares:init")
                    && edge["kind"] == "declares"
                    && edge["from"] == format!("spec:{spec}")
                    && edge["to"] == "init"
            }),
        "missing spec-to-init declares edge: {graph:#}"
    );
}

#[test]
fn requirements_init_only_requirement_has_init_covers_edge_and_no_disconnected_finding() {
    assert_init_contract(REQUIREMENTS, "CounterReq", "requirements");
}

#[test]
fn kernel_init_only_requirement_has_init_covers_edge_and_no_disconnected_finding() {
    assert_init_contract(KERNEL, "CounterKernel", "kernel");
}

/// Preservation, not a detector of #1351: this passes even while the base
/// separately reports its old init-only false positive. On head it rejects
/// blanket suppression of all orphan findings whenever an init node exists.
#[test]
fn a_truly_orphaned_requirement_still_fires_disconnected_requirement() {
    let kernel = fsl_core::parse_kernel_source(KERNEL, &fsl_core::FsResolver::new("."))
        .expect("parse kernel fixture");
    let model = fsl_core::build_model(kernel).expect("checked model");
    let mut graph = fsl_tools::build_tsg(&model);
    graph["nodes"].as_array_mut().expect("nodes").push(json!({
        "id":"requirement:REQ-ORPHAN", "kind":"requirement",
        "name":"REQ-ORPHAN", "label":"REQ-ORPHAN"
    }));
    graph["edges"].as_array_mut().expect("edges").push(json!({
        "id":"edge:spec:CounterKernel:declares:requirement:REQ-ORPHAN",
        "kind":"declares", "from":"spec:CounterKernel", "to":"requirement:REQ-ORPHAN"
    }));
    assert_no_dangling_edges(&graph);
    let findings = fsl_tools::structural_review_findings(&graph);
    let orphan = findings
        .iter()
        .filter(|finding| {
            finding["finding_type"] == "disconnected_requirement"
                && finding["involved_nodes"] == json!(["requirement:REQ-ORPHAN"])
        })
        .collect::<Vec<_>>();
    assert_eq!(orphan.len(), 1, "{findings:#?}");
    assert_eq!(orphan[0]["formal_status"], "not_a_violation");
    assert_eq!(orphan[0]["severity"], "review_required");
    assert_eq!(
        orphan[0]["witness"],
        json!({
            "kind":"isolated_node", "node":"requirement:REQ-ORPHAN"
        })
    );
}

/// Complete CLI values measured twice on base 5a539bbe27dc933d7fe8df95e6189cb75cec6cc9.
/// No fields are excluded: these envelopes contain no ambient cache/timing data.
#[test]
fn unannotated_init_preserves_full_analyze_json() {
    let directory = scratch("plain");
    let path = fixture(&directory, "plain.fsl", PLAIN);
    let expected_graph = json!({
        "fsl":"1.0", "result":"analyzed", "spec":"Plain", "analysis":"structure",
        "projection":"tsg", "schema_version":"tsg.v0",
        "nodes":[
            {"id":"phys_state:ready","kind":"phys_state","label":"ready","name":"ready","logical":"ready"},
            {"id":"spec:Plain","kind":"spec","label":"Plain","name":"Plain"},
            {"id":"state:ready","kind":"state","label":"ready","name":"ready","type":["bool"]}
        ],
        "edges":[
            {"id":"edge:spec:Plain:declares:phys_state:ready","kind":"declares","from":"spec:Plain","to":"phys_state:ready"},
            {"id":"edge:spec:Plain:declares:state:ready","kind":"declares","from":"spec:Plain","to":"state:ready"},
            {"id":"edge:state:ready:expands_to:phys_state:ready","kind":"expands_to","from":"state:ready","to":"phys_state:ready"}
        ]
    });
    let expected_review = json!({
        "fsl":"1.0", "result":"analyzed", "spec":"Plain", "analysis":"structure",
        "profile":"ai-review", "schema_version":"analysis-findings.v0",
        "findings":[{
            "finding_id":"STRUCT-UNWRITTEN-STATE-0001", "analysis":"structure",
            "finding_type":"unwritten_state", "severity":"review_required", "confidence":0.68,
            "formal_status":"not_a_violation", "involved_nodes":["state:ready"],
            "witness":{"kind":"state_has_no_action_writes","node":"state:ready","read_by":[]},
            "why_it_matters":"The state variable is initialized but no action writes it in the structural graph.",
            "candidate_repairs":[{"kind":"review_state_role","template":"Make the value a const/model parameter if it is intentionally fixed, or add the missing action/effect that changes it."}],
            "do_not_assume":["The state variable is useless.","A verifier property is violated.","The variable is safe to delete without checking generated dialect state."]
        }]
    });
    for _ in 0..2 {
        assert_eq!(run("analyze", &path, &[]), expected_graph);
        assert_eq!(
            run("analyze", &path, &["--profile", "ai-review"]),
            expected_review
        );
    }
}

#[test]
fn init_traceability_survives_requirement_projection_and_requirement_focus() {
    let directory = scratch("projections");
    for (name, source) in [("requirements.fsl", REQUIREMENTS), ("kernel.fsl", KERNEL)] {
        let path = fixture(&directory, name, source);
        let requirements = run(
            "analyze",
            &path,
            &["--projection", "requirement_property_graph"],
        );
        assert_init_anchor(&requirements);
        let impact = run(
            "analyze",
            &path,
            &[
                "--projection",
                "impact_graph",
                "--focus",
                "requirement:MODEL-CNT-001",
            ],
        );
        assert_init_anchor(&impact);
        let init = impact["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .find(|node| node["id"] == "init")
            .expect("init in impact slice");
        assert_eq!(init["direction"], "downstream", "{impact:#}");
        assert_eq!(init["focus_distance"], 1, "{impact:#}");
    }
}

#[test]
fn init_traceability_appears_in_dot_and_mermaid_exports() {
    let directory = scratch("exports");
    for (name, source) in [("requirements.fsl", REQUIREMENTS), ("kernel.fsl", KERNEL)] {
        let path = fixture(&directory, name, source);
        for format in ["dot", "mermaid"] {
            let first = invoke("analyze", &path, &["--format", format]);
            let second = invoke("analyze", &path, &["--format", format]);
            assert_eq!(first.status.code(), Some(0), "{first:?}");
            assert_eq!(second.status.code(), Some(0), "{second:?}");
            assert!(first.stderr.is_empty() && second.stderr.is_empty());
            assert_eq!(
                first.stdout, second.stdout,
                "{format} must be deterministic"
            );
            let text = String::from_utf8(first.stdout).expect("UTF-8 graph export");
            let (node, edge) = if format == "dot" {
                (
                    "\"init\" [label=\"init\"",
                    "\"requirement:MODEL-CNT-001\" -> \"init\" [label=\"covers\"];",
                )
            } else {
                (
                    "init[\"init\"]",
                    "requirement_MODEL_CNT_001 -->|covers| init",
                )
            };
            assert!(text.contains(node), "{format} missing init node: {text}");
            assert!(
                text.contains(edge),
                "{format} missing init covers edge: {text}"
            );
        }
    }
}

/// Reverse only the manifest's live layer transform, then compare all node
/// and edge fields. Every removed field must be present with its expected value.
#[test]
fn annotated_init_manifest_layer_preserves_standalone_tsg() {
    let directory = scratch("manifest");
    let source = fixture(&directory, "requirements.fsl", REQUIREMENTS);
    let manifest = fixture(
        &directory,
        "fsl-project.toml",
        "[requirements]\nfile = \"requirements.fsl\"\n",
    );
    let standalone = run("analyze", &source, &[]);
    let project = run(
        "analyze",
        &manifest,
        &["--projection", "traceability_graph"],
    );
    assert_no_dangling_edges(&project);
    let mut nodes = project["nodes"]
        .as_array()
        .expect("manifest nodes")
        .iter()
        .filter(|node| node["layer"] == "requirements")
        .cloned()
        .collect::<Vec<_>>();
    let mut edges = project["edges"]
        .as_array()
        .expect("manifest edges")
        .iter()
        .filter(|edge| edge["layer"] == "requirements")
        .cloned()
        .collect::<Vec<_>>();
    assert!(!nodes.is_empty() && !edges.is_empty(), "{project:#}");
    let mut renamed_spec = 0;
    for node in &mut nodes {
        let object = node.as_object_mut().expect("node object");
        assert_eq!(object.remove("layer"), Some(json!("requirements")));
        let id = object["id"].as_str().expect("prefixed node id");
        object.insert(
            "id".to_owned(),
            json!(id.strip_prefix("requirements:").expect("layer node prefix")),
        );
        if object["kind"] == "requirements_spec" {
            assert_eq!(object["id"], "spec:CounterReq");
            object.insert("kind".to_owned(), json!("spec"));
            renamed_spec += 1;
        }
    }
    assert_eq!(renamed_spec, 1, "spec-role reversal must be live");
    for edge in &mut edges {
        let object = edge.as_object_mut().expect("edge object");
        assert_eq!(object.remove("layer"), Some(json!("requirements")));
        for (key, prefix) in [
            ("id", "edge:requirements:"),
            ("from", "requirements:"),
            ("to", "requirements:"),
        ] {
            let value = object[key].as_str().expect("prefixed edge field");
            object.insert(
                key.to_owned(),
                json!(value.strip_prefix(prefix).expect("layer edge prefix")),
            );
        }
    }
    nodes.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    edges.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    assert_eq!(
        json!(nodes),
        standalone["nodes"],
        "complete layer node comparison"
    );
    assert_eq!(
        json!(edges),
        standalone["edges"],
        "complete layer edge comparison"
    );
    assert_init_anchor(&json!({"nodes":nodes,"edges":edges}));
}
