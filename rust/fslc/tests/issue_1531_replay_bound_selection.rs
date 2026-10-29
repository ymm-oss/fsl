// SPDX-License-Identifier: Apache-2.0

//! #1531: replay must use the run's bound selection. Expectations come from
//! the literal transitions and the accepted fail-closed Seq policy, not from
//! another engine's output. Every engine/depth is a separate fail-first cell.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

const RANGE: &str = "spec RangeSelected {\n  state { n: 0..1 }\n  init { n = 0 }\n  action inc() { n = n + 1 }\n  invariant P { n <= 2 }\n}\n";
const MULTI: &str = "spec ReviewSelectedOverflowMultiple {\n  type K = 0..1\n  state { xs: Seq<K, 1> }\n  init { xs = Seq {} }\n  action take() { xs = xs.push(1) }\n  invariant P { xs.size() <= 3 }\n}\n";

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fixture {
    Range,
    Multi,
}

impl Fixture {
    fn source(self) -> &'static str {
        match self {
            Self::Range => RANGE,
            Self::Multi => MULTI,
        }
    }

    fn bound(self) -> &'static str {
        match self {
            Self::Range => "_bounds_n",
            Self::Multi => "_bounds_xs",
        }
    }

    fn action(self) -> &'static str {
        match self {
            Self::Range => "inc",
            Self::Multi => "take",
        }
    }

    fn state(self, step: usize) -> Value {
        match self {
            Self::Range => json!({"n": step}),
            Self::Multi => json!({"xs": vec![1; step]}),
        }
    }

    fn trace(self, last: usize) -> Value {
        let field = if self == Self::Range { "n" } else { "xs" };
        let line = if self == Self::Range { 4 } else { 5 };
        json!((0..=last).map(|step| {
            if step == 0 {
                json!({"step": 0, "state": self.state(0)})
            } else {
                json!({
                    "step": step,
                    "state": self.state(step),
                    "action": {"name": self.action(), "params": {}, "loc": {"line": line, "column": 3}},
                    "changes": {field: {"from": self.state(step - 1)[field], "to": self.state(step)[field]}},
                    "blame": {"guards": [], "effects": []}
                })
            }
        }).collect::<Vec<_>>())
    }
}

struct Model(PathBuf);

