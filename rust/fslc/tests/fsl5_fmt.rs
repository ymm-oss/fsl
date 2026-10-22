// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S10 fmt, through the native CLI: `fslc fmt` on the next
//! edition's requirements members, `--lang ja|en` (the keyword table),
//! `--names alias|canonical` (the glossary), and the space `fmt` inserts for
//! `particle_needs_space`.
//!
//! For every `fsl 5` fixture, `fsl5/e2e/fsl5_en.fsl`, `fsl5_ja_particles.fsl`
//! and `fsl5_ja_full.fsl` (S5 acceptance 1's three spellings of one kernel)
//! among them: fmt is idempotent, the `--lang` and `--names` round trips
//! return the formatted document, and every output lowers to the kernel of
//! the input (the one comparison of `support/kernel_compare.rs`). The two
//! Japanese fixtures are each other's `--lang` translation, comments aside.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "support/kernel_compare.rs"]
mod kernel_compare;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fsl5")
}

fn e2e() -> PathBuf {
    fixtures().join("e2e")
}

/// Every fixture headed `fsl 5`, as `(example directory, file name)`.
fn next_edition_sources() -> Vec<(PathBuf, String)> {
    let mut sources = Vec::new();
    let mut directories = std::fs::read_dir(fixtures())
        .expect("fsl5 fixtures")
        .map(|entry| entry.expect("fixture directory").path())
        .collect::<Vec<_>>();
    directories.sort();
    for directory in directories {
        let mut files = std::fs::read_dir(&directory)
            .expect("example directory")
            .map(|entry| entry.expect("fixture").path())
            .filter(|path| read(path).starts_with("fsl 5\n"))
            .map(|path| {
                path.file_name()
                    .expect("name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        files.sort();
        sources.extend(files.into_iter().map(|file| (directory.clone(), file)));
    }
    sources
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// A scratch directory holding the files of the example `from` (the
/// fixtures' `implements` dependencies).
fn scratch_of(label: &str, from: &Path) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("fsl5_fmt")
        .join(label);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    for entry in std::fs::read_dir(from).expect("example directory") {
        let path = entry.expect("fixture").path();
        std::fs::copy(&path, directory.join(path.file_name().expect("name")))
            .expect("copy fixture");
    }
    directory
}

fn scratch(label: &str) -> PathBuf {
    scratch_of(label, &e2e())
}

/// `(stdout, exit)` of `fslc fmt <path> <args>` with the gate set when
/// `gate`.
fn fmt_raw(path: &Path, args: &[&str], gate: bool) -> (String, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .arg("fmt")
        .arg(path.file_name().expect("file name"))
        .args(args)
        .current_dir(path.parent().expect("directory"))
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if gate {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run fslc fmt");
    (
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        output.status.code().expect("exit code"),
    )
}

/// `fslc fmt` of `source` (written as `name` in `directory`) with `args`,
/// which must succeed; the formatted text, also written as `<name>.out.fsl`
/// next to it for the next step.
fn fmt(directory: &Path, name: &str, source: &str, args: &[&str]) -> (String, PathBuf) {
    let path = directory.join(name);
    std::fs::write(&path, source).expect("write source");
    let (output, status) = fmt_raw(&path, args, true);
    assert_eq!(status, 0, "fmt {name} {args:?}: {output}");
    let out = directory.join(format!("{}.out.fsl", name.trim_end_matches(".fsl")));
    std::fs::write(&out, &output).expect("write formatted");
    (output, out)
}

fn same_kernel(original: &Path, rewritten: &Path, context: &str) {
    kernel_compare::assert_same_kernel(
        &kernel_compare::kernel_v2_fsl5(original),
        &kernel_compare::kernel_v2_fsl5(rewritten),
        context,
    );
}

/// The language a fixture's keywords are in, and the other one.
fn languages(fixture: &str) -> (&'static str, &'static str) {
    if fixture == "fsl5_ja_full.fsl" {
        ("ja", "en")
    } else {
        ("en", "ja")
    }
}

/// fmt is idempotent and keeps the kernel; `--lang` to the other language
/// and back, and `--names canonical` then `alias`, return the formatted
/// document; every step keeps the kernel.
#[test]
fn fmt_lang_and_names_round_trips_are_identities_and_keep_the_kernel() {
    let sources = next_edition_sources();
    for (example, required) in [
        ("e2e", "fsl5_en.fsl"),
        ("e2e", "fsl5_ja_particles.fsl"),
        ("e2e", "fsl5_ja_full.fsl"),
        ("multi_entity", "fsl5_en.fsl"),
        ("multi_entity", "fsl5_ja_full.fsl"),
        ("relations", "fsl5_en.fsl"),
    ] {
        assert!(
            sources.iter().any(|(directory, file)| {
                directory == &fixtures().join(example) && file == required
            }),
            "{example}/{required} is not among the fixtures"
        );
    }
    for (example, fixture) in &sources {
        let fixture = fixture.as_str();
        let label = format!(
            "{}-{}",
            example.file_name().expect("example").to_string_lossy(),
            fixture.trim_end_matches(".fsl")
        );
        let directory = scratch_of(&label, example);
        let original = example.join(fixture);
        let (formatted, formatted_path) = fmt(&directory, "doc.fsl", &read(&original), &[]);
        same_kernel(&original, &formatted_path, &format!("{fixture}: fmt"));
        let (twice, _) = fmt(&directory, "again.fsl", &formatted, &[]);
        assert_eq!(twice, formatted, "{fixture}: fmt is not idempotent");

        let (own, other) = languages(fixture);
        let (translated, translated_path) =
            fmt(&directory, "lang.fsl", &formatted, &["--lang", other]);
        assert_ne!(
            translated, formatted,
            "{fixture}: --lang {other} changed nothing"
        );
        same_kernel(
            &original,
            &translated_path,
            &format!("{fixture}: --lang {other}"),
        );
        let (back, _) = fmt(&directory, "lang_back.fsl", &translated, &["--lang", own]);
        assert_eq!(back, formatted, "{fixture}: --lang {other} then {own}");

        let (canonical, canonical_path) = fmt(
            &directory,
            "names.fsl",
            &formatted,
            &["--names", "canonical"],
        );
        same_kernel(
            &original,
            &canonical_path,
            &format!("{fixture}: --names canonical"),
        );
        let (aliased, _) = fmt(
            &directory,
            "names_back.fsl",
            &canonical,
            &["--names", "alias"],
        );
        assert_eq!(
            aliased, formatted,
            "{fixture}: --names canonical then alias"
        );
    }
}

fn without_comments(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `fsl5_ja_particles.fsl` (English keywords) and `fsl5_ja_full.fsl`
/// (Japanese keywords) differ only in the keyword table's words: each is the
/// other's `--lang` translation.
#[test]
fn the_japanese_fixtures_are_each_others_translation() {
    let directory = scratch("translation");
    let particles = read(&e2e().join("fsl5_ja_particles.fsl"));
    let full = read(&e2e().join("fsl5_ja_full.fsl"));
    let (particles_formatted, _) = fmt(&directory, "particles.fsl", &particles, &[]);
    let (full_formatted, _) = fmt(&directory, "full.fsl", &full, &[]);
    let (to_japanese, _) = fmt(&directory, "to_ja.fsl", &particles, &["--lang", "ja"]);
    let (to_english, _) = fmt(&directory, "to_en.fsl", &full, &["--lang", "en"]);
    assert_eq!(
        without_comments(&to_japanese),
        without_comments(&full_formatted)
    );
    assert_eq!(
        without_comments(&to_english),
        without_comments(&particles_formatted)
    );
}

/// `fixture` (all Japanese) written with `--lang en --names canonical`,
/// which keeps its kernel; every non-ASCII word outside strings, comments
/// and the glossary's right sides is one of `allowed`.
fn english_of(directory: &Path, fixture: &Path, allowed: &[&str]) -> String {
    let (english, path) = fmt(
        directory,
        "full.fsl",
        &read(fixture),
        &["--lang", "en", "--names", "canonical"],
    );
    same_kernel(fixture, &path, "--lang en --names canonical");
    let mut in_glossary = false;
    for line in english.lines() {
        let code = line.split("//").next().unwrap_or_default();
        let code = code.split('"').step_by(2).collect::<String>();
        if code.trim_start().starts_with("glossary") {
            in_glossary = true;
        }
        if in_glossary {
            in_glossary = !code.trim().eq("}");
            continue;
        }
        for word in code.split_whitespace().filter(|word| !word.is_ascii()) {
            assert!(
                allowed.contains(&word),
                "'{word}' in an English document: {line}"
            );
        }
    }
    english
}

/// With the glossary's names, `--lang en --names canonical` writes every
/// keyword, name and phrase in English. What stays Japanese is the author's:
/// strings, comments, the glossary's right sides, and the scenarios'
/// instance labels (`申請1`, not glossary names), whose steps keep their
/// particles. The relations, resources, measures and goals of
/// `multi_entity` are written in English too.
#[test]
fn lang_en_names_canonical_writes_the_document_in_english() {
    let english = english_of(
        &scratch("english"),
        &e2e().join("fsl5_ja_full.fsl"),
        &["申請1", "が", "を", "で", "は"],
    );
    let multi_entity = fixtures().join("multi_entity");
    let orders = english_of(
        &scratch_of("english-orders", &multi_entity),
        &multi_entity.join("fsl5_ja_full.fsl"),
        &[],
    );
    for line in [
        "  Order belongs to User as buyer via place
",
        "  resource stock: Qty = 2
",
        "  measure refund_count = count Order in Refunded
",
        "some Order can reach Refunded
",
    ] {
        assert!(orders.contains(line), "{line}: {orders}");
    }
    assert!(
        english
            .contains("  event submit = Employee submits Claim with amount Draft -> Submitted\n")
    );
    assert!(english.contains("    allow Employee submits Claim only when input amount > 0\n"));
    assert!(english.contains("    LargeClaim => Manager approves or rejects\n"));
    assert!(english.contains("    Employee が 申請1 を amount 1 で submits\n"));
}

/// A line an event or a decision table row continues on (its `covers`) is
/// indented one level deeper than the statement, as the fixtures write it.
#[test]
fn continuation_lines_are_indented_under_their_statement() {
    let directory = scratch("continuation");
    let (formatted, _) = fmt(
        &directory,
        "doc.fsl",
        &read(&e2e().join("fsl5_en.fsl")),
        &[],
    );
    assert!(
        formatted.contains("Approved -> Paid\n    covers REQ"),
        "{formatted}"
    );
    assert!(
        formatted.contains("System approves\n      covers REQ"),
        "{formatted}"
    );
}

/// `particle_needs_space`: `check` rejects a particle written onto its word;
/// `fmt` inserts the space, and the result is the fixture's kernel.
#[test]
fn fmt_inserts_the_space_a_particle_needs() {
    let directory = scratch("particle-space");
    let fixture = read(&e2e().join("fsl5_ja_full.fsl"));
    let joined = fixture
        .replacen(
            "従業員 が 申請 を 提出する 申請金額 で",
            "従業員が 申請を 提出する 申請金額で",
            1,
        )
        .replacen("    従業員 が 申請1 を", "    従業員が 申請1 を", 1);
    assert_ne!(joined, fixture);
    let path = directory.join("joined.fsl");
    std::fs::write(&path, &joined).expect("write joined source");
    let check = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "joined.fsl"])
        .current_dir(&directory)
        .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
        .output()
        .expect("run fslc check");
    let report: Value = serde_json::from_slice(&check.stdout).expect("check JSON");
    assert_eq!(check.status.code(), Some(2), "{report}");
    assert!(
        report.to_string().contains("particle_needs_space"),
        "{report}"
    );
    assert!(
        report.to_string().contains("`fslc fmt` inserts the space"),
        "{report}"
    );

    let (spaced, spaced_path) = fmt(&directory, "joined.fsl", &joined, &[]);
    let (expected, _) = fmt(&directory, "fixture.fsl", &fixture, &[]);
    assert_eq!(spaced, expected);
    same_kernel(
        &e2e().join("fsl5_ja_full.fsl"),
        &spaced_path,
        "spaced particles",
    );
    let (_, status) = fmt_raw(&path, &["--check"], true);
    assert_eq!(status, 1, "--check reports the missing spaces as a change");
}

/// The options exist only behind the gate; without it `fmt` rejects them as
/// it always has, and a document without the keyword table is not respelled.
#[test]
fn lang_and_names_are_gated_and_need_a_next_edition_requirements_document() {
    let directory = scratch("gated");
    let path = directory.join("doc.fsl");
    std::fs::write(&path, read(&e2e().join("fsl5_en.fsl"))).expect("write source");
    for option in ["--lang", "--names"] {
        let (output, status) = fmt_raw(&path, &[option, "en"], false);
        assert_eq!(status, 2, "{output}");
        assert!(
            output.contains(&format!("unknown fmt option '{option}'")),
            "{output}"
        );
    }
    let spec = directory.join("spec.fsl");
    std::fs::write(
        &spec,
        "spec S {\n  state { x: Bool }\n  init { x = false }\n}\n",
    )
    .expect("write spec");
    let (output, status) = fmt_raw(&spec, &["--lang", "ja"], true);
    assert_eq!(status, 2, "{output}");
    assert!(output.contains("respell_not_applicable"), "{output}");
    let (output, status) = fmt_raw(&path, &["--lang", "fr"], true);
    assert_eq!(status, 2, "{output}");
    assert!(output.contains("--lang must be ja or en"), "{output}");
}
