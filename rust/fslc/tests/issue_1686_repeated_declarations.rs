// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1686: a `verify` bound, a `kpi` or a `concept` written twice lost a copy
//! without a word, so reordering the copies changed the result. Each is now
//! refused at the repeated declaration, on every route that loads the document.
//! A document that repeats nothing keeps its result.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const INSTANCES: &str = "  instances E = 1\n";
const WIDE: &str = "  values V = 0..3\n";
const NARROW: &str = "  values V = 0..1\n";

/// A document whose invariant holds only under the narrow bound.
fn bounded(bounds: &str) -> String {
    format!(
        "requirements R {{\n  number V\n  process E with v: V = 0 {{\n    stages A, B\n    \
         initial A\n    transition go A -> B by U with a: V set v = a covers REQ-1 \"t\"\n  }}\n  \
         kpi done = count E in B\n  invariant I {{ forall c: E {{ e_v[c] <= 1 }} }}\n}}\n\
         verify {{\n{bounds}}}\n"
    )
}

const FIXTURE_DIR: &str = "tests/fixtures/fsl5/e2e";

fn fixture(file: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(FIXTURE_DIR)
            .join(file),
    )
    .expect("read fixture")
}

/// `source` with `old`, which occurs exactly once, replaced by `new`.
fn edit(source: &str, old: &str, new: &str) -> String {
    assert_eq!(source.matches(old).count(), 1, "{old:?}");
    source.replacen(old, new, 1)
}

