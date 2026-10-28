// SPDX-License-Identifier: Apache-2.0

//! #1432: pin the existing digest serialization, not RFC 8785/JCS (#1526).

use std::path::Path;

use fsl_tools::{artifact_digest, canonical_json, lifecycle_record_digest};
use serde_json::{Value, json};

#[test]
fn preservation_keys_sort_recursively_by_rust_str_not_utf16() {
    let value: Value =
        serde_json::from_str(r#"{"z":[{"z":0,"a":1}],"a":{"𐀀":1,"":2,"é":3,"a":4}}"#)
            .expect("crafted object");
    assert_eq!(
        canonical_json(&value),
        r#"{"a":{"a":4,"é":3,"":2,"𐀀":1},"z":[{"a":1,"z":0}]}"#
    );
}

#[test]
fn preservation_integers_remain_exact_above_javascript_precision() {
    assert_eq!(
        canonical_json(&json!([
            i64::MIN,
            -9_007_199_254_740_993_i64,
            9_007_199_254_740_993_i64,
            i64::MAX,
            u64::MAX
        ])),
        "[-9223372036854775808,-9007199254740993,9007199254740993,9223372036854775807,18446744073709551615]"
    );
}

#[test]
fn preservation_floats_use_shortest_roundtrip_and_keep_integral_dot_zero() {
    assert_eq!(
        canonical_json(&json!([
            1.0,
            -0.0,
            0.1,
            1.234_567_890_123_456_7,
            1e30,
            1e-7
        ])),
        "[1.0,-0.0,0.1,1.2345678901234567,1e+30,1e-7]"
    );
}

#[test]
fn preservation_unicode_stays_raw_and_slash_is_not_escaped() {
    assert_eq!(
        canonical_json(&json!({"日本語":"é/😀"})),
        "{\"日本語\":\"é/😀\"}"
    );
}

#[test]
fn preservation_quote_backslash_and_all_json_controls_have_exact_escapes() {
    let value = format!("\"\\/{}", (0_u8..=31).map(char::from).collect::<String>());
    let expected = concat!(
        "\"\\\"\\\\/",
        "\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007",
        "\\b\\t\\n\\u000b\\f\\r\\u000e\\u000f",
        "\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017",
        "\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f\""
    );
    assert_eq!(canonical_json(&json!(value)), expected);
}

#[test]
fn preservation_stored_example_artifact_and_lifecycle_digests_still_verify() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("root");
    let mut artifacts = 0;
    let mut records = 0;
    for entry in
        std::fs::read_dir(root.join("examples/causal/evidence")).expect("evidence directory")
    {
        let path = entry.expect("directory entry").path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let value: Value = serde_json::from_slice(&std::fs::read(&path).expect("example bytes"))
            .expect("example JSON");
        if let Some(chain) = value.get("records").and_then(Value::as_array) {
            for record in chain {
                assert_eq!(
                    lifecycle_record_digest(
                        value["evidence_id"].as_str().expect("id"),
                        value["artifact_digest"].as_str().expect("artifact digest"),
                        record
                    ),
                    record["record_digest"],
                    "{}",
                    path.display()
                );
                records += 1;
            }
        } else if value.get("artifact_digest").is_some() {
            assert_eq!(
                artifact_digest(&value),
                value["artifact_digest"],
                "{}",
                path.display()
            );
            artifacts += 1;
        }
    }
    assert_eq!(
        (artifacts, records),
        (2, 2),
        "digest controls must remain live"
    );
}

#[test]
fn detector_design_documents_canonical_json_rules_and_jcs_followup() {
    let doc = include_str!("../../../docs/design/DESIGN-causal.md");
    let rules = doc
        .split("\n\n")
        .find(|paragraph| {
            paragraph.to_lowercase().contains("canonical json") && paragraph.contains("#1526")
        })
        .expect("canonical JSON rules paragraph must cite JCS follow-up #1526");
    for term in ["UTF-8", "serde_json", ".0", "2^53", "U+0000", "U+001F"] {
        assert!(
            rules.contains(term),
            "canonical JSON rules must state {term}: {rules}"
        );
    }
}
