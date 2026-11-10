// SPDX-License-Identifier: Apache-2.0

//! #1547: bindings exist only in a match's true continuation.

#[path = "support/pattern_continuation.rs"]
mod forms;
#[path = "support/pattern_reuse.rs"]
mod reuse;
#[path = "support/compose_scratch.rs"]
#[allow(dead_code)]
mod scratch;

use forms::Role;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn observe(dir: &Path, file: &str, consumer: &str) -> (Value, i32) {
    let binary = std::env::var_os("FSL_MATRIX_BINARY")
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_fslc")), PathBuf::from);
    let mut args = match consumer {
        "check" => vec!["check", file],
        "kernel" => vec!["kernel", file, "--kernel-version", "2"],
        _ => vec!["verify", file, "--depth", "3", "--no-cache"],
    };
    if !matches!(consumer, "check" | "kernel" | "default") {
        args.extend(["--engine", consumer]);
    }
    let output = Command::new(std::fs::canonicalize(binary).unwrap())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("native fslc");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    (
        serde_json::from_slice(&output.stdout).expect("native JSON"),
        output.status.code().unwrap(),
    )
}

fn point(source: &str, marker: &str) -> Value {
    let before = &source[..source.find(marker).expect("authored clause/arm")];
    json!({"line":before.bytes().filter(|b| *b == b'\n').count()+1,"column":before.rsplit('\n').next().unwrap().chars().count()+1})
}

fn refusal_location(case: &forms::Case, source: &str, composed: bool) -> Value {
    match case.id {
        "requires-or-ensures" | "requires-or-later-requires" => {
            json!({"line":5,"column":3})
        }
        "compose-sync-alias-or" => json!({"line":if composed { 4 } else { 5 },"column":3}),
        _ => point(source, case.marker),
    }
}

fn semantic_error(actual: &(Value, i32)) -> bool {
    actual.1 == 2 && actual.0["result"] == "error" && actual.0["kind"] == "semantics"
}

fn exact_error(actual: &(Value, i32), message: &str, location: Option<&Value>) -> bool {
    semantic_error(actual) && actual.0["message"] == message && actual.0.get("loc") == location
}

const LOCAL_COLLISION_REASON: &str = "a name already in scope (an action parameter, quantifier binder, let, or earlier pattern binding)";

fn collision_message(reason: &str, line: u64, column: u64) -> String {
    format!(
        "invalid model expression: pattern binding 'v' in 'is some(v)' shadows {reason} of the same name; rename the pattern variable at {line}:{column}"
    )
}

fn preserved_error(id: &str) -> Option<Value> {
    let (line, column) = match id {
        "type-if-else" => (6, 50),
        "type-if-then" => (6, 40),
        "type-not-outside"
        | "type-quantifier-escape"
        | "type-aggregate-escape"
        | "collision-state"
        | "collision-quantifier"
        | "collision-earlier-pattern" => (6, 3),
        "collision-constant" | "collision-enum" => (7, 3),
        "collision-parameter" | "collision-let" => (5, 3),
        _ => return None,
    };
    let reason = match id {
        "collision-state" => "a state variable",
        "collision-constant" => "a constant",
        "collision-enum" => "an enum member",
        _ => LOCAL_COLLISION_REASON,
    };
    let message = if id.starts_with("collision-") {
        collision_message(reason, line, column)
    } else {
        format!(
            "invalid model expression: public Kernel cannot type identifier 'v' at {line}:{column}"
        )
    };
    Some(json!({"message":message,"loc":{"line":line,"column":column}}))
}

fn paths(case: &forms::Case, composed: bool) -> Vec<&str> {
    case.paths
        .split(',')
        .filter(|path| match *path {
            "component-rewrite" | "alias-resolution" => composed,
            "compose-names" => composed && case.id == "compose-sync-alias-or",
            _ => true,
        })
        .collect()
}

fn fixture(case: &forms::Case) -> PathBuf {
    let files = [
        ("d.fsl", case.source.as_str()),
        ("a.fsl", case.component.as_str()),
        ("c.fsl", case.compose.as_str()),
    ];
    fixture_files(case.id, &files)
}

