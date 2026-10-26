// SPDX-License-Identifier: Apache-2.0

//! Execute the native-authored contract, not a second hand-written command list.
//! Missing inputs keep these probes solver-free and prevent output writes. Acceptance
//! means reaching a downstream error, not merely returning something other than 0.
//! Each sweep collects ALL failures so calibration cannot stop at the first leak.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

const MISSING: &str = ".codex/tmp/fsl-contract-no-such-input.fsl";
const OUTPUT: &str = ".codex/tmp/fsl-contract-no-such-directory/output";
const PROBE: &str = "--fsl-contract-probe";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
}

fn run(args: &[String]) -> Output {
    assert!(!root().join(MISSING).exists(), "probe input must not exist");
    assert!(!root().join(OUTPUT).parent().unwrap().exists());
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .env("TMPDIR", root().join(".codex/tmp"))
        .output()
        .expect("run native CLI")
}

fn contract() -> Value {
    serde_json::from_str(include_str!("../cli-contract.json")).expect("CLI contract")
}

fn walk<'a>(node: &'a Value, nodes: &mut Vec<&'a Value>) {
    nodes.push(node);
    for child in node["commands"].as_array().expect("commands") {
        walk(child, nodes);
    }
}

fn nodes(contract: &Value) -> Vec<&Value> {
    let mut nodes = Vec::new();
    walk(&contract["root"], &mut nodes);
    assert_eq!(nodes.len(), 61, "review additions/removals explicitly");
    assert_eq!(
        nodes
            .iter()
            .filter(|node| children(node).is_empty())
            .count(),
        52,
        "review additions/removals explicitly"
    );
    nodes
}

fn children(node: &Value) -> &[Value] {
    node["commands"].as_array().expect("commands")
}

fn actions(node: &Value) -> &[Value] {
    node["actions"].as_array().expect("actions")
}

fn flags(action: &Value) -> Vec<&str> {
    action["flags"]
        .as_array()
        .map(|flags| flags.iter().map(|flag| flag.as_str().unwrap()).collect())
        .unwrap_or_default()
}

