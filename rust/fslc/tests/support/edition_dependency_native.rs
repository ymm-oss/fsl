// SPDX-License-Identifier: Apache-2.0

#[path = "edition_dependency_cases.rs"]
mod cases;
#[path = "edition_cases.rs"]
mod common;
use super::*;
use fsl_tools::{build_causal_model_with_options as causal_model, compile_expectations};

fn fixture() -> (common::Scratch, std::path::PathBuf, String) {
    let scratch = common::Scratch::new();
    scratch.write("dep.fsl", &cases::next_leaf());
    let source = "compose C { use S as s from \"dep.fsl\" }\n".to_owned();
    let path = scratch.write("root.fsl", &source);
    (scratch, path, source)
}

fn consume(path: &Path, source: &str) -> (Value, i32) {
    match load_kernel_model_from_source(path, source) {
        Ok(_) => (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0),
        Err(error) => (spec_load_error_output(&error), 2),
    }
}

fn dependency_edition_o01_fresh_thread_has_no_owner_and_fails_closed() {
    assert!(
        crate::parse_options()
            .unstable()
            .contains(fsl_syntax::UnstableFeature::Fsl5),
        "run this adapter with FSLC_UNSTABLE=fsl5"
    );
    let (scratch, path, source) = fixture();
    scratch.write("dep.fsl", cases::LEAF);
    let canonical =
        std::thread::scope(|scope| scope.spawn(|| consume(&path, &source)).join().unwrap());
    assert_eq!(
        canonical,
        (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0),
        "canonical no-owner loader must really build a model"
    );
    scratch.write("dep.fsl", &cases::next_leaf());
    let root = path.to_string_lossy().into_owned();
    let result = std::thread::spawn(move || consume(&path, &source))
        .join()
        .unwrap();
    let mut observations = cases::Observations::default();
    observations.eq(
        "O01",
        result,
        (
            cases::unowned_expected(&cases::identity(&scratch, "dep.fsl"), &root),
            2,
        ),
    );
    observations.finish();
}

fn lenient_helper(id: &str, model: &KernelModel, path: &Path, source: &str) -> Value {
    match id {
        "P01" => reachable_counterfactuals_from_source(model, path, source, 2),
        "P02" => invariant_counterfactuals_from_source(model, path, source, 2),
        "P03" => json!(document_model_failure(source, path.parent().unwrap()).is_some()),
        "P04" => json!(readable_implements_text_from_source(path, source, model, 2)),
        _ => unreachable!(),
    }
}

fn dependency_edition_p01_p04_lenient_helpers_cannot_swallow_refusal() {
    let mut observations = cases::Observations::default();
    for id in ["P01", "P02", "P03", "P04"] {
        let (scratch, path, compose) = fixture();
        let source = if id == "P04" {
            cases::IMPLEMENTS
        } else {
            &compose
        };
        // Prepare names from a canonical twin before opening the critical
        // owner. Only the lenient helper reads the rejected child in that owner.
        let body = if id == "P01" {
            cases::LEAF
                .replace("{ done = true }", "{ requires false done = true }")
                .replace(
                    "invariant Typed",
                    "reachable Reached { done } invariant Typed",
                )
        } else {
            cases::LEAF.into()
        };
        scratch.write("dep.fsl", &body);
        let model = load_model_from_source(&path, source).unwrap();
        let body = if id == "P03" {
            "spec S { state {} invariant Typed { true } }\n".into()
        } else {
            body
        };
        scratch.write("dep.fsl", &body);
        let run = || {
            run_admitted_root(&path, Ok(source.into()), None, false, |source| {
                (
                    json!({"result":"explained","suggestions":lenient_helper(id,&model,&path,source)}),
                    0,
                )
            })
        };
        let positive = run();
        let repeated = run();
        assert_eq!(positive.1, 0);
        assert_eq!(repeated.1, 0);
        assert_eq!(cases::stable(positive.0.clone()), cases::stable(repeated.0));
        match id {
            "P01" | "P02" => assert!(
                !positive.0["suggestions"].as_array().unwrap().is_empty(),
                "canonical helper must reach a real result"
            ),
            "P03" => assert_eq!(positive.0["suggestions"], true),
            "P04" => assert_eq!(positive.0["suggestions"], "  - S: refines\n"),
            _ => unreachable!(),
        }
        let rejected = cases::next(&cases::legacy(&body));
        scratch.write("dep.fsl", &rejected);
        let produced = run();
        let expected = (cases::cli_refusal(&rejected, "dep.fsl"), 2);
        observations.eq(id, &produced, &expected);
    }
    observations.finish();
}

