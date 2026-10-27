// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1426 acceptance through the reported CLI and public model projection.
//! Dense cases always run in killable child processes, including the public
//! function case; a base calibration cannot strand an exponential DFS forever.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const M: u64 = 9_223_372_036_854_775_807;
const Q: u64 = 6_148_914_691_236_517_205;
const INNER: usize = 30;
const TIMEOUT: Duration = Duration::from_secs(60);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn scratch(label: &str) -> PathBuf {
    let directory = root().join(".codex/tmp").join(format!(
        "issue-1426-{}-{:?}-{label}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create local scratch");
    directory
}

fn run_bounded(mut command: Command, label: &str) -> Vec<u8> {
    let directory = scratch(label);
    let stdout = directory.join("stdout.log");
    let stderr = directory.join("stderr.log");
    // Dense graph JSON exceeds pipe capacity: regular files avoid blocking a
    // completed child while the parent polls its deadline instead of reading.
    let mut child = command
        .stdout(Stdio::from(File::create(&stdout).expect("stdout log")))
        .stderr(Stdio::from(File::create(&stderr).expect("stderr log")))
        .spawn()
        .expect("spawn bounded child");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status;
        }
        if started.elapsed() >= TIMEOUT {
            child.kill().expect("kill timed-out child");
            child.wait().expect("reap timed-out child");
            panic!(
                "{label} did not finish within 60 s; killed child; elapsed={:?}; stdout={}; stderr={}",
                started.elapsed(),
                stdout.display(),
                stderr.display()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let bytes = std::fs::read(&stdout).expect("read stdout");
    let errors = std::fs::read_to_string(&stderr).expect("read stderr");
    assert!(
        status.success(),
        "{label}: produced status={status}; stdout={}; stderr={errors}",
        String::from_utf8_lossy(&bytes)
    );
    bytes
}

fn claim(id: &str, from: &str, to: &str, min: u64, max: u64) -> String {
    format!(
        "  claim {id} {from} -> {to} {{\n    version 1\n    status active\n    polarity positive\n    lag {min}..{max}\n    persists 0..0\n    basis hypothesis\n  }}\n"
    )
}

fn dense_source() -> String {
    let mut source = "causal Dense { timebase day horizon 100\n".to_owned();
    for index in 0..INNER + 2 {
        let role = if index == 0 {
            "intervention"
        } else if index == INNER + 1 {
            "outcome"
        } else {
            "mediator"
        };
        source.push_str(&format!("variable v{index:02} {{ role {role} }}\n"));
    }
    for from in 0..INNER + 1 {
        for to in from + 1..INNER + 2 {
            source.push_str(&claim(
                &format!("C{from:02}_{to:02}"),
                &format!("v{from:02}"),
                &format!("v{to:02}"),
                0,
                0,
            ));
        }
    }
    source.push_str("}\n");
    source
}

fn assert_lower_bound(graph: &Value) {
    let paths = graph["paths"]
        .as_array()
        .unwrap_or_else(|| panic!("missing paths: {graph}"));
    let cap = graph["truncation"]["paths_cap"]
        .as_u64()
        .unwrap_or_else(|| panic!("missing cap: {graph}"));
    assert_eq!(cap, 16, "produced truncation={}", graph["truncation"]);
    assert!(paths.len() <= 16, "produced paths={paths:?}");
    assert_eq!(
        graph["truncation"]["paths_truncated_exact"], false,
        "produced truncation={}",
        graph["truncation"]
    );
    let omitted = graph["truncation"]["paths_truncated"]
        .as_u64()
        .unwrap_or_else(|| panic!("missing omitted count: {graph}"));
    let true_omitted = (1_u64 << INNER) - cap;
    assert!(
        (1..=true_omitted).contains(&omitted),
        "produced omitted={omitted}; exact omitted={true_omitted}; truncation={}",
        graph["truncation"]
    );
}

/// Detector (fails on 9ad07c7a by a 60 s timeout): complete DAG with k=30 inner
/// vertices has one path for each ordered subset, hence 2^k total and exactly
/// T=2^30-16 omitted paths. A budgeted walk returns a non-exact lower bound in
/// [1,T] and at most 16 paths. Primary detector: paths_truncated_exact=false.
#[test]
fn dense_cli_walk_returns_a_lower_bound_within_sixty_seconds() {
    let directory = scratch("dense-model");
    let path = directory.join("model.fsl");
    std::fs::write(&path, dense_source()).expect("write dense model");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(["causal", "analyze"])
        .arg(&path)
        .args(["--projection", "causal_graph"])
        .current_dir(root());
    let bytes = run_bounded(command, "dense-cli");
    let graph: Value = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "JSON={error}; produced stdout={}",
            String::from_utf8_lossy(&bytes)
        )
    });
    assert_lower_bound(&graph);
}

