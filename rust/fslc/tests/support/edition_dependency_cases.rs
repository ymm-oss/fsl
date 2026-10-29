// SPDX-License-Identifier: Apache-2.0
//! Shared dependency-admission cases. Frontends supply transport, not policy.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use super::common; // Each adapter supplies one existing edition harness.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    N,
    P,
    M45,
    M54,
    W,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub edge: &'static str,
    pub endpoint: &'static str,
    pub variant: Variant,
}

// Endpoint incidence is deliberately separate from line numbers: the inventory
// discovers actual method calls and binds them to their containing function.
pub const EDGES: &[(&str, &str, &str)] = &[
    (
        "E01",
        "use",
        "fsl-core/src/compose.rs:lower_compose_with_options",
    ),
    (
        "E02",
        "implements",
        "fsl-core/src/refinement.rs:requirements_implements_with_bounds_with_options",
    ),
    (
        "E03",
        "delegates",
        "fsl-core/src/dialect.rs:governance_business_catalog",
    ),
    (
        "E04",
        "checked_by",
        "fsl-core/src/dialect.rs:validate_governance_preservation",
    ),
    (
        "E05",
        "before",
        "fsl-core/src/dialect.rs:validate_governance_document",
    ),
    (
        "E06",
        "after",
        "fsl-core/src/dialect.rs:validate_governance_document",
    ),
    ("E07", "uses", "fsl-tools/src/causal.rs:resolve_imports"),
    (
        "E08",
        "uses",
        "fsl-tools/src/causal_expectation.rs:compile_expectations",
    ),
    (
        "E09",
        "uses",
        "fslc/src/causal.rs:run_causal_observe_expectations",
    ),
    ("E10", "after", "fsl-wasm/src/lib.rs:governance_output:0"),
    ("E11", "before", "fsl-wasm/src/lib.rs:governance_output:1"),
    (
        "E12",
        "checked_by",
        "fsl-wasm/src/lib.rs:governance_output:2",
    ),
];

pub fn flat_rows() -> Vec<Row> {
    EDGES
        .iter()
        .flat_map(|&(id, edge, endpoint)| {
            [
                Variant::N,
                Variant::P,
                Variant::M45,
                Variant::M54,
                Variant::W,
            ]
            .into_iter()
            .filter(move |v| !(matches!(id, "E07" | "E08" | "E09") && *v == Variant::M54))
            .map(move |variant| Row {
                id: format!("{id}/{variant:?}"),
                edge,
                endpoint,
                variant,
            })
        })
        .collect()
}

