// SPDX-License-Identifier: Apache-2.0

// Included only inside lib.rs's cfg(test) module, like the #1463 controls.
#[cfg(not(target_arch = "wasm32"))]
mod issue_1531 {
    use super::*;

    const RANGE: &str = "spec RangeSelected {\n  state { n: 0..1 }\n  init { n = 0 }\n  action inc() { n = n + 1 }\n  invariant P { n <= 2 }\n}\n";
    const MULTI: &str = "spec ReviewSelectedOverflowMultiple {\n  type K = 0..1\n  state { xs: Seq<K, 1> }\n  init { xs = Seq {} }\n  action take() { xs = xs.push(1) }\n  invariant P { xs.size() <= 3 }\n}\n";

    fn verify_all_bounds(source: &str, sequence: bool, ignored_selection: bool) {
        // Options has no property selection. Deserialize the real request
        // shape, including a separate preservation control for ignored keys.
        let mut options = json!({"depth": 4});
        if ignored_selection {
            options["property"] = json!("P");
        }
        let request: Request = serde_json::from_value(json!({
            "cmd": "verify", "source": source, "source_file": "model.fsl", "options": options,
        }))
        .expect("Worker request");
        // Public run also asks the browser for its solver version. Use the
        // existing native-host verify seam to execute the Worker orchestration;
        // these concrete boundary witnesses return before browser SMT is needed.
        let value = block_on(verify(&request, TEST_SOLVER_VERSION));
        eprintln!(
            "Worker verify sequence={sequence} ignored_selection={ignored_selection}: {value}"
        );
        assert_eq!(value["result"], "violated", "{value}");
        assert_eq!(value["violation_kind"], "type_bound", "{value}");
        assert_eq!(
            value["invariant"],
            fsl_core::SyntheticName::Bounds {
                state: if sequence { "xs" } else { "n" },
            }
            .internal(),
            "{value}"
        );
        assert_eq!(value["violated_at_step"], 2, "{value}");
        for key in ["message", "cti", "kind"] {
            assert_eq!(value[key], Value::Null, "{key}: {value}");
        }
        let trace = value["trace"].as_array().expect("Worker boundary trace");
        assert_eq!(trace.len(), 3, "{value}");
        for (step, entry) in trace.iter().enumerate() {
            assert_eq!(entry["step"], step, "{value}");
            assert_eq!(
                entry["state"],
                if sequence {
                    json!({"xs": vec![1; step]})
                } else {
                    json!({"n": step})
                },
                "{value}"
            );
            if step == 0 {
                assert_eq!(entry["action"], Value::Null, "{value}");
            } else {
                assert_eq!(
                    entry["action"],
                    json!({"name": if sequence { "take" } else { "inc" }, "params": {}, "loc": {"line": if sequence { 5 } else { 4 }, "column": 3}}),
                    "{value}"
                );
            }
        }
    }

    #[test]
    fn range_all_bounds() {
        verify_all_bounds(RANGE, false, false);
    }

    #[test]
    fn multi_all_bounds() {
        verify_all_bounds(MULTI, true, false);
    }

    #[test]
    fn range_ignores_property_key() {
        verify_all_bounds(RANGE, false, true);
    }

    #[test]
    fn multi_ignores_property_key() {
        verify_all_bounds(MULTI, true, true);
    }
}
