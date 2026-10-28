// SPDX-License-Identifier: Apache-2.0

//! #1519: a full Seq's pushed temporary retains the appended element.
//! Literal expectations below are independent of the symbolic implementation:
//! push appends, pop removes the first element, and only state assignment is
//! capacity bounded. Each engine has its own test, including on a failing base,
//! so a symbolic failure cannot hide explicit or Monitor observations.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use fsl_core::{FsResolver, FslValue, build_model, parse_kernel_source};
use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Property {
    InvariantAndReachable,
    Unreachable,
    StateOverflow,
    ActionRead,
}

struct Cell {
    name: &'static str,
    capacity: usize,
    initial: &'static [i64],
    expression: &'static str,
    property: Property,
}

impl Cell {
    fn induction_shape(&self) -> Option<&'static str> {
        // Induction needs a reachable-state lemma to exclude unreachable partial reads.
        match self.name {
            "H2" | "H3" | "F1" | "F2" => Some("xs.size() == 1"),
            "M1" => Some("c and xs.size() == 1"),
            _ => None,
        }
    }

    fn source(&self) -> String {
        let initial = self
            .initial
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let properties = match self.property {
            Property::InvariantAndReachable => format!(
                "  invariant {} {{ {} }}\n  reachable {}Reached {{ {} }}",
                self.name, self.expression, self.name, self.expression
            ),
            Property::Unreachable => {
                format!("  reachable {} {{ {} }}", self.name, self.expression)
            }
            Property::StateOverflow => format!("  invariant {} {{ true }}", self.name),
            // Deliberately reject the concrete successor to require a replayed
            // counterexample carrying the appended value, not a false proof.
            Property::ActionRead => format!("  invariant {} {{ x == 0 }}", self.name),
        };
        let properties = if let Some(shape) = self.induction_shape() {
            format!("  invariant Shape {{ {shape} }}\n{properties}")
        } else {
            properties
        };
        let action = match self.property {
            Property::StateOverflow => "action overflow() { xs = xs.push(1) }".to_owned(),
            Property::ActionRead => format!("action take() {{ x = {} }}", self.expression),
            _ => "action idle() { }".to_owned(),
        };
        let extra_state = if self.property == Property::ActionRead {
            ", x: K"
        } else {
            ""
        };
        let extra_init = if self.property == Property::ActionRead {
            " x = 0"
        } else {
            ""
        };
        format!(
            "spec PushTemporary {{\n  type K = 0..1\n  state {{ xs: Seq<K, {}>{extra_state} }}\n  init {{ xs = Seq {{ {initial} }}{extra_init} }}\n  {action}\n{properties}\n}}\n",
            self.capacity
        )
    }

    fn action(&self) -> &'static str {
        match self.property {
            Property::StateOverflow => "overflow",
            Property::ActionRead => "take",
            _ => "idle",
        }
    }

    fn state(&self) -> BTreeMap<String, FslValue> {
        let mut state = BTreeMap::from([(
            "xs".to_owned(),
            FslValue::Seq(self.initial.iter().copied().map(FslValue::Int).collect()),
        )]);
        if self.property == Property::ActionRead {
            state.insert("x".to_owned(), FslValue::Int(0));
        }
        state
    }
}

// Independent induction-step and option-F controls; keep the existing 166
// tests and their expectations unchanged.
mod induction_frames {
    use super::*;