// One definition per additional acceptance contract. Direct scope probes and
// isolated late endpoints are connected only after the implementation exists.
// A17 is the additional plain-Spec export control: A10's compose Kernel and
// typestate exports preserve their existing refusals, not fictitious successes.
pub const ADDITIONAL: &str = "\
W01|aliases|check|owner|one child warning
W02|path aliases|check|owner|canonical path dedup
W03|distinct findings|check|owner|two findings
W04|distinct children|check|owner|two canonical identities
W05|governance reread|check|nested owner|one finding
W06|causal reread|causal observe-expectations|owner|one finding
D01|implements compose|check/verify|owner|deep child refuses
D02|causal compose|causal check|owner|deep child refuses
D03|governance compose|check/verify|nested owner|both operand routes refuse
D04|requested next|check --edition next|owner|child default stays current
R01|repair lint|lint|repair|report without rejection
R02|repair migrate|migrate|repair|report without child edits
R03|repair format|fmt --check|repair|report without rewriting
O01|outside owner|kernel loader|fresh thread inactive|fail closed
O02|sequential refusal|owners|same thread|no leak
O03|sequential warnings|owners|same thread|each owner reports once
O04|nested owners|owners|same thread|one delivery
O05|child thread|loader|fresh thread inactive|fail closed
P01|reachable_counterfactuals_from_source|helper|owner|refusal beats swallowed error
P02|invariant_counterfactuals_from_source|helper|owner|refusal beats swallowed error
P03|document_model_failure|helper/document generate|owner|refusal identity and no artifact
P04|readable_implements_text_from_source|helper|owner|refusal beats Option
L01|publish_diagnostics|open rejected/disk canonical|explicit local|related child error and no model
L02|publish_diagnostics|open canonical/disk rejected|explicit local|open wins
L03|code_actions|open rejected/disk canonical|explicit local|no child edits; internal probe deferred
L04|code_actions|open canonical/disk rejected|explicit local|open wins; no child edits
L05|publish_diagnostics|mixed current child|explicit local|child UTF16 warning
L06|both Store routes|unsupported header|explicit local|child identity and no model
A01|testgen writer|testgen|owner|no artifact before admission
A02|html writer|html|owner|file and raw guarded
A03|ledger writer|ledger|nested owner|file raw overlay guarded
A04|counterexample writer|counterexample export|owner|violation exports only canonical
A05|approval unsigned|approval create|no outer scope; strict loader|no record before admission
A06|approval signed|approval create|no outer scope; strict loader|no signature or record
A07|causal evidence|causal observe-expectations|owner|single output guarded
A08|causal lifecycle|causal observe-expectations|owner|all suffixes guarded
A09|chain marker|chain jobs 1/2|fresh step owner; refine inactive|no implementation marker
A10|raw delivery|kernel/conformance/testplan/typestate/explain/analyze|owner|no successful raw output
A11|document generate|document generate|owner nonreader|no speculative dependency scan
A12|document claims|document claims|owner nonreader|no speculative dependency scan
A13|document check|document check|owner nonreader|root before artifact IO
A14|domain and SQL writers|domain/db import|owner nonreader|ordinary validation before output
A15|repair output|migrate --write/fmt|repair|only root edits
A16|verify cache|verify|owner Recording|refusal cold hit lemma
A17|plain kernel schema|kernel versions 1/2|owner nonreader|complete independent v1 golden and complete repeated v2 schema
T01|fslc/src/main.rs:main|check|fslc thread owner; main inactive|no model before refusal
T02|fslc/src/verification.rs:ranked_lasso_discharges|verify bmc/auto|owner; ranking inactive/read0|no solver after refusal
T03|sweep|sweep|outer owner; cells borrow; ranking inactive/read0|no multiplication
T04|fslc/src/main.rs:run_chain_step_isolated|chain jobs 1|fresh check owner|precise child refusal
T05|fslc/src/main.rs:run_chain_steps|chain jobs 2|pool inactive/read0; verify owner; ranking inactive/read0|parallel logs separate
T06|chain refine|chain jobs 1/2|step inactive strict loader|fail closed
T07|mutation baseline|mutate|outer owner; ranking inactive/read0|no mutants after refusal
T08|counterexample ranking|counterexample export|owner; ranking inactive/read0|no output after refusal
T09|html ranking|html|report owner; ranking inactive/read0|file/raw guarded
T10|ledger ranking|ledger|nested owner; ranking inactive/read0|one delivery
T11|database ranking|db check|specialized owner; ranking inactive/read0|canonical nonreader
T12|AI ranking|ai check|specialized owner; ranking inactive/read0|canonical nonreader
T13|domain ranking|domain check|specialized owner; ranking inactive/read0|canonical nonreader
T14|scenarios fallback|scenarios|owner; ranking inactive/read0|real fallback control
T15|testgen fallback|testgen|owner; ranking inactive/read0|violation stays exit1
T16|domain fallback|domain testgen|owner; ranking inactive/read0|canonical nonreader
T17|explain witness|explain|owner; ranking inactive/read0|swallowed status does not hide refusal
T18|approval rerender|approval create|initial inactive strict loader; renderer owner|all solver kinds guarded
T19|approval evaluation|approval check|direct inactive strict loader; renderer owner|live input only
T20|approval difference|approval diff|direct inactive strict loader; renderer owner|separate invocation logs
T21|fsl-lsp/src/server.rs:requirements_diagnostics|publish_diagnostics|local log; solver inactive/read0|no model means no solver
I01|inventory|source discovery|test only|all endpoints and incidences accounted
C01|Worker files|check/verify|request-local|unused file unread; option priority
C02|ordinary errors|all readers|unchanged|IO parse cycle suppression preserved
C03|direct duplicate slots|run_admitted_roots|owner|two root warnings remain
C04|materialization|literate kernel|owner|input materialization not user artifact";

