// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

// Attribution-bearing projection of verification.rs:583-608, 613-620, 648.
// Property-context CTIs deliberately have no last_action (539-552).
pub fn cti(kind: &str, name: &str, action: Option<&str>) -> Value {
    let mut value = json!({"result":"unknown_cti", "violation_kind":kind, "invariant":name,
        "trace_type":"induction_cti", "cti":{"states":[], "violated_at":1},
        "hint":"strengthen the obligation"});
    if let Some(action) = action {
        value["last_action"] = json!({"name":action});
    }
    value
}
