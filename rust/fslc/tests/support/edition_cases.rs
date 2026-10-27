// SPDX-License-Identifier: Apache-2.0
//! One test-only population for native, Worker, and LSP adapters (#1449).
#![allow(dead_code)] // Each adapter uses only its assigned portion.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

// leaf | mode | remaining consumer contract. These cells still execute.
pub const DEFERRED_TO_SECOND_PR: &[(&str, &str, &str)] = &[
    (
        "ai check",
        "all",
        "Admit the root and own warnings before AI verification.",
    ),
    (
        "ai compat",
        "all",
        "Admit and reuse the AI root before surface parsing.",
    ),
    (
        "ai drift",
        "all",
        "Admit project roots before AI evaluation.",
    ),
    (
        "ai eval",
        "all",
        "Admit project roots before AI evaluation.",
    ),
    (
        "ai regress",
        "all",
        "Admit project roots before AI evaluation.",
    ),
    ("ai replay", "all", "Admit and reuse the AI replay root."),
    ("analyze", "batch", "Admit every explicit batch root."),
    (
        "analyze",
        "project",
        "Admit every manifest root and mapping before projection.",
    ),
    (
        "approval check",
        "all",
        "Admit the live root before consuming records or trust.",
    ),
    (
        "approval create",
        "all",
        "Admit the root before artifact and record operations.",
    ),
    (
        "approval diff",
        "all",
        "Admit the live root before baseline operations.",
    ),
    (
        "chain",
        "admission",
        "Prepare all roots before layer work and implementation effects.",
    ),
    (
        "counterexample export",
        "ordering",
        "Reject the root before preflight, model and digest work.",
    ),
    (
        "db observe",
        "all",
        "Admit the root before the surface parser.",
    ),
    (
        "diff",
        "all",
        "Admit both live roots and mapping with per-slot warning ownership.",
    ),
    (
        "document check",
        "all",
        "Admit the root before claims and artifacts.",
    ),
    (
        "document claims",
        "all",
        "Admit the root and deliver raw warnings on stderr.",
    ),
    (
        "document generate",
        "all",
        "Admit the root before output and deliver raw warnings.",
    ),
    (
        "domain analyze",
        "all",
        "Admit the domain root before projection.",
    ),
    (
        "domain expand",
        "all",
        "Admit the root before projection and writes.",
    ),
    (
        "domain generate",
        "all",
        "Admit the root before scaffold generation.",
    ),
    (
        "domain replay",
        "all",
        "Admit the root before model and log consumption.",
    ),
    (
        "domain testgen",
        "all",
        "Own domain root admission and warnings outside generation.",
    ),
    ("html", "raw", "Deliver the raw-success warning on stderr."),
    (
        "ledger",
        "raw",
        "Deliver the raw-success warning on stderr.",
    ),
    (
        "mutate",
        "success-warning",
        "Forward the admitted legacy warning once on success.",
    ),
    (
        "refine",
        "all",
        "Admit all explicit root and mapping slots, including repeated paths.",
    ),
    (
        "replay",
        "from-log",
        "Admit the log-replay root and explicit mapping.",
    ),
    (
        "sweep",
        "capture",
        "Capture one root and reuse its bytes for every grid cell.",
    ),
];

pub const OUT_OF_SCOPE_1513: &[(&str, &str)] = &[
    (
        "git/old",
        "Historical Git revision root admission belongs to #1513.",
    ),
    (
        "git/new",
        "Historical Git revision root admission belongs to #1513.",
    ),
    (
        "approval/old-root",
        "Historical approval baseline root admission belongs to #1513.",
    ),
];

fn belongs(label: &str, leaf: &str) -> bool {
    let command = if let Some(dialect) = label.strip_prefix("preserved/dialect/") {
        dialect
            .split_once('/')
            .expect("dialect repeat and command")
            .1
    } else {
        [
            "preserved/argv/",
            "priority/output-parent/",
            "approval/current/",
            "preserved/",
            "route/",
            "slot/",
            "stderr/",
        ]
        .iter()
        .find_map(|prefix| label.strip_prefix(prefix))
        .unwrap_or(label)
    };
    command == leaf
        || command
            .strip_prefix(leaf)
            .is_some_and(|rest| rest.starts_with([' ', '/']))
}