pub fn additional_rows() -> Vec<Row> {
    ADDITIONAL
        .lines()
        .map(|line| {
            let p = line.split('|').collect::<Vec<_>>();
            assert_eq!(p.len(), 5);
            Row {
                id: p[0].into(),
                edge: "control",
                endpoint: p[1],
                variant: Variant::P,
            }
        })
        .collect()
}

pub fn all_rows() -> Vec<Row> {
    flat_rows().into_iter().chain(additional_rows()).collect()
}

pub fn assigned(adapter: &str) -> BTreeSet<String> {
    all_rows()
        .into_iter()
        .filter(|row| {
            let id = row.id.split('/').next().unwrap();
            match adapter {
                "worker" => worker_edge(id) || id == "C01",
                "lsp" => id.starts_with('L') || id == "T21",
                "private" => {
                    id.starts_with('O') || id.starts_with('P') || matches!(id, "C03" | "T06")
                }
                "cli" => matches!(id, "E01" | "E02" | "E03" | "E04" | "E05" | "E06"),
                "routes" => {
                    matches!(id, "E07" | "E08" | "E09" | "P03")
                        || (!id.starts_with('E')
                            && !id.starts_with('L')
                            && !id.starts_with('O')
                            && !id.starts_with('P')
                            && !matches!(id, "C01" | "C03" | "T21" | "I01"))
                }
                "inventory" => id == "I01",
                _ => panic!("unknown adapter {adapter}"),
            }
        })
        .map(|r| r.id)
        .collect()
}

fn worker_edge(id: &str) -> bool {
    matches!(
        id,
        "E01" | "E02" | "E03" | "E04" | "E05" | "E06" | "E10" | "E11" | "E12"
    )
}
pub fn worker_rows() -> Vec<Row> {
    flat_rows()
        .into_iter()
        .filter(|r| worker_edge(r.id.split('/').next().unwrap()))
        .collect()
}

pub fn deferred_rows() -> Vec<String> {
    all_rows()
        .into_iter()
        .filter(|row| {
            let id = row.id.split('/').next().unwrap();
            worker_edge(id) || matches!(id, "E08" | "E09" | "L03" | "O05") || id.starts_with('T')
        })
        .map(|r| r.id)
        .collect()
}

pub fn row_id(label: &str) -> String {
    if label.starts_with('E') {
        label.split('/').take(2).collect::<Vec<_>>().join("/")
    } else {
        label.split('/').next().unwrap().into()
    }
}

thread_local! {static CAPTURE:std::cell::RefCell<Option<Vec<(String,bool)>>> = const {std::cell::RefCell::new(None)};}
pub fn capture(run: impl FnOnce()) -> Vec<(String, bool)> {
    CAPTURE.with(|c| assert!(c.borrow_mut().replace(Vec::new()).is_none()));
    run();
    CAPTURE.with(|c| c.borrow_mut().take().unwrap())
}

pub fn assert_assignment(adapter: &str, observations: &[(String, bool)]) {
    let actual = observations
        .iter()
        .map(|(label, _)| row_id(label))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual,
        assigned(adapter),
        "unexecuted or unregistered acceptance row"
    );
}

pub fn exercise(adapter: &str, runners: &[fn()]) {
    let observations = capture(|| {
        for run in runners {
            if let Err(panic) = std::panic::catch_unwind(*run) {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("");
                assert!(
                    message.starts_with("dependency admission mismatches:"),
                    "fixture or runner failure, not acceptance red: {message}"
                );
            }
        }
    });
    assert_assignment(adapter, &observations);
    let failures = observations
        .iter()
        .filter(|(_, ok)| !*ok)
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "dependency admission mismatches: {failures:?}"
    );
}

