// SPDX-License-Identifier: Apache-2.0

//! Issue #1548: reports must not promote failed replay, bounded liveness,
//! or an unevaluated reachability obligation into a safe/proved claim.
//! All verification/replay evidence below comes from the native CLI, not a
//! hand-written approximation of its envelope. This target owns the CLI binary;
//! fsl-tools deliberately has no verifier dependency.

#[path = "support/ledger_findings.rs"]
#[allow(dead_code)]
mod findings;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

const MIXED: &str = r#"spec Mixed {
  type N = 0..3
  state { x: N, req: Bool }
  init { x = 0  req = false }
  fair action ask() { requires req == false  req = true }
  fair action serve() { requires req == true  req = false }
  @requirement("R-PROOF")
  invariant Bounded { x <= 3 }
  leadsTo Served { req == true ~> req == false }
}"#;

const ENSURES_FAILURE: &str = r#"spec Ens {
  type N = 0..3
  state { x: N }
  init { x = 0 }
  @requirement("R-INC")
  action inc() { requires x < 3  x = x + 1  ensures x == old(x) + 2 }
  @requirement("R-TOP")
  reachable Top { x == 3 }
}"#;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
}

fn existing_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn text(path: &Path) -> &str {
    // Document projection requires a portable source identity. All CLI paths
    // are repository-relative, while filesystem reads retain absolute paths.
    path.strip_prefix(root())
        .unwrap_or(path)
        .to_str()
        .expect("UTF-8 test path")
}

struct Fixture {
    directory: PathBuf,
    spec: PathBuf,
    depth: u8,
}

impl Fixture {
    fn new(label: &str, source: &str) -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("issue-1548-{label}-{}-{id}", std::process::id()));
        fs::create_dir_all(&directory).expect("create repository-local scratch directory");
        let spec = directory.join("spec.fsl");
        fs::write(&spec, source).expect("write test spec");
        Self {
            directory,
            spec,
            depth: 8,
        }
    }

    fn verify(&self, engine: &str, expected_exit: i32) -> Value {
        let depth = self.depth.to_string();
        json_output(
            &run(&[
                "verify",
                text(&self.spec),
                "--engine",
                engine,
                "--depth",
                &depth,
                "--deadlock",
                "ignore",
                "--no-cache",
            ]),
            expected_exit,
        )
    }

    fn ledger(&self, label: &str, extra: &[&str], expected_exit: i32) -> String {
        self.report("ledger", label, extra, expected_exit)
    }

    fn scenarios(&self, expected_exit: i32) -> Value {
        let depth = self.depth.to_string();
        let value = json_output(
            &run(&[
                "scenarios",
                text(&self.spec),
                "--depth",
                &depth,
                "--deadlock",
                "ignore",
            ]),
            expected_exit,
        );
        if expected_exit == 0 {
            assert_eq!(value["result"], "scenarios", "{value}");
        }
        value
    }

    fn report(&self, command: &str, label: &str, extra: &[&str], expected_exit: i32) -> String {
        let output_path = self.directory.join(label);
        let depth = self.depth.to_string();
        let mut args = vec![
            command,
            text(&self.spec),
            "--depth",
            &depth,
            "--deadlock",
            "ignore",
        ];
        args.extend_from_slice(extra);
        args.extend_from_slice(&["-o", text(&output_path)]);
        let envelope = json_output(&run(&args), expected_exit);
        assert_eq!(envelope["result"], "generated", "{envelope}");
        fs::read_to_string(output_path).expect("read generated report")
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run native fslc")
}

fn json_output(output: &Output, expected_exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("native CLI JSON envelope")
}

fn replay_fixture() -> Fixture {
    let source = fs::read_to_string(existing_fixture("replay_trace.fsl")).unwrap();
    // The issue removes only the intentionally partial action from a temporary
    // copy: otherwise verification itself fails and obscures the replay finding.
    let partial = "  action partial(i: Id) {\n    requires 1 / i > 0\n  }\n";
    assert_eq!(source.matches(partial).count(), 1);
    Fixture::new("replay", &source.replacen(partial, "", 1))
}

fn failed_replay_evidence(fixture: &Fixture) -> PathBuf {
    let trace = existing_fixture("replay_trace.state-mismatch.v1.json");
    let output = run(&["replay", text(&fixture.spec), "--trace", text(&trace)]);
    let replay = json_output(&output, 1);
    assert_eq!(replay["result"], "nonconformant", "{replay}");
    assert_eq!(replay["failed_at_event"], 0, "{replay}");
    let evidence = fixture.directory.join("replay.json");
    fs::write(&evidence, output.stdout).expect("save complete real replay envelope");
    evidence
}

