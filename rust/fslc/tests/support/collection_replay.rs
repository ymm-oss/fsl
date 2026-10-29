// SPDX-License-Identifier: Apache-2.0

//! Verdict-class agreement with a fixed concrete trace, retaining every observation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[allow(dead_code)]
#[path = "compose_scratch.rs"]
mod cli;

pub struct ReplayCase {
    pub name: String,
    directory: PathBuf,
    verify_options: Vec<String>,
    expected_trace_state: Option<(String, Value)>,
    expected_property: Option<(String, Option<String>)>,
}

impl ReplayCase {
    pub fn new(name: &str, source: &str, actions: &[&str]) -> Self {
        let trace: Vec<_> = actions
            .iter()
            .map(|action| json!({"action": action, "params": {}}))
            .collect();
        let directory = cli::scratch(
            name,
            &[
                ("input.fsl", source),
                (
                    "actions.json",
                    &serde_json::to_string_pretty(&trace).unwrap(),
                ),
            ],
        );
        println!("EVIDENCE {name} {}", directory.display());
        Self {
            name: name.to_owned(),
            directory,
            verify_options: Vec::new(),
            expected_trace_state: None,
            expected_property: None,
        }
    }

    pub fn with_verify_options(mut self, options: &[&str]) -> Self {
        self.verify_options = options.iter().map(|value| (*value).to_owned()).collect();
        self
    }

    pub fn with_expected_trace_state(mut self, variable: &str, value: Value) -> Self {
        self.expected_trace_state = Some((variable.to_owned(), value));
        self
    }

    pub fn with_expected_property(mut self, property: &str, induction_bound: Option<&str>) -> Self {
        self.expected_property = Some((property.to_owned(), induction_bound.map(str::to_owned)));
        self
    }