fn dependency_edition_o02_o03_sequential_owners_do_not_share_records() {
    let mut observations = cases::Observations::default();
    let (scratch, path, source) = fixture();
    let run = || {
        run_admitted_root(&path, Ok(source.clone()), None, false, |source| {
            consume(&path, source)
        })
    };
    let first = run();
    let child = "dep.fsl";
    let expected_a = (cases::cli_refusal(&cases::next_leaf(), child), 2);
    observations.eq("O02/A", &first, &expected_a);
    scratch.write("dep.fsl", cases::LEAF);
    let second = run();
    let expected_b = (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0);
    observations.eq("O02/B", &second, &expected_b);
    scratch.write("dep.fsl", cases::LEGACY_LEAF);
    let warning_a = run();
    let warning_b = run();
    let expected = (
        json!({"fsl":"1.0","result":"ok","warnings":[cases::metadata_finding(cases::LEGACY_LEAF, child, "current")]}),
        0,
    );
    for (label, result) in [("O03/A", warning_a), ("O03/B", warning_b)] {
        observations.eq(label, &result, &expected);
    }
    observations.finish();
}

fn dependency_edition_o04_nested_owner_and_o05_fresh_child_thread() {
    let (scratch, path, source) = fixture();
    scratch.write("dep.fsl", cases::LEGACY_LEAF);
    let nested = run_admitted_root(&path, Ok(source.clone()), None, false, |source| {
        run_admitted_root(&path, Ok(source.into()), None, false, |source| {
            consume(&path, source)
        })
    });
    let child = "dep.fsl";
    let expected = (
        json!({"fsl":"1.0","result":"ok","warnings":[cases::metadata_finding(cases::LEGACY_LEAF,child,"current")]}),
        0,
    );
    let mut observations = cases::Observations::default();
    observations.eq("O04", &nested, &expected);
    scratch.write("dep.fsl", &cases::next_leaf());
    let result = run_admitted_root(&path, Ok(source.clone()), None, false, |source| {
        std::thread::scope(|scope| scope.spawn(|| consume(&path, source)).join().unwrap())
    });
    observations.eq(
        "O05",
        result,
        (
            cases::unowned_expected(
                &cases::identity(&scratch, "dep.fsl"),
                path.to_str().unwrap(),
            ),
            2,
        ),
    );
    observations.finish();
}

fn dependency_edition_c03_direct_slots_keep_individual_warning_cardinality() {
    let scratch = common::Scratch::new();
    let path = scratch.write("direct.fsl", cases::LEGACY_LEAF);
    let result = run_admitted_roots(
        vec![
            (&path, Ok(cases::LEGACY_LEAF.into())),
            (&path, Ok(cases::LEGACY_LEAF.into())),
        ],
        None,
        false,
        |_, editions| {
            finish_admitted(
                (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0),
                editions,
                false,
            )
        },
    );
    let finding = cases::metadata_finding(cases::LEGACY_LEAF, path.to_str().unwrap(), "current");
    let expected = (
        json!({"fsl":"1.0","result":"ok","warnings":[finding.clone(),finding]}),
        0,
    );
    let mut observations = cases::Observations::default();
    observations.eq("C03", &result, &expected);
    observations.finish();
}

