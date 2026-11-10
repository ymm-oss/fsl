// SPDX-License-Identifier: Apache-2.0

// A module import preserves the frozen helper's inner documentation comments.
#[path = "../../fslc/tests/support/issue_1517_sync_argument_cases.rs"]
mod issue_1517_sync_argument_forms;

mod issue_1517_sync_argument {
    use super::{issue_1517_sync_argument_forms as forms, *};

    fn versions() -> Value {
        json!({
            "verifier":{"name":"fsl-wasm","version":env!("CARGO_PKG_VERSION")},
            "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
            "solver":{"name":"z3","backend":"z3-solver-wasm","version":TEST_SOLVER_VERSION}
        })
    }

    fn expected(id: &str) -> Value {
        // Literal authored coordinates and complete messages, not production spans
        // or native output. Root errors must NOT acquire a component cause/wrapper.
        let (line, column, message) = match id {
            "root_bool" => (
                5,
                16,
                "invalid model expression: expression of type Bool is not assignable to Named(\"part.K\") at 5:16",
            ),
            "root_index" => (
                5,
                16,
                "invalid model expression: public Kernel index requires Map or Seq at 5:16",
            ),
            "second_actual_index" => (
                5,
                19,
                "invalid model expression: public Kernel index requires Map or Seq at 5:19",
            ),
            "root_pad_bool" => (
                8,
                16,
                "invalid model expression: expression of type Bool is not assignable to Named(\"part.K\") at 8:16",
            ),
            "component_index" => (
                2,
                3,
                "invalid model expression: component \"piece.fsl\" failed to check (part.put: public Kernel index requires Map or Seq at piece.fsl:8:5) at 2:3",
            ),
            "valid_zero" => {
                return json!({"fsl":"1.0","versions":versions(),
                "result":"ok","spec":"Root","warnings":[{"kind":"no_user_invariants",
                "message":"spec declares no user invariants (only implicit type bounds are checked)"}]});
            }
            _ => panic!("case outside finite Worker union: {id}"),
        };
        json!({"fsl":"1.0","versions":versions(),"result":"error","kind":"semantics",
            "loc":{"line":line,"column":column},"message":message})
    }

    fn record(id: &str, route: &str, actual: &Value, expected: &Value) -> bool {
        let pass = actual == expected;
        println!(
            "1517_WORKER {}",
            json!({"id":id,"route":route,"actual":actual,
            "expected":expected,"pass":pass})
        );
        pass
    }

    #[allow(clippy::too_many_lines)] // Keep all preflight routes ahead of aggregate assertions.
    fn run(id: &str) {
        let case = forms::cases()
            .into_iter()
            .find(|c| c.id == id)
            .expect("case");
        // Original public pair: the relation-index actual is unused by Piece.put.
        // Preserve the exact authored root and provider, not a substituted use.
        let case = if id == "root_index" {
            forms::Case {
                piece: case.piece.replace("last = a", "last = 0"),
                ..case
            }
        } else {
            case
        };
        let mut request = Request {
            cmd: "check".into(),
            source: case.source,
            source_file: "root.fsl".into(),
            files: [
                ("piece.fsl".into(), case.piece),
                ("good.fsl".into(), forms::GOOD.into()),
            ]
            .into(),
            options: Options::default(),
        };
        println!(
            "1517_WORKER_SOURCE {}",
            json!({"id":id,"cmd":request.cmd,
            "source":request.source,"source_file":request.source_file,"files":request.files,
            "options":{"depth":8,"deadlock":"warn"},
            "producer":std::env::current_exe().expect("test executable"),
            "implementation_fingerprint":option_env!("FSLC_IMPLEMENTATION_FINGERPRINT")})
        );
        let mut no_solver = || -> Result<fsl_solver_z3js::Z3JsSolver, String> {
            panic!("{id}: preflight must not construct a solver")
        };
        let wanted = expected(id);
        let checked = block_on(check_with(&request, TEST_SOLVER_VERSION, &mut no_solver));
        let mut failures = Vec::new();
        if id == "valid_zero" {
            // No positive verify: a successful verifier is allowed to need SMT.
            let built = build(&request, TEST_SOLVER_VERSION, &std::rc::Rc::default());
            if !record(id, "check_with", &checked, &wanted) {
                failures.push("check_with");
            }
            let resolver = MemoryResolver {
                files: request.files.clone(),
            };
            let kernel = fsl_core::parse_kernel_source_with_file_with_options(
                &request.source,
                &resolver,
                &request.source_file,
                &fsl_syntax::ParseOptions::stable(),
            )
            .expect("valid0 native kernel");
            let model = fsl_core::build_model(kernel).expect("valid0 native model");
            // Full model equality is secondary preservation, not an ownership oracle.
            let wanted_build = json!({"model":format!("{model:#?}"),"diagnostics":[]});
            let (actual_build, action_start) = match built {
                Ok((model, diagnostics)) => {
                    let span = model
                        .actions
                        .iter()
                        .find(|a| a.name == "step")
                        .expect("step")
                        .span;
                    (
                        json!({"model":format!("{model:#?}"),"diagnostics":diagnostics}),
                        json!({"line":span.start.line,"column":span.start.column}),
                    )
                }
                Err(packet) => (packet, Value::Null),
            };
            if !record(id, "build", &actual_build, &wanted_build) {
                failures.push("build");
            }
            if !record(
                id,
                "action_start",
                &action_start,
                &json!({"line":4,"column":3}),
            ) {
                failures.push("action_start");
            }
        } else {
            request.cmd = "verify".into();
            let verified = block_on(verify_with(&request, TEST_SOLVER_VERSION, &mut no_solver));
            let built = match build(&request, TEST_SOLVER_VERSION, &std::rc::Rc::default()) {
                Err(packet) => packet,
                Ok((model, diagnostics)) => {
                    json!({"unexpected_success":format!("{model:#?}"),"diagnostics":diagnostics})
                }
            };
            // All rejecting routes have executed BEFORE any semantic assertion.
            for (route, actual) in [
                ("check_with", &checked),
                ("verify_with", &verified),
                ("build", &built),
            ] {
                if !record(id, route, actual, &wanted) {
                    failures.push(route);
                }
            }
            request.cmd = "check".into();
            let native = native_check_error(&request, TEST_SOLVER_VERSION);
            if !record(id, "native_parity_secondary", &checked, &native) {
                failures.push("native_parity_secondary");
            }
        }
        assert!(
            failures.is_empty(),
            "{id}: full-packet mismatches: {failures:?}"
        );
    }

    #[test]
    fn root_bool() {
        run("root_bool");
    }
    #[test]
    fn root_index() {
        run("root_index");
    }
    #[test]
    fn second_actual_index() {
        run("second_actual_index");
    }
    #[test]
    fn root_pad_bool() {
        run("root_pad_bool");
    }
    #[test]
    fn component_index() {
        run("component_index");
    }
    #[test]
    fn valid_zero() {
        run("valid_zero");
    }
}
