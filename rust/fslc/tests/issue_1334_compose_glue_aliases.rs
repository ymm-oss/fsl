// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1334: ordinary compose actions resolve aliases in every grammar-admitted
//! expression, parameter, statement and assignment target. Before the fix
//! reads fail with `cannot type identifier 'a'`, writes with `unknown update
//! target 'a'`, and typed params with `qualified type remained`. Physical-name
//! controls establish each intended verdict before asserting the alias form.
//! Shared target traversal must also resolve local-map indices in sync/init,
//! WITHOUT permitting component writes from those two restricted contexts.

#[path = "support/compose_scratch.rs"]
mod compose_scratch;

use compose_scratch::{check, scratch, verify};
use serde_json::{Value, json};

fn component(on: bool) -> String {
    format!(
        "spec A {{
  const LOW = 0
  const HIGH = 1
  type Id = 0..1
  state {{ on: Bool }}
  init {{ on = {on} }}
  action stay() {{ }}
}}
"
    )
}

fn compose(body: &str) -> String {
    format!("compose C {{\n  use A as a from \"a.fsl\"\n{body}\n}}\n")
}

fn verdict(result: &(Value, i32), expected: &str) {
    assert_eq!(result.1, i32::from(expected == "violated"), "{}", result.0);
    assert_eq!(result.0["result"], expected, "{}", result.0);
}

fn calibrated_cases(cases: &[(&str, &str, &str)]) -> Vec<Value> {
    // Calibrate ALL controls before asserting any alias result. Otherwise a
    // red first input would hide the opposite-polarity negative control.
    let mut controls = Vec::new();
    for &(component, body, expected) in cases {
        let source = compose(body);
        // Rewrite references only, never the use path or sync references.
        let physical = compose(
            &body
                .replace("a.on", "a__on")
                .replace("a.Id", "a__Id")
                .replace("a.LOW", "a__LOW")
                .replace("a.HIGH", "a__HIGH"),
        );
        let dir = scratch(
            "issue-1334",
            &[
                ("a.fsl", component),
                ("c.fsl", &source),
                ("physical.fsl", &physical),
            ],
        );
        let (checked, status) = check(&dir, "physical.fsl");
        assert_eq!(status, 0, "invalid physical-name control: {checked}");
        let control = verify(&dir, "physical.fsl");
        verdict(&control, expected);
        eprintln!(
            "{} physical control expected={expected} produced={} exit={} invariant={} kind={} last_action={} coverage={}",
            std::thread::current().name().expect("test thread"),
            control.0["result"],
            control.1,
            control.0["invariant"],
            control.0["violation_kind"],
            control.0["last_action"],
            control.0["action_coverage"]
        );
        controls.push((dir, expected));
    }
    controls
        .into_iter()
        .map(|(dir, expected)| {
            let (checked, status) = check(&dir, "c.fsl");
            assert_eq!(status, 0, "expected alias resolution, got {checked}");
            let result = verify(&dir, "c.fsl");
            verdict(&result, expected);
            result.0
        })
        .collect()
}

fn calibrated_case(component: &str, body: &str, expected: &str) -> Value {
    calibrated_cases(&[(component, body, expected)])
        .pop()
        .expect("one case")
}

fn boolean_cases(body: &str, off: &str, on: &str) -> Vec<Value> {
    calibrated_cases(&[(&component(false), body, off), (&component(true), body, on)])
}

fn scalar_body(action: &str, property: &str) -> String {
    format!("  state {{ seen: Bool }}\n  init {{ seen = false }}\n  {action}\n  {property}")
}

fn scalar_read(action: &str) {
    let body = scalar_body(action, "invariant NotSeen { not seen }");
    for (on, result) in [false, true]
        .into_iter()
        .zip(boolean_cases(&body, "verified", "violated"))
    {
        if on {
            assert_eq!(result["invariant"], "NotSeen", "{result}");
            assert_eq!(result["last_action"]["name"], "glue", "{result}");
        }
    }
}

