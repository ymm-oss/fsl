// SPDX-License-Identifier: Apache-2.0

//! #1694: a second `implements` is refused at that clause. The seam took the
//! first clause and dropped the rest, so the second contract was never checked:
//! writing a broken contract second passed, and writing it first failed. The
//! packets are pinned whole, in both orders and both editions.

#[path = "support/issue_1517_scratch.rs"]
mod scratch;

use serde_json::{Value, json};
use std::{fs, process::Command};

/// The contract the implementation below refines.
const HELD: &str =
    "spec Held { state { done: Bool } init { done = false } action finish() { done = true } }\n";
/// The contract it breaks: `finish` may not run from the initial state.
const BROKEN: &str = "spec Broken { state { done: Bool } init { done = false } action finish() { requires done done = true } }\n";

const USE_HELD: &str = "implements Held from \"held.fsl\" { }";
const USE_BROKEN: &str = "implements Broken from \"broken.fsl\" { }";
const USE_MISSING: &str = "implements Missing from \"missing.fsl\" { }";

const FILES: [(&str, &str); 3] = [
    ("held.fsl", HELD),
    ("broken.fsl", BROKEN),
    (
        "map.fsl",
        "refinement M { impl Impl abs Held map done = done }\n",
    ),
];

/// The clauses start on line 2 (`stable`) or line 3 (`next`), at column 3.
fn stable(clauses: &[&str]) -> String {
    format!(
        "requirements Impl {{\n  {}\n  state {{ done: Bool }}\n  init {{ done = false }}\n  action finish() {{ done = true }}\n  invariant Typed {{ true }}\n}}\n",
        clauses.join("\n  ")
    )
}

fn next(clauses: &[&str]) -> String {
    format!("fsl 5\n{}", stable(clauses))
}

struct Run {
    packet: Value,
    exit: i32,
}

fn run_in(id: &str, files: &[(&str, &str)], arguments: &[&str]) -> Run {
    let scratch = scratch::Scratch::new("FSLC_1694_INPUTS", id, &format!("1694-{id}"));
    for (name, source) in files {
        fs::write(scratch.path().join(name), source).expect("fixture");
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(arguments)
        .current_dir(scratch.path())
        .env("FSLC_CACHE", "off");
    let output = command.output().expect("CLI");
    Run {
        packet: serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{id}: {error}: {} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        }),
        exit: output.status.code().expect("exit"),
    }
}

fn run(id: &str, root: &str, arguments: &[&str]) -> Run {
    let mut files = FILES.to_vec();
    files.push(("root.fsl", root));
    run_in(id, &files, arguments)
}

fn versions() -> Value {
    fsl_core::version_metadata(
        "fslc-rust",
        env!("CARGO_PKG_VERSION"),
        "native-z3",
        fsl_solver_z3::version(),
    )
}

fn refused(message: &str, line: u32) -> Value {
    json!({"fsl":"1.0","result":"error","kind":"parse","message":message,
        "diagnostic_code":"FSL-PARSE","loc":{"line":line,"column":3},"versions":versions()})
}

/// The parser adds the clause's own position to every message it reports.
fn message(second: &str, first: &str, line: u32) -> String {
    format!(
        "duplicate implements '{second}' (a requirements document declares one, and '{first}' came first) at {line}:3"
    )
}

fn assert_refused(id: &str, root: &str, second: &str, first: &str, line: u32) {
    let run = run(id, root, &["check", "root.fsl"]);
    assert_eq!(
        (run.exit, &run.packet),
        (2, &refused(&message(second, first, line), line)),
        "{id}"
    );
}

/// Base: `ok` with `refines`, and the broken contract unread.
#[test]
fn broken_contract_written_second_is_refused_at_it() {
    assert_refused(
        "second-stable",
        &stable(&[USE_HELD, USE_BROKEN]),
        "Broken",
        "Held",
        3,
    );
}

/// Base: the first clause is the one read, so this order failed for another reason.
#[test]
fn broken_contract_written_first_is_refused_at_the_held_one() {
    assert_refused(
        "first-stable",
        &stable(&[USE_BROKEN, USE_HELD]),
        "Held",
        "Broken",
        3,
    );
}