fn dependency_edition_t06_isolates_the_actual_owned_refine_step() {
    let (scratch, path, source) = fixture();
    scratch.write("dep.fsl", cases::LEAF);
    scratch.write("abstract.fsl", &source);
    scratch.write("map.fsl", "refinement M { impl C abs C maps auto }\n");
    let sections=parse_project_manifest("[requirements]\nfile = \"abstract.fsl\"\n[design]\nfile = \"root.fsl\"\nrefine_against = \"requirements\"\nmapping = \"map.fsl\"\ndepth = 2\n").unwrap();
    let roots = prepare_project_roots(&sections, &scratch.0, read_unprefixed_spec_source).unwrap();
    let first = run_chain_step_isolated("refine", "design", &sections, &scratch.0, &roots);
    let second = run_chain_step_isolated("refine", "design", &sections, &scratch.0, &roots);
    assert!(!first.1 && !second.1);
    assert_eq!(first.0["detail"]["result"], "refines");
    assert_eq!(cases::stable(first.0), cases::stable(second.0));
    scratch.write("dep.fsl", &cases::next_leaf());
    let result = run_chain_step_isolated("refine", "design", &sections, path.parent().unwrap(), &roots);
    let mut observations = cases::Observations::default();
    let expected = json!({"layer":"design->requirements","kind":"refine","file":path,"against":"requirements","abs_file":scratch.0.join("abstract.fsl"),"mapping":scratch.0.join("map.fsl"),"depth":2,"status":"failed","result":"error","exit_code":2,"detail":cases::cli_refusal(&cases::next_leaf(),"dep.fsl")});
    observations.eq("T06/direct-step", result, (expected, true));
    observations.finish();
}

#[test]
fn ledger_overlay_uses_captured_root_after_disk_changes() {
    let scratch = common::Scratch::new();
    let source = cases::annotated(cases::LEAF);
    let path = scratch.write("root.fsl", &source);
    common::commit(&scratch, &["root.fsl"]);
    let inputs = approval::GenerationInputs::Solver(approval::SolverGenerationInputs {
        depth: 2, deadlock: "ignore".into(), engine: "bmc".into(),
    });
    let request = LedgerReportRequest {
        path: &path, depth: 2, deadlock_mode: "ignore", engine: "bmc",
        impl_log: None, evidence_paths: &[], output_path: None,
    };
    let (rendered, status) = generate_unapproved_ledger_report_from_source(&request, &source, None);
    assert_eq!(status, 0, "{rendered}");
    let artifact = scratch.write("artifact.md", rendered["content"].as_str().unwrap());
    let records = [scratch.0.join("record.json")];
    let created = run_approval_create(&path, "ledger", &artifact, "test", &[], &inputs, Some(&records[0]), None);
    assert_eq!(created.1, 0, "{created:?}");
    let expected = run_ledger_report_from_source(&request, &source, &records, &[]);
    assert_eq!(expected.1, 0, "{expected:?}");
    scratch.write("root.fsl", "not valid FSL");
    assert_eq!(run_ledger_report_from_source(&request, &source, &records, &[]), expected);
}

fn dependency_edition_mapping_role_calibration() {
    let scratch = common::Scratch::new();
    let implementation = scratch.write("after.fsl", cases::LEAF);
    let abstraction = scratch.write("before.fsl", cases::LEAF);
    for id in ["E04/P", "E04/N"] {
        let row = cases::row(id);
        let fixture = cases::fixture(&row);
        let source = &fixture.files[&fixture.child];
        let path = scratch.write("mapping.fsl", source);
        let run = || {
            run_admitted_root(&path, Ok(source.clone()), None, false, |_| {
                run_refine(&implementation, &abstraction, &path, 2)
            })
        };
        let actual = run();
        if row.variant == cases::Variant::N {
            let expected = (cases::cli_refusal(source, path.to_str().unwrap()), 2);
            println!("CALIBRATION {id}/mapping-role produced={actual:?} expected={expected:?}");
            assert_eq!(actual, expected);
        } else {
            assert_eq!(actual.1, 0);
            assert_eq!(actual.0["result"], "refines");
            let repeated = run();
            assert_eq!(cases::stable(actual.0), cases::stable(repeated.0));
        }
    }
}

