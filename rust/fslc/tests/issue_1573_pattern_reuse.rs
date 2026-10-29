// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1573: collision sets follow evaluator binding lifetime, without broadening
//! lexical name visibility. Renaming repairs not/if and where/body collisions.

#[path = "support/issue_1573_forms.rs"]
mod forms;
#[path = "support/scratch_identity.rs"]
mod scratch_identity;

use forms::{Case, Expected};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    file: &'static str,
    property: &'static str,
}

impl Fixture {
    fn joined(case: &Case, second: &str) -> Self {
        let mut fixture = Self::new(case, false);
        std::fs::write(fixture.root.join("b.fsl"), second).expect("write second source");
        std::fs::write(
            fixture.root.join("s.fsl"),
            include_str!("fixtures/issue_1573_inventory/s.fsl"),
        )
        .expect("write two-source sync");
        std::fs::write(
            fixture.root.join("trace.json"),
            json!([{"action": "joined", "params": {}}]).to_string(),
        )
        .expect("write joined replay");
        fixture.file = "s.fsl";
        fixture.property = "b.P";
        fixture
    }

    fn new(case: &Case, composed: bool) -> Self {
        let root = scratch_identity::unique(std::env::temp_dir().join(format!(
            "fslc-1573-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        std::fs::create_dir(&root).expect("create fresh #1573 fixture");
        std::fs::write(root.join("a.fsl"), &case.source).expect("write direct spec");
        std::fs::write(
            root.join("case.json"),
            json!({"name": case.name, "composed": composed, "expected": format!("{:?}", case.expected)}).to_string(),
        )
        .expect("retain case identity");
        let (file, property, action) = if composed {
            let name = case.source.split_whitespace().nth(1).expect("spec name");
            std::fs::write(
                root.join("s.fsl"),
                format!("compose C {{\n  use {name} as a from \"a.fsl\"\n}}\n"),
            )
            .expect("write composition");
            ("s.fsl", "a.P", "a.stay")
        } else {
            ("a.fsl", "P", "stay")
        };
        std::fs::write(
            root.join("trace.json"),
            json!([{"action": action, "params": case.params}]).to_string(),
        )
        .expect("write one-step Monitor trace");
        Self {
            root,
            file,
            property,
        }
    }

    fn observe(&self, label: &str, consumer: &str) -> (Value, i32) {
        let binary = std::env::var_os("FSL_MATRIX_BINARY")
            .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_fslc")), PathBuf::from);
        let binary = std::fs::canonicalize(binary).expect("selected native binary");
        let args = match consumer {
            "check" => vec!["check", self.file],
            "Monitor" => vec!["replay", self.file, "--trace", "trace.json"],
            "mutate" => vec!["mutate", self.file, "--depth", "4", "--max-mutants", "128"],
            "refine" => vec!["refine", "impl.fsl", "abs.fsl", self.file, "--depth", "4"],
            _ => vec![
                "verify",
                self.file,
                "--engine",
                consumer,
                "--depth",
                "4",
                "--no-cache",
            ],
        };
        let output = Command::new(&binary)
            .args(&args)
            .env("FSLC_CACHE", "off")
            .current_dir(&self.root)
            .output()
            .expect("run native fslc");
        assert!(output.stderr.is_empty(), "{label}/{consumer}: {output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).expect("native JSON");
        let exit = output.status.code().expect("native exit");
        // libtest status/panic output can interleave with stderr JSON. Keep
        // byte-complete CLI outputs in separate files, not only a mixed log.
        std::fs::write(
            self.root.join(format!("{consumer}.stdout.json")),
            &output.stdout,
        )
        .expect("retain full native output");
        std::fs::write(
            self.root.join(format!("{consumer}.record.json")),
            json!({"binary": binary, "args": args, "exit": exit}).to_string(),
        )
        .expect("retain invocation and exit");
        eprintln!(
            "OBS {label}/{consumer} exit={exit} result={}",
            value["result"]
        );
        (value, exit)
    }
}

fn collision(value: &Value, exit: i32, line: u32, column: u32, context: &str) -> bool {
    // Measured direct and compose errors both carry numeric coordinates only.
    let message = format!(
        "{context}: pattern binding 'v' in 'is some(v)' shadows a name already in scope (an action parameter, quantifier binder, let, or earlier pattern binding) of the same name; rename the pattern variable at {line}:{column}"
    );
    exit == 2
        && value["fsl"] == "1.0"
        && value["result"] == "error"
        && value["kind"] == "semantics"
        && value["message"] == message
        && value["loc"] == json!({"line": line, "column": column})
}

fn accepted(value: &Value, exit: i32) -> bool {
    exit == 0 && value["result"] == "ok" && no_error(value)
}

fn no_error(value: &Value) -> bool {
    ["kind", "message", "error"]
        .iter()
        .all(|key| value.get(*key).is_none())
}

fn verdict(value: &Value, exit: i32, consumer: &str, truth: bool, property: &str) -> bool {
    // Independent per-consumer contracts, not an equality comparison of native
    // envelopes: observed proof/closure/cost fields genuinely differ by engine.
    let result = match (consumer, truth) {
        ("Monitor", true) => "conformant",
        ("Monitor", false) => "nonconformant",
        ("bmc", true) => "verified",
        (_, true) => "proved",
        (_, false) => "violated",
    };
    if exit != i32::from(!truth) || value["result"] != result || !no_error(value) {
        return false;
    }
    if truth {
        [
            "violation",
            "violation_kind",
            "invariant",
            "failed_at_event",
        ]
        .iter()
        .all(|key| value.get(*key).is_none())
            && (consumer != "Monitor" || value["steps_checked"] == 1)
    } else if consumer == "Monitor" {
        value["failed_at_event"] == 0
            && value["violation"]["kind"] == "invariant"
            && value["violation"]["name"] == property
    } else {
        value["violation_kind"] == "invariant"
            && value["invariant"] == property
            && value["violated_at_step"].is_number()
    }
}

fn matrix(cases: Vec<Case>) {
    matrix_in_context(cases, "invalid model expression", &[false, true]);
}

fn matrix_in_context(cases: Vec<Case>, context: &str, composition_modes: &[bool]) {
    eprintln!(
        "AXIS visibility: unchanged, no name-export contract is granted by collision-set rows"
    );
    let mut failures = Vec::new();
    let mut count = 0;
    for case in cases {
        for &composed in composition_modes {
            count += 1;
            let label = format!("{}/compose={composed}", case.name);
            let fixture = Fixture::new(&case, composed);
            eprintln!("CELL {label} expected={:?}", case.expected);
            eprintln!("FIXTURE {label} root={}", fixture.root.display());
            let (value, exit) = fixture.observe(&label, "check");
            let matches = match case.expected {
                Expected::Reject(line, column) => collision(&value, exit, line, column, context),
                Expected::Check | Expected::Truth(_) => accepted(&value, exit),
            };
            if !matches {
                failures.push(format!(
                    "{label}/check: expected {:?}, got {exit}: {value}",
                    case.expected
                ));
            }
            if let Expected::Truth(truth) = case.expected {
                for consumer in ["bmc", "induction", "explicit", "Monitor"] {
                    let (value, exit) = fixture.observe(&label, consumer);
                    if !verdict(&value, exit, consumer, truth, fixture.property) {
                        failures.push(format!(
                            "{label}/{consumer}: expected truth={truth}, got {exit}: {value}"
                        ));
                    }
                }
            }
        }
    }
    eprintln!(
        "MATRIX collision-set cells={count} failures={}",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn issue_table_direct_and_composed() {
    matrix(forms::issue_table());
}

#[test]
fn hidden_bindings_collide_at_each_later_position() {
    matrix(forms::positions());
}

#[test]
fn conditional_arms_in_invariants_arguments_and_definitions() {
    matrix(forms::branch_arms());
}

#[test]
fn conditional_arms_with_both_conditions_and_payload_orders() {
    matrix(forms::branch_variants());
}

#[test]
fn where_bindings_collide_with_body_and_of() {
    eprintln!(
        "IMPOSSIBLE count/unique/exactlyOne where-to-body/of: no independent body/of in grammar; two-match filter controls instead (3 kinds x 2 names x 2 composition modes = 12 cells)"
    );
    matrix(forms::binders());
}

#[test]
fn candidate_local_names_do_not_collide_with_outer_siblings() {
    matrix(forms::isolation());
}

#[test]
fn existing_collision_diagnostics_are_unchanged() {
    matrix(forms::diagnostics());
}

#[test]
fn statement_if_bindings_collide_in_arms_and_later_statements() {
    matrix(forms::statements());
}

#[test]
fn statement_if_arms_remain_independent_unlike_expression_arms() {
    matrix(forms::independent_statement_arms());
}

#[test]
fn nested_statement_arm_names_are_reserved_in_later_statements() {
    matrix(forms::nested_statement_arm_exports());
}

#[test]
fn ordinary_init_entries_reject_retained_pattern_reuse() {
    matrix_in_context(
        forms::init_positions(),
        "invalid init statement",
        &[false, true],
    );
}

#[test]
fn direct_init_reservations_cross_blocks_and_inline_initializers() {
    // Composed fragments remain a known gap; never pin their false verified result.
    matrix_in_context(
        forms::direct_init_fragments(),
        "invalid init statement",
        &[false],
    );
}

fn refinement_error_packet(message: &str, column: u64, width: u64) -> Value {
    json!({
        "fsl": "1.0", "result": "error", "kind": "type", "message": message,
        "loc": {"line": 1, "column": column},
        "span": {
            "start": {"offset": column - 1, "line": 1, "column": column},
            "end": {"offset": column - 1 + width, "line": 1, "column": column + width}
        }
    })
}

#[test]
fn refinement_map_priority_and_alpha_repair_keep_complete_packets() {
    let (case, abstraction, mapping) = forms::refinement_priority();
    let later = "(q is some(z)) and z==3";
    assert_eq!(mapping.matches(later).count(), 1);
    for renamed in [false, true] {
        let mut fixture = Fixture::new(&case, false);
        let mapping = if renamed {
            mapping.replacen(later, "(q is some(w)) and w==3", 1)
        } else {
            mapping.to_owned()
        };
        for (file, source) in [
            ("impl.fsl", case.source.as_str()),
            ("abs.fsl", abstraction),
            ("map.fsl", mapping.as_str()),
        ] {
            std::fs::write(fixture.root.join(file), source).expect("write refinement input");
        }
        fixture.file = "map.fsl";
        let (value, exit) = fixture.observe(&case.name, "refine");
        let expected = if renamed {
            refinement_error_packet(
                "rename the mapping pattern binder 'z' to avoid capture",
                130,
                7,
            )
        } else {
            refinement_error_packet(
                "invalid map expression for 'b': pattern binding 'z' in 'is some(z)' shadows a name already in scope (an action parameter, quantifier binder, let, or earlier pattern binding) of the same name; rename the pattern variable",
                33,
                3,
            )
        };
        assert_eq!(exit, 2, "{value}");
        assert_eq!(value, expected, "renamed={renamed}");
    }
}

#[test]
fn composed_init_check_preserved_and_renamed_sources_are_conformant() {
    for renamed in [false, true] {
        let (case, second) = forms::init_source_pair(renamed);
        let fixture = Fixture::new(&case, true);
        std::fs::write(fixture.root.join("b.fsl"), second).expect("write init source B");
        std::fs::write(
            fixture.root.join("s.fsl"),
            "compose C {\n  use A as a from \"a.fsl\"\n  use B as b from \"b.fsl\"\n}\n",
        )
        .expect("write init composition");
        assert!(accepted_envelope(&fixture, &case.name, "C"));
        if renamed {
            for consumer in ["bmc", "induction", "explicit", "Monitor"] {
                let (value, exit) = fixture.observe(&case.name, consumer);
                assert!(verdict(&value, exit, consumer, true, "a.P"), "{value}");
                if consumer == "Monitor" {
                    assert_eq!(value["final_state"]["b.flag"], true);
                }
            }
        }
    }
}

fn accepted_envelope(fixture: &Fixture, label: &str, spec: &str) -> bool {
    let (value, exit) = fixture.observe(label, "check");
    exit == 0
        && value
            == json!({
                "fsl": "1.0", "result": "ok", "spec": spec, "warnings": [],
                "versions": {
                    "verifier": {"name": "fslc-rust", "version": "4.12.0"},
                    "core": {"name": "fsl-core", "version": "4.12.0"},
                    "solver": {"name": "z3", "backend": "native-z3", "version": "Z3 4.16.0.0"}
                }
            })
}

#[test]
fn accepted_normalized_aggregates_keep_the_full_envelope() {
    let mut failures = Vec::new();
    for case in forms::normalized_aggregate_controls() {
        for composed in [false, true] {
            let fixture = Fixture::new(&case, composed);
            if !accepted_envelope(&fixture, &case.name, if composed { "C" } else { "A" }) {
                failures.push(format!("{}/compose={composed}", case.name));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn accepted_sync_sources_keep_independent_statement_bindings() {
    let (case, second) = forms::sync_sources(false);
    let fixture = Fixture::new(&case, false);
    assert!(accepted_envelope(&fixture, "sync-source/A", "A"));
    let fixture = Fixture::new(&second, false);
    assert!(accepted_envelope(&fixture, "sync-source/B", "B"));
    let fixture = sync_fixture(false);
    assert!(accepted_envelope(&fixture, "sync-source/joined", "Joined"));
}

fn sync_fixture(negative: bool) -> Fixture {
    let (case, second) = forms::sync_sources(negative);
    Fixture::joined(&case, &second.source)
}

fn sync_verdicts(negative: bool) {
    let fixture = sync_fixture(negative);
    let label = format!("sync-source/negative={negative}");
    assert!(accepted_envelope(&fixture, &label, "Joined"));
    let mut failures = Vec::new();
    for consumer in ["bmc", "induction", "explicit", "Monitor"] {
        let (value, exit) = fixture.observe(&label, consumer);
        if !verdict(&value, exit, consumer, !negative, "b.P") {
            failures.push(format!("{consumer}: {exit}: {value}"));
        }
        if consumer == "Monitor" && !negative && value["final_state"]["b.flag"] != true {
            failures.push(format!("concrete final b.flag must remain true: {value}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn sync_source_summary_detects_cross_source_payload_capture() {
    sync_verdicts(false);
}

#[test]
fn sync_source_negative_remains_violated_with_or_without_hygiene() {
    sync_verdicts(true);
}

#[test]
fn source_inventory_keeps_foreign_conjunctive_guard_enabled() {
    let case = Case {
        name: "inventory-probe".into(),
        source: include_str!("fixtures/issue_1573_inventory/a.fsl").into(),
        expected: Expected::Check,
        params: json!({}),
    };
    let fixture = Fixture::joined(&case, include_str!("fixtures/issue_1573_inventory/b.fsl"));
    assert!(accepted_envelope(&fixture, &case.name, "Joined"));
    for consumer in ["bmc", "induction", "explicit", "Monitor"] {
        let (value, exit) = fixture.observe(&case.name, consumer);
        // An invariant failure after joined, not a disabled/failed requires,
        // establishes that B's conjunctive guard really enabled the action.
        assert!(verdict(&value, exit, consumer, false, "b.P"), "{value}");
    }
}

#[test]
fn source_inventory_preserves_s1_mutation_outcomes() {
    // Recalibrated against 299ac3df: source separation must not invalidate mutants.
    let case = Case {
        name: "s1-mutation".into(),
        source: include_str!("fixtures/issue_1547_guard_reservations/a.fsl").into(),
        expected: Expected::Check,
        params: json!({}),
    };
    let mut failures = Vec::new();
    for (file, composition, expected) in [
        (
            "c.fsl",
            include_str!("fixtures/issue_1547_guard_reservations/c.fsl"),
            json!([14, 12, 0]),
        ),
        (
            "c_if.fsl",
            include_str!("fixtures/issue_1547_guard_reservations/c_if.fsl"),
            json!([7, 8, 0]),
        ),
    ] {
        let mut fixture = Fixture::new(&case, false);
        std::fs::write(fixture.root.join(file), composition).expect("write S1 composition");
        fixture.file = file;
        assert!(accepted_envelope(&fixture, file, "C"));
        let (value, exit) = fixture.observe(file, "mutate");
        let summary = &value["summary"];
        let counts = json!([summary["killed"], summary["survived"], summary["invalid"]]);
        if exit != 0 || value["result"] != "mutated" || counts != expected {
            failures.push(format!("{file}: expected {expected}, got {exit}: {value}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn sibling_constant_and_definition_rejection_priority() {
    let data: Value =
        serde_json::from_str(include_str!("fixtures/issue_1573_sibling_priority.json"))
            .expect("calibrated sibling packets");
    let mut failures = Vec::new();
    let mut counterfactuals = 0;
    for row in data["cases"].as_array().expect("cases") {
        let source = row["source"].as_str().expect("source");
        for route in row["routes"].as_array().expect("routes") {
            let composed = route["composed"].as_bool().expect("composition mode");
            let mut forms = vec![("head", source.to_owned(), &route["head"])];
            if let Some(base) = route.get("base") {
                assert_eq!(source.matches("(p is some(v))").count(), 1);
                assert_eq!(source.matches("v == 2").count(), 1);
                let renamed = source
                    .replacen("(p is some(v))", "(p is some(w))", 1)
                    .replacen("v == 2", "w == 2", 1);
                forms.push(("rename-reuse", renamed, base));
                counterfactuals += 1;
            }
            for (form, source, packet) in forms {
                let case = Case {
                    name: format!(
                        "sibling-priority/{}/{form}",
                        row["name"].as_str().expect("name")
                    ),
                    source,
                    expected: Expected::Check,
                    params: json!({}),
                };
                let fixture = Fixture::new(&case, composed);
                let mut expected = data["envelope"].clone();
                expected
                    .as_object_mut()
                    .expect("envelope")
                    .extend(packet.as_object().expect("packet").clone());
                std::fs::write(
                    fixture.root.join("case.json"),
                    json!({"name": case.name, "composed": composed, "expected": "Error", "packet": expected}).to_string(),
                )
                .expect("retain complete rejection expectation");
                let (value, exit) = fixture.observe(&case.name, "check");
                if exit != 2 || value != expected {
                    failures.push(format!(
                        "{}/compose={composed}: expected {expected}, got {exit}: {value}",
                        case.name
                    ));
                }
            }
        }
    }
    assert_eq!(
        counterfactuals, 3,
        "all changed routes need a live repair control"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
