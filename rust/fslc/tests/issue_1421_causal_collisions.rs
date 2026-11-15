// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! CO1–CO3 observable publication contracts. Same-candidate reference runs are
//! controls only: named B/H envelope and raw-byte agreement is captured separately.

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);
const LINK: &str = "derived_from_claim C_Guardrails_AlertQuality";
const GUARD: &str = "OBS_E_GuardrailsVisible_IncidentResponse";
const ALERT: &str = "OBS_E_AlertQualityImproves_IncidentResponse";

fn repository_file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(name)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Node {
    Directory,
    File(Vec<u8>),
    Link(PathBuf),
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, Node> {
    fn visit(root: &Path, directory: &Path, found: &mut BTreeMap<PathBuf, Node>) {
        for entry in std::fs::read_dir(directory).expect("owned directory") {
            let entry = entry.expect("entry");
            let path = entry.path();
            let relative = path.strip_prefix(root).expect("owned path").to_path_buf();
            let kind = entry.file_type().expect("file type");
            if kind.is_symlink() {
                found.insert(
                    relative,
                    Node::Link(std::fs::read_link(path).expect("link target")),
                );
            } else if kind.is_dir() {
                found.insert(relative, Node::Directory);
                visit(root, &path, found);
            } else {
                assert!(kind.is_file());
                found.insert(
                    relative,
                    Node::File(std::fs::read(path).expect("raw bytes")),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    visit(root, root, &mut found);
    found
}

struct Fixture {
    root: PathBuf,
    linked: usize,
}
struct Reference {
    envelope: Value,
    evidence: Vec<Vec<u8>>,
    lifecycle: Vec<Vec<u8>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).expect("remove owned causal fixture");
    }
}

impl Fixture {
    fn new(linked: usize) -> Self {
        assert!(linked <= 2);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "fslc-1421-causal-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("inputs")).expect("owned inputs");
        let root = root.canonicalize().expect("physical invocation directory");
        let source =
            std::fs::read_to_string(repository_file("examples/causal/incident_response.fsl"))
                .expect("existing model");
        let mut seen = 0;
        let model: String = source
            .split_inclusive('\n')
            .filter(|line| {
                if line.trim() == LINK {
                    seen += 1;
                    linked == 2 || (linked == 1 && seen == 2)
                } else {
                    true
                }
            })
            .collect();
        assert_eq!(seen, 2, "only the two existing claim links are controlled");
        std::fs::write(root.join("inputs/model.fsl"), model).expect("model variant");
        for (source, target) in [
            ("examples/causal/incident_system.fsl", "incident_system.fsl"),
            (
                "examples/causal/evidence/incident-observation-log.jsonl",
                "log.jsonl",
            ),
            (
                "examples/causal/evidence/incident-log-mapping.fsl",
                "mapping.fsl",
            ),
            (
                "examples/causal/evidence/incident-replay-scope.json",
                "scope.json",
            ),
        ] {
            std::fs::copy(repository_file(source), root.join("inputs").join(target))
                .expect("existing input");
        }
        Self { root, linked }
    }

    fn ids(&self) -> &'static [&'static str] {
        match self.linked {
            0 => &[],
            1 => &[ALERT],
            2 => &[GUARD, ALERT],
            _ => unreachable!(),
        }
    }

    fn paths(&self, base: &Path) -> Vec<PathBuf> {
        if self.linked == 1 {
            return vec![base.to_path_buf()];
        }
        self.ids()
            .iter()
            .map(|id| {
                let stem = base
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("evidence");
                let extension = base.extension().and_then(|s| s.to_str()).unwrap_or("json");
                base.with_file_name(format!("{stem}.{id}.{extension}"))
            })
            .collect()
    }

    fn observe(&self, out: Option<&Path>, lifecycle: Option<&Path>) -> Output {
        self.process(out, lifecycle, true)
    }

    fn process(&self, out: Option<&Path>, lifecycle: Option<&Path>, period: bool) -> Output {
        let mut args = vec![
            "causal",
            "observe-expectations",
            "inputs/model.fsl",
            "--from-log",
            "inputs/log.jsonl",
            "--mapping",
            "inputs/mapping.fsl",
            "--scope",
            "inputs/scope.json",
        ];
        if period {
            args.extend(["--period-start", "2026-01-01", "--period-end", "2026-03-31"]);
        }
        if let Some(path) = out {
            args.extend(["--out", path.to_str().expect("output path")]);
        }
        if let Some(path) = lifecycle {
            args.extend(["--lifecycle-out", path.to_str().expect("lifecycle path")]);
        }
        fslc_cli::command()
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("causal CLI")
    }

    fn seed(&self, out: &Path, lifecycle: &Path) {
        let paths: BTreeSet<PathBuf> = self
            .paths(out)
            .into_iter()
            .chain(self.paths(lifecycle))
            .collect();
        for path in paths {
            let actual = self.root.join(&path);
            std::fs::create_dir_all(actual.parent().expect("parent")).expect("sentinel parent");
            std::fs::write(
                actual,
                format!("unchanged sentinel for {}\n", path.display()),
            )
            .expect("sentinel");
        }
    }

    fn reference(&self) -> Reference {
        std::fs::create_dir(self.root.join("reference")).expect("reference directory");
        let out = Path::new("reference/evidence.json");
        let lifecycle = Path::new("reference/lifecycle.json");
        let envelope = checked(&self.observe(Some(out), Some(lifecycle)), 0);
        self.assert_envelope(&envelope);
        let evidence = self
            .paths(out)
            .iter()
            .map(|p| std::fs::read(self.root.join(p)).expect("evidence bytes"))
            .collect();
        let lifecycle = self
            .paths(lifecycle)
            .iter()
            .map(|p| std::fs::read(self.root.join(p)).expect("lifecycle bytes"))
            .collect();
        let reference = Reference {
            envelope,
            evidence,
            lifecycle,
        };
        self.assert_artifacts(&reference);
        reference
    }

    fn assert_envelope(&self, value: &Value) {
        assert_eq!(value["result"], "causal_expectations_observed");
        assert_eq!(value["model"], "IncidentResponse");
        assert_eq!(value["schema_version"], "causal-observation.v0");
        assert_eq!(value["formal_result"], "not_run");
        assert_eq!(value["artifacts_generated"], self.linked);
        let expectations = value["expectations"].as_array().expect("expectations");
        assert_eq!(expectations.len(), 2);
        assert_eq!(expectations[0]["id"], "expectation:E_GuardrailsVisible");
        assert_eq!(expectations[0]["verdict"], "pass");
        assert_eq!(expectations[1]["id"], "expectation:E_AlertQualityImproves");
        assert_eq!(expectations[1]["verdict"], "violated");
        for entry in expectations {
            assert_eq!(entry["assurance"], "replay-observed");
            assert!(
                !entry["do_not_assume"]
                    .as_array()
                    .expect("caveats")
                    .is_empty()
            );
        }
        for claim in value["claims"].as_array().expect("claims") {
            assert_eq!(claim["formal_assurance"], "not_run");
            assert_eq!(claim["causal_support"], "untested");
        }
        assert!(
            !value["do_not_assume"]
                .as_array()
                .expect("caveats")
                .is_empty()
        );
    }

    fn assert_artifacts(&self, reference: &Reference) {
        assert_eq!(reference.evidence.len(), self.linked);
        assert_eq!(reference.lifecycle.len(), self.linked);
        for (index, id) in self.ids().iter().enumerate() {
            let bytes = &reference.evidence[index];
            assert!(bytes.ends_with(b"\n") && !bytes.ends_with(b"\n\n"));
            let artifact: Value = serde_json::from_slice(bytes).expect("artifact JSON");
            assert_eq!(artifact["schema_version"], "fsl-causal-evidence.v0");
            assert_eq!(artifact["evidence_id"], *id);
            assert_eq!(artifact["support"], "inconclusive");
            assert_eq!(artifact["design"], "observational");
            assert_eq!(artifact["formal_result"], "not_run");
            assert_eq!(artifact["observation"]["assurance"], "replay-observed");
            assert_eq!(
                artifact["artifact_digest"],
                reference.envelope["artifact_digests"][index]
            );
            let bytes = &reference.lifecycle[index];
            assert!(bytes.ends_with(b"\n") && !bytes.ends_with(b"\n\n"));
            let chain: Value = serde_json::from_slice(bytes).expect("lifecycle JSON");
            assert_eq!(chain["schema_version"], "fsl-causal-evidence-lifecycle.v0");
            assert_eq!(chain["evidence_id"], *id);
            assert_eq!(chain["artifact_digest"], artifact["artifact_digest"]);
            let records = chain["records"].as_array().expect("records");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0]["sequence"], 1);
            assert_eq!(records[0]["status"], "active");
            assert_eq!(records[0]["recorded_at"], "2026-03-31T00:00:00Z");
        }
    }
}