    const IS1: &str = r"spec Ind1 {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { } }
  action take() { xs = xs.push(1) }
  invariant P { xs.size() <= 1 or xs.at(1) == 1 }
}
";

    const IS2: &str = r"spec Ind3 {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { } }
  action take() { xs = xs.push(1) }
  trans T { xs.size() <= 1 or xs.at(1) == 1 }
}
";

    const IS3: &str = r"spec Ind2 {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { } }
  action take() { xs = xs.push(1) }
  invariant P { xs.size() <= 3 }
}
";

    const FR1: &str = r"spec FR1 {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { } }
  action take() { requires xs.size() < 1  xs = xs.push(1) }
  action drop() { requires xs.size() > 0  xs = xs.pop() }
  invariant P { xs.size() <= 1 }
  reachable R { xs.size() == 1 and xs.at(0) == 1 }
}
";

    const FR2: &str = r"spec FR2 {
  type K = 0..1
  type C = 0..3
  state { xs: Seq<K, 1>, c: C }
  init { xs = Seq { 0 }  c = 0 }
  action take() { xs = xs.push(1) }
  action tick() { requires c < 3  c = c + 1 }
  invariant Q { c <= 3 }
  reachable Rc { c == 1 }
}
";

    const FR3: &str = r"spec FR3 {
  type K = 0..1
  state { xs: Seq<K, 2> }
  init { xs = Seq { } }
  action take() { xs = xs.push(1) }
  reachable R1 { xs.size() == 1 }
}
";

    struct Fixture {
        name: &'static str,
        source: &'static str,
        depth: &'static str,
    }

    impl Fixture {
        fn run(&self, args: &[&str]) -> (Value, i32) {
            // Cell is only the existing CLI/temporary-directory harness here;
            // no expectation or model source comes from its generated spec.
            let cell = Cell {
                name: self.name,
                capacity: 1,
                initial: &[],
                expression: "true",
                property: Property::InvariantAndReachable,
            };
            let model = Model::new(&cell);
            std::fs::write(model.0.join("model.fsl"), self.source).expect("write frame model");
            model.run(&cell, args)
        }

        fn check(&self) {
            let (value, exit) = self.run(&["check", "model.fsl"]);
            assert_eq!(
                (exit, &value["result"], &value["message"]),
                (0, &json!("ok"), &Value::Null),
                "{} check: {value}",
                self.name
            );
        }

        fn verify(&self, engine: &str, property: Option<&str>) -> (Value, i32) {
            let mut args = vec![
                "verify",
                "model.fsl",
                "--engine",
                engine,
                "--depth",
                self.depth,
                "--no-cache",
            ];
            if let Some(property) = property {
                args.extend(["--property", property]);
            }
            self.run(&args)
        }

        fn monitor(&self) -> fsl_runtime::Monitor {
            let kernel = parse_kernel_source(self.source, &FsResolver::new("."))
                .expect("frame Monitor parse");
            fsl_runtime::Monitor::new(build_model(kernel).expect("frame Monitor model"))
                .expect("frame Monitor init")
        }
    }

    fn assert_success(value: &Value, exit: i32, result: &str) {
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["invariant"],
                &value["violation_kind"],
                &value["message"],
                &value["cti"],
            ),
            (
                0,
                &json!(result),
                &Value::Null,
                &Value::Null,
                &Value::Null,
                &Value::Null,
            ),
            "{value}"
        );
    }

    fn assert_error(value: &Value, exit: i32, kind: &str, message: &str) {
        assert_eq!(
            (exit, &value["result"], &value["kind"], &value["cti"]),
            (2, &json!("error"), &json!(kind), &Value::Null),
            "{value}"
        );
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|produced| produced.contains(message)),
            "{value}"
        );
    }

    fn assert_capacity_error(value: &Value, exit: i32) {
        assert_error(
            value,
            exit,
            "semantics",
            "model sequence length exceeds capacity",
        );
    }

    fn assert_bounds_cti(value: &Value, exit: i32) {
        // Preservation control for A: genuine bounds CTIs still reconstruct
        // the append, rather than being rejected with the non-bounds CTIs.
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["invariant"],
                &value["k"],
                &value["cti"]["violated_at"],
                &value["message"],
            ),
            (
                1,
                &json!("unknown_cti"),
                &json!("_bounds_xs"),
                &json!(1),
                &json!(1),
                &Value::Null,
            ),
            "{value}"
        );
        let trace = value["cti"]["states"].as_array().expect("bounds CTI");
        assert_eq!(trace.len(), 2, "{value}");
        let before = trace[0]["state"]["xs"].as_array().expect("CTI pre xs");
        let after = trace[1]["state"]["xs"].as_array().expect("CTI post xs");
        assert_eq!(before.len(), 1, "{value}");
        assert!(before[0] == 0 || before[0] == 1, "{value}");
        assert_eq!(after, &vec![before[0].clone(), json!(1)], "{value}");
        assert!(after.len() > 1, "_bounds_xs must really be false: {value}");
        // P is true in both CTI states, not an outstanding false property.
        assert!(before.len() <= 3 && after.len() <= 3, "{value}");
        assert!(value["k_used"]["P"].is_null() || value["k_used"]["P"] == 1);
        assert_eq!(trace[1]["action"]["name"], "take", "{value}");
        assert_eq!(trace[1]["action"]["params"], json!({}), "{value}");
    }

    fn induction_verify(fixture: &Fixture, engine: &str, property: Option<&str>) {
        let (value, exit) = fixture.verify(engine, property);
        if engine != "induction" {
            // [] -> [1] is valid and satisfies P/T at depth one. Explicit
            // remains bounded here: the overflowing second take is not run.
            assert_success(&value, exit, "verified");
        } else if fixture.name == "IS3" {
            assert_bounds_cti(&value, exit);
        } else {
            // Detector for MAJOR-1 / option A, on both selection routes:
            // induction may start with [0] then append 1. P/T is TRUE on
            // [0,1], but the N-slot successor loses that suffix during the
            // symbolic query. Reconstructing it as unknown_cti is false
            // evidence; a non-bounds SAT must fail closed, never render it.
            assert_capacity_error(&value, exit);
        }
    }

    fn seq_state(items: &[i64]) -> BTreeMap<String, FslValue> {
        BTreeMap::from([(
            "xs".to_owned(),
            FslValue::Seq(items.iter().copied().map(FslValue::Int).collect()),
        )])
    }

    fn assert_true_properties(monitor: &fsl_runtime::Monitor, state: &BTreeMap<String, FslValue>) {
        for property in monitor
            .model
            .invariants
            .iter()
            .chain(&monitor.model.transitions)
        {
            assert_eq!(
                fsl_runtime::eval(
                    &property.expr,
                    state,
                    &mut BTreeMap::new(),
                    &monitor.model,
                    Some(&monitor.state),
                ),
                Ok(FslValue::Bool(true)),
                "Monitor property {} on {state:?}",
                property.name
            );
        }
    }

    fn induction_monitor(fixture: &Fixture) {
        let mut monitor = fixture.monitor();
        assert_eq!(monitor.model.actions.len(), 1);
        assert_eq!(monitor.state, seq_state(&[]));
        assert_true_properties(&monitor, &monitor.state);
        let step = monitor
            .attempt("take", &BTreeMap::new())
            .expect("first take");
        eprintln!("{} Monitor step 1: {step:?}", fixture.name);
        assert_eq!(
            step,
            fsl_runtime::StepResult {
                action: "take".to_owned(),
                params: BTreeMap::new(),
                state: seq_state(&[1]),
                attempted_state: None,
                violation: None,
            }
        );
        assert_eq!(monitor.state, seq_state(&[1]));
        assert_true_properties(&monitor, &monitor.state);
        // Unlike the bound, P/T is also true on the actual attempted overflow.
        // This independent evaluator detects the misleading reconstructed CTI.
        assert_true_properties(&monitor, &seq_state(&[1, 1]));
        for step_number in 2..=4 {
            let step = monitor
                .attempt("take", &BTreeMap::new())
                .expect("overflow take");
            eprintln!("{} Monitor step {step_number}: {step:?}", fixture.name);
            assert_eq!(
                step,
                fsl_runtime::StepResult {
                    action: "take".to_owned(),
                    params: BTreeMap::new(),
                    state: seq_state(&[1]),
                    attempted_state: Some(seq_state(&[1, 1])),
                    violation: Some(fsl_runtime::Violation {
                        kind: "type_bound".to_owned(),
                        name: "_bounds_xs".to_owned(),
                        step: step_number,
                    }),
                }
            );
            assert_eq!(monitor.state, seq_state(&[1]));
        }
        // Only take exists; rollback closes the concrete reachable set at
        // [] and [1]. This does not turn depth-one CLI results into proofs.
    }

    macro_rules! induction_cell {
        ($module:ident, $name:literal, $source:ident) => {
            mod $module {
                use super::*;

                const FIXTURE: Fixture = Fixture {
                    name: $name,
                    source: $source,
                    depth: "1",
                };

                #[test]
                fn check() {
                    FIXTURE.check();
                }
                #[test]
                fn default_bmc() {
                    induction_verify(&FIXTURE, "bmc", None);
                }
                #[test]
                fn default_induction() {
                    induction_verify(&FIXTURE, "induction", None);
                }
                #[test]
                fn default_explicit() {
                    induction_verify(&FIXTURE, "explicit", None);
                }
                #[test]
                fn monitor() {
                    induction_monitor(&FIXTURE);
                }
            }
        };
    }

    // ind1 and ind3 are separate detectors for the two induction SAT arms.
    induction_cell!(is1, "IS1", IS1);
    induction_cell!(is2, "IS2", IS2);
    induction_cell!(is3, "IS3", IS3);

    macro_rules! selected_induction_cell {
        ($module:ident, $name:literal, $source:ident, $property:literal) => {
            mod $module {
                use super::*;

                const FIXTURE: Fixture = Fixture {
                    name: $name,
                    source: $source,
                    depth: "1",
                };

                #[test]
                fn bmc() {
                    induction_verify(&FIXTURE, "bmc", Some($property));
                }
                #[test]
                fn induction() {
                    induction_verify(&FIXTURE, "induction", Some($property));
                }
                #[test]
                fn explicit() {
                    induction_verify(&FIXTURE, "explicit", Some($property));
                }
            }
        };
    }

    selected_induction_cell!(is1_selected, "IS1", IS1, "P");
    selected_induction_cell!(is2_selected, "IS2", IS2, "T");

    mod fr1 {
        use super::*;

        const FIXTURE: Fixture = Fixture {
            name: "FR1",
            source: FR1,
            depth: "4",
        };

        #[test]
        fn check() {
            FIXTURE.check();
        }

        fn verify(engine: &str, property: &str) {
            let (value, exit) = FIXTURE.verify(engine, Some(property));
            if engine == "induction" && property == "R" {
                assert_error(
                    &value,
                    exit,
                    "usage",
                    "which the induction engine cannot prove",
                );
            } else {
                // Preservation control: all frames fit N. Rejecting every
                // selected Seq query would fail these cases, not just IS1/2.
                assert_success(
                    &value,
                    exit,
                    if engine == "bmc" {
                        "verified"
                    } else {
                        "proved"
                    },
                );
                if property == "P" {
                    // Induction publishes its implicit bounds alongside P;
                    // selected BMC/explicit publish only the selected P.
                    assert_eq!(
                        value["invariants_checked"],
                        if engine == "induction" {
                            json!(["_bounds_xs", "P"])
                        } else {
                            json!(["P"])
                        },
                        "{value}"
                    );
                    if engine == "induction" {
                        assert_eq!(value["k_used"]["P"], 1, "{value}");
                    }
                } else {
                    assert_eq!(value["reachables"]["R"]["witnessed_at_step"], 1, "{value}");
                    let witness = &value["reachables"]["R"]["witness"];
                    assert_eq!(witness[0]["state"], json!({"xs": []}), "{value}");
                    assert_eq!(witness[1]["state"], json!({"xs": [1]}), "{value}");
                    assert_eq!(witness[1]["action"]["name"], "take", "{value}");
                }
            }
        }

        #[test]
        fn bmc_p() {
            verify("bmc", "P");
        }
        #[test]
        fn bmc_r() {
            verify("bmc", "R");
        }
        #[test]
        fn induction_p() {
            verify("induction", "P");
        }
        #[test]
        fn induction_r() {
            verify("induction", "R");
        }
        #[test]
        fn explicit_p() {
            verify("explicit", "P");
        }
        #[test]
        fn explicit_r() {
            verify("explicit", "R");
        }

        #[test]
        fn monitor() {
            let mut monitor = FIXTURE.monitor();
            assert_eq!(monitor.model.actions.len(), 2);
            for step in 0..=4 {
                assert_eq!(
                    monitor.state,
                    if step % 2 == 0 {
                        seq_state(&[])
                    } else {
                        seq_state(&[1])
                    }
                );
                assert_true_properties(&monitor, &monitor.state);
                assert_eq!(monitor.model.reachables.len(), 1);
                let reachable = &monitor.model.reachables[0];
                assert_eq!(reachable.name, "R");
                assert_eq!(
                    fsl_runtime::eval(
                        &reachable.expr,
                        &monitor.state,
                        &mut BTreeMap::new(),
                        &monitor.model,
                        None
                    ),
                    Ok(FslValue::Bool(step % 2 == 1))
                );
                if step == 4 {
                    break;
                }
                let action = if step % 2 == 0 { "take" } else { "drop" };
                let result = monitor
                    .attempt(action, &BTreeMap::new())
                    .expect("guarded cycle");
                eprintln!("FR1 Monitor step {}: {result:?}", step + 1);
                assert_eq!(
                    result,
                    fsl_runtime::StepResult {
                        action: action.to_owned(),
                        params: BTreeMap::new(),
                        state: if step % 2 == 0 {
                            seq_state(&[1])
                        } else {
                            seq_state(&[])
                        },
                        attempted_state: None,
                        violation: None
                    }
                );
            }
            // take/drop are the entire enabled closure [] <-> [1].
        }
    }

    macro_rules! conservative_cell {
        ($module:ident, $name:literal, $source:ident, $depth:literal, $property:literal) => {
            mod $module {
                use super::*;

                const FIXTURE: Fixture = Fixture {
                    name: $name,
                    source: $source,
                    depth: $depth,
                };

                #[test]
                fn check() {
                    FIXTURE.check();
                }
                #[test]
                fn selected_bmc() {
                    let (value, exit) = FIXTURE.verify("bmc", Some($property));
                    assert_capacity_error(&value, exit);
                }
            }
        };
    }

    // F deliberately rejects more than base: Q reads only c, not xs. Still,
    // take reaches a frame beyond capacity and F conservatively fails closed;
    // this is a policy control, not a claim that Q is concretely false.
    conservative_cell!(fr2, "FR2", FR2, "2", "Q");
    // R1 is already reached at step 1 ([] -> [1]); step 3 reaches [1,1,1]
    // beyond N=2. F conservatively rejects even that already-witnessed goal,
    // rather than claiming the same acceptance set as base.
    conservative_cell!(fr3, "FR3", FR3, "3", "R1");
}

// Ranked projection controls are independent of the existing 199 tests.
mod ranked_frames {
    use super::*;

