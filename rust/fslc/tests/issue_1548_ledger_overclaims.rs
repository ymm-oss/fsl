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
        self.verify_with_deadlock(engine, "ignore", expected_exit)
    }

    fn verify_with_deadlock(&self, engine: &str, deadlock: &str, expected_exit: i32) -> Value {
        let depth = self.depth.to_string();
        let mut args = vec![
            "verify",
            text(&self.spec),
            "--depth",
            &depth,
            "--deadlock",
            deadlock,
            "--no-cache",
        ];
        // Pin the default BMC path, not merely an explicit --engine bmc alias.
        if engine != "bmc" {
            args.extend_from_slice(&["--engine", engine]);
        }
        json_output(&run(&args), expected_exit)
    }

    fn ledger(&self, label: &str, extra: &[&str], expected_exit: i32) -> String {
        self.report("ledger", label, extra, expected_exit)
    }

    fn scenarios(&self, expected_exit: i32) -> Value {
        self.scenarios_with_deadlock("ignore", expected_exit)
    }

    fn scenarios_with_deadlock(&self, deadlock: &str, expected_exit: i32) -> Value {
        let depth = self.depth.to_string();
        let value = json_output(
            &run(&[
                "scenarios",
                text(&self.spec),
                "--depth",
                &depth,
                "--deadlock",
                deadlock,
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
        let mut args = vec![command, text(&self.spec), "--depth", &depth];
        if !extra.contains(&"--deadlock") {
            args.extend_from_slice(&["--deadlock", "ignore"]);
        }
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

fn assert_reachable_evaluation_fields_absent(verification: &Value) {
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
    assert_reachable_evaluation_fields_absent(&verification);
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
    assert_reachable_evaluation_fields_absent(&verification);
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
fn unconfirmed_reachable_under_unknown_cti_keeps_its_evaluated_bounded_row() {
    // Round 2 drops the former condition (b): missing envelope fields do not
    // mean reachability was skipped. Keep the same derived input as round 1:
    // remove only Top's scenario from real producer output, then pass through
    // the real unknown_cti envelope unchanged. The base BMC had to verify
    // before this invariant CTI, so bounded evidence survives even without
    // the separate scenario confirmation. This is a renderer control, not a
    // claim that the current CLI naturally loses this scenario witness.
    let fixture = cti_fixture();
    let verification = fixture.verify("induction", 1);
    assert_eq!(verification["result"], "unknown_cti", "{verification}");
    assert_reachable_evaluation_fields_absent(&verification);
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
    assert_eq!(
        findings::risk_row(&body, "R-TOP"),
        "| R-TOP | — | 🟢 反例なし | bounded(BMC depth 3) | — | — | — | — |"
    );
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
    assert_reachable_evaluation_fields_absent(&verification);
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

// Round 2: observed producer/projection order, pinned by real CLI runs below.
//
// Engine   | result / kind                       | reachables / unreached | R-TOP
// BMC/BFS  | violated: invariant/trans/ensures/   | absent / absent        | not_run
//          | partial_op/type_bound               |                        |
// BMC      | violated: deadlock or leadsTo       | absent / absent        | bounded
// BFS      | violated: deadlock (either witness) | absent / absent        | not_run*
// BMC/BFS  | reachable_failed                    | absent / array         | red/bounded
// BMC      | unwitnessed + deadlock or leadsTo   | reachable_failed wins  | red/bounded
// BFS      | unwitnessed + deadlock              | deadlock wins          | not_run*
// BFS      | any leadsTo                        | semantics refusal      | no artifact
// induction| unknown_cti / invariant or rank     | absent / absent        | bounded
// * A real scenario confirmation still wins. The deadlock cases use
//   --deadlock error, so scenarios itself returns a failure, not a witness.
//
// Evidence: fsl-verifier/src/bmc.rs:664-701 checks state/action violations
// before record_reachables, then records deadlock and liveness at :705-793.
// fsl-runtime/src/explicit.rs:203-297 checks current violations, records
// reachables, then steps actions; :100-109 refuses leadsTo before BFS.
// fslc/src/verification_output.rs:2339-2372 projects BMC early violation >
// unreached > deadlock > leadsTo; :2398-2477 projects explicit early violation
// > deadlock > budget > unreached. The latter adds engine:"explicit" at :2657.
// fslc/src/verification.rs:271-291 returns a non-verified base before ordinary
// induction; :574-676/:679-740 render invariant CTI/rank failure without the
// base reachables. Explicit dispatch is :967-1010. Ledger main.rs:13599-13607
// passes --engine to verify and :13680-13687 passes its full envelope through.
// The ledger/html CLI options currently accept only bmc/induction
// (cli_options.rs:724-725); explicit cases therefore feed real verify,
// scenarios and explain CLI outputs to the same library renderers instead.
// These are envelope-evidence classifications, not an assertion that no earlier
// BFS level ever visited a reachable on an early-violation run (#1565).

#[derive(Clone, Copy)]
enum ProducerCase {
    Invariant,
    Transition,
    Ensures,
    PartialOp,
    Bounds,
    Deadlock,
    UnwitnessedDeadlock,
    LeadsTo,
    UnwitnessedLeadsTo,
    ReachableFailed,
}

impl ProducerCase {
    fn source(self) -> String {
        let (action, property, target) = match self {
            Self::Invariant => ("action step() { x = 1 }", "invariant Safe { x == 0 }", 0),
            Self::Transition => ("action step() { x = 1 }", "trans Stable { x == old(x) }", 0),
            Self::Ensures => ("action step() { x = 1 ensures x == old(x) + 2 }", "", 0),
            Self::PartialOp => ("action step() { requires 1 / x > 0 x = 1 }", "", 0),
            Self::Bounds => ("action step() { x = 4 }", "", 0),
            Self::Deadlock => ("action step() { requires x == 0 x = 1 }", "", 1),
            Self::UnwitnessedDeadlock => ("action step() { requires x == 0 x = 1 }", "", 2),
            Self::LeadsTo => (
                "action step() { x = x }",
                "leadsTo Progress { x == 0 ~> x == 1 }",
                0,
            ),
            Self::UnwitnessedLeadsTo => (
                "action step() { x = x }",
                "leadsTo Progress { x == 0 ~> x == 1 }",
                1,
            ),
            Self::ReachableFailed => ("action step() { x = x }", "", 1),
        };
        format!(
            r#"spec ProducerOrder {{
          state {{ x: 0..3 }}
          init {{ x = 0 }}
          {action}
          {property}
          @requirement("R-TOP")
          reachable Top {{ x == {target} }}
        }}"#
        )
    }
}

fn html_property_assurance<'a>(html: &'a str, name: &str) -> &'a str {
    let properties = html
        .split_once("id=\"properties\"")
        .expect("HTML properties section")
        .1
        .split_once("</section>")
        .unwrap()
        .0;
    let row = properties
        .split("<tr>")
        .find(|row| row.contains(&format!("<code>{name}</code>")))
        .unwrap_or_else(|| panic!("missing property {name}: {properties}"));
    row.split("<td>")
        .nth(3)
        .expect("property assurance cell")
        .split_once("</td>")
        .unwrap()
        .0
}

fn row_assurance<'a>(ledger: &'a str, id: &str) -> &'a str {
    findings::risk_row(ledger, id)
        .split('|')
        .nth(4)
        .unwrap()
        .trim()
}