fn checked(output: &Output, exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}
fn same(output: &Output, exit: i32, expected: &Value) {
    assert_eq!(
        checked(output, exit),
        *expected,
        "complete envelope without exclusions"
    );
}
fn collision(first: &Path, second: &Path) -> Value {
    json!({"fsl":"1.0", "result":"error", "kind":"usage",
        "message":format!("causal output destinations collide: {} and {}", first.display(), second.display())})
}

fn assert_output_bytes(fixture: &Fixture, base: &Path, expected: &[Vec<u8>]) {
    let actual: Vec<Vec<u8>> = fixture
        .paths(base)
        .iter()
        .map(|path| std::fs::read(fixture.root.join(path)).expect("actual output bytes"))
        .collect();
    assert_eq!(actual.as_slice(), expected);
}

#[test]
fn co1_distinct_single_and_multi_outputs_keep_full_envelopes_names_bytes_and_assurance() {
    for linked in [1, 2] {
        let fixture = Fixture::new(linked);
        let reference = fixture.reference();
        std::fs::create_dir(fixture.root.join("outputs")).expect("output directory");
        let out = Path::new("outputs/evidence.json");
        let lifecycle = Path::new("outputs/lifecycle.json");
        let mut expected = inventory(&fixture.root);
        same(&fixture.observe(None, None), 0, &reference.envelope);
        assert_eq!(inventory(&fixture.root), expected);
        same(&fixture.observe(Some(out), None), 0, &reference.envelope);
        for (path, bytes) in fixture.paths(out).into_iter().zip(&reference.evidence) {
            expected.insert(path, Node::File(bytes.clone()));
        }
        assert_eq!(inventory(&fixture.root), expected);
        assert_output_bytes(&fixture, out, &reference.evidence);
        same(
            &fixture.observe(None, Some(lifecycle)),
            0,
            &reference.envelope,
        );
        for (path, bytes) in fixture
            .paths(lifecycle)
            .into_iter()
            .zip(&reference.lifecycle)
        {
            expected.insert(path, Node::File(bytes.clone()));
        }
        assert_eq!(inventory(&fixture.root), expected);
        assert_output_bytes(&fixture, lifecycle, &reference.lifecycle);
        same(
            &fixture.observe(Some(out), Some(lifecycle)),
            0,
            &reference.envelope,
        );
        assert_eq!(inventory(&fixture.root), expected);
    }
}

