// SPDX-License-Identifier: Apache-2.0

//! Definition-local patterns must retain the existing collision diagnostic.

#[path = "support/compose_scratch.rs"]
#[allow(dead_code)]
mod compose_scratch;

use serde_json::{Value, json};

fn spec(state: &str, init: &str, definition: &str, action: &str) -> String {
    format!(
        "spec D {{\n  type K = 0..1\n  state {{ {state} }}\n  init {{ {init} }}\n  def {definition}\n  {action}\n  invariant P {{ true }}\n}}\n"
    )
}

fn check(source: &str) -> (Value, i32) {
    let dir = compose_scratch::scratch("issue-1450-pattern-scope", &[("s.fsl", source)]);
    compose_scratch::check(&dir, "s.fsl")
}

fn error(source: &str, kind: &str) -> Value {
    let (output, status) = check(source);
    assert_eq!(status, 2, "{output}");
    assert_eq!(output["result"], "error", "{output}");
    assert_eq!(output["kind"], kind, "{output}");
    output
}

fn assert_pattern(source: &str, name: &str, collision: &str, column: u32) {
    let output = error(source, "semantics");
    assert_eq!(
        output["message"],
        format!(
            "invalid model expression: pattern binding '{name}' in 'is some({name})' shadows {collision} of the same name; rename the pattern variable at 6:{column}"
        )
    );
    assert_eq!(output["loc"], json!({"line": 6, "column": column}));
}

#[test]
fn definition_own_pattern_keeps_parameter_collision_message_and_location() {
    let source = spec(
        "o: Option<K>, seen: Bool",
        "o = some(1) seen = false",
        "p() = (o is some(v)) and v == 1",
        "action g(v: K) { seen = p() }",
    );
    assert_pattern(
        &source,
        "v",
        "a name already in scope (an action parameter, quantifier binder, let, or earlier pattern binding)",
        20,
    );
}

#[test]
fn definition_own_pattern_keeps_state_collision_message_and_location() {
    let source = spec(
        "on: K, o: Option<K>, seen: Bool",
        "on = 0 o = some(1) seen = false",
        "p() = (o is some(on)) and on == 1",
        "action g() { let on = 1 seen = p() }",
    );
    assert_pattern(&source, "on", "a state variable", 27);
}

#[test]
fn repeated_definition_local_patterns_keep_the_existing_collision() {
    let source = spec(
        "o: Option<K>, seen: Bool",
        "o = some(1) seen = false",
        "p() = (o is some(v)) and v == 1",
        "action g(w: K) { seen = p() and p() }",
    );
    assert_pattern(
        &source,
        "v",
        "a name already in scope (an action parameter, quantifier binder, let, or earlier pattern binding)",
        20,
    );
    let single = source.replace("p() and p()", "p()");
    let (output, status) = check(&single);
    assert_eq!(status, 0, "{output}");
}

#[test]
fn non_global_caller_patterns_in_guards_and_binary_left_capture_free_names() {
    for body in [
        "requires o is some(v) seen = p(false)",
        "seen = (o is some(v)) and p(false)",
    ] {
        let source = spec(
            "o: Option<K>, seen: Bool",
            "o = some(1) seen = false",
            "p(z: Bool) = (v == 1) or z",
            &format!("action g() {{ {body} }}"),
        );
        let output = error(&source, "semantics");
        assert!(
            output["message"]
                .as_str()
                .unwrap()
                .contains("would capture variable 'v'"),
            "{output}"
        );
    }
}

#[test]
fn declaration_level_property_helper_capture_is_an_explicit_remaining_exception() {
    let source = spec(
        "on: K, seen: Bool",
        "on = 0 seen = false",
        "p() = on",
        "fair action g(k: K) { seen = true }\n  leadsTo L { forall on: K { seen ~> true } helpful g(p()) decreases p() }",
    );
    // Preservation of the explicitly deferred path, not a capture detector.
    let (output, status) = check(&source);
    assert_eq!(status, 0, "{output}");
}

#[test]
fn property_binder_where_still_checks_capture() {
    let source = spec(
        "on: K, seen: Bool",
        "on = 0 seen = false",
        "p() = on == 0",
        "action g() { seen = true }\n  leadsTo L { forall on in 0..1 where p() { seen ~> true } }",
    );
    let output = error(&source, "semantics");
    assert!(
        output["message"]
            .as_str()
            .unwrap()
            .contains("would capture variable 'on'"),
        "{output}"
    );
}

#[test]
fn deferred_parameter_order_change_retains_exit_and_error_kind() {
    let source = "spec D {\n  state { seen: Bool }\n  init { seen = false }\n  action g(x in 0..bad1()) { seen = bad2() }\n  invariant P { true }\n}\n";
    let output = error(source, "name");
    assert_eq!(output["message"], "undefined predicate 'bad2' at 4:37");
}