fn panel_assurance(html: &str) -> &str {
    html.split_once("<th>Assurance</th><td>")
        .expect("HTML status Assurance")
        .1
        .split_once("</td>")
        .unwrap()
        .0
}

fn library_reports(fixture: &Fixture, verification: &Value, scenarios: &Value) -> (String, String) {
    let source = fs::read_to_string(&fixture.spec).unwrap();
    let resolver = fsl_core::FsResolver::new(root());
    let model =
        fsl_core::build_model(fsl_core::parse_kernel_source(&source, &resolver).unwrap()).unwrap();
    let explained = json_output(&run(&["explain", text(&fixture.spec)]), 0);
    let ledger = fsl_tools::render_ledger(
        text(&fixture.spec),
        &model,
        verification,
        scenarios,
        None,
        &[],
    );
    let html = fsl_tools::render_html_report(
        text(&fixture.spec),
        &source,
        &model,
        &explained,
        verification,
        &[],
    );
    (ledger, html)
}

fn check_producer_case(
    engine: &str,
    case: ProducerCase,
    result: &str,
    kind: Option<&str>,
    unreached: bool,
    status: &str,
) {
    let fixture = Fixture::new("producer", &case.source());
    let verification = fixture.verify_with_deadlock(engine, "error", 1);
    assert_eq!(verification["result"], result, "{verification}");
    assert_eq!(
        verification["violation_kind"].as_str(),
        kind,
        "{verification}"
    );
    // A producer-order change must fail here even if presentation still happens
    // to show the expected status. No missing-field inference defines the rule.
    assert!(!verification["reachables"].is_object(), "{verification}");
    assert_eq!(
        verification["unreached"].is_array(),
        unreached,
        "{verification}"
    );
    if engine == "explicit" {
        assert_eq!(verification["engine"], "explicit", "{verification}");
    }
    let depth = verification["checked_to_depth"].as_u64().unwrap();
    let expected_label = if status == "⚪ 未評価" {
        "not_run".to_owned()
    } else {
        format!("bounded(BMC depth {depth})")
    };
    let extra = if engine == "bmc" {
        vec!["--deadlock", "error"]
    } else {
        vec!["--engine", engine, "--deadlock", "error"]
    };
    let (ledger, html) = if engine == "explicit" {
        let scenarios = fixture.scenarios_with_deadlock("error", 1);
        assert!(
            !scenarios["scenarios"]
                .as_array()
                .into_iter()
                .flatten()
                .any(confirms_top),
            "no scenario override in this result-kind matrix: {scenarios}"
        );
        library_reports(&fixture, &verification, &scenarios)
    } else {
        (
            fixture.ledger("matrix.md", &extra, 1),
            fixture.report("html", "matrix.html", &extra, 1),
        )
    };
    let row = findings::risk_row(&ledger, "R-TOP");
    let observed_status = row.split('|').nth(3).unwrap().trim();
    let html_label = html_property_assurance(&html, "Top");
    let agreements = [
        observed_status == status,
        row_assurance(&ledger, "R-TOP") == expected_label,
        html_label == expected_label,
    ];
    assert_eq!(
        agreements, [true; 3],
        "expected {status:?}/{expected_label:?}; observed ledger={row}, HTML property={html_label:?}; producer={result}/{kind:?}, engine={engine}"
    );
}

