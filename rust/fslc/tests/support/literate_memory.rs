// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita
//! Complete acceptance anchors captured from the CLI before the in-memory change.
//! The verifier and core versions are filled from the package when loaded; the Z3
//! version and solver counts stay pinned.
#![allow(dead_code)]

#[path = "fifo_snapshot.rs"]
pub mod child;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const LEAF: &str = "spec S { state { done: Bool = false } action finish() { done = true } invariant Typed { true } }\n";
pub const COMPOSE: &str = "compose C { use S as s from \"dep.fsl\" }\n";
pub const LEGACY: &str = "spec S { state { done: Bool = false } action finish() { done = true } invariant Typed \"REQ-DEP-001: 日本語😀\" { true } }\n";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
pub fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}
pub fn markdown(source: &str) -> String {
    format!("# Acceptance\n\n```fsl\n{source}```\n")
}
pub fn args(command: &'static str, path: &'static str) -> Vec<&'static str> {
    match command {
        "verify" => vec![command, path, "--depth", "4", "--no-cache"],
        "scenarios" => vec![command, path, "--depth", "4"],
        _ => vec![command, path],
    }
}
pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "fslc-1420-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).expect("exclusive disposable fixture ownership");
        let scratch = Self(dir);
        std::fs::create_dir(scratch.0.join("src")).unwrap();
        scratch
    }
    pub fn write(&self, name: &str, source: &str) {
        std::fs::write(self.0.join(name), source).unwrap();
    }
    pub fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command
            .args(arguments)
            .current_dir(&self.0)
            .env("FSLC_CACHE_DIR", self.0.join("cache"))
            .env_remove("FSLC_CACHE")
            .env_remove("FSLC_CACHE_VERIFY");
        command
    }
    pub fn run(&self, arguments: &[&str]) -> Observed {
        let mut child = child::ReapedChild::new(
            self.command(arguments)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let pid = child.child.id();
        let observed = Observed {
            pid,
            output: child::wait_for_output(&mut child),
        };
        raw(&format!("completed argv={arguments:?}"), &observed);
        observed // Raw bytes are logged BEFORE any caller's semantic precheck can panic.
    }
    pub fn names(&self) -> BTreeSet<std::ffi::OsString> {
        std::fs::read_dir(self.0.join("src"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub struct Observed {
    pub pid: u32,
    pub output: Output,
}
impl std::ops::Deref for Observed {
    type Target = Output;
    fn deref(&self) -> &Output {
        &self.output
    }
}
pub fn json(output: &Observed) -> Value {
    serde_json::from_slice(&output.stdout).expect("complete CLI JSON")
}
pub fn raw(label: &str, output: &Observed) {
    eprintln!(
        "1420 RAW {label}: pid={}; status={:?}; stdout_bytes={:?}; stderr_bytes={:?}",
        output.pid, output.status, output.stdout, output.stderr
    );
}
pub fn normal(command: &str, output: &Observed) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "raw status/stdout/stderr above"
    );
    let value = json(output);
    if command == "check" {
        assert_eq!(value["result"], "ok");
    }
    if command == "verify" {
        assert_eq!(value["result"], "verified");
        assert_eq!(value["completeness"], "bounded");
    }
}

#[derive(Clone)]
pub struct CacheBinding {
    pub key: String,
    pub cost: Value,
}
pub struct Case {
    pub fixture: &'static str,
    pub command: &'static str,
    pub input: &'static str,
    pub cache_binding: Option<CacheBinding>,
}
pub fn case(fixture: &'static str, command: &'static str, input: &'static str) -> Case {
    Case {
        fixture,
        command,
        input,
        cache_binding: None,
    }
}
pub fn keyed_case(fixture: &'static str, binding: &CacheBinding) -> Case {
    assert!(valid_key(&binding.key));
    Case {
        fixture,
        command: "verify",
        input: "src/doc.md",
        cache_binding: Some(binding.clone()),
    }
}
pub fn logical_label(input: &str, child_pid: u32) -> String {
    let path = Path::new(input);
    if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") {
        return input.into();
    }
    let stem = path.file_stem().and_then(std::ffi::OsStr::to_str).unwrap();
    path.with_file_name(format!(".{stem}.literate-{child_pid}.fsl"))
        .to_str()
        .unwrap()
        .into()
}

fn valid_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
// Only this test's cache/verify/v3/<2hex>/<64hex>.json; never a global cache walk.
#[allow(clippy::unnecessary_debug_formatting)]
pub fn cache_entries(s: &Scratch) -> BTreeMap<String, Value> {
    let directory = s.0.join("cache/verify/v3");
    let mut entries = BTreeMap::new();
    if !directory.exists() {
        return entries;
    }
    for prefix in std::fs::read_dir(directory).unwrap() {
        let prefix = prefix.unwrap();
        let name = prefix.file_name().into_string().unwrap();
        if name.len() != 2
            || !name
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            continue;
        }
        assert!(
            prefix.file_type().unwrap().is_dir(),
            "owned cache prefix must be a real directory"
        );
        for entry in std::fs::read_dir(prefix.path()).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let Some(key) = path.file_stem().and_then(std::ffi::OsStr::to_str) else {
                continue;
            };
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") || !valid_key(key)
            {
                continue;
            }
            assert!(entry.file_type().unwrap().is_file(), "no cache symlink");
            assert_eq!(&key[..2], name.as_str());
            let bytes = std::fs::read(&path).unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["schema"], "fslc-rust-cache.v2");
            assert_eq!(body["key"], key, "exact entry.key == filename stem");
            eprintln!("1420 OWN CACHE entry={path:?}; bytes={bytes:?}");
            assert!(entries.insert(key.into(), body).is_none());
        }
    }
    entries
}
pub fn bind_new_cache_key(
    s: &Scratch,
    before: &BTreeMap<String, Value>,
    cold: &Observed,
) -> CacheBinding {
    let output = json(cold);
    let spec = output["spec"].as_str().expect("cold root spec");
    let depth = output["depth"].as_u64().expect("cold root depth");
    let relevant = cache_entries(s)
        .into_iter()
        .filter(|(key, entry)| {
            !before.contains_key(key)
                && entry["output"]["spec"] == spec
                && entry["output"]["depth"] == depth
        })
        .collect::<Vec<_>>();
    assert_eq!(
        relevant.len(),
        1,
        "require one relevant NEW entry after cold VC"
    );
    let binding = CacheBinding {
        key: relevant[0].0.clone(),
        cost: relevant[0].1["output"]
            .get("cost")
            .expect("published solve cost")
            .clone(),
    };
    check_own_cost(&output, &binding.cost)
        .expect("cold output must equal own published entry cost");
    binding
}
fn warm_packet(mut semantic_body: Value, key: &str) -> Value {
    assert!(valid_key(key));
    assert!(
        semantic_body.get("cache").is_none(),
        "literal warm semantic anchor must not bake in another producer's cache key"
    );
    semantic_body["cache"] = json!({"hit":true,"key":key,"source":"exact"});
    semantic_body
}

