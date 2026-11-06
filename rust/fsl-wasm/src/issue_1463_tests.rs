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
            let built = build(&request, TEST_SOLVER_VERSION, &std::rc::Rc::default()).expect_err("verify build must reject");
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

    #[test]
    fn issue_1480_worker_range_admission_matches_checked_native_without_solver() {
        for (source, message, column) in [
            (
                "spec R {type T=0..3 state {n:T} init {n=0} action go() {n=1 ensures count(i in 0..n)>=0}}",
                "invalid model expression: 'n' is not an integer const at 1:44",
                44,
            ),
            (
                "spec R {type T=0..3 state {n:T} init {n=0} reachable P {forall i in 0..n {i>=0}}}",
                "'n' is not an integer const",
                44,
            ),
        ] {
            let mut request = Request {
                cmd: "check".into(),
                source: source.into(),
                source_file: "range.fsl".into(),
                files: std::collections::BTreeMap::default(),
                options: Options::default(),
            };
            let mut no_solver = || -> Result<fsl_solver_z3js::Z3JsSolver, String> {
                panic!("range admission must precede solver creation")
            };
            let native = native_check_error(&request, TEST_SOLVER_VERSION);
            assert_eq!(native["result"], "error");
            assert_eq!(native["kind"], "semantics");
            assert_eq!(native["message"], message);
            assert_eq!(native["loc"], serde_json::json!({"line":1,"column":column}));
            assert_eq!(
                block_on(check_with(&request, TEST_SOLVER_VERSION, &mut no_solver)),
                native
            );
            request.cmd = "verify".into();
            assert_eq!(
                block_on(verify_with(&request, TEST_SOLVER_VERSION, &mut no_solver)),
                native
            );
        }
    }

    #[test]
    fn issue_1480_worker_init_range_priority_matches_checked_native() {
        for (source, message) in [
            (
                "spec FieldRange {type K=0..2 struct Cell {a:K,b:K} state {s:Cell} init {s.a=1 s.b=0 forall i in 0..s.a {}} action idle(){}}",
                "init forall range bounds must be compile-time constants; state variable 's' is not allowed",
            ),
            (
                "spec FieldRange {type K=0..2 struct Cell {a:K,b:K} state {s:Cell} init {s.a=1 forall i in 0..s.a {}} action idle(){}}",
                "init forall range bounds must be compile-time constants; state variable 's' is not allowed",
            ),
            (
                "spec FieldRange {type K=0..2 struct Cell {a:K,b:K} state {s:Cell} init {s.a=1 forall i in 0..s.a {} s.b=0} action idle(){}}",
                "init cannot read 's.a' before state variable 's' is fully assigned",
            ),
        ] {
            let mut request = Request {
                cmd: "check".into(),
                source: source.into(),
                source_file: "priority.fsl".into(),
                files: std::collections::BTreeMap::default(),
                options: Options::default(),
            };
            let mut no_solver = || -> Result<fsl_solver_z3js::Z3JsSolver, String> {
                panic!("init admission must precede solver creation")
            };
            let native = native_check_error(&request, TEST_SOLVER_VERSION);
            assert_eq!(native["result"], "error");
            assert_eq!(native["kind"], "semantics");
            assert_eq!(native["message"], message);
            assert_eq!(
                native["loc"],
                serde_json::json!({"line":1,"column":source.find("forall").unwrap()+1})
            );
            assert_eq!(
                block_on(check_with(&request, TEST_SOLVER_VERSION, &mut no_solver)),
                native
            );
            request.cmd = "verify".into();
            assert_eq!(
                block_on(verify_with(&request, TEST_SOLVER_VERSION, &mut no_solver)),
                native
            );
            let built = build(&request, TEST_SOLVER_VERSION, &std::rc::Rc::default())
                .expect_err("checked build must retain init refusal");
            assert_eq!(built, native_check_error_unfinalized(&request, TEST_SOLVER_VERSION));
        }
    }
}
