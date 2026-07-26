// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #564: `docs/DESIGN-html-report.md` specifies
//! that the property table's "Deadline" column appears when at least one
//! property declares a `leadsTo ... within`, under the same rule that forbids
//! `none` filler cells for an absent requirement caption. `html.rs` had no
//! such column at all — `grep -ci deadline` was 0. The caption half of that
//! same design paragraph landed with #525; this is its remainder.
//!
//! Because the column is *conditional* output, the load-bearing control is the
//! negative one: a spec with no `within` must produce no column, not a column
//! of empty cells.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The corpus spec whose kernel `leadsTo` actually carries a `within`.
///
/// `examples/nfr/{sla_worker,support_sla,sla_worker_kernel}.fsl` declare NFR
/// deadlines, but those lower into generated `_deadline_*` invariants before
/// the report is rendered, so none of them reaches this column.
const DEADLINE_SPEC: &str = "examples/nfr/bounded_response.fsl";
const NO_DEADLINE_SPEC: &str = "specs/cart_v1.fsl";

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn scratch_dir(label: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "issue-564-{label}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&path).expect("create scratch directory");
    path
}

/// Render one report and return its `#properties` table section.
fn properties_section(spec: &str, label: &str) -> String {
    let output_path = scratch_dir(label).join("report.html");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "html",
            spec,
            "--depth",
            "3",
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
    html.split_once("id=\"properties\"")
        .expect("properties section")
        .1
        .split_once("</section>")
        .expect("properties section end")
        .0
        .to_owned()
}

fn row_named<'a>(section: &'a str, name: &str) -> &'a str {
    section
        .split("<tr>")
        .find(|row| row.contains(&format!("<code>{name}</code>")))
        .and_then(|row| row.split("</tr>").next())
        .unwrap_or_else(|| panic!("no row for {name} in {section}"))
}

// --- negative: no deadline anywhere means no column at all ----------------

#[test]
fn a_spec_without_a_within_renders_no_deadline_column() {
    let section = properties_section(NO_DEADLINE_SPEC, "absent");
    assert!(!section.contains("<th>Deadline</th>"), "{section}");
    // Not merely a missing header: the rows must keep their four cells, so an
    // always-rendered column of empty cells cannot pass this.
    let row = row_named(&section, "SoldOut");
    assert_eq!(row.matches("<td").count(), 4, "{row}");
    assert!(!row.contains("chip fair"), "{row}");
}

// --- positive: the column appears, and only where a deadline exists --------

#[test]
fn a_declared_within_renders_the_deadline_column() {
    let section = properties_section(DEADLINE_SPEC, "present");
    assert!(section.contains("<th>Deadline</th>"), "{section}");
    let deadline_row = row_named(&section, "RespondsInTwo");
    assert!(
        deadline_row.contains("<td><span class=\"chip fair\">within 2</span></td>"),
        "{deadline_row}"
    );
}

#[test]
fn a_property_without_a_deadline_gets_an_empty_cell_not_a_filler() {
    // Same report as above: `EventuallyResponds` has no `within`, so its cell
    // is empty rather than a "none" chip, and the two rows must still line up
    // with the header.
    let section = properties_section(DEADLINE_SPEC, "mixed");
    let plain_row = row_named(&section, "EventuallyResponds");
    assert!(plain_row.contains("<td></td>"), "{plain_row}");
    assert!(!plain_row.contains("chip fair"), "{plain_row}");
    assert!(!plain_row.contains("none"), "{plain_row}");
    assert_eq!(plain_row.matches("<td").count(), 5, "{plain_row}");
    assert_eq!(
        row_named(&section, "RespondsInTwo").matches("<td").count(),
        5,
        "{section}"
    );
}