    fn arguments<'a>(&'a self, route: &'a str, depth: &'a str) -> Vec<&'a str> {
        let mut args = match route {
            "check" => vec!["check", "input.fsl"],
            "monitor" => vec!["replay", "input.fsl", "--trace", "actions.json"],
            "kernel-v1" | "kernel-v2" => vec![
                "kernel",
                "input.fsl",
                "--kernel-version",
                route.strip_prefix("kernel-v").unwrap(),
            ],
            _ => vec![
                "verify",
                "input.fsl",
                "--engine",
                route,
                "--depth",
                depth,
                "--no-cache",
            ],
        };
        if ["bmc", "induction", "explicit"].contains(&route) {
            args.extend(self.verify_options.iter().map(String::as_str));
        }
        args
    }

    pub fn observe(&self, route: &str, depth: usize) -> (Value, i32) {
        let depth_string = depth.to_string();
        let args = self.arguments(route, &depth_string);
        let (report, exit) = cli::run(&self.directory, &args);
        let observation = json!({"args": args, "exit": exit, "report": report});
        std::fs::write(
            self.directory.join(format!("{route}-{depth}.json")),
            serde_json::to_vec_pretty(&observation).unwrap(),
        )
        .expect("retain complete CLI observation");
        println!(
            "CELL {} {route} depth={depth} produced={exit}:{} completeness={} evidence={}",
            self.name,
            report["result"],
            report["completeness"],
            self.directory.display()
        );
        (report, exit)
    }

    pub fn verify_with_deadline(
        &self,
        engine: &str,
        depth: usize,
        limit: Duration,
        errors: &mut Vec<String>,
    ) {
        self.verify_with_expected_deadline(engine, depth, limit, None, errors);
    }

    pub fn verify_with_expected_deadline(
        &self,
        engine: &str,
        depth: usize,
        limit: Duration,
        expected: Option<(&str, &str)>,
        errors: &mut Vec<String>,
    ) {
        let depth_string = depth.to_string();
        let args = self.arguments(engine, &depth_string);
        let prefix = format!("{engine}-{depth}-bounded");
        let stdout = self.directory.join(format!("{prefix}.stdout.json"));
        let stderr = self.directory.join(format!("{prefix}.stderr.txt"));
        // File-backed output cannot fill a pipe while the parent polls time.
        let mut child = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(&args)
            .current_dir(&self.directory)
            .env("FSLC_CACHE", "off")
            .env("TMPDIR", &self.directory)
            .stdout(Stdio::from(
                File::create_new(&stdout).expect("fresh bounded stdout"),
            ))
            .stderr(Stdio::from(
                File::create_new(&stderr).expect("fresh bounded stderr"),
            ))
            .spawn()
            .expect("spawn deadline-bound native CLI");
        let started = Instant::now();
        let mut timed_out = false;
        let mut kill_error = None;
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll deadline-bound CLI") {
                break status;
            }
            if started.elapsed() >= limit {
                timed_out = true;
                kill_error = child.kill().err().map(|error| error.to_string());
                // Reap even if the child exited between the poll and kill.
                break child.wait().expect("reap deadline-bound CLI");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let elapsed = started.elapsed().as_secs_f64();
        std::fs::write(
            self.directory.join(format!("{prefix}.command.json")),
            serde_json::to_vec_pretty(&json!({
                "args": args, "exit": status.code(), "elapsed_s": elapsed,
                "limit_s": limit.as_secs_f64(), "timed_out": timed_out, "kill_error": kill_error
            }))
            .unwrap(),
        )
        .expect("retain bounded process result");
        println!(
            "BOUNDED {} {engine} depth={depth} elapsed_s={elapsed:.3} timed_out={timed_out} exit={status}",
            self.name
        );
        if timed_out {
            errors.push(format!(
                "{} {engine} depth {depth} exceeded {}s; child killed and reaped; evidence={}",
                self.name,
                limit.as_secs(),
                self.directory.display()
            ));
            return;
        }
        if !status.success() {
            errors.push(format!(
                "{} {engine} depth {depth} failed with {status}; evidence={}",
                self.name,
                self.directory.display()
            ));
            return;
        }
        let bytes = std::fs::read(&stdout).expect("read bounded stdout");
        let report: Value = serde_json::from_slice(&bytes).expect("bounded CLI JSON");
        require(
            errors,
            matches!(report["result"].as_str(), Some("verified" | "proved")),
            &self.name,
            engine,
            &report,
            status.code().expect("successful process exit code"),
        );
        if let Some((result, completeness)) = expected {
            require(
                errors,
                report["result"] == result && report["completeness"] == completeness,
                &self.name,
                "bounded verdict and completeness",
                &report,
                status.code().unwrap(),
            );
        }
    }

    pub fn kernel_candidates(&self, version: usize, member: i64, errors: &mut Vec<String>) {
        self.inspect_kernel_candidates(version, member, &[member], false, errors);
    }

    pub fn kernel_captured_candidates(
        &self,
        version: usize,
        members: &[i64],
        errors: &mut Vec<String>,
    ) {
        self.inspect_kernel_candidates(version, 2, members, true, errors);
    }

    fn inspect_kernel_candidates(
        &self,
        version: usize,
        member: i64,
        members: &[i64],
        check_guard: bool,
        errors: &mut Vec<String>,
    ) {
        let route = format!("kernel-v{version}");
        let (report, exit) = self.observe(&route, 4);
        let exported = exit == 0 && report["result"] == "kernel";
        require(errors, exported, &self.name, &route, &report, exit);
        if !exported {
            return; // A refused export has already failed acceptance.
        }
        let body = report["actions"][0]["guards"]
            .as_array()
            .expect("exported action body");
        // Evaluate lets in order: captures retain their old value when a source
        // variable or collection name is subsequently shadowed.
        let mut bindings = BTreeMap::from([
            ("n".to_owned(), json!(member)),
            ("s".to_owned(), json!([])),
            ("r".to_owned(), json!([])),
        ]);
        for statement in body {
            if statement["kind"] == "let" {
                let value = export_value(&statement["value"], &bindings);
                bindings.insert(statement["name"].as_str().unwrap().to_owned(), value);
            }
        }
        let guard = &body
            .iter()
            .rev()
            .find(|statement| statement["kind"] == "requires")
            .expect("aggregate guard")["expression"];
        let mut candidates = BTreeSet::new();
        collect_candidates(guard, &bindings, &mut candidates);
        let expected: BTreeSet<_> = members.iter().copied().collect();
        let live: BTreeSet<_> = candidates.intersection(&expected).copied().collect();
        println!(
            "CANDIDATES {} {route} enumerated={candidates:?} live={live:?} expected={expected:?}",
            self.name
        );
        std::fs::write(
            self.directory.join(format!("{route}-candidates.json")),
            serde_json::to_vec_pretty(
                &json!({"enumerated": candidates, "live": live, "expected": expected}),
            )
            .unwrap(),
        )
        .expect("retain semantic candidate observation");
        if live != expected {
            errors.push(format!("{} {route}: outside member dropped from export; live={live:?}, expected={expected:?}, enumerated={candidates:?}", self.name));
        }
        if check_guard && export_value(guard, &bindings) != json!(true) {
            errors.push(format!(
                "{} {route}: exported aggregate guard rejects the Monitor-enabled action",
                self.name
            ));
        }
    }

    pub fn agreement(
        &self,
        depth: usize,
        conformant: bool,
        bound: Option<&str>,
        errors: &mut Vec<String>,
    ) {
        let (checked, exit) = self.observe("check", depth);
        require(
            errors,
            exit == 0 && checked["result"] == "ok",
            &self.name,
            "check",
            &checked,
            exit,
        );
        let (monitor, exit) = self.observe("monitor", depth);
        require(
            errors,
            if conformant {
                exit == 0 && monitor["result"] == "conformant"
            } else {
                exit == 1 && monitor["result"] == "nonconformant"
            },
            &self.name,
            "monitor",
            &monitor,
            exit,
        );
        if let Some(bound) = bound {
            require(
                errors,
                has_named_bound(&monitor, bound),
                &self.name,
                "monitor bound",
                &monitor,
                exit,
            );
        }
        for engine in ["bmc", "induction", "explicit"] {
            let (report, exit) = self.observe(engine, depth);
            // Unknown CTIs, refusals (exit 2), and tool errors are not agreement.
            let agrees = if conformant {
                exit == 0 && matches!(report["result"].as_str(), Some("verified" | "proved"))
            } else {
                exit == 1 && report["result"] == "violated"
            };
            require(errors, agrees, &self.name, engine, &report, exit);
            if let Some((property, induction_bound)) = &self.expected_property {
                require(
                    errors,
                    if engine == "induction"
                        && let Some(bound) = induction_bound
                    {
                        has_named_bound(&report, bound)
                    } else {
                        report["invariant"] == *property
                    },
                    &self.name,
                    &format!("{engine} selected property"),
                    &report,
                    exit,
                );
            }
            if let Some(bound) = bound {
                require(
                    errors,
                    has_named_bound(&report, bound),
                    &self.name,
                    &format!("{engine} bound"),
                    &report,
                    exit,
                );
            }
            if let Some((variable, expected)) = &self.expected_trace_state {
                let state = report["trace"]
                    .as_array()
                    .and_then(|trace| trace.last())
                    .map(|step| &step["state"][variable]);
                println!(
                    "TRACE {} {engine} {variable} produced={state:?} expected={expected}",
                    self.name
                );
                require(
                    errors,
                    state == Some(expected),
                    &self.name,
                    &format!("{engine} retained trace"),
                    &report,
                    exit,
                );
            }
        }
    }

    pub fn snapshot_bounds(&self, source: &str, member: i64, errors: &mut Vec<String>) {
        use fsl_core::FslValue;
        use std::future::Future;
        use std::task::{Context, Poll, Waker};

        let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
            .expect("snapshot fixture parses");
        let model = fsl_core::build_model(kernel).expect("snapshot fixture checks");
        let snapshot = BTreeMap::from([
            (
                "s".to_owned(),
                FslValue::Set(BTreeSet::from([FslValue::Int(member)])),
            ),
            ("n".to_owned(), FslValue::Int(member)),
        ]);
        let mut monitor = fsl_runtime::Monitor::from_state(model.clone(), snapshot.clone());
        let enabled = monitor
            .enabled()
            .expect("snapshot action guards")
            .into_iter()
            .find(|action| action.action == "go")
            .expect("fixed snapshot go action");
        let replay = monitor.step(&enabled).expect("snapshot Monitor step");
        let mut solver = fsl_solver_z3::Z3Solver::new().expect("native snapshot solver");
        let mut future = std::pin::pin!(fsl_verifier::verify_bounded_from_state(
            &model,
            &mut solver,
            1,
            None,
            &snapshot
        ));
        let result = match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("native snapshot solver unexpectedly yielded"),
        };
        std::fs::write(
            self.directory.join("snapshot-api.json"),
            serde_json::to_vec_pretty(&json!({
                "snapshot": {"s": [member], "n": member},
                "bmc": format!("{result:?}"), "monitor": format!("{replay:?}")
            }))
            .unwrap(),
        )
        .expect("retain snapshot API observations");
        let monitor_bound = replay.violation.as_ref().is_some_and(|violation| {
            violation.kind == "type_bound" && violation.name == "_bounds_s"
        });
        if !monitor_bound {
            errors.push(format!(
                "{} snapshot Monitor: expected _bounds_s, produced {replay:?}",
                self.name
            ));
        }
        match result {
            Ok(result) => {
                let bound = result.violation.as_ref().is_some_and(|violation| {
                    violation.kind == "type_bound" && violation.name == "_bounds_s"
                });
                let retained = result
                    .violation
                    .as_ref()
                    .and_then(|violation| violation.trace.first())
                    .and_then(|step| step.state.get("s"))
                    == snapshot.get("s");
                println!("SNAPSHOT {} bound={bound} retained={retained}", self.name);
                if !bound || !retained {
                    errors.push(format!(
                        "{} snapshot BMC: expected retained _bounds_s witness, produced {result:?}",
                        self.name
                    ));
                }
            }
            Err(error) => errors.push(format!(
                "{} snapshot BMC refused instead of preserving the member: {error}",
                self.name
            )),
        }
    }
}

