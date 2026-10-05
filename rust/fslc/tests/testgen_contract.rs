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
    generated_content_with(spec, depth, target, stem, &[])
}

fn generated_content_with(
    spec: &str,
    depth: &str,
    target: &str,
    stem: &str,
    extra: &[&str],
) -> String {
    let root = root();
    let directory = root.join("rust/target/testgen-contract");
    std::fs::create_dir_all(&directory).expect("create testgen output directory");
    let output_path = directory.join(format!("{stem}-{target}.out"));
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["testgen", spec, "--depth", depth, "--target", target])
        .args(extra)
        .arg("-o")
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

/// The six `specs/cart_v1.fsl --depth 3` scaffolds as every target emitted
/// them before issue #1200, when an unwired adapter skipped every test.
/// `--allow-unwired` must still reproduce them byte for byte, so a project
/// that opts back into the skip gets exactly the file it had. Kotlin is absent
/// because `--allow-unwired` rejects it (no runtime skip in kotlin.test).
const PRE_1200_SKIPPING_GOLDENS: [(&str, &str); 5] = [
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
        "dart",
        "c534f3d052103941937bbed6cb1943655033a1d37c49cc260d62fa096a71c06e",
    ),
    (
        "phpunit",
        "f5140ed71045fba394d1db93d017593de66089987a78fbfcb59ce71741350eb4",
    ),
];

#[test]
fn allow_unwired_reproduces_the_pre_1200_skipping_scaffolds() {
    for (target, digest) in PRE_1200_SKIPPING_GOLDENS {
        let content = generated_content_with(
            "specs/cart_v1.fsl",
            "3",
            target,
            "cart-allow-unwired",
            &["--allow-unwired"],
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(content.as_bytes())),
            digest,
            "{target} --allow-unwired output changed"
        );
    }
}

/// Issue #1200: kotlin.test has no portable runtime skip, so an unwired
/// Kotlin test could only return early and pass. `--allow-unwired` therefore
/// refuses the target instead of generating that silent green.
#[test]
fn allow_unwired_rejects_kotlin_and_unwired_kotlin_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "testgen",
            "specs/cart_v1.fsl",
            "--depth",
            "3",
            "--target",
            "kotlin",
            "--allow-unwired",
        ])
        .current_dir(root())
        .output()
        .expect("run native testgen");
    assert_eq!(output.status.code(), Some(2));
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("error envelope");
    assert_eq!(envelope["result"], "error", "{envelope:#}");
    assert!(
        envelope["message"]
            .as_str()
            .is_some_and(|message| message.contains("not available for --target kotlin")),
        "{envelope:#}"
    );
    let content = generated_content("specs/cart_v1.fsl", "3", "kotlin", "cart-unwired");
    assert!(!content.contains("?: return"), "{content}");
    assert_eq!(
        content
            .matches("val a = makeAdapter() ?: fail(ADAPTER_NOT_WIRED)")
            .count(),
        4,
        "three scenarios and the random walk must each fail when unwired:\n{content}"
    );
}

#[test]
fn all_six_public_kernel_targets_match_the_failing_by_default_goldens() {
    let expected = [
        (
            "pytest",
            "89454a73cbcc05d24fdbde88e30e97299c5eac7ac7098a06a62dc8d4a35076fb",
        ),
        (
            "vitest",
            "805c1659c48f50a3206c7c452a5113b5737780275c90297b4edb4c11ff997b96",
        ),
        (
            "swift",
            "69b6c6e728e6165b4130ab4635562d7931b705b8156325b62c5db163d0da9c92",
        ),
        (
            "kotlin",
            "7786f1afbbcd9b4523e1be5fa25abf1068aacc7df9940ef4f30ede8f92aa3f81",
        ),
        (
            "dart",
            "a23ffda2fbb99e906c1f0afb3ef6758941c8200a119538bdef14063107b95e9d",
        ),
        (
            "phpunit",
            "c3c38c14bbae0aa96c3336c4e0bf9020720b6ca1ed69cdd27626fe5d774188cf",
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
            "482c52bbc762d4bfc222c04af1bed21484ddf10d1b7742d6b6f180f89cd33a87",
        ),
        (
            "vitest",
            "b66938190d12f25bc1d043d300bb84a66d16a752a3ef65ac7bd7ee12a76337a9",
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
        "6990e0ad3329c88b8981a203139224c39e52a8f1676094cc8fe0759ba79a7f05",
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