pub fn deferred(label: &str) -> Option<(&'static str, &'static str)> {
    DEFERRED_TO_SECOND_PR.iter().find_map(|&(leaf, mode, _)| {
        let matches = match (leaf, mode) {
            (_, "all") => belongs(label, leaf),
            ("analyze", "batch") => belongs(label, "analyze impl.fsl abs.fsl"),
            ("analyze", "project") => {
                label.starts_with("project/") || label.starts_with("preserved/project/")
            }
            ("chain", "admission") => {
                belongs(label, leaf)
                    && !label.ends_with("/warning")
                    && (!label.starts_with("route/") || label.ends_with("\"fsl 5\\n\""))
            }
            ("html" | "ledger", "raw") => label.starts_with(&format!("stderr/{leaf} ")),
            ("replay", "from-log") => belongs(label, "replay impl.fsl --from-log"),
            ("counterexample export", "ordering") => {
                label.starts_with("control/counterexample export/")
            }
            ("mutate", "success-warning") => label.starts_with("control/mutate/"),
            ("sweep", "capture") => label.starts_with("control/sweep/"),
            _ => panic!("unregistered deferred mode {leaf}/{mode}"),
        };
        matches.then_some((leaf, mode))
    })
}

pub fn out_of_scope(label: &str) -> bool {
    OUT_OF_SCOPE_1513.iter().any(|(cell, _)| *cell == label)
}

thread_local! {
    static CAPTURE: std::cell::RefCell<Option<Vec<(String, bool)>>> = const { std::cell::RefCell::new(None) };
}

// The inventory reuses the actual adapters, not a second set of witnesses or
// stale filesystem reports. Capture is thread-local, so parallel tests cannot
// contribute evidence to another run.
pub fn capture(run: impl FnOnce()) -> Vec<(String, bool)> {
    CAPTURE.with(|cells| assert!(cells.borrow_mut().replace(Vec::new()).is_none()));
    run();
    CAPTURE.with(|cells| cells.borrow_mut().take().unwrap())
}

pub fn assert_deferred_evidence(cells: &[(String, bool)]) {
    for (label, agrees) in cells
        .iter()
        .filter(|(label, _)| label.starts_with("preserved/"))
    {
        assert!(agrees, "preserved control failed: {label}");
    }
    for &(leaf, mode, reason) in DEFERRED_TO_SECOND_PR {
        let group: Vec<_> = cells
            .iter()
            .filter(|(label, _)| {
                !label.starts_with("preserved/") && deferred(label) == Some((leaf, mode))
            })
            .collect();
        let red = group.iter().filter(|(_, agrees)| !agrees).count();
        for (label, agrees) in &group {
            if !agrees {
                println!("DEFERRED_RED {leaf}/{mode} cell={label}");
            }
        }
        println!(
            "DEFERRED {leaf}/{mode} executed={} red={red} reason={reason}",
            group.len()
        );
        assert!(!group.is_empty(), "unexecuted deferred mode {leaf}/{mode}");
        assert!(red > 0, "completed mode remains deferred: {leaf}/{mode}");
    }
}

pub fn invoke(binary: &str, args: &[String], cwd: &Path, gate: bool) -> std::process::Output {
    cli_command(binary, args, cwd, gate)
        .output()
        .expect("execute working-tree CLI")
}

fn cli_command(binary: &str, args: &[String], cwd: &Path, gate: bool) -> std::process::Command {
    let mut command = std::process::Command::new(binary);
    command
        .args(args)
        .current_dir(cwd)
        .env("FSLC_CACHE", "off")
        .env("TMPDIR", root().join(".codex/tmp"))
        .env_remove("FSLC_UNSTABLE");
    if gate {
        command.env("FSLC_UNSTABLE", "fsl5");
    }
    command
}

#[cfg(unix)]
pub fn invoke_input(
    binary: &str,
    args: &[String],
    cwd: &Path,
    source: &str,
) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = cli_command(binary, args, cwd, true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("execute CLI with single-use root stream");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .expect("supply root once then EOF");
    child
        .wait_with_output()
        .expect("collect stream-root result")
}