/// Detector (fails on 9ad07c7a by a 60 s timeout): same 2^30-16 lower-bound
/// contract through causal_graph_projection on a parsed typed model. Re-exec
/// this test in a child so the unbounded base function remains killable.
#[test]
fn dense_public_model_walk_has_the_same_lower_bound_contract() {
    const CHILD: &str = "FSL_ISSUE_1426_PUBLIC_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let resolver = fsl_core::FsResolver::new(root());
        let (model, _) =
            fsl_tools::build_causal_model(&dense_source(), &resolver).expect("dense typed model");
        let graph = fsl_tools::causal_graph_projection(&model).expect("zero-lag projection");
        assert_lower_bound(&graph);
        return;
    }
    let mut command = Command::new(std::env::current_exe().expect("test binary"));
    command
        .args([
            "--exact",
            "dense_public_model_walk_has_the_same_lower_bound_contract",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .current_dir(root());
    run_bounded(command, "dense-public-model");
}

fn cli_source(source: &str, label: &str) -> Value {
    let directory = scratch(label);
    std::fs::write(directory.join("model.fsl"), source).expect("write model");
    std::fs::write(directory.join("biz.fsl"), "spec Biz { state { x: 0..3 } init { x = 0 } action enable() { requires x < 3 x = x + 1 } invariant Ok { x >= 0 } }").expect("observable binding");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args([
            "causal",
            "analyze",
            "model.fsl",
            "--projection",
            "causal_graph",
        ])
        .current_dir(directory)
        .env("FSLC_CACHE", "off");
    let bytes = run_bounded(command, label);
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!("JSON={error}; produced={}", String::from_utf8_lossy(&bytes))
    })
}

/// Control (passes before/after): three stages of three parallel two-step
/// routes yield 3^3=27 paths, so exactly 27-16=11 are omitted. The truncation
/// object has ONLY paths_cap and paths_truncated, with no non-exact marker.
#[test]
fn small_twenty_seven_path_graph_keeps_exact_count_and_path_order() {
    let mut source = "causal Small { timebase day horizon 100\nvariable s0 { role intervention }\nvariable s1 { role mediator }\nvariable s2 { role mediator }\nvariable s3 { role outcome }\n".to_owned();
    for stage in 0..3 {
        for route in 0..3 {
            let mid = format!("m{stage}{route}");
            source.push_str(&format!("variable {mid} {{ role mediator }}\n"));
            source.push_str(&claim(
                &format!("A{stage}{route}"),
                &format!("s{stage}"),
                &mid,
                0,
                0,
            ));
            source.push_str(&claim(
                &format!("B{stage}{route}"),
                &mid,
                &format!("s{}", stage + 1),
                0,
                0,
            ));
        }
    }
    source.push_str("}\n");
    let graph = cli_source(&source, "small-exact");
    let expected: Vec<_> = (0..27)
        .take(16)
        .map(|index| {
            let routes = [index / 9, (index / 3) % 3, index % 3];
            let claims: Vec<_> = routes
                .iter()
                .enumerate()
                .flat_map(|(stage, route)| {
                    [
                        format!("claim:A{stage}{route}"),
                        format!("claim:B{stage}{route}"),
                    ]
                })
                .collect();
            json!({"source":"variable:s0", "target":"variable:s3", "claims":claims,
            "polarity":"positive", "first_response":{"min":0,"max":0}})
        })
        .collect();
    assert_eq!(
        graph["paths"],
        json!(expected),
        "produced paths={}",
        graph["paths"]
    );
    assert_eq!(
        graph["truncation"],
        json!({"paths_cap":16,"paths_truncated":11}),
        "produced truncation={}",
        graph["truncation"]
    );
}

