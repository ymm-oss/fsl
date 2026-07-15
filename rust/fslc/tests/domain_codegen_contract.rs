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

fn collect_files(directory: &Path, current: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(current).expect("read generated directory") {
        let path = entry.expect("read generated entry").path();
        if path.is_dir() {
            collect_files(directory, &path, files);
        } else {
            files.push(
                path.strip_prefix(directory)
                    .expect("relative file")
                    .to_owned(),
            );
        }
    }
}

fn generated_digest(target: &str) -> String {
    let root = root();
    let directory = root.join(format!("rust/target/domain-codegen-contract/{target}"));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("clear generated directory");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "domain",
            "generate",
            "rust/fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl",
            "--target",
            target,
            "-o",
        ])
        .arg(&directory)
        .current_dir(&root)
        .output()
        .expect("run domain generator");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut files = Vec::new();
    collect_files(&directory, &directory, &mut files);
    files.sort();
    let mut digest = Sha256::new();
    for relative in files {
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(std::fs::read(directory.join(relative)).expect("read generated file"));
        digest.update([0]);
    }
    format!("{:x}", digest.finalize())
}

fn assert_generation_succeeds(spec: &str, target: &str) {
    let root = root();
    let stem = Path::new(spec)
        .file_stem()
        .expect("fixture stem")
        .to_string_lossy();
    let directory = root.join(format!(
        "rust/target/domain-codegen-contract/corpus/{stem}/{target}"
    ));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("clear generated directory");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["domain", "generate", spec, "--target", target, "-o"])
        .arg(directory)
        .current_dir(root)
        .output()
        .expect("run domain generator");
    assert!(
        output.status.success(),
        "{spec} ({target}): {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn domain_testgen_digest() -> String {
    let root = root();
    let output_path = root.join("rust/target/domain-codegen-contract/domain.test.ts");
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).expect("create domain testgen directory");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "domain",
            "testgen",
            "rust/fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl",
            "--target",
            "vitest",
            "-o",
        ])
        .arg(&output_path)
        .current_dir(root)
        .output()
        .expect("run domain testgen");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(output_path).expect("read domain testgen output"))
    )
}

#[test]
fn all_five_public_kernel_domain_targets_match_pre_migration_goldens() {
    for (target, expected) in [
        (
            "typescript",
            "12d7306c526ccdf0876b82ddc447d8a680081bf1a7d6d7d4175e30567bc37c52",
        ),
        (
            "python",
            "a5598dc2babb34cc099b87cb7c0c09ad236721ed3c50e0df3bc0690aca73d03b",
        ),
        (
            "kotlin",
            "b9be8484edd9e1e08284c3b17c30c2e7f00ded455d10bda05a38581df470064b",
        ),
        (
            "swift",
            "7417ddd7c8b766ee60fc81685955d64f6f401466dd11d7c79eda86bd16808862",
        ),
        (
            "rust",
            "3edcf8710a511436f6a5b76f073d9e2ee19300b0dc323c7e3a12bbf420986745",
        ),
    ] {
        assert_eq!(
            generated_digest(target),
            expected,
            "{target} output changed"
        );
    }
}

#[test]
fn domain_testgen_adapter_and_effects_match_the_pre_migration_golden() {
    assert_eq!(
        domain_testgen_digest(),
        "14f8a1466b9140875d288fb083e58401acf5f511b0089571098e7f148a841cf8"
    );
}

#[test]
fn every_valid_domain_corpus_entry_generates_all_five_targets() {
    for spec in [
        "rust/fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl",
        "rust/fslc/tests/fixtures/domain_characterization/expressions_valid.fsl",
        "rust/fslc/tests/fixtures/domain_characterization/lvalues_surface.fsl",
        "examples/domain/order_async_effect.fsl",
        "examples/domain/order_fulfillment_saga.fsl",
        "examples/domain/order_functional_ddd.fsl",
        "examples/domain/unsafe_irreversible_effect_without_idempotency.fsl",
    ] {
        for target in ["typescript", "python", "kotlin", "swift", "rust"] {
            assert_generation_succeeds(spec, target);
        }
    }
}