fn path(node: &Value) -> Vec<String> {
    node["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap().to_owned())
        .collect()
}

fn label(node: &Value) -> String {
    let path = path(node).join(" ");
    if path.is_empty() {
        "fslc".to_owned()
    } else {
        path
    }
}

fn takes_value(action: &Value) -> bool {
    action["nargs"] != 0
}

fn sample(node: &Value, action: &Value) -> String {
    if let Some(default) = action.get("default")
        && (default.is_string() || default.is_number())
    {
        return default
            .as_str()
            .map_or_else(|| default.to_string(), str::to_owned);
    }
    if let Some(choices) = action["choices"].as_array() {
        return choices[0].as_str().unwrap().to_owned();
    }
    match action["dest"].as_str().unwrap() {
        "instances" if label(node) == "sweep" => "Probe=1..1".to_owned(),
        "instances" => "Probe=1".to_owned(),
        "values" => "Probe=0..1".to_owned(),
        "min_kill_rate" => "0.5".to_owned(),
        "output" | "out" | "lifecycle_out" => OUTPUT.to_owned(),
        "git" | "git_range" => "HEAD..HEAD".to_owned(),
        "period_start" => "2026-01-01T00:00:00Z".to_owned(),
        "period_end" | "as_of" => "2026-01-02T00:00:00Z".to_owned(),
        _ => MISSING.to_owned(),
    }
}

/// Context only for constraints not represented by actions (XOR / conditional
/// requirements). These are not command/flag inventories and never skip a probe.
fn context(node: &Value, tested: &str) -> Vec<String> {
    let mut args = path(node);
    for action in actions(node) {
        if action["positional"] == true {
            // Fill optional positionals too: this witnesses the finite maximum.
            if !matches!(action["nargs"].as_str(), Some("*")) {
                args.push(MISSING.to_owned());
            }
        } else if action["required"] == true && !flags(action).contains(&tested) {
            args.push(flags(action)[0].to_owned());
            args.push(sample(node, action));
        }
    }
    let extra: &[&str] = match label(node).as_str() {
        "replay" if tested == "--from-log" || tested == "--mapping" => {
            if tested == "--mapping" {
                &["--from-log", MISSING]
            } else {
                &["--mapping", MISSING]
            }
        }
        "replay" if tested != "--trace" => &["--trace", MISSING],
        "causal analyze" if tested != "--profile" && tested != "--projection" => {
            &["--projection", "causal_graph"]
        }
        _ => &[],
    };
    args.extend(extra.iter().map(|value| (*value).to_owned()));
    // approval's glossary/evidence belong to requirements_document, not ledger.
    if label(node) == "approval create" && matches!(tested, "--glossary" | "--evidence") {
        let index = args.iter().position(|value| value == "--kind").unwrap();
        "requirements_document".clone_into(&mut args[index + 1]);
    }
    args
}

fn append(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    args.push(flag.to_owned());
    if let Some(value) = value {
        args.push(value.to_owned());
    }
}

fn decoded(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

fn usage(output: &Output) -> bool {
    let value = decoded(output);
    output.status.code() == Some(2)
        && output.stderr.is_empty()
        && value["fsl"] == "1.0"
        && value["result"] == "error"
        && value["kind"] == "usage"
        && value["message"].is_string()
        && value.as_object().is_some_and(|object| object.len() == 4)
}

#[derive(Default)]
struct Sweep {
    checked: usize,
    failures: Vec<String>,
    by_command: BTreeMap<String, usize>,
    classifications: BTreeMap<(String, String, String), usize>,
}

impl Sweep {
    fn check(&mut self, category: &str, args: &[String], output: &Output, ok: bool) {
        self.checked += 1;
        if !ok {
            let command = args
                .iter()
                .take_while(|arg| {
                    !arg.starts_with('-') && arg.as_str() != MISSING && !arg.contains('/')
                })
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            *self.by_command.entry(command.clone()).or_default() += 1;
            let value = decoded(output);
            let class = if usage(output) { "B" } else { "A" };
            let mut shape = value["message"]
                .as_str()
                .unwrap_or("non-JSON output")
                .replace(MISSING, "<input>");
            if let Some(flag) = args.iter().rev().find(|arg| arg.starts_with('-')) {
                shape = shape.replace(flag, "<option>");
            }
            *self
                .classifications
                .entry((class.to_owned(), command, shape))
                .or_default() += 1;
            self.failures.push(format!(
                "{category} argv={args:?} exit={:?} stdout={} stderr={}",
                output.status.code(),
                serde_json::from_slice::<Value>(&output.stdout).map_or_else(
                    |_| String::from_utf8_lossy(&output.stdout)
                        .trim_end()
                        .to_owned(),
                    |value| value.to_string()
                ),
                String::from_utf8_lossy(&output.stderr).trim_end()
            ));
        }
    }

    fn finish(self, category: &str) {
        println!(
            "{category}: checked={} failed={}",
            self.checked,
            self.failures.len()
        );
        for (command, count) in self.by_command {
            println!("summary {category} command={command:?} failed={count}");
        }
        for ((class, command, shape), count) in self.classifications {
            println!(
                "classification {category} class={class} command={command:?} shape={shape:?} failed={count}"
            );
        }
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
    }
}

#[test]
fn advertised_options_reach_downstream_validation() {
    let contract = contract();
    let mut sweep = Sweep::default();
    for node in nodes(&contract) {
        for action in actions(node) {
            for flag in flags(action) {
                // Help has its own byte-exact live check below (it short-circuits parsing).
                if matches!(flag, "-h" | "--help") {
                    continue;
                }
                let values: Vec<Option<String>> = if !takes_value(action) {
                    vec![None]
                } else if let Some(choices) = action["choices"].as_array() {
                    choices
                        .iter()
                        .map(|value| Some(value.as_str().unwrap().to_owned()))
                        .collect()
                } else {
                    vec![Some(sample(node, action))]
                };
                for value in values {
                    let mut args = context(node, flag);
                    append(&mut args, flag, value.as_deref());
                    let output = run(&args);
                    let result = decoded(&output);
                    // --git with two paths proves recognition without running Git or
                    // materializing/deleting a tree. This exact downstream arity error
                    // is an arity witness; the artifact-I/O witness is below.
                    let git_witness = flag == "--git"
                        && usage(&output)
                        && result["message"] == "--git accepts at most one spec path";
                    let version = matches!(flag, "-V" | "--version")
                        && output.status.success()
                        && output.stdout
                            == format!("fslc {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
                        && output.stderr.is_empty();
                    let downstream = output.status.code() == Some(2)
                        && result["result"] == "error"
                        && matches!(
                            result["kind"].as_str(),
                            Some("io" | "semantics" | "schema" | "parse" | "config")
                        );
                    // Observed: approval's requirements_document input loader maps
                    // artifact I/O into Err(String), hence usage. Require the exact
                    // OS missing-file message, never accept arbitrary usage errors.
                    let artifact_io = label(node) == "approval create"
                        && usage(&output)
                        && result["message"] == "No such file or directory (os error 2)";
                    sweep.check(
                        "accepted",
                        &args,
                        &output,
                        git_witness || version || downstream || artifact_io,
                    );
                }
            }
        }
    }
    sweep.finish("accepted");
}

#[test]
fn every_unadvertised_spelling_is_usage_with_the_option_named() {
    let contract = contract();
    let nodes = nodes(&contract);
    let mut vocabulary: BTreeMap<String, BTreeSet<Option<String>>> = BTreeMap::new();
    for node in &nodes {
        for action in actions(node) {
            for flag in flags(action) {
                if matches!(flag, "-h" | "--help") {
                    continue;
                }
                let values = vocabulary.entry(flag.to_owned()).or_default();
                if takes_value(action) {
                    values.insert(Some(sample(node, action)));
                    if let Some(choices) = action["choices"].as_array() {
                        values.extend(
                            choices
                                .iter()
                                .map(|value| Some(value.as_str().unwrap().to_owned())),
                        );
                    }
                } else {
                    values.insert(None);
                }
            }
        }
    }
    // P-C spellings from the independent parser inventory (sigma-b1-ref 9b),
    // including gated extras. All other P-C spellings already occur in C elsewhere.
    for (flag, value) in [
        (PROBE, None),
        ("--cli-contract", None),
        ("--allow-unwired", None),
        ("--names", Some("canonical")),
        ("--lowering", None),
        ("--lang", Some("en")),
        ("--strict-names", None),
    ] {
        vocabulary
            .entry(flag.to_owned())
            .or_default()
            .insert(value.map(str::to_owned));
    }
    let mut sweep = Sweep::default();
    for node in nodes {
        let own: BTreeSet<_> = actions(node).iter().flat_map(flags).collect();
        for (flag, values) in &vocabulary {
            if own.contains(flag.as_str()) {
                continue;
            }
            // Introspection is live-checked below. The two allow-unwired additions
            // are tested as metadata defects plus effective behavior, not removals.
            if (path(node).is_empty() && flag == "--cli-contract")
                || (matches!(label(node).as_str(), "testgen" | "domain testgen")
                    && flag == "--allow-unwired")
            {
                continue;
            }
            for value in values {
                let mut args = context(node, flag);
                append(&mut args, flag, value.as_deref());
                let output = run(&args);
                let command = label(node);
                let expected = json!({"fsl":"1.0", "result":"error", "kind":"usage",
                    "message":format!("unknown {command} option '{flag}'")});
                let actual = decoded(&output);
                // Decision X preserves already-correct usage wording. These
                // observed wrong/omitted labels are explicitly being corrected;
                // #1341 also pins exact command-local ownership diagnostics.
                let exact = matches!(
                    command.as_str(),
                    "scenarios"
                        | "ai check"
                        | "domain check"
                        | "domain expand"
                        | "db observe"
                        | "explain"
                        | "typestate"
                        | "mutate"
                        | "testgen"
                        | "html"
                        | "ledger"
                );
                sweep.check(
                    "unknown",
                    &args,
                    &output,
                    usage(&output)
                        && actual["message"]
                            .as_str()
                            .is_some_and(|message| message.contains(flag))
                        && (!exact || actual == expected),
                );
            }
        }
    }
    sweep.finish("unknown");
}

#[test]
fn every_choice_rejects_a_value_outside_its_domain_before_io() {
    let contract = contract();
    let mut sweep = Sweep::default();
    for node in nodes(&contract) {
        for action in actions(node) {
            if let Some(choices) = action["choices"].as_array() {
                assert!(!choices.contains(&json!("fsl-contract-invalid-value")));
                for flag in flags(action) {
                    let mut args = context(node, flag);
                    append(&mut args, flag, Some("fsl-contract-invalid-value"));
                    let output = run(&args);
                    sweep.check("choices", &args, &output, usage(&output));
                }
            }
        }
    }
    sweep.finish("choices");
}

#[test]
fn finite_positional_capacity_is_enforced() {
    let contract = contract();
    let mut sweep = Sweep::default();
    let mut variadic = 0;
    for node in nodes(&contract) {
        if actions(node).iter().any(|action| {
            action["positional"] == true && matches!(action["nargs"].as_str(), Some("+" | "*"))
        }) {
            // There is no excess count for unbounded actions. Assert this set stays live.
            variadic += 1;
            continue;
        }
        let mut args = context(node, "");
        args.push("fsl-contract-extra-operand".to_owned());
        let output = run(&args);
        sweep.check("positional", &args, &output, usage(&output));
    }
    assert_eq!(variadic, 5, "review the unbounded positional controls");
    sweep.finish("positional");
}

#[test]
fn metadata_help_and_stable_gate_controls_are_live() {
    let contract = contract();
    assert_eq!(decoded(&run(&["--cli-contract".to_owned()])), contract);
    for node in nodes(&contract) {
        assert!(
            actions(node)
                .iter()
                .any(|action| flags(action) == ["-h", "--help"])
        );
        for flag in ["-h", "--help"] {
            let mut args = path(node);
            args.push(flag.to_owned());
            let output = run(&args);
            assert!(output.status.success(), "{args:?}");
            assert!(output.stderr.is_empty(), "{args:?}");
            assert_eq!(
                output.stdout,
                node["help"].as_str().unwrap().as_bytes(),
                "{args:?}"
            );
        }
    }
    // Gate-off controls: no FSLC_UNSTABLE inheritance. No gate-on exemption
    // is silently invented; #1434 owns publishing the unstable CLI surface.
    for args in [
        vec!["explain", MISSING, "--names", "canonical"],
        vec!["explain", MISSING, "--lowering"],
        vec!["fmt", MISSING, "--lang", "en"],
        vec!["fmt", MISSING, "--names", "canonical"],
        vec!["lint", MISSING, "--strict-names"],
    ] {
        let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        assert!(usage(&run(&args)), "{args:?}");
    }
}

#[test]
fn allow_unwired_is_effective_and_published_for_both_testgen_paths() {
    let contract = contract();
    let mut sweep = Sweep::default();
    for (command, fixture) in [
        ("testgen", "rust/fslc/tests/fixtures/sweep_clean.fsl"),
        ("domain testgen", "examples/domain/order_async_effect.fsl"),
    ] {
        let node = nodes(&contract)
            .into_iter()
            .find(|node| label(node) == command)
            .unwrap();
        let mut args = path(node);
        args.extend([fixture, "--target", "pytest", "--depth", "1"].map(str::to_owned));
        let baseline = run(&args);
        args.push("--allow-unwired".to_owned());
        let opted = run(&args);
        let content = |output: &Output| {
            let value = decoded(output);
            value["content"].as_str().map_or_else(
                || String::from_utf8(output.stdout.clone()).unwrap(),
                str::to_owned,
            )
        };
        let before = content(&baseline);
        let after = content(&opted);
        assert!(
            baseline.status.success() && opted.status.success(),
            "{args:?}: {before} / {after}"
        );
        assert!(
            before.contains("pytest.fail(ADAPTER_NOT_WIRED, pytrace=False)"),
            "{command}: {before}"
        );
        assert!(
            !before.contains("pytest.skip('Adapter not implemented')"),
            "{command}"
        );
        assert!(
            after.contains("pytest.skip('Adapter not implemented')"),
            "{command}: {after}"
        );
        assert!(
            !after.contains("pytest.fail(ADAPTER_NOT_WIRED, pytrace=False)"),
            "{command}"
        );
        assert_ne!(
            baseline.stdout, opted.stdout,
            "{command}: complete stdout comparison"
        );
        let published = actions(node)
            .iter()
            .any(|action| flags(action).contains(&"--allow-unwired"))
            && node["help"].as_str().unwrap().contains("--allow-unwired");
        sweep.check("allow-unwired metadata", &args, &opted, published);
    }
    sweep.finish("allow-unwired metadata");
}

#[test]
fn malformed_git_ranges_and_odd_refine_operands_are_usage() {
    let mut sweep = Sweep::default();
    for range in ["bad", "..HEAD", "HEAD..", "HEAD....HEAD"] {
        let args = ["diff", "--git", range, MISSING].map(str::to_owned);
        let output = run(&args);
        sweep.check(
            "git shape",
            &args,
            &output,
            usage(&output) && decoded(&output)["message"] == "--git range must be BASE..HEAD",
        );
    }
    let args = ["refine", MISSING, MISSING, MISSING, MISSING].map(str::to_owned);
    let output = run(&args);
    sweep.check(
        "odd refine",
        &args,
        &output,
        usage(&output)
            && decoded(&output)["message"]
                == "refine chain must list (abs map) pairs after the first mapping",
    );
    // Preserve #1343's literate-gate precedence even with the same odd count.
    let args = ["refine", "missing.md", MISSING, MISSING, MISSING].map(str::to_owned);
    let output = run(&args);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        decoded(&output)["diagnostic_code"],
        "FSL-INPUT-LITERATE-UNSUPPORTED"
    );
    sweep.finish("shape");
}
