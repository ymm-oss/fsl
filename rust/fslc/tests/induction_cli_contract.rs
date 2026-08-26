// SPDX-License-Identifier: Apache-2.0

//! Native CLI ownership for the sixteen induction envelope/exit cases in
//! `tools/check_rust_induction_parity.py` (issue #761 F3).
//!
//! Regenerate the observed contract from the repository root only after a
//! reviewed CLI contract change:
//!
//! `UPDATE_INDUCTION_CLI_CONTRACT=1 cargo test --manifest-path rust/Cargo.toml -p fslc-rust --test induction_cli_contract --locked`
//!
//! The update path executes the same roster, cache handling, normalization,
//! and exit capture as the ordinary comparison. Ordinary test/CI runs never
//! write the golden. Review the complete golden diff after regeneration.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};

const UPDATE_ENV: &str = "UPDATE_INDUCTION_CLI_CONTRACT";

struct Case {
    path: &'static str,
    depth: u8,
    k: u8,
}

const CASES: &[Case] = &[
    Case {
        path: "examples/gallery/valid/tiny_turnstile.fsl",
        depth: 4,
        k: 1,
    },
    Case {
        path: "examples/gallery/valid/tiny_traffic_light.fsl",
        depth: 5,
        k: 1,
    },
    Case {
        path: "examples/gallery/valid/tiny_bounded_counter.fsl",
        depth: 4,
        k: 1,
    },
    Case {
        path: "examples/gallery/valid/small_elevator.fsl",
        depth: 7,
        k: 1,
    },
    Case {
        path: "examples/gallery/adversarial/option_struct_set_seq_combo.fsl",
        depth: 5,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/induction_unknown_cti.fsl",
        depth: 4,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/induction_unknown_cti.fsl",
        depth: 4,
        k: 3,
    },
    Case {
        path: "tests/fixtures/rust_port/ranked_leadsto.fsl",
        depth: 5,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/ranked_leadsto_non_decreasing.fsl",
        depth: 5,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/ranked_leadsto_unbounded_below.fsl",
        depth: 5,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/ranked_leadsto_helpful.fsl",
        depth: 1,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/ranked_leadsto_helpful_nonfair.fsl",
        depth: 1,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/ranked_leadsto_helpful_blocked.fsl",
        depth: 1,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/ranked_leadsto_helpful_flickering.fsl",
        depth: 8,
        k: 1,
    },
    Case {
        path: "tests/fixtures/rust_port/ranked_leadsto_helpful_pumped.fsl",
        depth: 8,
        k: 1,
    },
    Case {
        path: "specs/cart_buggy.fsl",
        depth: 5,
        k: 1,
    },
];

#[derive(Clone, Copy)]
enum Treatment {
    Mask(&'static str),
    ValueShape,
    TraceShape,
}

struct Exclusion {
    path: &'static str,
    reason: &'static str,
    treatment: Treatment,
}

// Read from the parity harness's normalization and then tightened from two
// consecutive native observations. Timing is one path family because all
// elapsed_s values are wall-clock measurements; the original harness named
// only the root member even though the nested members vary for the same reason.
const EXCLUSIONS: &[Exclusion] = &[
    Exclusion {
        path: "$.cost.**.*elapsed_s",
        reason: "wall-clock timing varies between repeated invocations; cost kinds, names, check counts, solver statistics, and memory remain compared",
        treatment: Treatment::Mask("<elapsed>"),
    },
    Exclusion {
        path: "$.cti.states",
        reason: "the solver may choose a different induction counterexample; its existence and surrounding diagnostic remain compared",
        treatment: Treatment::Mask("<nondeterministic-cti>"),
    },
    Exclusion {
        path: "$.reachables.*.witness",
        reason: "a replayed BMC base witness is non-unique; witnessed depth and the rest of the reachable envelope remain compared",
        treatment: Treatment::Mask("<replayed-base-witness>"),
    },
    Exclusion {
        path: "$.trace",
        reason: "a BMC violation witness is non-unique and separately replay-owned; its complete structural shape remains compared",
        treatment: Treatment::TraceShape,
    },
    Exclusion {
        path: "$.violating_bindings",
        reason: "binding values derive from the non-unique BMC witness; their complete structural shape remains compared",
        treatment: Treatment::ValueShape,
    },
    Exclusion {
        path: "$.last_action.params",
        reason: "parameter values derive from the non-unique BMC witness; their complete structural shape remains compared",
        treatment: Treatment::ValueShape,
    },
    Exclusion {
        path: "$.blame.conjuncts.*.violating_bindings",
        reason: "blame binding values derive from the non-unique BMC witness; their complete structural shape remains compared",
        treatment: Treatment::ValueShape,
    },
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_induction(case: &Case) -> (Value, i32) {
    let depth = case.depth.to_string();
    let k = case.k.to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            case.path,
            "--depth",
            &depth,
            "--engine",
            "induction",
            "--k",
            &k,
            "--deadlock",
            "ignore",
        ])
        .current_dir(repository_root())
        .output()
        .expect("run native fslc induction");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for induction case {} depth={} k={}: {error}; stderr={}",
            case.path,
            case.depth,
            case.k,
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn exclusion_index(path: &str) -> Option<usize> {
    if path.starts_with("$.cost.") && path.ends_with("elapsed_s") {
        Some(0)
    } else if path == "$.cti.states" {
        Some(1)
    } else if path.starts_with("$.reachables.") && path.ends_with(".witness") {
        Some(2)
    } else if path == "$.trace" {
        Some(3)
    } else if path == "$.violating_bindings" {
        Some(4)
    } else if path == "$.last_action.params" {
        Some(5)
    } else if path.starts_with("$.blame.conjuncts.") && path.ends_with(".violating_bindings") {
        Some(6)
    } else {
        None
    }
}

fn value_shape(value: &Value) -> Value {
    match value {
        Value::Null => json!("<null>"),
        Value::Bool(_) => json!("<boolean>"),
        Value::Number(_) => json!("<number>"),
        Value::String(_) => json!("<string>"),
        Value::Array(items) => Value::Array(items.iter().map(value_shape).collect()),
        Value::Object(items) => {
            let mut keys: Vec<_> = items.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), value_shape(&items[key])))
                    .collect(),
            )
        }
    }
}

