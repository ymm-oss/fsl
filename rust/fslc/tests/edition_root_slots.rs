// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_cases.rs"]
mod cases;

use cases::{Results, Scratch, value};
use serde_json::Value;

fn run(scratch: &Scratch, args: &[String], gate: bool) -> std::process::Output {
    cases::invoke(env!("CARGO_BIN_EXE_fslc"), args, &scratch.0, gate)
}

#[test]
fn explicit_refine_diff_replay_and_batch_slots_are_admitted_individually() {
    let mut results = Results::default();
    for (template, slots) in [
        (
            "refine impl.fsl abs.fsl map.fsl --depth 2",
            vec!["impl.fsl", "abs.fsl", "map.fsl"],
        ),
        (
            "refine impl.fsl abs.fsl map.fsl abs2.fsl map2.fsl --depth 2",
            vec!["impl.fsl", "abs.fsl", "map.fsl", "abs2.fsl", "map2.fsl"],
        ),
        (
            "diff impl.fsl abs.fsl --depth 2",
            vec!["impl.fsl", "abs.fsl"],
        ),
        (
            "diff impl.fsl abs.fsl --mapping map.fsl --depth 2",
            vec!["impl.fsl", "abs.fsl", "map.fsl"],
        ),
        (
            "replay impl.fsl --from-log log.jsonl --mapping map.fsl",
            vec!["impl.fsl", "map.fsl"],
        ),
        ("analyze impl.fsl abs.fsl", vec!["impl.fsl", "abs.fsl"]),
    ] {
        for slot in slots {
            let scratch = Scratch::new();
            for name in ["impl.fsl", "abs.fsl", "abs2.fsl"] {
                scratch.write(name, &format!("fsl 5\n{}", cases::KERNEL));
            }
            let mapping =
                "refinement R { impl S abs S map done = done action finish() -> finish() }\n";
            for name in ["map.fsl", "map2.fsl"] {
                scratch.write(name, &format!("fsl 5\n{mapping}"));
            }
            scratch.write(
                "log.jsonl",
                "{\"action\":\"finish\",\"params\":{},\"state\":{\"done\":true}}\n",
            );
            let args = cases::split(template);
            let canonical = run(&scratch, &args, true);
            let canonical_value = value(&canonical);
            // The mapped log is a real conforming finish transition, not a
            // missing file or merely syntactically valid invalid record.
            results.check(
                &format!("preserved/{template}/{slot}"),
                (&canonical.status.code(), &canonical_value),
                "usable canonical roots and valid sidecars",
                canonical.status.code() == Some(0) && canonical_value["result"] != "error",
            );
            let rejected = if slot.starts_with("map") {
                cases::rejected_domain()
            } else {
                cases::rejected_metadata()
            };
            scratch.write(slot, &rejected);
            let wanted = cases::refusal(
                env!("CARGO_BIN_EXE_fslc"),
                &scratch.0,
                std::path::Path::new(slot),
            );
            let output = run(&scratch, &args, true);
            results.error_envelope(&format!("slot/{template}/{slot}"), &output, &wanted, true);
        }
    }
    results.finish();
}