#[test]
fn a1334_1_requires_really_disables_the_glue_action() {
    let body = scalar_body(
        "action glue() { requires a.on seen = true }",
        "invariant NotSeen { not seen }",
    );
    let results = boolean_cases(&body, "verified", "violated");
    let disabled = &results[0];
    assert_eq!(
        disabled["action_coverage"]["glue"]["covered"], false,
        "{disabled}"
    );
    let enabled = &results[1];
    assert_eq!(enabled["invariant"], "NotSeen", "{enabled}");
    assert_eq!(enabled["last_action"]["name"], "glue", "{enabled}");
}

#[test]
fn a1334_2_glue_component_write_can_violate_component_invariant() {
    let part = component(false).replacen(
        "  action stay()",
        "  invariant Off { not on }\n  action stay()",
        1,
    );
    let result = calibrated_case(&part, "  action glue() { a.on = true }", "violated");
    assert_eq!(result["invariant"], "a.Off", "{result}");
    assert_eq!(result["last_action"]["name"], "glue", "{result}");
}

#[test]
fn a1334_3_ensures_reads_component_post_state() {
    let body = scalar_body("action glue() { seen = true ensures a.on }", "");
    for (on, result) in [false, true]
        .into_iter()
        .zip(boolean_cases(&body, "violated", "verified"))
    {
        if !on {
            assert_eq!(result["violation_kind"], "ensures", "{result}");
            assert_eq!(result["last_action"]["name"], "glue", "{result}");
        }
    }
}

#[test]
fn a1334_3_let_reads_component_state() {
    scalar_read("action glue() { let enabled = a.on seen = enabled }");
}

#[test]
fn a1334_3_if_reads_component_state() {
    scalar_read("action glue() { if a.on { seen = true } else { seen = false } }");
}

#[test]
fn a1334_rhs_reads_component_state() {
    scalar_read("action glue() { seen = a.on }");
}

#[test]
fn a1334_parameter_type_resolves_component_type() {
    let body = scalar_body(
        "action glue(i: a.Id) { seen = i == 1 }",
        "invariant NotSeen { not seen }",
    );
    let result = calibrated_case(&component(false), &body, "violated");
    assert_eq!(result["last_action"]["params"]["i"], 1, "{result}");
}

#[test]
fn a1334_parameter_range_resolves_component_constants() {
    let body = scalar_body(
        "action glue(i in a.LOW..a.HIGH) { seen = i == 1 }",
        "invariant NotSeen { not seen }",
    );
    let result = calibrated_case(&component(false), &body, "violated");
    assert_eq!(result["last_action"]["params"]["i"], 1, "{result}");
}

#[test]
fn a1334_forall_type_and_where_resolve_aliases() {
    let body = "  state { seen: Map<a__Id, Bool> }
  init { forall i: a.Id { seen[i] = false } }
  action glue() { forall i: a.Id where a.on { seen[i] = true } }
  invariant NotSeen { forall i: a.Id { not seen[i] } }";
    boolean_cases(body, "verified", "violated");
}

fn local_map_index(route: &str) {
    let init = if route == "init" {
        "  init { seen[a.on] = not a.on }"
    } else {
        "  init { forall i in 0..1 { seen[i == 0] = false } }"
    };
    let action = match route {
        "glue" => "  action glue() { seen[a.on] = true }",
        "sync" => "  action glue() = a.stay() { seen[a.on] = true }",
        "init" => "",
        _ => panic!("unknown fixture route"),
    };
    let property = if route == "init" {
        "seen[a.on]"
    } else {
        "not seen[true]"
    };
    let body = format!(
        "  state {{ seen: Map<Bool, Bool> }}\n{init}\n{action}\n  invariant NotSeen {{ {property} }}"
    );
    boolean_cases(&body, "verified", "violated");
}

#[test]
fn a1334_index_glue_local_map() {
    local_map_index("glue");
}

#[test]
fn a1334_index_sync_addition_local_map() {
    local_map_index("sync");
}

#[test]
fn a1334_index_compose_init_local_map() {
    local_map_index("init");
}