impl Model {
    fn new(source: &str) -> Self {
        // Keep calibration artifacts in this worktree, with no cleanup that
        // could erase evidence. No shared target or external temporary tree.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.codex/tmp")
            .join(format!(
                "issue-1531-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).expect("create test directory");
        std::fs::write(root.join("model.fsl"), source).expect("write literal fixture");
        Self(root)
    }

    fn run_bin(&self, binary: &str, args: &[&str]) -> (Value, i32) {
        let output = Command::new(binary)
            .args(args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .output()
            .expect("run native binary");
        let exit = output.status.code().expect("native exit code");
        assert!(
            output.stderr.is_empty(),
            "{args:?}: exit={exit}, stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{args:?}: exit={exit}, invalid JSON {error}: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        eprintln!("{args:?}: exit={exit} {value}");
        (value, exit)
    }

    fn run(&self, args: &[&str]) -> (Value, i32) {
        self.run_bin(env!("CARGO_BIN_EXE_fslc"), args)
    }
}

fn assert_violation(value: &Value, exit: i32, fixture: Fixture, selected: bool) {
    let (kind, name, step) = if selected {
        ("invariant", "P", 3)
    } else {
        ("type_bound", fixture.bound(), 2)
    };
    assert_violation_at(value, exit, kind, name, step);
    assert_eq!(value["trace"], fixture.trace(step), "{value}");
    assert_eq!(value["last_action"]["name"], fixture.action(), "{value}");
    assert_eq!(value["last_action"]["params"], json!({}), "{value}");
}

fn assert_violation_at(value: &Value, exit: i32, kind: &str, name: &str, step: usize) {
    assert_eq!(
        (
            exit,
            &value["result"],
            &value["violation_kind"],
            &value["invariant"],
            &value["violated_at_step"]
        ),
        (
            1,
            &json!("violated"),
            &json!(kind),
            &json!(name),
            &json!(step)
        ),
        "{value}"
    );
    assert_eq!(value["message"], Value::Null, "{value}");
    assert_eq!(value["cti"], Value::Null, "{value}");
}

fn assert_shallow_cti(value: &Value, exit: i32, fixture: Fixture) {
    // Preservation, not a detector: today's induction strengthens P with its
    // implicit bound and finds an arbitrary-state, one-step bounds CTI.
    assert_eq!(
        (
            exit,
            &value["result"],
            &value["invariant"],
            &value["k"],
            &value["trace_type"]
        ),
        (
            1,
            &json!("unknown_cti"),
            &json!(fixture.bound()),
            &json!(1),
            &json!("induction_cti")
        ),
        "{value}"
    );
    assert_eq!(value["cti"]["violated_at"], 1, "{value}");
    assert_eq!(value["message"], Value::Null, "{value}");
    assert_eq!(value["trace"], Value::Null, "{value}");
    let states = value["cti"]["states"]
        .as_array()
        .expect("bounds CTI states");
    assert_eq!(states.len(), 2, "{value}");
    assert_eq!(states[0]["step"], 0, "{value}");
    assert_eq!(states[1]["step"], 1, "{value}");
    assert_eq!(states[0]["action"], Value::Null, "{value}");
    if fixture == Fixture::Range {
        assert_eq!(states[0]["state"], json!({"n": 1}), "{value}");
        assert_eq!(states[1]["state"], json!({"n": 2}), "{value}");
    } else {
        let before = states[0]["state"]["xs"].as_array().expect("pre xs");
        assert_eq!(before.len(), 1, "{value}");
        assert!(before[0] == 0 || before[0] == 1, "{value}");
        assert_eq!(states[0]["state"], json!({"xs": before}), "{value}");
        assert_eq!(states[1]["state"], json!({"xs": [before[0], 1]}), "{value}");
    }
    assert_eq!(states[1]["action"]["name"], fixture.action(), "{value}");
    assert_eq!(states[1]["action"]["params"], json!({}), "{value}");
}

fn selected(fixture: Fixture, engine: &str, depth: usize) {
    let model = Model::new(fixture.source());
    let (value, exit) = model.run(&[
        "verify",
        "model.fsl",
        "--engine",
        engine,
        "--depth",
        &depth.to_string(),
        "--no-cache",
        "--property",
        "P",
    ]);
    if engine == "induction" && depth >= 2 {
        // #1598: the selected base establishes the step's implicit bounds first.
        assert_violation(&value, exit, fixture, false);
        assert_eq!(value["completeness"], "bounded", "{value}");
    } else if fixture == Fixture::Range && depth >= 3 {
        // Detector: replay must not roll back n=2 and diverge at step three.
        assert_violation(&value, exit, fixture, true);
    } else if fixture == Fixture::Multi && depth >= 2 {
        // Detector on explicit; preservation on the symbolic engines (#1519).
        assert_eq!(
            (exit, &value["result"], &value["kind"], &value["message"]),
            (
                2,
                &json!("error"),
                &json!("semantics"),
                &json!("model sequence length exceeds capacity")
            ),
            "{value}"
        );
        for key in ["trace", "cti", "invariant", "violation_kind"] {
            assert_eq!(value[key], Value::Null, "{key}: {value}");
        }
    } else if engine == "induction" {
        assert_shallow_cti(&value, exit, fixture);
    } else {
        assert_eq!((exit, &value["result"]), (0, &json!("verified")), "{value}");
        assert_eq!(value["invariants_checked"], json!(["P"]), "{value}");
        for key in ["trace", "cti", "invariant", "violation_kind", "message"] {
            assert_eq!(value[key], Value::Null, "{key}: {value}");
        }
    }
}

macro_rules! depth_cells {
    ($module:ident, $fixture:expr, $depth:literal) => {
        mod $module {
            use super::*;
            #[test]
            fn bmc() {
                selected($fixture, "bmc", $depth);
            }
            #[test]
            fn induction() {
                selected($fixture, "induction", $depth);
            }
            #[test]
            fn explicit() {
                selected($fixture, "explicit", $depth);
            }
        }
    };
}

mod range_selected {
    use super::*;
    depth_cells!(depth_1, Fixture::Range, 1);
    depth_cells!(depth_2, Fixture::Range, 2);
    depth_cells!(depth_3, Fixture::Range, 3);
    depth_cells!(depth_4, Fixture::Range, 4);
    depth_cells!(depth_5, Fixture::Range, 5);
}

mod multi_selected {
    use super::*;
    depth_cells!(depth_1, Fixture::Multi, 1);
    depth_cells!(depth_2, Fixture::Multi, 2);
    depth_cells!(depth_3, Fixture::Multi, 3);
    depth_cells!(depth_4, Fixture::Multi, 4);
    depth_cells!(depth_5, Fixture::Multi, 5);
}

mod review_coverage {
    use super::*;

    const INIT_IDLE: &str = "spec InitIdle {\n  type K = 0..1\n  state { xs: Seq<K, 1>, t: Bool }\n  init { xs = Seq { 0, 1 }  t = false }\n  action idle() { t = t }\n  invariant P { xs.size() <= 5 }\n}\n";
    const RANGE_REACH: &str = "spec RangeReach {\n  state { n: 0..1 }\n  init { n = 0 }\n  action inc() { requires n < 4  n = n + 1 }\n  reachable R { n == 3 }\n  invariant P { n <= 9 }\n  trans T { n == old(n) + 1 and n <= 2 }\n  leadsTo L { n == 0 ~> n == 9 }\n}\n";

    fn verify(source: &str, engine: &str, depth: usize, property: &str) -> (Value, i32) {
        Model::new(source).run(&[
            "verify",
            "model.fsl",
            "--engine",
            engine,
            "--depth",
            &depth.to_string(),
            "--no-cache",
            "--property",
            property,
        ])
    }

    fn initial_overflow(engine: &str, depth: usize) {
        let (value, exit) = verify(INIT_IDLE, engine, depth, "P");
        if engine == "induction" {
            assert_violation_at(&value, exit, "type_bound", "_bounds_xs", 0);
            assert_eq!(value["completeness"], "bounded", "{value}");
            assert_eq!(
                value["trace"],
                json!([{"step": 0, "state": {"xs": [0, 1], "t": false}}]),
                "{value}"
            );
            return;
        }
        assert_eq!(
            (exit, &value["result"], &value["kind"], &value["message"]),
            (
                2,
                &json!("error"),
                &json!("semantics"),
                &json!("model sequence length exceeds capacity")
            ),
            "{value}"
        );
        for key in ["trace", "cti", "invariant", "violation_kind"] {
            assert_eq!(value[key], Value::Null, "{key}: {value}");
        }
    }

    macro_rules! initial_cells {
        ($module:ident, $depth:literal) => {
            mod $module {
                use super::*;
                #[test]
                fn bmc() {
                    initial_overflow("bmc", $depth);
                }
                #[test]
                fn induction() {
                    initial_overflow("induction", $depth);
                }
                #[test]
                fn explicit() {
                    initial_overflow("explicit", $depth);
                }
            }
        };
    }

    initial_cells!(initial_depth_1, 1);
    initial_cells!(initial_depth_3, 3);

    #[test]
    fn initial_depth_0_explicit_preserves_verified() {
        // The guard covers successors, not the initial frame itself.
        let (value, exit) = verify(INIT_IDLE, "explicit", 0, "P");
        assert_eq!((exit, &value["result"]), (0, &json!("verified")), "{value}");
        assert_eq!(value["checked_to_depth"], 0, "{value}");
        assert_eq!(value["completeness"], "bounded", "{value}");
        assert_eq!(value["invariants_checked"], json!(["P"]), "{value}");
        assert_eq!(value["closure"], false, "{value}");
        assert_eq!(value["trace"], Value::Null, "{value}");
    }

    fn witness_trace(last: usize) -> Value {
        // Reachable/deadlock/leadsTo witnesses have no per-step blame;
        // violation traces do. Compare each complete published trace exactly.
        json!((0..=last).map(|step| {
            if step == 0 {
                json!({"step": 0, "state": {"n": 0}})
            } else {
                json!({
                    "step": step, "state": {"n": step},
                    "action": {"name": "inc", "params": {}, "loc": {"line": 4, "column": 3}},
                    "changes": {"n": {"from": step - 1, "to": step}}
                })
            }
        }).collect::<Vec<_>>())
    }

    #[test]
    fn reachable_bmc() {
        let (value, exit) = verify(RANGE_REACH, "bmc", 5, "R");
        assert_eq!((exit, &value["result"]), (0, &json!("verified")), "{value}");
        assert_eq!(
            value["reachables"],
            json!({"R": {
                "witnessed_at_step": 3, "witness": witness_trace(3)
            }}),
            "{value}"
        );
        assert_eq!(
            value["deadlock"],
            json!({
                "found": true, "at_step": 4, "trace": witness_trace(4)
            }),
            "{value}"
        );
    }

    fn deadlock(engine: &str) {
        let (value, exit) = verify(RANGE_REACH, engine, 5, "P");
        let result = if engine == "explicit" {
            "proved"
        } else {
            "verified"
        };
        assert_eq!((exit, &value["result"]), (0, &json!(result)), "{value}");
        assert_eq!(value["invariants_checked"], json!(["P"]), "{value}");
        assert_eq!(value["reachables"], json!({}), "{value}");
        assert_eq!(
            value["deadlock"],
            json!({
                "found": true, "at_step": 4, "trace": witness_trace(4)
            }),
            "{value}"
        );
    }

    #[test]
    fn deadlock_bmc() {
        deadlock("bmc");
    }
    #[test]
    fn deadlock_explicit() {
        deadlock("explicit");
    }

    fn trans(engine: &str) {
        let (value, exit) = verify(RANGE_REACH, engine, 5, "T");
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["violation_kind"],
                &value["invariant"],
                &value["violated_at_step"]
            ),
            (
                1,
                &json!("violated"),
                &json!("trans"),
                &json!("T"),
                &json!(3)
            ),
            "{value}"
        );
        assert_eq!(value["trans"], "T", "{value}");
        assert_eq!(value["trace_type"], "trans", "{value}");
        assert_eq!(value["trace"], Fixture::Range.trace(3), "{value}");
    }

    #[test]
    fn trans_bmc() {
        trans("bmc");
    }
    #[test]
    fn trans_explicit() {
        trans("explicit");
    }

    #[test]
    fn leadsto_bmc() {
        let (value, exit) = verify(RANGE_REACH, "bmc", 5, "L");
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["violation_kind"],
                &value["invariant"]
            ),
            (1, &json!("violated"), &json!("leadsTo"), &json!("L")),
            "{value}"
        );
        assert_eq!(value["trace_type"], "leadsTo", "{value}");
        assert_eq!(value["pending_since"], 0, "{value}");
        assert_eq!(value["stutter"], true, "{value}");
        assert_eq!(value["trace"], witness_trace(4), "{value}");
    }