fn fixture_files(id: &str, files: &[(&str, &str)]) -> PathBuf {
    if let Some(root) = std::env::var_os("FSL_MATRIX_FIXTURES") {
        // A pre-materialized, read-only tree gives repeated public Kernel runs
        // identical source/provenance identities. Never trust stale fixtures.
        let dir = PathBuf::from(root).join(id);
        for &(name, source) in files {
            assert_eq!(
                std::fs::read_to_string(dir.join(name)).unwrap(),
                source,
                "{id} {name}"
            );
        }
        dir
    } else {
        scratch::scratch("issue-1547-cli-case", files)
    }
}

fn control(actual: &(Value, i32), role: Role, consumer: &str, composed: bool) -> bool {
    if consumer == "kernel" {
        return actual.1 == 0 && actual.0["schema_version"] == "2.0.0";
    }
    if consumer == "check" {
        return actual.1 == 0 && actual.0["result"] == "ok";
    }
    let (exit, result) = match role {
        Role::Violated => (1, "violated"),
        Role::Guard if consumer == "induction" => (1, "unknown_cti"),
        _ if matches!(consumer, "bmc" | "default") => (0, "verified"),
        _ => (0, "proved"),
    };
    // These are independently specified verdicts, not equality of engines'
    // different assurance/depth/cost envelopes. Raw packets remain in evidence.
    actual.1 == exit
        && actual.0["result"] == result
        && actual.0.get("kind").is_none()
        && actual.0.get("message").is_none()
        && (!matches!(role, Role::Violated)
            || (actual.0["violation_kind"] == "invariant"
                && actual.0["invariant"] == if composed { "part.P" } else { "P" }))
}

