// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fslc-issue-450-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("create scratch directory");
    path
}

fn write(directory: &Path, name: &str, source: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, source).expect("write fixture");
    path
}

fn refine(implementation: &Path, abstraction: &Path, mapping: &Path) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "refine",
            implementation.to_str().expect("utf-8 implementation path"),
            abstraction.to_str().expect("utf-8 abstraction path"),
            mapping.to_str().expect("utf-8 mapping path"),
            "--depth",
            "1",
        ])
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).expect("native JSON output");
    (value, output.status.code().expect("exit status"))
}

fn assert_positive_and_missing_row(
    name: &str,
    files: &[(&str, &str)],
    implementation_name: &str,
    abstraction_name: &str,
    mapping_source: &str,
    removed_row: &str,
) {
    let directory = scratch(name);
    for (path, source) in files {
        write(&directory, path, source);
    }
    let implementation = directory.join(implementation_name);
    let abstraction = directory.join(abstraction_name);
    let mapping = write(&directory, "mapping.fsl", mapping_source);
    let (success, status) = refine(&implementation, &abstraction, &mapping);
    assert_eq!(status, 0, "{name}: {success}");
    assert_eq!(success["result"], "refines", "{name}: {success}");

    let incomplete_source = mapping_source.replacen(removed_row, "", 1);
    assert_ne!(
        incomplete_source, mapping_source,
        "row must exist in fixture"
    );
    let incomplete = write(&directory, "mapping-incomplete.fsl", &incomplete_source);
    let (failure, status) = refine(&implementation, &abstraction, &incomplete);
    assert_eq!(status, 2, "{name}: {failure}");
    assert_eq!(failure["kind"], "type", "{name}: {failure}");
    assert!(failure["loc"].is_object(), "{name}: {failure}");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("missing source"))
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn generated_and_renamed_enum_paths_use_the_same_checked_conversion() {
    assert_positive_and_missing_row(
        "db-column",
        &[
            (
                "impl.fsl",
                r"spec DbDesign {
  type DesignVersion = 0..0
  enum DesignColumn { col_users_id }
  state {
    version: DesignVersion,
    d_exists: Map<DesignColumn, Bool>,
    d_backfilled: Map<DesignColumn, Bool>,
    d_not_null: Map<DesignColumn, Bool>
  }
  init {
    version = 0
    forall c: DesignColumn {
      d_exists[c] = true
      d_backfilled[c] = true
      d_not_null[c] = true
    }
  }
  action observe_schema_0() { version = 0 }
}",
            ),
            (
                "abs.fsl",
                r"dbsystem DbAbs {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; }
  }
}",
            ),
        ],
        "impl.fsl",
        "abs.fsl",
        r"refinement DbR {
  impl DbDesign
  abs DbAbs
  enum conversion column_key Column -> DesignColumn {
    col_users_id -> col_users_id
  }
  map schema_version = version
  map column_exists[c: Column] = d_exists[convert(column_key, c)]
  map column_backfilled[c: Column] = d_backfilled[convert(column_key, c)]
  map column_not_null[c: Column] = d_not_null[convert(column_key, c)]
  action observe_schema_0() -> observe_schema_0()
}",
        "    col_users_id -> col_users_id\n",
    );

    assert_positive_and_missing_row(
        "domain-effect",
        &[
            (
                "impl.fsl",
                r"spec DomainDesign {
  type RequestId = 0..0
  type DesignAttempt = 0..1
  enum DesignCaptureStatus {
    CaptureEffectStatus_NotStarted,
    CaptureEffectStatus_Pending,
    CaptureEffectStatus_Succeeded,
    CaptureEffectStatus_Failed,
    CaptureEffectStatus_TimedOut,
    CaptureEffectStatus_Cancelled,
    CaptureEffectStatus_Compensated
  }
  state {
    status: Map<RequestId, DesignCaptureStatus>,
    attempts: Map<RequestId, DesignAttempt>,
    requested: Bool
  }
  init {
    forall r: RequestId {
      status[r] = CaptureEffectStatus_NotStarted
      attempts[r] = 0
    }
    requested = false
  }
  action request(r: RequestId) {
    status[r] = CaptureEffectStatus_Pending
    attempts[r] = attempts[r] + 1
    requested = true
  }
}",
            ),
            (
                "abs.fsl",
                r"domain DomainAbs {
  implementation_profile functional_ddd
  type OrderId = 0..0
  type RequestId = 0..0
  enum Status { Draft, Done, Failed }
  aggregate Order {
    id OrderId
    state { status: Status = Draft; }
    command Request { request_id: RequestId }
    event Requested { request_id: RequestId }
    event Captured { request_id: RequestId }
    event Failed { request_id: RequestId }
    decide Request { requires status == Draft emits Requested }
    evolve Requested { status = Draft }
    evolve Captured { status = Done }
    evolve Failed { status = Failed }
  }
  effect Capture {
    async
    idempotency_key Order.id
    correlation_id Requested.request_id
    handles Requested
    emits one_of [Captured, Failed]
    retry { max_attempts 1 }
  }
}",
            ),
        ],
        "impl.fsl",
        "abs.fsl",
        r"refinement DomainR {
  impl DomainDesign
  abs DomainAbs
  enum conversion capture_status DesignCaptureStatus -> CaptureEffectStatus {
    CaptureEffectStatus_NotStarted -> CaptureEffectStatus_NotStarted
    CaptureEffectStatus_Pending -> CaptureEffectStatus_Pending
    CaptureEffectStatus_Succeeded -> CaptureEffectStatus_Succeeded
    CaptureEffectStatus_Failed -> CaptureEffectStatus_Failed
    CaptureEffectStatus_TimedOut -> CaptureEffectStatus_TimedOut
    CaptureEffectStatus_Cancelled -> CaptureEffectStatus_Cancelled
    CaptureEffectStatus_Compensated -> CaptureEffectStatus_Compensated
  }
  map capture_attempts[r: RequestId] = attempts[r]
  map capture_status[r: RequestId] = convert(capture_status, status[r])
  map event_Captured = false
  map event_Failed = false
  map event_Requested = requested
  map order_status = Status_Draft
  action request(r) -> order_request(r)
}",
        "    CaptureEffectStatus_Compensated -> CaptureEffectStatus_Compensated\n",
    );

    assert_positive_and_missing_row(
        "compose-renamed",
        &[
            (
                "pay_component.fsl",
                r"spec PayComponent {
  type PayId = 0..0
  enum PSt { Created, Captured }
  state { payment: Map<PayId, PSt> }
  init { forall p: PayId { payment[p] = Created } }
  action capture(p: PayId) {
    requires payment[p] == Created
    payment[p] = Captured
  }
}",
            ),
            (
                "impl.fsl",
                r#"compose PayCompose {
  use PayComponent as pay from "pay_component.fsl"
  action do(p: pay.PayId) = pay.capture(p) { }
  internal pay.capture
}"#,
            ),
            (
                "abs.fsl",
                r"spec PayAbs {
  type PayId = 0..0
  enum AbsPSt { Created, Captured }
  state { status: Map<PayId, AbsPSt> }
  init { forall p: PayId { status[p] = Created } }
  action do(p: PayId) {
    requires status[p] == Created
    status[p] = Captured
  }
}",
            ),
        ],
        "impl.fsl",
        "abs.fsl",
        r"refinement PayR {
  impl PayCompose
  abs PayAbs
  enum conversion payment_status pay__PSt -> AbsPSt {
    Created -> Created
    Captured -> Captured
  }
  map status[p: PayId] = convert(payment_status, pay__payment[p])
  action do(p) -> do(p)
}",
        "    Captured -> Captured\n",
    );
}
