// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Lowering contract for the next edition (fsl 5 plan §2, "共通の受け入れ条件").
//!
//! Each example under `tests/fixtures/fsl5/<example>/` holds the design
//! draft's hand-written `lowered.fsl` (the expected lowering, in today's
//! syntax), the `original.fsl` it must stay equivalent to, and the two
//! refinement mappings. `lowered.fsl` is a fixed expectation written by a
//! person: it is never regenerated from a lowering's output, and this test
//! has no golden-update switch. Changing it means reading the diff in review.
//!
//! - **K** (kernel equality): every next-edition source of an example lowers
//!   to exactly the kernel of its expected lowering, compared by
//!   `support/kernel_compare.rs`. The stages that add the syntax add the
//!   sources to [`FSL5_SOURCES`]. A source written without some of its rules
//!   (`fsl5_en_norules.fsl`; e2e's `fsl5_en_req1.fsl` without its decision
//!   table and `fsl5_en_decide.fsl` without its `rule` block) is compared
//!   with a reduced lowering: `lowered.fsl` with only the requires lines the
//!   missing declarations contribute deleted by hand, a diff read in review.
//! - **R** (refinement both ways): `lowered.fsl` and the original refine each
//!   other at depth 8, and so does every next-edition source K compares with
//!   `lowered.fsl` itself.
//! - **M** (the check can fail): one rule of `lowered.fsl` weakened makes R
//!   fail and K differ; one rule statement taken out of a complete
//!   next-edition source does the same.
//! - **T** (requirement attribution): K leaves out the requirement IDs
//!   (`requirement`, `annotations`, `meta`), because their origins differ.
//!   T compares what K leaves out: every complete next-edition source gives
//!   each action and property of its kernel the requirement IDs
//!   `lowered.fsl` gives it, in the annotations and in the kernel's
//!   `requirement`, and `fslc ledger` lists the same requirements with the
//!   same status. The IDs are the ones the author wrote (a `requirement`
//!   block or `@requirement` in `lowered.fsl`; `covers`, `rule`, `decide`,
//!   `goal` and `policy` in the source), never a note the lowering makes up.
//!   A reduced source is not under T: the requirement it leaves out is
//!   absent by design.
//!
//! The `rules` example has no original: its `lowered.fsl` is the hand-written
//! expectation for the rule statements no design-draft example uses, held to
//! K (and to M on the source) only. So is `relations`, for the forms of the
//! relations stage (S8) that no design-draft example uses: a `policy`,
//! `always … in … has <role>`, a resource a rule reads.
//!
//! A measure lowers to a projection, which the Public Kernel does not carry;
//! `fsl5_relations.rs` checks it.
//!
//! Acceptance scenarios do not reach the kernel and are not part of K.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "support/kernel_compare.rs"]
mod kernel_compare;

struct Example {
    name: &'static str,
    /// The abstraction `lowered.fsl` corresponds to name-for-name.
    original: &'static str,
    /// One rule of `lowered.fsl` weakened: `(exact text, replacement)`.
    weakening: (&'static str, &'static str),
    /// What `refine <weakened> <original>` must report.
    weakened_result: &'static str,
}

const EXAMPLES: &[Example] = &[
    Example {
        name: "e2e",
        original: "original.fsl",
        // The decide row `SmallClaim` admits one unit above the limit.
        weakening: (
            "      requires claim_amount[c] <= AUTO_LIMIT\n",
            "      requires claim_amount[c] <= AUTO_LIMIT + 1\n",
        ),
        weakened_result: "refinement_failed",
    },
    Example {
        name: "rejection",
        original: "original.fsl",
        // `allow Operator approves Request only for OperatorHeld` dropped.
        weakening: (
            "fair action approve_tool(r: Request) {\n      requires request_stage[r] == ToolApproval\n      requires request_role[r] == Operator\n",
            "fair action approve_tool(r: Request) {\n      requires request_stage[r] == ToolApproval\n",
        ),
        weakened_result: "refinement_failed",
    },
    Example {
        name: "multi_entity",
        // Entity and stage names of `original.fsl` differ from the lowering;
        // `original_renamed.fsl` is the same model under the lowering's names.
        original: "original_renamed.fsl",
        // `only when ... stock >= qty` dropped: stock can leave its range.
        weakening: ("requires qty > 0 and stock >= qty", "requires qty > 0"),
        weakened_result: "impl_violated",
    },
];