    #[test]
    fn selected_seq_bound_explicit() {
        let (value, exit) = verify(MULTI, "explicit", 3, Fixture::Multi.bound());
        assert_violation(&value, exit, Fixture::Multi, false);
        assert_eq!(value["trace_type"], "type_bound", "{value}");
    }
}

mod all_bounds {
    use super::*;

    // Guarded counterparts reach one legal state then deadlock. Unlike the
    // violated-input controls, these enter scenarios' replay call itself;
    // fsl-bmc also replays both reachable and deadlock witnesses.
    const RANGE_STOP: &str = "spec RangeStop {\n  state { n: 0..1 }\n  init { n = 0 }\n  action inc() { requires n < 1 n = n + 1 }\n  reachable R { n == 1 }\n}\n";
    const MULTI_STOP: &str = "spec MultiStop {\n  type K = 0..1\n  state { xs: Seq<K, 1> }\n  init { xs = Seq {} }\n  action take() { requires xs.size() < 1 xs = xs.push(1) }\n  reachable R { xs.size() == 1 }\n}\n";

    fn verify(fixture: Fixture, engine: &str) {
        let model = Model::new(fixture.source());
        let (value, exit) = model.run(&[
            "verify",
            "model.fsl",
            "--engine",
            engine,
            "--depth",
            "4",
            "--no-cache",
        ]);
        assert_violation(&value, exit, fixture, false);
    }

