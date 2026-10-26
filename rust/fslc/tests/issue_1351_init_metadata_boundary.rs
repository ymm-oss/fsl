// SPDX-License-Identifier: Apache-2.0

//! Round-one metadata boundaries and complete impact evidence for #1351.
//! Detector and preservation labels are calibrated with production reverted.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const METADATA: &str = r#"spec InitMetadata {
  state { ready: Bool }
  @requirement("REQ-A", "alpha")
  @requirement("REQ-B", "beta")
  @requirement("REQ-A", "alpha")
  init "REQ-LEGACY: legacy init" { ready = false }
}
"#;

const UNDECIDED: &str = r#"spec Plain {
  state { ready: Bool }
  @undecided("initial policy is pending")
  init { ready = false }
}
"#;

// The issue's requirements fixture, unchanged from the original regression.
const ISSUE: &str = r#"requirements CounterReq {
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

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

fn analyze(source: &str, label: &str, suffix: &[&str]) -> Value {
    let directory = root().join(".claude/work").join(format!(
        "tests-1351-r1-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create task-local scratch");
    let path = directory.join("input.fsl");
    std::fs::write(&path, source).expect("write test input");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("analyze")
        .arg(&path)
        .args(suffix)
        .current_dir(root())
        .output()
        .expect("run native analyze");
    assert_eq!(
        output.status.code(),
        Some(0),
        "analyze {} {suffix:?}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("analyze JSON")
}

fn requirement_impact() -> Value {
    analyze(
        ISSUE,
        "requirement-impact",
        &[
            "--projection",
            "impact_graph",
            "--focus",
            "requirement:MODEL-CNT-001",
        ],
    )
}

/// Detector: normalized typed and legacy links must survive on one init node.
#[test]
fn init_metadata_normalizes_multiple_duplicate_and_legacy_requirement_relations() {
    let graph = analyze(METADATA, "metadata", &[]);
    let inits = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .filter(|node| node["kind"] == "init" || node["id"] == "init")
        .collect::<Vec<_>>();
    assert_eq!(inits.len(), 1, "exactly one init anchor: {graph:#}");
    assert_eq!(
        *inits[0],
        json!({
            "id":"init", "kind":"init", "label":"init", "name":"init",
            "meta":{"id":"REQ-A","text":"alpha"},
            "requirements":[
                {"id":"REQ-A","text":"alpha"},
                {"id":"REQ-B","text":"beta"},
                {"id":"REQ-LEGACY","text":"legacy init"}
            ]
        })
    );
    let covers = graph["edges"]
        .as_array()
        .expect("edges")
        .iter()
        .filter(|edge| edge["kind"] == "covers")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        json!(covers),
        json!([
            {"id":"edge:requirement:REQ-A:covers:init","kind":"covers","from":"requirement:REQ-A","to":"init"},
            {"id":"edge:requirement:REQ-B:covers:init","kind":"covers","from":"requirement:REQ-B","to":"init"},
            {"id":"edge:requirement:REQ-LEGACY:covers:init","kind":"covers","from":"requirement:REQ-LEGACY","to":"init"}
        ])
    );
}

/// Preservation: an annotation alone is not a requirement relation. Both full
/// values were measured on base 5a539bbe; no ambient fields are excluded.
#[test]
fn undecided_only_init_preserves_complete_base_analyze_output() {
    let graph = analyze(UNDECIDED, "undecided-graph", &[]);
    assert!(
        graph["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .all(|node| node["id"] != "init" && node["kind"] != "init")
    );
    assert!(
        graph["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .all(|edge| edge["from"] != "init" && edge["to"] != "init")
    );
    assert_eq!(
        graph,
        json!({
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
        })
    );
    assert_eq!(
        analyze(UNDECIDED, "undecided-review", &["--profile", "ai-review"]),
        json!({
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
        })
    );
}

/// Detector: the induced triangle has three edges and one undirected cycle,
/// but no directed SCC/cycle. Its fan counts follow from those three edges.
#[test]
fn requirement_impact_matches_the_complete_three_node_slice_and_all_metrics() {
    assert_eq!(
        requirement_impact(),
        json!({
            "fsl":"1.0", "result":"analyzed", "spec":"CounterReq", "analysis":"structure",
            "projection":"impact_graph", "schema_version":"analysis-graph.v0",
            "formal_status":"not_a_violation",
            "nodes":[
                {"id":"init","kind":"init","label":"init","name":"init",
                 "meta":{"id":"MODEL-CNT-001","text":"検証用の初期値として 0 から始める"},
                 "requirements":[{"id":"MODEL-CNT-001","text":"検証用の初期値として 0 から始める"}],
                 "direction":"downstream","directions":["downstream"],"focus_distance":1,"downstream_distance":1},
                {"id":"requirement:MODEL-CNT-001","kind":"requirement","label":"MODEL-CNT-001","name":"MODEL-CNT-001",
                 "direction":"focus","directions":["focus"],"focus_distance":0,"upstream_distance":0,"downstream_distance":0},
                {"id":"spec:CounterReq","kind":"spec","label":"CounterReq","name":"CounterReq",
                 "direction":"upstream","directions":["upstream"],"focus_distance":1,"upstream_distance":1}
            ],
            "edges":[
                {"id":"edge:requirement:MODEL-CNT-001:covers:init","kind":"covers","from":"requirement:MODEL-CNT-001","to":"init","formal_status":"not_a_violation"},
                {"id":"edge:spec:CounterReq:declares:init","kind":"declares","from":"spec:CounterReq","to":"init","formal_status":"not_a_violation"},
                {"id":"edge:spec:CounterReq:declares:requirement:MODEL-CNT-001","kind":"declares","from":"spec:CounterReq","to":"requirement:MODEL-CNT-001","formal_status":"not_a_violation"}
            ],
            "components":[{"id":"component:0","nodes":["init","requirement:MODEL-CNT-001","spec:CounterReq"]}],
            "sccs":[],"cycles":[],
            "degree":[
                {"node":"init","in":2,"out":0,"total":2},
                {"node":"requirement:MODEL-CNT-001","in":1,"out":1,"total":2},
                {"node":"spec:CounterReq","in":0,"out":2,"total":2}
            ],
            "metrics":{
                "node_count":3,"edge_count":3,"component_count":1,"scc_count":0,"cycle_rank":1,
                "max_fan_in":{"value":2,"nodes":["init"],"truncated":false},
                "max_fan_out":{"value":2,"nodes":["spec:CounterReq"],"truncated":false},
                "top_fan_in":[{"node":"init","in":2},{"node":"requirement:MODEL-CNT-001","in":1},{"node":"spec:CounterReq","in":0}],
                "top_fan_out":[{"node":"spec:CounterReq","out":2},{"node":"requirement:MODEL-CNT-001","out":1},{"node":"init","out":0}]
            }
        })
    );
}

/// Detector: init becomes a valid focus, not an unknown-focus error.
#[test]
fn init_focus_resolves_with_both_zero_distance_fields() {
    let graph = analyze(
        ISSUE,
        "init-impact",
        &["--projection", "impact_graph", "--focus", "init"],
    );
    let init = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "init")
        .expect("init focus node");
    assert_eq!(init["direction"], "focus");
    assert_eq!(init["directions"], json!(["focus"]));
    for key in ["focus_distance", "upstream_distance", "downstream_distance"] {
        assert_eq!(init[key], 0, "{key}: {graph:#}");
    }
}

/// Preservation: the existing focus/upstream distances, review-only status,
/// one component, and lack of directed cycles remain valid even on base.
#[test]
fn requirement_focus_preserves_upstream_distances_and_review_only_acyclic_status() {
    let graph = requirement_impact();
    let nodes = graph["nodes"].as_array().expect("nodes");
    let focus = nodes
        .iter()
        .find(|node| node["id"] == "requirement:MODEL-CNT-001")
        .expect("requirement focus");
    assert_eq!(focus["direction"], "focus");
    assert_eq!(focus["directions"], json!(["focus"]));
    for key in ["focus_distance", "upstream_distance", "downstream_distance"] {
        assert_eq!(focus[key], 0, "{key}: {graph:#}");
    }
    let spec = nodes
        .iter()
        .find(|node| node["id"] == "spec:CounterReq")
        .expect("upstream spec");
    assert_eq!(spec["direction"], "upstream");
    assert_eq!(spec["directions"], json!(["upstream"]));
    assert_eq!(spec["focus_distance"], 1);
    assert_eq!(spec["upstream_distance"], 1);
    assert!(spec.get("downstream_distance").is_none());
    assert_eq!(graph["formal_status"], "not_a_violation");
    assert_eq!(graph["metrics"]["component_count"], 1);
    assert_eq!(graph["metrics"]["scc_count"], 0);
    assert_eq!(graph["sccs"], json!([]));
    assert_eq!(graph["cycles"], json!([]));
}
