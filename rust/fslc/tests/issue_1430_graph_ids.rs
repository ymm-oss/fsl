// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1430 CLI input-path evidence and byte-preservation control. `causal_graph`
//! nodes are variable:<ASCII identifier>, so sanitization is injective there.
//! Non-ASCII variables cannot reach the exporter: the standalone causal parser
//! calls stable `lex` without FSL 5 identifier options. Collision detectors live
//! in fsl-tools/tests/issue_1430_graph_ids.rs through public JSON functions.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn cli(path: &Path, format: &str, expected_status: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["causal", "analyze"])
        .arg(path)
        .args(["--projection", "causal_graph", "--format", format])
        .current_dir(root())
        .output()
        .expect("run fslc");
    assert_eq!(
        output.status.code(),
        Some(expected_status),
        "produced stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("JSON={error}; produced stdout={:?}", output.stdout))
}

/// Control (passes before/after): the suggested non-ASCII CLI collision input
/// is rejected before projection; this must not be mistaken for a detector.
#[test]
fn non_ascii_variable_collision_is_not_reachable_from_causal_cli() {
    let directory = root()
        .join(".codex/tmp")
        .join(format!("issue-1430-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("local scratch");
    let path = directory.join("unicode.fsl");
    std::fs::write(&path, "causal Collision { timebase day horizon 10 variable あ { role intervention } variable い { role outcome } }").expect("write probe");
    let value = cli(&path, "mermaid", 2);
    assert_eq!(value["result"], "error", "produced output={value}");
    assert!(
        value["message"].as_str().is_some_and(
            |message| message.contains("unexpected character") && message.contains('あ')
        ),
        "produced output={value}"
    );
}

/// Control (passes before/after): pin the full collision-free Mermaid content
/// of `MarketingFunnel`, the existing example in `causal_cli.rs`, on 9ad07c7a.
#[test]
fn marketing_funnel_mermaid_keeps_exact_bytes() {
    let value = cli(
        Path::new("examples/causal/marketing_funnel.fsl"),
        "mermaid",
        0,
    );
    let expected = concat!(
        "graph LR\n",
        "  variable_ads[\"variable:ads (intervention)\"]\n",
        "  variable_fatigue[\"variable:fatigue (mediator)\"]\n",
        "  variable_seasonality[\"variable:seasonality (context)\"]\n",
        "  variable_signups[\"variable:signups (outcome)\"]\n",
        "  variable_traffic[\"variable:traffic (mediator)\"]\n",
        "  variable_ads -->|\"claim:C_Ads_Fatigue positive\"| variable_fatigue\n",
        "  variable_ads -->|\"claim:C_Ads_Traffic positive\"| variable_traffic\n",
        "  variable_fatigue -->|\"claim:C_Fatigue_Signups negative\"| variable_signups\n",
        "  variable_seasonality -->|\"claim:C_Season_Signups positive\"| variable_signups\n",
        "  variable_seasonality -->|\"claim:C_Season_Traffic positive\"| variable_traffic\n",
        "  variable_traffic -->|\"claim:C_Traffic_Signups positive\"| variable_signups\n"
    );
    assert_eq!(value["content"], expected, "produced output={value}");
}