pub const LEAF: &str = "spec S { state { done: Bool = false } action finish() { done = true } invariant Typed { true } }\n";
pub const LEGACY_LEAF: &str = "spec S { state { done: Bool = false } action finish() { done = true } invariant Typed \"REQ-DEP-001: 日本語😀\" { true } }\n";
pub const IMPLEMENTS: &str = "requirements R { implements S from \"dep.fsl\" { map done = done action finish() -> finish() } state { done: Bool = false } action finish() { done = true } invariant Typed { true } }\n";
pub const GOVERNANCE: &str = "governance G { control CTRL-1 \"Keep\" preservation Keep { before S from \"before.fsl\" after S from \"after.fsl\" preserve CTRL-1 checked_by refinement \"mapping.fsl\" } }\n";
pub const MAPPING: &str =
    "refinement M {\n impl S\n abs S\n map done = done\n action finish() -> finish()\n}\n";
pub const BUSINESS: &str = "business Flow {\n actor User\n entity Item\n control CTRL-1 \"Keep\"\n process Item {\n  stages Open, Done\n  initial Open\n  transition finish Open -> Done by User\n }\n policy P-1 \"Finish\" satisfies CTRL-1\n  invariant { forall i in 0..1 { true } }\n}\nverify { instances Item = 1 }\n";

pub fn legacy(body: &str) -> String {
    body.replace(
        "invariant Typed",
        "invariant Typed \"REQ-DEP-001: 日本語😀\"",
    )
}

pub fn annotated(body: &str) -> String {
    body.replace(
        "invariant Typed",
        "@requirement(\"REQ-DEP-001\", \"日本語😀\") invariant Typed",
    )
}

pub fn next(body: &str) -> String {
    format!("fsl 5\n{body}")
}
pub fn next_leaf() -> String {
    next(LEGACY_LEAF)
}
pub fn causal_legacy() -> String {
    include_str!("../../../../examples/causal/incident_system.fsl").replace(
        "invariant PrecisionBounded",
        "invariant PrecisionBounded \"REQ-DEP-001: 日本語😀\"",
    )
}
pub fn row(id: &str) -> Row {
    flat_rows().into_iter().find(|r| r.id == id).unwrap()
}
pub fn identity(s: &common::Scratch, name: &str) -> String {
    s.0.join(name)
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .into()
}
pub fn fsl_envelope() -> serde_json::Map<String, Value> {
    serde_json::from_value(json!({"fsl":"1.0"})).unwrap()
}
pub fn cli_refusal(source: &str, path: &str) -> Value {
    refusal(source, path, fsl_envelope())
}
pub fn position(source: &str, offset: usize) -> (usize, usize, usize) {
    let prefix = &source[..offset];
    let last = prefix.rsplit('\n').next().unwrap();
    (
        prefix.bytes().filter(|b| *b == b'\n').count(),
        last.chars().count(),
        last.encode_utf16().count(),
    )
}
pub fn with_child_warnings(mut expected: Value, findings: Vec<Value>) -> Value {
    expected
        .as_object_mut()
        .unwrap()
        .entry("warnings")
        .or_insert(json!([]))
        .as_array_mut()
        .unwrap()
        .extend(findings);
    expected
}

pub struct Fixture {
    pub root: String,
    pub child: String,
    pub files: BTreeMap<String, String>,
}

impl Fixture {
    pub fn materialize(&self, scratch: &common::Scratch, root: &str) {
        for (name, source) in &self.files {
            scratch.write(name, source);
        }
        scratch.write(root, &self.root);
    }
}

pub fn twin_expected(
    row: &Row,
    fixture: &Fixture,
    scratch: &common::Scratch,
    run: impl FnOnce() -> Value,
) -> Value {
    let mut twin = row.clone();
    twin.variant = Variant::P;
    let twin = self::fixture(&twin);
    scratch.write(&fixture.child, &twin.files[&fixture.child]);
    let expected = run();
    scratch.write(&fixture.child, &fixture.files[&fixture.child]);
    if row.variant == Variant::W {
        with_child_warnings(
            expected,
            findings(
                &fixture.files[&fixture.child],
                &identity(scratch, &fixture.child),
                "current",
            ),
        )
    } else {
        expected
    }
}