#[test]
fn nonconformant_impl_log_is_a_red_spec_level_conformance_finding() {
    let fixture = replay_fixture();
    assert_eq!(fixture.verify("bmc", 0)["result"], "verified");
    let evidence = failed_replay_evidence(&fixture);
    let external = fixture.ledger("external.md", &["--evidence", text(&evidence)], 0);
    let external_row = findings::risk_row(&external, "（仕様全体）");
    assert!(external_row.contains("| 🔴 要確認 |"), "{external_row}");
    assert!(
        external_row.contains("| external_evidence |"),
        "{external_row}"
    );

    let trace = existing_fixture("replay_trace.state-mismatch.v1.json");
    let body = fixture.ledger("impl.md", &["--impl-log", text(&trace)], 0);
    assert!(body.contains("実装ログ適合: **非適合**（イベント 0 で乖離）"));
    let row = findings::risk_row(&body, "（仕様全体）");
    // Only severity is equated: these are intentionally different trace types,
    // and the impl-log header/detail is not an external-evidence artifact.
    assert_eq!(row.split('|').nth(3), external_row.split('|').nth(3));
    assert!(row.contains("| conformance |"), "{row}");
    let detail = findings::assert_issue(&body, "（仕様全体）", &["conformance"]);
    assert!(
        detail.contains("実装ログが仕様に非適合。実装か仕様のどちらが正かを確定する。"),
        "{detail}"
    );
    assert!(detail.contains("実装 or 仕様を一致させる"), "{detail}");
}

#[test]
fn conformant_impl_log_adds_no_finding() {
    let fixture = replay_fixture();
    assert_eq!(fixture.verify("bmc", 0)["result"], "verified");
    let trace = existing_fixture("replay_trace.valid.v1.json");
    let replay = json_output(
        &run(&["replay", text(&fixture.spec), "--trace", text(&trace)]),
        0,
    );
    assert_eq!(replay["result"], "conformant", "{replay}");
    let body = fixture.ledger("conformant.md", &["--impl-log", text(&trace)], 0);
    assert!(body.contains("実装ログ適合: 適合"), "{body}");
    findings::assert_no_findings(&body);
}

/// Assert the four aggregate surfaces together, after checking real producer
/// completeness. The document copy adds only attachment metadata, never changes
/// any verification field. A real failed replay supplies the ledger spec row;
/// it is independent of the proof's own verdict and assurance.
fn assert_four_surfaces(fixture: &Fixture, leads_to: &str, ranked: bool) {
    let mut verification = fixture.verify("induction", 0);
    assert_eq!(verification["result"], "proved", "{verification}");
    assert_eq!(verification["completeness"], "unbounded", "{verification}");
    if ranked {
        assert_eq!(
            verification["leads_to"][leads_to]["completeness"], "unbounded",
            "{verification}"
        );
    } else {
        assert_ne!(
            verification["leads_to"][leads_to]["completeness"], "unbounded",
            "{verification}"
        );
        assert!(verification["leads_to"].is_object(), "{verification}");
        assert!(
            verification["leads_to"][leads_to].is_object(),
            "{verification}"
        );
    }
    let depth = verification["checked_to_depth"]
        .as_u64()
        .expect("checked depth");
    let expected_label = if ranked {
        "proved(induction)".to_owned()
    } else {
        format!("bounded(BMC depth {depth})")
    };
    let expected_header = if ranked {
        "- 保証限界: k帰納法で **全実行を証明済み**（深さ無制限）".to_owned()
    } else {
        format!(
            "- 保証限界: BMC（有界モデル検査）: **深さ {depth} までの全実行を網羅**。それ以遠の反例は本台帳の対象外"
        )
    };

    let replay = replay_fixture();
    let failed = failed_replay_evidence(&replay);
    let ledger = fixture.ledger(
        "proof.md",
        &["--engine", "induction", "--evidence", text(&failed)],
        0,
    );
    let header = ledger
        .lines()
        .find(|line| line.starts_with("- 保証限界:"))
        .expect("guarantee header");
    let row = findings::risk_row(&ledger, "（仕様全体）");
    let row_label = row.split('|').nth(4).expect("spec-level assurance").trim();
    let html = fixture.report("html", "proof.html", &["--engine", "induction"], 0);
    let panel_label = html
        .split_once("<th>Assurance</th><td>")
        .expect("HTML status Assurance field")
        .1
        .split_once("</td>")
        .unwrap()
        .0;

    verification["requirements"] = json!(["R-PROOF"]);
    let evidence = fixture.directory.join("verification.json");
    fs::write(&evidence, serde_json::to_vec(&verification).unwrap()).unwrap();
    let document_path = fixture.directory.join("document.md");
    let generated = json_output(
        &run(&[
            "document",
            "generate",
            text(&fixture.spec),
            "--lang",
            "en",
            "--evidence",
            text(&evidence),
            "-o",
            text(&document_path),
        ]),
        0,
    );
    assert_eq!(generated["result"], "generated", "{generated}");
    let document = fs::read_to_string(document_path).unwrap();
    let overlay = document
        .lines()
        .find(|line| line.starts_with("- Formal verification:"))
        .expect("document-evidence overlay");
    let overlay_label = overlay.split('`').nth(1).expect("document formal label");

    // Do not fail at the header alone: show all four observations in the same
    // detector so a fix to only one renderer cannot hide the other overclaims.
    let agreements = [
        header == expected_header,
        panel_label == expected_label,
        row_label == expected_label,
        overlay_label == expected_label,
    ];
    assert_eq!(
        agreements, [true; 4],
        "expected header={expected_header:?}, label={expected_label:?}; observed markdown={header:?}, HTML={panel_label:?}, spec row={row_label:?}, document={overlay_label:?}"
    );
}