/// `(example, next-edition source, expected lowering)` checked by K;
/// `every_fsl5_source_is_under_k` keeps a source from being added without K.
const FSL5_SOURCES: &[(&str, &str, &str)] = &[
    ("e2e", "fsl5_en_norules.fsl", "lowered_norules.fsl"),
    ("e2e", "fsl5_en.fsl", "lowered.fsl"),
    // fsl5_en.fsl in Japanese aliases with particles (English keywords), and
    // in Japanese keywords too (S5): the three lower to the one kernel.
    ("e2e", "fsl5_ja_particles.fsl", "lowered.fsl"),
    ("e2e", "fsl5_ja_full.fsl", "lowered.fsl"),
    // The rule without the decision table: `lowered.fsl` minus the decide
    // rows' requires lines.
    ("e2e", "fsl5_en_req1.fsl", "lowered_req1.fsl"),
    // The decision table without the rule (S7): `lowered.fsl` minus REQ-1's
    // requires line.
    ("e2e", "fsl5_en_decide.fsl", "lowered_decide.fsl"),
    ("rejection", "fsl5_en_norules.fsl", "lowered_norules.fsl"),
    ("rejection", "fsl5_en.fsl", "lowered.fsl"),
    ("rules", "fsl5_en.fsl", "lowered.fsl"),
    // S8: the relation, the resource, the measures and the goal, in English
    // and in Japanese keywords with the glossary's aliases. The rule `always
    // … has buyer` is the one S6 left to this stage.
    ("multi_entity", "fsl5_en.fsl", "lowered.fsl"),
    ("multi_entity", "fsl5_ja_full.fsl", "lowered.fsl"),
    ("relations", "fsl5_en.fsl", "lowered.fsl"),
];

/// For each complete next-edition source (K against `lowered.fsl`): one rule
/// statement, as an exact line of the source, and the result of
/// `refine <source without it> <original>` (`None` without an original).
const FSL5_WEAKENINGS: &[(&str, &str, Option<&str>)] = &[
    (
        "rejection",
        "allow Operator approves Request only for OperatorHeld",
        Some("refinement_failed"),
    ),
    ("rules", "never User adds Order for Large", None),
    // Without the rule, `place` can take more than the stock.
    (
        "multi_entity",
        "allow User places Order only when input qty > 0 and stock >= input qty",
        Some("impl_violated"),
    ),
    (
        "multi_entity",
        "BuyerKnownAfterPlace: always Order not in Empty has buyer",
        None,
    ),
    (
        "relations",
        "always Ticket in Assigned, Closed, Escalated has assignee",
        None,
    ),
];

fn example_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5")
        .join(name)
}

/// `fslc refine <implementation> <abstraction> <mapping> --depth 8` in
/// `dir`, in the published language.
fn refine(dir: &Path, implementation: &str, abstraction: &str, mapping: &str) -> (Value, i32) {
    refine_with(dir, implementation, abstraction, mapping, None)
}

/// [`refine`] with the `unstable` features enabled.
fn refine_with(
    dir: &Path,
    implementation: &str,
    abstraction: &str,
    mapping: &str,
    unstable: Option<&str>,
) -> (Value, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args([
            "refine",
            implementation,
            abstraction,
            mapping,
            "--depth",
            "8",
        ])
        .current_dir(dir)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if let Some(features) = unstable {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, features);
    }
    let output = command.output().expect("run fslc refine");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "refine {implementation} {abstraction} in {}: invalid JSON ({error}): {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

#[test]
fn r_lowered_and_original_refine_each_other() {
    for example in EXAMPLES {
        let dir = example_dir(example.name);
        for (implementation, abstraction, mapping) in [
            (
                "lowered.fsl",
                example.original,
                "refines_lowered_to_original.fsl",
            ),
            (
                example.original,
                "lowered.fsl",
                "refines_original_to_lowered.fsl",
            ),
        ] {
            let (output, exit) = refine(&dir, implementation, abstraction, mapping);
            assert_eq!(
                (output["result"].as_str(), exit),
                (Some("refines"), 0),
                "{}: refine {implementation} {abstraction}: {output}",
                example.name
            );
        }
    }
}