#[test]
#[allow(clippy::too_many_lines)] // Collect the entire position/path table before the intended red assertion.
fn cli_pattern_positions_and_consumer_paths() {
    let cases = forms::cases();
    assert_eq!(cases.len(), 45);
    let report = scratch::scratch("issue-1547-cli-report", &[]);
    let mut inputs = Vec::new();
    let mut observations = Vec::new();
    let mut groups: BTreeMap<String, [usize; 2]> = BTreeMap::new();
    let mut missing_refusals = 0;
    let mut refusal_failures = 0;
    let mut control_failures = 0;
    let mut preservation_failures = 0;
    let mut inapplicable = Vec::new();
    for case in cases {
        let dir = fixture(&case);
        inputs.push(json!({"id":case.id,"paths":case.paths,"role":format!("{:?}",case.role),"source":case.source,"component":case.component,"compose":case.compose,"directory":dir}));
        for (route, file, composed) in [("direct", "d.fsl", false), ("compose", "c.fsl", true)] {
            let source = if composed && case.id == "compose-sync-alias-or" {
                &case.compose
            } else {
                &case.source
            };
            let preserved = preserved_error(case.id);
            let expected = preserved.as_ref().map_or_else(
                || refusal_location(&case, source, composed),
                |error| error["loc"].clone(),
            );
            let paths = paths(&case, composed);
            let class = if preserved.is_some() {
                "existing-refusal"
            } else if matches!(case.role, Role::Refuse) {
                "new-scope-refusal"
            } else {
                "valid-control"
            };
            let checked = observe(&dir, file, "check");
            let rejected = matches!(case.role, Role::Refuse | Role::Collision);
            missing_refusals += usize::from(rejected && checked.1 == 0);
            for consumer in ["check", "kernel", "bmc", "induction", "explicit", "default"] {
                if composed && consumer == "kernel" {
                    let reason = "public Kernel v2 cannot preserve component source filenames after compose lowering";
                    inapplicable.push(
                        json!({"id":case.id,"route":route,"consumer":consumer,"reason":reason}),
                    );
                    // A live preservation control for the exact publication
                    // limitation; do not mislabel it an unbound-v detector.
                    if case.id == "control-and" {
                        let actual = observe(&dir, file, consumer);
                        let ok = actual
                            == (
                                json!({"fsl":"1.0","result":"error","kind":"semantics","message":reason}),
                                2,
                            );
                        control_failures += usize::from(!ok);
                        observations.push(json!({"id":case.id,"class":"unsupported-compose-kernel","paths":["public-kernel"],"route":route,"consumer":consumer,"ok":ok,"output":actual.0,"exit":actual.1}));
                    }
                    continue;
                }
                let actual = if consumer == "check" {
                    checked.clone()
                } else {
                    observe(&dir, file, consumer)
                };
                let mut ok = if let Some(preserved) = &preserved {
                    exact_error(
                        &actual,
                        preserved["message"].as_str().unwrap(),
                        preserved.get("loc"),
                    )
                } else if rejected {
                    exact_error(
                        &actual,
                        &format!(
                            "invalid model expression: public Kernel cannot type identifier 'v' at {}:{}",
                            expected["line"], expected["column"]
                        ),
                        Some(&expected),
                    )
                } else {
                    control(&actual, case.role, consumer, composed)
                };
                let agreement =
                    if rejected && checked.1 == 2 && !matches!(consumer, "check" | "kernel") {
                        let same = actual == checked;
                        ok = ok && same;
                        Some(same)
                    } else {
                        None
                    };
                if !ok {
                    if preserved.is_some() {
                        preservation_failures += 1;
                    } else if rejected {
                        refusal_failures += 1;
                    } else {
                        control_failures += 1;
                    }
                }
                for path in &paths {
                    let count = groups
                        .entry(format!("{path}|{}|{route}|{consumer}", case.id))
                        .or_default();
                    count[0] += 1;
                    count[1] += usize::from(!ok);
                }
                observations.push(json!({"id":case.id,"paths":paths,"class":class,"role":format!("{:?}",case.role),"route":route,"consumer":consumer,"expected_location":expected,"ok":ok,"check_error_agreement":agreement,"output":actual.0,"exit":actual.1}));
            }
        }
    }
    std::fs::write(report.join("report.json"),serde_json::to_vec(&json!({"inputs":inputs,"observations":observations,"groups":groups,"inapplicable":inapplicable,"missing_refusals":missing_refusals,"refusal_failures":refusal_failures,"preservation_failures":preservation_failures,"control_failures":control_failures})).unwrap()).unwrap();
    eprintln!(
        "PATTERN_SCOPE_RESULT cases=45 N=45 observations={} missing_refusals={missing_refusals} refusal_failures={refusal_failures} preservation_failures={preservation_failures} control_failures={control_failures} report={}",
        observations.len(),
        report.display()
    );
    assert_eq!(
        control_failures, 0,
        "and/implies/requires/def control calibration changed"
    );
    assert_eq!(
        preservation_failures, 0,
        "existing refusal message/location changed"
    );
    assert_eq!(
        refusal_failures, 0,
        "out-of-scope binding escaped source validation"
    );
}

