// SPDX-License-Identifier: Apache-2.0

//! #1433: relation panels depend on declared skeleton types, not pair-shaped JSON.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "issue-1433-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("local scratch");
    dir
}

fn source(ty: &str, init: &str, property: &str) -> String {
    format!(
        "spec Graphs {{\n  type N = 0..1\n  state {{ edges: {ty} }}\n  init {{ edges = {init} }}\n  action link() {{ edges = edges }}\n  {property}\n}}\n"
    )
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("native fslc")
}

#[test]
fn detector_pair_shaped_seq_and_set_are_not_relation_graphs() {
    // Native FSL currently permits only scalar Seq/Set elements, not pairs.
    // Exercise the public HTML renderer with a real model/skeleton and an
    // adapted trace. This isolates the renderer's shape-vs-type decision; it
    // does NOT claim that pair collections are accepted by the language gate.
    let mut failures = Vec::new();
    for (ty, init, tag) in [("Seq<N, 2>", "Seq {}", "seq"), ("Set<N>", "Set {}", "set")] {
        let dir = scratch();
        let source = source(ty, init, "");
        std::fs::write(dir.join("graphs.fsl"), &source).expect("model fixture");
        let explained = run(&dir, &["explain", "graphs.fsl", "--depth", "1"]);
        assert_eq!(explained.status.code(), Some(0), "{explained:?}");
        let explained: Value =
            serde_json::from_slice(&explained.stdout).expect("real explain JSON");
        assert_eq!(explained["skeleton"]["state"]["edges"][0], tag);
        let kernel =
            fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new(".")).expect("parse");
        let model = fsl_core::build_model(kernel).expect("build model");
        let verification = json!({"spec":"Graphs", "result":"verified", "trace":[
            {"step":0, "state":{"edges":[[0,1],[1,0]]}, "action":null, "changes":{}}
        ]});
        let html = fsl_tools::render_html_report(
            "graphs.fsl",
            &source,
            &model,
            &explained,
            &verification,
            &[],
        );
        assert!(html.contains("Trace Review"), "trace panel remains live");
        if html.contains("<h3>Relation Graphs</h3>")
            || html.contains("<div class=\"relation-graphs\">")
        {
            failures.push(format!(
                "{ty}: renderer inferred a relation from [[0,1],[1,0]]"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn relation_html(empty: bool) -> String {
    let dir = scratch();
    let mut source = source(
        "relation N -> N",
        "Set {}",
        if empty {
            "reachable Empty { not edges.contains(0, 1) }"
        } else {
            "reachable Linked { edges.contains(0, 1) }"
        },
    );
    if !empty {
        source = source.replace("edges = edges", "edges = edges.add(0, 1)");
    }
    std::fs::write(dir.join("graphs.fsl"), source).expect("relation fixture");
    let output = run(
        &dir,
        &[
            "html",
            "graphs.fsl",
            "--depth",
            "1",
            "--no-timing",
            "--deadlock",
            "ignore",
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    String::from_utf8(output.stdout).expect("HTML")
}

#[test]
fn preservation_real_relation_report_is_byte_identical_to_base() {
    let html = relation_html(false);
    assert!(html.contains("<h3>Relation Graphs</h3>"));
    assert!(html.contains("<span class=\"edge\">0 -&gt; 1</span>"));
    assert_eq!(
        format!("{:x}", Sha256::digest(html.as_bytes())),
        "c2d5150e82d5a4615b61d088e4a8793596d185ed7b77c510517a625083848848"
    );
}

#[test]
fn preservation_empty_relation_report_is_byte_identical_to_base() {
    let html = relation_html(true);
    // Base suppresses empty relation panels. Preserve that behavior rather
    // than silently adding a panel while fixing type selection.
    assert!(!html.contains("<h3>Relation Graphs</h3>"));
    assert!(
        html.contains("Representative reachable trace"),
        "live empty witness"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(html.as_bytes())),
        "5c0119b47e7d44329bfcd23d2ca045621a4270605952930bb45afbd298731a6c"
    );
}