#[test]
fn co1_single_distinct_bases_without_and_with_extension_remain_distinct_destinations() {
    let fixture = Fixture::new(1);
    let reference = fixture.reference();
    let mut expected = inventory(&fixture.root);
    let out = Path::new("output");
    let lifecycle = Path::new("output.json");
    same(
        &fixture.observe(Some(out), Some(lifecycle)),
        0,
        &reference.envelope,
    );
    expected.insert(out.to_path_buf(), Node::File(reference.evidence[0].clone()));
    expected.insert(
        lifecycle.to_path_buf(),
        Node::File(reference.lifecycle[0].clone()),
    );
    assert_eq!(inventory(&fixture.root), expected);
}

#[test]
fn co2_zero_artifacts_do_not_collide_even_with_equal_output_flags() {
    let fixture = Fixture::new(0);
    let baseline = checked(&fixture.observe(None, None), 0);
    fixture.assert_envelope(&baseline);
    assert!(baseline.get("artifact_digests").is_none());
    std::fs::write(
        fixture.root.join("untouched.json"),
        b"zero-artifact sentinel\n",
    )
    .expect("sentinel");
    let before = inventory(&fixture.root);
    let path = Path::new("untouched.json");
    same(&fixture.observe(Some(path), Some(path)), 0, &baseline);
    assert_eq!(inventory(&fixture.root), before);
}

fn collision_case(linked: usize, out: &Path, lifecycle: &Path) {
    let fixture = Fixture::new(linked);
    let positive = checked(&fixture.observe(None, None), 0);
    fixture.assert_envelope(&positive);
    fixture.seed(out, lifecycle);
    let before = inventory(&fixture.root);
    let expected = collision(&fixture.paths(out)[0], &fixture.paths(lifecycle)[0]);
    same(&fixture.observe(Some(out), Some(lifecycle)), 2, &expected);
    assert_eq!(inventory(&fixture.root), before);
}

#[test]
fn co1_single_equal_destination_refuses_before_overwriting_any_sentinel() {
    collision_case(
        1,
        Path::new("outputs/shared.json"),
        Path::new("outputs/shared.json"),
    );
}
#[test]
fn co1_multi_equal_destinations_refuse_before_overwriting_any_sentinel() {
    collision_case(
        2,
        Path::new("outputs/shared.json"),
        Path::new("outputs/shared.json"),
    );
}
#[test]
fn co1_multi_distinct_bases_with_the_same_computed_default_extension_refuse() {
    collision_case(
        2,
        Path::new("outputs/shared"),
        Path::new("outputs/shared.json"),
    );
}