pub fn fixture(row: &Row) -> Fixture {
    let id = row.id.split('/').next().unwrap();
    let mut files = BTreeMap::from([
        ("before.fsl".into(), LEAF.into()),
        ("after.fsl".into(), LEAF.into()),
        ("mapping.fsl".into(), MAPPING.into()),
    ]);
    let (mut root, child, body, legacy) = match id {
        "E01" => {
            let legacy = include_str!("../../../../examples/named_predicate.fsl").to_owned();
            let canonical = legacy.replace("  invariant OnlyEligibleAutoApproved \"REQ-2: only eligible claims are auto-approved\" {", include_str!("../../../fsl-wasm/tests/support/dependency_canonical.txt").trim_end());
            (include_str!("../../../fsl-wasm/tests/support/dependency_root.txt").into(), "dep_next_colon.fsl", canonical, legacy)
        }
        "E02" => (IMPLEMENTS.into(), "dep.fsl", LEAF.into(), LEGACY_LEAF.into()),
        "E03" => (
            "governance G { control CTRL-1 \"Keep\" delegates Flow from \"dep.fsl\" { require CTRL-1 } }\n".into(),
            "dep.fsl", BUSINESS.into(), BUSINESS.replace("forall i in 0..1 { true }", "forall i in 0..1: true")),
        "E04" | "E12" => (GOVERNANCE.into(), "mapping.fsl", MAPPING.replace("map done = done", "map done = forall i in 0..1 { done }"), MAPPING.replace("map done = done", "map done = forall i in 0..1: done")),
        "E05" | "E11" => (GOVERNANCE.into(), "before.fsl", LEAF.into(), LEGACY_LEAF.into()),
        "E06" | "E10" => (GOVERNANCE.into(), "after.fsl", LEAF.into(), LEGACY_LEAF.into()),
        "E07" | "E08" | "E09" => (
            include_str!("../../../../examples/causal/incident_response.fsl").into(),
            "incident_system.fsl",
            include_str!("../../../../examples/causal/incident_system.fsl").into(),
            causal_legacy()),
        _ => panic!("unknown edge row {id}"),
    };
    let child_source = match row.variant {
        Variant::N => format!("fsl 5\n{legacy}"),
        Variant::W => format!("fsl 4\n{legacy}"),
        Variant::M45 => format!("fsl 5\n{body}"),
        Variant::P | Variant::M54 => format!("fsl 4\n{body}"),
    };
    if row.variant == Variant::M54 {
        root = format!("fsl 5\n{root}");
    }
    files.insert(child.into(), child_source);
    Fixture {
        root,
        child: child.into(),
        files,
    }
}

// Independent migration oracle: these are test literals and source spans, not
// findings obtained by calling the production planner or admission classifier.
pub fn metadata_finding(source: &str, file: &str, edition: &str) -> Value {
    let (start, end) = metadata_span(source);
    metadata_finding_at(source, file, edition, start, end)
}

