// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

fn diagnostic_cases() -> Vec<(String, Value)> {
    vec![
        (include_str!("fixtures/error_envelope_ai_project_unknown_tool.fsl").to_owned(), json!({"result":"error","kind":"semantics","message":"unknown tool 'MissingTool' in authority block"})),
        (include_str!("fixtures/error_envelope_ai_project_invalid_rule.fsl").to_owned(), json!({"result":"error","kind":"semantics","message":"unknown ai hard-contract rule 'unregistered_hard_rule' at 9:3","hint":"supported rules: forbidden_tool_blocked, human_approval_required, tool_authority, tool_precondition_declared, tool_schema_declared"})),
        ("ai_component NoTools { }\n".to_owned(), json!({"result":"error","kind":"type","message":"ai_component 'NoTools' declares no tools","loc":{"line":1,"column":1}})),
        ("ai_component EmptyAuthority { authority { } }\n".to_owned(), json!({"result":"error","kind":"type","message":"ai_component 'EmptyAuthority' declares no tools","loc":{"line":1,"column":1}})),
        ("ai_component Duplicate { tool Search { schema S; } fallback { when low require human; when low require human; } }\n".to_owned(), json!({"result":"error","kind":"semantics","message":"duplicate fallback reason 'low'"})),
    ]
}

fn metadata(worker: bool) -> Value {
    json!({"verifier":{"name":if worker {"fsl-wasm"} else {"fslc-rust"},"version":env!("CARGO_PKG_VERSION")},
        "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
        "solver":{"name":"z3","backend":if worker {"z3-solver-wasm"} else {"native-z3"},"version":"Z3 4.16.0.0"}})
}

#[test]
fn native_stdout_bytes_keep_the_complete_base_envelopes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../target/ai-envelope-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = diagnostic_cases();
    cases.push(("ai_component Valid { tool Search { schema S; } authority { } }\n".to_owned(), json!({"result":"ok","spec":"Valid","warnings":[{"kind":"no_user_invariants","message":"spec declares no user invariants (only implicit type bounds are checked)"}]})));
    for (i, (source, diagnostic)) in cases.into_iter().enumerate() {
        let path = root.join(format!("case-{i}.fsl"));
        std::fs::write(&path, source).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_fslc"))
            .arg("check")
            .arg(path)
            .env_remove("FSLC_CACHE")
            .env_remove("FSLC_UNSTABLE")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if diagnostic["result"] == "ok" { 0 } else { 2 })
        );
        assert!(output.stderr.is_empty(), "{:?}", output.stderr);
        let mut expected = json!({"fsl":"1.0"});
        expected
            .as_object_mut()
            .unwrap()
            .extend(diagnostic.as_object().unwrap().clone());
        expected["versions"] = metadata(false);
        let mut bytes = serde_json::to_vec_pretty(&expected).unwrap();
        bytes.push(b'\n');
        assert_eq!(output.stdout, bytes, "native case {i}");
    }
}

#[test]
fn worker_shaped_serialized_bytes_keep_fixed_metadata_and_legacy_errors() {
    let mut cases = diagnostic_cases();
    cases.push(("ai_component C { tool Search { schema S; } authority { } }\nstatistical_property Q {\n target C\n require min_samples >= 0\n}\n".to_owned(), json!({"result":"ok","spec":"project","dialect":"fsl-ai-project.v0","warnings":[],"ai_analysis_result":"ai_project_analyzed"})));
    for (source, diagnostic) in cases {
        let envelope = || {
            let mut output = json!({"fsl":"1.0"});
            output["versions"] = metadata(true);
            output.as_object().unwrap().clone()
        };
        let options = fsl_syntax::ParseOptions::stable();
        let admission = fslc_rust::migration::admit_source(
            &source,
            "project.fsl",
            None,
            &options,
            &envelope,
            false,
        )
        .unwrap();
        let mut no_solver =
            || -> Result<fsl_solver_z3::Z3Solver, String> { panic!("AI check requires no solver") };
        let resolver = fsl_core::FsResolver::new(".");
        let future = fslc_rust::check_stages::run_check_stages(
            &source,
            "project.fsl",
            "project.fsl",
            &resolver,
            &envelope,
            &options,
            &mut no_solver,
            &admission,
        );
        let mut future = std::pin::pin!(future);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let std::task::Poll::Ready(result) =
            std::future::Future::poll(future.as_mut(), &mut context)
        else {
            panic!("solver-free check yielded")
        };
        let (output, status) = result.expect_err("completed refusal");
        assert_eq!(status, if diagnostic["result"] == "ok" { 0 } else { 2 });
        let mut expected = envelope();
        expected.extend(diagnostic.as_object().unwrap().clone());
        assert_eq!(
            serde_json::to_vec(&output).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
    }
}
