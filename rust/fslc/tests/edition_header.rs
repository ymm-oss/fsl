// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The syntax-edition header behind the unstable gate (fsl 5 plan S1,
//! `docs/design/parse-entry-inventory.md`, "Edition header").
//!
//! A document without a header is unchanged with the gate on; R0
//! (`unstable_gate_r0.rs`) holds that for the whole corpus, positions
//! included. This file holds the rest against corpus copies:
//!
//! - a copy with `fsl 4` added reads like the original (the kernel without
//!   positions, and `check`'s verdict);
//! - a copy with `fsl 5` added checks like the original under
//!   `--edition next` (exit, codes and severities), frozen in
//!   `tests/goldens/edition_header_next.txt` so the next edition's syntax
//!   stages change it as a reviewed diff. `FSLC_UPDATE_EDITION_GOLDEN=1`
//!   rewrites it from the published `--edition next` run.
//!
//! and the header's own contract: conflicts, unknown editions, the dialects
//! without an edition, the literate fence, `fmt`, `lint` and `migrate`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "support/mod.rs"]
mod support;
use support::{corpus_files, repo_relative, root, top_level_keyword};

const GATE: &str = "fsl5";

/// `(stdout, exit)` of `fslc <arguments>` in `directory`, with the gate set
/// or unset.
fn fslc(directory: &Path, arguments: &[&str], gate: bool) -> (String, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(arguments)
        .current_dir(directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if gate {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, GATE);
    }
    let output = command.output().expect("run fslc");
    let exit = output
        .status
        .code()
        .unwrap_or_else(|| panic!("fslc {arguments:?} died by signal"));
    (
        String::from_utf8(output.stdout).expect("UTF-8 stdout"),
        exit,
    )
}

fn json(stdout: &str) -> Value {
    serde_json::from_str(stdout).unwrap_or_else(|error| panic!("{error}: {stdout}"))
}

/// A scratch directory unique to this process and `label`, emptied first.
fn scratch(label: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "fslc-edition-header-{}-{label}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

/// `source` with `header` on its own line before the first line.
fn with_header(source: &str, header: &str) -> String {
    match source.strip_prefix('\u{feff}') {
        Some(rest) => format!("\u{feff}{header}\n{rest}"),
        None => format!("{header}\n{source}"),
    }
}

/// Copy `specs/` and `examples/` under a scratch directory, adding `header`
/// to every `.fsl` file (imports included) when given.
fn mirror(label: &str, header: Option<&str>) -> PathBuf {
    let target = scratch(label);
    let root = root();
    for tree in ["specs", "examples"] {
        copy_tree(&root.join(tree), &target.join(tree), header);
    }
    target
}

fn copy_tree(from: &Path, to: &Path, header: Option<&str>) {
    std::fs::create_dir_all(to).expect("create mirror directory");
    for entry in std::fs::read_dir(from).expect("read corpus directory") {
        let path = entry.expect("corpus entry").path();
        let destination = to.join(path.file_name().expect("file name"));
        if path.is_dir() {
            copy_tree(&path, &destination, header);
        } else if let Some(header) =
            header.filter(|_| path.extension().is_some_and(|extension| extension == "fsl"))
        {
            let source = std::fs::read_to_string(&path).expect("read corpus file");
            std::fs::write(&destination, with_header(&source, header)).expect("write copy");
        } else {
            std::fs::copy(&path, &destination).expect("copy corpus file");
        }
    }
}

/// Corpus files whose dialect has a syntax edition: the causal and ai
/// documents reject the header (decisions/049 D8) and are checked apart.
fn edition_corpus() -> (Vec<String>, Vec<String>) {
    let root = root();
    corpus_files(&root)
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).expect("read corpus file");
            let editionless = top_level_keyword(&source)
                .is_some_and(|keyword| fsl_syntax::EDITIONLESS_DECLARATIONS.contains(&keyword));
            (repo_relative(&root, &path), editionless)
        })
        .fold(
            (Vec::new(), Vec::new()),
            |(mut with, mut without), (path, editionless)| {
                if editionless {
                    without.push(path);
                } else {
                    with.push(path);
                }
                (with, without)
            },
        )
}

