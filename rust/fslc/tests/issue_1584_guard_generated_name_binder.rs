// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1584: rule guards must rename binders named like generated state maps,
//! just as the event's main lowering does before expanding predicate calls.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

fn source(entity: &str, field: &str, def_binder: &str, rule_binder: &str) -> String {
    format!(
        r#"fsl 5
requirements R {{
  number Amount
  def same(x: {entity}) = exists {def_binder}: {entity} {{ {def_binder} == x }}
  actor Manager
  entity {entity} {{
    {field}: Amount = 0
    lifecycle Draft -> Approved
  }}
  event approve = Manager approves {entity} Draft -> Approved
  rule REQ-1 "x" {{
    allow Manager approves {entity} only when exists {rule_binder}: {entity} {{ same({rule_binder}) }}
  }}
}}
verify {{
  instances {entity} = 2
  values Amount = 0..2
}}
"#
    )
}

fn assert_accepts(label: &str, entity: &str, field: &str, def_binder: &str, rule_binder: &str) {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1584_guard_generated_name_binder")
        .join(std::process::id().to_string())
        .join(label);
    std::fs::create_dir_all(&directory).expect("create owned fixture directory");
    std::fs::write(
        directory.join("rule.fsl"),
        source(entity, field, def_binder, rule_binder),
    )
    .expect("write rule fixture");

    // Collect both modes before asserting so a check-time rejection does not
    // hide verify's result. Retain complete observations, including stderr.
    let outputs = ["check", "verify"].map(|mode| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args([mode, "rule.fsl"]);
        if mode == "verify" {
            command.args(["--depth", "2", "--no-cache"]);
        }
        let output = command
            .current_dir(&directory)
            .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run fslc");
        let value: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{label} {mode}: invalid JSON {error}: {output:?}"));
        let exit = output.status.code().expect("process exit code");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        std::fs::write(
            directory.join(format!("{mode}.json")),
            serde_json::to_vec_pretty(&json!({
                "exit": exit,
                "output": value,
                "stderr": stderr,
            }))
            .expect("serialize complete observation"),
        )
        .expect("retain observation");
        (mode, exit, value, stderr)
    });

    for (mode, exit, value, stderr) in &outputs {
        assert!(stderr.is_empty(), "{label} {mode}: {outputs:#?}");
        let expected = if *mode == "check" { "ok" } else { "verified" };
        assert_eq!(
            (
                *exit,
                value["result"].as_str(),
                value.get("diagnostic_code")
            ),
            (0, Some(expected), None),
            "{label}: expected {mode}={expected}/0; complete observations: {outputs:#?}"
        );
        if *mode == "verify" {
            assert_eq!(value["checked_to_depth"], 2, "{label}: {value:#}");
            assert_eq!(value["completeness"], "bounded", "{label}: {value:#}");
            assert_eq!(
                value["action_coverage"]["approve"], true,
                "{label}: {value:#}"
            );
        }
    }
}

macro_rules! accepts {
    ($name:ident, $entity:expr, $field:expr, $def_binder:expr, $rule_binder:expr) => {
        #[test]
        fn $name() {
            assert_accepts(
                stringify!($name),
                $entity,
                $field,
                $def_binder,
                $rule_binder,
            );
        }
    };
}

// naming::process_field_state / process_stage_state generate these names.
// Order.total also detects implementations hardcoded to Claim or to amount.
accepts!(
    detector_claim_amount,
    "Claim",
    "amount",
    "claim_amount",
    "claim_amount"
);
accepts!(
    detector_claim_stage,
    "Claim",
    "amount",
    "claim_stage",
    "claim_stage"
);
accepts!(
    detector_order_total,
    "Order",
    "total",
    "order_total",
    "order_total"
);
accepts!(
    detector_order_stage,
    "Order",
    "total",
    "order_stage",
    "order_stage"
);

// Change only the outer binder to d. Keeping the def's binder unchanged
// isolates main-path alpha-renaming, rather than asking it to fix real capture
// by a def whose own binder is also d.
accepts!(
    control_d_claim_amount,
    "Claim",
    "amount",
    "claim_amount",
    "d"
);
accepts!(control_d_claim_stage, "Claim", "amount", "claim_stage", "d");
accepts!(control_d_order_total, "Order", "total", "order_total", "d");
accepts!(control_d_order_stage, "Order", "total", "order_stage", "d");