macro_rules! producer_case {
    ($name:ident, $engine:literal, $case:ident, $result:literal, $kind:expr, $unreached:literal, $status:literal) => {
        #[test]
        fn $name() {
            check_producer_case(
                $engine,
                ProducerCase::$case,
                $result,
                $kind,
                $unreached,
                $status,
            );
        }
    };
}

producer_case!(
    bmc_invariant_stops_reachable_evaluation,
    "bmc",
    Invariant,
    "violated",
    Some("invariant"),
    false,
    "⚪ 未評価"
);
producer_case!(
    explicit_invariant_stops_reachable_evaluation,
    "explicit",
    Invariant,
    "violated",
    Some("invariant"),
    false,
    "⚪ 未評価"
);
producer_case!(
    bmc_transition_stops_reachable_evaluation,
    "bmc",
    Transition,
    "violated",
    Some("trans"),
    false,
    "⚪ 未評価"
);
producer_case!(
    explicit_transition_stops_reachable_evaluation,
    "explicit",
    Transition,
    "violated",
    Some("trans"),
    false,
    "⚪ 未評価"
);
producer_case!(
    bmc_ensures_stops_reachable_evaluation,
    "bmc",
    Ensures,
    "violated",
    Some("ensures"),
    false,
    "⚪ 未評価"
);
producer_case!(
    explicit_ensures_stops_reachable_evaluation,
    "explicit",
    Ensures,
    "violated",
    Some("ensures"),
    false,
    "⚪ 未評価"
);
producer_case!(
    bmc_partial_op_stops_reachable_evaluation,
    "bmc",
    PartialOp,
    "violated",
    Some("partial_op"),
    false,
    "⚪ 未評価"
);
producer_case!(
    explicit_partial_op_stops_reachable_evaluation,
    "explicit",
    PartialOp,
    "violated",
    Some("partial_op"),
    false,
    "⚪ 未評価"
);
producer_case!(
    bmc_bounds_stops_reachable_evaluation,
    "bmc",
    Bounds,
    "violated",
    Some("type_bound"),
    false,
    "⚪ 未評価"
);
producer_case!(
    explicit_bounds_stops_reachable_evaluation,
    "explicit",
    Bounds,
    "violated",
    Some("type_bound"),
    false,
    "⚪ 未評価"
);
producer_case!(
    bmc_deadlock_implies_reachable_was_witnessed,
    "bmc",
    Deadlock,
    "violated",
    Some("deadlock"),
    false,
    "🟢 反例なし"
);
producer_case!(
    explicit_deadlock_does_not_report_reachable_witness_status,
    "explicit",
    Deadlock,
    "violated",
    Some("deadlock"),
    false,
    "⚪ 未評価"
);
producer_case!(
    bmc_unreached_precedes_deadlock,
    "bmc",
    UnwitnessedDeadlock,
    "reachable_failed",
    None,
    true,
    "🔴 要確認"
);
producer_case!(
    explicit_deadlock_precedes_unreached,
    "explicit",
    UnwitnessedDeadlock,
    "violated",
    Some("deadlock"),
    false,
    "⚪ 未評価"
);
producer_case!(
    bmc_leadsto_implies_reachable_was_witnessed,
    "bmc",
    LeadsTo,
    "violated",
    Some("leadsTo"),
    false,
    "🟢 反例なし"
);
producer_case!(
    bmc_unreached_precedes_leadsto,
    "bmc",
    UnwitnessedLeadsTo,
    "reachable_failed",
    None,
    true,
    "🔴 要確認"
);
producer_case!(
    bmc_reachable_failed_is_evaluated,
    "bmc",
    ReachableFailed,
    "reachable_failed",
    None,
    true,
    "🔴 要確認"
);
producer_case!(
    explicit_reachable_failed_is_evaluated,
    "explicit",
    ReachableFailed,
    "reachable_failed",
    None,
    true,
    "🔴 要確認"
);