// Exact source recipes from causal_max_cli.rs and causal_numeric_cli.rs, the
// tests that produced these goldens. Compare the FULL causal_graph envelope;
// no projection fields (including truncation) are excluded or rewritten.
fn golden_source(name: &str) -> String {
    if name.starts_with("causal-") {
        let (horizon, lags) = match name {
            "causal-small-chain" => (730, [1, 1, 1]),
            "causal-u64-minus-one" => (M, [M, M, 0]),
            "causal-u64-max" => (M, [Q, Q, Q]),
            _ => panic!("unknown golden={name}"),
        };
        let mut source = format!(
            "// SPDX-License-Identifier: Apache-2.0\ncausal TimeProbe {{\n  timebase day\n  horizon {horizon}\n  variable a {{ role intervention }}\n  variable b {{ role mediator }}\n  variable c {{ role mediator }}\n  variable d {{ role outcome }}\n"
        );
        for ((id, from, to), lag) in [("AB", "a", "b"), ("BC", "b", "c"), ("CD", "c", "d")]
            .into_iter()
            .zip(lags)
        {
            source.push_str(&claim(id, from, to, lag, lag));
        }
        source.push_str("}\n");
        return source;
    }
    let model = match name {
        "max-chain" => "MaxChain",
        "max-cross-component" => "MaxCrossComponent",
        "max-reachable-scc" => "MaxReachableScc",
        "max-feedback" => "MaxFeedback",
        "max-response-window" => "MaxResponseWindow",
        _ => panic!("unknown golden={name}"),
    };
    let mut source = format!(
        "// SPDX-License-Identifier: Apache-2.0\ncausal {model} {{\n  uses biz from \"biz.fsl\"\n  timebase day\n  horizon {M}\n"
    );
    if name == "max-feedback" {
        source.push_str("  variable a { role mediator }\n  variable b { role mediator }\n  variable c { role mediator }\n");
        for (id, from, to, min) in [
            ("AB", "a", "b", 0),
            ("BC", "b", "c", 0),
            ("CA", "c", "a", 1),
        ] {
            source.push_str(&claim(id, from, to, min, Q));
        }
        source.push_str("  feedback F { claims CA, AB, BC }\n}\n");
        return source;
    }
    if name == "max-response-window" {
        source.push_str("  variable a { role intervention binds action biz.enable }\n  variable d { role outcome observes state biz.x window 0..1 }\n");
        source.push_str(&format!("  claim AD a -> d {{\n    version 1\n    status active\n    polarity positive\n    lag {M}..{M}\n    persists 0..{M}\n    basis hypothesis\n  }}\n}}\n"));
        return source;
    }
    source.push_str("  variable a { role intervention binds action biz.enable }\n  variable b { role mediator }\n  variable c { role mediator }\n  variable d { role outcome observes state biz.x }\n");
    let edges = match name {
        "max-chain" => vec![
            ("AB", "a", "b", 0, Q),
            ("BC", "b", "c", 0, Q),
            ("CD", "c", "d", 0, Q),
        ],
        "max-cross-component" => vec![
            ("AB", "a", "b", 0, M),
            ("BC", "b", "c", 0, M),
            ("CB", "c", "b", 1, 1),
            ("CD", "c", "d", 0, 0),
        ],
        "max-reachable-scc" => {
            source.push_str("  variable e { role mediator }\n");
            vec![
                ("AB", "a", "b", 0, 0),
                ("Z_BC", "b", "c", 0, Q),
                ("A_CE", "c", "e", 0, Q),
                ("B_EB", "e", "b", 1, Q),
                ("BD", "b", "d", 0, 0),
            ]
        }
        _ => panic!("unknown golden={name}"),
    };
    for (id, from, to, min, max) in edges {
        source.push_str(&claim(id, from, to, min, max));
    }
    source.push_str("}\n");
    source
}

/// Control (passes before/after): reproduce every committed #1475/#1506 golden
/// carrying truncation through its original causal_graph CLI command and compare
/// every byte of the serialized full graph envelope, not a selected field list.
/// The stored files wrap outputs with provenance; compare their graph payloads,
/// not wrapper indentation or unrelated check/timeline/review commands.
#[test]
fn all_existing_truncation_goldens_keep_exact_graph_bytes() {
    let mut compared = Vec::new();
    for family in ["issue_1475_golden", "issue_1506_golden"] {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(family);
        for entry in std::fs::read_dir(directory).expect("golden directory") {
            let path = entry.expect("golden entry").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let golden: Value =
                serde_json::from_slice(&std::fs::read(&path).expect("golden bytes"))
                    .expect("golden JSON");
            let expected = &golden["observed"]["consumers"]["causal_graph"];
            if expected.get("truncation").is_none() {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|name| name.to_str())
                .expect("golden name");
            let actual = cli_source(&golden_source(name), name);
            assert_eq!(
                serde_json::to_vec(&actual).expect("actual bytes"),
                serde_json::to_vec(expected).expect("expected bytes"),
                "golden={}; produced graph={actual}; expected graph={expected}",
                path.display()
            );
            compared.push(name.to_owned());
        }
    }
    compared.sort();
    assert_eq!(
        compared,
        [
            "causal-small-chain",
            "causal-u64-max",
            "causal-u64-minus-one",
            "max-chain",
            "max-cross-component",
            "max-feedback",
            "max-reachable-scc",
            "max-response-window"
        ],
        "produced compared goldens={compared:?}"
    );
}