fn reject_restricted_component_write(body: &str, target: &str) {
    let source = compose(body);
    let (line, column) = source
        .lines()
        .enumerate()
        .find_map(|(line, text)| {
            text.find(&format!("{target} ="))
                .map(|column| (line + 1, column + 1))
        })
        .expect("assignment location");
    let dir = scratch(
        "issue-1334-restricted",
        &[("a.fsl", &component(false)), ("c.fsl", &source)],
    );
    let (result, status) = check(&dir, "c.fsl");
    assert_eq!(status, 2, "{result}");
    assert_eq!(result["result"], "error", "{result}");
    assert_eq!(
        result["loc"],
        json!({"line": line, "column": column}),
        "{result}"
    );
    let message = result["message"]
        .as_str()
        .expect("located diagnostic message")
        .to_ascii_lowercase();
    assert!(
        message.contains("a.on") || message.contains("composition") || message.contains("compose"),
        "diagnostic must identify a.on or the compose-state write restriction: {result}"
    );
}

#[test]
fn a1334_sync_addition_rejects_component_write_at_assignment() {
    reject_restricted_component_write(
        "  action together() = a.stay() {\n    a.on = true\n  }",
        "a.on",
    );
}

#[test]
fn a1334_compose_init_rejects_component_write_at_assignment() {
    reject_restricted_component_write("  init {\n    a.on = true\n  }", "a.on");
}

#[test]
fn a1334_sync_addition_rejects_physical_component_write_at_assignment() {
    reject_restricted_component_write(
        "  action together() = a.stay() {\n    a__on = true\n  }",
        "a__on",
    );
}

fn shadowed_alias_case(body: &str) {
    let part_b = "spec B {
  struct R { on: Bool }
  state { r: R }
  init { r = R { on: true } }
  action stay() { }
}";
    let source = |binding| {
        format!(
            "compose C {{\n  use A as a from \"a.fsl\"\n  use B as b from \"b.fsl\"\n{}\n}}",
            body.replace("BINDING", binding)
        )
    };
    let dir = scratch(
        "issue-1334-shadow",
        &[
            ("a.fsl", &component(false)),
            ("b.fsl", part_b),
            ("alias.fsl", &source("a")),
            ("control.fsl", &source("x")),
        ],
    );
    let control = verify(&dir, "control.fsl");
    verdict(&control, "violated");
    let result = verify(&dir, "alias.fsl");
    eprintln!(
        "shadow renamed={} alias={} exit={}",
        control.0["result"], result.0["result"], result.1
    );
    verdict(&result, "violated");
    assert_eq!(result.0["result"], control.0["result"]);
    assert_eq!(result.0["violation_kind"], control.0["violation_kind"]);
    assert_eq!(result.0["invariant"], control.0["invariant"]);
}

#[test]
fn a1334_shadow_glue_let_respects_lexical_binding() {
    shadowed_alias_case(
        "  state { seen: Bool }\n  init { seen = false }\n  action glue() { let BINDING = b.r seen = BINDING.on }\n  invariant NotSeen { not seen }",
    );
}

#[test]
fn a1334_shadow_sync_let_respects_lexical_binding() {
    shadowed_alias_case(
        "  state { seen: Bool }\n  init { seen = false }\n  action s() = b.stay() { let BINDING = b.r seen = BINDING.on }\n  invariant NotSeen { not seen }",
    );
}

#[test]
fn a1334_shadow_property_quantifier_respects_lexical_binding() {
    // A scalar has no `on` field. Binding `a` must not turn this invalid
    // property into an accepted read of the component's Boolean state.
    let source = |binding| {
        compose(&format!(
            "  invariant Off {{ forall {binding} in 0..1 {{ not {binding}.on }} }}"
        ))
    };
    let dir = scratch(
        "issue-1334-shadow-property",
        &[
            ("a.fsl", &component(false)),
            ("alias.fsl", &source("a")),
            ("control.fsl", &source("x")),
        ],
    );
    let control = check(&dir, "control.fsl");
    assert_eq!(control.1, 2, "{}", control.0);
    eprintln!("quantifier renamed={} exit={}", control.0, control.1);
    let result = check(&dir, "alias.fsl");
    assert_eq!(result.1, control.1, "{}", result.0);
    assert_eq!(result.0["result"], control.0["result"]);
    assert_eq!(result.0["kind"], control.0["kind"]);
    assert_eq!(result.0["message"], control.0["message"]);
    assert_eq!(result.0["loc"], control.0["loc"]);
}
