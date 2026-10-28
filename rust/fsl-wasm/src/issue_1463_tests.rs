// SPDX-License-Identifier: Apache-2.0

mod issue_1463 {
    use super::*;

    #[allow(dead_code)] // The CLI uses baseline/verdict/selector metadata.
    mod forms {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fslc/tests/support/issue_1463_forms.rs"
        ));
    }

    #[test]
    fn issue_1463_worker_source_preflight_matches_native_without_solver() {
        let mut failures = Vec::new();
        for c in forms::cases() {
            let mut request = Request {
                cmd: "check".into(),
                source: c.source.clone(),
                source_file: "support.fsl".into(),
                files: c.files.iter().cloned().collect(),
                options: Options::default(),
                unstable: Vec::new(),
            };
            let mut no_solver = || -> Result<fsl_solver_z3js::Z3JsSolver, String> {
                panic!("{}: source preflight must not request a solver", c.id)
            };
            let checked = block_on(check_with(&request, TEST_SOLVER_VERSION, &mut no_solver));
            if c.rejection.is_none() {
                assert_eq!(
                    checked["result"],
                    if c.expected[0] == forms::Outcome::Error { "error" } else { "ok" },
                    "{}: {checked}",
                    c.id
                );
                continue;
            }
            if checked["result"] != "error" {
                failures.push(format!("{}: expected located semantics error before solver; produced {checked}", c.id));
                continue;
            }
            forms::assert_rejection(&c, &checked);
            let native = native_check_error(&request, TEST_SOLVER_VERSION);
            assert_eq!(
                checked, native,
                "{}: full native/Worker check envelope", c.id
            );
            request.cmd = "verify".into();
            let verified = block_on(verify_with(
                &request, TEST_SOLVER_VERSION, &mut no_solver,
            ));
            forms::assert_rejection(&c, &verified);
            assert_eq!(
                verified, native,
                "{}: full verify preflight envelope", c.id
            );
            let built = build(&request, TEST_SOLVER_VERSION).expect_err("verify build must reject");
            // Raw build precedes the public check/verify warning finalization.
            assert_eq!(
                built,
                native_check_error_unfinalized(&request, TEST_SOLVER_VERSION),
                "{}: build must preserve diagnostic",
                c.id
            );
        }
        assert!(
            failures.is_empty(),
            "{} failing Worker cells:\n{}", failures.len(), failures.join("\n")
        );
    }
}