fn dependency_edition_e08_compiles_against_a_canonical_prefix() {
    let mut observations = cases::Observations::default();
    for row in cases::flat_rows()
        .into_iter()
        .filter(|row| row.id.starts_with("E08/"))
    {
        let s = common::Scratch::new();
        let canonical = cases::fixture(&cases::row("E08/P"));
        canonical.materialize(&s, "root.fsl");
        let raw = fsl_core::FsResolver::new(&s.0);
        let (model, _) = causal_model(&canonical.root, &raw, crate::parse_options()).unwrap();
        let fixture = cases::fixture(&row);
        fixture.materialize(&s, "root.fsl");
        let surface = fsl_syntax::parse_causal(&fixture.root).unwrap();
        let actual = run_admitted_root(
            &s.0.join("root.fsl"),
            Ok(fixture.root.clone()),
            None,
            false,
            |_| {
                let resolver = dependency_resolver(&s.0);
                let compiled =
                    compile_expectations(&surface, &model, &resolver, crate::parse_options());
                observations.eq(
                    &format!("{}/compile-rejected", row.id),
                    compiled.is_err(),
                    row.variant == cases::Variant::N,
                );
                match compiled {
                    Ok(compiled) => (json!({"count":compiled.len(),"warnings":[]}), 0),
                    Err(_) => (Value::Null, 2),
                }
            },
        );
        let path = cases::identity(&s, &fixture.child);
        let expected = if row.variant == cases::Variant::N {
            (cases::cli_refusal(&fixture.files[&fixture.child], &path), 2)
        } else {
            (
                json!({"count":2,"warnings":if row.variant == cases::Variant::W { cases::findings(&fixture.files[&fixture.child], &path, "current") } else { Vec::<Value>::new() }}),
                0,
            )
        };
        observations.eq(&format!("{}/compile-only", row.id), actual, expected);
    }
    observations.finish();
}

// Match the existing adapter's process-local opt-in; never mutate the test
// runner's environment while other tests are executing.
fn in_fsl5_test_process() -> bool {
    if crate::parse_options()
        .unstable()
        .contains(fsl_syntax::UnstableFeature::Fsl5)
    {
        return true;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([std::thread::current().name().unwrap(), "--exact"])
        .env("FSLC_UNSTABLE", "fsl5")
        .status()
        .unwrap();
    assert!(
        status.success(),
        "opted-in seam test must execute successfully"
    );
    false
}

#[test]
fn dependency_edition_prepared_execution_has_exact_captured_slots_and_no_readmission() {
    use spec_loading::admission_observer::{self as observer, Event};
    if !in_fsl5_test_process() {
        return;
    }
    let s = common::Scratch::new();
    let layer = format!("fsl 4\n{}", cases::LEGACY_LEAF);
    let mapping = "fsl 5\nrefinement M { impl S abs S maps auto }\n";
    let layer_path = s.write("layer.fsl", &layer);
    let map_path = s.write("map.fsl", mapping);
    let sections = parse_project_manifest("[requirements]\nfile = \"layer.fsl\"\n[design]\nfile = \"layer.fsl\"\nrefine_against = \"requirements\"\nmapping = \"map.fsl\"\n").unwrap();
    let events = observer::Handle::default();
    let _guard = observer::install(Some(events.clone()));
    let roots = prepare_project_roots(&sections, &s.0, read_unprefixed_spec_source).unwrap();
    let census = vec![
        Event::Admission(layer_path.clone(), layer.clone()),
        Event::Admission(layer_path.clone(), layer.clone()),
        Event::Admission(map_path, mapping.into()),
    ];
    assert_eq!(
        *events.lock().unwrap(),
        census,
        "C2/preparation-positive-census"
    );
    s.write("layer.fsl", "not FSL after capture");
    s.write("map.fsl", "not a mapping after capture");
    let produced = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _guard = observer::install(Some(events.clone()));
                run_prepared_project_roots(&roots.iter().collect::<Vec<_>>(), |sources| {
                    assert_eq!(
                        sources,
                        [layer.as_str(), layer.as_str(), mapping],
                        "C2/captured-literals"
                    );
                    for source in &sources[..2] {
                        let (_, model) =
                            load_kernel_model_from_source(&layer_path, source).unwrap();
                        assert_eq!(model.name, "S", "C2/model-identity");
                    }
                    (json!({"fsl":"1.0","result":"ok","warnings":[]}), 0)
                })
            })
            .join()
            .unwrap()
    });
    let finding = cases::metadata_finding(&layer, layer_path.to_str().unwrap(), "current");
    assert_eq!(
        produced,
        (
            json!({"fsl":"1.0","result":"ok","fsl_edition":4,"warnings":[finding.clone(),finding]}),
            0
        ),
        "C2/complete-token-result"
    );
    assert_eq!(
        *events.lock().unwrap(),
        census,
        "C2/no-root-owner-readmission"
    );
}

