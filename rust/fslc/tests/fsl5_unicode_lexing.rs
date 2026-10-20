// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! fsl 5 plan §3 S2, through the native CLI: identifiers widen to UAX #31
//! only behind the `fsl5` gate and only in a document headed `fsl 5`; the
//! look-alike characters of design C.4 fail with their own code there, and
//! every other combination reads identifiers exactly as the published language
//! does.
//!
//! `fslc lint` is the command driven here because its parse already receives
//! the process's options; `check` and `kernel` parse in modules shared with
//! the Worker, which receive the options from the edition-wiring stage
//! (`docs/design/parse-entry-inventory.md`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("fslc-fsl5-unicode-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        Self(directory)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, source).expect("write fixture");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `(stdout, exit)` of `fslc <subcommand> <path>` with the gate unset or set.
fn fslc(subcommand: &str, path: &Path, gate: bool) -> (Vec<u8>, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .arg(subcommand)
        .arg(path)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if gate {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    }
    let output = command.output().expect("run fslc");
    (output.stdout, output.status.code().expect("fslc exit code"))
}

fn envelope(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).expect("fslc prints one JSON envelope")
}

fn line_column(output: &Value) -> (u64, u64) {
    (
        output["loc"]["line"].as_u64().expect("loc.line"),
        output["loc"]["column"].as_u64().expect("loc.column"),
    )
}

fn spec_with_state_field(header: &str, field: &str) -> String {
    format!("{header}spec S {{\n  state {{ {field}: Int }}\n}}\n")
}

/// Acceptance 1: `経費` stays an unexpected character unless both the gate and
/// the `fsl 5` header are present, and those outputs are byte-identical to
/// the gate-off output of the same file.
#[test]
fn japanese_identifier_needs_both_the_gate_and_the_fsl5_header() {
    let scratch = Scratch::new("acceptance-1");
    for (name, header, gate) in [
        ("no-header-gate-on.fsl", "", true),
        ("fsl4-gate-on.fsl", "fsl 4\n", true),
        ("fsl5-gate-off.fsl", "fsl 5\n", false),
    ] {
        let path = scratch.write(name, &spec_with_state_field(header, "経費"));
        for subcommand in ["lint", "check"] {
            let (stdout, exit) = fslc(subcommand, &path, gate);
            assert_eq!(exit, 2, "{subcommand} {name}");
            let output = envelope(&stdout);
            assert_eq!(output["kind"], "parse", "{name}: {output:#}");
            assert_eq!(output["diagnostic_code"], "FSL-PARSE", "{name}: {output:#}");
            assert!(
                output["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with("unexpected character '経'")),
                "{name}: {output:#}"
            );
            assert_eq!(
                fslc(subcommand, &path, false),
                (stdout, exit),
                "{subcommand} {name}: the gate changed the bytes"
            );
        }
    }

    // The lexer accepts the identifier; the header itself is not read as a
    // declaration yet, so dispatch rejects `fsl` as it does today.
    let path = scratch.write(
        "fsl5-gate-on.fsl",
        &spec_with_state_field("fsl 5\n", "経費"),
    );
    let (stdout, exit) = fslc("lint", &path, true);
    assert_eq!(exit, 2);
    let output = envelope(&stdout);
    assert_eq!(
        output["diagnostic_code"], "FSL-DIALECT-UNKNOWN",
        "{output:#}"
    );
    assert_eq!(line_column(&output), (1, 1));
}

/// Acceptance 2 (lexical rows of design C.4): each look-alike fails with its
/// own code at its own location under the gate and the `fsl 5` header, and
/// stays the published `FSL-PARSE` unexpected character without the gate.
#[test]
fn look_alike_characters_fail_with_their_own_code_behind_the_gate() {
    let scratch = Scratch::new("acceptance-2");
    for (name, field, code, column, hint) in [
        (
            "fullwidth-letters",
            "Ｃｌａｉｍ",
            "fullwidth_ascii_identifier",
            11,
            "'Claim'",
        ),
        (
            "fullwidth-digit",
            "x１",
            "fullwidth_ascii_identifier",
            11,
            "'x1'",
        ),
        ("fullwidth-hyphen", "a－b", "fullwidth_symbol", 12, "'-'"),
        (
            "middle-dot",
            "申請・承認",
            "middle_dot_identifier",
            13,
            "'・'",
        ),
        (
            "latin-cyrillic",
            "Сlaim",
            "mixed_script_identifier",
            11,
            "'Сlaim'",
        ),
    ] {
        let path = scratch.write(
            &format!("{name}.fsl"),
            &spec_with_state_field("fsl 5\n", field),
        );
        let (stdout, exit) = fslc("lint", &path, true);
        assert_eq!(exit, 2, "{name}");
        let output = envelope(&stdout);
        assert_eq!(output["kind"], "parse", "{name}: {output:#}");
        assert_eq!(output["diagnostic_code"], code, "{name}: {output:#}");
        assert_eq!(line_column(&output), (3, column), "{name}: {output:#}");
        assert!(
            output["message"]
                .as_str()
                .is_some_and(|message| message.contains(hint)),
            "{name}: message names {hint}: {output:#}"
        );

        let (stdout, exit) = fslc("lint", &path, false);
        assert_eq!(exit, 2, "{name}");
        let output = envelope(&stdout);
        assert_eq!(output["diagnostic_code"], "FSL-PARSE", "{name}: {output:#}");
        assert!(
            output["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("unexpected character")),
            "{name}: {output:#}"
        );
    }
}
