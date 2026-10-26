// SPDX-License-Identifier: Apache-2.0
//! One test-only population for native, Worker, and LSP adapters (#1449).
#![allow(dead_code)] // Each adapter uses only its assigned portion.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

pub fn invoke(binary: &str, args: &[String], cwd: &Path, gate: bool) -> std::process::Output {
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
    command.output().expect("execute working-tree CLI")
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

// key | owner | role | argv (placeholders are files, never inferred dialects).
const TABLE: &str = "\
ai check|native|reader|ai check {src} --depth 2
ai compare|native|nonreader|ai compare --from {metrics} --to {metrics}
ai compat|native|reader|ai compat {src}
ai drift|native|reader|ai drift {src} --logs {records}
ai eval|native|reader|ai eval {src} --records {records}
ai regress|native|reader|ai regress {src} --before-records {records} --after-records {records}
ai replay|native|reader|ai replay {src} --logs {records}
analyze|native|reader|analyze {src}
approval check|native|reader|approval check {src} --record {record}
approval create|native|reader|approval create {src} --kind ledger --artifact {artifact} --approver edition-test -o {out}
approval diff|native|reader|approval diff {src} --record {record}
causal analyze|native|reader|causal analyze {src} --projection causal_graph
causal check|native|reader|causal check {src}
causal diff|native|reader|causal diff {src} {src}
causal ledger|native|reader|causal ledger {src}
causal observe-expectations|native|reader|causal observe-expectations {src} --from-log {log} --mapping {mapping} --scope {scope} --period-start 2026-01-01 --period-end 2026-03-31 --out {out}
causal verify-expectations|native|reader|causal verify-expectations {src} --depth 2
chain|native|reader|chain {manifest}
check|native|reader|check {src}
compat check|native|reader|compat check {src}
conformance|native|reader|conformance {src} --depth 2
counterexample export|native|reader|counterexample export {src} --depth 2 -o {out}
db check|native|reader|db check {src} --depth 2
db import|native|nonreader|db import {sql} --source sql -o {out}
db observe|native|reader|db observe {src} --trace {trace}
diff|native|reader|diff {src} {src} --depth 2
document check|native|reader|document check {src} {artifact}
document claims|native|reader|document claims {src}
document generate|native|reader|document generate {src} -o {out}
domain analyze|native|reader|domain analyze {src}
domain check|native|reader|domain check {src} --depth 2
domain expand|native|reader|domain expand {src} -o {out}
domain generate|native|reader|domain generate {src} -o {out}
domain replay|native|reader|domain replay {src} --logs {domainlog}
domain testgen|native|reader|domain testgen {src} --depth 2 -o {out}
explain|native|reader|explain {src} --depth 2
fmt|native|repair|fmt {src} --check
html|native|reader|html {src} --depth 2 --no-timing -o {out}
kernel|native|reader|kernel {src}
ledger|native|reader|ledger {src} --depth 2 -o {out}
lint|native|repair|lint {src} --edition next
migrate|native|repair|migrate {src} --edition next
mutate|native|reader|mutate {src} --depth 2 --max-mutants 0
refine|native|reader|refine {src} {src} {mapping} --depth 2
replay|native|reader|replay {src} --trace {replaytrace}
scenarios|native|reader|scenarios {src} --depth 2
sweep|native|reader|sweep {src} --depth 2..2
testgen|native|reader|testgen {src} --depth 2 -o {out}
testplan|native|reader|testplan {src} --depth 2
typestate|native|reader|typestate {src}
verify|native|reader|verify {src} --depth 2 --no-cache
version|native|nonreader|version
worker check|worker|reader|check
worker verify|worker|reader|verify
lsp diagnostics|native|lsp|diagnostics_with_model";

#[derive(Debug)]
pub struct Row<'a> {
    pub key: &'a str,
    pub owner: &'a str,
    pub role: &'a str,
    pub argv: &'a str,
}

pub fn rows() -> Vec<Row<'static>> {
    TABLE
        .lines()
        .map(|line| {
            let fields: Vec<_> = line.split('|').collect();
            assert_eq!(fields.len(), 4, "malformed table row: {line}");
            Row {
                key: fields[0],
                owner: fields[1],
                role: fields[2],
                argv: fields[3],
            }
        })
        .collect()
}

pub fn assignment(owner: &str, executions: &[String]) {
    let executed: BTreeSet<_> = executions.iter().cloned().collect();
    let rows = rows();
    let native: BTreeSet<_> = rows
        .iter()
        .filter(|r| r.owner == "native")
        .map(|r| r.key.to_owned())
        .collect();
    let worker: BTreeSet<_> = rows
        .iter()
        .filter(|r| r.owner == "worker")
        .map(|r| r.key.to_owned())
        .collect();
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
            "edition-1449-{}-{}",
            std::process::id(),
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
        template
            .split_whitespace()
            .map(|word| {
                replacements
                    .iter()
                    .find(|(key, _)| *key == word)
                    .map_or_else(
                        || {
                            assert!(!word.starts_with('{'), "unknown placeholder {word}");
                            word.to_owned()
                        },
                        |(_, path)| path.to_str().unwrap().to_owned(),
                    )
            })
            .collect()
    }
}

#[derive(Default)]
pub struct Results {
    pub checked: usize,
    pub failures: Vec<String>,
}
impl Results {
    pub fn check(
        &mut self,
        label: &str,
        actual: impl std::fmt::Debug,
        expected: impl std::fmt::Debug,
        agrees: bool,
    ) {
        self.checked += 1;
        println!("CELL {label} agreement={agrees} produced={actual:?} expected={expected:?}");
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
        assert!(
            self.failures.is_empty(),
            "{} failing cells: {:?}",
            self.failures.len(),
            self.failures
        );
    }
}