#[test]
fn m_weakened_lowering_fails_r_and_differs_in_k() {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fsl5_lowering_contract");
    for example in EXAMPLES {
        let source = example_dir(example.name);
        let dir = scratch.join(example.name);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        for entry in std::fs::read_dir(&source).expect("read example") {
            let path = entry.expect("example entry").path();
            std::fs::copy(&path, dir.join(path.file_name().expect("file name")))
                .expect("copy example");
        }
        let lowered = std::fs::read_to_string(source.join("lowered.fsl")).expect("read lowered");
        let (text, replacement) = example.weakening;
        assert_eq!(
            lowered.matches(text).count(),
            1,
            "{}: the weakened rule must occur exactly once in lowered.fsl",
            example.name
        );
        std::fs::write(
            dir.join("weakened.fsl"),
            lowered.replacen(text, replacement, 1),
        )
        .expect("write weakened lowering");

        let (output, exit) = refine(
            &dir,
            "weakened.fsl",
            example.original,
            "refines_lowered_to_original.fsl",
        );
        assert_eq!(
            (output["result"].as_str(), exit),
            (Some(example.weakened_result), 1),
            "{}: refine weakened.fsl {}: {output}",
            example.name,
            example.original
        );

        let difference = kernel_compare::difference(
            &kernel_compare::kernel_v2(&source.join("lowered.fsl")),
            &kernel_compare::kernel_v2(&dir.join("weakened.fsl")),
        );
        assert!(
            difference.is_some(),
            "{}: K must notice the weakened rule",
            example.name
        );
    }
}

/// R for the next-edition sources: each complete source and the original
/// refine each other, with the same mappings as `lowered.fsl`.
#[test]
fn r_complete_fsl5_sources_and_the_original_refine_each_other() {
    let mut checked = 0;
    for &(example, source, lowering) in FSL5_SOURCES {
        let Some(contract) = EXAMPLES.iter().find(|candidate| candidate.name == example) else {
            continue;
        };
        if lowering != "lowered.fsl" {
            continue;
        }
        let dir = example_dir(example);
        for (implementation, abstraction, mapping) in [
            (source, contract.original, "refines_lowered_to_original.fsl"),
            (contract.original, source, "refines_original_to_lowered.fsl"),
        ] {
            let (output, exit) =
                refine_with(&dir, implementation, abstraction, mapping, Some("fsl5"));
            assert_eq!(
                (output["result"].as_str(), exit),
                (Some("refines"), 0),
                "{example}: refine {implementation} {abstraction}: {output}"
            );
        }
        checked += 1;
    }
    assert!(
        checked > 0,
        "not vacuous: a complete source has an original"
    );
}

/// M for the next-edition sources: a complete source without one rule
/// statement (and without its `rule` block when that was the block's only
/// statement) no longer lowers to `lowered.fsl`, and no longer refines the
/// original.
#[test]
fn m_a_source_without_a_rule_differs_in_k_and_fails_r() {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fsl5_lowering_contract_m5");
    for &(example, statement, refine_result) in FSL5_WEAKENINGS {
        let (_, source, lowering) = FSL5_SOURCES
            .iter()
            .copied()
            .find(|&(listed, _, lowering)| listed == example && lowering == "lowered.fsl")
            .expect("a complete source");
        let dir = scratch.join(example);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        for entry in std::fs::read_dir(example_dir(example)).expect("read example") {
            let path = entry.expect("example entry").path();
            std::fs::copy(&path, dir.join(path.file_name().expect("file name")))
                .expect("copy example");
        }
        let text = std::fs::read_to_string(dir.join(source)).expect("read source");
        let line = format!("    {statement}\n");
        assert_eq!(text.matches(&line).count(), 1, "{example}: {statement}");
        std::fs::write(dir.join("weakened.fsl"), without_statement(&text, &line))
            .expect("write weakened source");
        let difference = kernel_compare::difference(
            &kernel_compare::kernel_v2(&dir.join(lowering)),
            &kernel_compare::kernel_v2_fsl5(&dir.join("weakened.fsl")),
        );
        assert!(
            difference.is_some(),
            "{example}: K must notice `{statement}`"
        );
        if let Some(expected) = refine_result {
            let original = EXAMPLES
                .iter()
                .find(|candidate| candidate.name == example)
                .expect("an example with an original")
                .original;
            let (output, exit) = refine_with(
                &dir,
                "weakened.fsl",
                original,
                "refines_lowered_to_original.fsl",
                Some("fsl5"),
            );
            assert_eq!(
                (output["result"].as_str(), exit),
                (Some(expected), 1),
                "{example}: refine weakened.fsl {original}: {output}"
            );
        }
    }
}