    const CANDIDATE: &str = r"spec RankedMissingSuffix {
  type K = 0..1
  state { xs: Seq<K, 1>, go: Bool }
  init { xs = Seq {} go = false }
  action take() { requires go xs = xs.push(1) go = false }
  action idle() { requires not go }
  leadsTo L {
    go and xs.size() == 1 ~> xs.size() == 2 and xs.at(1) == 1
    decreases 0
  }
}
";

    const RL2: &str = r"spec RL2 {
  type K = 0..1
  state { xs: Seq<K, 1>, go: Bool }
  init { xs = Seq { 0 }  go = false }
  action take() { requires go  go = false  xs = xs.push(1) }
  action idle() { requires not go }
  leadsTo L {
    go and xs.size() == 1 ~> xs.size() == 2 and xs.at(1) == 1
    decreases 0
  }
}
";

    const RL3: &str = r"spec RL3 {
  type K = 0..1
  state { xs: Seq<K, 1>, go: Bool }
  init { xs = Seq { 0 }  go = false }
  action take() { requires go  go = false  xs = xs.push(1) }
  action idle() { requires not go }
  leadsTo L {
    xs.size() == 1 ~> xs.size() == 2 and xs.at(1) == 1
    decreases 0
  }
}
";

    const WITHIN_CAPACITY: &str = r"spec RankWithinCapacity {
  type K = 0..1
  state { xs: Seq<K, 1>, go: Bool }
  init { xs = Seq { 0 } go = false }
  action wait() { requires go }
  action idle() { requires not go }
  leadsTo L {
    go and xs == Seq { 0 } ~> xs.size() == 0
    decreases 0
  }
}
";

    struct Fixture {
        name: &'static str,
        source: &'static str,
        depth: &'static str,
        k: Option<&'static str>,
    }

    impl Fixture {
        fn run(&self, args: &[&str]) -> (Value, i32) {
            let cell = Cell {
                name: self.name,
                capacity: 1,
                initial: &[],
                expression: "true",
                property: Property::InvariantAndReachable,
            };
            let root = std::env::temp_dir().join(format!(
                "fslc-1519-{}-{}-{}",
                self.name,
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).expect("create ranked test directory");
            std::fs::write(root.join("model.fsl"), self.source).expect("write ranked model");
            let model = Model(root);
            model.run(&cell, args)
        }

        fn check(&self) {
            let (value, exit) = self.run(&["check", "model.fsl"]);
            assert_eq!(
                (exit, &value["result"], &value["message"]),
                (0, &json!("ok"), &Value::Null),
                "{value}"
            );
        }

        fn verify(&self, engine: &str) {
            let mut args = vec![
                "verify",
                "model.fsl",
                "--engine",
                engine,
                "--depth",
                self.depth,
                "--no-cache",
            ];
            // --k is induction-only; BMC/explicit use the identical spec/depth.
            if engine == "induction"
                && let Some(k) = self.k
            {
                args.extend(["--k", k]);
            }
            let (value, exit) = self.run(&args);
            if engine == "explicit" {
                assert_error(
                    &value,
                    exit,
                    "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property",
                );
            } else if self.name == "RK3" {
                assert_idle_lasso(&value, exit);
            } else if engine == "bmc" {
                // The reachable closure is just the initial state's idle loop.
                // go is false forever, so L has no pending trigger in RK1/2/4/5.
                assert_eq!(
                    (
                        exit,
                        &value["result"],
                        &value["invariant"],
                        &value["violation_kind"],
                        &value["message"],
                        &value["cti"],
                    ),
                    (
                        0,
                        &json!("verified"),
                        &Value::Null,
                        &Value::Null,
                        &Value::Null,
                        &Value::Null,
                    ),
                    "{value}"
                );
            } else {
                match self.name {
                    // Detectors for round-3 MAJOR-1: take appends 1, so on the
                    // reconstructed [x,1] the goal is TRUE. A rank failure read
                    // from the missing slot is not evidence. Fail closed, do
                    // not add successor bounds to hide the offending model.
                    "RK1" | "RK2" => {
                        assert_error(&value, exit, "model sequence length exceeds capacity");
                    }
                    "RK4" => assert_bounds_cti(&value, exit),
                    "RK5" => assert_rank_cti(&value, exit),
                    _ => unreachable!("all ranked fixtures classified"),
                }
            }
        }
    }

    fn assert_error(value: &Value, exit: i32, message: &str) {
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["kind"],
                &value["cti"],
                &value["violation_kind"],
            ),
            (
                2,
                &json!("error"),
                &json!("semantics"),
                &Value::Null,
                &Value::Null,
            ),
            "{value}"
        );
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|produced| produced.contains(message)),
            "{value}"
        );
    }

    fn assert_idle_lasso(value: &Value, exit: i32) {
        // Detector for the default BMC-fallback route: initially P is true,
        // Q is false, and only idle is enabled. The one-edge self-loop is a
        // genuine counterexample, not an arbitrary-state rank CTI. Reject its
        // downgrade to unknown_cti even though both processes would exit 1.
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["violation_kind"],
                &value["invariant"],
                &value["trace_type"],
            ),
            (
                1,
                &json!("violated"),
                &json!("leadsTo"),
                &json!("L"),
                &json!("leadsTo"),
            ),
            "{value}"
        );
        assert_eq!(value["bindings"], json!({}), "{value}");
        assert_eq!(value["pending_since"], 0, "{value}");
        assert_eq!(value["loop_start"], 0, "{value}");
        assert_eq!(value["stutter"], false, "{value}");
        assert_eq!(value["cti"], Value::Null, "{value}");
        assert_eq!(value["rank_failure"], Value::Null, "{value}");
        assert_eq!(value["message"], Value::Null, "{value}");
        let trace = value["trace"].as_array().expect("idle lasso trace");
        assert_eq!(trace.len(), 2, "{value}");
        for (step, state) in trace.iter().enumerate() {
            assert_eq!(state["step"], step, "{value}");
            assert_eq!(state["state"], json!({"go": false, "xs": [0]}), "{value}");
        }
        assert_eq!(trace[0]["action"], Value::Null, "{value}");
        assert_eq!(trace[1]["action"]["name"], "idle", "{value}");
        assert_eq!(trace[1]["action"]["params"], json!({}), "{value}");
    }

    fn assert_bounds_cti(value: &Value, exit: i32) {
        // Preservation, NOT a rank-missing-slot detector: k=1 may start full
        // with go=true. take really violates N=1, regardless of suffix values.
        assert_eq!(
            (exit, &value["result"], &value["invariant"], &value["k"]),
            (1, &json!("unknown_cti"), &json!("_bounds_xs"), &json!(1)),
            "{value}"
        );
        assert_eq!(value["trace_type"], "induction_cti", "{value}");
        assert_eq!(value["violation_kind"], Value::Null, "{value}");
        assert_eq!(value["rank_failure"], Value::Null, "{value}");
        assert_eq!(value["message"], Value::Null, "{value}");
        assert_eq!(value["cti"]["violated_at"], 1, "{value}");
        let trace = value["cti"]["states"].as_array().expect("bounds CTI");
        assert_eq!(trace.len(), 2, "{value}");
        let before = trace[0]["state"]["xs"].as_array().expect("full pre xs");
        assert_eq!(before.len(), 1, "{value}");
        assert!(before[0] == 0 || before[0] == 1, "{value}");
        assert_eq!(trace[0]["state"]["go"], true, "{value}");
        assert_eq!(
            trace[1]["state"],
            json!({"go": false, "xs": [before[0], 1]}),
            "{value}"
        );
        assert_eq!(trace[1]["action"]["name"], "take", "{value}");
        assert_eq!(trace[1]["action"]["params"], json!({}), "{value}");
    }

    fn assert_rank_cti(value: &Value, exit: i32) {
        // Preservation against blanket rejection of Seq rank failures: wait
        // stutters at go=true,xs=[0]. Both states fit N, P stays true, Q stays
        // false, and measure 0 does NOT strictly decrease. This CTI is real
        // (though unreachable from init), unlike RK1/2's reconstructed goal.
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["violation_kind"],
                &value["invariant"]
            ),
            (
                1,
                &json!("unknown_cti"),
                &json!("leadsTo_rank"),
                &json!("L")
            ),
            "{value}"
        );
        assert_eq!(value["trace_type"], "induction_cti", "{value}");
        assert_eq!(value["rank_failure"], "non_decreasing_action", "{value}");
        assert_eq!(value["measure"], "0", "{value}");
        assert_eq!(value["measure_before"], 0, "{value}");
        assert_eq!(value["measure_after"], 0, "{value}");
        assert_eq!(value["bindings"], json!({}), "{value}");
        assert_eq!(value["last_action"]["name"], "wait", "{value}");
        assert_eq!(value["last_action"]["params"], json!({}), "{value}");
        assert_eq!(value["cti"]["violated_at"], 1, "{value}");
        let trace = value["cti"]["states"].as_array().expect("rank CTI");
        assert_eq!(trace.len(), 2, "{value}");
        for (step, state) in trace.iter().enumerate() {
            assert_eq!(state["step"], step, "{value}");
            assert_eq!(state["state"], json!({"go": true, "xs": [0]}), "{value}");
        }
        assert_eq!(trace[0]["action"], Value::Null, "{value}");
        assert_eq!(trace[1]["action"]["name"], "wait", "{value}");
        assert_eq!(trace[1]["action"]["params"], json!({}), "{value}");
    }

    macro_rules! ranked_cell {
        ($module:ident, $name:literal, $source:ident, $depth:literal, $k:expr) => {
            mod $module {
                use super::*;

                const FIXTURE: Fixture = Fixture {
                    name: $name,
                    source: $source,
                    depth: $depth,
                    k: $k,
                };

                #[test]
                fn check() {
                    FIXTURE.check();
                }
                #[test]
                fn bmc() {
                    FIXTURE.verify("bmc");
                }
                #[test]
                fn induction() {
                    FIXTURE.verify("induction");
                }
                #[test]
                fn explicit() {
                    FIXTURE.verify("explicit");
                }
            }
        };
    }

    ranked_cell!(rk1, "RK1", CANDIDATE, "1", Some("2"));
    ranked_cell!(rk2, "RK2", RL2, "2", Some("2"));
    ranked_cell!(rk3, "RK3", RL3, "2", None);
    ranked_cell!(rk4, "RK4", CANDIDATE, "1", Some("1"));
    ranked_cell!(rk5, "RK5", WITHIN_CAPACITY, "2", None);

    // Helpful-branch controls supplement, rather than change, RK1-RK5.
    // Expectations follow the transitions and fairness, not a captured verdict.
    mod helpful_frames {
        use super::*;

        const MISSING_STICKY: &str = r"spec SiteMissingSticky {
  type K = 0..1
  state { xs: Seq<K, 1>, go: Bool }
  init { xs = Seq { 0 } go = false }
  fair action help0() { requires go }
  fair action help1() { requires go }
  action take() { requires go go = false xs = xs.push(1) }
  action idle() { requires not go }
  leadsTo L {
    xs.size() == 1 ~> xs.size() == 2 and xs.at(1) == 1
    helpful help0()
    helpful help1()
    decreases 0
  }
}
";

        const MISSING_HELPFUL: &str = r"spec SiteMissingHelpful {
  type K = 0..1
  state { xs: Seq<K, 1>, go: Bool }
  init { xs = Seq { 0 } go = false }
  fair action take() { requires go go = false xs = xs.push(1) }
  action idle() { requires not go }
  leadsTo L {
    go and xs.size() == 1 ~> xs.size() == 2 and xs.at(1) == 1
    helpful take()
    decreases 0
  }
}
";

        const MISSING_NONHELPFUL: &str = r"spec SiteMissingNonhelpful {
  type K = 0..1
  state { xs: Seq<K, 1>, go: Bool, done: Bool }
  init { xs = Seq { 0 } go = false done = false }
  fair action work() { requires go done = true }
  action take() { requires go requires not done go = false xs = xs.push(1) }
  action idle() { requires not go }
  leadsTo L {
    go and xs.size() == 1 ~> done or (xs.size() == 2 and xs.at(1) == 1)
    helpful work()
    decreases 0
  }
}
";

        const STICKY: &str = r"spec SiteSticky {
  state { phase: 0..1, done: Bool }
  init { phase = 0 done = false }
  fair action help0() { requires phase == 0 requires not done done = true }
  fair action help1() { requires phase == 1 requires not done done = true }
  action rotate() { requires not done phase = 1 - phase }
  leadsTo L { not done ~> done helpful help0() helpful help1() decreases 0 }
}
";

        const HELPFUL_STALL: &str = r"spec SiteHelpfulStall {
  state { x: 0..1 }
  init { x = 1 }
  fair action work() { requires x > 0 x = x }
  action idle() { requires x == 0 }
  leadsTo L { x > 0 ~> x == 0 helpful work() decreases x }
}
";

        const PUMP: &str = r"spec SitePump {
  state { x: Int }
  init { x = 5 }
  fair action work() { requires x > 0 x = x - 1 }
  action pump() { requires x > 0 x = x + 2 }
  invariant NonNeg { x >= 0 }
  leadsTo L { x > 0 ~> x == 0 helpful work() decreases x }
}
";

        fn assert_verified(value: &Value, exit: i32) {
            assert_eq!(exit, 0, "{value}");
            assert_eq!(value["result"], "verified", "{value}");
            for key in [
                "invariant",
                "violation_kind",
                "cti",
                "rank_failure",
                "message",
            ] {
                assert_eq!(value[key], Value::Null, "{key}: {value}");
            }
        }

        fn assert_rank_failure(fixture: &Fixture, value: &Value, exit: i32) {
            let (kind, action) = match fixture.name {
                "RK9" => ("helpful_action_enabledness_not_sticky", "rotate"),
                "RK10" => ("non_decreasing_helpful_action", "work"),
                "RK11" => ("non_helpful_action_increases_measure", "pump"),
                _ => unreachable!("only within-capacity controls have rank CTIs"),
            };
            assert_eq!(exit, 1, "{value}");
            assert_eq!(value["result"], "unknown_cti", "{value}");
            assert_eq!(value["violation_kind"], "leadsTo_rank", "{value}");
            assert_eq!(value["invariant"], "L", "{value}");
            assert_eq!(value["trace_type"], "induction_cti", "{value}");
            assert_eq!(value["rank_failure"], kind, "{value}");
            assert_eq!(value["bindings"], json!({}), "{value}");
            assert_eq!(value["last_action"]["name"], action, "{value}");
            assert_eq!(value["last_action"]["params"], json!({}), "{value}");
            assert_eq!(value["cti"]["violated_at"], 1, "{value}");
            let trace = value["cti"]["states"].as_array().expect("helpful rank CTI");
            assert_eq!(trace.len(), 2, "{value}");
            assert_eq!(trace[0]["step"], 0, "{value}");
            assert_eq!(trace[1]["step"], 1, "{value}");
            assert_eq!(trace[0]["action"], Value::Null, "{value}");
            assert_eq!(trace[1]["action"]["name"], action, "{value}");
            assert_eq!(trace[1]["action"]["params"], json!({}), "{value}");
            if fixture.name == "RK9" {
                // rotate disables the previously enabled, unselected help.
                // Both states are legal, pending, and have Q=false.
                let phase = trace[0]["state"]["phase"].as_i64().expect("phase");
                assert!((0..=1).contains(&phase), "{value}");
                assert_eq!(trace[0]["state"], json!({"phase": phase, "done": false}));
                assert_eq!(
                    trace[1]["state"],
                    json!({"phase": 1 - phase, "done": false})
                );
                assert_eq!(value["measure"], "0", "{value}");
            } else {
                let before = trace[0]["state"]["x"].as_i64().expect("pre x");
                let after = trace[1]["state"]["x"].as_i64().expect("post x");
                assert!(
                    before > 0 && after > 0,
                    "P must stay true and Q false: {value}"
                );
                assert_eq!(trace[0]["state"], json!({"x": before}), "{value}");
                assert_eq!(trace[1]["state"], json!({"x": after}), "{value}");
                if fixture.name == "RK10" {
                    assert_eq!((before, after), (1, 1), "fair work really stalls: {value}");
                } else {
                    assert_eq!(
                        after,
                        before + 2,
                        "non-helpful pump really increases: {value}"
                    );
                }
                assert_eq!(value["measure"], "x", "{value}");
                assert_eq!(value["measure_before"], before, "{value}");
                assert_eq!(value["measure_after"], after, "{value}");
            }
        }

        fn assert_lasso(fixture: &Fixture, value: &Value, exit: i32) {
            assert_eq!(exit, 1, "{value}");
            assert_eq!(value["result"], "violated", "{value}");
            assert_eq!(value["violation_kind"], "leadsTo", "{value}");
            assert_eq!(value["invariant"], "L", "{value}");
            assert_eq!(value["trace_type"], "leadsTo", "{value}");
            assert_eq!(value["bindings"], json!({}), "{value}");
            assert_eq!(value["pending_since"], 0, "{value}");
            assert_eq!(value["stutter"], false, "{value}");
            for key in ["cti", "rank_failure", "message"] {
                assert_eq!(value[key], Value::Null, "{key}: {value}");
            }
            let trace = value["trace"].as_array().expect("real helpful lasso");
            let loop_start = usize::try_from(value["loop_start"].as_u64().expect("loop start"))
                .expect("loop start fits usize");
            assert!(trace.len() >= 2 && loop_start < trace.len() - 1, "{value}");
            assert_eq!(
                trace[loop_start]["state"],
                trace.last().unwrap()["state"],
                "{value}"
            );
            assert_eq!(trace[0]["action"], Value::Null, "{value}");
            let initial = match fixture.name {
                "RK9" => json!({"phase": 0, "done": false}),
                "RK10" => json!({"x": 1}),
                "RK11" => json!({"x": 5}),
                _ => unreachable!("within-capacity lasso control"),
            };
            assert_eq!(trace[0]["state"], initial, "{value}");
            let mut fair_work_in_loop = false;
            for (step, state) in trace.iter().enumerate() {
                assert_eq!(state["step"], step, "{value}");
                if fixture.name == "RK9" {
                    let phase = state["state"]["phase"].as_i64().expect("phase");
                    assert!((0..=1).contains(&phase), "{value}");
                    assert_eq!(state["state"], json!({"phase": phase, "done": false}));
                    if step > 0 {
                        assert_eq!(state["action"]["name"], "rotate", "{value}");
                        assert_eq!(
                            phase,
                            1 - trace[step - 1]["state"]["phase"].as_i64().unwrap()
                        );
                    }
                } else {
                    let x = state["state"]["x"].as_i64().expect("pending x");
                    assert!(x > 0, "P true and Q false throughout the lasso: {value}");
                    assert_eq!(state["state"], json!({"x": x}), "{value}");
                    if step > 0 {
                        let before = trace[step - 1]["state"]["x"].as_i64().unwrap();
                        match state["action"]["name"].as_str() {
                            Some("work") => {
                                assert_eq!(
                                    x,
                                    if fixture.name == "RK10" {
                                        before
                                    } else {
                                        before - 1
                                    }
                                );
                                fair_work_in_loop |= step > loop_start;
                            }
                            Some("pump") if fixture.name == "RK11" => assert_eq!(x, before + 2),
                            _ => panic!("not a real pending transition: {value}"),
                        }
                    }
                }
                if step > 0 {
                    assert_eq!(state["action"]["params"], json!({}), "{value}");
                }
            }
            if fixture.name == "RK9" {
                // The closed rotate cycle alternates phase: neither helpful
                // action stays continuously enabled, so weak fairness permits it.
                assert_ne!(trace[loop_start]["state"], trace[loop_start + 1]["state"]);
            } else {
                // work is continuously enabled and must occur in the loop.
                assert!(
                    fair_work_in_loop,
                    "the lasso must respect fair work: {value}"
                );
            }
        }

        fn verify(fixture: &Fixture, engine: &str) {
            let mut args = vec![
                "verify",
                "model.fsl",
                "--engine",
                engine,
                "--depth",
                fixture.depth,
                "--no-cache",
            ];
            if engine == "induction"
                && let Some(k) = fixture.k
            {
                args.extend(["--k", k]);
            }
            let (value, exit) = fixture.run(&args);
            if engine == "explicit" {
                assert_error(
                    &value,
                    exit,
                    "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property",
                );
            } else if fixture.name == "RK6" {
                // init has P=true,Q=false and only idle enabled. Helpful
                // instances are disabled forever, so this loop is fair. Even
                // when ranking rejects a missing suffix, retain the BMC lasso.
                assert_idle_lasso(&value, exit);
            } else if matches!(fixture.name, "RK7" | "RK8") {
                // go=false is closed under all reachable actions; P is never
                // triggered. In arbitrary go=true/full states, take appends 1
                // and really resolves Q (work in RK8 resolves it via done).
                // The missing-slot rank model is not a genuine failure CTI.
                if engine == "bmc" {
                    assert_verified(&value, exit);
                } else {
                    assert_error(&value, exit, "model sequence length exceeds capacity");
                }
            } else if engine == "bmc" {
                assert_lasso(fixture, &value, exit);
            } else {
                assert_rank_failure(fixture, &value, exit);
            }
        }

        macro_rules! helpful_cell {
            ($module:ident, $name:literal, $source:ident, $depth:literal, $k:expr) => {
                mod $module {
                    use super::*;

                    const FIXTURE: Fixture = Fixture {
                        name: $name,
                        source: $source,
                        depth: $depth,
                        k: $k,
                    };

                    #[test]
                    fn check() {
                        FIXTURE.check();
                    }
                    #[test]
                    fn bmc() {
                        verify(&FIXTURE, "bmc");
                    }
                    #[test]
                    fn induction() {
                        verify(&FIXTURE, "induction");
                    }
                    #[test]
                    fn explicit() {
                        verify(&FIXTURE, "explicit");
                    }
                }
            };
        }

        helpful_cell!(rk6, "RK6", MISSING_STICKY, "1", None);
        helpful_cell!(rk7, "RK7", MISSING_HELPFUL, "2", Some("2"));
        helpful_cell!(rk8, "RK8", MISSING_NONHELPFUL, "2", Some("2"));
        helpful_cell!(rk9, "RK9", STICKY, "2", None);
        helpful_cell!(rk10, "RK10", HELPFUL_STALL, "2", None);
        helpful_cell!(rk11, "RK11", PUMP, "3", None);
    }
}