pub fn split(template: &str) -> Vec<String> {
    template.split_whitespace().map(str::to_owned).collect()
}

pub fn command(binary: &str, cwd: &Path, template: &str, gate: bool) -> std::process::Output {
    invoke(binary, &split(template), cwd, gate)
}

pub fn expand(template: &str, replacements: &[(&str, PathBuf)]) -> Vec<String> {
    split(template)
        .into_iter()
        .map(|mut word| {
            for (key, path) in replacements {
                word = word.replace(key, path.to_str().unwrap());
            }
            assert!(!word.contains('{'), "unknown placeholder {word}");
            word
        })
        .collect()
}

pub fn refusal(binary: &str, cwd: &Path, path: &Path) -> Value {
    let output = invoke(
        binary,
        &["check".into(), path.to_str().unwrap().into()],
        cwd,
        true,
    );
    assert_eq!(output.status.code(), Some(2));
    let output = without_versions(value(&output));
    assert!(matches!(
        output["kind"].as_str(),
        Some("unsupported_in_edition" | "deprecated_domain_enum_union")
    ));
    output
}

pub fn refusal_envelope(
    source: &str,
    path: &str,
    mut envelope: serde_json::Map<String, Value>,
) -> Value {
    let findings = findings(source, path, fsl_syntax::Edition::Fsl5);
    assert!(!findings.is_empty());
    envelope.extend(serde_json::from_value::<serde_json::Map<String, Value>>(json!({
        "result":"error", "kind":if findings.iter().all(|f| f["code"] == "deprecated_domain_enum_union") {"deprecated_domain_enum_union"} else {"unsupported_in_edition"},
        "edition":"next", "fsl_edition":5, "findings":findings
    })).unwrap());
    Value::Object(envelope)
}

