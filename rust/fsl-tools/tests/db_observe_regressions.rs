// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for `fslc db observe` runtime-observation gaps: #505
//! (envelope/event validation) and #506 (feature-flag snapshots).

use fsl_syntax::DbSystem;
use serde_json::{Value, json};

const SYSTEM: &str = r"
dbsystem Db505 {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column email_normalized: Text absent; }
  }
  migration add_email_normalized from 0 to 1 { add users.email_normalized nullable; }
  artifact server_legacy { responds response.email; }
  artifact server_new { reads users.email_normalized; responds response.email_normalized; }
  environment prod {
    schema 0..1;
    flag email_v2 { off, on } default off;
    active server_legacy when schema 0..1 when flag email_v2=off;
    active server_new when schema 1..1 when flag email_v2=on;
  }
}";

fn system() -> DbSystem {
    fsl_syntax::parse_db_system(SYSTEM).expect("parse dbsystem")
}

fn observe(payload: &Value) -> Result<Value, fsl_tools::DbToolError> {
    fsl_tools::observe_db(&system(), payload)
}

// #505: a wrong root `schema_version` used to be silently accepted (shared
// with the frozen Python reference), then the event evaluated as if it
// were the current envelope.
#[test]
fn observe_rejects_wrong_root_schema_version() {
    let error = observe(&json!({"schema_version": "wrong.v9", "events": []}))
        .expect_err("wrong root schema_version must be rejected");
    assert!(error.message.contains("wrong.v9"), "{}", error.message);
}

// #505: a non-object event used to be defaulted into a fabricated witness
// (empty strings, schema version 0) instead of being rejected.
#[test]
fn observe_rejects_non_object_event() {
    let error = observe(&json!({"schema_version": "fsl-db-observation.v0", "events": [null]}))
        .expect_err("a null event must be rejected");
    assert!(error.message.contains("event 0"), "{}", error.message);
    assert!(error.message.contains("object"), "{}", error.message);
}

// #505: a missing required field used to be defaulted (e.g. capability
// silently treated as empty) instead of raising a located parse error.
#[test]
fn observe_rejects_event_missing_required_field() {
    let error = observe(&json!({"events": [{
        "environment": "prod",
        "schema_version": 0,
        "artifact": "server_legacy",
        "target": "users.id"
    }]}))
    .expect_err("a missing 'capability' field must be rejected");
    assert!(error.message.contains("capability"), "{}", error.message);
}

// #505: an unknown capability value outside the closed vocabulary
// (reads/writes/calls/requires/provides) used to pass through untyped.
#[test]
fn observe_rejects_unknown_capability() {
    let error = observe(&json!({"events": [{
        "environment": "prod",
        "schema_version": 0,
        "artifact": "server_legacy",
        "capability": "deletes",
        "target": "users.id"
    }]}))
    .expect_err("an unknown capability must be rejected");
    assert!(error.message.contains("deletes"), "{}", error.message);
}

// #505 control: a well-formed envelope/event must still validate and
// evaluate normally (this is the shape `docs/DESIGN-db.md` documents).
#[test]
fn observe_accepts_well_formed_envelope() {
    let result = observe(
        &json!({"schema_version": "fsl-db-observation.v0", "events": [{
            "environment": "prod",
            "schema_version": 1,
            "artifact": "server_new",
            "capability": "reads",
            "target": "users.email_normalized",
            "flags": {"email_v2": "on"}
        }]}),
    )
    .expect("well-formed observation must be accepted");
    assert_eq!(result["result"], "observed_conformant", "{result}");
}

// #506: `server_new` is only declared active at schema 1 under
// `email_v2=on`. An event claiming schema 1 but `email_v2=off` used to be
// reported conformant because the observer ignored the flag snapshot
// entirely.
#[test]
fn observe_flag_mismatch_is_unsupported_artifact() {
    let result = observe(
        &json!({"schema_version": "fsl-db-observation.v0", "events": [{
            "environment": "prod",
            "schema_version": 1,
            "artifact": "server_new",
            "capability": "reads",
            "target": "users.email_normalized",
            "flags": {"email_v2": "off"}
        }]}),
    )
    .expect("well-formed observation must be accepted");
    assert_eq!(result["result"], "observed_mismatch", "{result}");
    assert_eq!(
        result["findings"][0]["kind"],
        "unsupported_artifact_observed"
    );
    assert_eq!(
        result["findings"][0]["witness"]["flags"],
        json!({"email_v2": "off"})
    );
}

// #506 control: the matching flag variant must stay conformant (paired
// with the mismatch test above so a fix cannot just reject every flagged
// observation).
#[test]
fn observe_flag_match_is_conformant() {
    let result = observe(
        &json!({"schema_version": "fsl-db-observation.v0", "events": [{
            "environment": "prod",
            "schema_version": 1,
            "artifact": "server_new",
            "capability": "reads",
            "target": "users.email_normalized",
            "flags": {"email_v2": "on"}
        }]}),
    )
    .expect("well-formed observation must be accepted");
    assert_eq!(result["result"], "observed_conformant", "{result}");
}
