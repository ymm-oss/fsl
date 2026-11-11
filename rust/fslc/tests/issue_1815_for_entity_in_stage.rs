// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1815: a stage-qualified entity after `for` — `decide ID "text" for …`,
//! `allow P only for …`, `never P for …` — may be written `Entity in Stage`
//! (`申請 段階 提出済み`) beside `Stage Entity`, as `always E in S has X` and
//! `count E in S` write it. Both orders lower to the same kernel and the same
//! lowering text, and `fmt` keeps the order each was written in. `in` names
//! one stage, as `Stage Entity` does, and stands on the entity's line: a rule
//! statement starts on a new line.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "support/kernel_compare.rs"]
mod kernel_compare;

fn e2e() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fsl5/e2e")
}

/// A scratch directory holding a copy of the e2e example (its `implements`
/// dependency `1_business.fsl` included).
fn scratch(label: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1815")
        .join(label);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    for entry in std::fs::read_dir(e2e()).expect("read e2e") {
        let path = entry.expect("e2e entry").path();
        std::fs::copy(&path, directory.join(path.file_name().expect("file name")))
            .expect("copy e2e");
    }
    directory
}

/// `source` with `old`, which occurs exactly once, replaced by `new`.
fn edit(source: &str, old: &str, new: &str) -> String {
    assert_eq!(source.matches(old).count(), 1, "{old:?}");
    source.replacen(old, new, 1)
}

/// The exit and stdout of `fslc <args>` in `directory`.
fn fslc(directory: &Path, args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(directory)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    (
        output.status.code().expect("exit code"),
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
    )
}

fn check(directory: &Path, file: &str) -> (i32, Value) {
    let (exit, stdout) = fslc(directory, &["check", file]);
    let report = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("check {file}: {error}: {stdout}"));
    (exit, report)
}

fn assert_checks(directory: &Path, file: &str) {
    let (exit, report) = check(directory, file);
    assert_eq!(
        (exit, report["result"].as_str()),
        (0, Some("ok")),
        "{file}: {report:#}"
    );
}