fn metadata_finding_at(source: &str, file: &str, edition: &str, start: usize, end: usize) -> Value {
    let literal = &source[start..end];
    let text = &literal[1..literal.len() - 1];
    let (id, description) = text.split_once(": ").unwrap();
    let replacement = format!("@requirement({id:?}, {description:?})");
    let mut declaration = [
        "action ",
        "invariant ",
        "trans ",
        "reachable ",
        "leadsTo ",
        "unless ",
    ]
    .iter()
    .filter_map(|word| source[..start].rfind(*word))
    .max()
    .expect("fixture metadata declaration");
    if source[declaration..].starts_with("action ") && source[..declaration].ends_with("fair ") {
        declaration -= 5;
    }
    let point = |offset| {
        let (line, column, _) = position(source, offset);
        (line + 1, column + 1)
    };
    let (line, column) = point(start);
    let (end_line, end_column) = point(end);
    let (_, declaration_column) = point(declaration);
    let insertion = format!("{replacement}\n{}", " ".repeat(declaration_column - 1));
    json!({"code":"legacy_string_metadata","kind":"legacy_string_metadata","taxonomy":"deprecated","severity":if edition == "next" {"error"} else {"warning"},
        "edition":edition,"message":"legacy string metadata is deprecated; use a typed annotation",
        "loc":{"file":file,"line":line,"column":column,"end_line":end_line,"end_column":end_column},
        "canonical_replacement":replacement,"machine_applicable":true,
        "edits":[{"start":declaration,"end":declaration,"replacement":insertion},{"start":start,"end":end,"replacement":""}]})
}

pub fn metadata_span(source: &str) -> (usize, usize) {
    *metadata_spans(source)
        .first()
        .expect("metadata literal attached to a declaration")
}

fn metadata_spans(source: &str) -> Vec<(usize, usize)> {
    source
        .match_indices("\"REQ-")
        .filter_map(|(start, _)| {
            let end = start + source[start + 1..].find('"')? + 2;
            source[end..]
                .trim_start()
                .starts_with('{')
                .then_some((start, end))
        })
        .collect()
}

pub fn findings(source: &str, file: &str, edition: &str) -> Vec<Value> {
    if source.contains("\"REQ-") {
        return metadata_spans(source)
            .into_iter()
            .map(|(start, end)| metadata_finding_at(source, file, edition, start, end))
            .collect();
    }
    let start = source.find("forall ").expect("colon quantifier");
    let colon = source[start..].find(':').unwrap() + start;
    let end = source[colon..]
        .find('\n')
        .map_or(source.len(), |i| colon + i);
    let end = source[..end]
        .trim_end()
        .trim_end_matches('}')
        .trim_end()
        .len();
    let (line, column, _) = position(source, start);
    let (line, column) = (line + 1, column + 1);
    vec![
        json!({"code":"legacy_quantifier_colon","kind":"legacy_quantifier_colon","taxonomy":"deprecated","severity":if edition == "next" {"error"} else {"warning"},"edition":edition,
        "message":"legacy colon quantifier syntax is deprecated","loc":{"file":file,"line":line,"column":column,"end_line":line,"end_column":column+6},
        "canonical_replacement":"braced quantifier","machine_applicable":true,
        "edits":[{"start":colon,"end":colon+1,"replacement":" {"},{"start":end,"end":end,"replacement":" }"}]}),
    ]
}

pub fn refusal(source: &str, path: &str, envelope: serde_json::Map<String, Value>) -> Value {
    let mut expected = Value::Object(envelope);
    for (key, value) in json!({"result":"error","kind":"unsupported_in_edition","edition":"next","fsl_edition":5,"findings":findings(source, path, "next")}).as_object().unwrap() {
        expected[key] = value.clone();
    }
    expected
}

pub fn expected_check(
    row: &Row,
    fixture: &Fixture,
    child_identity: &str,
    envelope: serde_json::Map<String, Value>,
) -> Value {
    if row.variant == Variant::N {
        return refusal(&fixture.files[&fixture.child], child_identity, envelope);
    }
    let mut result = Value::Object(envelope);
    result["result"] = json!("ok");
    result["spec"] = json!(match row.edge {
        "use" => "UsesNextDep",
        "implements" => "R",
        _ => "G",
    });
    result["warnings"] = if row.variant == Variant::W {
        json!(findings(
            &fixture.files[&fixture.child],
            child_identity,
            "current"
        ))
    } else {
        json!([])
    };
    if row.edge == "implements" {
        result["implements"] = json!({"abs":"S","result":"refines"});
    }
    if !matches!(row.edge, "use" | "implements") {
        result["governance"] = if row.edge == "delegates" {
            json!({"name":"G","controls":["CTRL-1"],"delegates":[{"business":"Flow","required":["CTRL-1"],"satisfied":{"CTRL-1":[{"kind":"policy","id":"P-1"}]}}],"preservations":[]})
        } else {
            json!({"name":"G","controls":["CTRL-1"],"delegates":[],"preservations":[{"name":"Keep","before":"S","after":"S","preserve":["CTRL-1"],"result":"refines"}]})
        };
    }
    if row.variant == Variant::M54 {
        result["edition"] = json!("next");
        result["fsl_edition"] = json!(5);
    }
    result
}