struct Model(PathBuf);

impl Model {
    fn new(cell: &Cell) -> Self {
        let root = std::env::temp_dir().join(format!(
            "fslc-1519-{}-{}-{}",
            cell.name,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create test directory");
        std::fs::write(root.join("model.fsl"), cell.source()).expect("write test model");
        Self(root)
    }

    fn run(&self, cell: &Cell, args: &[&str]) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .output()
            .expect("run native fslc");
        let status = output.status.code().expect("native exit code");
        assert!(
            output.stderr.is_empty(),
            "{} {args:?}: exit={status}, stdout={}, stderr={}",
            cell.name,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{} {args:?}: exit={status}, invalid JSON {error}: {}",
                cell.name,
                String::from_utf8_lossy(&output.stdout)
            )
        });
        // Preserve the entire raw observation for base/fault calibration. The
        // assertions test semantic outcomes, not equality of engine-specific
        // proof/cost/coverage envelopes or a production-derived golden.
        let observation = format!("{} {args:?}: exit={status} {value}", cell.name);
        eprintln!("{observation}");
        (value, status)
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove test directory");
    }
}

fn check(cell: &Cell) {
    let model = Model::new(cell);
    let (value, exit) = model.run(cell, &["check", "model.fsl"]);
    assert_eq!(exit, 0, "{} check: {value}", cell.name);
    assert_eq!(value["result"], "ok", "{} check: {value}", cell.name);
    assert_eq!(
        value["message"],
        Value::Null,
        "{} check: {value}",
        cell.name
    );
}