    macro_rules! verify_controls {
        ($module:ident, $fixture:expr, $stop:expr) => {
            mod $module {
                use super::*;
                #[test]
                fn bmc() { verify($fixture, "bmc"); }
                #[test]
                fn induction() { verify($fixture, "induction"); }
                #[test]
                fn explicit() { verify($fixture, "explicit"); }

                #[test]
                fn scenarios_violated_input() {
                    let model = Model::new($fixture.source());
                    let (value, exit) = model.run(&["scenarios", "model.fsl", "--depth", "4"]);
                    assert_violation(&value, exit, $fixture, false);
                }

                #[test]
                fn fsl_bmc_violation_replay() {
                    let model = Model::new($fixture.source());
                    let (value, exit) = model.run_bin(env!("CARGO_BIN_EXE_fsl-bmc"), &["model.fsl", "4"]);
                    assert_eq!(exit, 0, "{value}");
                    assert_eq!(value["violation"], json!({"kind": "type_bound", "name": $fixture.bound(), "step": 2}), "{value}");
                    // The small BMC binary deliberately has no source locations.
                    let mut expected = $fixture.trace(2);
                    for entry in expected.as_array_mut().unwrap() {
                        entry.as_object_mut().unwrap().remove("blame");
                        if let Some(action) = entry.get_mut("action") {
                            action.as_object_mut().unwrap().remove("loc");
                        }
                    }
                    assert_eq!(value["witnesses"]["violation"], expected, "{value}");
                }

                #[test]
                fn scenarios_reachable_and_deadlock_replay() {
                    let model = Model::new($stop);
                    let (value, exit) = model.run(&["scenarios", "model.fsl", "--depth", "4"]);
                    assert_eq!((exit, &value["result"]), (0, &json!("scenarios")), "{value}");
                    let scenarios = value["scenarios"].as_array().expect("scenarios");
                    let reachable = scenarios.iter().find(|entry| entry["kind"] == "reachable").expect("reachable scenario");
                    assert_eq!(reachable["property"], "R", "{value}");
                    assert_eq!(reachable["initial_state"], $fixture.state(0), "{value}");
                    let steps = reachable["steps"].as_array().expect("scenario steps");
                    assert_eq!(steps.len(), 1, "{value}");
                    assert_eq!(steps[0]["action"], $fixture.action(), "{value}");
                    assert_eq!(steps[0]["params"], json!({}), "{value}");
                    assert_eq!(reachable["expected_states"], json!([$fixture.state(1)]), "{value}");
                    assert!(scenarios.iter().any(|entry| entry["kind"] == "deadlock"), "{value}");
                }

                #[test]
                fn fsl_bmc_reachable_and_deadlock_replay() {
                    let model = Model::new($stop);
                    let (value, exit) = model.run_bin(env!("CARGO_BIN_EXE_fsl-bmc"), &["model.fsl", "4"]);
                    assert_eq!(exit, 0, "{value}");
                    assert_eq!(value["violation"], Value::Null, "{value}");
                    assert_eq!(value["reachables"], json!({"R": 1}), "{value}");
                    assert_eq!(value["deadlock_step"], 1, "{value}");
                    for trace in [&value["witnesses"]["reachables"]["R"], &value["witnesses"]["deadlock"]] {
                        let entries = trace.as_array().expect("replayed witness");
                        assert_eq!(entries.len(), 2, "{value}");
                        assert_eq!(entries[0]["state"], $fixture.state(0), "{value}");
                        assert_eq!(entries[1]["state"], $fixture.state(1), "{value}");
                        assert_eq!(entries[1]["action"], json!({"name": $fixture.action(), "params": {}}), "{value}");
                    }
                }
            }
        };
    }