// Observed native BMC costs include wall-clock durations and process solver
// memory. Validate every excluded value and preserve their live JSON paths;
// no verdict, location, assurance, statistic count or warning is discarded.
pub fn stable(mut value: Value) -> (Value, BTreeSet<String>) {
    fn metric(value: &Value, path: String, nullable: bool, paths: &mut BTreeSet<String>) {
        assert!(
            (nullable && value.is_null())
                || value.as_f64().is_some_and(|v| v.is_finite() && v >= 0.0),
            "invalid ambient cost {path}"
        );
        paths.insert(path);
    }
    fn walk(value: &mut Value, path: &str, paths: &mut BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if (object.get("fsl").and_then(Value::as_str) == Some("1.0")
                    || path == "/verification/engine_metadata")
                    && object.contains_key("cost")
                {
                    let cost = &object["cost"];
                    for pointer in ["/elapsed_s", "/solver/check_elapsed_s", "/solver/memory_mb"] {
                        let value = cost.pointer(pointer).expect("missing excluded cost field");
                        metric(
                            value,
                            format!("{path}/cost{pointer}"),
                            pointer.ends_with("memory_mb"),
                            paths,
                        );
                    }
                    let properties = cost["properties"]
                        .as_array()
                        .expect("cost property timing array");
                    if cost["solver"]["checks"].as_u64().unwrap() > 0 {
                        assert!(
                            !properties.is_empty(),
                            "measured solver run needs property timing evidence"
                        );
                    }
                    for (i, property) in properties.iter().enumerate() {
                        metric(
                            property
                                .get("elapsed_s")
                                .expect("missing excluded property elapsed_s"),
                            format!("{path}/cost/properties/{i}/elapsed_s"),
                            false,
                            paths,
                        );
                    }
                }
                for (key, value) in object {
                    let path = format!("{path}/{key}");
                    if paths.contains(&path) {
                        *value = Value::Null;
                    } else {
                        walk(value, &path, paths);
                    }
                }
            }
            Value::Array(array) => {
                for (i, value) in array.iter_mut().enumerate() {
                    walk(value, &format!("{path}/{i}"), paths);
                }
            }
            _ => {}
        }
    }
    let mut paths = BTreeSet::new();
    walk(&mut value, "", &mut paths);
    (value, paths)
}

pub fn native_value(value: Value) -> Value {
    if value.get("$schema").is_some() {
        value
    } else {
        common::without_versions(value)
    }
}

pub fn versioned_native_value(value: Value) -> Value {
    assert!(
        value.get("versions").is_some(),
        "versioned check/verify envelope must carry its live runtime identity"
    );
    native_value(value)
}

#[derive(Default)]
pub struct Observations(common::Results);
impl Observations {
    pub fn eq<T: PartialEq + std::fmt::Debug>(&mut self, id: &str, actual: T, expected: T) {
        let equal = actual == expected;
        self.compare(id, actual, expected, equal);
    }
    pub fn compare(
        &mut self,
        id: &str,
        actual: impl std::fmt::Debug,
        expected: impl std::fmt::Debug,
        equal: bool,
    ) {
        CAPTURE.with(|c| {
            if let Some(c) = c.borrow_mut().as_mut() {
                c.push((id.into(), equal));
            }
        });
        self.0
            .check(&format!("DEPENDENCY {id}"), actual, expected, equal);
    }
    pub fn finish(self) {
        assert!(
            self.0.failures.is_empty(),
            "dependency admission mismatches: {:?}",
            self.0.failures
        );
    }
}