fn verify(cell: &Cell, engine: &str, successful_result: &str) {
    let model = Model::new(cell);
    let (value, exit) = model.run(
        cell,
        &[
            "verify",
            "model.fsl",
            "--engine",
            engine,
            "--depth",
            "4",
            "--no-cache",
        ],
    );
    let label = format!("{} {engine}: exit={exit} {value}", cell.name);
    let expected_result = match cell.property {
        Property::InvariantAndReachable => successful_result,
        Property::Unreachable => "reachable_failed",
        Property::StateOverflow | Property::ActionRead => "violated",
    };
    // One tuple pins all verdict/name/error observables even when base returns
    // an error instead of a verdict (its diagnostic is visible in the failure).
    assert_eq!(
        (
            exit,
            &value["result"],
            &value["violation_kind"],
            &value["invariant"],
            &value["last_action"]["name"],
            &value["message"],
        ),
        (
            i32::from(cell.property != Property::InvariantAndReachable),
            &json!(expected_result),
            &match cell.property {
                Property::StateOverflow => json!("type_bound"),
                Property::ActionRead => json!("invariant"),
                _ => Value::Null,
            },
            &match cell.property {
                Property::StateOverflow => json!("_bounds_xs"),
                Property::ActionRead => json!(cell.name),
                _ => Value::Null,
            },
            &if matches!(
                cell.property,
                Property::StateOverflow | Property::ActionRead
            ) {
                json!(cell.action())
            } else {
                Value::Null
            },
            &Value::Null,
        ),
        "{label}"
    );
    match cell.property {
        Property::InvariantAndReachable => {
            assert_eq!(
                value["invariants_checked"],
                if cell.induction_shape().is_some() {
                    json!(["_bounds_xs", "Shape", cell.name])
                } else {
                    json!(["_bounds_xs", cell.name])
                },
                "{label}"
            );
            let reached_name = format!("{}Reached", cell.name);
            assert_eq!(
                value["reachables"]
                    .as_object()
                    .expect("reachable map")
                    .len(),
                1,
                "{label}"
            );
            assert_eq!(
                value["reachables"][&reached_name]["witnessed_at_step"], 0,
                "{label}"
            );
            assert_eq!(
                value["reachables"][&reached_name]["witness"],
                json!([{"step": 0, "state": {"xs": cell.initial}}]),
                "{label}"
            );
            assert_eq!(value["action_coverage"]["idle"], true, "{label}");
        }
        Property::Unreachable => {
            let unreached = value["unreached"].as_array().expect("unreached goals");
            assert_eq!(unreached.len(), 1, "{label}");
            assert_eq!(unreached[0]["name"], cell.name, "{label}");
        }
        Property::StateOverflow | Property::ActionRead => {
            assert_eq!(value["violated_at_step"], 1, "{label}");
            assert_eq!(value["last_action"]["params"], json!({}), "{label}");
            assert_failure_trace(cell, &value, &label);
        }
    }
}

fn assert_failure_trace(cell: &Cell, value: &Value, label: &str) {
    let trace = value["trace"].as_array().expect("replayable failure trace");
    assert_eq!(trace.len(), 2, "{label}");
    let mut initial = json!({"xs": cell.initial});
    let attempted = if cell.property == Property::ActionRead {
        initial["x"] = json!(0);
        json!({"xs": cell.initial, "x": 1})
    } else {
        json!({"xs": [0, 1]})
    };
    assert_eq!(trace[0]["state"], initial, "{label}");
    assert_eq!(trace[1]["state"], attempted, "{label}");
    assert_eq!(trace[1]["action"]["name"], cell.action(), "{label}");
    assert_eq!(trace[1]["action"]["params"], json!({}), "{label}");
}

fn assert_monitor_properties(cell: &Cell, monitor: &fsl_runtime::Monitor, step: usize) {
    // Monitor itself checks invariants after an action, not at creation.
    // Explicit evaluation also covers step zero and reachable predicates.
    for (index, property) in monitor.model.invariants.iter().enumerate() {
        let name = if cell.induction_shape().is_some() && index == 0 {
            "Shape"
        } else {
            cell.name
        };
        assert_eq!(property.name, name, "{} Monitor invariant", cell.name);
        assert_eq!(
            fsl_runtime::eval(
                &property.expr,
                &monitor.state,
                &mut BTreeMap::new(),
                &monitor.model,
                None,
            ),
            Ok(FslValue::Bool(true)),
            "{} Monitor invariant step {step}",
            cell.name
        );
    }
    for property in &monitor.model.reachables {
        let expected_name = if cell.property == Property::Unreachable {
            cell.name.to_owned()
        } else {
            format!("{}Reached", cell.name)
        };
        assert_eq!(
            property.name, expected_name,
            "{} Monitor reachable",
            cell.name
        );
        assert_eq!(
            fsl_runtime::eval(
                &property.expr,
                &monitor.state,
                &mut BTreeMap::new(),
                &monitor.model,
                None,
            ),
            Ok(FslValue::Bool(cell.property != Property::Unreachable)),
            "{} Monitor reachable step {step}",
            cell.name
        );
    }
}