fn sorted_keys(value: &Value) -> Value {
    match value.as_object() {
        Some(object) => {
            let mut keys: Vec<_> = object.keys().cloned().collect();
            keys.sort();
            Value::Array(keys.into_iter().map(Value::String).collect())
        }
        None => value_shape(value),
    }
}

fn trace_shape(value: &Value) -> Value {
    let Some(trace) = value.as_array() else {
        return value_shape(value);
    };
    Value::Array(
        trace
            .iter()
            .map(|entry| {
                let Some(object) = entry.as_object() else {
                    return value_shape(entry);
                };
                let mut shaped = Map::new();
                shaped.insert("keys".to_owned(), sorted_keys(entry));
                if let Some(action) = object.get("action") {
                    shaped.insert("action_keys".to_owned(), sorted_keys(action));
                }
                if let Some(changes) = object.get("changes") {
                    let changes_shape = match changes.as_object() {
                        Some(changes) => {
                            let change_key_shapes: BTreeSet<Vec<String>> = changes
                                .values()
                                .map(|change| match change.as_object() {
                                    Some(change) => {
                                        let mut keys: Vec<_> = change.keys().cloned().collect();
                                        keys.sort();
                                        keys
                                    }
                                    None => vec!["<non-object>".to_owned()],
                                })
                                .collect();
                            Value::Array(
                                change_key_shapes
                                    .into_iter()
                                    .map(|shape| {
                                        Value::Array(shape.into_iter().map(Value::String).collect())
                                    })
                                    .collect(),
                            )
                        }
                        None => value_shape(changes),
                    };
                    shaped.insert("changes_shape".to_owned(), changes_shape);
                }
                if let Some(blame) = object.get("blame") {
                    shaped.insert("blame_keys".to_owned(), sorted_keys(blame));
                }
                Value::Object(shaped)
            })
            .collect(),
    )
}

fn normalize(value: &Value, path: &str, hits: &mut [usize]) -> Value {
    if let Some(index) = exclusion_index(path) {
        hits[index] += 1;
        return match EXCLUSIONS[index].treatment {
            Treatment::Mask(marker) => Value::String(marker.to_owned()),
            Treatment::ValueShape => value_shape(value),
            Treatment::TraceShape => trace_shape(value),
        };
    }

    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| {
                        (
                            key.clone(),
                            normalize(&object[key], &format!("{path}.{key}"), hits),
                        )
                    })
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, value)| normalize(value, &format!("{path}.{index}"), hits))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Cache presence depends on ambient execution order. Compare the stable
/// envelope without it, but fail closed on any present cache block that is not
/// the exact public shape of a successful cache hit (`exact` or `cross_depth`). This is deliberately
/// separate from `EXCLUSIONS`: requiring a live hit would make a clean cache
/// fail, while requiring absence would make a warm cache fail.
fn remove_and_validate_optional_cache(output: &mut Value) {
    let Some(cache) = output
        .as_object_mut()
        .expect("CLI envelope object")
        .remove("cache")
    else {
        return;
    };
    let cache = cache.as_object().expect("cache object");
    let actual_keys: BTreeSet<&str> = cache.keys().map(String::as_str).collect();
    let expected_keys: BTreeSet<&str> = ["hit", "key", "source"].into_iter().collect();
    assert_eq!(
        actual_keys, expected_keys,
        "unexpected cache envelope: {cache:#?}"
    );
    assert_eq!(cache["hit"], true, "{cache:#?}");
    assert!(
        matches!(cache["source"].as_str(), Some("exact" | "cross_depth")),
        "cache source is not a public successful-hit source: {cache:#?}"
    );
    let key = cache["key"].as_str().expect("cache key string");
    assert!(
        valid_cache_key(key),
        "cache key is not 64 lowercase hexadecimal characters: {key}"
    );
}

