// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
mod aggregate_and_component_locations {
    use super::*;
    #[allow(dead_code)]
    mod forms {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../fslc/tests/support/issue_1463_aggregate_and_component_locations.rs"));
    }

    mod nested_forms {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../fslc/tests/support/issue_1463_nested_aggregates_and_sync_ownership.rs"));
    }

    #[test]
    fn issue_1463_aggregate_and_component_preflight_locations() {
        check_cases(forms::cases());
    }

    #[test]
    fn issue_1463_nested_aggregate_and_sync_preflight_locations() {
        check_cases(nested_forms::cases());
    }

    fn check_cases(cases: Vec<forms::Case>) {
        for case in cases {
            let mut request = Request { cmd: "check".into(), source: case.source.clone(), source_file: "root.fsl".into(), files: case.files.iter().cloned().collect(), options: Options::default(), unstable: vec![] };
            let mut no_solver = || -> Result<fsl_solver_z3js::Z3JsSolver, String> { panic!("unexpected solver: {}", case.id) };
            let report = block_on(check_with(&request, TEST_SOLVER_VERSION, &mut no_solver));
            if case.rejection {
                assert_eq!(report["result"], "error", "{}: {report}", case.id);
                let (line, column) = case.location();
                assert_eq!(report["loc"], json!({"line":line,"column":column}), "{}: {report}", case.id);
                assert_eq!(report["kind"], "semantics");
                nested_forms::assert_message(&case, report["message"].as_str().unwrap());
                assert_eq!(report, native_check_error(&request, TEST_SOLVER_VERSION), "{}: full native/Worker error", case.id);
                request.cmd = "verify".into();
                assert_eq!(block_on(verify_with(&request, TEST_SOLVER_VERSION, &mut no_solver)), report, "{}: verify preflight", case.id);
                assert_eq!(build(&request, TEST_SOLVER_VERSION).unwrap_err(), report);
            } else {
                assert_eq!(report["result"], "ok", "{}: {report}", case.id);
            }
        }
    }
}