    verify_controls!(range, Fixture::Range, RANGE_STOP);
    verify_controls!(multi, Fixture::Multi, MULTI_STOP);

    #[test]
    fn mutation_ensures_replay() {
        // Removing the state update violates inc's ensures, not a bound.
        // This deliberately exercises mutation_model_verdict's replay branch
        // (type-bound-only mutants do not call replay at that site).
        let model = Model::new(
            "spec MutationReplay {\n  state { n: 0..1 }\n  init { n = 0 }\n  action inc() { requires n == 0 n = 1 ensures n == 1 }\n}\n",
        );
        let (value, exit) = model.run(&[
            "mutate",
            "model.fsl",
            "--depth",
            "4",
            "--max-mutants",
            "100",
            "--oracle-attribution",
        ]);
        assert_eq!((exit, &value["result"]), (0, &json!("mutated")), "{value}");
        let mutants = value["mutants"].as_array().expect("mutants");
        let removed_update = mutants
            .iter()
            .find(|mutant| mutant["op"] == "assignment_remove" && mutant["loc"]["line"] == 4)
            .expect("action update removal mutant");
        assert_eq!(removed_update["status"], "killed", "{value}");
        assert_eq!(removed_update["killed_by"], "inc", "{value}");
        assert_eq!(removed_update["killers"], json!(["inc"]), "{value}");
        assert_eq!(removed_update["error"], Value::Null, "{value}");
    }

