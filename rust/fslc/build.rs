// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const IMPLEMENTATION_CRATES: &[&str] = &[
    "fsl-core",
    "fsl-runtime",
    "fsl-solver",
    "fsl-solver-z3",
    "fsl-syntax",
    "fsl-tools",
    "fsl-verifier",
    "fslc",
];

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        files.push(path.to_path_buf());
        return;
    }
    let mut entries = std::fs::read_dir(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
        .map(|entry| entry.expect("read implementation entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for entry in entries {
        collect_files(&entry, files);
    }
}

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let workspace = manifest.parent().expect("Rust workspace root");
    let mut files = vec![workspace.join("Cargo.toml"), workspace.join("Cargo.lock")];
    for package in IMPLEMENTATION_CRATES {
        collect_files(&workspace.join(package), &mut files);
    }
    files.sort();

    let mut digest = Sha256::new();
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let relative = file
            .strip_prefix(workspace)
            .expect("workspace-relative input");
        digest.update(relative.as_os_str().as_encoded_bytes());
        digest.update(b"\0");
        digest.update(
            std::fs::read(&file)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", file.display())),
        );
        digest.update(b"\0");
    }
    for name in ["PROFILE", "TARGET", "CARGO_ENCODED_RUSTFLAGS"] {
        println!("cargo:rerun-if-env-changed={name}");
        digest.update(name.as_bytes());
        digest.update(b"=");
        digest.update(std::env::var(name).unwrap_or_default().as_bytes());
        digest.update(b"\0");
    }
    let mut features = std::env::vars()
        .filter(|(name, _)| name.starts_with("CARGO_FEATURE_"))
        .collect::<Vec<_>>();
    features.sort();
    for (name, value) in features {
        digest.update(name.as_bytes());
        digest.update(b"=");
        digest.update(value.as_bytes());
        digest.update(b"\0");
    }
    println!("cargo:rerun-if-env-changed=RUSTC");
    let rustc = std::process::Command::new(std::env::var_os("RUSTC").expect("rustc path"))
        .arg("-vV")
        .output()
        .expect("run rustc -vV");
    assert!(rustc.status.success(), "rustc -vV failed");
    digest.update(&rustc.stdout);
    println!(
        "cargo:rustc-env=FSLC_IMPLEMENTATION_FINGERPRINT={:x}",
        digest.finalize()
    );
}