/// `value` without source positions, which the header line shifts.
fn without_positions(mut value: Value) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for key in ["line", "column", "end_line", "end_column"] {
                    fields.remove(key);
                }
                fields.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut value);
    value
}

/// The verdict fields of a `check` envelope: exit, result, kind and code.
fn verdict(stdout: &str, exit: i32) -> String {
    let output = json(stdout);
    format!(
        "exit={exit} result={} kind={} code={}",
        output["result"], output["kind"], output["diagnostic_code"]
    )
}

/// What `--edition next` decides for a `check`: the verdict, then each
/// finding's and warning's code with its severity, in order.
fn next_summary(stdout: &str, exit: i32) -> String {
    let output = json(stdout);
    let codes = |key: &str| {
        output[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| {
                format!(
                    "{}:{}",
                    item["code"]
                        .as_str()
                        .or(item["kind"].as_str())
                        .unwrap_or("?"),
                    item["severity"].as_str().unwrap_or("-")
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{} edition={} findings=[{}] warnings=[{}]",
        verdict(stdout, exit),
        output["edition"],
        codes("findings"),
        codes("warnings")
    )
}

/// Run `job` for every corpus path on a few threads, keeping the order.
fn each_path<T: Send>(paths: &[String], job: impl Fn(&str) -> T + Sync) -> Vec<T> {
    let chunk = paths.len().div_ceil(3).max(1);
    std::thread::scope(|scope| {
        let handles = paths
            .chunks(chunk)
            .map(|slice| scope.spawn(|| slice.iter().map(|path| job(path)).collect::<Vec<_>>()))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("corpus worker"))
            .collect()
    })
}

#[test]
fn an_fsl_4_header_reads_the_corpus_as_without_one() {
    let (paths, _) = edition_corpus();
    assert!(paths.len() > 150, "corpus floor: {}", paths.len());
    let plain = mirror("plain4", None);
    let four = mirror("fsl4", Some("fsl 4"));
    let failures = each_path(&paths, |path| {
        let mut failures = Vec::new();
        let (plain_kernel, plain_exit) = fslc(&plain, &["kernel", path], true);
        let (four_kernel, four_exit) = fslc(&four, &["kernel", path], true);
        let same_kernel = if plain_exit == 0 {
            four_exit == 0
                && without_positions(json(&plain_kernel)) == without_positions(json(&four_kernel))
        } else {
            verdict(&plain_kernel, plain_exit) == verdict(&four_kernel, four_exit)
        };
        if !same_kernel {
            failures.push(format!(
                "{path}: kernel differs with `fsl 4` (exit {plain_exit} -> {four_exit})"
            ));
        }
        let (plain_check, plain_status) = fslc(&plain, &["check", path], true);
        let (four_check, four_status) = fslc(&four, &["check", path], true);
        if verdict(&plain_check, plain_status) != verdict(&four_check, four_status) {
            failures.push(format!(
                "{path}: check {} -> {}",
                verdict(&plain_check, plain_status),
                verdict(&four_check, four_status)
            ));
        }
        failures
    })
    .concat();
    let _ = std::fs::remove_dir_all(&plain);
    let _ = std::fs::remove_dir_all(&four);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_fsl_5_header_checks_the_corpus_as_edition_next_does() {
    let (paths, _) = edition_corpus();
    let plain = mirror("plain5", None);
    let five = mirror("fsl5", Some("fsl 5"));
    let rows = each_path(&paths, |path| {
        let (next, next_exit) = fslc(&plain, &["check", path, "--edition", "next"], false);
        let (header, header_exit) = fslc(&five, &["check", path], true);
        (
            format!("{path}\t{}", next_summary(&next, next_exit)),
            format!("{path}\t{}", next_summary(&header, header_exit)),
        )
    });
    let _ = std::fs::remove_dir_all(&plain);
    let _ = std::fs::remove_dir_all(&five);
    let next = rows
        .iter()
        .map(|(next, _)| next.as_str())
        .collect::<Vec<_>>();
    let golden_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/edition_header_next.txt");
    let actual = format!("{}\n", next.join("\n"));
    if std::env::var_os("FSLC_UPDATE_EDITION_GOLDEN").is_some() {
        std::fs::write(&golden_path, &actual).expect("update edition golden");
    }
    let golden = std::fs::read_to_string(&golden_path).expect("read edition golden");
    assert_eq!(
        golden, actual,
        "the published `--edition next` corpus run moved; review the golden diff"
    );
    let differences = rows
        .iter()
        .filter(|(next, header)| next != header)
        .map(|(next, header)| format!("--edition next: {next}\nfsl 5 header:   {header}"))
        .collect::<Vec<_>>();
    assert!(differences.is_empty(), "{}", differences.join("\n"));
    assert!(
        golden.contains("findings=[") && golden.lines().any(|line| line.contains(":error")),
        "not vacuous: some corpus file must have a next-edition error finding"
    );
}

#[test]
fn editionless_dialects_in_the_corpus_reject_the_header() {
    let (_, paths) = edition_corpus();
    assert!(!paths.is_empty(), "the corpus has causal or ai documents");
    let five = mirror("editionless", Some("fsl 5"));
    for path in &paths {
        let (stdout, exit) = fslc(&five, &["check", path], true);
        let output = json(&stdout);
        assert_eq!(
            (exit, output["diagnostic_code"].as_str()),
            (2, Some(fsl_syntax::EDITION_HEADER_NOT_ACCEPTED)),
            "{path}: {stdout}"
        );
        assert_eq!(
            (
                output["loc"]["line"].as_u64(),
                output["loc"]["column"].as_u64()
            ),
            (Some(1), Some(1)),
            "{path}"
        );
    }
    let _ = std::fs::remove_dir_all(&five);
}

/// A small spec every edition accepts.
const LEGACY: &str =
    "spec S {\n  state { n: 0..2 = 0 }\n  action inc() { requires n < 2  n = n + 1 }\n}\n";

fn write(directory: &Path, name: &str, contents: &str) -> String {
    std::fs::write(directory.join(name), contents).expect("write fixture");
    name.to_owned()
}

#[test]
fn header_conflicts_and_unknown_editions_exit_2() {
    let directory = scratch("conflict");
    let five = write(&directory, "five.fsl", &with_header(LEGACY, "fsl 5"));
    let four = write(&directory, "four.fsl", &with_header(LEGACY, "fsl 4"));
    let six = write(&directory, "six.fsl", &with_header(LEGACY, "fsl 6"));
    let plain = write(&directory, "plain.fsl", LEGACY);

    for (file, requested, declared) in [(&five, "4", 5), (&four, "5", 4), (&four, "next", 4)] {
        for command in ["check", "verify"] {
            let (stdout, exit) = fslc(&directory, &[command, file, "--edition", requested], true);
            let output = json(&stdout);
            assert_eq!(exit, 2, "{command} {file} --edition {requested}: {stdout}");
            assert_eq!(
                output["diagnostic_code"],
                fsl_syntax::EDITION_CONFLICT,
                "{stdout}"
            );
            assert_eq!(output["loc"]["line"], 1, "{stdout}");
            let wanted = if requested == "next" { "5" } else { requested };
            assert_eq!(
                output["message"],
                format!(
                    "the document declares 'fsl {declared}' but --edition {wanted} was requested at 1:1"
                ),
                "{stdout}"
            );
        }
    }
    for (file, requested) in [(&five, "5"), (&five, "next"), (&four, "4"), (&plain, "4")] {
        let (stdout, exit) = fslc(&directory, &["check", file, "--edition", requested], true);
        assert_eq!(exit, 0, "{file} --edition {requested}: {stdout}");
    }

    let (stdout, exit) = fslc(&directory, &["check", &six], true);
    let output = json(&stdout);
    assert_eq!(exit, 2, "{stdout}");
    assert_eq!(
        output["diagnostic_code"],
        fsl_syntax::UNSUPPORTED_EDITION,
        "{stdout}"
    );
    assert_eq!(
        output["message"],
        "unsupported syntax edition 'fsl 6'; supported editions: 4, 5 at 1:1"
    );

    for file in [&five, &six] {
        let (stdout, exit) = fslc(&directory, &["check", file], false);
        assert_eq!(exit, 2, "{stdout}");
        assert_eq!(
            json(&stdout)["diagnostic_code"],
            "FSL-DIALECT-UNKNOWN",
            "without the gate a header stays the unknown declaration `fsl`"
        );
    }
    for requested in ["4", "5"] {
        let (stdout, exit) = fslc(
            &directory,
            &["check", &plain, "--edition", requested],
            false,
        );
        assert_eq!(exit, 2, "{stdout}");
        assert_eq!(
            json(&stdout)["message"],
            "--edition must be current or next"
        );
    }
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn fsl_stays_an_identifier_outside_the_header_line() {
    let directory = scratch("identifier");
    let spec = "spec S {\n  const fsl = 1\n  state { n: 0..1 = 0 }\n  action a() { requires n < fsl  n = n + 1 }\n}\n";
    let plain = write(&directory, "plain.fsl", spec);
    let five = write(&directory, "five.fsl", &with_header(spec, "fsl 5"));
    for (file, gate) in [(&plain, false), (&plain, true), (&five, true)] {
        let (stdout, exit) = fslc(&directory, &["check", file], gate);
        assert_eq!(exit, 0, "{file} gate={gate}: {stdout}");
        assert_eq!(json(&stdout)["result"], "ok");
    }
    let (plain_kernel, _) = fslc(&directory, &["kernel", &plain], true);
    let (five_kernel, _) = fslc(&directory, &["kernel", &five], true);
    assert_eq!(
        without_positions(json(&plain_kernel))["constants"],
        without_positions(json(&five_kernel))["constants"]
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_header_in_the_first_literate_fence_selects_the_edition() {
    let directory = scratch("literate");
    let fence = |header: &str| {
        format!(
            "# Counter\n\nProse before the fence.\n\n```fsl\n{header}spec S {{\n  state {{ n: 0..2 = 0 }}\n  action inc() {{ requires n < 2  n = n + 1 }}\n}}\n```\n"
        )
    };
    let five = write(&directory, "five.fsl.md", &fence("fsl 5\n"));
    let four = write(&directory, "four.fsl.md", &fence("fsl 4\n"));
    let (stdout, exit) = fslc(&directory, &["check", &five], true);
    let output = json(&stdout);
    assert_eq!(exit, 0, "{stdout}");
    assert_eq!(output["fsl_edition"], 5, "{stdout}");
    assert_eq!(output["edition"], "next", "{stdout}");
    let (stdout, exit) = fslc(&directory, &["check", &four], true);
    assert_eq!(
        (exit, &json(&stdout)["fsl_edition"]),
        (0, &Value::from(4)),
        "{stdout}"
    );
    let (stdout, exit) = fslc(&directory, &["check", &five, "--edition", "4"], true);
    assert_eq!(exit, 2, "{stdout}");
    assert_eq!(
        json(&stdout)["diagnostic_code"],
        fsl_syntax::EDITION_CONFLICT
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn fmt_keeps_and_lint_and_migrate_add_the_header() {
    let directory = scratch("tools");
    let source = format!("// 日本語の先頭コメント\n{LEGACY}");
    let plain = write(&directory, "plain.fsl", &source);
    let five = write(&directory, "five.fsl", &with_header(LEGACY, "fsl 5"));

    let (formatted, exit) = fslc(&directory, &["fmt", &five], true);
    assert_eq!(exit, 0, "{formatted}");
    assert!(formatted.starts_with("fsl 5\n"), "{formatted}");
    let formatted_file = write(&directory, "formatted.fsl", &formatted);
    let (check, exit) = fslc(&directory, &["fmt", "--check", &formatted_file], true);
    assert_eq!(exit, 0, "a formatted header is stable: {check}");

    let (lint, exit) = fslc(&directory, &["lint", &plain, "--edition", "5"], true);
    let output = json(&lint);
    assert_eq!(exit, 1, "{lint}");
    let finding = &output["files"][0]["findings"][0];
    assert_eq!(
        finding["code"],
        fsl_syntax::MISSING_EDITION_HEADER,
        "{lint}"
    );
    assert_eq!(finding["severity"], "info", "{lint}");
    assert_eq!(
        (
            finding["loc"]["line"].as_u64(),
            finding["loc"]["column"].as_u64()
        ),
        (Some(2), Some(1))
    );
    let (lint, exit) = fslc(&directory, &["lint", &five, "--edition", "5"], true);
    assert_eq!(
        (exit, &json(&lint)["finding_count"]),
        (0, &Value::from(0)),
        "{lint}"
    );
    for (arguments, gate) in [
        (&["lint", plain.as_str(), "--edition", "next"][..], false),
        (&["lint", plain.as_str()][..], true),
    ] {
        let (lint, exit) = fslc(&directory, arguments, gate);
        assert_eq!(
            (exit, &json(&lint)["finding_count"]),
            (0, &Value::from(0)),
            "{arguments:?} gate={gate}: {lint}"
        );
    }

    let (migrate, exit) = fslc(&directory, &["migrate", &plain, "--write"], true);
    assert_eq!(exit, 0, "{migrate}");
    assert_eq!(
        std::fs::read_to_string(directory.join(&plain)).expect("read migrated"),
        format!("// 日本語の先頭コメント\nfsl 5\n{LEGACY}")
    );
    let (again, _) = fslc(&directory, &["migrate", &plain], true);
    assert_eq!(json(&again)["changed"], 0, "{again}");
    let untouched = write(&directory, "untouched.fsl", &source);
    let (migrate, _) = fslc(&directory, &["migrate", &untouched, "--write"], false);
    assert_eq!(json(&migrate)["changed"], 0, "{migrate}");
    assert_eq!(
        std::fs::read_to_string(directory.join(&untouched)).expect("read untouched"),
        source,
        "without the gate migrate inserts no header"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// `migrate` defaults to the next edition, so an explicit `--edition current`
/// must stay distinct from an omitted flag, with or without the gate, and it
/// inserts no header.
#[test]
fn migrate_with_edition_current_keeps_the_current_edition() {
    let directory = scratch("migrate-current");
    let legacy = write(
        &directory,
        "legacy.fsl",
        "spec S {\n  state { n: 0..3 }\n  init { n = 0 }\n  action bump() \"REQ-1: bump\" {\n    requires n < 3\n    n = n + 1\n  }\n  invariant Bound { n >= 0 }\n}\n",
    );
    for gate in [false, true] {
        for (arguments, edition, severity) in [
            (&["migrate", legacy.as_str()][..], "next", "error"),
            (
                &["migrate", legacy.as_str(), "--edition", "current"][..],
                "current",
                "warning",
            ),
        ] {
            let (stdout, exit) = fslc(&directory, arguments, gate);
            let output = json(&stdout);
            assert_eq!(exit, 0, "{arguments:?} gate={gate}: {stdout}");
            assert_eq!(output["edition"], edition, "{arguments:?} gate={gate}");
            let findings = output["files"][0]["findings"]
                .as_array()
                .unwrap_or_else(|| panic!("findings: {stdout}"));
            assert!(!findings.is_empty(), "{arguments:?} gate={gate}: {stdout}");
            let codes = findings
                .iter()
                .map(|finding| finding["code"].as_str().unwrap_or_default())
                .collect::<Vec<_>>();
            assert_eq!(
                codes.contains(&fsl_syntax::MISSING_EDITION_HEADER),
                gate && edition == "next",
                "{arguments:?} gate={gate}: {stdout}"
            );
            for finding in findings {
                if finding["code"] != fsl_syntax::MISSING_EDITION_HEADER {
                    assert_eq!(
                        finding["severity"], severity,
                        "{arguments:?} gate={gate}: {stdout}"
                    );
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&directory);
}