#[test]
#[allow(clippy::too_many_lines)] // Retain both routes and all error packets before asserting.
fn def_call_scope_after_or_preserves_free_names() {
    let report = scratch::scratch("issue-1547-def-call-report", &[]);
    let mut rows = Vec::new();
    let mut inputs = Vec::new();
    let mut detectors = 0;
    let mut controls = 0;
    for case in forms::def_call_cases() {
        let dir = fixture(&case);
        let reviewed = case.id == "def-call-implies-free-name";
        let scope_error = case.id != "def-call-and-capture-control";
        let clause = point(&case.source, case.marker);
        if scope_error {
            // Both routes retain exit 2 / semantics (exact_error). The old direct
            // capture had no location; both routes now identify this clause.
            assert_eq!(clause, json!({"line":if reviewed {6} else {7},"column":3}));
        }
        let call = point(&case.source, if reviewed { "k()) }" } else { "p() }" });
        inputs.push(json!({"id":case.id,"source":case.source,"component":case.component,"compose":case.compose,"directory":dir,"direct_only":reviewed,"review_equivalent":if reviewed {Some("def/d06.fsl")} else {None}}));
        for (route, file, composed) in [("direct", "d.fsl", false), ("compose", "c.fsl", true)] {
            if reviewed && composed {
                continue;
            }
            let capture =
                "predicate 'p' call would capture variable 'v'; rename the binder at the call site";
            let (message, location) = if scope_error {
                (
                    format!(
                        "invalid model expression: public Kernel cannot type identifier 'v' at {}:{}",
                        clause["line"], clause["column"]
                    ),
                    Some(clause.clone()),
                )
            } else if composed {
                (
                    format!(
                        "component \"a.fsl\" failed to lower ({capture} at a.fsl:{}:{}) at c.fsl:2:3",
                        call["line"], call["column"]
                    ),
                    Some(json!({"line":2,"column":3})),
                )
            } else {
                // The existing direct capture diagnostic locates the call in
                // its full message and has NO structured loc (not loc:null).
                (
                    format!("{capture} at {}:{}", call["line"], call["column"]),
                    None,
                )
            };
            let checked = observe(&dir, file, "check");
            for consumer in ["check", "bmc", "induction", "explicit", "default"] {
                let actual = if consumer == "check" {
                    checked.clone()
                } else {
                    observe(&dir, file, consumer)
                };
                let agreement = actual == checked;
                let ok = exact_error(&actual, &message, location.as_ref()) && agreement;
                if scope_error {
                    detectors += usize::from(!ok);
                } else {
                    controls += usize::from(!ok);
                }
                rows.push(json!({"id":case.id,"path":case.paths,"route":route,"consumer":consumer,"class":if scope_error {"caller-scope-detector"} else {"capture-control"},"expected_message":message,"expected_location":location,"structured_location_present":location.is_some(),"call_location":call,"ok":ok,"check_error_agreement":agreement,"output":actual.0,"exit":actual.1}));
            }
        }
    }
    std::fs::write(report.join("report.json"),serde_json::to_vec(&json!({"inputs":inputs,"observations":rows,"detector_failures":detectors,"control_failures":controls})).unwrap()).unwrap();
    eprintln!(
        "DEF_CALL_SCOPE_RESULT cells=3 routes=5 observations={} detector_failures={detectors} control_failures={controls} report={}",
        rows.len(),
        report.display()
    );
    assert_eq!(
        controls, 0,
        "conjunction must retain the existing call-site capture diagnostic"
    );
    assert_eq!(
        detectors, 0,
        "a disjunctive match must not populate the def call's locals"
    );
}

fn baseline_collision_packet(location: &Value) -> Value {
    // Keep the entire baseline envelope. Version values describe this build
    // and linked solver, never the target's observed verdict or diagnostics.
    json!({
        "fsl":"1.0", "result":"error", "kind":"semantics",
        "message":collision_message(LOCAL_COLLISION_REASON,location["line"].as_u64().unwrap(),location["column"].as_u64().unwrap()),
        "loc":location,
        "versions":{
            "verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},
            "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
            "solver":{"name":"z3","backend":"native-z3","version":fsl_solver_z3::version()}
        }
    })
}

