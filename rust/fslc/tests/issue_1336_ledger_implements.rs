// SPDX-License-Identifier: Apache-2.0

//! #1336: the real ledger CLI must display the same seam verdict as its exit.

#[path = "support/report_cli.rs"]
mod cli;

fn assert_seam_body(fixture: &str, expected_status: i32) {
    let (body, status) = cli::run("ledger", fixture, 3, &[]);
    let (verified, verify_status) = cli::run_json("verify", fixture, 3, &[]);
    assert_eq!(status, expected_status, "{body}");
    assert_eq!(status, verify_status, "{verified:#}");
    let section = body
        .split_once("\n## inline implements\n")
        .unwrap_or_else(|| panic!("missing inline implements section: {body}"))
        .1
        .split("\n## ")
        .next()
        .expect("section body");
    assert!(
        section.contains(&format!(
            "- 検証判定: `{}`",
            verified["result"].as_str().expect("verdict")
        )),
        "{section}"
    );
    for key in ["abs", "result"] {
        let value = verified["implements"][key].as_str().expect("seam field");
        assert!(
            section.contains(&format!("`{value}`")),
            "missing {key}: {section}"
        );
    }
}

#[test]
fn failing_seam_is_visible_in_the_ledger_body() {
    assert_seam_body("tests/fixtures/chain/requirements_broken_implements.fsl", 1);
}

#[test]
fn passing_seam_is_visible_in_the_ledger_body() {
    assert_seam_body("tests/fixtures/chain/requirements.fsl", 0);
}

#[test]
fn governance_section_is_preserved_without_an_implements_section() {
    for (fixture, verdict, mark, result, status) in [
        (
            "governance.fsl",
            "refinement_failed",
            "🔴",
            "refinement_failed",
            1,
        ),
        ("governance_refines.fsl", "verified", "🟢", "refines", 0),
    ] {
        let fixture = format!("rust/fslc/tests/fixtures/issue_1303/{fixture}");
        let (body, actual_status) = cli::run("ledger", &fixture, 3, &[]);
        assert_eq!(actual_status, status, "{body}");
        assert!(!body.contains("## inline implements"), "{body}");
        assert!(body.contains("## governance preservation\n"), "{body}");
        assert!(
            body.contains(&format!("- 検証判定: `{verdict}`\n")),
            "{body}"
        );
        assert!(
            body.contains(&format!(
                "| `P` | `A` → `I` | `CTRL-1` | {mark} `{result}` |\n"
            )),
            "{body}"
        );
    }
}
