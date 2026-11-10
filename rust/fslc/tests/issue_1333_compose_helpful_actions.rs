// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1333: a component's helpful action AND its argument expressions retain
//! their referents after composition. Leadsto binders stay bound, not prefixed.
//! Before the fix a component that verifies alone fails after composition with
//! exit 2, `helpful action 'flip' is not declared` (or `step` for per-binding
//! progress). Actual violated inputs must remain violated, not become green.
//! The physical-name compose helper is explicitly a preservation control.

#[path = "support/compose_scratch.rs"]
mod compose_scratch;

use compose_scratch::{check, scratch, verify, verify_induction};
use fsl_syntax::{Expr, HelpfulAction, SpecItem};
use serde_json::json;
use std::path::Path;

const COMPOSE: &str = "compose C {\n  use A as a from \"a.fsl\"\n}\n";

fn lowered_helper(dir: &Path) -> HelpfulAction {
    let kernel = fsl_core::parse_kernel_source(COMPOSE, &fsl_core::FsResolver::new(dir))
        .expect("lower compose");
    kernel
        .syntax()
        .items
        .iter()
        .find_map(|item| match item {
            SpecItem::LeadsTo { helpful, .. } => helpful.first().cloned(),
            _ => None,
        })
        .expect("helpful action")
}

fn flip_component(reaches_goal: bool) -> String {
    format!(
        "spec A {{
  state {{ on: Bool }}
  init {{ on = false }}
  fair action flip() {{ on = {reaches_goal} }}
  leadsTo Progress {{ not on ~> on helpful flip() decreases if on then 0 else 1 }}
}}
"
    )
}

fn component_and_compose_verdict(reaches_goal: bool, expected: &str, code: i32) {
    let dir = scratch(
        "issue-1333",
        &[("a.fsl", &flip_component(reaches_goal)), ("c.fsl", COMPOSE)],
    );
    let (standalone, status) = verify(&dir, "a.fsl");
    assert_eq!(
        (status, standalone["result"].as_str()),
        (code, Some(expected)),
        "{standalone}"
    );
    if reaches_goal {
        let (proof, status) = verify_induction(&dir, "a.fsl");
        assert_eq!(status, 0, "{proof}");
        assert_eq!(proof["result"], "proved", "{proof}");
        assert_eq!(proof["leads_to"]["Progress"]["proof"], "ranking", "{proof}");
    }
    let (checked, status) = check(&dir, "c.fsl");
    assert_eq!(status, 0, "A1333 check: {checked}");
    let (composed, status) = verify(&dir, "c.fsl");
    assert_eq!(
        (status, composed["result"].as_str()),
        (code, Some(expected)),
        "{composed}"
    );
    if reaches_goal {
        assert!(composed["leads_to"]["a.Progress"].is_object(), "{composed}");
        let (proof, status) = verify_induction(&dir, "c.fsl");
        assert_eq!(status, 0, "{proof}");
        assert_eq!(proof["result"], "proved", "{proof}");
        assert_eq!(
            proof["leads_to"]["a.Progress"]["proof"], "ranking",
            "{proof}"
        );
    } else {
        assert_eq!(composed["violation_kind"], "leadsTo", "{composed}");
    }
}

#[test]
fn a1333_1_component_helpful_progress_verifies() {
    component_and_compose_verdict(true, "verified", 0);
}

#[test]
fn a1333_2_component_helpful_progress_rejects_a_real_lasso() {
    component_and_compose_verdict(false, "violated", 1);
}