#[test]
#[allow(clippy::too_many_lines)] // Record every regression and unchanged route before the intended red assertion.
fn sibling_pattern_names_preserve_collisions_and_public_spelling() {
    let cases = reuse::cases();
    assert_eq!(cases.len(), 15);
    let report = scratch::scratch("issue-1547-reuse-report", &[]);
    let mut inputs = Vec::new();
    let mut rows = Vec::new();
    let mut collision_failures = 0;
    let mut control_failures = 0;
    let mut missing_collisions = 0;
    let mut changed_diagnostics = 0;
    for case in cases {
        let dir = fixture(&case);
        let collision = matches!(case.role, Role::Collision);
        let expected = baseline_collision_packet(&point(&case.source, case.marker));
        inputs.push(json!({"id":case.id,"source":case.source,"component":case.component,"compose":case.compose,"directory":dir}));
        for (route, file, composed) in [("direct", "d.fsl", false), ("compose", "c.fsl", true)] {
            let checked = observe(&dir, file, "check");
            if collision {
                missing_collisions += usize::from(checked.1 == 0);
                changed_diagnostics += usize::from(checked.1 == 2 && checked.0 != expected);
            }
            for consumer in ["check", "bmc", "induction", "explicit", "default"] {
                let actual = if consumer == "check" {
                    checked.clone()
                } else {
                    observe(&dir, file, consumer)
                };
                let ok = if collision {
                    actual == (expected.clone(), 2)
                } else {
                    control(&actual, case.role, consumer, composed)
                };
                if collision {
                    collision_failures += usize::from(!ok);
                } else {
                    control_failures += usize::from(!ok);
                }
                rows.push(json!({"id":case.id,"route":route,"consumer":consumer,"class":if collision {"collision-preservation"} else {"renamed-control"},"expected_packet":if collision {Some(&expected)} else {None},"ok":ok,"output":actual.0,"exit":actual.1}));
            }
        }
    }
    let dir = fixture_files("h1", reuse::RENAMER_FILES);
    inputs.push(json!({"id":"h1","files":reuse::RENAMER_FILES,"directory":dir}));
    let mut renamer_failures = 0;
    for consumer in ["check", "bmc", "induction", "explicit", "default"] {
        let actual = observe(&dir, "h1.fsl", consumer);
        let ok = semantic_error(&actual)
            && actual.0["message"].as_str().is_some_and(|message| {
                message.contains("'v'") && !message.contains("v_substitution")
            });
        renamer_failures += usize::from(!ok);
        rows.push(json!({"id":"h1","route":"compose","consumer":consumer,"class":"two-source-renamer","ok":ok,"output":actual.0,"exit":actual.1}));
    }
    std::fs::write(report.join("report.json"),serde_json::to_vec(&json!({"inputs":inputs,"observations":rows,"collision_failures":collision_failures,"control_failures":control_failures,"missing_collisions":missing_collisions,"changed_diagnostics":changed_diagnostics,"renamer_failures":renamer_failures,"inapplicable":[{"id":"h1","route":"direct","reason":"Two synchronized component sources are required to exercise Renamer; no direct-spec adapter represents that path."}]})).unwrap()).unwrap();
    eprintln!(
        "PATTERN_REUSE_RESULT cells=16 observations={} missing_collisions={missing_collisions} changed_diagnostics={changed_diagnostics} collision_failures={collision_failures} control_failures={control_failures} renamer_failures={renamer_failures} report={}",
        rows.len(),
        report.display()
    );
    assert_eq!(
        control_failures, 0,
        "distinct pattern names must keep the calibrated verdicts"
    );
    assert_eq!(
        collision_failures, 0,
        "true-continuation visibility must not weaken evaluator-name collisions"
    );
    assert_eq!(
        renamer_failures, 0,
        "two-source diagnostics must name user v, not a substitution temporary"
    );
}

fn priority_observations(
    cases: impl IntoIterator<Item = (forms::Case, Value)>,
) -> (Vec<Value>, Vec<Value>, usize) {
    let mut inputs = Vec::new();
    let mut rows = Vec::new();
    let mut failures = 0;
    for (case, location) in cases {
        let dir = fixture(&case);
        let renamed = matches!(case.role, Role::Collision);
        let mut expected = baseline_collision_packet(&location);
        if !renamed {
            expected["message"] = json!(format!(
                "invalid model expression: public Kernel cannot type identifier 'v' at {}:{}",
                location["line"], location["column"]
            ));
        }
        inputs.push(json!({"id":case.id,"source":case.source,"component":case.component,"compose":case.compose,"counterfactual":renamed,"directory":dir}));
        for (route, file) in [("direct", "d.fsl"), ("compose", "c.fsl")] {
            for consumer in ["check", "bmc", "induction", "explicit", "default"] {
                let actual = observe(&dir, file, consumer);
                let ok = actual == (expected.clone(), 2);
                failures += usize::from(!ok);
                rows.push(json!({"id":case.id,"route":route,"consumer":consumer,"class":if renamed {"collision-counterfactual"} else {"earlier-invalid-read"},"ok":ok,"expected_packet":expected,"output":actual.0,"exit":actual.1}));
            }
        }
    }
    (inputs, rows, failures)
}