#[test]
fn next_edition_refuses_in_both_orders() {
    for (id, clauses, second, first) in [
        ("second-next", [USE_HELD, USE_BROKEN], "Broken", "Held"),
        ("first-next", [USE_BROKEN, USE_HELD], "Held", "Broken"),
    ] {
        let run = run(id, &next(&clauses), &["check", "root.fsl"]);
        assert_eq!(run.exit, 2, "{id}: {}", run.packet);
        assert_eq!(run.packet["kind"], "parse", "{id}");
        assert_eq!(
            run.packet["message"],
            json!(message(second, first, 4)),
            "{id}"
        );
        assert_eq!(run.packet["loc"], json!({"line":4,"column":3}), "{id}");
    }
}

/// Reading the file is what failed on base, in the order that names it first.
#[test]
fn a_contract_that_cannot_be_read_is_refused_without_reading_it() {
    for (id, clauses, second, first) in [
        ("missing-second", [USE_HELD, USE_MISSING], "Missing", "Held"),
        ("missing-first", [USE_MISSING, USE_HELD], "Held", "Missing"),
    ] {
        assert_refused(id, &stable(&clauses), second, first, 3);
    }
}

#[test]
fn the_same_contract_written_twice_is_refused() {
    assert_refused(
        "same-stable",
        &stable(&[USE_HELD, USE_HELD]),
        "Held",
        "Held",
        3,
    );
    let run = run(
        "same-next",
        &next(&[USE_HELD, USE_HELD]),
        &["check", "root.fsl"],
    );
    assert_eq!(run.exit, 2, "{}", run.packet);
    assert_eq!(
        run.packet["message"],
        json!(message("Held", "Held", 4)),
        "{}",
        run.packet
    );
}

#[test]
fn clauses_apart_are_refused_at_the_later_one() {
    let root = format!(
        "requirements Impl {{\n  {USE_HELD}\n  state {{ done: Bool }}\n  init {{ done = false }}\n  {USE_BROKEN}\n  action finish() {{ done = true }}\n}}\n"
    );
    let run = run("apart", &root, &["check", "root.fsl"]);
    assert_eq!(
        (run.exit, &run.packet),
        (2, &refused(&message("Broken", "Held", 5), 5)),
    );
}

#[test]
fn a_third_clause_names_the_first() {
    let root = stable(&[USE_HELD, USE_BROKEN, USE_MISSING]);
    let run = run("third", &root, &["check", "root.fsl"]);
    assert_eq!(
        (run.exit, &run.packet),
        (2, &refused(&message("Broken", "Held", 3), 3)),
    );
}

/// Every command that reads the clause, or its name, or the document around it.
/// `--property` skips the seam and on base reported the first clause's name.
#[test]
fn every_route_that_loads_the_document_refuses_it() {
    let root = stable(&[USE_HELD, USE_BROKEN]);
    let expected = message("Broken", "Held", 3);
    let routes: &[&[&str]] = &[
        &["check", "root.fsl"],
        &["verify", "root.fsl", "--depth", "2"],
        &[
            "verify",
            "root.fsl",
            "--depth",
            "2",
            "--engine",
            "induction",
        ],
        &["verify", "root.fsl", "--depth", "2", "--engine", "explicit"],
        &["verify", "root.fsl", "--depth", "2", "--property", "Typed"],
        &[
            "verify",
            "root.fsl",
            "--depth",
            "2",
            "--exclude-property",
            "Typed",
        ],
        &["kernel", "root.fsl"],
        &["refine", "root.fsl", "held.fsl", "map.fsl"],
        &["mutate", "root.fsl", "--depth", "2"],
        &["ledger", "root.fsl", "--depth", "2"],
        &["lint", "root.fsl"],
        &["fmt", "root.fsl", "--check"],
        &["migrate", "root.fsl", "--edition", "next"],
        &["scenarios", "root.fsl"],
        &["testgen", "root.fsl"],
        &["html", "root.fsl"],
        &["conformance", "root.fsl"],
        &["diff", "root.fsl", "held.fsl"],
        &["typestate", "root.fsl"],
        &["analyze", "root.fsl"],
        &["explain", "root.fsl"],
        &["document", "claims", "root.fsl"],
        &["document", "generate", "root.fsl"],
    ];
    for arguments in routes {
        let id = format!("route-{}", arguments.join("-").replace(['.', ' '], "_"));
        let run = run(&id, &root, arguments);
        assert_eq!(run.exit, 2, "{arguments:?}: {}", run.packet);
        assert_eq!(
            (&run.packet["kind"], &run.packet["message"]),
            (&json!("parse"), &json!(expected)),
            "{arguments:?}: {}",
            run.packet
        );
        // `lint` and `migrate` add the file to `loc`.
        assert_eq!(
            (&run.packet["loc"]["line"], &run.packet["loc"]["column"]),
            (&json!(3), &json!(3)),
            "{arguments:?}: {}",
            run.packet
        );
    }
}