/// `text` without `line`, and without the `rule` block `line` was the only
/// statement of (a rule has at least one).
fn without_statement(text: &str, line: &str) -> String {
    let lines = text
        .split_inclusive('\n')
        .filter(|candidate| *candidate != line)
        .collect::<Vec<_>>();
    let mut kept = String::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].starts_with("  rule ") && lines.get(index + 1) == Some(&"  }\n") {
            index += 2;
            continue;
        }
        kept.push_str(lines[index]);
        index += 1;
    }
    kept
}

#[test]
fn k_fsl5_sources_lower_to_the_lowered_kernel() {
    for &(example, source, lowering) in FSL5_SOURCES {
        let dir = example_dir(example);
        kernel_compare::assert_same_kernel(
            &kernel_compare::kernel_v2_fsl5(&dir.join(source)),
            &kernel_compare::kernel_v2(&dir.join(lowering)),
            &format!("{example}/{source} against {lowering}"),
        );
    }
}

/// A no-rules lowering differs from `lowered.fsl` by deleted `requires`
/// lines only: every other line of `lowered.fsl` is kept, in order, and every
/// deleted line is a `requires`, or the `@requirement` of a decision table the
/// reduced source leaves out (not under K, and a reduced source is not under
/// T).
#[test]
fn norules_lowerings_only_delete_requires_lines() {
    for &(example, _, lowering) in FSL5_SOURCES {
        if lowering == "lowered.fsl" {
            continue;
        }
        let dir = example_dir(example);
        let full = std::fs::read_to_string(dir.join("lowered.fsl")).expect("read lowered");
        let reduced = std::fs::read_to_string(dir.join(lowering)).expect("read lowering");
        let mut kept = reduced.lines().peekable();
        let mut deleted = 0;
        for line in full.lines() {
            if kept.peek() == Some(&line) {
                kept.next();
            } else {
                let line = line.trim_start();
                assert!(
                    line.starts_with("requires ") || line.starts_with("@requirement("),
                    "{example}/{lowering} deletes a line that is not a requires: {line:?}"
                );
                deleted += usize::from(line.starts_with("requires "));
            }
        }
        assert_eq!(
            kept.next(),
            None,
            "{example}/{lowering} adds or reorders lines"
        );
        assert!(deleted > 0, "{example}/{lowering} deletes nothing");
    }
}

/// The requirement IDs of every target of `file`'s kernel model that has
/// one (`action:submit`, `property:invariant:…`), from its annotations.
fn annotated_requirements(
    dir: &Path,
    file: &str,
    fsl5: bool,
) -> BTreeMap<String, BTreeSet<String>> {
    let source = std::fs::read_to_string(dir.join(file)).expect("read source");
    let options = if fsl5 {
        fsl_syntax::ParseOptions::with_unstable(
            fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
        )
    } else {
        fsl_syntax::ParseOptions::default()
    };
    let kernel = fsl_core::parse_kernel_source_with_file_with_options(
        &source,
        &fsl_core::FsResolver::new(dir.to_path_buf()),
        file,
        &options,
    )
    .unwrap_or_else(|error| panic!("lower {file}: {error:?}"));
    fsl_core::build_model(kernel)
        .unwrap_or_else(|error| panic!("model of {file}: {error:?}"))
        .requirement_targets()
        .into_iter()
        .map(|(target, links)| (target, links.into_iter().map(|link| link.id).collect()))
        .collect()
}

/// The `requirement` id of every action and property of a Public Kernel v2,
/// by model target (`action:submit`, `property:invariant:…`), `None` where it
/// has none.
fn kernel_requirements(kernel: &Value) -> BTreeMap<String, Option<String>> {
    let id = |item: &Value| item["requirement"]["id"].as_str().map(str::to_owned);
    let name = |item: &Value| item["name"].as_str().expect("name").to_owned();
    let mut requirements = BTreeMap::new();
    for action in kernel["actions"].as_array().expect("actions") {
        requirements.insert(format!("action:{}", name(action)), id(action));
    }
    for (group, kind) in [
        ("invariants", "invariant"),
        ("transitions", "trans"),
        ("reachables", "reachable"),
        ("leads_to", "leadsTo"),
    ] {
        for property in kernel["properties"][group].as_array().expect("properties") {
            requirements.insert(format!("property:{kind}:{}", name(property)), id(property));
        }
    }
    requirements
}