// Only the normalized let/sum/add/count fragment exercised by these fixtures.
// This is not a general Public Kernel interpreter or a copy of native eval.
fn export_value(expression: &Value, bindings: &BTreeMap<String, Value>) -> Value {
    let eval = |value: &Value| export_value(value, bindings);
    match expression["kind"].as_str().expect("export expression kind") {
        "num" | "bool" => expression["value"].clone(),
        "var" => bindings[expression["name"].as_str().unwrap()].clone(),
        "not" => json!(!eval(&expression["operand"]).as_bool().unwrap()),
        "ite" => eval(if eval(&expression["condition"]).as_bool().unwrap() {
            &expression["then"]
        } else {
            &expression["else"]
        }),
        "binary" => {
            let left = eval(&expression["left"]);
            let right = eval(&expression["right"]);
            match expression["operator"].as_str().unwrap() {
                "+" => json!(left.as_i64().unwrap() + right.as_i64().unwrap()),
                "and" => json!(left.as_bool().unwrap() && right.as_bool().unwrap()),
                "==" => json!(left == right),
                "!=" => json!(left != right),
                operator => panic!("unsupported fixture export operator {operator}"),
            }
        }
        "method" => {
            let receiver = eval(&expression["receiver"]);
            let mut items = receiver.as_array().unwrap().clone();
            let args: Vec<_> = expression["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .map(eval)
                .collect();
            let item = if args.len() == 1 {
                args[0].clone()
            } else {
                json!(args)
            };
            match expression["method"].as_str().unwrap() {
                "contains" => json!(items.contains(&item)),
                "add" => {
                    if !items.contains(&item) {
                        items.push(item);
                    }
                    json!(items)
                }
                method => panic!("unsupported fixture export method {method}"),
            }
        }
        "rel_domain" | "rel_range" => {
            let index = usize::from(expression["kind"] == "rel_range");
            let relation = eval(&expression["operand"]);
            let mut values = Vec::new();
            for pair in relation.as_array().unwrap() {
                if !values.contains(&pair[index]) {
                    values.push(pair[index].clone());
                }
            }
            json!(values)
        }
        kind => panic!("unsupported fixture export expression {kind}"),
    }
}

fn collect_candidates(
    expression: &Value,
    bindings: &BTreeMap<String, Value>,
    candidates: &mut BTreeSet<i64>,
) {
    if expression["kind"] == "method" && expression["method"] == "contains" {
        let arguments = expression["arguments"]
            .as_array()
            .expect("membership arguments");
        assert_eq!(arguments.len(), 1, "aggregate iterates scalar Set members");
        candidates.insert(
            export_value(&arguments[0], bindings)
                .as_i64()
                .expect("numeric aggregate candidate or captured scalar"),
        );
    }
    match expression {
        Value::Object(fields) => {
            for value in fields.values() {
                collect_candidates(value, bindings, candidates);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_candidates(value, bindings, candidates);
            }
        }
        _ => {}
    }
}

fn has_named_bound(report: &Value, bound: &str) -> bool {
    (report["invariant"] == bound && report["violation_kind"] == "type_bound")
        || (report["violation"]["name"] == bound && report["violation"]["kind"] == "type_bound")
}

pub fn require(
    errors: &mut Vec<String>,
    condition: bool,
    name: &str,
    route: &str,
    report: &Value,
    exit: i32,
) {
    if !condition {
        errors.push(format!(
            "{name} {route}: exit {exit}, result={}, completeness={}, invariant={}, message={}",
            report["result"], report["completeness"], report["invariant"], report["message"]
        ));
    }
}

pub fn finish(errors: &[String]) {
    assert!(
        errors.is_empty(),
        "{} disagreement(s):\n{}",
        errors.len(),
        errors.join("\n")
    );
}
