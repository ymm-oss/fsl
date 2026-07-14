// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::json;

#[test]
fn induction_drops_only_typed_deadlock_warnings() {
    let warnings = vec![
        json!({"kind": "vacuous_implication", "message": "mentions deadlock intentionally"}),
        json!({"kind": "deadlock", "message": "deadlock reachable at step 0"}),
        json!({"message": "action is never enabled"}),
    ];

    assert_eq!(
        fsl_runtime::induction_warnings(&warnings),
        vec![warnings[0].clone(), warnings[2].clone()]
    );
}