#[test]
fn dependency_edition_standalone_and_nested_whole_reports_keep_independent_findings() {
    let s = common::Scratch::new();
    let source = "compose C { use S as s from \"dep.fsl\" }\n";
    let path = s.write("root.fsl", source);
    let inputs = approval::GenerationInputs::Solver(approval::SolverGenerationInputs {
        depth: 2,
        deadlock: "ignore".into(),
        engine: "bmc".into(),
    });
    for legacy in [false, true] {
        s.write(
            "dep.fsl",
            if legacy {
                cases::LEGACY_LEAF
            } else {
                cases::LEAF
            },
        );
        let expected = if legacy {
            vec![cases::metadata_finding(
                cases::LEGACY_LEAF,
                "dep.fsl",
                "current",
            )]
        } else {
            vec![]
        };
        for kind in ["html", "ledger"] {
            let standalone = if kind == "html" {
                run_html_report_with_timing(&path, 2, "ignore", "bmc", None, false)
            } else {
                run_ledger_report_from_source(
                    &LedgerReportRequest {
                        path: &path,
                        depth: 2,
                        deadlock_mode: "ignore",
                        engine: "bmc",
                        impl_log: None,
                        evidence_paths: &[],
                        output_path: None,
                    },
                    source,
                    &[],
                    &[],
                )
            };
            assert_eq!(standalone.1, 0, "C1/standalone-status/{kind}");
            let artifact = standalone.0["content"].as_str().unwrap();
            let findings: Value = if kind == "html" {
                let raw = artifact.split_once("<details><summary>verify JSON</summary><div><div class=\"code-block\"><pre>").unwrap().1.split_once("</pre>").unwrap().0
                    .replace("&quot;","\"").replace("&#x27;","'").replace("&lt;","<").replace("&gt;",">").replace("&amp;","&");
                serde_json::from_str::<Value>(&raw).unwrap()["warnings"].clone()
            } else {
                serde_json::from_str(
                    artifact
                        .split_once("<details><summary>raw findings</summary>\n\n```json\n")
                        .unwrap()
                        .1
                        .split_once("\n```")
                        .unwrap()
                        .0,
                )
                .unwrap()
            };
            assert_eq!(
                findings,
                json!(expected),
                "C1/{kind}/independent-whole-report-findings"
            );
            assert_eq!(
                findings.as_array().unwrap().len(),
                usize::from(legacy),
                "C1/{kind}/independent-whole-report-count"
            );
            let mut nested = None;
            let result = run_admitted_root(&path, Ok(source.into()), None, false, |captured| {
                let edition = admit_root_source(&path, captured, None).unwrap();
                nested = Some(
                    approval_artifact(&path, captured, &edition, kind, &inputs)
                        .unwrap()
                        .0,
                );
                (json!({"result":"ok"}), 0)
            });
            assert_eq!(result.1, 0);
            assert_eq!(
                approval::normalized_artifact(kind, &nested.unwrap()).unwrap(),
                approval::normalized_artifact(kind, artifact.as_bytes()).unwrap(),
                "C1/{kind}/complete-normalized-artifact"
            );
        }
    }
}

