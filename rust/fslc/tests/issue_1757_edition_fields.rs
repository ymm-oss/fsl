// SPDX-License-Identifier: Apache-2.0

//! #1757: every command that takes `--edition` publishes the edition a
//! document was read in, and its header's number, by one rule
//! (`docs/manual/LANGUAGE.md`, "Editions"); a conflict echoes the request as
//! it was typed.
//!
//! The matrix is `check`, `verify`, `domain check`, `fmt --check`, `lint` and
//! `migrate` × {headerless, `fsl 4`, `fsl 5`} × {omitted, `current`, `next`,
//! `4`, `5`}. Each cell's expectation is derived from the rule, not from a
//! recorded run:
//!
//! - the read edition is the header's, else the request's, else the
//!   command's default (`next` for `migrate`, `current` otherwise);
//! - `check`, `verify` and `domain check` carry `"edition": "next"` only
//!   when the document was read in the next edition; `fmt --check`, `lint`
//!   and `migrate` always carry `edition` (`current` or `next`), as 4.12 did;
//! - `fsl_edition` is present exactly when the document has a header.
//!
//! Before #1757, `lint`, `migrate` and `fmt --check` published the request
//! instead (`"current"` for an `fsl 5` document read without `--edition`)
//! and never `fsl_edition`, and `--edition next` conflicting with `fsl 4`
//! was reported as "--edition 5 was requested".

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const SPEC: &str =
    "spec S {\n  state { n: 0..2 = 0 }\n  action inc() { requires n < 2  n = n + 1 }\n}\n";
const DOMAIN: &str = "domain D { enum Status { Open, Closed } aggregate Doc { state { status: Status = Open } command Close {} event ClosedDoc {} decide Close { requires status == Open emits ClosedDoc } evolve ClosedDoc { status = Closed } } }\n";

/// A scratch directory unique to this process and `label`, emptied first.
fn scratch(label: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("fslc-issue-1757-{}-{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn write(directory: &Path, name: &str, header: Option<u32>, body: &str) -> String {
    let contents = header.map_or_else(|| body.to_owned(), |number| format!("fsl {number}\n{body}"));
    std::fs::write(directory.join(name), contents).expect("write fixture");
    name.to_owned()
}

/// `(output, exit)` of `fslc <arguments>` in `directory`.
fn fslc(directory: &Path, arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(directory)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    let value = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("fslc {arguments:?}: {error}: {stdout}"));
    (value, output.status.code().expect("exit status"))
}

/// The edition an `--edition` spelling requests; `None` for `current` and an
/// omitted option.
fn requested(option: Option<&str>) -> Option<u32> {
    match option {
        None | Some("current") => None,
        Some("next" | "5") => Some(5),
        Some("4") => Some(4),
        Some(other) => panic!("unpublished --edition {other}"),
    }
}

#[test]
fn every_edition_command_publishes_the_edition_it_read() {
    let directory = scratch("matrix");
    let commands: [(&[&str], &str, &[&str]); 6] = [
        (&["check"], SPEC, &[]),
        (&["verify"], SPEC, &["--depth", "2"]),
        (&["domain", "check"], DOMAIN, &["--depth", "2"]),
        (&["fmt", "--check"], SPEC, &[]),
        (&["lint"], SPEC, &[]),
        (&["migrate"], SPEC, &[]),
    ];
    let mut failures = Vec::new();
    let mut cells = 0;
    for (command, body, extra) in commands {
        let report = matches!(command[0], "fmt" | "lint" | "migrate");
        for header in [None, Some(4), Some(5)] {
            let file = write(&directory, "doc.fsl", header, body);
            for option in [None, Some("current"), Some("next"), Some("4"), Some("5")] {
                cells += 1;
                let mut arguments = command.to_vec();
                arguments.push(&file);
                arguments.extend_from_slice(extra);
                if let Some(option) = option {
                    arguments.extend(["--edition", option]);
                }
                let (output, exit) = fslc(&directory, &arguments);
                let cell = format!("{arguments:?} header {header:?}");
                let request = requested(option);
                if let (Some(declared), Some(wanted)) = (header, request)
                    && declared != wanted
                {
                    let message = format!(
                        "the document declares 'fsl {declared}' but --edition {} was requested at 1:1",
                        option.expect("an explicit request")
                    );
                    if (exit, &output["diagnostic_code"], &output["message"])
                        != (2, &Value::from("edition_conflict"), &Value::from(message))
                    {
                        failures.push(format!("{cell}: conflict expected, got {exit}: {output}"));
                    }
                    continue;
                }
                let default = if command == ["migrate"] && option.is_none() {
                    5
                } else {
                    4
                };
                let read = header.or(request).unwrap_or(default);
                let edition = match (read, report) {
                    (5, _) => Value::from("next"),
                    (_, true) => Value::from("current"),
                    (_, false) => Value::Null,
                };
                let fsl_edition = header.map_or(Value::Null, Value::from);
                let produced = (
                    output.get("edition").cloned().unwrap_or(Value::Null),
                    output.get("fsl_edition").cloned().unwrap_or(Value::Null),
                );
                if exit == 2 || produced != (edition.clone(), fsl_edition.clone()) {
                    failures.push(format!(
                        "{cell}: expected edition {edition}, fsl_edition {fsl_edition}; \
                         produced {produced:?}, exit {exit}: {output}"
                    ));
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&directory);
    assert_eq!(cells, 90);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A report over several documents says `next` only when every one was read
/// in the next edition, and `fsl_edition` only when every one declares the
/// same header; a report that read nothing keeps the request.
#[test]
fn a_report_over_several_documents_publishes_what_they_share() {
    let directory = scratch("several");
    let plain = write(&directory, "plain.fsl", None, SPEC);
    let four = write(&directory, "four.fsl", Some(4), SPEC);
    let four_b = write(&directory, "four_b.fsl", Some(4), SPEC);
    let five = write(&directory, "five.fsl", Some(5), SPEC);
    let five_b = write(&directory, "five_b.fsl", Some(5), SPEC);
    std::fs::create_dir_all(directory.join("empty")).expect("create empty directory");
    let cases: [(&[&str], &str, Option<u32>); 9] = [
        (&["lint", &plain, &five], "current", None),
        (&["lint", &five, &five_b], "next", Some(5)),
        (&["lint", &four, &four_b], "current", Some(4)),
        (&["lint", "--edition", "next", &plain, &five], "next", None),
        (&["fmt", "--check", &four, &five], "current", None),
        (&["fmt", "--check", &five, &five_b], "next", Some(5)),
        (&["migrate", &four, &five], "current", None),
        (&["lint", "empty"], "current", None),
        (&["lint", "--edition", "next", "empty"], "next", None),
    ];
    let mut failures = Vec::new();
    for (arguments, edition, fsl_edition) in cases {
        let (output, exit) = fslc(&directory, arguments);
        let produced = (
            output.get("edition").cloned().unwrap_or(Value::Null),
            output.get("fsl_edition").cloned().unwrap_or(Value::Null),
        );
        let expected = (
            Value::from(edition),
            fsl_edition.map_or(Value::Null, Value::from),
        );
        if exit == 2 || produced != expected {
            failures.push(format!(
                "{arguments:?}: expected {expected:?}, produced {produced:?}, exit {exit}: {output}"
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&directory);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
