// SPDX-License-Identifier: Apache-2.0

//! Literal packet anchors grounded on f8e3726e before observer implementation.
//! These assert named contract fields, NOT whole-envelope/engine agreement.
//! BMC and explicit share the diagnostic collector and are not independent proof.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);

const OR_LEFT: &str = "spec W { state { o: Option<Int> = some(1) } action tick() {} invariant Q { forall k in 0..0 { ((o is some(v)) and v == 2) or false } } }\n";
const NOT_MATCH: &str = "spec W { state { o: Option<Int> = some(1) } action tick() {} invariant Q { forall k in 0..0 { not (o is some(v)) } } }\n";
const SELECTED_FILTER: &str = "spec W { state { o: Option<Int> = some(1) } action tick() {} invariant Q { forall k in 0..0 where o is some(w) and w == 1 { not (o is some(v)) } } }\n";
const FALLBACK: &str = "spec W { state { o: Option<Int> = some(1) } action tick() {} invariant Q { not (o is some(v)) } }\n";

fn run(dir: &Path, args: &[&str], tag: &str, source: &str) -> (Value, i32) {
    let binary = env!("CARGO_BIN_EXE_fslc");
    let output = Command::new(binary)
        .args(args)
        .current_dir(dir)
        .env_remove("FSLC_CACHE")
        .output()
        .expect("native fslc");
    let exit = output.status.code().expect("exit code");
    for (suffix, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
        std::fs::write(dir.join(format!("{tag}.{suffix}")), bytes).expect("raw packet");
    }
    std::fs::write(
        dir.join(format!("{tag}.metadata.json")),
        serde_json::to_vec_pretty(&json!({
            "argv":std::iter::once(binary).chain(args.iter().copied()).collect::<Vec<_>>(),
            "cwd":dir,"exit":exit,
            "source_sha256":format!("{:x}",Sha256::digest(source.as_bytes())),
            "binary":binary,
            "binary_sha256":format!("{:x}",Sha256::digest(std::fs::read(binary).expect("binary identity")))
        }))
        .unwrap(),
    )
    .expect("packet metadata");
    assert!(output.stderr.is_empty(), "{tag}: {output:?}");
    let report = serde_json::from_slice(&output.stdout).expect("native JSON envelope");
    eprintln!("PACKET {tag} exit={exit} evidence={}", dir.display());
    (report, exit)
}

fn packet(source: &str, engine: &str, repeat: u8, text: &str, bindings: &Value) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../.codex/tmp/issue-1622-evidence-{}-{}-{engine}-{repeat}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("owned evidence directory");
    std::fs::write(dir.join("w.fsl"), source).expect("exact source bytes");
    let (checked, exit) = run(&dir, &["check", "w.fsl"], "check", source);
    assert_eq!(exit, 0, "fixture admission: {checked:#}");
    assert_eq!(checked["result"], "ok", "{checked:#}");
    let (report, exit) = run(
        &dir,
        &[
            "verify",
            "w.fsl",
            "--engine",
            engine,
            "--depth",
            "2",
            "--no-cache",
        ],
        "verify",
        source,
    );
    // Check truth/location/trace before evidence so a red anchor is not a build,
    // refusal, runtime error, or silently changed verdict.
    assert_eq!(exit, 1, "{report:#}");
    assert_eq!(report["fsl"], "1.0");
    assert_eq!(report["spec"], "W");
    assert_eq!(report["result"], "violated");
    assert_eq!(report["violation_kind"], "invariant");
    assert_eq!(report["invariant"], "Q");
    assert_eq!(report["loc"], json!({"line":1,"column":62}));
    assert_eq!(report["trace"], json!([{"step":0,"state":{"o":1}}]));
    assert_eq!(report["trace_type"], "invariant");
    assert_eq!(report["violated_at_step"], 0);
    assert_eq!(report["checked_to_depth"], 0);
    assert_eq!(report["completeness"], "bounded");
    assert_eq!(report["last_action"], Value::Null);
    // Compare both copies together: a missing top map must not hide an
    // independently incorrect blame map or rendered expression.
    let actual = json!({"bindings":report["violating_bindings"],"blame":report["blame"]});
    let expected = json!({"bindings":bindings,"blame":{"conjuncts":[{
        "index":0,"text":text,"holds":false,"violating_bindings":bindings
    }]}});
    eprintln!("EVIDENCE engine={engine} repeat={repeat} expected={expected} produced={actual}");
    assert_eq!(actual, expected, "literal reached-evidence packet");
}

macro_rules! case {
    ($name:ident, $source:ident, $text:literal, $bindings:tt) => {
        mod $name {
            use super::*;
            #[test]
            fn bmc_1() {
                packet($source, "bmc", 1, $text, &json!($bindings));
            }
            #[test]
            fn bmc_2() {
                packet($source, "bmc", 2, $text, &json!($bindings));
            }
            #[test]
            fn explicit_1() {
                packet($source, "explicit", 1, $text, &json!($bindings));
            }
            #[test]
            fn explicit_2() {
                packet($source, "explicit", 2, $text, &json!($bindings));
            }
        }
    };
}

case!(or_left, OR_LEFT, "forall k in 0..0 { o is some(v) and v == 2 or false }", [{"k":0,"v":1}]);
case!(not_match, NOT_MATCH, "forall k in 0..0 { not o is some(v) }", [{"k":0,"v":1}]);
case!(selected_filter, SELECTED_FILTER, "forall k in 0..0 where o is some(w) and w == 1 { not o is some(v) }", [{"k":0,"v":1,"w":1}]);
case!(fallback, FALLBACK, "not o is some(v)", [{"v":1}]);

#[test]
fn supplied_raw_sources_are_byte_exact() {
    assert_eq!(
        format!("{:x}", Sha256::digest(OR_LEFT.as_bytes())),
        "637fc35f975dfa61bc846431d388a202374ddc291435c2af525acd47c7b78685"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(NOT_MATCH.as_bytes())),
        "77baf140453a9d8fe76bc16ce5443d608bd05cfea328c29f74481aa500a3eb35"
    );
}
