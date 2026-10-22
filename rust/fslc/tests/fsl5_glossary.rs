// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S3, through the native CLI: a requirements document's
//! `glossary` gives canonical (English) names aliases, the aliases are
//! resolved to the canonical names before lowering, and implementations see
//! the canonical names only.
//!
//! The fixtures under `tests/fixtures/fsl5_glossary/<example>/` are
//! examples of `tests/fixtures/fsl5/` written in aliases: the e2e example in
//! the next edition's entity / event / scenario form, the rejection example in
//! the requirement / action form both editions read:
//!
//! - acceptance 1 (K): each lowers to exactly the kernel of its canonical
//!   twin;
//! - acceptance 2: `kernel`, `testgen` and `explain --names canonical` print
//!   only ASCII names (string literals, the requirement wording, aside), for
//!   these fixtures and for the e2e example in Japanese keywords and in
//!   particles ([`JAPANESE_KEYWORD_SOURCES`]), whose rule and decision-row
//!   lowering steps are recorded in canonical names and English keywords;
//! - acceptance 3: each glossary diagnostic, and an alias inside
//!   `implements`, fails with its code at the offending text.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "support/kernel_compare.rs"]
mod kernel_compare;

/// A glossary source and its canonical twin, the same model in canonical
/// names: `fsl5/<example>/<twin>`, read behind the gate when `twin_gated`.
struct Source {
    example: &'static str,
    name: &'static str,
    twin: &'static str,
    twin_gated: bool,
}

/// The entity / event / scenario fixture in Japanese aliases, which
/// `aliases_resolve_inside_entity_event_and_scenario` covers.
const ENTITY_FORM: Source = Source {
    example: "e2e",
    name: "glossary_ja.fsl",
    twin: "fsl5_en_norules.fsl",
    twin_gated: true,
};

const SOURCES: &[Source] = &[
    Source {
        example: "e2e",
        name: "glossary_ascii.fsl",
        twin: "fsl5_en_norules.fsl",
        twin_gated: true,
    },
    Source {
        example: "rejection",
        name: "glossary_ja.fsl",
        twin: "original.fsl",
        twin_gated: false,
    },
];

/// The e2e example in the keyword table's Japanese keywords and in
/// particles, both with the glossary's Japanese aliases
/// (`tests/fixtures/fsl5/e2e/`). They lower to `lowered.fsl`
/// (`fsl5_lowering_contract.rs`); here only their outputs are scanned.
const JAPANESE_KEYWORD_SOURCES: &[&str] = &["fsl5_ja_full.fsl", "fsl5_ja_particles.fsl"];

impl Source {
    fn path(&self) -> PathBuf {
        source(self.example, self.name)
    }

    fn twin_path(&self) -> PathBuf {
        fixtures().join("fsl5").join(self.example).join(self.twin)
    }

    fn context(&self) -> String {
        format!("{}/{}", self.example, self.name)
    }
}

/// Keys whose values are the author's prose, which may be in any language.
const PROSE_KEYS: &[&str] = &["text", "reason", "recommended_action"];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn source(example: &str, name: &str) -> PathBuf {
    fixtures().join("fsl5_glossary").join(example).join(name)
}

/// `(stdout, exit)` of `fslc <args...> <path>` run in `path`'s directory.
fn fslc(args: &[&str], path: &Path, gate: bool) -> (String, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(args)
        .arg(path.file_name().expect("file name"))
        .current_dir(path.parent().expect("directory"))
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if gate {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run fslc");
    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        output.status.code().expect("exit code"),
    )
}

fn json(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|error| panic!("{error}: {stdout}"))
}