#[test]
fn unranked_leadsto_makes_all_four_aggregate_surfaces_plain_bounded() {
    // The issue's Mixed spec, with requirement metadata only for the overlay.
    assert_four_surfaces(&Fixture::new("mixed", MIXED), "Served", false);
}

#[test]
fn every_leadsto_ranked_keeps_all_four_aggregate_surfaces_proved() {
    let source =
        fs::read_to_string(root().join("tests/fixtures/rust_port/ranked_leadsto.fsl")).unwrap();
    let source = source.replace(
        "  leadsTo ReachFive",
        "  @requirement(\"R-PROOF\")\n  leadsTo ReachFive",
    );
    assert_four_surfaces(&Fixture::new("ranked", &source), "ReachFive", true);
}

fn assert_reachables_not_evaluated(verification: &Value) {
    assert!(!verification["reachables"].is_object(), "{verification}");
    assert!(!verification["unreached"].is_array(), "{verification}");
}

fn assert_white_not_run(body: &str) {
    let row = findings::risk_row(body, "R-TOP");
    assert!(
        row.contains("| ⚪ 未評価 | not_run |"),
        "unevaluated reachable must be white/not_run, observed: {row}"
    );
    assert!(!row.contains("🟢"), "{row}");
}

fn confirms_top(scenario: &Value) -> bool {
    scenario["requirement"]["id"] == "R-TOP"
        || (scenario["kind"] == "reachable" && scenario["property"] == "Top")
}

#[test]
fn reachable_skipped_after_ensures_violation_is_not_evaluated() {
    let fixture = Fixture::new("ensures", ENSURES_FAILURE);
    let verification = fixture.verify("bmc", 1);
    assert_eq!(verification["result"], "violated", "{verification}");
    assert_eq!(verification["violation_kind"], "ensures", "{verification}");
    assert_eq!(verification["checked_to_depth"], 1, "{verification}");
    assert_reachables_not_evaluated(&verification);
    let scenarios = fixture.scenarios(1);
    assert_eq!(scenarios["result"], "violated", "{scenarios}");
    assert!(
        !scenarios["scenarios"]
            .as_array()
            .into_iter()
            .flatten()
            .any(confirms_top),
        "{scenarios}"
    );
    assert_white_not_run(&fixture.ledger("ensures.md", &[], 1));
}

fn cti_fixture() -> Fixture {
    let source =
        fs::read_to_string(root().join("tests/fixtures/rust_port/induction_unknown_cti.fsl"))
            .unwrap();
    let (prefix, suffix) = source.rsplit_once('}').unwrap();
    let source =
        format!("{prefix}\n  @requirement(\"R-TOP\")\n  reachable Top {{ x == 2 }}\n}}{suffix}");
    let mut fixture = Fixture::new("cti", &source);
    fixture.depth = 3;
    fixture
}

