// SPDX-License-Identifier: Apache-2.0

#[path = "../../fslc/tests/support/ledger_findings.rs"]
#[allow(dead_code)]
mod findings;

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use serde_json::{Value, json};

fn evidence(result: &str) -> Value {
    json!({"result":result, "invariant":"Safe", "checked_to_depth":3,
        "trace_type":if result == "unknown_cti" { "induction_cti" } else { "invariant" },
        "cti":{"states":[{"state":{"cost":17, "cache":23}}], "violated_at":1},
        "trace":[{"state":{"cost":31, "cache":37}}],
        "hint":"preserve this hint", "rank_failure":"non_decreasing_action"})
}

fn render(value: &Value) -> String {
    let source = "spec StableEvidence { state { cost: Int, cache: Int } init { cost = 0 cache = 0 } invariant Safe { cost >= 0 } }";
    let model = build_model(parse_kernel_source(source, &FsResolver::new(".")).unwrap()).unwrap();
    fsl_tools::render_ledger("synthetic.fsl", &model, value, &json!({}), None, &[])
}

fn assert_ambient_removed(key: &str) {
    for result in ["violated", "unknown_cti"] {
        let stable = evidence(result);
        let mut first = stable.clone();
        first[key] = json!({"elapsed_s":0.1, "status":"miss"});
        let mut second = stable.clone();
        second[key] = json!({"elapsed_s":0.9, "status":"hit"});
        let before = first.clone();
        let body = render(&first);
        assert_eq!(first, before, "renderer must not mutate verify JSON");
        assert_eq!(
            findings::raw_findings(&body),
            json!([stable]),
            "strip only raw top-level {key}"
        );
        assert_eq!(
            body,
            render(&second),
            "ambient {key} must not affect artifact bytes"
        );
    }
}

#[test]
fn raw_top_level_cost_is_not_approval_content() {
    assert_ambient_removed("cost");
}
#[test]
fn raw_top_level_cache_is_not_approval_content() {
    assert_ambient_removed("cache");
}

#[test]
fn cti_hint_rank_failure_and_domain_cost_cache_remain_exact() {
    for result in ["violated", "unknown_cti"] {
        let value = evidence(result);
        assert_eq!(findings::raw_findings(&render(&value)), json!([value]));
    }
}
