// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The syntax-edition header (fsl 5 plan S1,
//! `docs/design/parse-entry-inventory.md`, "Edition header").
//!
//! A document without a header is read as `fsl 4`; the #1434 type-boundary
//! sweep recorded that publishing `fsl 5` changed no headerless verdict. This
//! file holds the rest against corpus copies:
//!
//! - a copy with `fsl 4` added reads like the original (the kernel without
//!   positions, and `check`'s verdict);
//! - a copy with `fsl 5` added checks like the original under
//!   `--edition next` (exit, codes and severities), frozen in
//!   `tests/goldens/edition_header_next.txt` so the next edition's syntax
//!   stages change it as a reviewed diff. Locally,
//!   `FSLC_UPDATE_EDITION_GOLDEN=1` rewrites it from the published
//!   `--edition next` run; when `CI` is set, any set value fails the test
//!   instead of rewriting the golden and passing (#1413).
//!
//! and the header's own contract: conflicts, unknown editions, the dialects
//! without an edition, the literate fence, `fmt`, `lint` and `migrate`.

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use std::path::{Path, PathBuf};

use serde_json::Value;

#[path = "support/golden_update.rs"]
mod golden_update;
#[path = "support/mod.rs"]
mod support;
use support::{corpus_files, repo_relative, root, top_level_keyword};

/// `(stdout, exit)` of `fslc <arguments>` in `directory`.
fn fslc(directory: &Path, arguments: &[&str]) -> (String, i32) {
    let mut command = fslc_cli::command();
    command.args(arguments).current_dir(directory);
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
/// to every `.fsl` file (imports included) when given. A file that already
/// declares its edition (`examples/fsl5/`) is copied unchanged: a second
/// header line is a parse error, not the edition under test.
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
            let declared = matches!(
                fsl_syntax::edition_header(&source, &fsl_syntax::ParseOptions::stable()),
                Ok(Some(_))
            );
            let copy = if declared {
                source
            } else {
                with_header(&source, header)
            };
            std::fs::write(&destination, copy).expect("write copy");
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
        let (plain_kernel, plain_exit) = fslc(&plain, &["kernel", path]);
        let (four_kernel, four_exit) = fslc(&four, &["kernel", path]);
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
        let (plain_check, plain_status) = fslc(&plain, &["check", path]);
        let (four_check, four_status) = fslc(&four, &["check", path]);
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
        let (next, next_exit) = fslc(&plain, &["check", path, "--edition", "next"]);
        let (header, header_exit) = fslc(&five, &["check", path]);
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
    let update = golden_update::golden_update_requested_from_env("FSLC_UPDATE_EDITION_GOLDEN")
        .unwrap_or_else(|error| panic!("{error}"));
    if update {
        std::fs::write(&golden_path, &actual).expect("update edition golden");
    }
    let golden = std::fs::read_to_string(&golden_path).expect("read edition golden");
    assert_eq!(
        golden, actual,
        "the published `--edition next` corpus run moved; review the golden diff"
    );
    // fsl 5 plan S4 (decisions/046 Q2): a `fsl 5` requirements document does
    // not take the process form, so every corpus requirements file with a
    // `process`, and any file checked through one, fails with
    // `mixed_requirements_forms` where `--edition next` reads it. No other
    // row may differ.
    let mixed_forms = rows
        .iter()
        .filter(|(_, header)| header.contains("code=\"mixed_requirements_forms\""))
        .map(|(_, header)| header.split('\t').next().expect("row path"))
        .collect::<std::collections::BTreeSet<_>>();
    let root = root();
    // A document with its own header keeps it in the mirror, so it is not
    // read under an added `fsl 5`; an `fsl 5` document's surface already
    // carries the process its entities and events lower to.
    let process_requirements = paths
        .iter()
        .filter(|path| {
            let source = std::fs::read_to_string(root.join(path)).expect("read corpus file");
            matches!(
                fsl_syntax::edition_header(&source, &fsl_syntax::ParseOptions::stable()),
                Ok(None)
            ) && matches!(
                fsl_syntax::parse_surface_document(&source),
                Ok(fsl_syntax::SurfaceDocument::Requirements(requirements))
                    if requirements
                        .items
                        .iter()
                        .any(|item| matches!(item, fsl_syntax::RequirementsItem::Process(_)))
            )
        })
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert!(
        !process_requirements.is_empty(),
        "not vacuous: the corpus has process-form requirements"
    );
    let unflagged = process_requirements
        .iter()
        .filter(|path| !mixed_forms.contains(*path))
        .collect::<Vec<_>>();
    assert!(
        unflagged.is_empty(),
        "process-form requirements read under `fsl 5` without mixed_requirements_forms: {unflagged:?}"
    );
    let differences = rows
        .iter()
        .filter(|(next, header)| {
            next != header && !header.contains("code=\"mixed_requirements_forms\"")
        })
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
        let (stdout, exit) = fslc(&five, &["check", path]);
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
            let (stdout, exit) = fslc(&directory, &[command, file, "--edition", requested]);
            let output = json(&stdout);
            assert_eq!(exit, 2, "{command} {file} --edition {requested}: {stdout}");
            assert_eq!(
                output["diagnostic_code"],
                fsl_syntax::EDITION_CONFLICT,
                "{stdout}"
            );
            assert_eq!(output["loc"]["line"], 1, "{stdout}");
            // #1757: the message echoes the request as typed (`next`, not
            // its number `5`).
            assert_eq!(
                output["message"],
                format!(
                    "the document declares 'fsl {declared}' but --edition {requested} was requested at 1:1"
                ),
                "{stdout}"
            );
        }
    }
    for (file, requested) in [(&five, "5"), (&five, "next"), (&four, "4"), (&plain, "4")] {
        let (stdout, exit) = fslc(&directory, &["check", file, "--edition", requested]);
        assert_eq!(exit, 0, "{file} --edition {requested}: {stdout}");
    }

    let (stdout, exit) = fslc(&directory, &["check", &six]);
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

    // #1434: the numeric spellings are published; other values still refuse.
    for requested in ["6", "fsl5", ""] {
        let (stdout, exit) = fslc(&directory, &["check", &plain, "--edition", requested]);
        assert_eq!(exit, 2, "{stdout}");
        assert_eq!(
            json(&stdout)["message"],
            "--edition must be current, next, 4, or 5"
        );
    }
    let _ = std::fs::remove_dir_all(&directory);
}

