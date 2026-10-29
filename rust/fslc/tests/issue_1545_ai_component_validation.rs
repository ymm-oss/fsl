// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

#[path = "support/scratch_identity.rs"]
mod scratch_identity;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn fixture(source: &str) -> PathBuf {
    let root = scratch_identity::unique(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1545-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("component.fsl");
    std::fs::write(&path, source).unwrap();
    path
}

fn run(command: &[&str], path: &Path) -> (Value, i32) {
    let mut process = Command::new(env!("CARGO_BIN_EXE_fslc"));
    process
        .args(command)
        .arg(path)
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_UNSTABLE");
    if command == ["ai", "replay"] {
        process.args(["--logs", "tests/fixtures/error_envelope_empty_records.json"]);
    }
    let output = process
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{command:?}: {error}: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (value, output.status.code().unwrap())
}

#[test]
fn no_tools_are_rejected_before_profiles_replay_and_project_analysis() {
    for (name, body) in [("NoTools", ""), ("EmptyAuthority", "authority { }")] {
        let standalone = format!("\n  ai_component {name} {{ {body} }}\n");
        for project in [false, true] {
            let source = if project {
                format!(
                    "{standalone}\nstatistical_property Q {{\n  target {name}\n  require min_samples >= 0\n}}\n"
                )
            } else {
                standalone.clone()
            };
            let path = fixture(&source);
            for command in [
                &["check"][..],
                &["ai", "check"],
                &["ai", "compat"],
                &["ai", "replay"],
            ] {
                let (value, status) = run(command, &path);
                println!("{command:?}, project={project}: exit={status}, {value}");
                assert_eq!(status, 2, "{value}");
                let mut expected = json!({"fsl":"1.0", "result":"error", "kind":"type",
                    "message":format!("ai_component '{name}' declares no tools"),
                    "loc":{"line":2,"column":3}});
                if command == ["check"] {
                    expected["versions"] = json!({
                        "verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},
                        "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
                        "solver":{"name":"z3","backend":"native-z3","version":"Z3 4.16.0.0"}
                    });
                }
                assert_eq!(value, expected);
            }
            if !project {
                let (value, status) = run(&["kernel"], &path);
                assert_eq!(status, 2);
                assert_eq!(
                    value,
                    json!({"fsl":"1.0","result":"error","kind":"type",
                    "message":format!("ai_component '{name}' declares no tools"),"loc":{"line":2,"column":3}})
                );
            }
        }
    }
    let path = fixture("ai_component OneTool { tool Search { schema SearchV1; } authority { } }");
    for (command, expected) in [
        (&["check"][..], "ok"),
        (&["ai", "compat"], "compat_profile_generated"),
        (&["ai", "replay"], "replay_conformant"),
        (&["ai", "check"], "verified_under_assumptions"),
    ] {
        let (value, status) = run(command, &path);
        assert_eq!(status, 0, "{command:?}: {value}");
        assert_eq!(value["result"], expected);
    }
}

#[test]
fn lowering_only_commands_reject_unselected_authority_reference() {
    let path = fixture(
        "ai_component ReviewAuthority {\n  tool Search { schema S; }\n  authority { may_execute Search; forbidden X; }\n  check hard { rule tool_authority; }\n}\n",
    );
    for command in ["analyze", "scenarios", "explain"] {
        let (value, status) = run(&[command], &path);
        assert_eq!(status, 2, "{command}: {value}");
        assert_eq!(
            value,
            json!({"fsl":"1.0","result":"error","kind":"semantics",
            "message":"unknown tool 'X' in authority block"}),
            "{command}"
        );
    }
}

#[test]
fn lowering_only_commands_report_changed_duplicate_fallback_priority() {
    let path = fixture(
        "ai_component DupNoTools {\n  fallback {\n    when x require human_review;\n    when x require human_review;\n  }\n}\n",
    );
    for command in ["analyze", "scenarios", "explain"] {
        let (value, status) = run(&[command], &path);
        assert_eq!(status, 2, "{command}: {value}");
        assert_eq!(
            value,
            json!({"fsl":"1.0","result":"error","kind":"semantics",
            "message":"duplicate fallback reason 'x'"}),
            "{command}"
        );
    }
}
