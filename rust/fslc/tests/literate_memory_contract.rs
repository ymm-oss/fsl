// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita
//! A4–A8: generated complete BASE anchors with exact PID/private-key bindings.
#![cfg(unix)]
#[path = "support/literate_memory.rs"]
mod support;
use serde_json::json;
use support::*;

#[test]
fn lexical_alias_parent_and_wrong_parent_reject() {
    let mut packets = Vec::new();
    let s = Scratch::new();
    std::fs::create_dir(s.0.join("real")).unwrap();
    s.write("real/doc.md", &markdown(COMPOSE));
    s.write("src/dep.fsl", LEAF);
    std::os::unix::fs::symlink("../real/doc.md", s.0.join("src/doc.md")).unwrap();
    for command in ["check", "verify", "scenarios"] {
        let positive = s.run(&args(command, "src/doc.md"));
        normal(command, &positive);
        packets.push((case("compose", command, "src/doc.md"), positive));
        let negative = s.run(&args(command, "real/doc.md"));
        assert_eq!(negative.status.code(), Some(2));
        assert_eq!(json(&negative)["result"], "error");
        assert!(
            json(&negative)["message"]
                .as_str()
                .unwrap()
                .contains("dep.fsl")
        );
        packets.push((case("wrong-parent", command, "real/doc.md"), negative));
    }
    compare_packets(packets);
}

