// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1417: equivalent default verification must reach the same stages through
//! every applicable consumer. New test code is necessary to make the requested
//! route inventory, report-preload detector and preservation controls executable.
//! Edition/D3, selected-Agent options, lower-only consumers and Worker are not
//! this contract. Passing mutation runs observe the baseline, not mutant strength.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Verify,
    Sweep,
    Chain,
    Ledger,
    Html,
    Mutate,
    Db,
    Ai,
    Domain,
}

// The representative's eight routes. Domain is an additional projection control,
// not a replacement for any of these entries.
const ROUTES: &[Route] = &[
    Route::Verify,
    Route::Sweep,
    Route::Chain,
    Route::Ledger,
    Route::Html,
    Route::Mutate,
    Route::Db,
    Route::Ai,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Other,
    Db,
    Ai,
    Domain,
}

struct Probe {
    name: &'static str,
    file: &'static str,
    dialect: Dialect,
    result: &'static str,
    status: i32,
    elapsed: usize,
}

// One probe inventory drives both red pairs and controls. All inputs already
// exist; no fixture copies with divergent imports or locations are needed.
const PROBES: &[Probe] = &[
    Probe {
        name: "broken_causal",
        file: "rust/fslc/tests/fixtures/error_envelope_broken_causal.fsl",
        dialect: Dialect::Other,
        result: "error",
        status: 2,
        elapsed: 0,
    },
    Probe {
        name: "agent",
        file: "rust/fslc/tests/fixtures/issue_468_unsafe_graph.fsl",
        dialect: Dialect::Other,
        result: "error",
        status: 2,
        elapsed: 0,
    },
    Probe {
        name: "db_unknown_column",
        file: "rust/fslc/tests/fixtures/error_envelope_db_unknown_column.fsl",
        dialect: Dialect::Db,
        result: "error",
        status: 2,
        elapsed: 0,
    },
    Probe {
        name: "ai_unknown_tool",
        file: "rust/fslc/tests/fixtures/error_envelope_ai_unknown_tool.fsl",
        dialect: Dialect::Ai,
        result: "error",
        status: 2,
        elapsed: 0,
    },
    Probe {
        name: "control_seam_fail",
        file: "rust/fslc/tests/fixtures/issue_1008_not_evaluated/impl_broken.fsl",
        dialect: Dialect::Other,
        result: "refinement_failed",
        status: 1,
        elapsed: 12,
    },
    Probe {
        name: "control_seam_pass",
        file: "tests/fixtures/chain/requirements.fsl",
        dialect: Dialect::Other,
        result: "verified",
        status: 0,
        elapsed: 8,
    },
    Probe {
        name: "control_governance_fail",
        file: "examples/refinement_liveness/governance_detects_safety_loss.fsl",
        dialect: Dialect::Other,
        result: "refinement_failed",
        status: 1,
        elapsed: 10,
    },
    Probe {
        name: "control_vacuity_warn",
        file: "rust/fslc/tests/fixtures/issue_1008_not_evaluated/impl_vacuous.fsl",
        dialect: Dialect::Other,
        result: "verified",
        status: 0,
        elapsed: 12,
    },
    Probe {
        name: "control_db_pass",
        file: "examples/db/safe_add_nullable_column.fsl",
        dialect: Dialect::Db,
        result: "verified",
        status: 0,
        elapsed: 36,
    },
    Probe {
        name: "control_ai_pass",
        file: "examples/ai/refund_agent_tool_safety.fsl",
        dialect: Dialect::Ai,
        result: "verified",
        status: 0,
        elapsed: 21,
    },
    Probe {
        name: "control_trace",
        file: "rust/fslc/tests/fixtures/issue_1008_not_evaluated/impl_out_of_scope_acceptance.fsl",
        dialect: Dialect::Other,
        result: "verified",
        status: 0,
        elapsed: 11,
    },
    Probe {
        name: "control_domain_projection",
        file: "rust/fslc/tests/fixtures/issue_641_domain_clean.fsl",
        dialect: Dialect::Domain,
        result: "verified",
        status: 0,
        elapsed: 13,
    },
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

fn applicable(route: Route, probe: &Probe) -> bool {
    match route {
        Route::Db => probe.dialect == Dialect::Db,
        Route::Ai => probe.dialect == Dialect::Ai,
        Route::Domain => probe.dialect == Dialect::Domain,
        _ => true,
    }
}

struct Run {
    manifest: PathBuf,
    source: PathBuf,
}

impl Run {
    fn new(probe: &Probe) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "fslc-1417-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).expect("create test-owned chain directory");
        let source = root().join(probe.file);
        let manifest = directory.join("chain.toml");
        std::fs::write(
            &manifest,
            format!("[design]\nfile = {}\ndepth = 2\n", json!(source)),
        )
        .expect("write chain manifest with the identical absolute source path");
        Self { manifest, source }
    }

    fn run(&self, route: Route) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.current_dir(root()).env("FSLC_CACHE", "off");
        match route {
            Route::Chain => {
                command.arg("chain").arg(&self.manifest);
            }
            Route::Db | Route::Ai | Route::Domain => {
                command.arg(match route {
                    Route::Db => "db",
                    Route::Ai => "ai",
                    _ => "domain",
                });
                command
                    .arg("check")
                    .arg(&self.source)
                    .args(["--depth", "2"]);
            }
            _ => {
                command
                    .arg(match route {
                        Route::Verify => "verify",
                        Route::Sweep => "sweep",
                        Route::Ledger => "ledger",
                        Route::Html => "html",
                        Route::Mutate => "mutate",
                        _ => unreachable!(),
                    })
                    .arg(&self.source)
                    .args(["--depth", if route == Route::Sweep { "2..2" } else { "2" }]);
                match route {
                    Route::Verify => {
                        command.arg("--no-cache");
                    }
                    Route::Html => {
                        command.arg("--no-timing");
                    }
                    Route::Mutate => {
                        command.args(["--max-mutants", "0"]);
                    }
                    _ => {}
                }
            }
        }
        command.output().expect("execute native CLI")
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        // Only the directory created by this Run is owned here; never HOME cache.
        std::fs::remove_dir_all(self.manifest.parent().expect("manifest directory"))
            .expect("remove test-owned chain directory");
    }
}

