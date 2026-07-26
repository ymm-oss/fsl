// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #525: `fslc html --engine induction`
//! stamped one report-wide assurance class on every property row, so a
//! `reachable` rendered as `proved(induction)` even though a reachability
//! witness under induction comes only from the bounded base BMC. In an audit
//! artifact "proved" means all-depth universal evidence; that is an assurance
//! misstatement, which `AGENTS.md` names as never allowlistable. The same
//! renderer also never called `requirement_caption` for property rows, so
//! invariants/reachables/leadsTo lost the requirement text that actions and
//! counterfactuals keep (`docs/DESIGN-html-report.md`).
//!
//! Property rows now classify per element through `ledger::formal_assurance`,
//! the rule `fslc ledger` already applies
//! (`docs/DESIGN-assurance-classes.md`).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn scratch_dir(label: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "issue-525-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&path).expect("create scratch directory");
    path
}

/// Render one induction report and return its `#properties` table rows.
fn property_rows(spec: &str, label: &str) -> Vec<String> {
    let output_path = scratch_dir(label).join("report.html");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "html",
            &fixture(spec).display().to_string(),
            "--engine",
            "induction",
            "-o",
            &output_path.display().to_string(),
        ])
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(envelope["result"], "generated", "{envelope}");
    let html = std::fs::read_to_string(&output_path).expect("read report");
    let section = html
        .split_once("id=\"properties\"")
        .expect("properties section")
        .1
        .split_once("</section>")
        .expect("properties section end")
        .0
        .to_owned();
    section
        .split("<tr>")
        .skip(1)
        .map(|row| row.split("</tr>").next().unwrap_or_default().to_owned())
        .collect()
}

fn row_named<'a>(rows: &'a [String], name: &str) -> &'a str {
    rows.iter()
        .find(|row| row.contains(&format!("<code>{name}</code>")))
        .unwrap_or_else(|| panic!("no row for {name} in {rows:#?}"))
}

// --- negative: one report, two different per-property classes -------------

#[test]
fn induction_report_proves_the_invariant_but_keeps_reachables_bounded() {
    // Both assertions read the *same* report: two separate single-property
    // tests could each pass while disagreeing about what the report says.
    let rows = property_rows("issue_525_property_assurance.fsl", "mixed");
    let invariant = row_named(&rows, "NonNegative");
    assert!(
        invariant.contains("<td>proved(induction)</td>"),
        "{invariant}"
    );
    let reachable = row_named(&rows, "Funded");
    assert!(
        reachable.contains("<td>bounded(BMC depth 8)</td>"),
        "{reachable}"
    );
    assert!(
        !reachable.contains("proved"),
        "a reachability witness under induction is bounded: {reachable}"
    );
    let untagged_reachable = row_named(&rows, "Drained");
    assert!(
        untagged_reachable.contains("<td>bounded(BMC depth 8)</td>"),
        "{untagged_reachable}"
    );
}

#[test]
fn unranked_leadsto_stays_bounded_inside_a_proved_report() {
    // `RequestFlow` proves (`result:"proved"`, `completeness:"unbounded"`),
    // but its `leadsTo` carries no unbounded ranking, so only a bounded claim
    // is supported.
    let rows = property_rows("issue_525_leadsto_assurance.fsl", "leadsto");
    let leadsto = row_named(&rows, "POL-ANSWER");
    assert!(
        leadsto.contains("<td>bounded(BMC depth 8)</td>"),
        "{leadsto}"
    );
    assert!(!leadsto.contains("proved"), "{leadsto}");
    let reachable = row_named(&rows, "CanAnswer");
    assert!(
        reachable.contains("<td>bounded(BMC depth 8)</td>"),
        "{reachable}"
    );
}

// --- requirement captions, per row, without leakage -----------------------

#[test]
fn each_property_row_renders_its_own_requirement_caption() {
    let rows = property_rows("issue_525_property_assurance.fsl", "captions");
    let invariant = row_named(&rows, "NonNegative");
    assert!(
        invariant
            .contains("<div class=\"req-caption\">REQ-INV: the balance never goes negative</div>"),
        "{invariant}"
    );
    let reachable = row_named(&rows, "Funded");
    assert!(
        reachable
            .contains("<div class=\"req-caption\">REQ-REACH: a funded balance is reachable</div>"),
        "{reachable}"
    );
    // Two differently tagged rows: neither may show the other's text.
    assert!(!invariant.contains("REQ-REACH"), "{invariant}");
    assert!(!reachable.contains("REQ-INV"), "{reachable}");
}

#[test]
fn an_untagged_property_row_renders_no_caption() {
    // `docs/DESIGN-html-report.md`: the caption is "omitted entirely for a row
    // that has none -- no `none` filler cells".
    let rows = property_rows("issue_525_property_assurance.fsl", "untagged");
    let untagged = row_named(&rows, "Drained");
    assert!(!untagged.contains("req-caption"), "{untagged}");
    assert!(!untagged.contains("None"), "{untagged}");
}

#[test]
fn requirement_caption_text_is_html_escaped() {
    // The caption is rendered as markup, so its text must stay escaped.
    let rows = property_rows("issue_525_escaping.fsl", "escaping");
    let tagged = row_named(&rows, "Bounded");
    assert!(
        tagged.contains("&lt;script&gt;") && tagged.contains("&amp;"),
        "{tagged}"
    );
    assert!(!tagged.contains("<script>"), "{tagged}");
}