#[test]
fn co2_dot_components_and_relative_absolute_spelling_share_collision_keys() {
    for linked in [1, 2] {
        collision_case(
            linked,
            Path::new("outputs/./shared.json"),
            Path::new("outputs/shared.json"),
        );
        let fixture = Fixture::new(linked);
        let positive = checked(&fixture.observe(None, None), 0);
        fixture.assert_envelope(&positive);
        let relative = Path::new("outputs/shared.json");
        let absolute = fixture.root.join(relative);
        fixture.seed(relative, &absolute);
        let before = inventory(&fixture.root);
        let expected = collision(&fixture.paths(relative)[0], &fixture.paths(&absolute)[0]);
        same(
            &fixture.observe(Some(relative), Some(&absolute)),
            2,
            &expected,
        );
        assert_eq!(inventory(&fixture.root), before);
    }
}

#[test]
fn co2_a_parent_spelled_through_dotdot_shares_the_collision_key() {
    for linked in [1, 2] {
        let fixture = Fixture::new(linked);
        std::fs::create_dir_all(fixture.root.join("outputs/sub")).expect("existing parent");
        let out = Path::new("outputs/a.json");
        let lifecycle = Path::new("outputs/sub/../a.json");
        fixture.seed(out, lifecycle);
        let before = inventory(&fixture.root);
        let expected = collision(&fixture.paths(out)[0], &fixture.paths(lifecycle)[0]);
        same(&fixture.observe(Some(out), Some(lifecycle)), 2, &expected);
        assert_eq!(inventory(&fixture.root), before);
    }
}

#[test]
fn co2_a_parent_that_does_not_exist_compares_by_its_spelling() {
    let fixture = Fixture::new(1);
    let out = Path::new("a.json");
    let lifecycle = Path::new("missing/../a.json");
    let expected = collision(out, lifecycle);
    let before = inventory(&fixture.root);
    same(&fixture.observe(Some(out), Some(lifecycle)), 2, &expected);
    assert_eq!(inventory(&fixture.root), before);
}

#[cfg(unix)]
#[test]
fn co2_a_parent_linked_to_the_same_directory_shares_the_collision_key() {
    let fixture = Fixture::new(1);
    std::os::unix::fs::symlink(".", fixture.root.join("self")).expect("controlled owned link");
    let out = Path::new("a.json");
    let lifecycle = Path::new("self/a.json");
    let before = inventory(&fixture.root);
    let expected = collision(out, lifecycle);
    same(&fixture.observe(Some(out), Some(lifecycle)), 2, &expected);
    assert_eq!(inventory(&fixture.root), before);
}

#[cfg(unix)]
#[test]
fn co2_dotdot_is_retained_and_a_controlled_symlink_can_reach_distinct_destinations() {
    let fixture = Fixture::new(1);
    let reference = fixture.reference();
    std::fs::create_dir_all(fixture.root.join("real/deep")).expect("controlled target directories");
    std::os::unix::fs::symlink("real/deep", fixture.root.join("link"))
        .expect("controlled owned link");
    let mut expected = inventory(&fixture.root);
    same(
        &fixture.observe(
            Some(Path::new("link/../out.json")),
            Some(Path::new("out.json")),
        ),
        0,
        &reference.envelope,
    );
    expected.insert(
        PathBuf::from("real/out.json"),
        Node::File(reference.evidence[0].clone()),
    );
    expected.insert(
        PathBuf::from("out.json"),
        Node::File(reference.lifecycle[0].clone()),
    );
    assert_eq!(inventory(&fixture.root), expected);
}

fn early_fault(fixture: &Fixture, file: &str, fault: &[u8], kind: &str) {
    let path = fixture.root.join(file);
    let bytes = std::fs::read(&path).expect("original input");
    std::fs::write(&path, fault).expect("explicit input fault");
    let before = inventory(&fixture.root);
    let baseline = checked(
        &fixture.observe(
            Some(Path::new("outputs/distinct-evidence.json")),
            Some(Path::new("outputs/distinct-lifecycle.json")),
        ),
        2,
    );
    assert_eq!(baseline["result"], "error");
    assert_eq!(baseline["kind"], kind);
    if kind == "observation_replay_nonconformant" {
        assert_eq!(baseline["failed_at_record"], 0);
        assert_eq!(
            baseline["expected_state"],
            json!({"guardrails": 1, "alert_precision": 3, "mttr_hours": 24})
        );
        assert_eq!(
            baseline["observed_state"],
            json!({"guardrails": 0, "alert_precision": 3, "mttr_hours": 24})
        );
        assert_eq!(
            baseline["mismatches"],
            json!([{"path": "guardrails", "expected": 1, "observed": 0}])
        );
        assert!(
            baseline["message"]
                .as_str()
                .expect("replay message")
                .contains("state mismatch")
        );
    }
    same(
        &fixture.observe(
            Some(Path::new("outputs/shared.json")),
            Some(Path::new("outputs/shared.json")),
        ),
        2,
        &baseline,
    );
    assert_eq!(inventory(&fixture.root), before);
    std::fs::write(path, bytes).expect("restore exact input bytes");
    fixture.assert_envelope(&checked(&fixture.observe(None, None), 0));
}