fn parsed(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; exit={:?}; stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn oracle(run: &Run, probe: &Probe) -> Value {
    let output = run.run(Route::Verify);
    assert_eq!(output.status.code(), Some(probe.status), "{}", probe.name);
    let mut value = parsed(&output);
    assert_eq!(value["result"], probe.result, "{}", probe.name);
    // Dispatch-only versions is the sole metadata difference. Check the whole
    // shape first, rather than silently dropping an absent key on both sides.
    let versions = value
        .as_object_mut()
        .unwrap()
        .remove("versions")
        .expect("verify versions");
    let solver_version = versions["solver"]["version"]
        .as_str()
        .expect("solver version string");
    assert!(solver_version.starts_with("Z3 ") && solver_version.len() > 3);
    assert_eq!(
        versions,
        json!({
            "verifier": {"name": "fslc-rust", "version": env!("CARGO_PKG_VERSION")},
            "core": {"name": "fsl-core", "version": env!("CARGO_PKG_VERSION")},
            "solver": {"name": "z3", "backend": "native-z3", "version": solver_version}
        })
    );
    value
}

// Only these observed leaves vary with process timing. Counts are measured for
// each probe, not inferred from whatever survives a mutation. All solver memory,
// counters, locations, traces, warnings, summaries and assurance stay compared.
fn normalize(value: &mut Value, elapsed: usize) -> Result<(), String> {
    fn leaf(value: &mut Value) -> Result<(), String> {
        let number = value.as_f64().ok_or("missing/non-numeric elapsed leaf")?;
        if !number.is_finite() || number < 0.0 {
            return Err("invalid elapsed leaf".into());
        }
        *value = json!(0);
        Ok(())
    }
    let cost = value.get_mut("cost").ok_or("missing cost")?;
    let properties = cost["properties"].as_array().ok_or("missing properties")?;
    if properties.len() + 2 != elapsed {
        return Err(format!(
            "elapsed count: expected {elapsed}, produced {}",
            properties.len() + 2
        ));
    }
    leaf(
        cost.pointer_mut("/elapsed_s")
            .ok_or("missing cost.elapsed_s")?,
    )?;
    leaf(
        cost.pointer_mut("/solver/check_elapsed_s")
            .ok_or("missing solver.check_elapsed_s")?,
    )?;
    for property in cost["properties"].as_array_mut().unwrap() {
        leaf(
            property
                .get_mut("elapsed_s")
                .ok_or("missing property elapsed_s")?,
        )?;
    }
    Ok(())
}

fn sweep_expected(verification: &Value, probe: &Probe) -> Value {
    let mut verification = verification.clone();
    assert!(
        verification
            .as_object_mut()
            .unwrap()
            .insert(
                "bounds_overrides".into(),
                json!({"instances": {}, "values": {}})
            )
            .is_none()
    );
    let cell = json!({
        "scope": {"instances": {}, "values": {}, "depth": 2},
        "summary": {"result": probe.result, "checked_to_depth": 2},
        "verification": verification
    });
    json!({
        "fsl": "1.0", "result": if probe.status == 0 { "sweep_passed" } else { "sweep_failed" },
        "spec": verification["spec"],
        "sweep": {
            "minimality_order": ["instances", "values", "depth"],
            "ranges": {"instances": {}, "values": {}, "depth": [2, 2]},
            "results": [cell], "minimal_counterexample": if probe.status == 0 { Value::Null } else { cell },
            "inconclusive_scopes": []
        }
    })
}

fn chain_expected(run: &Run, verification: &Value, probe: &Probe) -> Value {
    let mut expected = json!({
        "fsl": "1.0", "result": if probe.status == 0 { "verified" } else if probe.status == 2 { "error" } else { "violated" },
        "manifest": run.manifest, "keep_going": false,
        "layers": [{"layer": "design", "kind": "verify", "file": run.source,
            "status": if probe.status == 0 { "passed" } else { "failed" },
            "result": probe.result, "exit_code": probe.status, "detail": verification, "depth": 2}]
    });
    if probe.status != 0 {
        expected["failed"] = json!(["design"]);
    }
    if probe.status == 2 {
        expected["kind"] = json!("chain");
        expected["message"] = json!("one or more chain layers returned an error");
    }
    expected
}

#[test]
fn route_inventory_is_explicit() {
    assert_eq!(
        ROUTES,
        &[
            Route::Verify,
            Route::Sweep,
            Route::Chain,
            Route::Ledger,
            Route::Html,
            Route::Mutate,
            Route::Db,
            Route::Ai
        ]
    );
    for route in ROUTES {
        assert!(
            PROBES
                .iter()
                .any(|probe| probe.elapsed == 0 && applicable(*route, probe))
        );
        assert!(
            PROBES
                .iter()
                .any(|probe| probe.elapsed > 0 && applicable(*route, probe))
        );
    }
}

#[test]
fn report_preloads_cannot_replace_the_common_error_envelope() {
    let mut failed = Vec::new();
    for probe in PROBES.iter().filter(|probe| probe.elapsed == 0) {
        let run = Run::new(probe);
        let expected = oracle(&run, probe);
        for route in ROUTES
            .iter()
            .copied()
            .filter(|route| applicable(*route, probe))
        {
            if route == Route::Verify {
                continue;
            } // already checked as the oracle
            let output = run.run(route);
            let actual = serde_json::from_slice::<Value>(&output.stdout);
            let wanted = if route == Route::Chain {
                chain_expected(&run, &expected, probe)
            } else {
                expected.clone()
            };
            let agrees = output.status.code() == Some(probe.status)
                && actual.as_ref().is_ok_and(|value| value == &wanted);
            let name = format!("{}/{route:?}", probe.name);
            println!(
                "PAIR {name}: produced exit={:?}, expected exit={}, agreement={agrees}",
                output.status.code(),
                probe.status
            );
            if !agrees {
                failed.push(format!(
                    "{name}: produced={} stderr={}\nexpected={wanted}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }
    // Do not stop at the first diagnostic: all eight expected red report pairs
    // must be visible, including AI's pre-fix panic/empty-stdout/exit 101.
    assert!(
        failed.is_empty(),
        "red pairs ({}):\n{}",
        failed.len(),
        failed.join("\n")
    );
}

fn projected(probe: &Probe) -> Value {
    // Independent public output expectations, NOT product project_kernel applied
    // to both sides. AI intentionally has neither warnings nor action coverage.
    let (spec, actions) = match probe.dialect {
        Dialect::Db => (
            "SafeAddNullableColumn",
            json!({"migrate_add_nickname": true}),
        ),
        Dialect::Ai => ("RefundAgentToolSafety", Value::Null),
        Dialect::Domain => (
            "CleanDiagnosticDomain",
            json!({"doc_close_doc": true, "doc_reopen_doc": true}),
        ),
        Dialect::Other => unreachable!(),
    };
    let mut kernel = json!({"result": "verified", "spec": spec, "depth": 2, "checked_to_depth": 2, "completeness": "bounded"});
    if probe.dialect != Dialect::Ai {
        kernel["warnings"] = json!([]);
        kernel["action_coverage"] = actions;
    }
    kernel
}

fn control_route(run: &Run, route: Route, probe: &Probe, expected: &Value) {
    let first = run.run(route);
    let second = run.run(route);
    let guard = route == Route::Mutate && matches!(probe.dialect, Dialect::Db | Dialect::Ai);
    let status = if guard { 2 } else { probe.status };
    for output in [&first, &second] {
        assert_eq!(
            output.status.code(),
            Some(status),
            "{} {route:?}: {}",
            probe.name,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if matches!(route, Route::Ledger | Route::Html) {
        assert!(!first.stdout.is_empty());
        assert_eq!(
            first.stdout, second.stdout,
            "{} {route:?}: whole rendered artifact",
            probe.name
        );
        if probe.name == "control_vacuity_warn" {
            assert!(String::from_utf8_lossy(&first.stdout).contains("Hollow"));
        }
        return;
    }
    let mut values = [parsed(&first), parsed(&second)];
    for value in &mut values {
        match route {
            Route::Verify => unreachable!("oracle checked separately"),
            Route::Sweep => {
                let cells = value
                    .pointer_mut("/sweep/results")
                    .and_then(Value::as_array_mut)
                    .expect("sweep cells");
                assert_eq!(cells.len(), 1);
                normalize(&mut cells[0]["verification"], probe.elapsed).unwrap();
                if probe.status != 0 {
                    normalize(
                        value
                            .pointer_mut("/sweep/minimal_counterexample/verification")
                            .expect("minimal failing envelope"),
                        probe.elapsed,
                    )
                    .unwrap();
                }
                assert_eq!(
                    *value,
                    sweep_expected(expected, probe),
                    "{} whole sweep",
                    probe.name
                );
            }
            Route::Chain => {
                normalize(
                    value.pointer_mut("/layers/0/detail").expect("chain detail"),
                    probe.elapsed,
                )
                .unwrap();
                assert_eq!(
                    *value,
                    chain_expected(run, expected, probe),
                    "{} whole chain",
                    probe.name
                );
            }
            Route::Mutate if guard => assert_eq!(
                *value,
                json!({"fsl": "1.0", "result": "error", "kind": "semantics", "message": "mutate expects a spec-like FSL file"})
            ),
            Route::Mutate if probe.status != 0 => {
                normalize(value, probe.elapsed).unwrap();
                assert_eq!(value, expected, "{} failing mutate re-emission", probe.name);
            }
            Route::Mutate => {
                assert_eq!(value["result"], "mutated");
                assert_eq!(value["baseline"], "verified");
                assert_eq!(value["summary"]["total"], 0);
            }
            Route::Db | Route::Ai | Route::Domain => {
                assert_eq!(value["result"], "verified_under_assumptions");
                assert_eq!(
                    value["kernel"],
                    projected(probe),
                    "{} whole public kernel projection",
                    probe.name
                );
            }
            Route::Ledger | Route::Html => unreachable!(),
        }
    }
    assert_eq!(
        values[0], values[1],
        "{} {route:?}: whole repeated outer envelope",
        probe.name
    );
}

#[test]
fn preservation_controls_compare_whole_envelopes_and_artifacts_twice() {
    for probe in PROBES.iter().filter(|probe| probe.elapsed > 0) {
        let run = Run::new(probe);
        let mut expected = oracle(&run, probe);
        normalize(&mut expected, probe.elapsed).unwrap();
        let mut repeat = oracle(&run, probe);
        normalize(&mut repeat, probe.elapsed).unwrap();
        assert_eq!(expected, repeat, "{} Verify repeat", probe.name);
        match probe.name {
            "control_seam_fail" => {
                assert_eq!(expected["implements"]["result"], "refinement_failed");
            }
            "control_seam_pass" | "control_trace" => {
                assert_eq!(expected["implements"]["result"], "refines");
            }
            "control_governance_fail" => assert_eq!(
                expected["governance"]["preservations"][0]["result"],
                "refinement_failed"
            ),
            "control_vacuity_warn" => assert!(
                expected["warnings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|warning| warning["kind"] == "vacuous_implication"
                        && warning["name"] == "Hollow")
            ),
            _ => {}
        }
        println!(
            "PAIR {}/Verify: produced/expected exit={}, whole agreement=true twice",
            probe.name, probe.status
        );
        for route in ROUTES
            .iter()
            .copied()
            .chain(std::iter::once(Route::Domain))
            .filter(|route| *route != Route::Verify && applicable(*route, probe))
        {
            control_route(&run, route, probe, &expected);
            println!("PAIR {}/{route:?}: whole agreement=true twice", probe.name);
        }
    }
}

#[test]
fn control_elapsed_exclusions_are_live_and_reject_missing_leaves() {
    let probe = PROBES
        .iter()
        .find(|probe| probe.name == "control_trace")
        .unwrap();
    let run = Run::new(probe);
    let original = oracle(&run, probe);
    for pointer in [
        "/cost",
        "/cost/elapsed_s",
        "/cost/solver/check_elapsed_s",
        "/cost/properties/0/elapsed_s",
    ] {
        let mut mutated = original.clone();
        *mutated.pointer_mut(pointer).expect("live exclusion target") = Value::Null;
        assert!(
            normalize(&mut mutated, probe.elapsed).is_err(),
            "dead exclusion accepted at {pointer}"
        );
    }
    let mut missing_property = original.clone();
    missing_property["cost"]["properties"]
        .as_array_mut()
        .unwrap()
        .pop()
        .unwrap();
    assert!(normalize(&mut missing_property, probe.elapsed).is_err());
    for invalid in [json!(-1), json!("0")] {
        let mut mutated = original.clone();
        mutated["cost"]["elapsed_s"] = invalid;
        assert!(normalize(&mut mutated, probe.elapsed).is_err());
    }
}