#[test]
fn explicit_leadsto_is_a_refusal_not_a_reachable_verdict() {
    for case in [ProducerCase::LeadsTo, ProducerCase::UnwitnessedLeadsTo] {
        let fixture = Fixture::new("explicit-leadsto-refusal", &case.source());
        let verification = fixture.verify("explicit", 2);
        assert_eq!(verification["result"], "error", "{verification}");
        assert_eq!(verification["kind"], "semantics", "{verification}");
        assert!(verification["violation_kind"].is_null(), "{verification}");
        assert_reachable_evaluation_fields_absent(&verification);
        assert!(
            verification["message"]
                .as_str()
                .unwrap()
                .contains("explicit engine does not support leadsTo"),
            "{verification}"
        );
        for command in ["ledger", "html"] {
            let output = fixture.directory.join(format!("refused.{command}"));
            let envelope = json_output(
                &run(&[
                    command,
                    text(&fixture.spec),
                    "--engine",
                    "explicit",
                    "-o",
                    text(&output),
                ]),
                2,
            );
            assert_eq!(envelope["result"], "error", "{envelope}");
            // Reports currently reject the engine option even before the
            // explicit engine's own leadsTo semantics gate is reached.
            assert_eq!(envelope["kind"], "usage", "{envelope}");
            assert_eq!(
                envelope["message"], "--engine must be bmc or induction",
                "{envelope}"
            );
            assert!(
                !output.exists(),
                "refused report must not fabricate a property row"
            );
        }
    }
}

const REVIEW_CTI: &str = r#"spec Cti {
  type N = 0..100
  state { x: N }
  init { x = 0 }
  action step() { requires x < 98 x = x + 2 }
  @requirement("R-ODD")
  invariant NotOdd { x != 51 }
  @requirement("R-TOP")
  reachable Top { x == 4 }
}"#;