fn monitor(cell: &Cell) {
    let source = cell.source();
    let kernel = parse_kernel_source(&source, &FsResolver::new(".")).expect("Monitor parse");
    let model = build_model(kernel).expect("Monitor model");
    let mut monitor = fsl_runtime::Monitor::new(model).expect("Monitor init");
    assert_eq!(
        monitor.model.invariants.len(),
        usize::from(cell.property != Property::Unreachable)
            + usize::from(cell.induction_shape().is_some()),
        "{} Monitor invariant count",
        cell.name
    );
    assert_eq!(
        monitor.model.reachables.len(),
        usize::from(matches!(
            cell.property,
            Property::InvariantAndReachable | Property::Unreachable
        )),
        "{} Monitor reachable count",
        cell.name
    );
    let initial = cell.state();
    assert_eq!(monitor.state, initial, "{} Monitor initial", cell.name);
    for step in 0..=4 {
        assert_monitor_properties(cell, &monitor, step);
        if step == 4 {
            break;
        }
        let result = monitor
            .attempt(cell.action(), &BTreeMap::new())
            .unwrap_or_else(|error| panic!("{} Monitor step {step}: {error:?}", cell.name));
        eprintln!("{} Monitor step {}: {result:?}", cell.name, step + 1);
        let expected = if cell.property == Property::ActionRead {
            let mut attempted = initial.clone();
            attempted.insert("x".to_owned(), FslValue::Int(1));
            fsl_runtime::StepResult {
                action: "take".to_owned(),
                params: BTreeMap::new(),
                state: initial.clone(),
                attempted_state: Some(attempted),
                violation: Some(fsl_runtime::Violation {
                    kind: "invariant".to_owned(),
                    name: cell.name.to_owned(),
                    step: step + 1,
                }),
            }
        } else if cell.property == Property::StateOverflow {
            fsl_runtime::StepResult {
                action: "overflow".to_owned(),
                params: BTreeMap::new(),
                state: initial.clone(),
                attempted_state: Some(BTreeMap::from([(
                    "xs".to_owned(),
                    FslValue::Seq(vec![FslValue::Int(0), FslValue::Int(1)]),
                )])),
                violation: Some(fsl_runtime::Violation {
                    kind: "type_bound".to_owned(),
                    name: "_bounds_xs".to_owned(),
                    step: step + 1,
                }),
            }
        } else {
            fsl_runtime::StepResult {
                action: "idle".to_owned(),
                params: BTreeMap::new(),
                state: initial.clone(),
                attempted_state: None,
                violation: None,
            }
        };
        assert_eq!(result, expected, "{} Monitor step {}", cell.name, step + 1);
        assert_eq!(
            monitor.state, initial,
            "{} Monitor rollback/idle",
            cell.name
        );
    }
    // These models have exactly one action, which either stutters or rolls
    // back. The inspected initial state is therefore the entire reachable set;
    // false reachable observations are not just an unexhausted sampled trace.
    assert_eq!(
        monitor.model.actions.len(),
        1,
        "{} Monitor closure",
        cell.name
    );
}

macro_rules! cell {
    ($module:ident, $name:literal, $capacity:literal, $initial:expr, $expression:literal, $property:ident) => {
        mod $module {
            use super::*;

            const CELL: Cell = Cell {
                name: $name,
                capacity: $capacity,
                initial: $initial,
                expression: $expression,
                property: Property::$property,
            };

            #[test]
            fn check() {
                super::check(&CELL);
            }

            #[test]
            fn bmc() {
                verify(&CELL, "bmc", "verified");
            }

            #[test]
            fn induction() {
                verify(&CELL, "induction", "proved");
            }

            #[test]
            fn explicit() {
                verify(&CELL, "explicit", "proved");
            }

            #[test]
            fn monitor() {
                super::monitor(&CELL);
            }
        }
    };
}

// Full capacity-one temporary is [0, 1], independently of its declared bound.
cell!(
    h1,
    "H1",
    1,
    &[0],
    "xs.push(1).head() == 0",
    InvariantAndReachable
);
cell!(
    h2,
    "H2",
    1,
    &[0],
    "xs.push(1).at(1) == 1",
    InvariantAndReachable
);
cell!(
    h3,
    "H3",
    1,
    &[0],
    "xs.push(1)[1] == 1",
    InvariantAndReachable
);
cell!(
    h4,
    "H4",
    1,
    &[0],
    "xs.push(1).contains(1)",
    InvariantAndReachable
);
cell!(
    h5,
    "H5",
    1,
    &[0],
    "xs.push(1).size() == 2",
    InvariantAndReachable
);
cell!(
    h6,
    "H6",
    1,
    &[0],
    "xs.push(1) == Seq { 0, 1 }",
    InvariantAndReachable
);

// Full capacity-zero temporary is [1], not an empty/undefined sequence.
cell!(
    z1,
    "Z1",
    0,
    &[],
    "xs.push(1).head() == 1",
    InvariantAndReachable
);
cell!(
    z2,
    "Z2",
    0,
    &[],
    "xs.push(1).at(0) == 1",
    InvariantAndReachable
);
cell!(
    z3,
    "Z3",
    0,
    &[],
    "xs.push(1)[0] == 1",
    InvariantAndReachable
);
cell!(
    z4,
    "Z4",
    0,
    &[],
    "xs.push(1).contains(1)",
    InvariantAndReachable
);
cell!(
    z5,
    "Z5",
    0,
    &[],
    "xs.push(1).size() == 1",
    InvariantAndReachable
);

// Rejecting polarity: these values are never zero, including after idle.
cell!(r1, "R1", 1, &[0], "xs.push(1).at(1) == 0", Unreachable);
cell!(r2, "R2", 0, &[], "xs.push(1).head() == 0", Unreachable);

// pop drops the first element: [0,1].pop() == [1]. Repeated push grows twice.
cell!(
    f1,
    "F1",
    1,
    &[0],
    "xs.push(1).pop().head() == 1",
    InvariantAndReachable
);
cell!(
    f2,
    "F2",
    1,
    &[0],
    "xs.push(1).push(0).at(2) == 0",
    InvariantAndReachable
);

// Preservation controls: overflow is rejected on assignment, not on creation;
// an ordinary within-capacity push remains defined and retains its value.
cell!(s1, "S1", 1, &[0], "true", StateOverflow);
cell!(
    s2,
    "S2",
    1,
    &[],
    "xs.push(1).head() == 1",
    InvariantAndReachable
);

// Action-context reads must assign 1 and produce a replayable counterexample
// to x == 0, not exit 3 with an internally inconsistent symbolic trace.
cell!(ac1, "AC1", 0, &[], "xs.push(1).head()", ActionRead);
cell!(ac2, "AC2", 1, &[0], "xs.push(1).at(1)", ActionRead);
cell!(
    eq1,
    "EQ1",
    0,
    &[],
    "xs.push(1) != xs.push(0)",
    InvariantAndReachable
);

// Independently frozen sibling controls. Keep the original 100 tests and their
// helpers unchanged: these cases also need a Boolean state and a reachable-only
// context, and B1's attempted overflow is [1], not S1's [0, 1].
mod siblings {
    use super::*;

    struct Sibling {
        base: Cell,
        condition: Option<bool>,
        reachable_only: bool,
    }

    impl Sibling {
        fn source(&self) -> String {
            let cell = &self.base;
            let initial = cell
                .initial
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let (extra_state, extra_init) = if let Some(condition) = self.condition {
                (", c: Bool".to_owned(), format!(" c = {condition}"))
            } else if cell.property == Property::ActionRead {
                (", x: K".to_owned(), " x = 0".to_owned())
            } else {
                (String::new(), String::new())
            };
            let properties = match cell.property {
                Property::InvariantAndReachable => {
                    let invariant = if self.reachable_only {
                        String::new()
                    } else {
                        format!("invariant {} {{ {} }}", cell.name, cell.expression)
                    };
                    format!(
                        "{invariant}\n reachable {}Reached {{ {} }}",
                        cell.name, cell.expression
                    )
                }
                Property::StateOverflow => format!("invariant {} {{ true }}", cell.name),
                Property::ActionRead => format!("invariant {} {{ x == 0 }}", cell.name),
                Property::Unreachable => unreachable!("no sibling uses a negative reachable"),
            };
            let properties = if let Some(shape) = cell.induction_shape() {
                format!("invariant Shape {{ {shape} }}\n {properties}")
            } else {
                properties
            };
            let action = match cell.property {
                Property::StateOverflow => "action overflow() { xs = xs.push(1) }".to_owned(),
                Property::ActionRead => format!("action take() {{ x = {} }}", cell.expression),
                _ => "action idle() { }".to_owned(),
            };
            format!(
                "spec SeqSibling {{\n type K = 0..1\n state {{ xs: Seq<K, {}>{extra_state} }}\n init {{ xs = Seq {{ {initial} }}{extra_init} }}\n {action}\n {properties}\n}}\n",
                cell.capacity
            )
        }

        fn state(&self) -> BTreeMap<String, FslValue> {
            let mut state = self.base.state();
            if let Some(condition) = self.condition {
                state.insert("c".to_owned(), FslValue::Bool(condition));
            }
            state
        }

        fn attempted(&self) -> BTreeMap<String, FslValue> {
            let mut attempted = self.state();
            match self.base.property {
                Property::StateOverflow => {
                    // B1 is capacity zero: appending 1 yields exactly [1].
                    attempted.insert("xs".to_owned(), FslValue::Seq(vec![FslValue::Int(1)]));
                }
                Property::ActionRead => {
                    // Literal [0,1] equals the pushed temporary, so x becomes 1.
                    attempted.insert("x".to_owned(), FslValue::Int(1));
                }
                _ => unreachable!("only rejecting actions have attempted states"),
            }
            attempted
        }

        fn model(&self) -> Model {
            let model = Model::new(&self.base);
            std::fs::write(model.0.join("model.fsl"), self.source()).expect("write sibling model");
            model
        }
    }