fn anchor_data() -> Value {
    let mut data: Value = serde_json::from_str(include_str!("literate_memory_anchors.json"))
        .expect("generated BASE-only anchors");
    for anchor in data["anchors"].as_object_mut().unwrap().values_mut() {
        let Some(versions) = anchor["json"].get_mut("versions") else {
            continue;
        };
        for component in ["verifier", "core"] {
            versions[component]["version"] = json!(env!("CARGO_PKG_VERSION"));
        }
    }
    data
}
fn anchor_for(fixture: &str, command: &str) -> Option<Value> {
    let data = anchor_data();
    let name = format!("{fixture}/{command}");
    data["bindings"]
        .get(name.as_str())
        .and_then(Value::as_str)
        .map(|id| data["anchors"][id].clone())
}
fn toggle_verify() -> Value {
    anchor_for("toggle", "verify").unwrap()["json"].clone()
}
fn check_number(value: f64) -> Result<(), String> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err("nonfinite/negative diagnostic scalar".into())
    }
}
fn numeric_scalar(value: &Value, pointer: &str) -> Result<(), String> {
    let number = value
        .pointer(pointer)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("missing/nonnumeric observed scalar {pointer}"))?;
    check_number(number)
}
fn equal_json(
    fixture: &str,
    command: &str,
    expected: &Value,
    actual: &Value,
) -> Result<(), String> {
    let mut comparable = actual.clone();
    if let Some(anchor) = anchor_for(fixture, command) {
        for pointer in anchor["times"]
            .as_array()
            .expect("finite observed time pointers")
        {
            let pointer = pointer.as_str().unwrap();
            numeric_scalar(expected, pointer)?;
            numeric_scalar(actual, pointer)?;
            *comparable.pointer_mut(pointer).unwrap() = expected.pointer(pointer).unwrap().clone();
        }
        // Classify only the immutable known BASE binding, never either comparison clone.
        // Historical numeric_memory flags are measurements, not permission for warm drift.
        // Queried native-Z3 allocation telemetry is independent only outside a prebound hit.
        if command == "verify"
            && anchor["warm"] == false
            && anchor["json"]["versions"]["solver"]["backend"] == "native-z3"
            && anchor["json"]["cost"]["solver"]["checks"]
                .as_u64()
                .is_some_and(|checks| checks > 0)
        {
            const MEMORY: &str = "/cost/solver/memory_mb";
            numeric_scalar(expected, MEMORY)?;
            numeric_scalar(actual, MEMORY)?;
            *comparable.pointer_mut(MEMORY).unwrap() = expected.pointer(MEMORY).unwrap().clone();
        }
    }
    if expected != &comparable {
        return Err(format!(
            "full JSON mismatch: expected={expected:#}; actual={actual:#}"
        ));
    }
    Ok(())
}
fn check_own_cost(public: &Value, cached_cost: &Value) -> Result<(), String> {
    let cost = public.get("cost").ok_or("missing public cost")?;
    if !cost.is_object() || !cached_cost.is_object() || cost != cached_cost {
        return Err(format!(
            "public cost differs from own published/prebound cost: {cost:#} != {cached_cost:#}"
        ));
    }
    Ok(())
}
fn bind_warm_memory(
    fixture: &str,
    expected: &mut Value,
    prebound_cost: &Value,
) -> Result<(), String> {
    if let Some(anchor) = anchor_for(fixture, "verify")
        && anchor["warm"] == true
        && anchor["json"]["versions"]["solver"]["backend"] == "native-z3"
        && anchor["json"]["cost"]["solver"]["checks"]
            .as_u64()
            .is_some_and(|checks| checks > 0)
    {
        // This is exact expected-side reconstruction from the entry captured BEFORE
        // the warm CLI, not an independent warm-memory allowance or actual-output rewrite.
        numeric_scalar(expected, "/cost/solver/memory_mb")?;
        numeric_scalar(prebound_cost, "/solver/memory_mb")?;
        *expected.pointer_mut("/cost/solver/memory_mb").unwrap() =
            prebound_cost.pointer("/solver/memory_mb").unwrap().clone();
    }
    Ok(())
}
fn equal_envelope(
    exit: i32,
    stderr: &[u8],
    actual_exit: Option<i32>,
    actual_stderr: &[u8],
) -> Result<(), String> {
    if actual_exit != Some(exit) {
        return Err("exit mismatch".into());
    }
    if actual_stderr != stderr {
        return Err("stderr bytes mismatch".into());
    }
    Ok(())
}
fn root_edition5(file: &str) -> Value {
    let source = format!("\n\n\nfsl 5\n{LEGACY}\n");
    let start = source.find("\"REQ-DEP-001").unwrap();
    let end = start + source[start + 1..].find('"').unwrap() + 2;
    let declaration = source[..start].rfind("invariant Typed").unwrap();
    let point = |offset| {
        let prefix = &source[..offset];
        (
            prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
            prefix.rsplit('\n').next().unwrap().chars().count() + 1,
        )
    };
    let (line, column) = point(start);
    let (end_line, end_column) = point(end);
    let (_, declaration_column) = point(declaration);
    let replacement = "@requirement(\"REQ-DEP-001\", \"日本語😀\")";
    json!({"fsl":"1.0","result":"error","kind":"unsupported_in_edition","edition":"next","fsl_edition":5,
        "findings":[{"code":"legacy_string_metadata","kind":"legacy_string_metadata","taxonomy":"deprecated",
            "severity":"error","edition":"next","message":"legacy string metadata is deprecated; use a typed annotation",
            "loc":{"file":file,"line":line,"column":column,"end_line":end_line,"end_column":end_column},
            "canonical_replacement":replacement,"machine_applicable":true,
            "edits":[{"start":declaration,"end":declaration,"replacement":format!("{replacement}\n{}", " ".repeat(declaration_column - 1))},
                {"start":start,"end":end,"replacement":""}]}]})
}
fn expected(c: &Case, pid: u32) -> (i32, Value, Vec<u8>) {
    match (c.fixture, c.command) {
        ("root-edition5", "check" | "verify" | "scenarios") => {
            let file = if c.command == "scenarios" {
                logical_label(c.input, pid)
            } else {
                c.input.into()
            };
            let mut body = root_edition5(&file);
            if c.command != "scenarios" {
                body["versions"] = toggle_verify()["versions"].clone();
            }
            (2, body, vec![])
        }
        ("no-fence", _) => (
            2,
            json!({"fsl":"1.0","result":"error","kind":"io",
            "message":format!("{}: Markdown file does not contain any ```fsl fenced code blocks", c.input)}),
            vec![],
        ),
        ("unsupported", "html" | "testgen" | "ledger") => (
            2,
            json!({
                "fsl":"1.0","result":"error","kind":"usage","diagnostic_code":"FSL-INPUT-LITERATE-UNSUPPORTED",
                "loc":{"file":c.input}, "message":format!("fslc {} does not support literate Markdown input; only check, verify, scenarios extract ```fsl fenced code from a .md file -- pass '{}' to one of those, or extract the fenced code into a .fsl file first", c.command, c.input)
            }),
            vec![],
        ),
        _ => {
            let anchor = anchor_for(c.fixture, c.command)
                .expect("all authored fixture/command bindings compiled in");
            assert_eq!(anchor["input"], c.input);
            let mut body = anchor["json"].clone(); // BASE first capture only, never current SUT output.
            for field in anchor["pid_fields"].as_array().unwrap() {
                let value = format!(
                    "{}{}{}",
                    field["prefix"].as_str().unwrap(),
                    logical_label(c.input, pid),
                    field["suffix"].as_str().unwrap()
                );
                *body
                    .pointer_mut(field["pointer"].as_str().unwrap())
                    .unwrap() = json!(value);
            }
            if anchor["warm"] == true {
                let binding = c
                    .cache_binding
                    .as_ref()
                    .expect("exact key/cost bound before warm CLI");
                assert!(valid_key(&binding.key));
                assert_eq!(body["cache"]["hit"], true);
                assert_eq!(body["cache"]["source"], "exact");
                *body.pointer_mut("/cache/key").unwrap() = json!(binding.key.as_str());
                bind_warm_memory(c.fixture, &mut body, &binding.cost)
                    .expect("valid exact prebound warm memory");
            } else {
                assert!(c.cache_binding.is_none());
            }
            let stderr: Vec<u8> = serde_json::from_value(anchor["stderr_bytes"].clone()).unwrap();
            (
                i32::try_from(anchor["exit"].as_i64().unwrap()).unwrap(),
                body,
                stderr,
            )
        }
    }
}
pub fn compare_packets(packets: Vec<(Case, Observed)>) {
    for (c, output) in &packets {
        raw(&format!("{}/{}", c.fixture, c.command), output);
    }
    for (c, output) in packets {
        let (exit, body, stderr) = expected(&c, output.pid);
        let actual = json(&output);
        if let Some(binding) = &c.cache_binding {
            check_own_cost(&actual, &binding.cost)
                .expect("warm output must retain its exact prebound entry cost");
        }
        equal_envelope(exit, &stderr, output.status.code(), &output.stderr).unwrap();
        equal_json(c.fixture, c.command, &body, &actual).unwrap();
    }
}
pub struct ReadOnly {
    path: PathBuf,
    original: std::fs::Permissions,
}
impl ReadOnly {
    pub fn new(path: PathBuf) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let original = std::fs::metadata(&path).unwrap().permissions();
        let guard = Self { path, original };
        std::fs::set_permissions(&guard.path, std::fs::Permissions::from_mode(0o555)).unwrap();
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(guard.path.join(".same-identity-write-denial"))
        {
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
            other => panic!("BLOCKED: no actual same-identity write denial; {other:?}"),
        }
        guard
    }
}
impl Drop for ReadOnly {
    fn drop(&mut self) {
        std::fs::set_permissions(&self.path, self.original.clone())
            .expect("restore owned permissions");
    }
}