const PER_BINDING: &str = "spec A {
  type Case = 0..1
  type Level = 0..2
  state { level: Map<Case, Level>, c: Case }
  init { c = 0 forall c: Case { level[c] = 2 } }
  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  leadsTo Progress {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
";

#[test]
fn a1333_3_per_binding_helpful_keeps_its_bound_argument() {
    // The state decoy still becomes a__c if a bound helpful argument is captured.
    let dir = scratch(
        "issue-1333-binding",
        &[("a.fsl", PER_BINDING), ("c.fsl", COMPOSE)],
    );
    let (standalone, status) = verify(&dir, "a.fsl");
    assert_eq!(status, 0, "{standalone}");
    assert_eq!(standalone["result"], "verified", "{standalone}");
    let (proof, status) = verify_induction(&dir, "a.fsl");
    assert_eq!(status, 0, "{proof}");
    assert_eq!(proof["result"], "proved", "{proof}");
    let (checked, status) = check(&dir, "c.fsl");
    assert_eq!(status, 0, "{checked}");
    let (composed, status) = verify(&dir, "c.fsl");
    assert_eq!(status, 0, "{composed}");
    assert_eq!(composed["result"], standalone["result"], "{composed}");
    let (proof, status) = verify_induction(&dir, "c.fsl");
    assert_eq!(status, 0, "{proof}");
    assert_eq!(proof["result"], "proved", "{proof}");
    let helper = lowered_helper(&dir);
    assert_eq!(helper.action, "a__step");
    assert_eq!(helper.args, vec![Expr::Var("c".to_owned())]);
}

#[test]
fn a1333_4_helpful_argument_component_constant_is_prefixed() {
    // Both sources check; #1580 also pins induction lookup of a__ONE.
    // Inspect the lowered argument to keep the compose rewrite explicit.
    let component = "spec A {
  const ONE = 1
  type Id = 0..1
  state { n: Id }
  init { n = 1 }
  fair action flip(i: Id) { n = 0 }
  leadsTo Progress { n > 0 ~> n == 0 helpful flip(ONE) decreases n }
}
";
    let dir = scratch(
        "issue-1333-constant",
        &[("a.fsl", component), ("c.fsl", COMPOSE)],
    );
    let (standalone, status) = check(&dir, "a.fsl");
    assert_eq!(status, 0, "{standalone}");
    let (standalone, status) = verify(&dir, "a.fsl");
    assert_eq!(status, 0, "{standalone}");
    assert_eq!(standalone["result"], "verified", "{standalone}");
    let (composed, status) = check(&dir, "c.fsl");
    assert_eq!(status, 0, "{composed}");
    for (engine, expected) in [("bmc", "verified"), ("induction", "proved")] {
        let (output, status) = compose_scratch::run(
            &dir,
            &[
                "verify",
                "c.fsl",
                "--engine",
                engine,
                "--depth",
                "3",
                "--no-cache",
            ],
        );
        eprintln!("{engine} exit={status} output={output}");
        assert_eq!(
            (status, output["result"].as_str()),
            (0, Some(expected)),
            "{output}"
        );
    }
    assert_eq!(
        lowered_helper(&dir)
            .args
            .iter()
            .map(Expr::kernel_ast_v1)
            .collect::<Vec<_>>(),
        vec![json!(["var", "a__ONE"])]
    );
}

#[test]
fn preservation_compose_helpful_physical_name_already_works() {
    let component = "spec A {
  state { on: Bool }
  init { on = false }
  fair action flip() { on = true }
}
";
    let source = "compose C {
  use A as a from \"a.fsl\"
  leadsTo Progress { not a.on ~> a.on helpful a__flip() decreases if a.on then 0 else 1 }
}
";
    let dir = scratch(
        "issue-1333-preservation",
        &[("a.fsl", component), ("c.fsl", source)],
    );
    let (checked, status) = check(&dir, "c.fsl");
    assert_eq!(status, 0, "{checked}");
    let (verified, status) = verify(&dir, "c.fsl");
    assert_eq!(status, 0, "{verified}");
    assert_eq!(verified["result"], "verified", "{verified}");
    let (induction, status) = verify_induction(&dir, "c.fsl");
    assert_eq!(status, 0, "{induction}");
    assert_eq!(induction["result"], "proved", "{induction}");
    assert_eq!(
        induction["leads_to"]["Progress"]["proof"], "ranking",
        "{induction}"
    );
}
