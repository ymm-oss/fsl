// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for the `fslc db check` false-green/false-firing audit:
//! issues #490 (8 missing detections) and #491 (4 false firings), plus the
//! undocumented `failed_rule` name in #492. Each test pins one symptom
//! (`DB-NN` in the issue text) with, where applicable, both a violating and
//! a compliant variant so a fix that only silences or only adds findings
//! shows up as a failure here.

use fsl_syntax::DbSystem;
use serde_json::Value;

fn system(source: &str) -> DbSystem {
    fsl_syntax::parse_db_system(source).unwrap_or_else(|error| panic!("parse dbsystem: {error}"))
}

fn check(source: &str) -> Value {
    fsl_tools::check_db(&system(source)).unwrap_or_else(|error| panic!("check_db: {error}"))
}

fn kinds(result: &Value) -> Vec<&str> {
    result["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|finding| finding["kind"].as_str().expect("finding kind"))
        .collect()
}

fn failed_rules(result: &Value) -> Vec<&str> {
    result["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|finding| finding["failed_rule"].as_str().expect("failed_rule"))
        .collect()
}

// DB-01 (#490): an unknown `rule` name in `check compatibility` used to be
// accepted and silently select nothing, so a typo turned a real violation
// into a zero-obligation `verified_under_assumptions`.
#[test]
fn db01_unknown_rule_name_is_rejected() {
    const SOURCE: &str = r"
dbsystem Db01 {
  database app { schema 0 table users { column id: Int present backfilled not_null; } }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..0; active server_v1 when schema 0..0; }
  check compatibility { rule not_null_after_backfil; }
}";
    let error = fsl_tools::validate_db(&system(SOURCE)).expect_err("typo rule must be rejected");
    assert!(
        error
            .message
            .contains("unknown db compatibility rule 'not_null_after_backfil'"),
        "{}",
        error.message
    );
    assert!(
        error.message.contains("not_null_after_backfill"),
        "{}",
        error.message
    );
}

#[test]
fn db01_known_rule_name_is_accepted() {
    const SOURCE: &str = r"
dbsystem Db01Control {
  database app { schema 0 table users { column id: Int present backfilled not_null; } }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..0; active server_v1 when schema 0..0; }
  check compatibility { rule not_null_after_backfill; }
}";
    fsl_tools::validate_db(&system(SOURCE)).expect("documented rule name must be accepted");
}

// DB-02 (#490): an `active` artifact's own `calls`/`expects`/`emits_offline`
// declarations were never checked; the artifact was only ever consulted as
// a provider.
#[test]
fn db02_active_role_consumer_capabilities_are_checked() {
    const SOURCE: &str = r"
dbsystem Db02 {
  database app { schema 0 table users { column id: Int present backfilled not_null; } }
  artifact client_v1 { calls api.X; expects response.y; emits_offline api.Z ttl 1; }
  environment prod { schema 0..0; active client_v1 when schema 0..0; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    let mut found = kinds(&result);
    found.sort_unstable();
    assert_eq!(
        found,
        [
            "api_call_not_accepted",
            "api_response_field_missing",
            "offline_payload_not_accepted",
        ]
    );
}

// DB-03 (#490): `set_not_null` without a prior `backfill` made the kernel
// action unreachable (the violating trajectory was disabled, not reported),
// so the migration verified vacuously instead of failing
// `not_null_after_backfill`.
#[test]
fn db03_set_not_null_without_backfill_is_detected() {
    const SOURCE: &str = r"
dbsystem Db03 {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column email: Text absent; }
  }
  migration add_email from 0 to 1 { add users.email nullable; }
  migration require_email from 1 to 2 { set_not_null users.email; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..2; active server_v1 when schema 0..2; }
  check compatibility { rule not_null_after_backfill; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["not_null_before_backfill"]);
}

#[test]
fn db03_set_not_null_after_backfill_is_safe() {
    const SOURCE: &str = r"
dbsystem Db03Control {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column email: Text absent; }
  }
  migration add_email from 0 to 1 { add users.email nullable; }
  migration backfill_email from 1 to 2 { backfill users.email; }
  migration require_email from 2 to 3 { set_not_null users.email; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..3; active server_v1 when schema 0..3; }
  check compatibility { rule not_null_after_backfill; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

// DB-04 (#490): an `environment schema lo..hi` window could include a
// schema version never reached by the declared migration plan.
#[test]
fn db04_unreachable_environment_schema_is_rejected() {
    const SOURCE: &str = r"
dbsystem Db04 {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column nickname: Text absent; }
  }
  migration add_nickname from 0 to 1 { add users.nickname nullable; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..2; active server_v1 when schema 0..1; }
}";
    let error =
        fsl_tools::validate_db(&system(SOURCE)).expect_err("unreachable schema must be rejected");
    assert!(
        error
            .message
            .contains("environment 'prod' includes schema 2, which is not reachable"),
        "{}",
        error.message
    );
}

