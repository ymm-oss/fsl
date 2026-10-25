// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1443: resolve compose State initializer/range aliases without changing the
//! inline-initializer type boundary. Orchestrator ruling: LANGUAGE.md's inline
//! initializer contract (lines 576/596) and model.rs's state-root prohibition
//! (line 839) outrank the issue table's implied acceptance of `seen = a.on`.
//! State reads must still reject, now naming the resolved public `a.on`; const
//! initializers and range bounds must check and affect the actual verdict.

#[path = "support/compose_scratch.rs"]
mod compose_scratch;

use compose_scratch::{check, scratch, verify};

fn constants(max: i32) -> String {
    format!("spec Constants {{ const MAX = {max} action stay() {{ }} }}")
}

fn composed(body: &str) -> String {
    format!("compose C {{\n  use Constants as a from \"a.fsl\"\n{body}\n}}")
}

fn const_verdict_pair(body: &str, results: [&str; 2]) {
    // All physical controls run before any alias assertion, so red alias
    // lowering cannot hide the opposing-polarity semantic control.
    let mut cases = Vec::new();
    for (max, expected) in [0, 1].into_iter().zip(results) {
        let source = composed(body);
        let physical = source.replace("a.MAX", "a__MAX");
        let dir = scratch(
            "issue-1443-const",
            &[
                ("a.fsl", &constants(max)),
                ("c.fsl", &source),
                ("physical.fsl", &physical),
            ],
        );
        let checked = check(&dir, "physical.fsl");
        assert_eq!(checked.1, 0, "{}", checked.0);
        let control = verify(&dir, "physical.fsl");
        assert_eq!(
            control.1,
            i32::from(expected == "violated"),
            "{}",
            control.0
        );
        assert_eq!(control.0["result"], expected, "{}", control.0);
        eprintln!(
            "MAX={max} physical expected={expected} produced={}",
            control.0["result"]
        );
        cases.push((dir, control));
    }
    for (dir, control) in cases {
        let checked = check(&dir, "c.fsl");
        assert_eq!(checked.1, 0, "{}", checked.0);
        let result = verify(&dir, "c.fsl");
        assert_eq!(result.1, control.1, "{}", result.0);
        assert_eq!(result.0["result"], control.0["result"], "{}", result.0);
        assert_eq!(result.0["violation_kind"], control.0["violation_kind"]);
        assert_eq!(result.0["invariant"], control.0["invariant"]);
    }
}

#[test]
fn initializer_const_alias_controls_initial_value() {
    const_verdict_pair(
        "  state { seen: Int = a.MAX }\n  invariant Zero { seen == 0 }",
        ["verified", "violated"],
    );
}

#[test]
fn range_const_alias_controls_the_upper_type_bound() {
    const_verdict_pair(
        "  state { seen: 0..a.MAX }\n  init { seen = 0 }\n  action choose(i in 0..1) { seen = i }",
        ["violated", "verified"],
    );
}

#[test]
fn initializer_state_alias_keeps_the_direct_spec_state_read_rejection() {
    let part = "spec A { state { on: Bool } init { on = false } }";
    let source = "compose C {\n  use A as a from \"a.fsl\"\n  state { seen: Bool = a.on }\n}";
    let direct = "spec Direct {\n  state { on: Bool }\n  state { seen: Bool = on }\n  init { on = false }\n}";
    let dir = scratch(
        "issue-1443-state-read",
        &[("a.fsl", part), ("c.fsl", source), ("direct.fsl", direct)],
    );
    let control = check(&dir, "direct.fsl");
    assert_eq!(control.1, 2, "{}", control.0);
    assert_eq!(
        control.0["message"],
        "inline initializer for 'seen' must not read state root 'on' at 3:24"
    );
    let result = check(&dir, "c.fsl");
    assert_eq!(result.1, 2, "{}", result.0);
    assert_eq!(result.0["kind"], control.0["kind"]);
    assert_eq!(
        result.0["message"],
        control.0["message"]
            .as_str()
            .expect("direct message")
            .replace("'on'", "'a.on'")
    );
    assert_eq!(result.0["loc"], control.0["loc"]);
}
