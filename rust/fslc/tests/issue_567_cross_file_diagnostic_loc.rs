// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #567: a kernel-stage failure inside a
//! `use ... from` component reported the *parent* document's path with the
//! *component's* line and column, so `loc` pointed at whatever happened to sit
//! at that position in the parent — for the compose error-gallery fixture, a
//! comment line. `docs/DESIGN-v1.md` G2 requires the output JSON alone to say
//! where the problem is, and a location in the wrong file is worse than none.
//!
//! `loc` is `{line, column}` with no `file`, so it can only ever mean "a
//! position in the file the envelope is about". The position is therefore the
//! parent's `use` declaration — which really is in the parent, and really is
//! the line to look at — and the component's own path and position moved into
//! the message. Neither file's identity is left implicit.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const PARENT: &str = "examples/gallery/errors/semantics_compose_component_parse_failure.fsl";
const COMPONENT: &str = "semantics_compose_broken_component.fsl";

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

/// The text at `loc` inside the parent document, so the assertion is about
/// what the position actually points at rather than about two numbers.
fn text_at(loc: &Value) -> String {
    let source = std::fs::read_to_string(workspace_root().join(PARENT)).expect("read parent");
    let line = usize::try_from(loc["line"].as_u64().expect("loc.line")).expect("line fits usize");
    let column =
        usize::try_from(loc["column"].as_u64().expect("loc.column")).expect("column fits usize");
    let text = source.lines().nth(line - 1).expect("loc.line exists");
    text.chars().skip(column - 1).collect()
}

#[test]
fn the_location_points_at_the_use_declaration_in_the_parent() {
    let (value, status) = run(&["check", PARENT]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["kind"], "semantics", "{value}");
    // Exact position, not "not null": a present-but-wrong location is the
    // defect, and `loc: {line: 7, column: 18}` was present the whole time.
    assert_eq!(value["loc"]["line"], 13, "{value}");
    assert_eq!(value["loc"]["column"], 3, "{value}");
    // And what actually sits there is the `use` declaration that pulls in the
    // broken component — pinning the numbers alone would survive the file
    // being re-indented into pointing somewhere else again.
    assert!(
        text_at(&value["loc"]).starts_with("use "),
        "loc points at {:?}, not a `use` declaration",
        text_at(&value["loc"])
    );
}

#[test]
fn the_message_names_both_files_and_both_positions() {
    let (value, _) = run(&["check", PARENT]);
    let message = value["message"].as_str().expect("message");
    // The component, with its own path and its own position.
    assert!(message.contains(COMPONENT), "{message}");
    assert!(
        message.contains(&format!("{COMPONENT}:7:18")),
        "the component's own position must survive in the message: {message}"
    );
    // The parent, with the `use` line.
    assert!(message.contains(&format!("{PARENT}:13:3")), "{message}");
    // The defect was pairing the parent's path with the component's position.
    assert!(
        !message.contains(&format!("{PARENT}:7:18")),
        "parent path is still paired with the component's position: {message}"
    );
}

#[test]
fn every_spec_reading_command_reports_the_same_corrected_location() {
    // The loader is shared, so a command that skipped this path would be the
    // interesting case.
    for args in [
        vec!["check", PARENT],
        vec!["verify", PARENT, "--depth", "2"],
        vec!["explain", PARENT],
        vec!["analyze", PARENT],
    ] {
        let (value, status) = run(&args);
        assert_eq!(status, 2, "{args:?}: {value}");
        assert_eq!(value["loc"]["line"], 13, "{args:?}: {value}");
        assert_eq!(value["loc"]["column"], 3, "{args:?}: {value}");
        assert!(
            value["message"]
                .as_str()
                .is_some_and(|message| message.contains(COMPONENT)),
            "{args:?}: {value}"
        );
    }
}

#[test]
fn an_unreadable_component_is_located_at_its_own_use_declaration() {
    // Same class: the read failure carried `1:1`, which in the parent is the
    // `compose` keyword rather than the import that failed.
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "issue-567-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    let path = directory.join("missing_component.fsl");
    std::fs::write(
        &path,
        "compose MissingComponent {\n  use Absent as absent from \"absent.fsl\"\n  state { x: Int }\n  init { x = 0 }\n  invariant I { x >= 0 }\n}\n",
    )
    .expect("write scratch spec");
    let (value, status) = run(&["check", &path.display().to_string()]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["loc"]["line"], 2, "{value}");
    assert_eq!(value["loc"]["column"], 3, "{value}");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.contains("absent.fsl")),
        "{value}"
    );
}

// --- positive: a healthy compose is untouched -----------------------------

#[test]
fn a_healthy_compose_document_still_checks() {
    // Re-anchoring must not touch the success path, and `specs/bank_system.fsl`
    // is the corpus compose that loads two real components.
    let (value, status) = run(&["check", "specs/bank_system.fsl"]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ok", "{value}");
}