    #[test]
    fn refine_progress_lasso_replay() {
        // This existing fixture shape enters refinement.rs's progress replay,
        // unlike an impl_violated input that returns before that call.
        let model = Model::new(include_str!(
            "fixtures/inline_implements_preserve_progress/req_plain.fsl"
        ));
        std::fs::write(
            model.0.join("policy.fsl"),
            include_str!("fixtures/inline_implements_preserve_progress/policy.fsl"),
        )
        .expect("write abstraction");
        std::fs::write(
            model.0.join("map.fsl"),
            include_str!("fixtures/inline_implements_preserve_progress/progress_refines.fsl"),
        )
        .expect("write progress mapping");
        let (value, exit) = model.run(&[
            "refine",
            "model.fsl",
            "policy.fsl",
            "map.fsl",
            "--depth",
            "8",
        ]);
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["kind"],
                &value["violation_kind"],
                &value["invariant"]
            ),
            (
                1,
                &json!("refinement_failed"),
                &json!("progress_lost"),
                &json!("leadsTo"),
                &json!("EveryClaimDecided")
            ),
            "{value}"
        );
        assert_eq!(
            value["progress_failure"], "lasso_blocks_progress",
            "{value}"
        );
        assert_eq!(value["message"], Value::Null, "{value}");
        let trace = value["impl_trace"].as_array().expect("progress trace");
        assert_eq!(trace.len(), 4, "{value}");
        assert_eq!(value["loop_start"], 1, "{value}");
        for (step, entry) in trace.iter().enumerate() {
            assert_eq!(entry["step"], step, "{value}");
            assert_eq!(
                entry["state"],
                if step == 0 {
                    json!({"d": "DSubmitted", "phase": 0})
                } else {
                    json!({"d": "DAutoReview", "phase": (step - 1) % 2})
                },
                "{value}"
            );
            if step > 0 {
                assert_eq!(
                    entry["action"]["name"],
                    if step == 1 { "route" } else { "heartbeat" },
                    "{value}"
                );
                assert_eq!(entry["action"]["params"], json!({}), "{value}");
            }
        }
    }
}
