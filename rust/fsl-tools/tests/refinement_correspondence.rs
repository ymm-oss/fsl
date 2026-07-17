// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{SurfaceDocument, parse_surface_document};
use fsl_tools::analyze_refinement;

#[test]
fn refinement_graph_exposes_correspondence_origin_and_shared_progress_identity() {
    let document = parse_surface_document(
        "refinement R { impl Impl abs Abs action step(v: N) -> run(v) preserve progress { respond Eventually by step } }",
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