    fn check(sibling: &Sibling) {
        let model = sibling.model();
        let cell = &sibling.base;
        let (value, exit) = model.run(cell, &["check", "model.fsl"]);
        assert_eq!(
            (exit, &value["result"], &value["message"]),
            (0, &json!("ok"), &Value::Null),
            "{} check: {value}",
            cell.name
        );
    }

    fn verify(sibling: &Sibling, engine: &str, success: &str) {
        let model = sibling.model();
        let cell = &sibling.base;
        let (value, exit) = model.run(
            cell,
            &[
                "verify",
                "model.fsl",
                "--engine",
                engine,
                "--depth",
                "4",
                "--no-cache",
            ],
        );
        let label = format!("{} {engine}: exit={exit} {value}", cell.name);
        let failure = match cell.property {
            Property::StateOverflow => Some(("type_bound", "_bounds_xs")),
            Property::ActionRead => Some(("invariant", cell.name)),
            _ => None,
        };
        let (expected_exit, result, kind, invariant, action) = if let Some((kind, name)) = failure {
            (
                1,
                "violated",
                json!(kind),
                json!(name),
                json!(cell.action()),
            )
        } else {
            (0, success, Value::Null, Value::Null, Value::Null)
        };
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["violation_kind"],
                &value["invariant"],
                &value["last_action"]["name"],
                &value["message"],
            ),
            (
                expected_exit,
                &json!(result),
                &kind,
                &invariant,
                &action,
                &Value::Null,
            ),
            "{label}"
        );
        if failure.is_some() {
            assert_eq!(value["violated_at_step"], 1, "{label}");
            assert_eq!(value["last_action"]["params"], json!({}), "{label}");
            let initial = fslc_rust::state_json(&sibling.state());
            let attempted = fslc_rust::state_json(&sibling.attempted());
            let trace = value["trace"].as_array().expect("replayable sibling trace");
            assert_eq!(trace.len(), 2, "{label}");
            assert_eq!(trace[0]["state"], initial, "{label}");
            assert_eq!(trace[1]["state"], attempted, "{label}");
            assert_eq!(trace[1]["action"]["name"], cell.action(), "{label}");
            assert_eq!(trace[1]["action"]["params"], json!({}), "{label}");
        } else {
            let invariants = if sibling.reachable_only {
                json!(["_bounds_xs"])
            } else if cell.induction_shape().is_some() {
                json!(["_bounds_xs", "Shape", cell.name])
            } else {
                json!(["_bounds_xs", cell.name])
            };
            assert_eq!(value["invariants_checked"], invariants, "{label}");
            let reached = format!("{}Reached", cell.name);
            let state = fslc_rust::state_json(&sibling.state());
            assert_eq!(
                value["reachables"]
                    .as_object()
                    .expect("reachable map")
                    .len(),
                1,
                "{label}"
            );
            assert_eq!(
                value["reachables"][&reached]["witnessed_at_step"], 0,
                "{label}"
            );
            assert_eq!(
                value["reachables"][&reached]["witness"],
                json!([{"step": 0, "state": state}]),
                "{label}"
            );
            assert_eq!(value["action_coverage"]["idle"], true, "{label}");
        }
    }

    fn monitor(sibling: &Sibling) {
        let cell = &sibling.base;
        let source = sibling.source();
        let kernel = parse_kernel_source(&source, &FsResolver::new(".")).expect("sibling parse");
        let model = build_model(kernel).expect("sibling model");
        let mut monitor = fsl_runtime::Monitor::new(model).expect("sibling init");
        let initial = sibling.state();
        assert_eq!(monitor.state, initial, "{} Monitor initial", cell.name);
        assert_eq!(
            monitor.model.invariants.len(),
            usize::from(!sibling.reachable_only) + usize::from(cell.induction_shape().is_some()),
            "{} Monitor invariant count",
            cell.name
        );
        assert_eq!(
            monitor.model.reachables.len(),
            usize::from(cell.property == Property::InvariantAndReachable),
            "{} Monitor reachable count",
            cell.name
        );
        assert_eq!(
            monitor.model.actions.len(),
            1,
            "{} Monitor closure",
            cell.name
        );
        for step in 0..=4 {
            for (index, property) in monitor.model.invariants.iter().enumerate() {
                let name = if cell.induction_shape().is_some() && index == 0 {
                    "Shape"
                } else {
                    cell.name
                };
                assert_eq!(property.name, name, "{} Monitor invariant", cell.name);
            }
            for property in &monitor.model.reachables {
                assert_eq!(
                    property.name,
                    format!("{}Reached", cell.name),
                    "{} Monitor reachable",
                    cell.name
                );
            }
            for property in monitor
                .model
                .invariants
                .iter()
                .chain(&monitor.model.reachables)
            {
                assert_eq!(
                    fsl_runtime::eval(
                        &property.expr,
                        &monitor.state,
                        &mut BTreeMap::new(),
                        &monitor.model,
                        None,
                    ),
                    Ok(FslValue::Bool(true)),
                    "{} Monitor property {} step {step}",
                    cell.name,
                    property.name
                );
            }
            if step == 4 {
                break;
            }
            let failure = match cell.property {
                Property::StateOverflow => Some(("type_bound", "_bounds_xs")),
                Property::ActionRead => Some(("invariant", cell.name)),
                _ => None,
            };
            let result = monitor
                .attempt(cell.action(), &BTreeMap::new())
                .unwrap_or_else(|error| panic!("{} Monitor step {step}: {error:?}", cell.name));
            eprintln!("{} Monitor step {}: {result:?}", cell.name, step + 1);
            let expected = fsl_runtime::StepResult {
                action: cell.action().to_owned(),
                params: BTreeMap::new(),
                state: initial.clone(),
                attempted_state: failure.map(|_| sibling.attempted()),
                violation: failure.map(|(kind, name)| fsl_runtime::Violation {
                    kind: kind.to_owned(),
                    name: name.to_owned(),
                    step: step + 1,
                }),
            };
            assert_eq!(result, expected, "{} Monitor step {}", cell.name, step + 1);
            assert_eq!(
                monitor.state, initial,
                "{} Monitor rollback/idle",
                cell.name
            );
        }
        // Idle stutters and failed actions roll back, so this is the complete
        // concrete reachable closure, not a sampled non-exhaustive trace.
    }

    macro_rules! sibling {
        ($module:ident, $name:literal, $capacity:literal, $initial:expr,
         $expression:literal, $property:ident, $condition:expr, $reachable_only:literal) => {
            mod $module {
                use super::*;

                const CELL: Sibling = Sibling {
                    base: Cell {
                        name: $name,
                        capacity: $capacity,
                        initial: $initial,
                        expression: $expression,
                        property: Property::$property,
                    },
                    condition: $condition,
                    reachable_only: $reachable_only,
                };

                #[test]
                fn check() {
                    super::check(&CELL);
                }

                #[test]
                fn bmc() {
                    verify(&CELL, "bmc", "verified");
                }

                #[test]
                fn induction() {
                    verify(&CELL, "induction", "proved");
                }

                #[test]
                fn explicit() {
                    verify(&CELL, "explicit", "proved");
                }

                #[test]
                fn monitor() {
                    super::monitor(&CELL);
                }
            }
        };
    }

    // B1 complements unchanged S1: [] -> [1] violates the declared bound 0.
    sibling!(b1, "B1", 0, &[], "true", StateOverflow, None, false);
    // Selecting [0,1] must not truncate it to [0] during the branch merge.
    sibling!(
        m1,
        "M1",
        1,
        &[0],
        "(if c then xs.push(1) else xs).at(1) == 1",
        InvariantAndReachable,
        Some(true),
        false
    );
    // Selecting [0] must neither expose padding nor retain the other length.
    sibling!(
        m2,
        "M2",
        1,
        &[0],
        "(if c then xs.push(1) else xs).size() == 1 and (if c then xs.push(1) else xs).head() == 0",
        InvariantAndReachable,
        Some(false),
        false
    );
    sibling!(
        q1,
        "Q1",
        1,
        &[0],
        "xs.push(1) == xs.push(1)",
        InvariantAndReachable,
        None,
        false
    );
    sibling!(
        q2,
        "Q2",
        1,
        &[0],
        "xs.push(1) != xs",
        InvariantAndReachable,
        None,
        false
    );
    // No invariant evaluates this literal: require a genuine reachable witness.
    sibling!(
        l2r,
        "L2R",
        1,
        &[0],
        "xs.push(1) == Seq { 0, 1 }",
        InvariantAndReachable,
        None,
        true
    );
    // In action context the literal is a temporary, never assigned to xs.
    // Its equality must select 1, causing an invariant violation with x=1.
    sibling!(
        l2a,
        "L2A",
        1,
        &[0],
        "if xs.push(1) == Seq { 0, 1 } then 1 else 0",
        ActionRead,
        None,
        false
    );
}

// Independent symbolic-bound detectors; leave the original 135 tests frozen.
mod symbolic_bounds {
    use super::*;

    const FREE_INIT_ERROR: &str = "init does not assign state variable(s): go";

    fn source(cell: &Cell) -> String {
        let initial = cell
            .initial
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        // A genuinely free root makes the concrete pre-pass inapplicable, so
        // these cases reach symbolic bounds instead of being masked by the
        // concrete overflow found in S1/B1. They must reject a P-only mutant:
        // push grows physical slots M, but state capacity must still be N.
        format!(
            "spec SymbolicBounds {{\n type K = 0..1\n state {{ xs: Seq<K, {}>, go: Bool }}\n init {{ xs = Seq {{ {initial} }} }}\n action take() {{ requires go  xs = xs.push(1) }}\n}}\n",
            cell.capacity
        )
    }

