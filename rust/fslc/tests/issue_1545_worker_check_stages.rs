// SPDX-License-Identifier: Apache-2.0

//! Worker-shaped shared-stage coverage. The existing Worker adapter forwarding
//! tests exercise its Err-return seam; native public run requires a JS bridge.

use std::future::Future;
use std::task::{Context, Poll, Waker};

use serde_json::{Map, Value, json};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}

fn worker_envelope() -> Map<String, Value> {
    json!({"fsl":"1.0", "versions":{
        "verifier":{"name":"fsl-wasm","version":env!("CARGO_PKG_VERSION")},
        "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
        "solver":{"name":"z3","backend":"z3-solver-wasm","version":"Z3 4.16.0.0"}
    }})
    .as_object()
    .unwrap()
    .clone()
}

#[test]
fn ai_project_components_are_rejected_with_the_full_worker_envelope() {
    let unknown = include_str!("fixtures/error_envelope_ai_project_unknown_tool.fsl");
    let rule = include_str!("fixtures/error_envelope_ai_project_invalid_rule.fsl");
    let later = format!("ai_component First {{ tool Search {{ schema S; }} }}\n{unknown}");
    let no_tools = "ai_component NoTools { }\nstatistical_property Q {\n target NoTools\n require min_samples >= 0\n}\n";
    for (source, diagnostic) in [
        (
            unknown,
            json!({"result":"error","kind":"semantics","message":"unknown tool 'MissingTool' in authority block"}),
        ),
        (
            rule,
            json!({"result":"error","kind":"semantics","message":"unknown ai hard-contract rule 'unregistered_hard_rule' at 9:3",
            "hint":"supported rules: forbidden_tool_blocked, human_approval_required, tool_authority, tool_precondition_declared, tool_schema_declared"}),
        ),
        (
            include_str!("fixtures/ai_project_inline_invalid_rule.fsl"),
            json!({"result":"error","kind":"semantics","message":"unknown ai hard-contract rule 'unregistered_inline_rule' at 4:33",
            "hint":"supported rules: forbidden_tool_blocked, human_approval_required, tool_authority, tool_precondition_declared, tool_schema_declared"}),
        ),
        (
            include_str!("fixtures/ai_project_later_invalid_rule.fsl"),
            json!({"result":"error","kind":"semantics","message":"unknown ai hard-contract rule 'unregistered_later_rule' at 12:7",
            "hint":"supported rules: forbidden_tool_blocked, human_approval_required, tool_authority, tool_precondition_declared, tool_schema_declared"}),
        ),
        (
            later.as_str(),
            json!({"result":"error","kind":"semantics","message":"unknown tool 'MissingTool' in authority block"}),
        ),
        (
            no_tools,
            json!({"result":"error","kind":"type","message":"ai_component 'NoTools' declares no tools","loc":{"line":1,"column":1}}),
        ),
    ] {
        let options = fsl_syntax::ParseOptions::stable();
        let admitted = fslc_rust::migration::admit_source(
            source,
            "project.fsl",
            None,
            &options,
            &worker_envelope,
            false,
        )
        .unwrap();
        let mut no_solver = || -> Result<fsl_solver_z3::Z3Solver, String> {
            panic!("AI project checking must not construct a solver")
        };
        let (output, status) = block_on(fslc_rust::check_stages::run_check_stages(
            source,
            "project.fsl",
            "project.fsl",
            &fsl_core::FsResolver::new("."),
            &worker_envelope,
            &options,
            &mut no_solver,
            &admitted,
        ))
        .expect_err("project check returns a completed envelope");
        println!("produced exit={status}, {output}");
        assert_eq!(status, 2);
        // Fixture-owned literals, not the production validator/renderer. The
        // whole object, including fixed Worker metadata, is compared.
        let mut expected = worker_envelope();
        expected.extend(diagnostic.as_object().unwrap().clone());
        assert_eq!(output, Value::Object(expected));
    }
}