#[test]
fn manifest_layers_and_mapping_refuse_before_the_implementation_marker() {
    let mut results = Results::default();
    for layer in ["business", "requirements", "design", "mapping"] {
        for (jobs, keep_going, checks_only) in [
            (1, false, false),
            (3, false, false),
            (3, true, false),
            (1, false, true),
            (3, true, true),
        ] {
            let scratch = Scratch::new();
            for name in ["business", "requirements", "design"] {
                scratch.write(&format!("{name}.fsl"), &format!("fsl 5\n{}", cases::KERNEL));
            }
            scratch.write("mapping.fsl", "fsl 5\nrefinement R { impl S abs S map done = done action finish() -> finish() }\n");
            let rejected = if layer == "mapping" {
                cases::rejected_domain()
            } else {
                cases::rejected_metadata()
            };
            let manifest = "[business]\nfile = \"business.fsl\"\ndepth = 2\n[requirements]\nfile = \"requirements.fsl\"\ndepth = 2\n[design]\nfile = \"design.fsl\"\ndepth = 2\nrefine_against = \"requirements\"\nmapping = \"mapping.fsl\"\n[impl]\ncommand = \"touch marker\"\nevidence = \"exit_code\"\n";
            let manifest = if checks_only {
                manifest.replace("depth = 2\n", "")
            } else {
                manifest.to_owned()
            };
            let manifest_path = scratch.write(
                "project.toml",
                &manifest.replace("touch marker", "touch canonical-marker"),
            );
            let mut args = vec![
                "chain".to_owned(),
                manifest_path.to_str().unwrap().to_owned(),
                "--jobs".into(),
                jobs.to_string(),
            ];
            if keep_going {
                args.push("--keep-going".into());
            }
            let canonical = run(&scratch, &args, true);
            results.check(
                &format!("preserved/chain/{layer}/{jobs}/{keep_going}/{checks_only}"),
                canonical.status.code(),
                "0 and implementation marker",
                canonical.status.code() == Some(0) && scratch.0.join("canonical-marker").exists(),
            );
            let analysis_args = [
                "analyze".into(),
                manifest_path.to_str().unwrap().into(),
                "--projection".into(),
                "traceability_graph".into(),
            ];
            if jobs == 1 && !checks_only {
                let canonical = run(&scratch, &analysis_args, true);
                results.check(
                    &format!("preserved/project/{layer}"),
                    canonical.status.code(),
                    0,
                    canonical.status.code() == Some(0),
                );
            }
            let path = scratch.write(&format!("{layer}.fsl"), &rejected);
            scratch.write("project.toml", &manifest);
            let wanted = cases::refusal(env!("CARGO_BIN_EXE_fslc"), &scratch.0, &path);
            let output = run(&scratch, &args, true);
            results.error_envelope(
                &format!("chain/{layer}/{jobs}/{keep_going}/{checks_only}"),
                &output,
                &wanted,
                !scratch.0.join("marker").exists(),
            );
            if jobs == 1 && !checks_only {
                let analysis = run(&scratch, &analysis_args, true);
                results.error_envelope(&format!("project/{layer}"), &analysis, &wanted, true);
            }
        }
    }
    results.finish();
}

#[test]
fn causal_explicit_mapping_is_a_root_and_missing_sidecars_lose_to_refusal() {
    let mut results = Results::default();
    let root = cases::root();
    let scratch = Scratch::new();
    let source = cases::rejected_domain();
    let mapping = scratch.write("mapping.fsl", &source);
    let wanted = cases::refusal(env!("CARGO_BIN_EXE_fslc"), &scratch.0, &mapping);
    for missing in [false, true] {
        let log = if missing {
            scratch.0.join("missing.jsonl")
        } else {
            root.join("examples/causal/evidence/incident-observation-log.jsonl")
        };
        let command = format!(
            "causal observe-expectations {}/examples/causal/incident_response.fsl --from-log {} --mapping {} --scope {}/examples/causal/evidence/incident-replay-scope.json --period-start 2026-01-01 --period-end 2026-03-31 --out refused.json",
            root.display(),
            log.display(),
            mapping.display(),
            root.display()
        );
        let args = cases::split(&command);
        let output = run(&scratch, &args, true);
        results.error_envelope(
            &format!("causal/mapping/missing={missing}"),
            &output,
            &wanted,
            !scratch.0.join("refused.json").exists(),
        );
    }
    results.finish();
}

#[test]
fn closed_schemas_and_raw_output_send_the_exact_warning_to_stderr_once() {
    let mut results = Results::default();
    let testplan_source = include_str!("fixtures/testplan_limit.fsl").replace(
        "action submit(q: Qty) {",
        "action submit(q: Qty) \"REQ-PROBE-001: 日本語😀\" {",
    );
    for template in [
        "kernel {src}",
        "conformance {src} --depth 2",
        "testplan {src} --depth 2",
        "document claims {src}",
        "document generate {src}",
        "explain {src} --readable",
        "html {src} --depth 2 --no-timing",
        "ledger {src} --depth 2",
    ] {
        let scratch = Scratch::new();
        let source = if template.starts_with("testplan ") {
            &testplan_source
        } else {
            cases::DOCUMENT_LEGACY
        };
        let mut args = scratch.args(template, source);
        // Closed document/Kernel v2 source identities must be portable, not
        // absolute host paths. The command executes inside this scratch tree.
        for arg in &mut args {
            if arg == scratch.0.join("source.fsl").to_str().unwrap() {
                *arg = "source.fsl".into();
            }
        }
        let output = run(&scratch, &args, false);
        let expected = cases::findings(source, "source.fsl", fsl_syntax::Edition::Fsl4);
        let stderr = String::from_utf8(output.stderr).unwrap();
        let actual: Vec<Value> = stderr
            .lines()
            .filter_map(|line| line.strip_prefix("warning: "))
            .map(|line| serde_json::from_str(line).expect("structured warning on stderr"))
            .collect();
        let closed_unchanged = if matches!(
            template.split_whitespace().next(),
            Some("kernel" | "conformance" | "testplan")
        ) {
            serde_json::from_slice::<Value>(&output.stdout)
                .unwrap()
                .get("warnings")
                .is_none()
        } else {
            !output.stdout.is_empty()
        };
        results.check(
            &format!("stderr/{template}"),
            (output.status.code(), &actual),
            &expected,
            output.status.code() == Some(0) && actual == expected && closed_unchanged,
        );
    }
    results.finish();
}