#[test]
fn strict_tags_keep_extracted_requirements_and_ignore_markdown_and_other_fences() {
    let mut packets = Vec::new();
    let s = Scratch::new();
    let inner = fixture("strict_tag_empty_requirement_block.fsl");
    let noise = inner.replace("REQ-EMPTY", "REQ-MARKDOWN-ONLY");
    s.write(
        "src/doc.md",
        &format!("{noise}\n```python\n{noise}```\n{}", markdown(&inner)),
    );
    let warnings = json!([
        {"kind":"no_user_invariants","message":"spec declares no user invariants (only implicit type bounds are checked)"},
        {"kind":"unreferenced_requirement","element":"requirement","name":"REQ-EMPTY","loc":null,
         "hint":"no declaration tag, acceptance, or forbidden block references this requirement ID"}
    ]);
    for command in ["check", "verify"] {
        let mut arguments = args(command, "src/doc.md");
        arguments.push("--strict-tags");
        if command == "verify" {
            arguments.extend(["--deadlock", "ignore"]);
        }
        let output = s.run(&arguments);
        normal(command, &output);
        assert_eq!(json(&output)["warnings"], warnings);
        packets.push((case("strict-tags", command, "src/doc.md"), output));
    }
    // Also exercise the successful induction/lemma strict-tags finalizer; do not
    // pretend the bounded-verify packet/time allowance covers this other route.
    let output = s.run(&[
        "verify",
        "src/doc.md",
        "--depth",
        "4",
        "--no-cache",
        "--deadlock",
        "ignore",
        "--strict-tags",
        "--engine",
        "induction",
        "--lemma",
        "true",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json(&output)["result"], "proved");
    assert_eq!(json(&output)["warnings"], warnings);
    packets.push((case("strict-tags-lemma", "verify", "src/doc.md"), output));
    compare_packets(packets);
}

#[test]
fn root_and_dependency_editions_preserve_refusal_and_warning_order() {
    let mut packets = Vec::new();
    for dependency in [false, true] {
        for edition in [4, 5] {
            let s = Scratch::new();
            let source = format!("fsl {edition}\n{LEGACY}");
            if !dependency && edition == 5 {
                assert_eq!(
                    fsl_syntax::extract_literate_fsl(&markdown(&source)).unwrap(),
                    format!("\n\n\nfsl 5\n{LEGACY}\n"),
                    "pin full finding/edit-coordinate fixture"
                );
            }
            s.write(
                "src/doc.md",
                &markdown(if dependency { COMPOSE } else { &source }),
            );
            s.write("src/dep.fsl", &source);
            for command in ["check", "verify", "scenarios"] {
                let output = s.run(&args(command, "src/doc.md"));
                let value = json(&output);
                if edition == 5 {
                    assert_eq!(output.status.code(), Some(2));
                    assert_eq!(value["result"], "error");
                    assert_eq!(value["kind"], "unsupported_in_edition");
                    assert_eq!(value["findings"][0]["code"], "legacy_string_metadata");
                } else {
                    normal(command, &output);
                    assert_eq!(value["warnings"][0]["code"], "legacy_string_metadata");
                }
                let anchor = match (dependency, edition) {
                    (false, 4) => "root-edition4",
                    (false, 5) => "root-edition5",
                    (true, 4) => "dependency-edition4",
                    (true, 5) => "dependency-edition5",
                    _ => unreachable!(),
                };
                packets.push((case(anchor, command, "src/doc.md"), output));
            }
        }
    }
    compare_packets(packets);
}

#[test]
fn multifence_diagnostics_and_early_errors_keep_full_locations_and_classes() {
    let mut packets = Vec::new();
    let s = Scratch::new();
    // Input tokens are at 11:3 and 11:19. The full diagnostic spans/labels must be
    // separately observed, not inferred from these authored token coordinates.
    let prefix = "# Map\n\n```python\nthis is not FSL\n```\n```fsl\nspec Bad {\n  state { x: Bool = false }\n```\n```fsl\n";
    let cases = [
        ("parse", format!("{prefix}  $\n}}\n```\n")),
        (
            "type",
            format!("{prefix}  invariant Bad {{ unknown }}\n}}\n```\n"),
        ),
        ("causal", markdown("causal IncidentResponse {\n")),
        ("no-fence", "# No FSL\n```python\nnot FSL\n```\n".into()),
    ];
    for (anchor, source) in cases {
        s.write("src/doc.md", &source);
        for command in ["check", "verify", "scenarios"] {
            let output = s.run(&args(command, "src/doc.md"));
            assert_eq!(output.status.code(), Some(2));
            assert_eq!(json(&output)["result"], "error");
            if anchor == "no-fence" {
                assert_eq!(json(&output)["kind"], "io");
            }
            packets.push((case(anchor, command, "src/doc.md"), output));
        }
    }
    for command in ["check", "verify", "scenarios"] {
        let output = s.run(&args(command, "src/missing.md"));
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(json(&output)["kind"], "io");
        packets.push((case("missing", command, "src/missing.md"), output));
    }
    compare_packets(packets);
}

#[test]
fn accepted_domain_warning_and_scenarios_option_precedence() {
    let s = Scratch::new();
    let mut packets = Vec::new();
    s.write(
        "src/doc.md",
        &markdown(&fixture("domain_legacy_enum_union.fsl")),
    );
    let output = s.run(&["check", "src/doc.md"]);
    normal("check", &output);
    let value = json(&output);
    let finding = value["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["code"] == "deprecated_domain_enum_union")
        .expect("edition finding retained");
    assert_eq!(finding["loc"]["file"], "src/doc.md");
    packets.push((case("domain-warning", "check", "src/doc.md"), output));
    s.write("src/doc.md", &fixture("literate_toggle.md"));
    for (phase, kind) in [("valid", "usage"), ("no-fence", "usage")] {
        if phase == "no-fence" {
            s.write("src/doc.md", "# Not FSL\n");
        }
        let output = s.run(&["scenarios", "src/doc.md", "--strict-tags"]);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(json(&output)["kind"], kind);
        packets.push((case("scenarios-option", "scenarios", "src/doc.md"), output));
    }
    compare_packets(packets);
}

#[test]
#[allow(clippy::too_many_lines)]
fn implements_and_governance_fold_including_cached_kernel() {
    let mut packets = Vec::new();
    let s = Scratch::new();
    for name in ["abs.fsl", "impl.fsl", "impl_refines.fsl", "map.fsl"] {
        s.write(
            &format!("src/{name}"),
            &fixture(&format!("issue_1303/{name}")),
        );
    }
    let broken = fixture("issue_1303/impl.fsl");
    s.write("src/impl.fsl", &fixture("issue_1303/impl_refines.fsl"));
    s.write(
        "src/doc.md",
        &markdown(&fixture("issue_1303/governance.fsl")),
    );
    for command in ["check", "verify", "scenarios"] {
        let output = s.run(&args(command, "src/doc.md"));
        normal(command, &output);
        assert_eq!(
            json(&output)["governance"]["preservations"][0]["result"],
            "refines"
        );
        packets.push((case("governance-ok", command, "src/doc.md"), output));
    }
    let mut governance_key = None;
    for phase in ["cold", "warm", "broken"] {
        if phase == "broken" {
            s.write("src/impl.fsl", &broken);
        }
        let before = if phase == "cold" {
            Some(cache_entries(&s))
        } else {
            None
        };
        let warm_binding = if phase == "cold" {
            None
        } else {
            governance_key.clone()
        };
        let output = s.run(&["verify", "src/doc.md", "--depth", "4"]);
        if phase == "cold" {
            governance_key = Some(bind_new_cache_key(&s, before.as_ref().unwrap(), &output));
        }
        if phase == "cold" {
            assert!(json(&output).get("cache").is_none());
        }
        if phase == "warm" {
            assert_eq!(json(&output)["cache"]["hit"], true);
        }
        if phase == "broken" {
            assert_eq!(
                json(&output)["cache"]["hit"],
                true,
                "cached kernel must be folded again"
            );
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(json(&output)["result"], "refinement_failed");
            assert_eq!(
                json(&output)["governance"]["preservations"][0]["result"],
                "refinement_failed"
            );
        } else {
            normal("verify", &output);
        }
        let anchor = match phase {
            "cold" => "governance-cold",
            "warm" => "governance-warm",
            _ => "governance-cached-broken",
        };
        let expected = if phase == "cold" {
            case(anchor, "verify", "src/doc.md")
        } else {
            keyed_case(anchor, warm_binding.as_ref().unwrap())
        };
        packets.push((expected, output));
    }
    for command in ["check", "scenarios"] {
        let output = s.run(&args(command, "src/doc.md"));
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(json(&output)["result"], "refinement_failed");
        packets.push((case("governance-bad", command, "src/doc.md"), output));
    }
    let chain = root().join("tests/fixtures/chain");
    s.write(
        "src/business.fsl",
        &std::fs::read_to_string(chain.join("business.fsl")).unwrap(),
    );
    for (phase, file) in [
        ("positive", "requirements.fsl"),
        ("negative", "requirements_broken_implements.fsl"),
    ] {
        s.write(
            "src/doc.md",
            &markdown(&std::fs::read_to_string(chain.join(file)).unwrap()),
        );
        for command in ["check", "verify", "scenarios"] {
            let output = s.run(&args(command, "src/doc.md"));
            if phase == "positive" {
                normal(command, &output);
                assert_eq!(json(&output)["implements"]["result"], "refines");
            } else {
                assert_eq!(output.status.code(), Some(1));
                assert_eq!(json(&output)["result"], "refinement_failed");
                assert_eq!(json(&output)["implements"]["result"], "refinement_failed");
            }
            packets.push((
                case(
                    if phase == "positive" {
                        "implements-ok"
                    } else {
                        "implements-bad"
                    },
                    command,
                    "src/doc.md",
                ),
                output,
            ));
        }
    }
    compare_packets(packets);
}

#[test]
fn cache_uses_markdown_bytes_and_used_alias_dependency_not_unrelated_sibling() {
    let mut packets = Vec::new();
    let s = Scratch::new();
    std::fs::create_dir(s.0.join("real")).unwrap();
    let doc = markdown(COMPOSE);
    let prose_edit = doc.replacen("# Acceptance\n", "# ACCEPTANCE\n", 1);
    assert_ne!(doc.as_bytes(), prose_edit.as_bytes());
    assert_eq!(doc.len(), prose_edit.len());
    assert_eq!(doc.lines().count(), prose_edit.lines().count());
    let original_fsl = fsl_syntax::extract_literate_fsl(&doc).expect("literal compose fence");
    assert_eq!(
        fsl_syntax::extract_literate_fsl(&prose_edit).expect("same compose fence"),
        original_fsl
    );
    s.write("real/doc.md", &doc);
    std::os::unix::fs::symlink("../real/doc.md", s.0.join("src/doc.md")).unwrap();
    s.write("src/dep.fsl", LEAF);
    let mut keys: Vec<CacheBinding> = Vec::new();
    for (phase, hit) in [
        ("cold", false),
        ("warm", true),
        ("root-edit", false),
        ("root-warm", true),
        ("dependency-edit", false),
        ("dependency-warm", true),
        ("unrelated", true),
    ] {
        if phase == "root-edit" {
            s.write("real/doc.md", &prose_edit);
        }
        if phase == "dependency-edit" {
            s.write(
                "src/dep.fsl",
                &LEAF.replace("done = true", "done = not done"),
            );
        }
        if phase == "unrelated" {
            s.write("src/unrelated.fsl", "not even valid FSL\n");
        }
        let before = if hit { None } else { Some(cache_entries(&s)) };
        let warm_binding = if hit {
            Some(keys.last().unwrap().clone())
        } else {
            None
        };
        let output = s.run(&["verify", "src/doc.md", "--depth", "4"]);
        if !hit {
            let key = bind_new_cache_key(&s, before.as_ref().unwrap(), &output);
            assert!(
                !keys.iter().any(|previous| previous.key == key.key),
                "K_original, K_prose and K_dependency must differ pairwise"
            );
            keys.push(key); // Bound before the corresponding warm CLI, never from its cache field.
        }
        normal("verify", &output);
        if hit {
            assert_eq!(json(&output)["cache"]["hit"], true);
        } else {
            assert!(json(&output).get("cache").is_none());
        }
        let anchor = match phase {
            "cold" => "compose-cold",
            "warm" => "compose-warm",
            "root-edit" => "compose-prose-cold",
            "root-warm" => "compose-prose-warm",
            "dependency-edit" => "compose-changed-cold",
            "dependency-warm" => "compose-changed-warm",
            _ => "compose-unrelated-warm",
        };
        let expected = if hit {
            keyed_case(anchor, warm_binding.as_ref().unwrap())
        } else {
            case(anchor, "verify", "src/doc.md")
        };
        packets.push((expected, output));
    }
    compare_packets(packets);
}

#[test]
fn ordinary_fsl_and_unsupported_markdown_preserve_their_boundaries() {
    let mut packets = Vec::new();
    let s = Scratch::new();
    s.write("src/plain.fsl", LEAF);
    s.write("src/doc.md", "# Deliberately not FSL\n");
    for command in ["check", "verify", "scenarios"] {
        let output = s.run(&args(command, "src/plain.fsl"));
        normal(command, &output);
        packets.push((case("ordinary", command, "src/plain.fsl"), output));
    }
    for command in ["html", "testgen", "ledger"] {
        let output = s.run(&[command, "src/doc.md"]);
        let value = json(&output);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(value["result"], "error");
        assert_eq!(value["diagnostic_code"], "FSL-INPUT-LITERATE-UNSUPPORTED");
        assert_eq!(value["loc"]["file"], "src/doc.md");
        assert!(value["loc"].get("line").is_none() && value["loc"].get("column").is_none());
        packets.push((case("unsupported", command, "src/doc.md"), output));
    }
    compare_packets(packets);
}