fn check_induction_reachable(fixture: &Fixture, rank_failure: bool) {
    let verification = fixture.verify("induction", 1);
    assert_eq!(verification["result"], "unknown_cti", "{verification}");
    assert_eq!(
        verification["violation_kind"].as_str(),
        rank_failure.then_some("leadsTo_rank"),
        "{verification}"
    );
    if rank_failure {
        assert_eq!(
            verification["rank_failure"], "non_decreasing_action",
            "{verification}"
        );
    }
    assert_reachable_evaluation_fields_absent(&verification);
    let scenarios = fixture.scenarios(0);
    assert!(
        scenarios["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .any(confirms_top),
        "{scenarios}"
    );
    let ledger = fixture.ledger("induction.md", &["--engine", "induction"], 1);
    let html = fixture.report("html", "induction.html", &["--engine", "induction"], 1);
    let expected = format!("bounded(BMC depth {})", fixture.depth);
    let row = findings::risk_row(&ledger, "R-TOP");
    let property = html_property_assurance(&html, "Top");
    assert_eq!(
        [
            row.contains("| 🟢 確認済（承認可） |"),
            row_assurance(&ledger, "R-TOP") == expected,
            property == expected
        ],
        [true; 3],
        "evaluated induction reachable: ledger={row}, HTML={property:?}, expected={expected}"
    );
}

#[test]
fn review_cti_keeps_ledger_and_html_reachable_bounded() {
    check_induction_reachable(&Fixture::new("review-cti", REVIEW_CTI), false);
}

#[test]
fn rank_failure_keeps_ledger_and_html_reachable_bounded() {
    let source = fs::read_to_string(
        root().join("tests/fixtures/rust_port/ranked_leadsto_non_decreasing.fsl"),
    )
    .unwrap();
    let (prefix, suffix) = source.rsplit_once('}').unwrap();
    let mut fixture = Fixture::new(
        "rank-failure",
        &format!("{prefix}\n @requirement(\"R-TOP\")\n reachable Top {{ x == 2 }}\n}}{suffix}"),
    );
    fixture.depth = 3;
    check_induction_reachable(&fixture, true);
}

const TWO_LEADSTOS: &str = r#"spec Two {
  state { x: Int, req: Bool }
  init { x = 0 req = false }
  fair action inc() { requires x < 5 x = x + 1 }
  fair action ask() { requires x == 5 requires req == false req = true }
  fair action serve() { requires x == 5 requires req == true req = false }
  invariant XRange { x >= 0 and x <= 5 }
  @requirement("R-FIVE")
  leadsTo ReachFive { x < 5 ~> x == 5 decreases 5 - x }
  @requirement("R-SERVED")
  leadsTo Served { req == true ~> req == false }
}"#;

#[test]
fn two_leadstos_need_any_bounded_obligation_not_all() {
    let fixture = Fixture::new("two-leadstos", TWO_LEADSTOS);
    let verification = fixture.verify("induction", 0);
    assert_eq!(verification["result"], "proved", "{verification}");
    assert_eq!(verification["completeness"], "unbounded", "{verification}");
    assert_eq!(verification["leads_to"].as_object().unwrap().len(), 2);
    assert_eq!(
        verification["leads_to"]["ReachFive"]["completeness"], "unbounded",
        "{verification}"
    );
    assert!(
        verification["leads_to"]["Served"]["completeness"].is_null(),
        "unranked producer entry deliberately omits completeness: {verification}"
    );
    assert_eq!(
        verification["leads_to"]["Served"]["checked_to_depth"], 8,
        "{verification}"
    );
    let ledger = fixture.ledger("two.md", &["--engine", "induction"], 0);
    let html = fixture.report("html", "two.html", &["--engine", "induction"], 0);
    assert!(ledger.contains("- 保証限界: BMC（有界モデル検査）: **深さ 8 までの全実行を網羅**。それ以遠の反例は本台帳の対象外"), "{ledger}");
    assert_eq!(panel_assurance(&html), "bounded(BMC depth 8)");
    assert_eq!(row_assurance(&ledger, "R-FIVE"), "proved(induction)");
    assert_eq!(row_assurance(&ledger, "R-SERVED"), "bounded(BMC depth 8)");
    assert_eq!(
        html_property_assurance(&html, "ReachFive"),
        "proved(induction)"
    );
    assert_eq!(
        html_property_assurance(&html, "Served"),
        "bounded(BMC depth 8)"
    );
}

