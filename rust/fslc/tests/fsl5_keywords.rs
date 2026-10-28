// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S5, through the native CLI: the English / Japanese keyword
//! table and the particles が・を・に・で・は.
//!
//! Acceptance 1 (K: `fsl5_en.fsl`, `fsl5_ja_particles.fsl` and
//! `fsl5_ja_full.fsl` lower to the one `lowered.fsl`) lives in
//! `fsl5_lowering_contract.rs`, acceptance 2 (the table itself) in
//! `fsl-syntax/src/keywords.rs`. This file holds acceptance 3: one failing
//! document per particle diagnostic, each a one-line change of
//! `fsl5/e2e/fsl5_ja_full.fsl`, failing `check` with its code at the text it
//! names, including a phrase that marks some words and not others
//! (decisions/048 J9).
//!
//! It also holds the requirements skill's side of the table (fsl 5 plan §3
//! S13, decisions/045 addendum): the keyword table that
//! `skills-next/fsl-requirements/references/keywords.md` prints is rendered
//! from [`KEYWORDS`] and [`PARTICLES`], and the skill's English example and
//! its Japanese spelling are one spec.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use fsl_syntax::keywords::{
    KEYWORDS, KeywordContext, PARTICLES, Position, SHARED_JAPANESE, Status,
};
use serde_json::Value;

#[path = "support/kernel_compare.rs"]
mod kernel_compare;

fn e2e() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fsl5/e2e")
}