/// A scratch directory holding a copy of the e2e fixtures.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("issue_1686")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR);
        for entry in std::fs::read_dir(fixtures).expect("read fixtures") {
            let path = entry.expect("fixture entry").path();
            std::fs::copy(&path, directory.join(path.file_name().expect("file name")))
                .expect("copy fixture");
        }
        Self(directory)
    }

    fn write(&self, name: &str, source: &str) {
        std::fs::write(self.0.join(name), source).expect("write source");
    }

    /// The exit and, when stdout is JSON, the packet of `fslc <args>`.
    fn fslc(&self, args: &[&str]) -> (i32, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env("FSLC_CACHE", "off")
            .env("FSLC_UNSTABLE", "fsl5")
            .output()
            .expect("run fslc");
        (
            output.status.code().expect("exit code"),
            serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
        )
    }

    fn check(&self, source: &str) -> (i32, Value) {
        self.write("doc.fsl", source);
        self.fslc(&["check", "doc.fsl"])
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn summary((exit, packet): &(i32, Value)) -> Value {
    json!({
        "exit": exit,
        "result": packet["result"],
        "kind": packet["kind"],
        "message": packet["message"],
        "loc": packet["loc"],
        "diagnostic_code": packet["diagnostic_code"],
    })
}

fn refusal(what: &str, name: &str, line: u32) -> Value {
    json!({
        "exit": 2,
        "result": "error",
        "kind": "semantics",
        "message": format!("duplicate {what} '{name}' at doc.fsl:{line}:3"),
        "loc": {"line": line, "column": 3},
        "diagnostic_code": null,
    })
}

fn assert_ok(run: &(i32, Value), label: &str) {
    assert_eq!(
        (run.0, &run.1["result"]),
        (0, &json!("ok")),
        "{label}: {:#}",
        run.1
    );
}

/// Each engine: the bound alone decides the verdict, so the copy that was
/// dropped decided it too. Either order is now refused.
#[test]
fn a_repeated_bound_is_refused_and_a_single_bound_keeps_its_verdict() {
    let scratch = Scratch::new("verdicts");
    scratch.write("wide.fsl", &bounded(&format!("{INSTANCES}{WIDE}")));
    scratch.write("narrow.fsl", &bounded(&format!("{INSTANCES}{NARROW}")));
    scratch.write("doc.fsl", &bounded(&format!("{INSTANCES}{WIDE}{NARROW}")));
    for (engine, narrow_result) in [
        ("bmc", "verified"),
        ("explicit", "proved"),
        ("induction", "proved"),
    ] {
        let run = |file: &str| scratch.fslc(&["verify", file, "--engine", engine, "--depth", "3"]);
        let (exit, packet) = run("wide.fsl");
        assert_eq!(
            (exit, &packet["result"]),
            (1, &json!("violated")),
            "{engine}: {packet:#}"
        );
        let (exit, packet) = run("narrow.fsl");
        assert_eq!(
            (exit, &packet["result"]),
            (0, &json!(narrow_result)),
            "{engine}: {packet:#}"
        );
        assert_eq!(
            summary(&run("doc.fsl")),
            refusal("values bound for", "V", 14),
            "{engine}"
        );
    }
}

#[test]
fn a_repeated_bound_is_refused_whatever_its_value_and_order() {
    let scratch = Scratch::new("bounds");
    let instances_one = "  instances E = 2\n";
    for (label, bounds, what, name, line) in [
        (
            "values-wide-narrow",
            format!("{INSTANCES}{WIDE}{NARROW}"),
            "values bound for",
            "V",
            14,
        ),
        (
            "values-narrow-wide",
            format!("{INSTANCES}{NARROW}{WIDE}"),
            "values bound for",
            "V",
            14,
        ),
        (
            "values-equal",
            format!("{INSTANCES}{WIDE}{WIDE}"),
            "values bound for",
            "V",
            14,
        ),
        (
            "instances-1-2",
            format!("{INSTANCES}{instances_one}{WIDE}"),
            "instances bound for",
            "E",
            13,
        ),
        (
            "instances-2-1",
            format!("{instances_one}{INSTANCES}{WIDE}"),
            "instances bound for",
            "E",
            13,
        ),
        (
            "instances-apart",
            format!("{INSTANCES}{WIDE}{INSTANCES}"),
            "instances bound for",
            "E",
            14,
        ),
    ] {
        assert_eq!(
            summary(&scratch.check(&bounded(&bounds))),
            refusal(what, name, line),
            "{label}"
        );
    }
}

/// A KPI has no verdict, but `analyze` keeps one node per name: the last copy.
#[test]
fn a_repeated_kpi_is_refused_in_requirements_and_business() {
    let scratch = Scratch::new("kpis");
    let requirements = bounded(&format!("{INSTANCES}{WIDE}"));
    let kpi = "  kpi done = count E in B\n";
    for (label, copy) in [
        ("kpi-same", kpi),
        ("kpi-other-stage", "  kpi done = count E in A\n"),
    ] {
        let source = edit(&requirements, kpi, &format!("{kpi}{copy}"));
        assert_eq!(
            summary(&scratch.check(&source)),
            refusal("kpi", "done", 9),
            "{label}"
        );
    }
    let distinct = edit(
        &requirements,
        kpi,
        &format!("{kpi}  kpi started = count E in A\n"),
    );
    assert_ok(&scratch.check(&distinct), "kpi-distinct");

    let business = fixture("1_business.fsl");
    let kpi = "  kpi paid_claims = count Claim in Paid\n";
    assert_ok(&scratch.check(&business), "business");
    let line = u32::try_from(
        business
            .lines()
            .position(|line| line == kpi.trim_end())
            .expect("the kpi")
            + 2,
    )
    .expect("line");
    let source = edit(
        &business,
        kpi,
        &format!("{kpi}  kpi paid_claims = count Claim in Draft\n"),
    );
    assert_eq!(
        summary(&scratch.check(&source)),
        refusal("kpi", "paid_claims", line)
    );

    let next = fixture("fsl5_en_decide.fsl");
    let source = edit(&next, kpi, &format!("{kpi}{kpi}"));
    assert_eq!(
        summary(&scratch.check(&source)),
        refusal("kpi", "paid_claims", 41)
    );
}

#[test]
fn a_repeated_business_bound_is_refused() {
    let scratch = Scratch::new("business-bounds");
    let business = fixture("1_business.fsl");
    let bound = "  instances Claim = 3\n";
    assert_eq!(business.matches(bound).count(), 1);
    let line = u32::try_from(
        business
            .lines()
            .position(|line| line == bound.trim_end())
            .expect("the bound")
            + 2,
    )
    .expect("line");
    for (label, copy) in [("same", bound), ("other", "  instances Claim = 1\n")] {
        let source = edit(&business, bound, &format!("{bound}{copy}"));
        assert_eq!(
            summary(&scratch.check(&source)),
            refusal("instances bound for", "Claim", line),
            "{label}"
        );
    }
    // A business document does not read `values`, so one bound is accepted.
    let bound_of = |copies: &str| edit(&business, bound, &format!("{bound}{copies}"));
    let values = "  values Nope = 0..1\n";
    assert_ok(&scratch.check(&bound_of(values)), "one values bound");
    assert_eq!(
        summary(&scratch.check(&bound_of(&format!("{values}  values Nope = 0..2\n")))),
        refusal("values bound for", "Nope", line + 1),
    );
}

/// A `spec` with an `entity` or a `number` refuses a repeated bound while it reads
/// them. A `spec` with neither does not read its bounds, and that stays as it was.
#[test]
fn a_spec_refuses_a_repeated_bound_only_where_it_reads_bounds() {
    let scratch = Scratch::new("spec-bounds");
    let (exit, packet) = scratch.check(
        "spec S {\n  entity E\n  state { n: Bool }\n  init { n = false }\n  \
         action go() { n = n }\n}\nverify {\n  instances E = 1\n  instances E = 2\n}\n",
    );
    assert_eq!(
        (exit, &packet["kind"], &packet["message"]),
        (
            2,
            &json!("semantics"),
            &json!("duplicate instances bound for 'E' at 9:3")
        ),
        "{packet:#}"
    );
    assert_ok(
        &scratch.check(
            "spec S {\n  type V = 0..3\n  state { n: V }\n  init { n = 0 }\n  \
             action go() { n = n }\n}\nverify {\n  instances V = 1\n  instances V = 2\n}\n",
        ),
        "a spec with neither",
    );
}

/// The issue's row: the first copy was kept, so swapping the two moved `check`
/// between `ok` and a decision-table error.
#[test]
fn a_repeated_concept_is_refused() {
    let scratch = Scratch::new("concepts");
    let source = fixture("fsl5_en_decide.fsl");
    let concept = "  concept SmallClaim = Claim where amount <= AUTO_LIMIT\n";
    let flipped = "  concept SmallClaim = Claim where amount > AUTO_LIMIT\n";
    assert_ok(&scratch.check(&source), "one copy");
    let (exit, packet) = scratch.check(&edit(&source, concept, flipped));
    assert_eq!(
        (exit, &packet["kind"]),
        (2, &json!("semantics")),
        "{packet:#}"
    );
    assert!(
        packet["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("decision table 'REQ-2-3'")),
        "{packet:#}"
    );
    for (label, copies) in [
        ("kept-first", format!("{concept}{flipped}")),
        ("flipped-first", format!("{flipped}{concept}")),
        ("equal", format!("{concept}{concept}")),
    ] {
        assert_eq!(
            summary(&scratch.check(&edit(&source, concept, &copies))),
            refusal("concept", "SmallClaim", 24),
            "{label}"
        );
    }
    let distinct = edit(
        &source,
        concept,
        &format!("{concept}  concept Tiny = Claim where amount == 0\n"),
    );
    assert_ok(&scratch.check(&distinct), "distinct concept");
}

/// Two bounds of different names, bounds in another order, or one name on two
/// kinds of declaration repeat nothing.
#[test]
fn different_names_are_not_repeats() {
    let scratch = Scratch::new("controls");
    let two_kinds = "requirements R {\n  number V\n  number W\n  process E with v: V = 0, w: W = 0 {\n    \
        stages A, B\n    initial A\n    transition go A -> B by U\n  }\n}\n\
        verify {\n  instances E = 1\n  values V = 0..1\n  values W = 0..1\n}\n";
    assert_ok(&scratch.check(two_kinds), "two values bounds");
    assert_ok(
        &scratch.check(&bounded(&format!("{WIDE}{INSTANCES}"))),
        "other order",
    );
    let kpi = "  kpi paid_claims = count Claim in Paid\n";
    let shared = edit(
        &fixture("fsl5_en_decide.fsl"),
        kpi,
        &format!("{kpi}  concept paid_claims = Claim where amount == 0\n"),
    );
    assert_ok(&scratch.check(&shared), "a concept and a kpi of one name");
}

/// Every route that loads the document, with the bound repeated and not.
#[test]
fn every_route_that_loads_the_document_refuses() {
    let scratch = Scratch::new("routes");
    scratch.write("doc.fsl", &bounded(&format!("{INSTANCES}{WIDE}{NARROW}")));
    scratch.write("one.fsl", &bounded(&format!("{INSTANCES}{NARROW}")));
    scratch.write(
        "map.fsl",
        "refinement M {\n  impl R\n  abs R\n  map e_stage[c: E] = e_stage[c]\n  \
         map e_v[c: E] = e_v[c]\n  action go(c, a) -> go(c, a)\n}\n",
    );
    scratch.write("trace.json", "{\"events\":[]}");
    let located: &[&[&str]] = &[
        &["check", "F"],
        &["verify", "F", "--depth", "3"],
        &["verify", "F", "--engine", "explicit", "--depth", "3"],
        &["verify", "F", "--engine", "induction"],
        &["explain", "F"],
        &["analyze", "F"],
        &["conformance", "F"],
        &["scenarios", "F", "--depth", "2"],
        &["testgen", "F", "--depth", "2"],
        &["mutate", "F", "--depth", "2"],
        &["html", "F"],
        &["ledger", "F"],
        &["diff", "F", "one.fsl"],
        &["diff", "one.fsl", "F"],
        &["replay", "F", "--trace", "trace.json"],
    ];
    for route in located {
        let with = |file: &'static str| {
            route
                .iter()
                .map(|argument| if *argument == "F" { file } else { argument })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            summary(&scratch.fslc(&with("doc.fsl"))),
            refusal("values bound for", "V", 14),
            "{route:?}"
        );
        assert_eq!(scratch.fslc(&with("one.fsl")).0, 0, "{route:?}");
    }
    // These name the file differently, or not at all, or omit `loc`.
    for route in [
        // An override of the bound does not make the repeat go away.
        &["verify", "doc.fsl", "--depth", "3", "--values", "V=0..1"][..],
        &["kernel", "doc.fsl", "--kernel-version", "1"],
        &["kernel", "doc.fsl", "--kernel-version", "2"],
        &["typestate", "doc.fsl"],
        &["lint", "doc.fsl"],
        &["fmt", "doc.fsl"],
        &["fmt", "--check", "doc.fsl"],
        &["refine", "doc.fsl", "one.fsl", "map.fsl"],
        &["refine", "one.fsl", "doc.fsl", "map.fsl"],
        &["document", "generate", "doc.fsl"],
        &["document", "claims", "doc.fsl"],
        &["migrate", "doc.fsl"],
        &["sweep", "doc.fsl", "--depth", "0..1"],
    ] {
        let (exit, packet) = scratch.fslc(route);
        assert_eq!(
            (exit, &packet["kind"]),
            (2, &json!("semantics")),
            "{route:?}: {packet:#}"
        );
        assert!(
            packet["message"]
                .as_str()
                .is_some_and(|message| message.starts_with("duplicate values bound for 'V' at")),
            "{route:?}: {packet:#}"
        );
    }
    let (exit, packet) = scratch.fslc(&["refine", "one.fsl", "one.fsl", "map.fsl"]);
    assert_eq!(
        (exit, &packet["result"]),
        (0, &json!("refines")),
        "{packet:#}"
    );
}

/// An inline `implements` keeps `loc` on itself, so its message names the place.
#[test]
fn an_abstraction_written_twice_is_named_from_the_implements() {
    let scratch = Scratch::new("implements");
    let source = fixture("fsl5_en_decide.fsl");
    let business = fixture("1_business.fsl");
    assert_ok(&scratch.check(&source), "one copy");
    let kpi = "  kpi paid_claims = count Claim in Paid\n";
    let line = 1
        + business
            .lines()
            .position(|line| line == kpi.trim_end())
            .expect("the kpi")
        + 1;
    scratch.write(
        "1_business.fsl",
        &edit(&business, kpi, &format!("{kpi}{kpi}")),
    );
    let (exit, packet) = scratch.fslc(&["check", "doc.fsl"]);
    assert_eq!(
        (exit, &packet["kind"], &packet["message"], &packet["loc"]),
        (
            2,
            &json!("type"),
            &json!(format!(
                "implements \"1_business.fsl\" failed to check \
                 (duplicate kpi 'paid_claims' at 1_business.fsl:{line}:3)"
            )),
            &json!({"line": 8, "column": 3})
        ),
        "{packet:#}"
    );
}