#[cfg(test)]
mod comparison_controls {
    use super::*;
    const INDEPENDENT: &[&str] = &[
        "toggle",
        "compose",
        "strict-tags",
        "strict-tags-lemma",
        "root-edition4",
        "dependency-edition4",
        "governance-ok",
        "implements-ok",
        "implements-bad",
        "ordinary",
        "governance-cold",
        "compose-cold",
        "compose-prose-cold",
        "compose-changed-cold",
    ];
    const PREBOUND: &[&str] = &[
        "governance-warm",
        "governance-cached-broken",
        "compose-warm",
        "compose-prose-warm",
        "compose-changed-warm",
        "compose-unrelated-warm",
    ];
    fn remove_leaf(value: &mut Value, pointer: &str) {
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        value
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key)
            .unwrap();
    }
    fn scalar_controls(fixture: &str, command: &str, seed: &Value, pointer: &str) {
        let mut changed = seed.clone();
        *changed.pointer_mut(pointer).unwrap() = json!(123.5);
        assert!(equal_json(fixture, command, seed, &changed).is_ok());
        for invalid in [
            Value::Null,
            json!("0.1"),
            json!(true),
            json!({}),
            json!([]),
            json!(-1),
        ] {
            let mut bad = seed.clone();
            *bad.pointer_mut(pointer).unwrap() = invalid;
            assert!(equal_json(fixture, command, seed, &bad).is_err());
            assert!(equal_json(fixture, command, &bad, seed).is_err());
            assert!(equal_json(fixture, command, &bad, &bad).is_err());
        }
        let mut missing = seed.clone();
        remove_leaf(&mut missing, pointer);
        assert!(equal_json(fixture, command, seed, &missing).is_err());
        assert!(equal_json(fixture, command, &missing, seed).is_err());
        assert!(equal_json(fixture, command, &missing, &missing).is_err());
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert!(check_number(number).is_err()); // JSON cannot encode nonfinite Numbers.
        }
    }
    #[test]
    fn all_observed_time_paths_are_live_numeric_and_fixture_scoped() {
        let data = anchor_data();
        for (id, anchor) in data["anchors"].as_object().unwrap() {
            let (fixture, command) = id.rsplit_once('/').unwrap();
            for pointer in anchor["times"].as_array().unwrap() {
                scalar_controls(fixture, command, &anchor["json"], pointer.as_str().unwrap());
            }
        }
        let seed = toggle_verify();
        let mut changed = seed.clone();
        changed["cost"]["elapsed_s"] = json!(123.5);
        assert!(equal_json("toggle", "scenarios", &seed, &changed).is_err());
        assert!(equal_json("unobserved-fixture", "verify", &seed, &changed).is_err());
    }
    #[test]
    fn process_labels_and_all_location_coordinates_stay_exact() {
        assert_eq!(logical_label("src/doc.md", 41), "src/.doc.literate-41.fsl");
        for (fixture, command, input) in [
            ("bad-dependency", "verify", "src/doc.md"),
            ("bad-dependency", "scenarios", "src/doc.md"),
            ("wrong-parent", "verify", "real/doc.md"),
            ("wrong-parent", "scenarios", "real/doc.md"),
            ("root-edition4", "scenarios", "src/doc.md"),
            ("implements-ok", "scenarios", "src/doc.md"),
            ("implements-bad", "scenarios", "src/doc.md"),
        ] {
            let c = case(fixture, command, input);
            let (_, expected_41, _) = expected(&c, 41);
            let (_, wrong_pid, _) = expected(&c, 42);
            assert!(equal_json(fixture, command, &expected_41, &wrong_pid).is_err());
        }
        let expected = root_edition5(&logical_label("src/doc.md", 41));
        for key in ["line", "column", "end_line", "end_column"] {
            let mut changed = expected.clone();
            changed["findings"][0]["loc"][key] = json!(999);
            assert!(equal_json("root-edition5", "scenarios", &expected, &changed).is_err());
        }
    }
    #[test]
    fn bound_keys_reject_valid_other_stale_and_unrelated_drift() {
        let original = "0".repeat(64);
        let prose = "1".repeat(64);
        let dependency = "2".repeat(64);
        let other = "3".repeat(64);
        let semantic = toggle_verify();
        let compare = |expected_key: &str, actual_key: &str| {
            equal_json(
                "cache-key-control",
                "verify",
                &warm_packet(semantic.clone(), expected_key),
                &warm_packet(semantic.clone(), actual_key),
            )
        };
        assert!(compare(&original, &original).is_ok());
        assert!(compare(&original, &other).is_err());
        assert!(compare(&prose, &original).is_err());
        assert!(compare(&dependency, &original).is_err());
        assert!(compare(&dependency, &other).is_err());
    }
    #[test]
    fn memory_roles_are_disjoint_complete_and_live_for_all_base_bindings() {
        const MEMORY: &str = "/cost/solver/memory_mb";
        let independent: BTreeSet<_> = INDEPENDENT.iter().copied().collect();
        let prebound: BTreeSet<_> = PREBOUND.iter().copied().collect();
        assert_eq!(independent.len(), 14);
        assert_eq!(prebound.len(), 6);
        assert!(independent.is_disjoint(&prebound));
        let data = anchor_data();
        let bindings = data["bindings"].as_object().unwrap();
        assert_eq!(bindings.len(), 62);
        let mut counts = [0; 3];
        for (binding, id) in bindings {
            let (fixture, command) = binding.rsplit_once('/').unwrap();
            let anchor = &data["anchors"][id.as_str().unwrap()];
            let seed = &anchor["json"];
            assert!(equal_json(fixture, command, seed, seed).is_ok());
            let mut changed = seed.clone();
            changed["cost"]["solver"]["memory_mb"] = json!(123.5);
            if command == "verify" && independent.contains(fixture) {
                counts[0] += 1;
                assert_eq!(anchor["warm"], false);
                assert_eq!(seed["versions"]["solver"]["backend"], "native-z3");
                assert!(seed["cost"]["solver"]["checks"].as_u64().unwrap() > 0);
                scalar_controls(fixture, command, seed, MEMORY);
            } else {
                if command == "verify" && prebound.contains(fixture) {
                    counts[1] += 1;
                    assert_eq!(anchor["warm"], true);
                    assert_eq!(seed["versions"]["solver"]["backend"], "native-z3");
                    assert!(seed["cost"]["solver"]["checks"].as_u64().unwrap() > 0);
                } else {
                    counts[2] += 1;
                }
                assert!(equal_json(fixture, command, seed, &changed).is_err());
            }
        }
        assert_eq!(counts, [14, 6, 42]);
        for &fixture in INDEPENDENT {
            let seed = anchor_for(fixture, "verify").unwrap()["json"].clone();
            let mut counter = seed.clone();
            counter["cost"]["solver"]["checks"] = json!(999);
            assert!(equal_json(fixture, "verify", &seed, &counter).is_err());
            let mut location = seed.clone();
            location["loc"] = json!({"file":"wrong","line":999,"column":999});
            assert!(equal_json(fixture, "verify", &seed, &location).is_err());
            // Forging both clones cannot turn immutable independent eligibility off.
            let mut disguised = seed.clone();
            disguised["versions"]["solver"]["backend"] = json!("other");
            disguised["cost"]["solver"]["checks"] = json!(0);
            scalar_controls(fixture, "verify", &disguised, MEMORY);
        }
        // Nor can a queried native body opt unknown/unbound/nonqueried roles in.
        let seed = toggle_verify();
        let mut measured = seed.clone();
        *measured.pointer_mut(MEMORY).unwrap() = json!(123.5);
        for (fixture, command) in [
            ("unobserved-fixture", "verify"),
            ("root-edition5", "verify"),
            ("parse", "verify"),
            ("toggle", "check"),
        ] {
            assert!(equal_json(fixture, command, &seed, &measured).is_err());
        }
    }
    #[test]
    fn ci_observed_memory_changes_pass_but_stable_fields_and_other_bindings_reject() {
        const MEMORY: &str = "/cost/solver/memory_mb";
        for (fixture, baseline, observed) in [
            ("compose", 17.67, 17.24),
            ("governance-ok", 17.68, 17.24),
            ("strict-tags", 18.36, 17.91),
            ("governance-cold", 17.68, 17.24),
            ("root-edition4", 17.68, 17.24),
            ("strict-tags-lemma", 18.35, 17.9),
        ] {
            let seed = anchor_for(fixture, "verify").unwrap()["json"].clone();
            if fixture == "strict-tags-lemma" {
                assert_eq!(seed["result"], "proved");
            }
            assert_eq!(seed.pointer(MEMORY).unwrap(), &json!(baseline));
            let mut measured = seed.clone();
            *measured.pointer_mut(MEMORY).unwrap() = json!(observed);
            assert!(equal_json(fixture, "verify", &seed, &measured).is_ok());
            for command in ["check", "scenarios"] {
                assert!(equal_json(fixture, command, &seed, &measured).is_err());
            }
            assert!(equal_json("unobserved-fixture", "verify", &seed, &measured).is_err());
            for (pointer, replacement) in [
                ("/result", json!("violated")),
                ("/fsl", json!("drift")),
                ("/versions/verifier/version", json!("drift")),
                ("/versions/core/version", json!("drift")),
                ("/versions/solver/version", json!("drift")),
                ("/checked_to_depth", json!(999)),
                ("/cost/solver/checks", json!(999)),
                ("/cost/solver/decisions", json!(999)),
                ("/cost/solver/conflicts", json!(999)),
                ("/cost/solver/propagations", json!(999)),
                ("/cost/properties/0/checks", json!(999)),
            ] {
                let mut changed = measured.clone();
                *changed.pointer_mut(pointer).unwrap() = replacement;
                assert!(equal_json(fixture, "verify", &seed, &changed).is_err());
            }
            let mut location = measured.clone();
            location["loc"] = json!({"file":"wrong","line":999,"column":999});
            assert!(equal_json(fixture, "verify", &seed, &location).is_err());
            if fixture == "strict-tags" {
                let mut location = measured.clone();
                location["warnings"][1]["loc"] = json!({"file":"wrong","line":999});
                assert!(equal_json(fixture, "verify", &seed, &location).is_err());
            }
        }
    }
    #[test]
    fn cold_and_warm_costs_must_equal_own_published_or_prebound_cost() {
        for fixture in [
            "compose-cold",
            "compose-warm",
            "compose-prose-cold",
            "compose-prose-warm",
            "compose-changed-cold",
            "compose-changed-warm",
            "compose-unrelated-warm",
            "governance-cold",
            "governance-warm",
            "governance-cached-broken",
        ] {
            let public = anchor_for(fixture, "verify").unwrap()["json"].clone();
            let cached = public["cost"].clone();
            assert!(check_own_cost(&public, &cached).is_ok());
            let mut wrong = cached.clone();
            wrong["solver"]["memory_mb"] = json!(123.5);
            assert!(check_own_cost(&public, &wrong).is_err());
            let mut altered = public.clone();
            altered["cost"]["elapsed_s"] = json!(123.5);
            assert!(check_own_cost(&altered, &cached).is_err());
            let mut absent = public.clone();
            absent.as_object_mut().unwrap().remove("cost");
            assert!(check_own_cost(&absent, &cached).is_err());
            assert!(check_own_cost(&public, &Value::Null).is_err());
        }
    }
    #[test]
    fn dependency_measurement_and_exact_bound_warm_context_pass_before_rejects() {
        const MEMORY: &str = "/cost/solver/memory_mb";
        let cold_seed = anchor_for("compose-changed-cold", "verify").unwrap()["json"].clone();
        let mut measured_cold = cold_seed.clone();
        *measured_cold.pointer_mut(MEMORY).unwrap() = json!(123.5);
        assert!(equal_json("compose-changed-cold", "verify", &cold_seed, &measured_cold).is_ok());
        assert!(check_own_cost(&measured_cold, &measured_cold["cost"]).is_ok());
        let cold_error = check_own_cost(&measured_cold, &cold_seed["cost"]).unwrap_err();
        assert!(cold_error.contains("public cost differs"));
        for &fixture in PREBOUND {
            let seed = anchor_for(fixture, "verify").unwrap()["json"].clone();
            let original_binding = CacheBinding {
                key: seed["cache"]["key"].as_str().unwrap().into(),
                cost: seed["cost"].clone(),
            };
            let original_case = keyed_case(fixture, &original_binding);
            let (_, original_expected, _) = expected(&original_case, 41);
            assert!(equal_json(fixture, "verify", &original_expected, &seed).is_ok());
            assert!(check_own_cost(&seed, &original_binding.cost).is_ok());
            let mut new_binding = original_binding.clone();
            new_binding.cost["solver"]["memory_mb"] =
                measured_cold["cost"]["solver"]["memory_mb"].clone();
            let bound_case = keyed_case(fixture, &new_binding);
            let (_, bound_expected, _) = expected(&bound_case, 41);
            let mut actual = seed.clone();
            *actual.pointer_mut(MEMORY).unwrap() = measured_cold.pointer(MEMORY).unwrap().clone();
            assert!(equal_json(fixture, "verify", &bound_expected, &actual).is_ok());
            assert!(check_own_cost(&actual, &new_binding.cost).is_ok());
            let current_cache_cost = actual["cost"].clone();
            assert!(check_own_cost(&actual, &current_cache_cost).is_ok());
            // Against an UNCHANGED prebound context, the same drift must reject.
            // Public/current-cache agreement above cannot replace the captured prior binding.
            assert!(
                equal_json(fixture, "verify", &original_expected, &actual)
                    .unwrap_err()
                    .contains("full JSON mismatch")
            );
            assert!(
                check_own_cost(&actual, &original_binding.cost)
                    .unwrap_err()
                    .contains("public cost differs")
            );
            for invalid in [
                Value::Null,
                json!("0.1"),
                json!(true),
                json!({}),
                json!([]),
                json!(-1),
            ] {
                let mut bad_expected = seed.clone();
                *bad_expected.pointer_mut(MEMORY).unwrap() = invalid.clone();
                let error =
                    bind_warm_memory(fixture, &mut bad_expected, &new_binding.cost).unwrap_err();
                assert!(error.contains("observed scalar") || error.contains("nonfinite/negative"));
                let mut bad_cost = new_binding.cost.clone();
                bad_cost["solver"]["memory_mb"] = invalid;
                let error = bind_warm_memory(fixture, &mut seed.clone(), &bad_cost).unwrap_err();
                assert!(error.contains("observed scalar") || error.contains("nonfinite/negative"));
                assert!(bind_warm_memory(fixture, &mut bad_expected, &bad_cost).is_err());
            }
            let mut missing_expected = seed.clone();
            remove_leaf(&mut missing_expected, MEMORY);
            let mut missing_cost = new_binding.cost.clone();
            remove_leaf(&mut missing_cost, "/solver/memory_mb");
            assert!(
                bind_warm_memory(fixture, &mut missing_expected.clone(), &new_binding.cost)
                    .unwrap_err()
                    .contains("observed scalar")
            );
            assert!(
                bind_warm_memory(fixture, &mut seed.clone(), &missing_cost)
                    .unwrap_err()
                    .contains("observed scalar")
            );
            assert!(
                bind_warm_memory(fixture, &mut missing_expected, &missing_cost)
                    .unwrap_err()
                    .contains("observed scalar")
            );
            let mut wrong_version = actual.clone();
            wrong_version["versions"]["verifier"]["version"] = json!("drift");
            assert!(
                equal_json(fixture, "verify", &bound_expected, &wrong_version)
                    .unwrap_err()
                    .contains("full JSON mismatch")
            );
            for (pointer, replacement) in [
                ("/cache/key", json!("f".repeat(64))),
                ("/cache/source", json!("other")),
                ("/result", json!("drift")),
                ("/fsl", json!("drift")),
                ("/cost/solver/checks", json!(999)),
                ("/cost/solver/decisions", json!(999)),
            ] {
                let mut changed = actual.clone();
                *changed.pointer_mut(pointer).unwrap() = replacement;
                assert!(equal_json(fixture, "verify", &bound_expected, &changed).is_err());
            }
            let mut location = actual.clone();
            location["loc"] = json!({"file":"wrong","line":999});
            assert!(equal_json(fixture, "verify", &bound_expected, &location).is_err());
        }
    }
    #[test]
    fn exit_and_raw_stderr_controls_pass_first_then_reject_exact_reason() {
        assert!(equal_envelope(0, &[], Some(0), &[]).is_ok());
        assert!(equal_envelope(2, &[0, 255], Some(2), &[0, 255]).is_ok());
        assert_eq!(
            equal_envelope(0, &[], Some(2), &[]).unwrap_err(),
            "exit mismatch"
        );
        assert_eq!(
            equal_envelope(0, &[], None, &[]).unwrap_err(),
            "exit mismatch"
        );
        assert_eq!(
            equal_envelope(2, &[0, 255], Some(2), &[255, 0]).unwrap_err(),
            "stderr bytes mismatch"
        );
        assert_eq!(
            equal_envelope(0, &[], Some(0), &[0]).unwrap_err(),
            "stderr bytes mismatch"
        );
    }
}