fn valid_cache_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn observe_contract() -> Value {
    assert_eq!(
        CASES.len(),
        16,
        "the retired harness owned exactly 16 cases"
    );
    let mut hits = vec![0; EXCLUSIONS.len()];
    let mut observed = Vec::with_capacity(CASES.len());
    for case in CASES {
        let (mut output, exit) = run_induction(case);
        remove_and_validate_optional_cache(&mut output);
        let normalized = normalize(&output, "$", &mut hits);
        observed.push(json!({
            "path": case.path,
            "depth": case.depth,
            "k": case.k,
            "exit": exit,
            "output": normalized,
        }));
    }

    for (exclusion, hits) in EXCLUSIONS.iter().zip(hits) {
        assert!(
            hits > 0,
            "dead exclusion {} matched no field in either observed/native side; reason was: {}",
            exclusion.path,
            exclusion.reason
        );
    }
    Value::Array(observed)
}

#[test]
fn sixteen_induction_cases_pin_the_complete_stable_cli_envelope_and_exit() {
    let actual = observe_contract();
    if std::env::var_os(UPDATE_ENV).is_some() {
        let golden =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/induction_cli_contract.json");
        std::fs::write(
            &golden,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&actual).expect("serialize induction CLI golden")
            ),
        )
        .unwrap_or_else(|error| panic!("write {}: {error}", golden.display()));
        return;
    }

    let expected: Value = serde_json::from_str(include_str!("goldens/induction_cli_contract.json"))
        .expect("parse induction CLI golden");
    let actual = actual.as_array().expect("observed induction array");
    let expected = expected.as_array().expect("induction golden array");
    assert_eq!(expected.len(), CASES.len(), "golden/case roster drift");

    for (index, case) in CASES.iter().enumerate() {
        let actual_case = &actual[index];
        let expected_case = &expected[index];
        assert_eq!(expected_case["path"], case.path, "case {index} path drift");
        assert_eq!(
            expected_case["depth"], case.depth,
            "{} depth drift",
            case.path
        );
        assert_eq!(expected_case["k"], case.k, "{} k drift", case.path);
        assert_eq!(
            actual_case["exit"],
            expected_case["exit"],
            "{} depth={} k={}: produced exit={}, expected={}; output={:#}",
            case.path,
            case.depth,
            case.k,
            actual_case["exit"],
            expected_case["exit"],
            actual_case["output"]
        );
        assert_eq!(
            actual_case["output"], expected_case["output"],
            "{} depth={} k={}: complete stable envelope drift",
            case.path, case.depth, case.k
        );
    }
}

#[test]
fn cache_key_contract_rejects_uppercase_hex_fixture() {
    let lowercase = "a".repeat(64);
    assert!(valid_cache_key(&lowercase), "lowercase control must pass");

    let uppercase = "A".repeat(64);
    let mut uppercase_fixture = json!({
        "cache": {
            "hit": true,
            "key": uppercase,
            "source": "exact",
        }
    });
    let produced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        remove_and_validate_optional_cache(&mut uppercase_fixture);
    }));
    assert!(
        produced.is_err(),
        "uppercase cache-key fixture: produced=accepted, expected=rejected"
    );
}

#[test]
fn nonexistent_induction_case_is_an_io_error_with_exit_two() {
    let case = Case {
        path: "tests/fixtures/rust_port/induction_case_does_not_exist.fsl",
        depth: 1,
        k: 1,
    };
    let (output, exit) = run_induction(&case);
    assert_eq!(exit, 2, "produced exit={exit}, expected=2; {output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "io", "{output:#}");
    assert!(
        output["message"]
            .as_str()
            .is_some_and(|message| message.contains(case.path)),
        "{output:#}"
    );
}