    fn model(cell: &Cell) -> Model {
        let model = Model::new(cell);
        std::fs::write(model.0.join("model.fsl"), source(cell))
            .expect("write symbolic bounds model");
        model
    }

    fn check(cell: &Cell) {
        let model = model(cell);
        let (value, exit) = model.run(cell, &["check", "model.fsl"]);
        assert_eq!(
            (exit, &value["result"], &value["message"]),
            (0, &json!("ok"), &Value::Null),
            "{} check: {value}",
            cell.name
        );
    }

    fn verify(cell: &Cell, engine: &str) {
        let model = model(cell);
        let (value, exit) = model.run(
            cell,
            &[
                "verify",
                "model.fsl",
                "--engine",
                engine,
                "--depth",
                "3",
                "--no-cache",
            ],
        );
        let label = format!("{} {engine}: exit={exit} {value}", cell.name);
        if engine == "explicit" {
            assert_eq!(
                (exit, &value["result"], &value["message"]),
                (2, &json!("error"), &json!(FREE_INIT_ERROR)),
                "{label}"
            );
        } else {
            // Independently: xs initially has N elements, go=true is allowed,
            // and take appends one. Step zero is valid; step one exceeds N.
            // Pin the semantic violation, not an unobserved projection format
            // for an oversized attempted state. Model::run logs the full JSON.
            assert_eq!(
                (
                    exit,
                    &value["result"],
                    &value["violation_kind"],
                    &value["invariant"],
                    &value["violated_at_step"],
                ),
                (
                    1,
                    &json!("violated"),
                    &json!("type_bound"),
                    &json!("_bounds_xs"),
                    &json!(1),
                ),
                "{label}"
            );
        }
    }

    fn monitor(cell: &Cell) {
        let kernel = parse_kernel_source(&source(cell), &FsResolver::new("."))
            .expect("symbolic bounds Monitor parse");
        let model = build_model(kernel).expect("symbolic bounds Monitor model");
        let Err(error) = fsl_runtime::Monitor::new(model) else {
            panic!("{} Monitor must reject free init", cell.name);
        };
        eprintln!("{} Monitor init: {error:?}", cell.name);
        assert_eq!(error.message, FREE_INIT_ERROR, "{} Monitor init", cell.name);
    }

    macro_rules! bound_cell {
        ($module:ident, $name:literal, $capacity:literal, $initial:expr) => {
            mod $module {
                use super::*;

                const CELL: Cell = Cell {
                    name: $name,
                    capacity: $capacity,
                    initial: $initial,
                    expression: "true",
                    property: Property::StateOverflow,
                };

                #[test]
                fn check() {
                    super::check(&CELL);
                }

                #[test]
                fn bmc() {
                    verify(&CELL, "bmc");
                }

                #[test]
                fn induction() {
                    verify(&CELL, "induction");
                }

                #[test]
                fn explicit() {
                    verify(&CELL, "explicit");
                }

                #[test]
                fn monitor() {
                    super::monitor(&CELL);
                }
            }
        };
    }

    // [0].push(1) attempts [0,1], length 2 > declared capacity 1.
    bound_cell!(b2, "B2", 1, &[0]);
    // [].push(1) attempts [1], length 1 > declared capacity 0.
    bound_cell!(b3, "B3", 0, &[]);
}

// Selected-property fail-closed detectors: an overflowing state frame must not
// lose the appended value and emit a wrong verdict (false violated for SF1,
// false verified for SF2; both polarities for SF3). Keep all 145 existing tests
// unchanged. The identical
// sources without property selection also pin concrete type-bound evidence.
mod selected_overflow {
    use super::*;

    const SF1: &str = r"spec ReviewSelectedOverflowRead {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { 0 } }
  action take() { xs = xs.push(1) }
  invariant P { xs.size() == 1 or xs.at(1) == 1 }
}
";

    const SF2: &str = r"spec ReviewSelectedOverflowReachable {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { 0 } }
  action take() { xs = xs.push(1) }
  reachable R { xs.size() == 2 and xs.at(1) == 0 }
}
";

    const SF3: &str = r"spec P12 {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { } }
  action a1() { requires xs.size() == 0  xs = xs.push(1) }
  action a0() { requires xs.size() == 1  xs = xs.push(0) }
  invariant P { xs.size() == 2 => xs.at(1) == 0 }
  reachable R { xs.size() == 2 and xs.at(1) == 1 }
}
";

    fn assert_default(value: &Value, exit: i32, two_step: bool, label: &str) {
        // Independently: [0,1] (SF1/SF2) or [1,0] (SF3) exceeds N=1.
        // All-properties checking must report the bound, not P or R.
        assert_eq!(
            (
                exit,
                &value["result"],
                &value["violation_kind"],
                &value["invariant"],
                &value["violated_at_step"],
                &value["message"],
            ),
            (
                1,
                &json!("violated"),
                &json!("type_bound"),
                &json!("_bounds_xs"),
                &json!(if two_step { 2 } else { 1 }),
                &Value::Null,
            ),
            "{label}"
        );
        let trace = value["trace"].as_array().expect("concrete overflow trace");
        if two_step {
            assert_eq!(trace.len(), 3, "{label}");
            assert_eq!(trace[0]["state"], json!({"xs": []}), "{label}");
            assert_eq!(trace[1]["state"], json!({"xs": [1]}), "{label}");
            assert_eq!(trace[2]["state"], json!({"xs": [1, 0]}), "{label}");
            assert_eq!(trace[1]["action"]["name"], "a1", "{label}");
            assert_eq!(trace[2]["action"]["name"], "a0", "{label}");
        } else {
            assert_eq!(trace.len(), 2, "{label}");
            assert_eq!(trace[0]["state"], json!({"xs": [0]}), "{label}");
            assert_eq!(trace[1]["state"], json!({"xs": [0, 1]}), "{label}");
        }
        assert_eq!(
            value["last_action"]["name"],
            if two_step { "a0" } else { "take" },
            "{label}"
        );
        assert_eq!(value["last_action"]["params"], json!({}), "{label}");
    }

    fn verify(name: &'static str, source: &str, property: &str, engine: &str, selected: bool) {
        let two_step = name == "SF3";
        let cell = Cell {
            name,
            capacity: 1,
            initial: if two_step { &[] } else { &[0] },
            expression: "true",
            property: Property::StateOverflow,
        };
        let model = Model::new(&cell);
        std::fs::write(model.0.join("model.fsl"), source).expect("write selected overflow model");
        let mut args = vec![
            "verify",
            "model.fsl",
            "--engine",
            engine,
            "--depth",
            if two_step { "2" } else { "1" },
            "--no-cache",
        ];
        if selected {
            args.extend(["--property", property]);
        }
        let (value, exit) = model.run(&cell, &args);
        let label =
            format!("{name} {engine} property={property} selected={selected}: exit={exit} {value}");
        if !selected {
            assert_default(&value, exit, two_step, &label);
        } else if engine == "induction" && property == "R" {
            // Selecting a reachable is unsupported by induction, independent
            // of overflow. This is a usage rejection, never a proof of R.
            assert_eq!(
                (exit, &value["result"], &value["kind"]),
                (2, &json!("error"), &json!("usage")),
                "{label}"
            );
            assert!(
                value["message"].as_str().is_some_and(|message| {
                    message.contains("which the induction engine cannot prove")
                }),
                "{label}"
            );
        } else {
            // ANS-1519selected option F: bounds are not selected, but a state
            // frame of capacity 1 cannot represent length 2. Fail closed at
            // that boundary instead of proving anything using a lost suffix.
            // Explicit now fails closed like bmc and induction when a
            // successor exceeds an unchecked Seq capacity (#1531).
            assert_eq!(
                (exit, &value["result"], &value["kind"]),
                (2, &json!("error"), &json!("semantics")),
                "{label}"
            );
            assert!(
                value["message"].as_str().is_some_and(|message| {
                    message.contains("model sequence length exceeds capacity")
                }),
                "{label}"
            );
        }
    }

    macro_rules! selected_cell {
        ($module:ident, $name:literal, $source:ident, $property:literal) => {
            mod $module {
                use super::*;

                #[test]
                fn selected_bmc() {
                    verify($name, $source, $property, "bmc", true);
                }

                #[test]
                fn selected_induction() {
                    verify($name, $source, $property, "induction", true);
                }

                #[test]
                fn selected_explicit() {
                    verify($name, $source, $property, "explicit", true);
                }

                #[test]
                fn default_bmc() {
                    verify($name, $source, $property, "bmc", false);
                }

                #[test]
                fn default_induction() {
                    verify($name, $source, $property, "induction", false);
                }

                #[test]
                fn default_explicit() {
                    verify($name, $source, $property, "explicit", false);
                }
            }
        };
    }

    selected_cell!(sf1, "SF1", SF1, "P");
    selected_cell!(sf2, "SF2", SF2, "R");

    mod sf3 {
        use super::*;

        macro_rules! engine_cell {
            ($module:ident, $engine:literal) => {
                mod $module {
                    use super::*;

                    #[test]
                    fn selected_p() {
                        verify("SF3", SF3, "P", $engine, true);
                    }

                    #[test]
                    fn selected_r() {
                        verify("SF3", SF3, "R", $engine, true);
                    }

                    #[test]
                    fn default() {
                        verify("SF3", SF3, "", $engine, false);
                    }
                }
            };
        }

        engine_cell!(bmc, "bmc");
        engine_cell!(induction, "induction");
        engine_cell!(explicit, "explicit");
    }
}
