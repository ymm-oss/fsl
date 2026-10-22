// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! What `mutate --by-requirement`, `lint` and `explain --readable` report
//! for a requirements document headed `fsl 5`, behind the `fsl5` gate.
//!
//! - `mutate --by-requirement`: a rule or a decision table whose statements
//!   are guards is formalized by those guards, so each killed mutant of such
//!   a guard counts for the rule's or table's ID. The note `by <actor>` the
//!   lowering gives an event's action names the actor; it is not a
//!   requirement ID.
//! - `lint`: the ID policy applies to the ID every declaration of the
//!   document starts with (`rule`, `decide`, `covers`, `scenario`,
//!   `forbidden`, `goal`, `policy`), in English or Japanese keywords.
//! - `explain --readable` of a document with a glossary shows the aliases
//!   wherever a name is shown: in each `requires`, in a property's body and
//!   in an event's actor note, as everywhere else.
//!
//! The fixtures are the returns example of the next skills, in English and
//! in Japanese with a glossary (`fixtures/fsl5/outputs`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fsl5/outputs")
        .join(name)
}

/// `(stdout, exit)` of `fslc <args...> <path>` behind the gate, run in
/// `path`'s directory.
fn fslc(args: &[&str], path: &Path) -> (String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .arg(path.file_name().expect("file name"))
        .current_dir(path.parent().expect("directory"))
        .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
        .output()
        .expect("run fslc");
    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        output.status.code().expect("exit code"),
    )
}

fn json(args: &[&str], path: &Path) -> Value {
    let (stdout, _) = fslc(args, path);
    serde_json::from_str(&stdout).unwrap_or_else(|error| panic!("{args:?}: {error}: {stdout}"))
}

/// A copy of fixture `name` with each `(from, to)` replaced exactly once,
/// written to a scratch file.
fn edited_copy(name: &str, edits: &[(&str, &str)]) -> (PathBuf, String) {
    let mut text = std::fs::read_to_string(fixture(name)).expect("fixture");
    for (from, to) in edits {
        assert_eq!(text.matches(from).count(), 1, "{name}: {from}");
        text = text.replacen(from, to, 1);
    }
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fsl5_report_outputs");
    std::fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join(name);
    std::fs::write(&path, &text).expect("write scratch copy");
    (path, text)
}

/// The rule and table guards of `explain --lowering`: each guard's mutant
/// target (`request requires #2`) with the ID of the rule or table it came
/// from.
fn rule_and_table_guards(path: &Path) -> Vec<(String, String)> {
    let explained = json(&["explain", "--lowering"], path);
    let mut guards = Vec::new();
    for action in explained["lowering"]["actions"]
        .as_array()
        .expect("actions")
    {
        let name = action["action"].as_str().expect("action name");
        for guard in action["requires"].as_array().expect("requires") {
            let index = guard["index"].as_u64().expect("index");
            for origin in guard["origins"].as_array().expect("origins") {
                if ["allow", "never", "after", "decide"].contains(&origin["kind"].as_str().unwrap())
                {
                    let id = origin["detail"]
                        .as_str()
                        .and_then(|detail| detail.split(' ').next())
                        .expect("origin detail starts with its ID");
                    guards.push((format!("{name} requires #{}", index + 1), id.to_owned()));
                }
            }
        }
    }
    guards
}