pub fn value(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected JSON, {error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

pub fn without_versions(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut()
        && let Some(versions) = object.remove("versions")
    {
        let solver = versions["solver"]["version"].as_str().unwrap();
        assert!(solver.starts_with("Z3 "));
        assert_eq!(
            versions,
            json!({
                "verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},
                "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
                "solver":{"name":"z3","backend":"native-z3","version":solver}
            })
        );
    }
    value
}

// key | owner | role | argv | canonical CLI exit/result/kind/warnings/artifact.
// Non-CLI rows and version have their own adapter/cli-contract assertions.
const TABLE: &str = r#"ai check|native|reader|ai check {src} --depth 2|[2,"error","semantics",null,false]
ai compare|native|nonreader|ai compare --from {metrics} --to {metrics}|[0,"compared",null,null,false]
ai compat|native|reader|ai compat {src}|[2,"error","semantics",null,false]
ai drift|native|reader|ai drift {src} --logs {records}|[2,"error","parse",null,false]
ai eval|native|reader|ai eval {src} --records {records}|[2,"error","parse",null,false]
ai regress|native|reader|ai regress {src} --before-records {records} --after-records {records}|[2,"error","parse",null,false]
ai replay|native|reader|ai replay {src} --logs {records}|[2,"error","semantics",null,false]
analyze|native|reader|analyze {src}|[0,"analyzed",null,null,false]
approval check|native|reader|approval check {src} --record {record}|[2,"error","io",null,false]
approval create|native|reader|approval create {src} --kind ledger --artifact {artifact} --approver edition-test -o {out}|[2,"error","io",null,false]
approval diff|native|reader|approval diff {src} --record {record}|[2,"error","io",null,false]
causal analyze|native|reader|causal analyze {src} --projection causal_graph|[2,"error","parse",null,false]
causal check|native|reader|causal check {src}|[2,"error","parse",null,false]
causal diff|native|reader|causal diff {src} {src}|[2,"error","parse",null,false]
causal ledger|native|reader|causal ledger {src}|[2,"error","parse",null,false]
causal observe-expectations|native|reader|causal observe-expectations {src} --from-log {log} --mapping {mapping} --scope {scope} --period-start 2026-01-01 --period-end 2026-03-31 --out {out}|[2,"error","parse",null,false]
causal verify-expectations|native|reader|causal verify-expectations {src} --depth 2|[2,"error","parse",null,false]
chain|native|reader|chain {manifest}|[0,"verified",null,null,false]
check|native|reader|check {src}|[0,"ok",null,[],false]
compat check|native|reader|compat check {src}|[2,"error","semantics",null,false]
conformance|native|reader|conformance {src} --depth 2|[0,"conformance",null,null,false]
counterexample export|native|reader|counterexample export {src} --depth 2 -o {out}|[2,"error","semantics",null,false]
db check|native|reader|db check {src} --depth 2|[2,"error","semantics",null,false]
db import|native|nonreader|db import {sql} --source sql -o {out}|[0,"imported",null,[],true]
db observe|native|reader|db observe {src} --trace {trace}|[2,"error","semantics",null,false]
diff|native|reader|diff {src} {src} --depth 2|[0,"semantic_diff",null,null,false]
document check|native|reader|document check {src} {artifact}|[2,"error","document",null,false]
document claims|native|reader|document claims {src}|[2,"error","document",null,false]
document generate|native|reader|document generate {src} -o {out}|[2,"error","document",null,false]
domain analyze|native|reader|domain analyze {src}|[0,"analyzed",null,null,false]
domain check|native|reader|domain check {src} --depth 2|[0,"verified_under_assumptions",null,null,false]
domain expand|native|reader|domain expand {src} -o {out}|[0,"expanded",null,null,true]
domain generate|native|reader|domain generate {src} -o {out}|[0,"generated",null,null,true]
domain replay|native|reader|domain replay {src} --logs {domainlog}|[1,"nonconformant",null,null,false]
domain testgen|native|reader|domain testgen {src} --depth 2 -o {out}|[0,"generated","domain_testgen",[],true]
explain|native|reader|explain {src} --depth 2|[0,"explained",null,null,false]
fmt|native|repair|fmt {src} --check|[1,"format_check",null,null,false]
html|native|reader|html {src} --depth 2 --no-timing -o {out}|[0,"generated","html_report",null,true]
kernel|native|reader|kernel {src}|[0,"kernel",null,null,false]
ledger|native|reader|ledger {src} --depth 2 -o {out}|[0,"generated","audit_ledger",null,true]
lint|native|repair|lint {src} --edition next|[0,"lint",null,null,false]
migrate|native|repair|migrate {src} --edition next|[0,"migrated",null,null,false]
mutate|native|reader|mutate {src} --depth 2 --max-mutants 0|[0,"mutated",null,null,false]
refine|native|reader|refine {src} {src} {mapping} --depth 2|[2,"error","type",null,false]
replay|native|reader|replay {src} --trace {replaytrace}|[2,"error","io",null,false]
scenarios|native|reader|scenarios {src} --depth 2|[0,"scenarios",null,[],false]
sweep|native|reader|sweep {src} --depth 2..2|[0,"sweep_passed",null,null,false]
testgen|native|reader|testgen {src} --depth 2 -o {out}|[0,"generated",null,null,true]
testplan|native|reader|testplan {src} --depth 2|[0,"testplan",null,null,false]
typestate|native|reader|typestate {src}|[0,"typestate",null,null,false]
verify|native|reader|verify {src} --depth 2 --no-cache|[0,"verified",null,[],false]
version|native|nonreader|version|null
worker check|worker|reader|check|null
worker verify|worker|reader|verify|null
lsp diagnostics|native|lsp|diagnostics_with_model|null"#;

#[derive(Debug)]
pub struct Row<'a> {
    pub key: &'a str,
    pub owner: &'a str,
    pub role: &'a str,
    pub argv: &'a str,
    pub preserved: Value,
}

pub fn rows() -> Vec<Row<'static>> {
    TABLE
        .lines()
        .map(|line| {
            let fields: Vec<_> = line.split('|').collect();
            assert_eq!(fields.len(), 5, "malformed table row: {line}");
            Row {
                key: fields[0],
                owner: fields[1],
                role: fields[2],
                argv: fields[3],
                preserved: serde_json::from_str(fields[4]).expect("canonical control fields"),
            }
        })
        .collect()
}

