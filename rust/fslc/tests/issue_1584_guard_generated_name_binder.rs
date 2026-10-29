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
    assert_source_accepts(label, &source(entity, field, def_binder, rule_binder));
}

fn assert_source_accepts(label: &str, source: &str) {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1584_guard_generated_name_binder")
        .join(std::process::id().to_string())
        .join(label);
    std::fs::create_dir_all(&directory).expect("create owned fixture directory");
    std::fs::write(directory.join("rule.fsl"), source).expect("write rule fixture");

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

fn second_field_source(rule_binder: &str) -> String {
    format!(
        r#"fsl 5
requirements R {{
  number Amount
  def same(x: Claim) = exists claim_limit: Claim {{ claim_limit == x }}
  actor Manager
  entity Claim {{
    amount: Amount = 0
    limit: Amount = 0
    lifecycle Draft -> Approved
  }}
  event approve = Manager approves Claim Draft -> Approved
  rule REQ-1 "x" {{
    allow Manager approves Claim only when exists {rule_binder}: Claim {{ same({rule_binder}) }}
  }}
}}
verify {{
  instances Claim = 2
  values Amount = 0..2
}}
"#
    )
}

// A fix that gathers only the first declared field must fail this detector.
#[test]
fn detector_second_field_claim_limit() {
    assert_source_accepts(
        "detector_second_field_claim_limit",
        &second_field_source("claim_limit"),
    );
}

#[test]
fn control_d_second_field_claim_limit() {
    assert_source_accepts(
        "control_d_second_field_claim_limit",
        &second_field_source("d"),
    );
}

fn relation_role_source(rule_binder: &str) -> String {
    format!(
        r#"fsl 5
requirements R {{
  number Qty
  def same(x: Order) = exists order_buyer: Order {{ order_buyer == x }}
  actor User, Shop
  entity User
  entity Order {{
    qty: Qty = 0
    lifecycle Empty -> Placed -> Approved
  }}
  Order belongs to User as buyer via place
  event place = User places Order Empty -> Placed
  event approve = Shop approves Order Placed -> Approved
  rule REQ-1 "x" {{
    allow Shop approves Order only when exists {rule_binder}: Order {{ same({rule_binder}) }}
  }}
}}
verify {{
  instances Order = 2
  instances User = 1
  values Qty = 0..1
}}
"#
    )
}

// Relation roles also contribute generated entity state maps.
#[test]
fn detector_relation_role_order_buyer() {
    assert_source_accepts(
        "detector_relation_role_order_buyer",
        &relation_role_source("order_buyer"),
    );
}

#[test]
fn control_d_relation_role_order_buyer() {
    assert_source_accepts(
        "control_d_relation_role_order_buyer",
        &relation_role_source("d"),
    );
}

#[test]
fn reserved_relation_role_cannot_collide_with_guard_fresh_name() {
    let source = r#"fsl 5
requirements R {
  number Amount
  def same(x: Claim) = exists __claim_amount_substitution_0: Claim { __claim_amount_substitution_0 == x }
  actor User
  entity User
  entity Claim {
    amount: Amount = 0
    lifecycle Draft -> Approved
  }
  Claim belongs to User as __claim_amount_substitution_0 via approve
  event approve = User approves Claim Draft -> Approved
  rule REQ-1 "x" {
    allow User approves Claim only when exists claim_amount: Claim { same(claim_amount) }
  }
}
verify {
  instances User = 2
  instances Claim = 2
  values Amount = 0..2
}
"#;
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1584_guard_generated_name_binder")
        .join(std::process::id().to_string())
        .join("reserved_relation_role");
    std::fs::create_dir_all(&directory).expect("create owned fixture directory");
    std::fs::write(directory.join("rule.fsl"), source).expect("write rule fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "rule.fsl"])
        .current_dir(&directory)
        .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON {error}: {output:?}"));
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.status.code(), Some(2), "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "name", "{value:#}");
    assert!(
        value["message"].as_str().is_some_and(|message| message.starts_with(
            "relation role name '__claim_amount_substitution_0' contains '__', which is reserved for the names FSL generates"
        )),
        "{value:#}"
    );
    assert_eq!(value["loc"], json!({"line": 11, "column": 28}));
}