fn finish_priority_report(
    report: &Path,
    label: &str,
    cells: usize,
    inputs: &[Value],
    rows: &[Value],
    failures: usize,
) {
    std::fs::write(
        report.join("report.json"),
        serde_json::to_vec(&json!({"inputs":inputs,"observations":rows,"failures":failures}))
            .unwrap(),
    )
    .unwrap();
    eprintln!(
        "{label} cells={cells} observations={} failures={failures} report={}",
        rows.len(),
        report.display()
    );
    assert_eq!(
        failures, 0,
        "earlier invalid reads and collision-return controls"
    );
}

#[test]
fn earlier_invalid_read_has_priority_and_renaming_restores_collision() {
    let report = scratch::scratch("issue-1547-priority-report", &[]);
    let (mut inputs, mut rows, mut failures) =
        priority_observations(reuse::priority_cases().into_iter().map(|case| {
            let location = point(&case.source, case.marker);
            (case, location)
        }));
    // Two receiver scopes force a fresh pattern name. The and result must carry
    // that rename to ensures, independently of the RHS rename inside the guard.
    let files = reuse::renamer_export_files();
    let borrowed = files
        .iter()
        .map(|(name, source)| (*name, source.as_str()))
        .collect::<Vec<_>>();
    let dir = fixture_files("renamer-export-and-control", &borrowed);
    let actual = observe(&dir, "h1.fsl", "check");
    let ok = actual.1 == 0 && actual.0["result"] == "ok";
    failures += usize::from(!ok);
    inputs.push(json!({"id":"renamer-export-and-control","files":files,"directory":dir}));
    rows.push(json!({"id":"renamer-export-and-control","route":"compose","consumer":"check","class":"export-admission-control","ok":ok,"expected_packet":{"exit":0,"result":"ok"},"output":actual.0,"exit":actual.1}));
    finish_priority_report(
        &report,
        "PATTERN_PRIORITY_RESULT",
        5,
        &inputs,
        &rows,
        failures,
    );
}

#[test]
fn conditional_arm_errors_precede_condition_freshness_errors() {
    let report = scratch::scratch("issue-1547-conditional-priority-report", &[]);
    let (inputs, rows, failures) =
        priority_observations(reuse::conditional_priority_cases().into_iter().map(
            |(case, column)| {
                assert_eq!(
                    point(&case.source, case.marker),
                    json!({"line":6,"column":12})
                );
                (case, json!({"line":6,"column":column}))
            },
        ));
    finish_priority_report(
        &report,
        "PATTERN_CONDITIONAL_PRIORITY_RESULT",
        6,
        &inputs,
        &rows,
        failures,
    );
}

fn probe(
    rows: &mut Vec<Value>,
    id: &str,
    path: &str,
    class: &str,
    expected: Value,
    actual: Value,
    ok: bool,
) -> usize {
    let mut row = json!({"id":id,"path":path,"class":class,"ok":ok});
    row["expected"] = expected;
    row["actual"] = actual;
    rows.push(row);
    usize::from(!ok)
}

fn built(source: &str) -> fsl_core::KernelModel {
    fsl_core::build_model(fsl_core::parse_direct_kernel_spec(source).unwrap()).unwrap()
}

struct Source(String);
impl fsl_core::FileResolver for Source {
    fn read(&self, path: &str) -> Result<String, fsl_core::CoreError> {
        assert_eq!(path, "a.fsl");
        Ok(self.0.clone())
    }
}