/// Every lowering step of the kernel's provenance, `(kind, detail)`, sorted:
/// the text each guard and property was lowered from, which
/// [`kernel_compare`] leaves out with the rest of the provenance.
fn lowering_steps(kernel: &Value) -> Vec<(String, String)> {
    let mut steps = kernel["provenance"]["origins"]
        .as_array()
        .expect("provenance origins")
        .iter()
        .flat_map(|origin| {
            origin["lowering_steps"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|step| {
            (
                step["kind"].as_str().expect("step kind").to_owned(),
                step["detail"].as_str().expect("step detail").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    steps.sort();
    steps
}

/// `stage_first` and `entity_first`, written in `directory`, both check and
/// lower to the same kernel with the same lowering text.
fn assert_same_lowering(directory: &Path, stage_first: &str, entity_first: &str, context: &str) {
    std::fs::write(directory.join("stage_first.fsl"), stage_first).expect("write stage_first");
    std::fs::write(directory.join("entity_first.fsl"), entity_first).expect("write entity_first");
    assert_checks(directory, "stage_first.fsl");
    assert_checks(directory, "entity_first.fsl");
    let left = kernel_compare::kernel_v2(&directory.join("stage_first.fsl"));
    let right = kernel_compare::kernel_v2(&directory.join("entity_first.fsl"));
    kernel_compare::assert_same_kernel(&left, &right, context);
    assert_eq!(lowering_steps(&left), lowering_steps(&right), "{context}");
}

/// The e2e example's decision table in each of its three spellings, headed
/// `for Claim in Submitted` / `for 申請 in 提出済み` / `対象 申請 段階 提出済み`.
#[test]
fn a_decision_table_for_entity_in_stage_is_the_table_for_stage_entity() {
    for (file, stage_first, entity_first) in [
        (
            "fsl5_en.fsl",
            "for Submitted Claim {",
            "for Claim in Submitted {",
        ),
        (
            "fsl5_ja_particles.fsl",
            "for 提出済み 申請 {",
            "for 申請 in 提出済み {",
        ),
        (
            "fsl5_ja_full.fsl",
            "対象 提出済み 申請 {",
            "対象 申請 段階 提出済み {",
        ),
    ] {
        let directory = scratch(&format!("decide-{file}"));
        let source = std::fs::read_to_string(e2e().join(file)).expect("read fixture");
        let entity_first = edit(&source, stage_first, entity_first);
        assert_same_lowering(&directory, &source, &entity_first, file);
    }
}

/// A document whose `for` positions are filled in by `stage_entity` from
/// `(stage, entity)`; English keywords when `japanese` is false.
fn rules_document(japanese: bool, stage_entity: impl Fn(&str, &str) -> String) -> String {
    let placed = stage_entity("Placed", "Order");
    let packed = stage_entity("Packed", "Order");
    let kw = |english: &'static str, japanese_spelling: &'static str| {
        if japanese { japanese_spelling } else { english }
    };
    let requirements = kw("requirements", "要件");
    let decide = kw("decide", "決定表");
    let decide_for = kw("for", "対象");
    let otherwise = kw("otherwise => Shop packs", "それ以外 => Shop が packs");
    let rule = kw("rule", "規則");
    let allow = kw(
        "allow Shop touches Order only for",
        "許可 Shop が Order を touches 対象",
    );
    let never = kw(
        "never Shop touches Order for",
        "禁止 Shop が Order を touches 対象",
    );
    let and = kw("and", "かつ");
    format!(
        "fsl 5
{requirements} R {{
  number Qty
  actor Shop
  entity Order {{
    qty: Qty = 0
    lifecycle Placed -> Packed
  }}
  concept Rush = Order where qty > 0
  event pack = Shop packs Order Placed -> Packed
  event touch = Shop touches Order in Placed
  {decide} REQ-1 \"who packs\" {decide_for} {placed} {{ {otherwise} }}
  {rule} R1 \"t\" {{
    {allow} Rush {and} {placed}
    {allow} {placed} {and} Rush
    {never} {packed}
  }}
}}
verify {{
  instances Order = 1
  values Qty = 0..1
}}
"
    )
}

/// `rules_document` in English with `Order in Placed` and `Packed Order`.
fn placed_entity_first() -> String {
    rules_document(false, |stage, entity| {
        if stage == "Placed" {
            format!("{entity} in {stage}")
        } else {
            format!("{stage} {entity}")
        }
    })
}

/// `allow … only for`, `never … for` and `decide … for`, in a conjunction
/// on either side of `and`, in English and in Japanese keywords.
#[test]
fn every_for_takes_entity_in_stage_with_the_same_lowering() {
    for (japanese, label, word) in [(false, "en", "in"), (true, "ja", "段階")] {
        let directory = scratch(&format!("rules-{label}"));
        let stage_first = rules_document(japanese, |stage, entity| format!("{stage} {entity}"));
        let entity_first =
            rules_document(japanese, |stage, entity| format!("{entity} {word} {stage}"));
        assert_ne!(stage_first, entity_first);
        assert_same_lowering(&directory, &stage_first, &entity_first, label);
    }
}

/// `fmt` keeps the order written, and `--lang` translates the `in` of each
/// `for`: `in` and `段階` round-trip.
#[test]
fn fmt_keeps_the_order_written_and_translates_in() {
    let directory = scratch("fmt");
    let source = edit(
        &placed_entity_first(),
        "only for Rush and Order in Placed",
        "only for Rush and Placed Order",
    );
    std::fs::write(directory.join("mixed.fsl"), &source).expect("write mixed");
    let (exit, formatted) = fslc(&directory, &["fmt", "mixed.fsl"]);
    assert_eq!(exit, 0, "{formatted}");
    for line in [
        "decide REQ-1 \"who packs\" for Order in Placed {",
        "allow Shop touches Order only for Rush and Placed Order",
        "allow Shop touches Order only for Order in Placed and Rush",
        "never Shop touches Order for Packed Order",
    ] {
        assert!(formatted.contains(line), "{line}\n{formatted}");
    }
    std::fs::write(directory.join("formatted.fsl"), &formatted).expect("write formatted");
    let (exit, again) = fslc(&directory, &["fmt", "formatted.fsl"]);
    assert_eq!((exit, &again), (0, &formatted), "fmt is idempotent");

    let (exit, japanese) = fslc(&directory, &["fmt", "formatted.fsl", "--lang", "ja"]);
    assert_eq!(exit, 0, "{japanese}");
    for line in [
        "決定表 REQ-1 \"who packs\" 対象 Order 段階 Placed {",
        "許可 Shop が Order を touches 対象 Rush かつ Placed Order",
        "許可 Shop が Order を touches 対象 Order 段階 Placed かつ Rush",
        "禁止 Shop が Order を touches 対象 Packed Order",
    ] {
        assert!(japanese.contains(line), "{line}\n{japanese}");
    }
    std::fs::write(directory.join("japanese.fsl"), &japanese).expect("write japanese");
    let (exit, english) = fslc(&directory, &["fmt", "japanese.fsl", "--lang", "en"]);
    assert_eq!((exit, &english), (0, &formatted), "--lang ja then en");
}

/// `in` names one stage after `for`, as `Stage Entity` does: a list is
/// refused at its comma, naming the order that has one stage.
#[test]
fn entity_in_stage_after_for_names_one_stage() {
    let directory = scratch("one-stage");
    for (label, old, new) in [
        (
            "decide",
            "for Order in Placed {",
            "for Order in Placed, Packed {",
        ),
        (
            "allow",
            "only for Order in Placed and Rush",
            "only for Order in Placed, Packed and Rush",
        ),
    ] {
        let broken = edit(&placed_entity_first(), old, new);
        let file = format!("{label}.fsl");
        std::fs::write(directory.join(&file), &broken).expect("write broken");
        let (exit, report) = check(&directory, &file);
        assert_eq!(exit, 2, "{label}: {report:#}");
        assert_eq!(
            report["message"]
                .as_str()
                .and_then(|message| message.split(" at ").next()),
            Some("`Order in Placed` after `for` names one stage, as `Placed Order` does"),
            "{label}: {report:#}"
        );
        let line = broken[..broken.find(new).expect("the edit")]
            .matches('\n')
            .count()
            + 1;
        let column = new.find(',').expect("the comma")
            + broken
                .lines()
                .nth(line - 1)
                .expect("the line")
                .find(new)
                .expect("on its line")
            + 1;
        assert_eq!(
            (
                report["loc"]["line"].as_u64(),
                report["loc"]["column"].as_u64()
            ),
            (Some(line as u64), Some(column as u64)),
            "{label}: {report:#}"
        );
    }
}

/// The `in` of `Entity in Stage` stands on the entity's line, as the entity
/// of `Stage Entity` stands on the stage's: a statement on the next line that
/// starts with a word spelled `in` / `段階` (a property named so) is a
/// statement of its own, and `in Stage` on the next line is not part of the
/// condition. The stage that an `in` requires may follow on the next line, as
/// the term an `and` requires may.
#[test]
fn the_in_stands_on_the_entitys_line() {
    let directory = scratch("line");
    for name in ["in", "段階"] {
        let source = rules_document(false, |stage, entity| format!("{stage} {entity}"));
        let next_line = edit(
            &source,
            "    never Shop touches Order for Packed Order\n",
            &format!(
                "    never Shop packs Order for Rush\n    {name}: always Order in Placed has qty >= 0\n"
            ),
        );
        let file = format!("named-{}.fsl", if name == "in" { "in" } else { "ja" });
        std::fs::write(directory.join(&file), &next_line).expect("write named");
        assert_checks(&directory, &file);
    }

    let source = rules_document(false, |stage, entity| format!("{stage} {entity}"));
    let split = edit(
        &source,
        "    never Shop touches Order for Packed Order\n",
        "    never Shop touches Order for Order\n      in Packed\n",
    );
    std::fs::write(directory.join("split.fsl"), &split).expect("write split");
    let (exit, report) = check(&directory, "split.fsl");
    assert_eq!(
        (exit, report["kind"].as_str()),
        (2, Some("parse")),
        "{report:#}"
    );

    let wrapped = edit(
        &source,
        "    never Shop touches Order for Packed Order\n",
        "    never Shop touches Order for Order in\n      Packed\n",
    );
    assert_same_lowering(&directory, &source, &wrapped, "stage after `in` wrapped");
}