/// `chain` reads a layer from a manifest and reports it inside its own packet.
#[test]
fn chain_refuses_the_layer() {
    let root = stable(&[USE_HELD, USE_BROKEN]);
    let mut files = FILES.to_vec();
    files.push(("root.fsl", &root));
    files.push(("fsl-project.toml", "[requirements]\nfile = \"root.fsl\"\n"));
    let run = run_in("chain", &files, &["chain"]);
    assert_ne!(run.exit, 0, "{}", run.packet);
    assert!(
        run.packet
            .to_string()
            .contains(&message("Broken", "Held", 3)),
        "{}",
        run.packet
    );
}

/// Reached through another document's clause: the abstraction is read with the
/// same parser, and `loc` stays on the clause that named it. Like any parse failure
/// of an abstraction, the message names neither that file nor the place in it.
#[test]
fn an_abstraction_that_repeats_the_clause_is_refused() {
    let middle = stable(&[USE_HELD, USE_BROKEN]);
    let root = stable(&["implements Impl from \"middle.fsl\" { }"]);
    let mut files = FILES.to_vec();
    files.push(("middle.fsl", &middle));
    files.push(("root.fsl", &root));
    let run = run_in("abstraction", &files, &["check", "root.fsl"]);
    assert_eq!(run.exit, 2, "{}", run.packet);
    assert_eq!(run.packet["kind"], "type");
    assert_eq!(
        run.packet["message"],
        "duplicate implements 'Broken' (a requirements document declares one, and 'Held' came first)"
    );
    assert_eq!(run.packet["loc"], json!({"line":2,"column":3}));
}

/// A literate document keeps the source lines, so both positions stay true.
#[test]
fn a_literate_document_points_at_the_clause() {
    let body = stable(&[USE_HELD, USE_BROKEN]);
    let root = format!("# Title\n\n```fsl\n{body}```\n");
    let mut files = FILES.to_vec();
    files.push(("root.md", &root));
    let run = run_in("literate", &files, &["check", "root.md"]);
    assert_eq!(run.exit, 2, "{}", run.packet);
    assert_eq!(run.packet["kind"], "parse");
    assert_eq!(run.packet["message"], json!(message("Broken", "Held", 6)));
    assert_eq!(run.packet["loc"], json!({"line":6,"column":3}));
}

/// Preservation control: one clause is still read and still decides, either way.
/// It keeps passing with the refusal removed.
#[test]
fn one_clause_keeps_deciding() {
    let held = run("one-held", &stable(&[USE_HELD]), &["check", "root.fsl"]);
    assert_eq!(held.exit, 0, "{}", held.packet);
    assert_eq!(
        held.packet["implements"],
        json!({"abs":"Held","result":"refines"}),
    );
    let broken = run("one-broken", &stable(&[USE_BROKEN]), &["check", "root.fsl"]);
    assert_eq!(broken.exit, 1, "{}", broken.packet);
    assert_eq!(broken.packet["result"], "refinement_failed");
    assert_eq!(broken.packet["implements"]["abs"], "Broken");
}

/// Preservation control: `implements` is a word the clause owns only at the start
/// of an item, and a document of another dialect is not touched.
#[test]
fn a_document_with_no_clause_is_unaffected() {
    let run = run(
        "none",
        "requirements Impl {\n  state { done: Bool }\n  init { done = false }\n  action finish() { done = true }\n  invariant Typed { true }\n}\n",
        &["check", "root.fsl"],
    );
    assert_eq!(run.exit, 0, "{}", run.packet);
    assert!(run.packet.get("implements").is_none(), "{}", run.packet);
}
