// SPDX-License-Identifier: Apache-2.0
//! Shared assertions for source-field freshness, not rule-conflict diagnostics.

use serde_json::{Value, json};

pub fn assert_refused(exit: i32, report: &Value, name: &str, file: &str, line: u64, column: u64) {
    assert_eq!(exit, 2, "{report:#}");
    assert_eq!(report["result"], "error", "{report:#}");
    assert_eq!(report["kind"], "semantics", "{report:#}");
    assert_eq!(
        report["message"],
        format!(
            "pattern binding '{name}' in 'is some({name})' shadows an entity field of the same name; rename the pattern variable at {file}:{line}:{column}"
        ),
        "{report:#}"
    );
    assert_eq!(
        report["loc"],
        json!({"line": line, "column": column}),
        "{report:#}"
    );
    assert!(report.get("diagnostic_code").is_none(), "{report:#}");
}