/// The rows of `fslc ledger <file>`'s risk table, one per requirement ID:
/// `(ID, status)`, in order. The whole-spec row (`（仕様全体）`) is left out:
/// it holds the findings of no requirement, such as the warnings only a
/// next-edition source has.
fn ledger_rows(dir: &Path, file: &str, fsl5: bool) -> Vec<(String, String)> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(["ledger", file])
        .current_dir(dir)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if fsl5 {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run fslc ledger");
    let ledger = String::from_utf8(output.stdout).expect("UTF-8 ledger");
    assert!(output.status.success(), "ledger {file}: {ledger}");
    let table = ledger
        .split_once("## リスク一覧")
        .and_then(|(_, rest)| rest.split("\n## ").next())
        .unwrap_or_else(|| panic!("ledger {file} has a risk table: {ledger}"));
    table
        .lines()
        .filter_map(|line| {
            let cells = line.split('|').map(str::trim).collect::<Vec<_>>();
            (cells.len() > 4 && cells[1] != "要件ID" && !cells[1].starts_with("---"))
                .then(|| (cells[1].to_owned(), cells[3].to_owned()))
        })
        .filter(|(id, _)| id != "（仕様全体）")
        .collect()
}

#[test]
fn t_fsl5_sources_attribute_the_requirements_of_the_lowered_kernel() {
    let mut checked = 0;
    for &(example, source, lowering) in FSL5_SOURCES {
        if lowering != "lowered.fsl" {
            continue;
        }
        let dir = example_dir(example);
        let annotated = annotated_requirements(&dir, source, true);
        assert_eq!(
            annotated,
            annotated_requirements(&dir, lowering, false),
            "{example}/{source}: requirement annotations against {lowering}"
        );
        // The kernel's one `requirement` of a target is one of its annotated
        // IDs; an action's is the one `lowered.fsl`'s requirement block gives
        // it. (A `trans` or an `@requirement` in `lowered.fsl` reaches the
        // annotations only, so properties are held to the first check.)
        let kernel = kernel_requirements(&kernel_compare::kernel_v2_fsl5(&dir.join(source)));
        for (target, id) in &kernel {
            if let Some(id) = id {
                assert!(
                    annotated.get(target).is_some_and(|ids| ids.contains(id)),
                    "{example}/{source}: {target} has requirement {id}, not annotated: {annotated:?}"
                );
            }
        }
        let actions = |requirements: BTreeMap<String, Option<String>>| {
            requirements
                .into_iter()
                .filter(|(target, _)| target.starts_with("action:"))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            actions(kernel),
            actions(kernel_requirements(&kernel_compare::kernel_v2(
                &dir.join(lowering)
            ))),
            "{example}/{source}: the actions' kernel `requirement` against {lowering}"
        );
        // The ledger lists the same requirements, each with the same status.
        let expected = ledger_rows(&dir, lowering, false);
        assert!(!expected.is_empty(), "{example}/{lowering}: no ledger rows");
        assert_eq!(
            ledger_rows(&dir, source, true),
            expected,
            "{example}/{source}: ledger rows against {lowering}"
        );
        checked += 1;
    }
    assert!(checked > 0, "not vacuous: a complete source is listed");
}

#[test]
fn every_fsl5_source_is_under_k() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fsl5");
    for example in std::fs::read_dir(&root).expect("read fixtures") {
        let example = example.expect("fixture entry").path();
        let example_name = example
            .file_name()
            .expect("example name")
            .to_string_lossy()
            .into_owned();
        for entry in std::fs::read_dir(&example).expect("read example") {
            let name = entry
                .expect("example entry")
                .file_name()
                .to_string_lossy()
                .into_owned();
            if name.starts_with("fsl5_") {
                assert!(
                    FSL5_SOURCES
                        .iter()
                        .any(|&(listed, source, _)| (listed, source)
                            == (example_name.as_str(), name.as_str())),
                    "{example_name}/{name} is a next-edition source without a K check"
                );
            }
        }
    }
}
