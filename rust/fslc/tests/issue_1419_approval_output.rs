// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

#[path = "support/atomic_output_cli.rs"]
#[allow(dead_code)]
mod cli;

use cli::{OLD, args_with_output, io, json, root, run, scratch, text};
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::Path;
use std::process::Command;

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_DATE", "2026-10-27T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-10-27T00:00:00Z")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
#[allow(clippy::default_trait_access, clippy::too_many_lines)]
fn both_approval_writers_publish_records_and_reject_final_links() {
    // Reuse #190's fixture/invocation and deterministic #1475 Git baseline.
    // Only this test-owned repo is initialized/committed; no repo/global config.
    let home = scratch("approval");
    std::fs::copy(
        root().join("tests/fixtures/approval.fsl"),
        home.join("spec.fsl"),
    )
    .unwrap();
    git(&home, &["init", "-q"]);
    git(&home, &["add", "spec.fsl"]);
    git(
        &home,
        &[
            "-c",
            "user.name=Approval Test",
            "-c",
            "user.email=approval-test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
            "commit",
            "-qm",
            "approval baseline",
        ],
    );
    let ledger = args_with_output(
        &["ledger", "spec.fsl", "--depth", "2"],
        &home.join("review.md"),
    );
    json(&run(&home, &home, &ledger, None), 0);
    let key = SigningKey::from_bytes(&[7; 32]);
    let private = home.join("private.pem");
    let public = home.join("public.pem");
    std::fs::write(
        &private,
        key.to_pkcs8_pem(Default::default()).unwrap().as_bytes(),
    )
    .unwrap();
    std::fs::write(
        &public,
        key.verifying_key()
            .to_public_key_pem(Default::default())
            .unwrap(),
    )
    .unwrap();
    for signed in [false, true] {
        let out = home.join(if signed {
            "out/signed.json"
        } else {
            "out/unsigned.json"
        });
        let mut base_arguments = vec![
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "review.md",
            "--approver",
            "alice",
            "--depth",
            "2",
        ];
        if signed {
            base_arguments.extend(["--signing-key", text(&private)]);
        }
        let argv = args_with_output(&base_arguments, &out);
        let created = json(&run(&home, &home, &argv, None), 0);
        assert_record(&out, &created, signed);
        std::fs::write(&out, OLD).unwrap();
        let alias = out.with_extension("old");
        std::fs::hard_link(&out, &alias).unwrap();
        let inode = std::fs::metadata(&alias).unwrap().ino();
        let replaced = json(&run(&home, &home, &argv, None), 0);
        assert_record(&out, &replaced, signed);
        assert_eq!(std::fs::read(&alias).unwrap(), OLD);
        assert_eq!(std::fs::metadata(&alias).unwrap().ino(), inode);
        assert_ne!(std::fs::metadata(&out).unwrap().ino(), inode);
        let mut check = vec!["approval", "check", "spec.fsl", "--record", text(&out)];
        if signed {
            check.extend(["--trust-key", text(&public)]);
        }
        let check: Vec<String> = check.into_iter().map(str::to_owned).collect();
        let checked = json(&run(&home, &home, &check, None), 0);
        assert_eq!(checked["status"], "approved");
        if signed {
            assert_eq!(checked["signature_status"], "signed");
        }
        std::fs::remove_file(&out).unwrap();
        symlink(&alias, &out).unwrap();
        let before = cli::state(&home.join("out"));
        io(&run(&home, &home, &argv, None));
        assert_eq!(
            cli::state(&home.join("out")),
            before,
            "write_record signed={signed}: no link/target/sibling change"
        );
        let missing = home.join("out/missing/record.json");
        io(&run(
            &home,
            &home,
            &args_with_output(&base_arguments, &missing),
            None,
        ));
        assert!(
            !missing.parent().unwrap().exists(),
            "approval does not create missing parents"
        );
        assert_eq!(cli::state(&home.join("out")), before);
    }
}

fn assert_record(path: &Path, envelope: &serde_json::Value, signed: bool) {
    let bytes = std::fs::read(path).unwrap();
    assert!(
        bytes.ends_with(b"\n"),
        "existing record serialization newline"
    );
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        record, envelope["record"],
        "entire serialized record, including digests/signature"
    );
    assert_eq!(
        record["schema"],
        if signed {
            "fslc.approval.v2"
        } else {
            "fslc.approval.v1"
        }
    );
}