#[test]
fn header_flag_conflict_and_gate_off_modes_are_preserved() {
    let mut results = Results::default();
    for &(code, _, body) in cases::WARNING_CASES {
        let legacy = cases::legacy(code, body);
        for verb in ["check", "verify"] {
            for (header, flag, gate, expected_kind, expected_code) in [
                ("", "current", false, "", ""),
                (
                    "",
                    "next",
                    false,
                    if code == "deprecated_domain_enum_union" {
                        "deprecated_domain_enum_union"
                    } else {
                        "unsupported_in_edition"
                    },
                    "",
                ),
                ("fsl 4\n", "next", true, "parse", "edition_conflict"),
                ("fsl 5\n", "4", true, "parse", "edition_conflict"),
                ("fsl 5\n", "next", false, "parse", "FSL-DIALECT-UNKNOWN"),
            ] {
                let scratch = Scratch::new();
                let path = scratch.write("source.fsl", &format!("{header}{legacy}"));
                let mut args = vec![
                    verb.to_owned(),
                    path.to_str().unwrap().to_owned(),
                    "--edition".into(),
                    flag.into(),
                ];
                if verb == "verify" {
                    args.extend(["--depth".into(), "2".into(), "--no-cache".into()]);
                }
                let output = run(&scratch, &args, gate);
                let actual = value(&output);
                let agrees = if expected_kind.is_empty() {
                    output.status.code() == Some(0)
                        && actual["warnings"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|w| w["code"] == code)
                            .count()
                            == 1
                } else {
                    output.status.code() == Some(2)
                        && actual["kind"] == expected_kind
                        && (expected_code.is_empty() || actual["diagnostic_code"] == expected_code)
                };
                results.check(
                    &format!("mode/{code}/{verb}/{header:?}/{flag}/{gate}"),
                    (output.status.code(), &actual),
                    (expected_kind, expected_code),
                    agrees,
                );
            }
        }
    }
    results.finish();
}

#[test]
fn checked_repairs_and_editionless_sources_are_preserved() {
    let mut results = Results::default();
    for &(code, _, body) in cases::WARNING_CASES {
        let legacy = cases::legacy(code, body);
        let scratch = Scratch::new();
        let source = format!("fsl 5\n{legacy}");
        let path = scratch.write("source.fsl", &source);
        let output = run(
            &scratch,
            &[
                "migrate".into(),
                path.to_str().unwrap().into(),
                "--write".into(),
            ],
            true,
        );
        let actual = value(&output);
        let migrated = std::fs::read_to_string(&path).unwrap();
        results.check(
            &format!("preserved/migrate-write/{code}"),
            (output.status.code(), &actual),
            "checked write/0",
            output.status.code() == Some(0)
                && actual["written"] == true
                && migrated != source
                && cases::findings(&migrated, path.to_str().unwrap(), fsl_syntax::Edition::Fsl5)
                    .is_empty(),
        );
        let formatted = run(
            &scratch,
            &["fmt".into(), path.to_str().unwrap().into()],
            true,
        );
        results.check(
            &format!("preserved/fmt/{code}"),
            formatted.status.code(),
            0,
            formatted.status.code() == Some(0),
        );
    }
    let scratch = Scratch::new();
    let path = cases::root().join("examples/causal/incident_response.fsl");
    for gate in [false, true] {
        let output = run(
            &scratch,
            &[
                "causal".into(),
                "check".into(),
                path.to_str().unwrap().into(),
            ],
            gate,
        );
        let actual = value(&output);
        results.check(
            &format!("preserved/editionless/{gate}"),
            (output.status.code(), &actual),
            "causal check without edition metadata",
            output.status.code() == Some(0) && actual.get("fsl_edition").is_none(),
        );
    }
    results.finish();
}