/// `(stdout as JSON, exit)` of `fslc check <file>` in `directory`, with the
/// `fsl5` gate set when `gate`.
fn check(directory: &Path, file: &str, gate: bool) -> (Value, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(["check", file])
        .current_dir(directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if gate {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run fslc check");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "check {file}: invalid JSON ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

/// A scratch directory holding `1_business.fsl` (the fixtures' `implements`
/// dependency) and `source` as `doc.fsl`.
fn scratch(label: &str, source: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fsl5_keywords")
        .join(label);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    std::fs::copy(
        e2e().join("1_business.fsl"),
        directory.join("1_business.fsl"),
    )
    .expect("copy the business layer");
    std::fs::write(directory.join("doc.fsl"), source).expect("write scratch source");
    directory
}

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

/// `fsl5_ja_full.fsl` with `from` replaced by `to` (exactly once in the
/// fixture) must fail `check` with `code` at the `occurrence`-th match of
/// `at` in the changed source.
struct Broken {
    label: &'static str,
    code: &'static str,
    from: &'static str,
    to: &'static str,
    at: &'static str,
    occurrence: usize,
}

const BROKEN: &[Broken] = &[
    // Plan S5 acceptance 3: the actor marked を and the entity marked が.
    Broken {
        label: "event-roles-swapped",
        code: "particle_role_mismatch",
        from: "submit       = 従業員 が 申請 を 提出する",
        to: "submit       = 従業員 を 申請 が 提出する",
        at: "従業員 を",
        occurrence: 0,
    },
    Broken {
        label: "rule-roles-swapped",
        code: "particle_role_mismatch",
        from: "許可 従業員 が 申請 を 提出する",
        to: "許可 従業員 を 申請 が 提出する",
        at: "従業員 を",
        occurrence: 0,
    },
    Broken {
        label: "step-roles-swapped",
        code: "particle_role_mismatch",
        from: "    従業員 が 申請1 を 申請金額 1 で 提出する\n",
        to: "    申請1 が 従業員 を 申請金額 1 で 提出する\n",
        at: "従業員 を",
        occurrence: 0,
    },
    // A decision table row names its actor and verbs; the table names the
    // entity, so nothing in a row takes を.
    Broken {
        label: "row-object",
        code: "particle_role_mismatch",
        from: "少額申請 => システム が 承認する",
        to: "少額申請 => システム を 承認する",
        at: "システム を",
        occurrence: 0,
    },
    // No phrase of this edition has a counterpart (に).
    Broken {
        label: "event-counterpart",
        code: "particle_role_mismatch",
        from: "pay          = 経理 が 申請 を 支払う",
        to: "pay          = 経理 が 申請 に 支払う",
        at: "申請 に",
        occurrence: 0,
    },
    // Plan S5 acceptance 3: particles written onto the words they mark.
    Broken {
        label: "event-no-space",
        code: "particle_needs_space",
        from: "submit       = 従業員 が 申請 を 提出する",
        to: "submit       = 従業員が 申請を 提出する",
        at: "従業員が",
        occurrence: 0,
    },
    Broken {
        label: "step-no-space",
        code: "particle_needs_space",
        from: "    システム が 申請1 を 承認する\n",
        to: "    システム が 申請1を 承認する\n",
        at: "申請1を",
        occurrence: 0,
    },
    // J9: with particles every word but the verb takes one. The entity
    // without を leaves two words without a particle.
    Broken {
        label: "event-mixed",
        code: "mixed_particles",
        from: "pay          = 経理 が 申請 を 支払う",
        to: "pay          = 経理 が 申請 支払う",
        at: "支払う",
        occurrence: 1,
    },
    Broken {
        label: "rule-mixed",
        code: "mixed_particles",
        from: "許可 従業員 が 申請 を 提出する",
        to: "許可 従業員 申請 を 提出する",
        at: "提出する 条件",
        occurrence: 0,
    },
    Broken {
        label: "step-mixed",
        code: "mixed_particles",
        from: "    経理 が 申請1 を 支払う\n",
        to: "    経理 が 申請1 支払う\n",
        at: "支払う",
        occurrence: 2,
    },
];

#[test]
fn each_particle_diagnostic_fails_check_at_its_text() {
    let original = std::fs::read_to_string(e2e().join("fsl5_ja_full.fsl")).expect("fixture");
    for broken in BROKEN {
        assert_eq!(
            original.matches(broken.from).count(),
            1,
            "{}: {}",
            broken.label,
            broken.from
        );
        let text = original.replacen(broken.from, broken.to, 1);
        let directory = scratch(broken.label, &text);
        let (output, exit) = check(&directory, "doc.fsl", true);
        assert_eq!(exit, 2, "{}: {output:#}", broken.label);
        assert_eq!(
            output["diagnostic_code"], broken.code,
            "{}: {output:#}",
            broken.label
        );
        let (line, column) = position(&text, broken.at, broken.occurrence);
        assert_eq!(
            (
                output["loc"]["line"].as_u64(),
                output["loc"]["column"].as_u64()
            ),
            (Some(line), Some(column)),
            "{}: {output:#}",
            broken.label
        );
    }
}

/// The negatives above start from a document that checks: each fails for
/// its own change only.
#[test]
fn the_negative_examples_start_from_a_document_that_checks() {
    let original = std::fs::read_to_string(e2e().join("fsl5_ja_full.fsl")).expect("fixture");
    let directory = scratch("clean", &original);
    let (output, exit) = check(&directory, "doc.fsl", true);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("ok"), 0),
        "{output:#}"
    );
}

/// The table is read in next-edition requirements only: behind the gate, a
/// `spec` headed `fsl 5` does not read `定数` as `const`, and without the
/// gate a requirements document in Japanese keywords is not read.
#[test]
fn japanese_keywords_are_read_in_next_edition_requirements_only() {
    let spec = "fsl 5\nspec S {\n  定数 N = 1\n  state { x: Int }\n  init { x = 0 }\n}\n";
    let directory = scratch("spec", spec);
    let (output, exit) = check(&directory, "doc.fsl", true);
    assert_eq!(exit, 2, "{output:#}");
    assert_eq!(output["diagnostic_code"], "FSL-PARSE", "{output:#}");
    assert_eq!(
        (
            output["loc"]["line"].as_u64(),
            output["loc"]["column"].as_u64()
        ),
        (Some(3), Some(3)),
        "{output:#}"
    );

    let original = std::fs::read_to_string(e2e().join("fsl5_ja_full.fsl")).expect("fixture");
    let directory = scratch("ungated", &original);
    let (output, exit) = check(&directory, "doc.fsl", false);
    assert_eq!(
        (output["result"].as_str(), exit),
        (Some("error"), 2),
        "{output:#}"
    );
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

/// Where in a requirements spec the rows of `context` are read, as the
/// skill's table heads them. Exhaustive on purpose: a new context does not
/// compile until it has a heading.
const fn context_heading(context: KeywordContext) -> &'static str {
    match context {
        KeywordContext::Document => "Blocks of the file",
        KeywordContext::Declaration => "The leading word of a member of `requirements`",
        KeywordContext::Verify => "Inside `verify { }`",
        KeywordContext::Entity => "Inside `entity Name { }`",
        KeywordContext::Concept => "`concept C = E in S, … | where cond`",
        KeywordContext::Relation => "`R belongs to E as role via event`",
        KeywordContext::Event => {
            "`[fault] event name = phrase [with …] A -> B | in S [set …] [covers …]`"
        }
        KeywordContext::RuleExpression => "Inside a rule's or an event's expression",
        KeywordContext::Allow => "`allow P only when X | only for C`",
        KeywordContext::Never => "`never P when X | for C`",
        KeywordContext::After => "`P only after Q`",
        KeywordContext::Enters => "`E enters T only from S, …`",
        KeywordContext::Always => "`always C1 is C2`, `always E [not] in S, … has X`",
        KeywordContext::Deadline => "`deadline age <= bound` in a rule",
        KeywordContext::Decide => {
            "`decide ID \"text\" for S E { … [otherwise => …] } [assuming Q]`"
        }
        KeywordContext::DecideRow => "A decision row's outcome `Actor verb {or verb}`",
        KeywordContext::Goal => "`goal ID \"text\" some E can reach S`",
        KeywordContext::Policy => "`policy ID \"text\" every E in S must eventually be T {or T}`",
        KeywordContext::Measure => "`measure m = count C | count E in S, …`",
        KeywordContext::Scenario => "`scenario` / `forbidden` steps and their `then`",
        KeywordContext::Expression => "Boolean operators of an expression",
    }
}

/// The Markdown table the skill prints between its markers, rendered from
/// the parser's table: one section per context in table order, then the
/// shared Japanese words and the particles.
fn render_keyword_table() -> String {
    let mut contexts: Vec<KeywordContext> = Vec::new();
    for keyword in KEYWORDS {
        if !contexts.contains(&keyword.context) {
            contexts.push(keyword.context);
        }
    }
    let mut out = String::new();
    for context in contexts {
        writeln!(out, "\n**{}**\n", context_heading(context)).expect("write");
        out.push_str("| English | Japanese | Note |\n|---|---|---|\n");
        for keyword in KEYWORDS.iter().filter(|keyword| keyword.context == context) {
            let mut notes = Vec::new();
            if keyword.position == Position::After {
                notes.push("after the word it marks".to_owned());
            }
            if keyword.status == Status::Reserved {
                notes.push("reserved: not read yet".to_owned());
            }
            writeln!(
                out,
                "| `{}` | `{}` | {} |",
                keyword.english,
                keyword.japanese,
                notes.join("; ")
            )
            .expect("write");
        }
    }
    out.push_str(
        "\n**Japanese words shared by several places** (each means the same in all of them)\n\n",
    );
    out.push_str("| Japanese | English |\n|---|---|\n");
    for (japanese, english) in SHARED_JAPANESE {
        let english = english
            .iter()
            .map(|word| format!("`{word}`"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "| `{japanese}` | {english} |").expect("write");
    }
    out.push_str("\n**Particles** (a word of their own, after the word they mark)\n\n");
    out.push_str("| Particle | Marks |\n|---|---|\n");
    for (particle, spelling) in PARTICLES {
        writeln!(out, "| `{spelling}` | {} |", particle.role()).expect("write");
    }
    out
}

const TABLE_BEGIN: &str = "<!-- keyword-table:begin (generated; do not edit by hand) -->\n";
const TABLE_END: &str = "<!-- keyword-table:end -->\n";

#[path = "support/golden_update.rs"]
mod golden_update;

/// fsl 5 plan §3 S13 (decisions/045 addendum: skills are English-canonical
/// and print the keyword table): the table in the requirements skill is the
/// parser's table, rendered, so it can neither miss nor invent a keyword.
/// Locally, `FSLC_SKILLS_KEYWORDS_UPDATE=1` rewrites the generated block after the
/// table changed; the diff is then reviewed like any other.
/// When `CI` is set, any set update variable fails before comparison or writing.
#[test]
fn the_requirements_skill_prints_the_parser_keyword_table() {
    let update = std::env::var_os("FSLC_SKILLS_KEYWORDS_UPDATE");
    let ci = std::env::var_os("CI");
    let update_requested = golden_update::golden_update_requested(
        update.as_ref().map(|value| value.to_str().unwrap_or("")),
        ci.as_ref().map(|value| value.to_str().unwrap_or("")),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let path = repository().join("skills-next/fsl-requirements/references/keywords.md");
    let text = std::fs::read_to_string(&path).expect("read the skill's keyword table");
    let begin = text.find(TABLE_BEGIN).expect("begin marker") + TABLE_BEGIN.len();
    let end = text.find(TABLE_END).expect("end marker");
    let expected = render_keyword_table();
    if update_requested {
        let updated = format!("{}{expected}{}", &text[..begin], &text[end..]);
        std::fs::write(&path, updated).expect("rewrite the skill's keyword table");
        return;
    }
    assert!(
        text[begin..end] == expected,
        "{} is not the parser's keyword table; run this test with \
         FSLC_SKILLS_KEYWORDS_UPDATE=1 and review the diff.\nexpected:\n{expected}",
        path.display()
    );
}

/// The `fsl` fences of a Markdown document, joined as `fslc check` joins a
/// literate file's fences.
fn fenced_fsl(markdown: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if inside {
            if trimmed.starts_with("```") {
                inside = false;
            } else {
                out.push_str(line);
                out.push('\n');
            }
        } else if trimmed
            .strip_prefix("```")
            .is_some_and(|info| info.split_whitespace().next() == Some("fsl"))
        {
            inside = true;
        }
    }
    out
}

/// fsl 5 plan §3 S13: the requirements skill's English example and its
/// Japanese spelling in the glossary reference (aliases, Japanese keywords,
/// particles) are one spec: their kernels are equal without spans, origins
/// and annotations. A change to one example that is not made to the other
/// fails here.
#[test]
fn the_requirements_skill_example_reads_the_same_in_japanese() {
    let skill = repository().join("skills-next/fsl-requirements");
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fsl5_keywords/skill-example");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    let mut kernels = Vec::new();
    for (document, file) in [
        ("SKILL.md", "english.fsl"),
        ("references/glossary.md", "japanese.fsl"),
    ] {
        let markdown = std::fs::read_to_string(skill.join(document)).expect("read the skill");
        let source = fenced_fsl(&markdown);
        assert!(
            source.starts_with("fsl 5\n"),
            "{document} holds no next-edition example"
        );
        let path = directory.join(file);
        std::fs::write(&path, source).expect("write the example");
        kernels.push(kernel_compare::kernel_v2_fsl5(&path));
    }
    kernel_compare::assert_same_kernel(
        &kernels[0],
        &kernels[1],
        "fsl-requirements SKILL.md example against its Japanese spelling",
    );
}