#[test]
#[allow(clippy::too_many_lines)] // Public seams bypass CLI admission; preserve all observations before failing.
fn public_scope_entry_points() {
    use fsl_core::{ActionGuard, KernelStatement, TypeRef};
    let report = scratch::scratch("issue-1547-public-report", &[]);
    let model = built(&forms::scalar("  action stay() {}\n", "true"));
    let span = model.invariants[0].span;
    let expected_loc = json!({"line":span.start.line,"column":span.start.column});
    let mut rows = Vec::new();
    let mut detectors = 0;
    let mut controls = 0;
    for (id, text, refused) in [
        ("public-kernel-or", "(o is some(v)) or v == 1", true),
        (
            "public-kernel-comparison-escape",
            "((o is some(v)) == false) and v == 1",
            true,
        ),
        (
            "public-kernel-implies-left-or",
            "((o is some(v)) or x == 0) => v == 1",
            true,
        ),
        (
            "public-kernel-and-control",
            "(o is some(v)) and v == 1",
            false,
        ),
        (
            "public-kernel-implies-control",
            "(o is some(v)) => v == 1",
            false,
        ),
    ] {
        let expr = fsl_syntax::parse_expr(text).unwrap();
        // This entry still validates via typecheck internally. It bypasses CLI
        // admission, but is NOT an isolated proof about the private serializer.
        let actual = match fsl_core::public_kernel_expression(
            &expr,
            &model,
            "probe.fsl",
            span,
            Some(&TypeRef::Bool),
        ) {
            Ok(value) => json!({"value":value}),
            Err(error) => {
                json!({"message":error.message,"loc":error.span.map(|s|json!({"line":s.start.line,"column":s.start.column}))})
            }
        };
        let ok = if refused {
            actual["message"] == "public Kernel cannot type identifier 'v'"
                && actual["loc"] == expected_loc
        } else {
            actual.get("value").is_some()
        };
        let failed = probe(
            &mut rows,
            id,
            "public-kernel/typecheck-coupled",
            if refused { "detector" } else { "control" },
            json!({"refused":refused,"loc":expected_loc}),
            actual,
            ok,
        );
        if refused {
            detectors += failed;
        } else {
            controls += failed;
        }
    }

    // The model has a global v solely to observe the free-name walker. These
    // expressions deliberately bypass freshness admission; this is not a claim
    // that a source-level pattern/state collision should be accepted.
    let names = built(&forms::spec(
        "o: Option<Int>, v: Int",
        "o = none v = 0",
        "  action stay() {}\n",
        "true",
    ));
    for (id, text, free) in [
        ("free-name-or", "(o is some(v)) or v == 1", true),
        (
            "free-name-implies-escape",
            "((o is some(v)) => true) and v == 1",
            true,
        ),
        ("free-name-and-control", "(o is some(v)) and v == 1", false),
        (
            "free-name-implies-control",
            "(o is some(v)) => v == 1",
            false,
        ),
    ] {
        let expr = fsl_syntax::parse_expr(text).unwrap();
        let actual = json!(fsl_core::state_references(
            &expr,
            &names,
            std::iter::empty::<&str>()
        ));
        let expected = if free {
            json!(["o", "v"])
        } else {
            json!(["o"])
        };
        let ok = actual == expected;
        let failed = probe(
            &mut rows,
            id,
            "free-name-walk-shared-by-def-and-compose",
            if free { "detector" } else { "control" },
            expected,
            actual,
            ok,
        );
        if free {
            detectors += failed;
        } else {
            controls += failed;
        }
    }

    let map = built(&forms::spec(
        "m: Map<K, Int>, y: Int",
        "forall k: K { m[k] = 0 } y = 0",
        "  action a() { y = 0 }\n",
        "true",
    ));
    for (id, guard, body, count) in [
        (
            "partial-operation-or",
            "(some(m) is some(v)) or v[0] == 1",
            false,
            1,
        ),
        (
            "partial-operation-and-control",
            "(some(m) is some(v)) and v[0] == 1",
            false,
            0,
        ),
        (
            "partial-operation-implies-control",
            "(some(m) is some(v)) => v[0] == 1",
            false,
            0,
        ),
        (
            "partial-requires-export-or",
            "(some(m) is some(v)) or true",
            true,
            1,
        ),
        (
            "partial-requires-export-and-control",
            "(some(m) is some(v)) and true",
            true,
            0,
        ),
    ] {
        let mut action = map.actions[0].clone();
        action.guards = vec![ActionGuard::Requires(
            fsl_syntax::parse_expr(guard).unwrap(),
        )];
        action.require_spans = vec![action.span];
        if body {
            let KernelStatement::Assign { value, .. } = &mut action.statements[0] else {
                panic!("authored assignment")
            };
            *value = fsl_syntax::parse_expr("v[0]").unwrap();
        } else {
            action.statements.clear();
        }
        // A typed Map read is total; an uninferable collection remains an Index
        // candidate. Wrongly exporting v suppresses this candidate on base.
        let sites = fsl_core::action_partial_operations(&map, &action);
        let actual = json!(
            sites
                .iter()
                .map(|site| format!("{:?}", site.operation))
                .collect::<Vec<_>>()
        );
        let expected = if count == 1 {
            json!(["Index"])
        } else {
            json!([])
        };
        let ok = actual == expected;
        let detector = count == 1;
        let failed = probe(
            &mut rows,
            id,
            "partial-operation/requires-export",
            if detector { "detector" } else { "control" },
            expected,
            actual,
            ok,
        );
        if detector {
            detectors += failed;
        } else {
            controls += failed;
        }
    }

    for (id, argument, reject) in [
        (
            "def-expansion-or-argument-capture",
            "(o is some(v)) or v == 1",
            true,
        ),
        ("def-expansion-free-argument-control", "x == 0", false),
    ] {
        let source = forms::scalar(
            "  def p(z: Bool) = forall v in 0..1 { z }\n  action stay() {}\n",
            &format!("p({argument})"),
        );
        let result = fsl_core::parse_direct_kernel_spec(&source);
        let ok = if reject {
            result
                .as_ref()
                .is_err_and(|e| e.message.contains("capture") && e.message.contains('v'))
        } else {
            result.is_ok()
        };
        let actual = match result {
            Ok(kernel) => kernel.kernel_ast_v1(),
            Err(error) => json!({"error":format!("{error:?}")}),
        };
        let failed = probe(
            &mut rows,
            id,
            "def-expansion/free-name",
            if reject { "detector" } else { "control" },
            json!({"capture_refused":reject}),
            actual,
            ok,
        );
        if reject {
            detectors += failed;
        } else {
            controls += failed;
        }
    }

    let component = "spec A {\n struct R { n: Int }\n state { r: R }\n init { r = R { n: 1 } }\n action stay() {}\n}\n";
    let compose = "compose C {\n use A as v from \"a.fsl\"\n invariant P { (some(v.r) is some(v)) and v.n == 1 }\n}\n";
    let kernel = fsl_core::parse_kernel_source(compose, &Source(component.into()));
    let expected = fsl_syntax::parse_expr("(some(v__r) is some(v)) and v.n == 1").unwrap();
    let ok = kernel.as_ref().is_ok_and(|kernel|kernel.syntax().items.iter().any(|item| matches!(item,fsl_syntax::SpecItem::Invariant {name,expr,..} if name == "P" && expr.as_ref() == &expected)));
    let actual = match kernel {
        Ok(kernel) => kernel.kernel_ast_v1(),
        Err(error) => json!({"error":format!("{error:?}")}),
    };
    controls += probe(
        &mut rows,
        "compose-alias-and-payload-control",
        "compose-alias-resolution",
        "control",
        expected.kernel_ast_v1(),
        actual,
        ok,
    );

    let collision = forms::cases()
        .into_iter()
        .find(|c| c.id == "collision-earlier-pattern")
        .unwrap();
    let result = fsl_core::parse_kernel_source(&collision.compose, &Source(collision.component));
    let ok = result.as_ref().is_err_and(|e| {
        e.message.contains("pattern binding 'v'")
            && e.message.contains("rename the pattern variable")
    });
    let actual = match result {
        Ok(kernel) => kernel.kernel_ast_v1(),
        Err(error) => json!({"error":format!("{error:?}")}),
    };
    controls += probe(
        &mut rows,
        "component-rewrite-and-collision-control",
        "compose-component-rewrite",
        "control",
        json!({"collision_refused":true}),
        actual,
        ok,
    );
    std::fs::write(
        report.join("report.json"),
        serde_json::to_vec(
            &json!({"observations":rows,"detector_failures":detectors,"control_failures":controls}),
        )
        .unwrap(),
    )
    .unwrap();
    eprintln!(
        "PATTERN_PUBLIC_RESULT probes={} detector_failures={detectors} control_failures={controls} report={}",
        rows.len(),
        report.display()
    );
    assert_eq!(controls, 0, "public-entry preservation control changed");
    assert_eq!(
        detectors, 0,
        "public walker exported an out-of-scope binding"
    );
}