#[test]
fn scenario_confirmed_reachable_under_unknown_cti_keeps_its_current_row() {
    let fixture = cti_fixture();
    let verification = fixture.verify("induction", 1);
    assert_eq!(verification["result"], "unknown_cti", "{verification}");
    assert_reachables_not_evaluated(&verification);
    let scenarios = fixture.scenarios(0);
    assert!(
        scenarios["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .any(confirms_top),
        "{scenarios}"
    );
    let body = fixture.ledger("cti-confirmed.md", &["--engine", "induction"], 1);
    assert_eq!(
        findings::risk_row(&body, "R-TOP"),
        "| R-TOP | — | 🟢 確認済（承認可） | bounded(BMC depth 3) | — | — | — | — |"
    );
}

#[test]
fn unconfirmed_reachable_under_unknown_cti_is_intentionally_not_evaluated() {
    // Approved condition (b), with the clarified scenario-confirmation priority:
    // unknown_cti + no reachable evaluation + no scenario witness is intended
    // to show white/not_run. Real CLI attempts with Top at x == 4 (depth 3),
    // or a ranked leadsTo with unreachable Top, returned reachable_failed
    // before induction instead. A reachable at x == 2 permits unknown_cti,
    // but scenarios confirms it. Thus derive ONLY this absence-of-confirmation
    // case by removing that one Top entry from the real scenarios output.
    // The real unknown_cti verification envelope is passed through unchanged;
    // no verification/scenario JSON is invented. The separate CLI control
    // above pins the priority of the original, real scenario witness.
    let fixture = cti_fixture();
    let verification = fixture.verify("induction", 1);
    assert_eq!(verification["result"], "unknown_cti", "{verification}");
    assert_reachables_not_evaluated(&verification);
    let original = fixture.scenarios(0);
    let mut derived = original.clone();
    let scenarios = derived["scenarios"].as_array_mut().unwrap();
    let matching = scenarios
        .iter()
        .enumerate()
        .filter_map(|(index, scenario)| confirms_top(scenario).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "exactly one Top witness: {original}");
    let index = matching[0];
    let witness = scenarios.remove(index);
    assert_eq!(witness["kind"], "reachable", "{witness}");
    assert_eq!(witness["property"], "Top", "{witness}");
    assert!(!scenarios.iter().any(confirms_top));
    // Reinsertion must recover the complete producer output, not a selected
    // list of fields: only the one documented scenario entry was removed.
    let mut restored = derived.clone();
    restored["scenarios"]
        .as_array_mut()
        .unwrap()
        .insert(index, witness);
    assert_eq!(restored, original);
    let source = fs::read_to_string(&fixture.spec).unwrap();
    let resolver = fsl_core::FsResolver::new(root());
    let model =
        fsl_core::build_model(fsl_core::parse_kernel_source(&source, &resolver).unwrap()).unwrap();
    let body = fsl_tools::render_ledger(
        text(&fixture.spec),
        &model,
        &verification,
        &derived,
        None,
        &[],
    );
    assert_white_not_run(&body);
}

fn evaluated_reachable(target: u8) -> Fixture {
    Fixture::new(
        "evaluated",
        &format!(
            r#"spec Evaluated {{
      state {{ x: 0..3 }}
      init {{ x = 0 }}
      action inc() {{ requires x < 2  x = x + 1 }}
      @requirement("R-TOP")
      reachable Top {{ x == {target} }}
    }}"#
        ),
    )
}

#[test]
fn evaluated_witnessed_reachable_keeps_its_confirmed_bounded_row() {
    let fixture = evaluated_reachable(2);
    let verification = fixture.verify("bmc", 0);
    assert!(verification["reachables"].is_object(), "{verification}");
    let body = fixture.ledger("witness.md", &[], 0);
    let row = findings::risk_row(&body, "R-TOP");
    assert_eq!(
        row,
        "| R-TOP | — | 🟢 確認済（承認可） | bounded(BMC depth 8) | — | — | — | — |"
    );
}

#[test]
fn evaluated_unwitnessed_reachable_keeps_its_red_bounded_row() {
    let fixture = evaluated_reachable(3);
    let verification = fixture.verify("bmc", 1);
    assert_eq!(verification["result"], "reachable_failed", "{verification}");
    assert!(
        verification["reachables"].is_object() || verification["unreached"].is_array(),
        "{verification}"
    );
    let body = fixture.ledger("unwitnessed.md", &[], 1);
    let row = findings::risk_row(&body, "R-TOP");
    assert_eq!(
        row,
        "| R-TOP | — | 🔴 要確認 | bounded(BMC depth 8) | reachable | 要確認 | ____ | not witnessed within depth 8; try a larger --depth |"
    );
}

#[test]
fn untagged_skipped_reachable_adds_no_requirement_row_or_finding() {
    let source = ENSURES_FAILURE.replace("  @requirement(\"R-TOP\")\n", "");
    let fixture = Fixture::new("untagged", &source);
    let verification = fixture.verify("bmc", 1);
    assert_eq!(verification["result"], "violated", "{verification}");
    assert_reachables_not_evaluated(&verification);
    let body = fixture.ledger("untagged.md", &[], 1);
    let rows = findings::section(&body, "リスク一覧（要件ID別）");
    assert_eq!(
        rows.lines().filter(|line| line.starts_with("| ")).count(),
        2,
        "only the header and R-INC row: {rows}"
    );
    assert!(!rows.contains("⚪ 未評価"), "{rows}");
    let raw = findings::raw_findings(&body);
    let raw = raw.as_array().unwrap();
    assert_eq!(raw.len(), 1, "only the existing ensures finding: {raw:?}");
    assert_eq!(raw[0]["trace_type"], "ensures");
}