pub fn assignment(owner: &str, executions: &[String]) {
    let executed: BTreeSet<_> = executions.iter().cloned().collect();
    let rows = rows();
    let population = |owner: &str| {
        rows.iter()
            .filter(|r| r.owner == owner)
            .map(|r| r.key.to_owned())
            .collect::<BTreeSet<_>>()
    };
    let native = population("native");
    let worker = population("worker");
    assert_eq!((native.len(), worker.len(), rows.len()), (53, 2, 55));
    assert!(native.is_disjoint(&worker));
    assert_eq!(native.union(&worker).count(), rows.len());
    let assigned = if owner == "native" { &native } else { &worker };
    println!(
        "ASSIGNMENT owner={owner} executed={} native={} worker={} total={}",
        executions.len(),
        native.len(),
        worker.len(),
        rows.len()
    );
    assert_eq!(
        executions.len(),
        assigned.len(),
        "actual primary-row execution count"
    );
    assert_eq!(&executed, assigned, "actual primary-row execution set");
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}

pub fn gated() -> fsl_syntax::ParseOptions {
    fsl_syntax::ParseOptions::with_unstable(
        fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
    )
}

pub const DOMAIN: &str = "domain D { enum Status { Open, Closed } aggregate Doc { state { status: Status = Open } command Close {} command Reopen {} event ClosedDoc {} event OpenedDoc {} decide Close { requires status == Open emits ClosedDoc } decide Reopen { requires status == Closed emits OpenedDoc } evolve ClosedDoc { status = Closed } evolve OpenedDoc { status = Open } invariant Typed { status == Open or status == Closed } } }\n";
pub const KERNEL: &str = "spec S { state { done: Bool = false } action finish() { done = true } invariant Typed { true } }\n";
pub const ABSTRACT: &str = "spec Abstract { state { done: Bool = false } action finish() { done = true } invariant Typed { true } }\n";
pub const DOCUMENT_LEGACY: &str = "requirements S { state { done: Bool = false } requirement REQ-RAW-001 \"Raw\" { action finish() \"REQ-PROBE-001: 日本語😀\" { done = true } } invariant Typed { true } }\n";
pub const WARNING_CASES: &[(&str, &str, &str)] = &[
    (
        "deprecated_domain_enum_union",
        "type Status = Open | Closed",
        DOMAIN,
    ),
    (
        "legacy_string_metadata",
        "\"REQ-PROBE-001: 日本語😀\"",
        "spec S { state { done: Bool = false } action finish() \"REQ-PROBE-001: 日本語😀\" { done = true } invariant Typed { true } }\n",
    ),
    (
        "legacy_quantifier_colon",
        "forall",
        "spec S { state { done: Bool = false } action finish() { done = true } @requirement(\"REQ-PROBE-001\", \"日本語😀\") invariant Typed { forall i in 0..1: done == true or done == false } }\n",
    ),
    (
        "inline_action_maps",
        "maps finish()",
        "requirements S { implements Abstract from \"abstract.fsl\" { map done = done } state { done: Bool = false } action complete() maps finish() { done = true } invariant Typed { true } }\n",
    ),
];

pub fn legacy(code: &str, body: &str) -> String {
    if code == "deprecated_domain_enum_union" {
        body.replace(
            "enum Status { Open, Closed }",
            "type Status = Open | Closed",
        )
    } else {
        body.to_owned()
    }
}

pub fn headed(body: &str) -> String {
    format!("fsl 5\n{body}")
}
pub fn rejected_domain() -> String {
    headed(&legacy("deprecated_domain_enum_union", DOMAIN))
}
pub fn rejected_metadata() -> String {
    headed(WARNING_CASES[1].2)
}
pub fn model_error(source: &str) -> String {
    let broken = source
        .replace("status == Open or status == Closed", "missing_name")
        .replace("done == true or done == false", "missing_name")
        .replace(
            "invariant Typed { true }",
            "invariant Typed { missing_name }",
        );
    assert_ne!(broken, source);
    broken
}