#[test]
fn refinement_failure_over_induction_keeps_unranked_liveness_and_reachable_bounded() {
    let fixture = Fixture::new(
        "refinement",
        r#"requirements R {
      implements A from "abs_ensures.fsl" { map y = z action go() -> go() }
      type N = 0..4
      state { z: N, req: Bool }
      init { z = 0 req = false }
      action go() { z = 0 }
      fair action ask() { requires req == false req = true }
      fair action serve() { requires req == true req = false }
      @requirement("R-LT")
      leadsTo Served { req == true ~> req == false }
      @requirement("R-TOP")
      reachable Top { req == true }
    }"#,
    );
    fs::write(
        fixture.directory.join("abs_ensures.fsl"),
        r"spec A {
      type N = 0..4
      state { y: N }
      init { y = 0 }
      action go() { y = y ensures y == 3 }
    }",
    )
    .unwrap();
    let verification = fixture.verify("induction", 1);
    assert_eq!(
        verification["result"], "refinement_failed",
        "{verification}"
    );
    assert_eq!(verification["completeness"], "unbounded", "{verification}");
    let ledger = fixture.ledger("refinement.md", &["--engine", "induction"], 1);
    let html = fixture.report("html", "refinement.html", &["--engine", "induction"], 1);
    let observed = [
        row_assurance(&ledger, "R-LT"),
        row_assurance(&ledger, "R-TOP"),
        html_property_assurance(&html, "Served"),
        html_property_assurance(&html, "Top"),
        panel_assurance(&html),
    ];
    assert_eq!(
        observed, ["bounded(BMC depth 8)"; 5],
        "rows/properties must match the bounded aggregate under refinement_failed"
    );
    assert!(ledger.contains("- 保証限界: BMC（有界モデル検査）: **深さ 8 までの全実行を網羅**。それ以遠の反例は本台帳の対象外"), "{ledger}");
}

#[test]
fn unbounded_completeness_keeps_properties_bounded_after_implements_budget() {
    // Use the real 50,000-state implements cutoff, not an edited envelope or
    // injected budget. Never-assigned Bool init roots are nondeterministic
    // (#493): 2^16 initial valuations exceed the seam's self-consistency budget.
    let fixture = Fixture::new(
        "induction-budget",
        r#"requirements Budget {
      implements Wide from "wide.fsl" { maps auto }
      state { a: Bool, b: Bool, c: Bool, d: Bool, e: Bool, f: Bool, g: Bool, h: Bool,
              i: Bool, j: Bool, k: Bool, l: Bool, m: Bool, n: Bool, o: Bool, p: Bool }
      init {}
      action stay() { a = a }
      invariant Bounded { a or not a }
      @requirement("R-LT")
      leadsTo Served { a == true ~> a == true }
      @requirement("R-TOP")
      reachable Top { a == true }
    }"#,
    );
    fs::write(
        fixture.directory.join("wide.fsl"),
        r"spec Wide {
      state { a: Bool, b: Bool, c: Bool, d: Bool, e: Bool, f: Bool, g: Bool, h: Bool,
              i: Bool, j: Bool, k: Bool, l: Bool, m: Bool, n: Bool, o: Bool, p: Bool }
      init {}
      action stay() { a = a }
    }",
    )
    .unwrap();
    let verification = fixture.verify("induction", 1);
    assert_eq!(verification["result"], "unknown_budget", "{verification}");
    assert_eq!(verification["completeness"], "unbounded", "{verification}");
    assert_eq!(
        verification["implements"]["result"], "unknown_budget",
        "{verification}"
    );
    assert!(
        verification["implements"]["states_explored"]
            .as_u64()
            .is_some_and(|count| count >= 50_000),
        "{verification}"
    );
    let ledger = fixture.ledger("budget.md", &["--engine", "induction"], 1);
    let html = fixture.report("html", "budget.html", &["--engine", "induction"], 1);
    let observed = [
        row_assurance(&ledger, "R-LT"),
        row_assurance(&ledger, "R-TOP"),
        html_property_assurance(&html, "Served"),
        html_property_assurance(&html, "Top"),
    ];
    assert_eq!(
        observed, ["bounded(BMC depth 8)"; 4],
        "unbounded completeness needs per-group assurance beyond refinement_failed"
    );
}