/// #1440: `--edition current` follows the document's header in every command
/// that takes `--edition` (`docs/manual/LANGUAGE.md`, "`--edition current`
/// follows the document"). Only an explicit edition naming another edition
/// than the header is `edition_conflict`: `4` on `fsl 5`, and `5` or `next`
/// on `fsl 4`. `lint` and `migrate` once read `current` as an explicit `4`,
/// so an `fsl 5` document was refused with "--edition 4 was requested".
#[test]
fn edition_current_follows_the_header_in_every_command() {
    const DOMAIN: &str = "domain D { enum Status { Open, Closed } aggregate Doc { state { status: Status = Open } command Close {} event ClosedDoc {} decide Close { requires status == Open emits ClosedDoc } evolve ClosedDoc { status = Closed } } }\n";
    let directory = scratch("current-follows");
    let commands: [(&[&str], &str, &[&str]); 6] = [
        (&["check"], LEGACY, &[]),
        (&["verify"], LEGACY, &["--depth", "2", "--no-cache"]),
        (&["domain", "check"], DOMAIN, &["--depth", "2"]),
        (&["fmt"], LEGACY, &[]),
        (&["lint"], LEGACY, &[]),
        (&["migrate"], LEGACY, &[]),
    ];
    for (command, body, extra) in commands {
        for (header, declared) in [
            (None, None),
            (Some("fsl 4"), Some(4)),
            (Some("fsl 5"), Some(5)),
        ] {
            let file = write(
                &directory,
                "doc.fsl",
                &header.map_or_else(|| body.to_owned(), |header| with_header(body, header)),
            );
            let run = |option: Option<&str>| {
                let mut arguments = command.to_vec();
                arguments.push(&file);
                arguments.extend_from_slice(extra);
                if let Some(option) = option {
                    arguments.extend(["--edition", option]);
                }
                fslc(&directory, &arguments)
            };
            let cell =
                |option: Option<&str>| format!("{command:?} {header:?} --edition {option:?}");
            let mut outputs = std::collections::BTreeMap::new();
            for option in [None, Some("current"), Some("next"), Some("4"), Some("5")] {
                let (stdout, exit) = run(option);
                let conflict = match (declared, option) {
                    // #1757: the message echoes the spelling, `next` or `5`.
                    (Some(5), Some("4")) => Some("4"),
                    (Some(4), Some(spelling @ ("next" | "5"))) => Some(spelling),
                    _ => None,
                };
                if let Some(requested) = conflict {
                    let output = json(&stdout);
                    assert_eq!(exit, 2, "{}: {stdout}", cell(option));
                    assert_eq!(output["kind"], "parse", "{}: {stdout}", cell(option));
                    assert_eq!(
                        output["diagnostic_code"],
                        fsl_syntax::EDITION_CONFLICT,
                        "{}: {stdout}",
                        cell(option)
                    );
                    assert_eq!(
                        output["message"],
                        format!(
                            "the document declares 'fsl {}' but --edition {requested} was requested at 1:1",
                            declared.unwrap()
                        ),
                        "{}",
                        cell(option)
                    );
                } else {
                    assert!(
                        !stdout.contains(fsl_syntax::EDITION_CONFLICT),
                        "{}: {stdout}",
                        cell(option)
                    );
                    assert!(exit < 2, "{}: exit {exit}: {stdout}", cell(option));
                }
                let mut value =
                    serde_json::from_str(&stdout).unwrap_or_else(|_| Value::String(stdout.clone()));
                // `verify` reports elapsed times under `cost`, which differ
                // between two runs of one command; every other field is
                // compared, and the exclusion must find the field.
                if command == ["verify"] && conflict.is_none() {
                    let removed = value
                        .as_object_mut()
                        .and_then(|fields| fields.remove("cost"));
                    assert!(removed.is_some(), "{}: {stdout}", cell(option));
                }
                outputs.insert(option, (value, exit));
            }
            // `current` is the omitted option, except that `migrate` defaults
            // to the next edition: there `current` reads a headerless
            // document as `4` does. Since #1757 `migrate` publishes the
            // edition it read rather than the request, so the whole output
            // agrees, `edition` included: `next` only under `fsl 5`.
            let current = &outputs[&Some("current")];
            if command == ["migrate"] {
                assert_eq!(
                    current.0["edition"],
                    if declared == Some(5) {
                        "next"
                    } else {
                        "current"
                    },
                    "{current:?}"
                );
                let baseline = if header.is_none() { Some("4") } else { None };
                assert_eq!(current, &outputs[&baseline], "{}", cell(Some("current")));
            } else {
                assert_eq!(current, &outputs[&None], "{}", cell(Some("current")));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn fsl_stays_an_identifier_outside_the_header_line() {
    let directory = scratch("identifier");
    let spec = "spec S {\n  const fsl = 1\n  state { n: 0..1 = 0 }\n  action a() { requires n < fsl  n = n + 1 }\n}\n";
    let plain = write(&directory, "plain.fsl", spec);
    let five = write(&directory, "five.fsl", &with_header(spec, "fsl 5"));
    for file in [&plain, &five] {
        let (stdout, exit) = fslc(&directory, &["check", file]);
        assert_eq!(exit, 0, "{file}: {stdout}");
        assert_eq!(json(&stdout)["result"], "ok");
    }
    let (plain_kernel, _) = fslc(&directory, &["kernel", &plain]);
    let (five_kernel, _) = fslc(&directory, &["kernel", &five]);
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
    let (stdout, exit) = fslc(&directory, &["check", &five]);
    let output = json(&stdout);
    assert_eq!(exit, 0, "{stdout}");
    assert_eq!(output["fsl_edition"], 5, "{stdout}");
    assert_eq!(output["edition"], "next", "{stdout}");
    let (stdout, exit) = fslc(&directory, &["check", &four]);
    assert_eq!(
        (exit, &json(&stdout)["fsl_edition"]),
        (0, &Value::from(4)),
        "{stdout}"
    );
    let (stdout, exit) = fslc(&directory, &["check", &five, "--edition", "4"]);
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

    let (formatted, exit) = fslc(&directory, &["fmt", &five]);
    assert_eq!(exit, 0, "{formatted}");
    assert!(formatted.starts_with("fsl 5\n"), "{formatted}");
    let formatted_file = write(&directory, "formatted.fsl", &formatted);
    let (check, exit) = fslc(&directory, &["fmt", "--check", &formatted_file]);
    assert_eq!(exit, 0, "a formatted header is stable: {check}");

    let (lint, exit) = fslc(&directory, &["lint", &plain, "--edition", "5"]);
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
    let (lint, exit) = fslc(&directory, &["lint", &five, "--edition", "5"]);
    assert_eq!(
        (exit, &json(&lint)["finding_count"]),
        (0, &Value::from(0)),
        "{lint}"
    );
    // #1434: `--edition next` is `--edition 5`, so it reports the missing
    // header too; a lint for the current edition does not.
    let (lint, exit) = fslc(&directory, &["lint", &plain, "--edition", "next"]);
    assert_eq!(
        (exit, &json(&lint)["files"][0]["findings"][0]["code"]),
        (1, &Value::from(fsl_syntax::MISSING_EDITION_HEADER)),
        "{lint}"
    );
    let (lint, exit) = fslc(&directory, &["lint", &plain]);
    assert_eq!(
        (exit, &json(&lint)["finding_count"]),
        (0, &Value::from(0)),
        "{lint}"
    );

    let (migrate, exit) = fslc(&directory, &["migrate", &plain, "--write"]);
    assert_eq!(exit, 0, "{migrate}");
    assert_eq!(
        std::fs::read_to_string(directory.join(&plain)).expect("read migrated"),
        format!("// 日本語の先頭コメント\nfsl 5\n{LEGACY}")
    );
    let (again, _) = fslc(&directory, &["migrate", &plain]);
    assert_eq!(json(&again)["changed"], 0, "{again}");
    let untouched = write(&directory, "untouched.fsl", &source);
    let (migrate, _) = fslc(
        &directory,
        &["migrate", &untouched, "--edition", "current", "--write"],
    );
    assert_eq!(json(&migrate)["changed"], 0, "{migrate}");
    assert_eq!(
        std::fs::read_to_string(directory.join(&untouched)).expect("read untouched"),
        source,
        "a migration for the current edition inserts no header"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// `migrate` defaults to the next edition, so an explicit `--edition current`
/// must stay distinct from an omitted flag, and it inserts no header.
#[test]
fn migrate_with_edition_current_keeps_the_current_edition() {
    let directory = scratch("migrate-current");
    let legacy = write(
        &directory,
        "legacy.fsl",
        "spec S {\n  state { n: 0..3 }\n  init { n = 0 }\n  action bump() \"REQ-1: bump\" {\n    requires n < 3\n    n = n + 1\n  }\n  invariant Bound { n >= 0 }\n}\n",
    );
    for (arguments, edition, severity) in [
        (&["migrate", legacy.as_str()][..], "next", "error"),
        (
            &["migrate", legacy.as_str(), "--edition", "current"][..],
            "current",
            "warning",
        ),
    ] {
        let (stdout, exit) = fslc(&directory, arguments);
        let output = json(&stdout);
        assert_eq!(exit, 0, "{arguments:?}: {stdout}");
        assert_eq!(output["edition"], edition, "{arguments:?}");
        let findings = output["files"][0]["findings"]
            .as_array()
            .unwrap_or_else(|| panic!("findings: {stdout}"));
        assert!(!findings.is_empty(), "{arguments:?}: {stdout}");
        let codes = findings
            .iter()
            .map(|finding| finding["code"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(
            codes.contains(&fsl_syntax::MISSING_EDITION_HEADER),
            edition == "next",
            "{arguments:?}: {stdout}"
        );
        for finding in findings {
            if finding["code"] != fsl_syntax::MISSING_EDITION_HEADER {
                assert_eq!(finding["severity"], severity, "{arguments:?}: {stdout}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&directory);
}

// #1818 D1: the development minor `fsl 5.DEV_MINOR`, read only behind the
// `fsl5-minor` gate. It has no form of its own yet, so a document headed by
// it must read exactly as the `fsl 5` document does.

/// The development minor's header line.
fn minor_header() -> String {
    format!("fsl 5.{}", fsl_syntax::DEV_MINOR)
}

/// `(stdout, exit)` of `fslc <arguments>` in `directory` behind the gate.
fn fslc_gated(directory: &Path, arguments: &[&str]) -> (String, i32) {
    let mut command = fslc_cli::command();
    command
        .args(arguments)
        .current_dir(directory)
        .env("FSLC_UNSTABLE", "fsl5-minor");
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

/// `source` headed by `header`: an `fsl 5` header is replaced, a document
/// without one gets it, and an `fsl 4` document is kept.
fn with_minor_header(source: &str, header: &str) -> String {
    match fsl_syntax::edition_header(source, &fsl_syntax::ParseOptions::stable()) {
        Ok(Some(declared)) if declared.edition == fsl_syntax::Edition::Fsl5 => format!(
            "{}{header}{}",
            &source[..declared.span.start.offset],
            &source[declared.span.end.offset..]
        ),
        Ok(Some(_)) => source.to_owned(),
        _ => with_header(source, header),
    }
}

/// [`mirror`] for the minor header ([`with_minor_header`] on every `.fsl`).
fn minor_mirror(label: &str, header: &str) -> PathBuf {
    fn copy(from: &Path, to: &Path, header: &str) {
        std::fs::create_dir_all(to).expect("create mirror directory");
        for entry in std::fs::read_dir(from).expect("read corpus directory") {
            let path = entry.expect("corpus entry").path();
            let destination = to.join(path.file_name().expect("file name"));
            if path.is_dir() {
                copy(&path, &destination, header);
            } else if path.extension().is_some_and(|extension| extension == "fsl") {
                let source = std::fs::read_to_string(&path).expect("read corpus file");
                std::fs::write(&destination, with_minor_header(&source, header))
                    .expect("write copy");
            } else {
                std::fs::copy(&path, &destination).expect("copy corpus file");
            }
        }
    }
    let target = scratch(label);
    for tree in ["specs", "examples"] {
        copy(&root().join(tree), &target.join(tree), header);
    }
    target
}

/// `stdout` as JSON with `directory` written as `<mirror>` and the
/// wall-clock and allocator fields removed (`memory_mb` is Z3 allocator
/// noise, as `edition_all_commands.rs` masks it).
fn comparable(stdout: &str, directory: &Path) -> Value {
    fn mask(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for key in ["elapsed_s", "check_elapsed_s", "memory_mb"] {
                    fields.remove(key);
                }
                fields.values_mut().for_each(mask);
            }
            Value::Array(items) => items.iter_mut().for_each(mask),
            _ => {}
        }
    }
    let mut value = json(&stdout.replace(&*directory.to_string_lossy(), "<mirror>"));
    mask(&mut value);
    value
}

/// `output` without the `fsl_edition_minor` a minor document adds, which
/// must be [`fsl_syntax::DEV_MINOR`] exactly where `fsl_edition` is 5.
fn without_minor(mut output: Value) -> Result<Value, String> {
    let Value::Object(fields) = &mut output else {
        return Ok(output);
    };
    let minor = fields.remove("fsl_edition_minor");
    match (fields.get("fsl_edition"), minor) {
        (Some(major), Some(minor)) if major == 5 && minor == fsl_syntax::DEV_MINOR => Ok(output),
        (Some(major), None) if major != 5 => Ok(output),
        (None, None) => Ok(output),
        (major, minor) => Err(format!(
            "fsl_edition {major:?} with fsl_edition_minor {minor:?}"
        )),
    }
}

/// Acceptance of #1818 D1 PR1: every document the `fsl 5` corpus test reads,
/// headed `fsl 5.DEV_MINOR` with the gate on (a document headed `fsl 5`
/// re-headed, an `fsl 4` one kept), gives `kernel` and `check` the output
/// the `fsl 5` mirror gives without the gate, but for `fsl_edition_minor`.
/// The `fsl 5` mirror re-heads the `examples/fsl5/` documents the same way,
/// so the Japanese ones are compared too.
#[test]
fn a_development_minor_header_reads_the_corpus_as_fsl_5_does() {
    let (paths, _) = edition_corpus();
    let header = minor_header();
    // `fsl 5` padded with blanks to the minor header's length, so byte
    // offsets (a finding's edits) match too.
    let five = minor_mirror(
        "family5",
        &format!("{:width$}", "fsl 5", width = header.len()),
    );
    let minor = minor_mirror("family5-minor", &header);
    let minor_rows = std::sync::atomic::AtomicUsize::new(0);
    let failures = each_path(&paths, |path| {
        let mut failures = Vec::new();
        for command in ["kernel", "check"] {
            let (expected, expected_exit) = fslc(&five, &[command, path]);
            let (actual, actual_exit) = fslc_gated(&minor, &[command, path]);
            let expected = comparable(&expected, &five);
            let actual = comparable(&actual, &minor);
            if actual.get("fsl_edition_minor").is_some() {
                minor_rows.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            match without_minor(actual) {
                Ok(actual) if actual == expected && actual_exit == expected_exit => {}
                Ok(actual) => failures.push(format!(
                    "{path}: {command} exit {expected_exit} -> {actual_exit}\nfsl 5:  {expected}\n{header}: {actual}"
                )),
                Err(error) => failures.push(format!("{path}: {command}: {error}")),
            }
        }
        failures
    })
    .concat();
    let _ = std::fs::remove_dir_all(&five);
    let _ = std::fs::remove_dir_all(&minor);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        minor_rows.into_inner() > 100,
        "not vacuous: most corpus `check`s report the minor header"
    );
}

/// Without the gate a minor header is an unsupported edition in every
/// command and `--edition 5.N` is a usage error; `fsl 5.0` is `fsl 5`.
#[test]
fn a_development_minor_needs_the_gate() {
    let directory = scratch("minor-gate");
    let header = minor_header();
    // One file name in three directories, so `kernel` spans compare.
    let [minor, zero, five] = ["minor", "zero", "five"].map(|label| directory.join(label));
    for (directory, header) in [
        (&minor, header.as_str()),
        (&zero, "fsl 5.0"),
        (&five, "fsl 5"),
    ] {
        std::fs::create_dir_all(directory).expect("create directory");
        write(directory, "spec.fsl", &with_header(LEGACY, header));
    }
    let run = |directory: &Path, gated: bool, arguments: &[&str]| {
        let (stdout, exit) = if gated {
            fslc_gated(directory, arguments)
        } else {
            fslc(directory, arguments)
        };
        (comparable(&stdout, directory), exit)
    };
    let message = format!("unsupported syntax edition '{header}'; supported editions: 4, 5 at 1:1");
    for command in ["check", "verify", "kernel"] {
        let (output, exit) = run(&minor, false, &[command, "spec.fsl"]);
        assert_eq!(exit, 2, "{command}: {output}");
        assert_eq!(
            output["diagnostic_code"],
            fsl_syntax::UNSUPPORTED_EDITION,
            "{command}: {output}"
        );
        assert_eq!(output["message"], message, "{command}: {output}");
        // `fsl 5.0` is `fsl 5`, with or without the gate.
        let expected = run(&five, false, &[command, "spec.fsl"]);
        assert_eq!(
            run(&zero, false, &[command, "spec.fsl"]),
            expected,
            "{command}"
        );
        assert_eq!(
            run(&zero, true, &[command, "spec.fsl"]),
            expected,
            "{command} behind the gate"
        );
        // Behind the gate the minor document reads as `fsl 5`.
        let (gated, exit) = run(&minor, true, &[command, "spec.fsl"]);
        assert_eq!(exit, expected.1, "{command}: {gated}");
        assert_eq!(without_minor(gated), Ok(expected.0), "{command}");
    }
    for command in ["fmt", "lint", "migrate"] {
        let (stdout, exit) = fslc(&minor, &[command, "spec.fsl"]);
        assert_eq!(exit, 2, "{command}: {stdout}");
        assert!(
            stdout.contains(fsl_syntax::UNSUPPORTED_EDITION),
            "{command}: {stdout}"
        );
    }
    let usage = "--edition must be current, next, 4, or 5";
    for command in ["check", "verify", "fmt", "lint", "migrate"] {
        for value in [header.trim_start_matches("fsl "), "5.0"] {
            let (stdout, exit) = fslc(&five, &[command, "spec.fsl", "--edition", value]);
            let output = json(&stdout);
            assert_eq!(exit, 2, "{command} --edition {value}: {stdout}");
            assert_eq!(output["kind"], "usage", "{stdout}");
            assert_eq!(output["message"], usage, "{stdout}");
        }
    }
    let (stdout, _) = fslc_gated(&five, &["check", "spec.fsl", "--edition", "5.0"]);
    assert_eq!(
        json(&stdout)["message"],
        usage,
        "`5.0` is no request spelling"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// Behind the gate a conflict compares major and minor, and echoes both
/// spellings; a header-less document takes `--edition 5.N` as `next`.
#[test]
fn a_development_minor_conflicts_by_major_and_minor() {
    let directory = scratch("minor-conflict");
    let header = minor_header();
    let number = header.trim_start_matches("fsl ").to_owned();
    let minor = write(&directory, "minor.fsl", &with_header(LEGACY, &header));
    let zero = write(&directory, "zero.fsl", &with_header(LEGACY, "fsl 5.0"));
    let five = write(&directory, "five.fsl", &with_header(LEGACY, "fsl 5"));
    let four = write(&directory, "four.fsl", &with_header(LEGACY, "fsl 4"));
    let plain = write(&directory, "plain.fsl", LEGACY);
    for (file, requested, declared) in [
        (&minor, "5", number.as_str()),
        (&minor, "next", number.as_str()),
        (&minor, "4", number.as_str()),
        (&five, number.as_str(), "5"),
        (&zero, number.as_str(), "5"),
        (&four, number.as_str(), "4"),
    ] {
        for command in ["check", "verify", "fmt", "lint", "migrate"] {
            let (stdout, exit) = fslc_gated(&directory, &[command, file, "--edition", requested]);
            assert_eq!(exit, 2, "{command} {file} --edition {requested}: {stdout}");
            assert!(
                stdout.contains(fsl_syntax::EDITION_CONFLICT)
                    && stdout.contains(&format!(
                        "the document declares 'fsl {declared}' but --edition {requested} was requested"
                    )),
                "{command} {file} --edition {requested}: {stdout}"
            );
        }
    }
    for (file, requested) in [
        (&minor, number.as_str()),
        (&minor, "current"),
        (&plain, number.as_str()),
    ] {
        let (stdout, exit) = fslc_gated(&directory, &["check", file, "--edition", requested]);
        assert_eq!(exit, 0, "{file} --edition {requested}: {stdout}");
    }
    let (as_minor, _) = fslc_gated(&directory, &["check", &plain, "--edition", &number]);
    let (as_next, _) = fslc(&directory, &["check", &plain, "--edition", "next"]);
    assert_eq!(
        comparable(&as_minor, &directory),
        comparable(&as_next, &directory)
    );
    let _ = std::fs::remove_dir_all(&directory);
}

/// Behind the gate `fmt` keeps a minor document byte for byte, and `fmt
/// --check`, `lint` and `migrate` report on it what they report on the `fsl
/// 5` document (no finding), plus `fsl_edition_minor`. A run over an `fsl 5`
/// and a minor document publishes no `fsl_edition`: they declare different
/// editions.
#[test]
fn fmt_lint_and_migrate_read_a_development_minor_document_as_fsl_5() {
    let header = minor_header();
    let example = std::fs::read_to_string(root().join("examples/fsl5/leave_request.ja.fsl"))
        .expect("read example");
    let five_dir = scratch("minor-tools-5");
    let minor_dir = scratch("minor-tools-minor");
    let five = write(&five_dir, "leave.fsl", &example);
    let minor_source = with_minor_header(&example, &header);
    assert!(minor_source.starts_with(&format!("{header}\n")));
    let minor = write(&minor_dir, "leave.fsl", &minor_source);
    let (formatted_five, exit) = fslc(&five_dir, &["fmt", &five]);
    assert_eq!((exit, formatted_five.as_str()), (0, example.as_str()));
    let (formatted, exit) = fslc_gated(&minor_dir, &["fmt", &minor]);
    assert_eq!(
        (exit, formatted.as_str()),
        (0, minor_source.as_str()),
        "fmt keeps the minor document byte for byte"
    );
    for arguments in [
        vec!["fmt", "--check", "leave.fsl"],
        vec!["lint", "leave.fsl"],
        vec!["migrate", "leave.fsl"],
        vec!["migrate", "leave.fsl", "--write"],
    ] {
        let (expected, expected_exit) = fslc(&five_dir, &arguments);
        let (actual, actual_exit) = fslc_gated(&minor_dir, &arguments);
        assert_eq!(actual_exit, expected_exit, "{arguments:?}: {actual}");
        assert_eq!(expected_exit, 0, "{arguments:?}: {expected}");
        let actual = comparable(&actual, &minor_dir);
        assert_eq!(
            actual["fsl_edition_minor"],
            fsl_syntax::DEV_MINOR,
            "{arguments:?}: {actual}"
        );
        assert_eq!(
            without_minor(actual),
            Ok(comparable(&expected, &five_dir)),
            "{arguments:?}"
        );
    }
    let (lint, _) = fslc_gated(&minor_dir, &["lint", "leave.fsl"]);
    assert_eq!(json(&lint)["finding_count"], 0, "{lint}");
    assert_eq!(
        std::fs::read_to_string(minor_dir.join("leave.fsl")).expect("read back"),
        minor_source,
        "migrate --write leaves the minor document as it is"
    );
    write(&minor_dir, "five.fsl", &example);
    for command in ["lint", "migrate"] {
        let (mixed, exit) = fslc_gated(&minor_dir, &[command, "five.fsl", "leave.fsl"]);
        let output = json(&mixed);
        assert_eq!(exit, 0, "{command}: {mixed}");
        assert_eq!(output["edition"], "next", "{command}: {mixed}");
        assert!(
            output.get("fsl_edition").is_none() && output.get("fsl_edition_minor").is_none(),
            "{command}: `fsl 5` and `{header}` are different editions: {mixed}"
        );
    }
    let _ = std::fs::remove_dir_all(&five_dir);
    let _ = std::fs::remove_dir_all(&minor_dir);
}

/// The requirements checks the enumeration defers to the solver run for a
/// minor document as for the `fsl 5` one: `check`, `verify` and `kernel`
/// reject the decisions/048 J3 fixtures with the same envelope.
#[test]
fn the_solver_refutes_a_development_minor_document_as_it_refutes_fsl_5() {
    let header = minor_header();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fsl5_solver");
    for name in ["large_domain.fsl", "resource_condition.fsl"] {
        let source = std::fs::read_to_string(fixtures.join(name)).expect("read fixture");
        let five = scratch("minor-solver-5");
        let minor = scratch("minor-solver-minor");
        // `fsl 5` padded to the minor header's length, so offsets match.
        write(
            &five,
            "doc.fsl",
            &with_minor_header(
                &source,
                &format!("{:width$}", "fsl 5", width = header.len()),
            ),
        );
        write(&minor, "doc.fsl", &with_minor_header(&source, &header));
        for command in [
            vec!["check"],
            vec!["verify", "--depth", "3"],
            vec!["kernel"],
        ] {
            let arguments = [command.as_slice(), &["doc.fsl"]].concat();
            let (expected, expected_exit) = fslc(&five, &arguments);
            let (actual, actual_exit) = fslc_gated(&minor, &arguments);
            let expected = comparable(&expected, &five);
            assert_eq!(expected_exit, 2, "{name} {command:?}: {expected}");
            assert!(
                expected["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("checked by the solver")),
                "{name} {command:?}: {expected}"
            );
            assert_eq!(actual_exit, expected_exit, "{name} {command:?}");
            assert_eq!(
                without_minor(comparable(&actual, &minor)),
                Ok(expected),
                "{name} {command:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&five);
        let _ = std::fs::remove_dir_all(&minor);
    }
}