pub fn findings(source: &str, path: &str, edition: fsl_syntax::Edition) -> Vec<Value> {
    fslc_rust::migration::plan_migration(source, path, edition, &gated())
        .expect("valid migration case")
        .diagnostics
        .iter()
        .map(|f| f.json(path, edition))
        .collect()
}

pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = root().join(".codex/tmp").join(format!(
            "edition-1449-{}-{}-{}",
            std::process::id(),
            module_path!().replace("::", "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("test scratch inside worktree");
        let this = Self(path);
        this.write("abstract.fsl", ABSTRACT);
        this.write(
            "mapping.fsl",
            "refinement R { impl S abs S map done = done action finish() -> finish() }\n",
        );
        this.write(
            "metrics.jsonl",
            "{\"metric\":\"accuracy\",\"passed\":true}\n",
        );
        this.write("records.json", "[]\n");
        this.write(
            "schema.sql",
            "CREATE TABLE users (id INTEGER PRIMARY KEY);\n",
        );
        this
    }
    pub fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, source).expect("write test-owned input");
        path
    }
    pub fn args(&self, template: &str, source: &str) -> Vec<String> {
        let src = self.write("source.fsl", source);
        self.write(
            "chain.toml",
            &format!("[design]\nfile = {}\ndepth = 2\n", json!(src)),
        );
        let replacements = [
            ("{src}", src),
            ("{manifest}", self.0.join("chain.toml")),
            ("{out}", self.0.join("out")),
            (
                "{artifact}",
                root().join("rust/fslc/tests/fixtures/error_envelope_document.md"),
            ),
            ("{record}", self.0.join("record.json")),
            ("{mapping}", self.0.join("mapping.fsl")),
            ("{metrics}", self.0.join("metrics.jsonl")),
            ("{records}", self.0.join("records.json")),
            ("{sql}", self.0.join("schema.sql")),
            (
                "{trace}",
                root().join("rust/fslc/tests/fixtures/error_envelope_empty_records.json"),
            ),
            (
                "{replaytrace}",
                root().join("rust/fslc/tests/fixtures/replay_trace.valid.v1.json"),
            ),
            (
                "{domainlog}",
                root().join("rust/fslc/tests/fixtures/issue_518_clean.jsonl"),
            ),
            (
                "{log}",
                root().join("examples/causal/evidence/incident-observation-log.jsonl"),
            ),
            (
                "{scope}",
                root().join("examples/causal/evidence/incident-replay-scope.json"),
            ),
        ];
        expand(template, &replacements)
    }
}

#[derive(Default)]
pub struct Results {
    pub checked: usize,
    pub failures: Vec<String>,
    observations: Vec<(String, bool)>,
}
impl Results {
    pub fn error_envelope(
        &mut self,
        label: &str,
        output: &std::process::Output,
        expected: &Value,
        side_effects_absent: bool,
    ) {
        let actual = without_versions(value(output));
        self.check(
            label,
            (output.status.code(), &actual, side_effects_absent),
            (Some(2), expected, true),
            output.status.code() == Some(2) && actual == *expected && side_effects_absent,
        );
    }
    pub fn check(
        &mut self,
        label: &str,
        actual: impl std::fmt::Debug,
        expected: impl std::fmt::Debug,
        agrees: bool,
    ) {
        self.checked += 1;
        println!("CELL {label} agreement={agrees} produced={actual:?} expected={expected:?}");
        self.observations.push((label.to_owned(), agrees));
        if !agrees {
            self.failures.push(label.to_owned());
        }
    }
    pub fn finish(self) {
        println!(
            "TOTAL checked={} preserved={} red={}",
            self.checked,
            self.checked - self.failures.len(),
            self.failures.len()
        );
        CAPTURE.with(|cells| {
            if let Some(cells) = cells.borrow_mut().as_mut() {
                cells.extend(self.observations);
            }
        });
        let unexpected: Vec<_> = self
            .failures
            .iter()
            .filter(|label| {
                label.starts_with("preserved/")
                    || (deferred(label).is_none() && !out_of_scope(label))
            })
            .collect();
        assert!(
            unexpected.is_empty(),
            "{} failing cells: {:?}",
            unexpected.len(),
            unexpected
        );
    }
}