#[test]
fn db04_reachable_environment_schema_is_accepted() {
    const SOURCE: &str = r"
dbsystem Db04Control {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column nickname: Text absent; }
  }
  migration add_nickname from 0 to 1 { add users.nickname nullable; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
}";
    fsl_tools::validate_db(&system(SOURCE)).expect("reachable schema window must be accepted");
}

// DB-05 (#490): `rollback_not_equivalent` only ever fired for `drop`; a
// lossy rollbackable `split`/`merge` never produced it.
#[test]
fn db05_rollback_not_equivalent_fires_for_lossy_split() {
    const SOURCE: &str = r"
dbsystem Db05 {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column full_name: Text present backfilled nullable;
      column first_name: Text absent;
      column last_name: Text absent;
    }
  }
  migration split_full_name from 0 to 1 rollbackable {
    split users.full_name into users.first_name, users.last_name lossy;
  }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
  check compatibility { rule rollback_equivalent; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["rollback_not_equivalent"]);
}

#[test]
fn db05_rollback_equivalent_lossless_split_is_safe() {
    const SOURCE: &str = r"
dbsystem Db05Control {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column full_name: Text present backfilled nullable;
      column first_name: Text absent;
      column last_name: Text absent;
    }
  }
  migration split_full_name from 0 to 1 rollbackable {
    split users.full_name into users.first_name, users.last_name lossless;
  }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
  check compatibility { rule rollback_equivalent; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

// DB-06 (#490): `data_preserved` never fired for dropping an existing
// column (only split/merge were checked).
#[test]
fn db06_data_preservation_loss_fires_for_existing_column_drop() {
    const SOURCE: &str = r"
dbsystem Db06 {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column legacy: Text present backfilled nullable; }
  }
  migration drop_legacy from 0 to 1 { drop users.legacy irreversible; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
  check compatibility { rule data_preserved; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["data_preservation_loss"]);
}

#[test]
fn db06_dropping_an_already_absent_column_is_safe() {
    const SOURCE: &str = r"
dbsystem Db06Control {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column ghost: Text absent; }
  }
  migration drop_ghost from 0 to 1 { drop users.ghost irreversible; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
  check compatibility { rule data_preserved; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

// DB-07 (#491): a `supported` provider's `accepts`/`responds`/`provides`
// was ignored; only `active` counted as a provider.
#[test]
fn db07_supported_provider_is_honored() {
    const SOURCE: &str = r"
dbsystem Db07 {
  database app { schema 0 table users { column id: Int present backfilled not_null; } }
  artifact p { accepts api.X; responds response.y; provides cap.z; }
  artifact c { calls api.X; expects response.y; requires cap.z; }
  environment prod { schema 0..0; supported p when schema 0..0; may_exist c when schema 0..0; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

#[test]
fn db07_may_exist_is_still_not_a_provider() {
    const SOURCE: &str = r"
dbsystem Db07Control {
  database app { schema 0 table users { column id: Int present backfilled not_null; } }
  artifact p { accepts api.X; responds response.y; provides cap.z; }
  artifact c { calls api.X; expects response.y; requires cap.z; }
  environment prod { schema 0..0; may_exist p when schema 0..0; may_exist c when schema 0..0; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    let mut found = kinds(&result);
    found.sort_unstable();
    assert_eq!(
        found,
        [
            "api_call_not_accepted",
            "api_response_field_missing",
            "required_capability_missing",
        ]
    );
}

// DB-08 (#491): `destructive` (as opposed to `irreversible`) was never
// accepted, and a drop of an already-absent column still fired.
#[test]
fn db08_destructive_annotation_and_absent_column_are_safe() {
    const SOURCE: &str = r"
dbsystem Db08 {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column c: Text present backfilled nullable;
      column ghost: Text absent;
    }
  }
  migration drop_c from 0 to 1 { drop users.c destructive; drop users.ghost; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

#[test]
fn db08_genuinely_unannotated_existing_drop_is_still_caught() {
    const SOURCE: &str = r"
dbsystem Db08Control {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column c: Text present backfilled nullable; }
  }
  migration drop_c from 0 to 1 { drop users.c; }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["destructive_migration_unannotated"]);
}

// DB-09 (#491): `irreversible` was never accepted as a preservation
// classification for `split`/`merge` (only `lossless`/`lossy` were).
#[test]
fn db09_irreversible_split_annotation_is_accepted() {
    const SOURCE: &str = r"
dbsystem Db09 {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column full_name: Text present backfilled nullable;
      column first_name: Text absent;
      column last_name: Text absent;
    }
  }
  migration split_full_name from 0 to 1 {
    split users.full_name into users.first_name, users.last_name irreversible;
  }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
  check compatibility { rule preservation_transforms_annotated; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

#[test]
fn db09_unannotated_split_is_still_caught() {
    const SOURCE: &str = r"
dbsystem Db09Control {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column full_name: Text present backfilled nullable;
      column first_name: Text absent;
      column last_name: Text absent;
    }
  }
  migration split_full_name from 0 to 1 {
    split users.full_name into users.first_name, users.last_name;
  }
  artifact server_v1 { reads users.id; }
  environment prod { schema 0..1; active server_v1 when schema 0..1; }
  check compatibility { rule preservation_transforms_annotated; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["preservation_transform_unannotated"]);
}

// DB-10 (#491): only `destructive_operations_annotated` respected rule
// selection; every other finding kind (including the opt-in
// `data_preserved`/`rollback_equivalent`) fired unconditionally.
#[test]
fn db10_opt_in_rule_stays_off_when_not_selected() {
    const SOURCE: &str = r"
dbsystem Db10 {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column full_name: Text present backfilled nullable;
      column first_name: Text absent;
      column last_name: Text absent;
    }
  }
  migration split_full_name from 0 to 1 rollbackable {
    split users.full_name into users.first_name, users.last_name lossy;
  }
  artifact server_v1 { reads users.id, users.first_name, users.last_name; }
  environment prod { schema 0..1; active server_v1 when schema 1..1; }
  check compatibility { rule all_active_reads_exist; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

#[test]
fn db10_opt_in_rule_fires_when_selected() {
    const SOURCE: &str = r"
dbsystem Db10Control {
  database app {
    schema 0
    table users {
      column id: Int present backfilled not_null;
      column full_name: Text present backfilled nullable;
      column first_name: Text absent;
      column last_name: Text absent;
    }
  }
  migration split_full_name from 0 to 1 rollbackable {
    split users.full_name into users.first_name, users.last_name lossy;
  }
  artifact server_v1 { reads users.id, users.first_name, users.last_name; }
  environment prod { schema 0..1; active server_v1 when schema 1..1; }
  check compatibility { rule all_active_reads_exist; rule rollback_equivalent; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["rollback_not_equivalent"]);
}

// DB-11 (#490): reads/writes were checked against "a drop op exists
// somewhere in the source" instead of materialized column state, both
// missing an initially absent column with a live reader and false-firing
// on a column dropped then re-added.
#[test]
fn db11_column_dropped_then_readded_is_safe() {
    const SOURCE: &str = r"
dbsystem Db11Readd {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column legacy: Text present backfilled nullable; }
  }
  migration drop_legacy from 0 to 1 { drop users.legacy irreversible; }
  migration readd_legacy from 1 to 2 { add users.legacy nullable; }
  artifact server_v1 { reads users.id, users.legacy; }
  environment prod { schema 0..2; active server_v1 when schema 2..2; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}

#[test]
fn db11_initially_absent_column_with_reader_is_caught() {
    const SOURCE: &str = r"
dbsystem Db11Absent {
  database app {
    schema 0
    table users { column id: Int present backfilled not_null; column ghost: Text absent; }
  }
  artifact server_v1 { reads users.id, users.ghost; }
  environment prod { schema 0..0; active server_v1 when schema 0..0; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["column_removed_while_still_read"]);
}

// DB-12 (#492): `api_response_field_missing`'s `failed_rule` must be the
// documented `api_responses_expected`, not an undocumented name.
#[test]
fn db12_api_response_field_missing_uses_documented_rule_name() {
    const SOURCE: &str = r"
dbsystem Db12 {
  database app { schema 0 table users { column id: Int present backfilled not_null; } }
  artifact server_v2 { responds response.email_normalized; }
  artifact ios_v1 { expects response.email; }
  environment prod { schema 0..0; active server_v2 when schema 0..0; may_exist ios_v1 when schema 0..0; }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["api_response_field_missing"]);
    assert_eq!(failed_rules(&result), ["api_responses_expected"]);
}

// DB-14 (#490): an offline payload's TTL window never extended the
// acceptance obligation past the emitting schema snapshot.
#[test]
fn db14_offline_ttl_window_extends_obligation() {
    const SOURCE: &str = r"
dbsystem Db14 {
  database app { schema 0 table users { column id: Int present backfilled not_null; } }
  migration noop from 0 to 1 { backfill users.id; }
  artifact server_v0 { accepts api.X; }
  artifact server_v1 { accepts api.Y; }
  artifact ios_v1 { emits_offline api.X ttl 2; }
  environment prod {
    schema 0..1;
    active server_v0 when schema 0..0;
    active server_v1 when schema 1..1;
    active ios_v1 when schema 0..0;
  }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "violated");
    assert_eq!(kinds(&result), ["offline_payload_not_accepted"]);
    let witness = &result["findings"][0]["witness"];
    assert_eq!(witness["ttl_ticks"], 2);
    assert_eq!(witness["unaccepted_schema_version"], 1);
}

#[test]
fn db14_offline_ttl_window_accepted_throughout_is_safe() {
    const SOURCE: &str = r"
dbsystem Db14Control {
  database app { schema 0 table users { column id: Int present backfilled not_null; } }
  migration noop from 0 to 1 { backfill users.id; }
  artifact server_v0 { accepts api.X; }
  artifact server_v1 { accepts api.X, api.Y; }
  artifact ios_v1 { emits_offline api.X ttl 2; }
  environment prod {
    schema 0..1;
    active server_v0 when schema 0..0;
    active server_v1 when schema 1..1;
    active ios_v1 when schema 0..0;
  }
}";
    let result = check(SOURCE);
    assert_eq!(result["result"], "verified_under_assumptions", "{result}");
}