#[test]
fn co3_existing_model_mapping_log_scope_period_and_replay_errors_beat_collision() {
    let fixture = Fixture::new(2);
    fixture.assert_envelope(&checked(&fixture.observe(None, None), 0));
    fixture.seed(
        Path::new("outputs/shared.json"),
        Path::new("outputs/shared.json"),
    );
    let before = inventory(&fixture.root);
    for (file, fault, kind) in [
        ("inputs/model.fsl", b"not a causal model".as_slice(), "parse"),
        ("inputs/mapping.fsl", b"not a refinement mapping".as_slice(), "parse"),
        ("inputs/log.jsonl", b"not JSON".as_slice(), "io"),
        ("inputs/scope.json", b"not JSON".as_slice(), "io"),
        ("inputs/log.jsonl", br#"{"action":"deploy_guardrails","params":{},"state":{"guardrails":0,"alert_precision":3,"mttr_hours":24}}"#.as_slice(), "observation_replay_nonconformant"),
    ] { early_fault(&fixture, file, fault, kind); }
    let distinct = checked(
        &fixture.process(
            Some(Path::new("outputs/e.json")),
            Some(Path::new("outputs/l.json")),
            false,
        ),
        2,
    );
    assert_eq!(distinct["kind"], "usage");
    assert!(
        distinct["message"]
            .as_str()
            .expect("missing-period message")
            .contains("--period-start")
    );
    same(
        &fixture.process(
            Some(Path::new("outputs/shared.json")),
            Some(Path::new("outputs/shared.json")),
            false,
        ),
        2,
        &distinct,
    );
    assert_eq!(inventory(&fixture.root), before);
}

#[test]
fn co3_valid_collision_beats_blocked_parent_publication_io_without_any_writes() {
    for linked in [1, 2] {
        let fixture = Fixture::new(linked);
        fixture.assert_envelope(&checked(&fixture.observe(None, None), 0));
        std::fs::write(
            fixture.root.join("blocked"),
            b"regular file, never a directory\n",
        )
        .expect("blocked parent");
        let before = inventory(&fixture.root);
        let path = Path::new("blocked/out.json");
        let expected = collision(&fixture.paths(path)[0], &fixture.paths(path)[0]);
        same(&fixture.observe(Some(path), Some(path)), 2, &expected);
        assert_eq!(inventory(&fixture.root), before);
    }
}

#[test]
fn co3_distinct_publication_failure_keeps_interleaving_and_does_not_roll_back_prior_evidence() {
    for linked in [1, 2] {
        let fixture = Fixture::new(linked);
        let reference = fixture.reference();
        let out = Path::new("outputs/evidence.json");
        let lifecycle = Path::new("outputs/lifecycle.json");
        fixture.seed(out, lifecycle);
        let first_evidence = fixture.paths(out)[0].clone();
        let first_lifecycle = fixture.paths(lifecycle)[0].clone();
        let failed = fixture.root.join(&first_lifecycle);
        std::fs::remove_file(&failed).expect("replace first lifecycle sentinel with a directory");
        std::fs::create_dir(&failed).expect("deterministic lifecycle publication failure");
        std::fs::write(failed.join("keep.txt"), b"preserved child sentinel\n")
            .expect("child sentinel");
        let mut expected_tree = inventory(&fixture.root);
        // Publication refuses a non-regular target itself, on every platform.
        let expected = json!({"fsl":"1.0", "result":"error", "kind":"io",
            "message":format!("cannot write {}: output target is not a regular file", first_lifecycle.display()),
            "written":[first_evidence.display().to_string()]});
        same(&fixture.observe(Some(out), Some(lifecycle)), 2, &expected);
        expected_tree.insert(first_evidence, Node::File(reference.evidence[0].clone()));
        assert_eq!(
            inventory(&fixture.root),
            expected_tree,
            "only the earlier first evidence is published; later files and sentinels remain untouched"
        );
    }
}
