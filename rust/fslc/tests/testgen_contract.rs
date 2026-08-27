// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;
use sha2::{Digest, Sha256};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

/// A fresh scratch directory per call under `rust/target/`, so parallel test
/// binaries — and repeated runs in the same worktree — never collide and no
/// cleanup step is required (gitignored). Same idiom as
/// `rust/fslc/tests/chain_cli.rs`'s `scratch_dir` (issue #539).
fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = root().join(format!(
        "rust/target/testgen-contract-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn generated_content(spec: &str, depth: &str, target: &str, stem: &str) -> String {
    let root = root();
    let directory = root.join("rust/target/testgen-contract");
    std::fs::create_dir_all(&directory).expect("create testgen output directory");
    let output_path = directory.join(format!("{stem}-{target}.out"));
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["testgen", spec, "--depth", depth, "--target", target, "-o"])
        .arg(&output_path)
        .current_dir(root)
        .output()
        .expect("run native testgen");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(output_path).expect("read generated scaffold")
}

fn generated_digest(spec: &str, depth: &str, target: &str, stem: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(generated_content(spec, depth, target, stem).as_bytes())
    )
}

#[test]
fn all_six_public_kernel_targets_match_the_pre_migration_goldens() {
    let expected = [
        (
            "pytest",
            "8b4187523682e08090072c56177fb888cddc842ea023963261e858589add7f1c",
        ),
        (
            "vitest",
            "ccd23beba0a6fc8960f8d4b83075efe69e531e305b1e424644a2e3408e4109d9",
        ),
        (
            "swift",
            "4811e2f029636e27096f37b081a907d0e38fdab9f61c198da5def2d59a5fee71",
        ),
        (
            "kotlin",
            "d5e271917aadc4e19ddaf4825fa18b8a1ee90e7bab94cf1954cee9077da09a65",
        ),
        (
            "dart",
            "c534f3d052103941937bbed6cb1943655033a1d37c49cc260d62fa096a71c06e",
        ),
        (
            "phpunit",
            "f5140ed71045fba394d1db93d017593de66089987a78fbfcb59ce71741350eb4",
        ),
    ];
    for (target, digest) in expected {
        assert_eq!(
            generated_digest("specs/cart_v1.fsl", "3", target, "cart"),
            digest,
            "{target} output changed"
        );
    }
}

#[test]
fn nested_option_expected_state_is_lossless() {
    let source = r"
spec NestedOptionTestgen {
  type Bit = 0..1
  state { x: Option<Option<Bit>> }
  init { x = none }
  action wrap() { requires x == none  x = some(none) }
  action fill() { requires x == some(none)  x = some(some(1)) }
  action clear() { requires x == some(some(1))  x = none }
}
";
    let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
        .expect("parse nested Option testgen model");
    let model = fsl_core::build_model(kernel).expect("build nested Option testgen model");
    let fslc_rust::TestgenWalk::Clean(trace) =
        fslc_rust::testgen_trace_vectors(&model).expect("generate nested Option testgen trace")
    else {
        panic!("nested Option cycle must not violate");
    };

    assert_eq!(trace["initial"], json!({"x": null}));
    assert_eq!(
        trace["steps"][0]["expected"],
        json!({"x": {"kind":"some","value":null}})
    );
    assert_eq!(
        trace["steps"][1]["expected"],
        json!({"x": {"kind":"some","value":1}})
    );
    assert_eq!(trace["steps"][2]["expected"], json!({"x": null}));
}

#[test]
fn compose_bridge_preserves_pytest_and_baked_target_goldens() {
    for (target, digest) in [
        (
            "pytest",
            "870aa7f2aea4e759990e9d52acd9e55e4b133957a8b2fa3e730900d49704547c",
        ),
        (
            "vitest",
            "08142b2a05359c7d1697e28cf8dfc95701d859bc0ab5cfed33e7ed8f3d6d9587",
        ),
    ] {
        assert_eq!(
            generated_digest("specs/bank_system.fsl", "2", target, "compose"),
            digest,
            "compose {target} output changed"
        );
    }
}

/// Issue #471: the native `emit_pytest` scenario loop dropped the
/// `forbidden`-scenario rejection assertion (`_assert_rejected`), so a
/// generated pytest harness that named itself `test_scenario_forbidden_FB_1`
/// asserted nothing about the forbidden transition and passed against a
/// guard-weakened implementation. `specs/cart_v1.fsl` (the golden above) has
/// no `forbidden` declaration, so that golden alone cannot catch this class
/// of regression. This is the coupled regression case: a golden digest for a
/// spec that *does* declare `forbidden`, plus the byte-identical-to-Python
/// content this golden guards being non-trivial (both lines the emitter had
/// been silently dropping, mirroring `tests/test_verified_bugs.py`'s
/// `test_forbidden_testgen_rejection_assertion`, which only exercises the
/// frozen Python `fslc.cli.run_testgen`).
#[test]
fn pytest_target_emits_the_forbidden_rejection_assertion() {
    let content = generated_content(
        "examples/gallery/valid/small_forbidden_guarded_cancel.fsl",
        "3",
        "pytest",
        "fbcancel",
    );
    assert!(
        content.contains("result = adapter.step('cancel', {'o': 0})"),
        "forbidden step call missing from generated pytest:\n{content}"
    );
    assert!(
        content.contains("_assert_rejected(result, 'requires_failed')"),
        "forbidden rejection assertion missing from generated pytest:\n{content}"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(content.as_bytes())),
        "da26cf763e96508472a0fcda8c2b4d7c69652d478794c772bd2053389e1b2e51",
        "small_forbidden_guarded_cancel.fsl pytest output changed"
    );
}

#[cfg(unix)]
#[test]
fn symlink_source_name_and_canonical_pytest_path_remain_distinct() {
    use std::os::unix::fs::symlink;

    let root = root();
    let fixture_root = scratch_dir("symlink");
    let directory = fixture_root.join("path-context");
    let real_output_parent = fixture_root.join("path-output-real");
    std::fs::create_dir_all(&directory).expect("create path-context fixture");
    std::fs::create_dir_all(&real_output_parent).expect("create real output directory");
    let generated = directory.join("generated-link");
    symlink(&real_output_parent, &generated).expect("create output-parent symlink");
    let alias = directory.join("cart-alias.fsl");
    symlink(root.join("specs/cart_v1.fsl"), &alias).expect("create spec symlink");
    let output_path = generated.join("cart.py");

    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("testgen")
        .arg(&alias)
        .args(["--depth", "3", "--target", "pytest", "-o"])
        .arg(&output_path)
        .current_dir(&root)
        .output()
        .expect("run symlinked native testgen");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let generated = std::fs::read_to_string(&output_path).expect("read symlink pytest output");
    assert!(generated.contains("Source: cart-alias.fsl"));
    assert!(
        generated.contains(
            "SPEC_PATH = Path(__file__).resolve().parent / '../../../../specs/cart_v1.fsl'"
        )
    );
}
