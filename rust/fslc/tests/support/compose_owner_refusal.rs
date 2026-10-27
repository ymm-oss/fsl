// SPDX-License-Identifier: Apache-2.0

//! Independent #1463 diagnostic envelope expectation, derived from a live
//! standalone check, not from the compose diagnostic being tested.

use serde_json::{Value, json};

pub fn checks(
    direct: &(Value, i32),
    composed: &(Value, i32),
    component: &str,
    declaration: &str,
    compose_file: &str,
    use_location: (u32, u32),
) -> [bool; 3] {
    assert_eq!(direct.1, 2, "standalone must actually refuse: {}", direct.0);
    assert_eq!(direct.0["result"], "error");
    let line = direct.0["loc"]["line"].as_u64().expect("source line");
    let column = direct.0["loc"]["column"].as_u64().expect("source column");
    let message = direct.0["message"].as_str().expect("source message");
    // Both observed #1463 envelopes: Kernel expression diagnostics already
    // render their location; standalone wrapper-binder diagnostics do not.
    let (prefix, inner, outer) =
        if let Some(body) = message.strip_prefix("invalid model expression: ") {
            (
                "invalid model expression: ",
                body.strip_suffix(&format!(" at {line}:{column}"))
                    .expect("located Kernel expression diagnostic"),
                format!("{}:{}", use_location.0, use_location.1),
            )
        } else {
            (
                "",
                message,
                format!("{compose_file}:{}:{}", use_location.0, use_location.1),
            )
        };
    let mut expected = direct.0.clone();
    expected["message"] = json!(format!(
        "{prefix}component \"{component}\" failed to check ({declaration}: {inner} at {component}:{line}:{column}) at {outer}"
    ));
    expected["loc"] = json!({"line":use_location.0,"column":use_location.1});
    [
        composed.1 == 2 && composed.0["result"] == "error",
        composed.0["kind"] == direct.0["kind"],
        // Preserve ALL fields, with only the documented wrapping transformation.
        composed.0 == expected,
    ]
}

pub fn assert_preserved(failures: [usize; 3]) {
    assert_eq!(failures[0], 0, "owner refused but compose accepted");
    assert_eq!(failures[1], 0, "owner refusal kind changed");
    assert_eq!(
        failures[2], 0,
        "owner inner diagnostic/component location changed"
    );
}