/// Every object key, and every string outside [`PROSE_KEYS`], that is not
/// ASCII.
fn non_ascii_names(value: &Value) -> Vec<String> {
    fn walk(value: &Value, prose: bool, found: &mut Vec<String>) {
        match value {
            Value::String(text) if !prose && !text.is_ascii() => found.push(text.clone()),
            Value::Array(items) => items.iter().for_each(|item| walk(item, prose, found)),
            Value::Object(object) => {
                for (key, item) in object {
                    if !key.is_ascii() {
                        found.push(key.clone());
                    }
                    walk(item, PROSE_KEYS.contains(&key.as_str()), found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(value, false, &mut found);
    found
}

/// The prose strings of an envelope (see [`PROSE_KEYS`]).
fn prose(value: &Value) -> Vec<String> {
    fn walk(value: &Value, prose: bool, found: &mut Vec<String>) {
        match value {
            Value::String(text) if prose => found.push(text.clone()),
            Value::Array(items) => items.iter().for_each(|item| walk(item, prose, found)),
            Value::Object(object) => object
                .iter()
                .for_each(|(key, item)| walk(item, PROSE_KEYS.contains(&key.as_str()), found)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    walk(value, false, &mut found);
    found
}

/// The non-ASCII identifiers of program or report text: maximal runs of
/// letters, digits and `_`, outside each of `prose`. String literals are
/// scanned too: generated code passes action and state names as strings.
fn non_ascii_identifiers(text: &str, prose: &[String]) -> Vec<String> {
    let mut text = text.to_owned();
    for kept in prose {
        text = text.replace(kept.as_str(), " ");
    }
    text.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|word| !word.is_ascii())
        .map(str::to_owned)
        .collect()
}

/// A copy of `e2e/glossary_ja.fsl` with `from` replaced by `to` (once),
/// written to a scratch file; its `implements` dependency is the fixture's.
fn broken_copy(label: &str, from: &str, to: &str) -> (PathBuf, String) {
    let original = std::fs::read_to_string(source("e2e", "glossary_ja.fsl")).expect("fixture");
    assert_eq!(original.matches(from).count(), 1, "{from}");
    let business = fixtures().join("fsl5/e2e/1_business.fsl");
    let text = original
        .replacen(from, to, 1)
        .replace("../../fsl5/e2e/1_business.fsl", &business.to_string_lossy());
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fsl5_glossary");
    std::fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join(format!("{label}.fsl"));
    std::fs::write(&path, &text).expect("write scratch source");
    (path, text)
}

/// K: the source checks, and lowers to the kernel its canonical twin lowers
/// to, names and all.
fn assert_canonical_kernel(source: &Source) {
    let (check, exit) = fslc(&["check"], &source.path(), true);
    assert_eq!(exit, 0, "{}: {check}", source.context());
    let twin = if source.twin_gated {
        kernel_compare::kernel_v2_fsl5(&source.twin_path())
    } else {
        kernel_compare::kernel_v2(&source.twin_path())
    };
    kernel_compare::assert_same_kernel(
        &kernel_compare::kernel_v2_fsl5(&source.path()),
        &twin,
        &source.context(),
    );
}

/// Acceptance 1 (K): written in aliases, each example lowers to the kernel
/// of its canonical twin.
#[test]
fn k_glossary_sources_lower_to_the_canonical_kernel() {
    for source in SOURCES {
        assert_canonical_kernel(source);
    }
}

/// Acceptance 2 for one source: the outputs an implementation consumes
/// carry canonical names only. For a source in Japanese aliases, the same
/// scan finds the aliases in `explain`'s default rendering, so it is not
/// vacuous.
fn assert_ascii_outputs(path: &Path, context: &str, japanese: bool) {
    for version in ["1", "2"] {
        let (stdout, exit) = fslc(&["kernel", "--kernel-version", version], path, true);
        assert_eq!(exit, 0, "{context}: kernel v{version}: {stdout}");
        assert_eq!(
            non_ascii_names(&json(&stdout)),
            Vec::<String>::new(),
            "{context}: kernel v{version}"
        );
    }
    for target in ["pytest", "vitest", "swift", "kotlin", "dart", "phpunit"] {
        let (stdout, exit) = fslc(&["testgen", "--target", target], path, true);
        assert_eq!(exit, 0, "{context}: testgen {target}: {stdout}");
        assert_eq!(
            non_ascii_identifiers(&stdout, &[]),
            Vec::<String>::new(),
            "{context}: testgen {target}"
        );
    }
    let (stdout, exit) = fslc(&["explain", "--names", "canonical"], path, true);
    assert_eq!(exit, 0, "{context}: explain: {stdout}");
    let explained = json(&stdout);
    assert_eq!(
        non_ascii_names(&explained),
        Vec::<String>::new(),
        "{context}: explain --names canonical"
    );
    let wording = prose(&explained);
    let (readable, exit) = fslc(
        &["explain", "--readable", "--names", "canonical"],
        path,
        true,
    );
    assert_eq!(exit, 0, "{context}: explain --readable: {readable}");
    assert_eq!(
        non_ascii_identifiers(&readable, &wording),
        Vec::<String>::new(),
        "{context}: explain --readable --names canonical"
    );
    if japanese {
        let (aliased, _) = fslc(&["explain"], path, true);
        assert!(
            !non_ascii_names(&json(&aliased)).is_empty(),
            "{context}: explain shows aliases by default"
        );
        let (aliased, _) = fslc(&["explain", "--readable"], path, true);
        assert!(
            !non_ascii_identifiers(&aliased, &wording).is_empty(),
            "{context}: explain --readable shows aliases by default"
        );
    }
}

/// Acceptance 2: the outputs an implementation consumes carry canonical
/// names only.
#[test]
fn implementations_see_only_ascii_canonical_names() {
    for source in SOURCES {
        assert_ascii_outputs(
            &source.path(),
            &source.context(),
            source.name.ends_with("_ja.fsl"),
        );
    }
    for name in JAPANESE_KEYWORD_SOURCES {
        assert_ascii_outputs(
            &fixtures().join("fsl5/e2e").join(name),
            &format!("fsl5/e2e/{name}"),
            true,
        );
    }
}

/// Aliases written in the next edition's entity, lifecycle, concept, event
/// phrase, event input and scenario step resolve before they are desugared:
/// the document lowers to the kernel of its canonical twin (K), the outputs
/// an implementation consumes are ASCII, and the `event` lowering step that
/// records each phrase (an origin, outside K) names canonical words too.
#[test]
fn aliases_resolve_inside_entity_event_and_scenario() {
    let source = ENTITY_FORM;
    assert_canonical_kernel(&source);
    assert_ascii_outputs(&source.path(), &source.context(), true);
    let kernel = kernel_compare::kernel_v2_fsl5(&source.path());
    let mut phrases = Vec::new();
    for origin in kernel["provenance"]["origins"].as_array().expect("origins") {
        for step in origin["lowering_steps"].as_array().into_iter().flatten() {
            if step["kind"] == "event" {
                phrases.push(step["detail"].as_str().expect("event detail").to_owned());
            }
        }
    }
    assert!(
        phrases.contains(&"Employee submits Claim Draft -> Submitted".to_owned()),
        "{phrases:?}"
    );
    assert_eq!(phrases.len(), 5, "{phrases:?}");
    assert!(
        phrases.iter().all(|phrase| phrase.is_ascii()),
        "{phrases:?}"
    );
}

/// `explain --names canonical` of a glossary document is `explain` of its
/// canonical twin, apart from the requirement wording, source positions and
/// origins (the two files differ in layout) and timings.
#[test]
fn explain_names_canonical_is_the_canonical_explanation() {
    fn comparable(mut value: Value) -> Value {
        fn strip(value: &mut Value) {
            match value {
                Value::Array(items) => items.iter_mut().for_each(strip),
                Value::Object(object) => {
                    object.retain(|key, _| {
                        !PROSE_KEYS.contains(&key.as_str())
                            && !["loc", "origin", "cost"].contains(&key.as_str())
                    });
                    object.values_mut().for_each(strip);
                }
                _ => {}
            }
        }
        strip(&mut value);
        value
    }
    for source in SOURCES.iter().chain([&ENTITY_FORM]) {
        let (canonical, _) = fslc(&["explain", "--names", "canonical"], &source.path(), true);
        let (twin, _) = fslc(&["explain"], &source.twin_path(), source.twin_gated);
        assert_eq!(
            comparable(json(&canonical)),
            comparable(json(&twin)),
            "{}",
            source.context()
        );
    }
}

/// One way to break `e2e/glossary_ja.fsl`: replace `from` by `to` (exactly
/// once), and expect `code` at the `occurrence`-th match of `at` in the
/// broken source.
struct Broken {
    code: &'static str,
    from: &'static str,
    to: &'static str,
    at: &'static str,
    occurrence: usize,
}

const BROKEN: &[Broken] = &[
    Broken {
        code: "alias_collision",
        from: "    Finance      = 経理\n",
        to: "    Finance      = 申請\n",
        at: "申請\n",
        occurrence: 1,
    },
    Broken {
        code: "duplicate_alias",
        from: "    Claim        = 申請\n",
        to: "    Claim        = 申請\n    Claim        = 請求\n",
        at: "Claim        = 請求",
        occurrence: 0,
    },
    Broken {
        code: "alias_shadows_name",
        from: "    Finance      = 経理\n",
        to: "    Finance      = Paid\n",
        at: "Paid\n",
        occurrence: 0,
    },
    // A name the lowering declares (the generated stage map) is a canonical
    // name too, although the source never writes it.
    Broken {
        code: "alias_shadows_name",
        from: "    Finance      = 経理\n",
        to: "    Finance      = 経理\n    Ledger       = claim_stage\n",
        at: "claim_stage",
        occurrence: 0,
    },
    Broken {
        code: "alias_shadows_keyword",
        from: "    Finance      = 経理\n",
        to: "    Finance      = 条件\n",
        at: "条件",
        occurrence: 0,
    },
    Broken {
        code: "alias_shadows_keyword",
        from: "    Finance      = 経理\n",
        to: "    Finance      = state\n",
        at: "state",
        occurrence: 0,
    },
    Broken {
        code: "canonical_name_not_ascii",
        from: "    Finance      = 経理\n",
        to: "    財務         = 経理\n",
        at: "財務",
        occurrence: 0,
    },
    // A name with no glossary entry is canonical, so it must be ASCII.
    Broken {
        code: "canonical_name_not_ascii",
        from: "    Finance      = 経理\n",
        to: "",
        at: "経理",
        occurrence: 0,
    },
    Broken {
        code: "alias_in_implements",
        from: "    action submit(c, amount) -> submit(c)\n",
        to: "    action 提出(c, amount) -> submit(c)\n",
        at: "提出(c, amount)",
        occurrence: 0,
    },
    Broken {
        code: "misplaced_glossary",
        from: "  number 金額\n",
        to: "  number 金額\n  glossary { Finance = 財務 }\n",
        at: "glossary { Finance",
        occurrence: 0,
    },
];

/// `(line, column)` of the `occurrence`-th match of `at`, as diagnostics
/// count them (1-based, Unicode scalar columns).
fn position(source: &str, at: &str, occurrence: usize) -> (u64, u64) {
    let (offset, _) = source
        .match_indices(at)
        .nth(occurrence)
        .unwrap_or_else(|| panic!("'{at}' #{occurrence} in the broken source"));
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .expect("line prefix")
        .chars()
        .count()
        + 1;
    (line as u64, column as u64)
}

/// Acceptance 3: each diagnostic fails `check` with its own code, at the
/// text it names.
#[test]
fn each_glossary_diagnostic_fails_check_at_its_text() {
    for (index, broken) in BROKEN.iter().enumerate() {
        let (path, text) = broken_copy(
            &format!("broken-{index}-{}", broken.code),
            broken.from,
            broken.to,
        );
        let (stdout, exit) = fslc(&["check"], &path, true);
        let output = json(&stdout);
        assert_eq!(exit, 2, "{}: {output:#}", broken.code);
        assert_eq!(output["diagnostic_code"], broken.code, "{output:#}");
        let (line, column) = position(&text, broken.at, broken.occurrence);
        assert_eq!(
            (
                output["loc"]["line"].as_u64(),
                output["loc"]["column"].as_u64()
            ),
            (Some(line), Some(column)),
            "{}: {output:#}",
            broken.code
        );
    }
}

/// A diagnostic names the canonical name; for a document with a glossary
/// the message shows the alias beside it and the envelope adds `name` and
/// `display` (nothing is removed or renamed).
#[test]
fn diagnostics_show_the_alias_beside_the_canonical_name() {
    for (index, (from, to, message, name, display)) in [
        // A rejection at an alias the source wrote.
        (
            "  number 金額\n",
            "  number 金額 金額\n",
            "expected requirements declaration",
            "Amount",
            "金額",
        ),
        // A next-edition scenario's diagnostic, quoting canonical names.
        (
            "    then 申請1 is 支払済み",
            "    then 申請1 is 金額",
            "'金額（Amount）' is not a stage of '申請（Claim）'",
            "Amount",
            "金額",
        ),
        // A rejection that quotes a canonical name.
        (
            "  values 金額 = 0..3\n",
            "",
            "number '金額（Amount）' has no 'values' bound in verify block",
            "Amount",
            "金額",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (path, _) = broken_copy(&format!("display-{index}"), from, to);
        let (stdout, exit) = fslc(&["check"], &path, true);
        let output = json(&stdout);
        assert_eq!(exit, 2, "{output:#}");
        assert!(
            output["message"]
                .as_str()
                .is_some_and(|text| text.starts_with(message)),
            "{output:#}"
        );
        assert_eq!(
            (&output["name"], &output["display"]),
            (&Value::from(name), &Value::from(display)),
            "{output:#}"
        );
    }
}

/// `lint --strict-names` reports a canonical name also written as its alias
/// (`mixed_names`); plain `lint` does not.
#[test]
fn strict_names_reports_mixed_names() {
    let (path, text) = broken_copy(
        "mixed",
        "  concept 少額申請 = 申請 where 申請金額 <= 自動承認上限",
        "  concept 少額申請 = 申請 where amount <= 自動承認上限",
    );
    let mixed = |args: &[&str]| {
        let (stdout, _) = fslc(args, &path, true);
        json(&stdout)["files"][0]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .filter(|finding| finding["code"] == "mixed_names")
            .cloned()
            .collect::<Vec<_>>()
    };
    let findings = mixed(&["lint", "--strict-names"]);
    assert_eq!(findings.len(), 1, "{findings:#?}");
    let (line, column) = position(&text, "amount <=", 0);
    assert_eq!(
        (
            findings[0]["loc"]["line"].as_u64(),
            findings[0]["loc"]["column"].as_u64(),
            findings[0]["severity"].as_str()
        ),
        (Some(line), Some(column), Some("warning"))
    );
    assert!(mixed(&["lint"]).is_empty());
    // The unaltered fixture writes each name one way.
    let (stdout, _) = fslc(
        &["lint", "--strict-names"],
        &source("e2e", "glossary_ja.fsl"),
        true,
    );
    assert!(
        json(&stdout)["files"][0]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .all(|finding| finding["code"] != "mixed_names"),
        "{stdout}"
    );
}

/// Outside a gated `fsl 5` document nothing changes: `glossary` is the
/// unknown requirements declaration it always was, byte for byte, and the
/// two options exist only behind the gate.
#[test]
fn glossary_and_its_options_need_the_gate_and_the_fsl5_header() {
    let fsl5 = std::fs::read_to_string(source("e2e", "glossary_ascii.fsl")).expect("fixture");
    let body = fsl5.strip_prefix("fsl 5\n").expect("header");
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fsl5_glossary");
    std::fs::create_dir_all(&directory).expect("scratch directory");
    for (name, text) in [
        ("no-header.fsl", body.to_owned()),
        ("fsl4.fsl", format!("fsl 4\n{body}")),
    ] {
        let path = directory.join(name);
        std::fs::write(&path, &text).expect("write");
        let (line, column) = position(&text, "glossary {", 0);
        let expected = format!("expected requirements declaration at {line}:{column}");
        let (gated, gated_exit) = fslc(&["check"], &path, true);
        let (published, published_exit) = fslc(&["check"], &path, false);
        assert_eq!(gated_exit, 2, "{name}: {gated}");
        assert_eq!(published_exit, 2, "{name}: {published}");
        let mut gated = json(&gated);
        let published = json(&published);
        if name == "fsl4.fsl" {
            // A header read under the gate adds only `fsl_edition`; without
            // the gate the header is the unknown declaration `fsl`.
            assert_eq!(gated["fsl_edition"], 4);
            assert_eq!(gated["message"], expected.as_str());
            assert_eq!(published["diagnostic_code"], "FSL-DIALECT-UNKNOWN");
        } else {
            gated
                .as_object_mut()
                .expect("envelope")
                .remove("fsl_edition");
            assert_eq!(gated, published, "{name}");
            assert_eq!(published["message"], expected.as_str());
        }
    }
    let path = source("e2e", "glossary_ascii.fsl");
    for (args, message) in [
        (
            &["explain", "--names", "canonical"][..],
            "unknown explain option '--names'",
        ),
        (
            &["lint", "--strict-names"][..],
            "unknown lint option '--strict-names'",
        ),
    ] {
        let (stdout, exit) = fslc(args, &path, false);
        assert_eq!(exit, 2, "{args:?}: {stdout}");
        assert_eq!(json(&stdout)["message"], message, "{args:?}");
    }
    let (stdout, exit) = fslc(&["explain", "--names", "both"], &path, true);
    assert_eq!(
        (exit, json(&stdout)["message"].as_str()),
        (2, Some("--names must be canonical or alias"))
    );
}
