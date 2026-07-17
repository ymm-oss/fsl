// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn generated_digest(spec: &str, depth: &str, target: &str, stem: &str) -> String {
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
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(output_path).expect("read generated scaffold"))
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