#[test]
#[allow(clippy::too_many_lines)] // Keep positional, IO, and captured grouping controls on one carrier.
fn dependency_edition_positional_carrier_keeps_errors_and_captured_grouping() {
    let s = common::Scratch::new();
    let layer = s.write("same.fsl", cases::LEAF);
    let mapping_source = "refinement M { impl S abs S maps auto }\n";
    let mapping = s.write("same-map.fsl", mapping_source);
    let missing = s.0.join("later.fsl");
    let io = std::fs::read_to_string(&missing).unwrap_err().to_string();
    let manifest = "[business]\nfile = \"same.fsl\"\n[requirements]\nfile = \"same.fsl\"\nrefine_against = \"business\"\nmapping = \"same-map.fsl\"\n[design]\nfile = \"later.fsl\"\nrefine_against = \"requirements\"\nmapping = \"same-map.fsl\"\n";
    let sections = parse_project_manifest(manifest).unwrap();
    let roots = prepare_project_roots(&sections, &s.0, read_spec_source).unwrap();
    let error = json!({"fsl":"1.0","result":"error","kind":"io","message":format!("{}: {io}", missing.display())});
    let projection = || {
        roots
            .iter()
            .map(|root| {
                (
                    root.layer,
                    root.mapping,
                    root.path.clone(),
                    root.source().map(str::to_owned),
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = vec![
        ("business", false, layer.clone(), Ok(cases::LEAF.to_owned())),
        ("requirements", false, layer, Ok(cases::LEAF.to_owned())),
        ("design", false, missing, Err((error.clone(), 2))),
        (
            "requirements",
            true,
            mapping.clone(),
            Ok(mapping_source.into()),
        ),
        ("design", true, mapping, Ok(mapping_source.into())),
    ];
    assert_eq!(projection(), expected, "C4/five-positional-slots");
    s.write("later.fsl", cases::LEAF);
    s.write("same.fsl", "changed after capture");
    s.write("same-map.fsl", "changed mapping");
    assert_eq!(projection(), expected, "C4/captured-error-and-bytes");
    assert_eq!(
        run_prepared_project_roots(&[&roots[2]], |_| panic!("C4/unreadable-callback-ran")),
        (error, 2),
        "C4/unreadable-result"
    );
    let steps = vec![
        ("spec".into(), "business".into()),
        ("impl".into(), "impl".into()),
        ("spec".into(), "requirements".into()),
        ("spec".into(), "design".into()),
        ("refine".into(), "design".into()),
    ];
    let mut grouping = cases::Observations::default();
    grouping.eq(
        "C4/equal-captured-grouping",
        chain_step_groups(&steps, &roots),
        vec![vec![0, 2], vec![3], vec![4]],
    );

    s.write("same.fsl", cases::LEAF);
    s.write("later.fsl", &format!("{}\n", cases::LEAF));
    s.write("same-map.fsl", mapping_source);
    let path = s.write("project.toml", manifest);
    let prepared = prepare_analysis(&path).unwrap();
    let run = || {
        run_prepared_analysis(
            &path,
            &prepared,
            "traceability_graph",
            None,
            "json",
            None,
            None,
            None,
        )
    };
    let before = run();
    assert_eq!(before.1, 0, "C4/real-analysis: {before:?}");
    s.write("project.toml", "not a manifest");
    s.write("same.fsl", "invalid root");
    s.write("later.fsl", "invalid root");
    s.write("same-map.fsl", "invalid mapping");
    assert_eq!(prepared.source, manifest, "C4/captured-manifest");
    let project = prepared.project.as_ref().unwrap();
    assert_eq!(
        project
            .sections
            .iter()
            .map(|(name, section)| (name.as_str(), json!(section.values)))
            .collect::<Vec<_>>(),
        vec![
            ("business", json!({"file":"same.fsl"})),
            (
                "design",
                json!({"file":"later.fsl","refine_against":"requirements","mapping":"same-map.fsl"})
            ),
            (
                "requirements",
                json!({"file":"same.fsl","refine_against":"business","mapping":"same-map.fsl"})
            ),
        ],
        "C4/complete-captured-sections"
    );
    assert_eq!(
        project
            .roots
            .iter()
            .map(|root| (
                root.layer,
                root.mapping,
                root.path.clone(),
                root.source().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("business", false, s.0.join("same.fsl"), cases::LEAF),
            ("requirements", false, s.0.join("same.fsl"), cases::LEAF),
            (
                "design",
                false,
                s.0.join("later.fsl"),
                format!("{}\n", cases::LEAF).as_str()
            ),
            (
                "requirements",
                true,
                s.0.join("same-map.fsl"),
                mapping_source
            ),
            ("design", true, s.0.join("same-map.fsl"), mapping_source),
        ],
        "C4/top-level-slot-order"
    );
    grouping.eq(
        "C4/distinct-captured-grouping",
        chain_step_groups(&steps, &project.roots),
        vec![vec![0, 2], vec![3], vec![4]],
    );
    assert_eq!(run(), before, "C4/complete-captured-analysis");

    // Different paths, initially equal bytes: changing just one backing file
    // must not split their captured-source group.
    s.write("same.fsl", cases::LEAF);
    s.write("twin.fsl", cases::LEAF);
    let sections = parse_project_manifest("[business]\nfile = \"same.fsl\"\n[requirements]\nfile = \"twin.fsl\"\n[design]\nfile = \"absent.fsl\"\n").unwrap();
    let roots = prepare_project_roots(&sections, &s.0, read_spec_source).unwrap();
    s.write("same.fsl", "different A");
    s.write("twin.fsl", "different B");
    grouping.eq(
        "C4/equal-to-different-captured-grouping",
        chain_step_groups(&steps, &roots),
        vec![vec![0, 2], vec![3], vec![4]],
    );
    grouping.finish();
}

#[test]
#[allow(clippy::too_many_lines)]
fn dependency_edition_actual_workers_transport_tokens_and_finish_detail_before_wrapper() {
    use spec_loading::admission_observer::{self as observer, Event};
    if !in_fsl5_test_process() {
        return;
    }
    let s = common::Scratch::new();
    let caller = std::thread::current().id();
    let layers = ["business", "requirements", "design"];
    let sources =
        layers.map(|layer| format!("fsl 4\ncompose C {{ use S as s from \"{layer}-dep.fsl\" }}\n"));
    let mapping = "fsl 5\nrefinement M { impl C abs C maps auto }\n";
    for (layer, source) in layers.iter().zip(&sources) {
        s.write(&format!("{layer}.fsl"), source);
        s.write(&format!("{layer}-dep.fsl"), cases::LEGACY_LEAF);
    }
    s.write("map.fsl", mapping);
    let sections = parse_project_manifest("[business]\nfile = \"business.fsl\"\n[requirements]\nfile = \"requirements.fsl\"\n[design]\nfile = \"design.fsl\"\nrefine_against = \"requirements\"\nmapping = \"map.fsl\"\nrefine_depth = 2\n").unwrap();
    let steps = vec![
        ("spec".into(), "business".into()),
        ("spec".into(), "requirements".into()),
        ("spec".into(), "design".into()),
        ("refine".into(), "design".into()),
    ];
    let mut observations = cases::Observations::default();
    for rejected in [false, true] {
        // Recreate the captured roots, but mutate backing roots/mapping before any worker consumes them.
        for (layer, source) in layers.iter().zip(&sources) {
            s.write(&format!("{layer}.fsl"), source);
        }
        s.write("map.fsl", mapping);
        let events = observer::Handle::default();
        let _guard = observer::install(Some(events.clone()));
        let roots = prepare_project_roots(&sections, &s.0, read_unprefixed_spec_source).unwrap();
        let mut census = layers
            .iter()
            .zip(&sources)
            .map(|(layer, source)| {
                Event::Admission(s.0.join(format!("{layer}.fsl")), source.clone())
            })
            .collect::<Vec<_>>();
        census.push(Event::Admission(s.0.join("map.fsl"), mapping.into()));
        assert_eq!(
            *events.lock().unwrap(),
            census,
            "C3/preparation-positive-census"
        );
        let allocations = roots
            .iter()
            .map(|root| root.source.as_ref().unwrap().as_ptr() as usize)
            .collect::<Vec<_>>();
        for layer in layers {
            s.write(&format!("{layer}.fsl"), "invalid changed root");
        }
        s.write("map.fsl", "invalid changed mapping");
        if rejected {
            s.write("design-dep.fsl", &cases::next_leaf());
        }
        let actual = run_chain_steps(&steps, &sections, &s.0, roots, true, 3);
        let child = |layer: &str| {
            cases::metadata_finding(cases::LEGACY_LEAF, &format!("{layer}-dep.fsl"), "current")
        };
        let mut expected = Vec::new();
        for layer in layers {
            let failed = rejected && layer == "design";
            let detail = if failed {
                cases::cli_refusal(&cases::next_leaf(), "design-dep.fsl")
            } else {
                json!({"fsl":"1.0","result":"ok","spec":"C","warnings":[child(layer)],"fsl_edition":4})
            };
            // Dependency refusal replaces the finalized detail, retaining the child's edition.
            let mut detail = detail;
            if !failed {
                detail["fsl_edition"] = json!(4);
            }
            expected.push(Some((json!({"layer":layer,"kind":"check","file":s.0.join(format!("{layer}.fsl")),"status":if failed {"failed"} else {"passed"},"result":if failed {"error"} else {"ok"},"exit_code":if failed {2} else {0},"detail":detail}),failed)));
        }
        let mut detail = if rejected {
            cases::cli_refusal(&cases::next_leaf(), "design-dep.fsl")
        } else {
            json!({"fsl":"1.0","impl":"C","abs":"C","result":"refines","checked_to_depth":2,"action_map":{"s.finish":"s.finish"},"warnings":[child("design"),child("requirements")]})
        };
        if !rejected {
            detail["fsl_edition"] = json!(4);
        }
        expected.push(Some((json!({"layer":"design->requirements","kind":"refine","file":s.0.join("design.fsl"),"against":"requirements","abs_file":s.0.join("requirements.fsl"),"mapping":s.0.join("map.fsl"),"depth":2,"status":if rejected {"failed"} else {"passed"},"result":if rejected {"error"} else {"refines"},"exit_code":if rejected {2} else {0},"detail":detail}),rejected)));
        observations.eq(
            &format!("C3/complete-ordered-isolated-packets/rejected={rejected}"),
            &actual,
            &expected,
        );
        let observed = events.lock().unwrap();
        assert_eq!(
            observed
                .iter()
                .filter(|event| matches!(event, Event::Admission(..)))
                .cloned()
                .collect::<Vec<_>>(),
            census,
            "C3/no-root-owner-readmission"
        );
        for (_, layer) in &steps {
            // design has both a check and refine dispatch; pair in per-worker event order below.
            let workers = observed
                .iter()
                .filter_map(|event| match event {
                    Event::Hop("worker", name, thread, roots, bytes) if name == layer => {
                        Some((thread, roots, bytes))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let fresh = observed
                .iter()
                .filter_map(|event| match event {
                    Event::Hop("step", name, thread, roots, bytes) if name == layer => {
                        Some((thread, roots, bytes))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let count = if layer == "design" { 2 } else { 1 };
            assert_eq!(workers.len(), count, "C3/worker-observer-live/{layer}");
            assert_eq!(fresh.len(), count, "C3/fresh-step-observer-live/{layer}");
            assert!(
                workers.iter().all(|(worker, _, _)| **worker != caller),
                "C3/worker-not-caller"
            );
            for (thread, root, bytes) in &fresh {
                assert_ne!(**thread, caller, "C3/fresh-not-caller");
                assert!(
                    workers.iter().all(|(worker, _, _)| worker != thread),
                    "C3/fresh-not-worker"
                );
                assert!(
                    workers
                        .iter()
                        .all(|(_, worker_root, worker_bytes)| worker_root == root
                            && worker_bytes == bytes),
                    "C3/shared-carrier-identity"
                );
                assert_eq!(
                    **bytes, allocations,
                    "C3/captured-source-allocation-identity"
                );
            }
        }
    }
    observations.finish();
}

#[test]
fn dependency_edition_private_adapter_executes_all_assigned_rows() {
    let gate = fsl_syntax::UnstableFeature::Fsl5;
    if !crate::parse_options().unstable().contains(gate) {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args([std::thread::current().name().unwrap(), "--exact"]);
        child.env("FSLC_UNSTABLE", "fsl5");
        assert!(child.status().unwrap().success());
        return;
    }
    let scratch = common::Scratch::new();
    scratch.write("dep.fsl", "fsl 99\n");
    let log = fslc_rust::migration::DependencyRecords::default();
    let resolver = dependency_resolver(&scratch.0).report_to(log.clone());
    assert_eq!(
        fsl_core::FileResolver::read(&resolver, "dep.fsl").unwrap(),
        "fsl 99\n"
    );
    assert_eq!(
        log.borrow().warnings,
        vec![
            json!({"code":"unsupported_edition","kind":"parse","severity":"warning","message":"unsupported syntax edition 'fsl 99'; supported editions: 4, 5 at 1:1","loc":{"file":"dep.fsl","line":1,"column":1}})
        ]
    );
    cases::exercise(
        "private",
        &[
            dependency_edition_o01_fresh_thread_has_no_owner_and_fails_closed,
            dependency_edition_o02_o03_sequential_owners_do_not_share_records,
            dependency_edition_o04_nested_owner_and_o05_fresh_child_thread,
            dependency_edition_p01_p04_lenient_helpers_cannot_swallow_refusal,
            dependency_edition_c03_direct_slots_keep_individual_warning_cardinality,
            dependency_edition_t06_isolates_the_actual_owned_refine_step,
            dependency_edition_mapping_role_calibration,
            dependency_edition_e08_compiles_against_a_canonical_prefix,
        ],
    );
}