#[test]
fn mutate_by_requirement_credits_rule_and_table_ids_with_their_guard_kills() {
    let path = fixture("returns_en.fsl");
    let guards = rule_and_table_guards(&path);
    let ids = guards
        .iter()
        .map(|(_, id)| id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids,
        BTreeSet::from(["REQ-RET-001", "REQ-RET-002"]),
        "{guards:?}"
    );
    let mutated = json(&["mutate", "--by-requirement", "--depth", "3"], &path);
    assert_eq!(mutated["result"], "mutated", "{mutated}");
    // Neither ID is carried by a property or named by a trace of this
    // document, so its kills are exactly the killed mutants of its guards.
    let mut expected = BTreeMap::<&str, u64>::new();
    for mutant in mutated["mutants"].as_array().expect("mutants") {
        let target = mutant["target"].as_str().expect("target");
        for (guard, id) in &guards {
            let of_guard = target
                .strip_prefix(guard.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '));
            if of_guard && mutant["status"] == "killed" {
                *expected.entry(id.as_str()).or_default() += 1;
            }
        }
    }
    let by_requirement = mutated["by_requirement"]
        .as_object()
        .expect("by_requirement");
    for id in ids {
        let kills = expected.get(id).copied().unwrap_or_default();
        assert!(kills > 0, "{id}: no guard mutant of it is killed");
        assert_eq!(
            by_requirement[id],
            serde_json::json!({"kills": kills}),
            "{id}: {by_requirement:?}"
        );
    }
    // An event's actor note (`request: by Customer`) is not a requirement.
    let explained = json(&["explain", "--lowering"], &path);
    let events = explained["lowering"]["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .map(|action| action["action"].as_str().expect("action name"))
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 5, "{events:?}");
    for event in events {
        assert!(
            !by_requirement.contains_key(event),
            "{event}: {by_requirement:?}"
        );
    }
}

/// Non-canonical IDs for every ID-bearing declaration of the fixtures, with
/// the ID kind `lint` names.
const NON_CANONICAL: &[(&str, &str, &str)] = &[
    ("REQ-RET-001", "REQ-1", "requirement"),
    ("REQ-RET-002", "REQ-2", "requirement"),
    ("REQ-RET-003", "REQ-3", "requirement"),
    ("AC-RET-001", "AC-1", "acceptance"),
    ("FB-RET-001", "FB-1", "forbidden"),
    ("GOAL-RET-001", "GOAL-1", "goal"),
    ("POL-RET-001", "POL-1", "policy"),
];

#[test]
fn lint_applies_the_id_policy_to_every_declared_id_in_either_spelling() {
    for name in ["returns_en.fsl", "returns_ja.fsl"] {
        let clean = json(&["lint"], &fixture(name));
        assert_eq!(clean["finding_count"], 0, "{name}: {clean}");
        let edits = NON_CANONICAL
            .iter()
            .map(|(from, to, _)| (*from, *to))
            .collect::<Vec<_>>();
        let (path, text) = edited_copy(name, &edits);
        let expected = NON_CANONICAL
            .iter()
            .map(|(_, id, kind)| {
                let line = text
                    .lines()
                    .position(|line| line.contains(&format!(" {id} ")))
                    .expect("edited line");
                (
                    format!("{kind} ID '{id}' does not match the active ID policy"),
                    u64::try_from(line + 1).expect("line"),
                )
            })
            .collect::<BTreeSet<_>>();
        let linted = json(&["lint"], &path);
        let findings = linted["files"][0]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .filter(|finding| finding["code"] == "non_canonical_id")
            .map(|finding| {
                (
                    finding["message"].as_str().expect("message").to_owned(),
                    finding["loc"]["line"].as_u64().expect("line"),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(findings.len(), expected.len(), "{name}: {findings:?}");
        assert_eq!(
            findings.into_iter().collect::<BTreeSet<_>>(),
            expected,
            "{name}"
        );
    }
}

/// The canonical names of the fixture's glossary that have an alias, and
/// its quoted strings (the author's wording).
fn glossary_and_wording(source: &str) -> (BTreeSet<String>, Vec<String>) {
    let block = source
        .split_once("用語集 {")
        .and_then(|(_, rest)| rest.split_once('}'))
        .expect("glossary block")
        .0;
    let aliased = block
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(canonical, _)| canonical.trim())
        .filter(|canonical| !canonical.contains('.'))
        .map(str::to_owned)
        .collect();
    let wording = source
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    (aliased, wording)
}

/// The words of `text` that are canonical names with an alias, outside the
/// author's `wording`.
fn canonical_words(text: &str, aliased: &BTreeSet<String>, wording: &[String]) -> Vec<String> {
    let mut text = text.to_owned();
    for kept in wording {
        text = text.replace(kept.as_str(), " ");
    }
    text.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|word| aliased.contains(*word))
        .map(str::to_owned)
        .collect()
}

/// Every string and key of `value`, except the `reason` / `recommended_action`
/// prose.
fn strings(value: &Value, key: Option<&str>, found: &mut Vec<String>) {
    match value {
        Value::String(text)
            if !key.is_some_and(|key| ["reason", "recommended_action"].contains(&key)) =>
        {
            found.push(text.clone());
        }
        Value::Array(items) => items.iter().for_each(|item| strings(item, key, found)),
        Value::Object(object) => {
            for (key, item) in object {
                found.push(key.clone());
                strings(item, Some(key), found);
            }
        }
        _ => {}
    }
}

#[test]
fn explain_with_a_glossary_shows_aliases_in_guards_bodies_and_actor_notes() {
    let path = fixture("returns_ja.fsl");
    let source = std::fs::read_to_string(&path).expect("fixture");
    let (aliased, wording) = glossary_and_wording(&source);
    assert!(
        aliased.contains("Draft") && aliased.contains("Customer"),
        "{aliased:?}"
    );

    let (readable, exit) = fslc(&["explain", "--readable"], &path);
    assert_eq!(exit, 0, "{readable}");
    assert_eq!(
        canonical_words(&readable, &aliased, &wording),
        Vec::<String>::new(),
        "{readable}"
    );
    for line in [
        "requirement: request: by 顧客",
        "- requires return_stage[r] == 下書き  ← move 下書き -> 申請済み",
        "body: forall r: 返品 { return_stage[r] != 下書き => return_amount[r] > 0 }",
    ] {
        assert!(readable.contains(line), "{line}: {readable}");
    }

    let mut found = Vec::new();
    strings(&json(&["explain"], &path), None, &mut found);
    let canonical = found
        .iter()
        .flat_map(|text| canonical_words(text, &aliased, &wording))
        .collect::<Vec<_>>();
    assert_eq!(canonical, Vec::<String>::new(), "explain JSON");

    // `--names canonical` shows the canonical names in the same places.
    let (readable, exit) = fslc(&["explain", "--readable", "--names", "canonical"], &path);
    assert_eq!(exit, 0, "{readable}");
    for line in [
        "requirement: request: by Customer",
        "- requires return_stage[r] == Draft  ← move Draft -> Requested",
        "body: forall r: Return { return_stage[r] != Draft => return_amount[r] > 0 }",
    ] {
        assert!(readable.contains(line), "{line}: {readable}");
    }
}
