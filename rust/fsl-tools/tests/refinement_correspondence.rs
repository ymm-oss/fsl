// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{SurfaceDocument, parse_surface_document};
use fsl_tools::analyze_refinement;

#[test]
fn refinement_graph_exposes_correspondence_origin_and_shared_progress_identity() {
    let document = parse_surface_document(
        "refinement R { impl Impl abs Abs enum conversion status ImplStatus -> AbsStatus { Open -> Ready Closed -> Done } enum abstraction collapse ImplPhase -> AbsPhase { Queued -> Pending Running -> Pending Finished -> Done } action step(v: N) -> run(v) preserve progress { respond Eventually by step } }",
    )
    .expect("parse refinement");
    let SurfaceDocument::Refinement(refinement) = document else {
        panic!("expected refinement document");
    };
    let graph = analyze_refinement(&refinement);
    let action = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "action_map:step")
        .expect("action node");
    assert_eq!(action["origin"], "refinement_file");
    let conversion = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "enum_conversion:status")
        .expect("enum conversion node");
    assert_eq!(conversion["source_type"], "ImplStatus");
    assert_eq!(conversion["target_type"], "AbsStatus");
    assert_eq!(
        conversion["members"],
        serde_json::json!([
            {"source": "Open", "target": "Ready"},
            {"source": "Closed", "target": "Done"}
        ])
    );
    let abstraction = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "enum_abstraction:collapse")
        .expect("enum abstraction node");
    assert_eq!(abstraction["source_type"], "ImplPhase");
    assert_eq!(abstraction["target_type"], "AbsPhase");
    assert_eq!(
        abstraction["members"],
        serde_json::json!([
            {"source": "Queued", "target": "Pending"},
            {"source": "Running", "target": "Pending"},
            {"source": "Finished", "target": "Done"}
        ])
    );
    assert!(
        graph["edges"]
            .as_array()
            .expect("edges")
            .iter()
            .any(|edge| {
                edge["from"] == "progress_response:Eventually" && edge["to"] == "impl_action:step"
            })
    );
}
