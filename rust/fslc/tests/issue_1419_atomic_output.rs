// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

#[path = "support/atomic_output_cli.rs"]
#[allow(dead_code)]
mod cli;

#[path = "support/cex_timing.rs"]
mod cex_timing;

use cli::{DOMAIN, OLD, PM, args_with_output, io, json, names, root, run, scratch};
use std::path::Path;

fn families() -> Vec<(&'static str, Vec<&'static str>, i32)> {
    vec![
        ("document-generate", vec!["document", "generate", PM], 0),
        ("document-claims", vec!["document", "claims", PM], 0),
        (
            "db-import",
            vec![
                "db",
                "import",
                "examples/db/minimal_import.sql",
                "--name",
                "ImportedDb",
            ],
            0,
        ),
        ("domain-expand", vec!["domain", "expand", DOMAIN], 0),
        (
            "domain-testgen",
            vec!["domain", "testgen", DOMAIN, "--target", "vitest"],
            0,
        ),
        (
            "testgen",
            vec![
                "testgen",
                "specs/cart_v1.fsl",
                "--depth",
                "3",
                "--target",
                "pytest",
            ],
            0,
        ),
        (
            "html",
            vec![
                "html",
                "rust/fslc/tests/fixtures/sweep_clean.fsl",
                "--no-timing",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
            ],
            0,
        ),
        (
            "ledger",
            vec![
                "ledger",
                "rust/fslc/tests/fixtures/sweep_clean.fsl",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
            ],
            0,
        ),
        (
            "cex",
            vec![
                "counterexample",
                "export",
                "rust/fslc/tests/fixtures/sweep_violating.fsl",
                "--depth",
                "4",
            ],
            1,
        ),
    ]
}

// This common publication contract is deliberately exercised at every caller,
// not satisfied merely by a helper's tests or the number of command families.
#[cfg(unix)]
fn single_file_caller(label: &str) {
    use std::os::unix::fs::{MetadataExt, symlink};
    let (label, base_arguments, status) = families()
        .into_iter()
        .find(|(name, _, _)| *name == label)
        .unwrap();
    let home = scratch(label);
    let out = home.join("out/artifact");
    let argv = args_with_output(&base_arguments, &out);
    let first = run(&home, &root(), &argv, None);
    json(&first, status);
    let healthy = std::fs::read(&out).unwrap();
    assert!(!healthy.is_empty());
    std::fs::write(&out, OLD).unwrap();
    let alias = home.join("out/alias");
    std::fs::hard_link(&out, &alias).unwrap();
    let inode = std::fs::metadata(&alias).unwrap().ino();
    let second = run(&home, &root(), &argv, None);
    json(&second, status);
    if label == "cex" {
        let before = cex_timing::Packet::new(
            first.status.code().expect("normal CLI exit"),
            first.stdout,
            first.stderr,
            healthy,
        );
        let after = cex_timing::Packet::new(
            second.status.code().expect("normal CLI exit"),
            second.stdout,
            second.stderr,
            std::fs::read(&out).unwrap(),
        );
        cex_timing::agree(&before, &after).expect("cex: only nine measured durations may vary");
    } else {
        assert_eq!(
            std::fs::read(&out).unwrap(),
            healthy,
            "{label}: full healthy bytes"
        );
    }
    assert_eq!(
        std::fs::read(&alias).unwrap(),
        OLD,
        "{label}: alias keeps old inode"
    );
    assert_eq!(std::fs::metadata(&alias).unwrap().ino(), inode);
    assert_ne!(std::fs::metadata(&out).unwrap().ino(), inode);
    std::fs::remove_file(&out).unwrap();
    symlink(&alias, &out).unwrap();
    let before = cli::state(&home.join("out"));
    let failure = run(&home, &root(), &argv, None);
    if label == "testgen" {
        cli::testgen_io(&failure, cli::TestgenIoCase::FinalNonregular);
    } else {
        io(&failure);
    }
    assert_eq!(
        cli::state(&home.join("out")),
        before,
        "{label}: link, target, bytes, modes and siblings"
    );
    let missing = home.join("out/missing/artifact");
    let failure = run(
        &home,
        &root(),
        &args_with_output(&base_arguments, &missing),
        None,
    );
    if label == "testgen" {
        cli::testgen_io(&failure, cli::TestgenIoCase::MissingParent);
    } else {
        io(&failure);
    }
    assert!(
        !missing.parent().unwrap().exists(),
        "{label}: preserve no-parent-creation policy"
    );
    assert_eq!(cli::state(&home.join("out")), before);
}

#[test]
fn single_file_document_generate() {
    single_file_caller("document-generate");
}
#[test]
fn single_file_document_claims() {
    single_file_caller("document-claims");
}
#[test]
fn single_file_db_import() {
    single_file_caller("db-import");
}
#[test]
fn single_file_domain_expand() {
    single_file_caller("domain-expand");
}
#[test]
fn single_file_domain_testgen() {
    single_file_caller("domain-testgen");
}
#[test]
fn single_file_testgen() {
    single_file_caller("testgen");
}
#[test]
fn single_file_html() {
    single_file_caller("html");
}
#[test]
fn single_file_ledger() {
    single_file_caller("ledger");
}
#[test]
fn single_file_cex() {
    single_file_caller("cex");
}

#[cfg(target_os = "linux")]
#[test]
fn html_healthy_output_is_byte_identical_across_replacements() {
    let home = scratch("healthy");
    let out = home.join("out/artifact.html");
    let (_, base_arguments, _) = families()
        .into_iter()
        .find(|(name, _, _)| *name == "html")
        .unwrap();
    let argv = args_with_output(&base_arguments, &out);
    let first = run(&home, &root(), &argv, None);
    json(&first, 0);
    let expected = std::fs::read(&out).unwrap();
    std::fs::write(&out, OLD).unwrap();
    let second = run(&home, &root(), &argv, None);
    json(&second, 0);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
    assert_eq!(std::fs::read(&out).unwrap(), expected);
    assert_eq!(names(&home.join("out")), vec![out]);
}

/// A kill during the write can leave the temporary file, but its partial bytes are
/// never wider than the owner, whether or not the destination existed.
#[cfg(target_os = "linux")]
#[test]
fn abrupt_termination_leaves_only_a_private_temporary_file() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    let (_, base_arguments, _) = families()
        .into_iter()
        .find(|(name, _, _)| *name == "html")
        .unwrap();
    for existing in [true, false] {
        let home = scratch(if existing {
            "abrupt-existing"
        } else {
            "abrupt-new"
        });
        let out = home.join("out/artifact.html");
        let argv = args_with_output(&base_arguments, &out);
        json(&run(&home, &root(), &argv, None), 0);
        assert!(
            std::fs::read(&out).unwrap().len() > 1024,
            "healthy caller must exceed child filesize limit"
        );
        let old = vec![b'O'; 8192];
        if existing {
            std::fs::write(&out, &old).unwrap();
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o600)).unwrap();
        } else {
            std::fs::remove_file(&out).unwrap();
        }
        // SIGXFSZ is left at its default, so the write ends the process.
        let killed = run(
            &home,
            &root(),
            &argv,
            Some("ulimit -f 1 || exit 97; exec \"$@\""),
        );
        assert_eq!(killed.status.code(), None, "{killed:?}");
        assert!(killed.status.signal().is_some(), "{killed:?}");
        let left: Vec<_> = names(&home.join("out"))
            .into_iter()
            .filter(|path| path != &out)
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
        let name = left[0].file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with(".fslc-output-"), "{name}");
        let metadata = std::fs::metadata(&left[0]).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert!(metadata.len() <= 1024, "partial bytes only");
        if existing {
            assert_eq!(std::fs::read(&out).unwrap(), old);
        } else {
            assert!(!out.exists());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn html_actual_partial_write_failure_preserves_old_bytes_and_cleans_scratch() {
    let home = scratch("partial");
    let out = home.join("out/artifact.html");
    let (_, base_arguments, _) = families()
        .into_iter()
        .find(|(name, _, _)| *name == "html")
        .unwrap();
    let argv = args_with_output(&base_arguments, &out);
    let healthy = run(&home, &root(), &argv, None);
    json(&healthy, 0);
    let expected = std::fs::read(&out).unwrap();
    assert!(
        expected.len() > 1024,
        "healthy caller must exceed child filesize limit"
    );
    let old = vec![b'O'; 8192]; // created by unrestricted parent, before child limit
    std::fs::write(&out, &old).unwrap();
    let before = cli::state(&home.join("out"));
    let failure = run(
        &home,
        &root(),
        &argv,
        Some("trap '' XFSZ; ulimit -f 1 || exit 97; exec \"$@\""),
    );
    let error = io(&failure);
    assert!(
        error["message"].as_str().unwrap().contains("os error 27"),
        "must reach EFBIG write, not parser/early IO: {error}"
    );
    assert!(
        !std::fs::read(&out).unwrap().is_empty(),
        "reject empty-only failure as partial-write evidence"
    );
    // On old direct-write source, this assertion must RED with actual truncated bytes.
    assert_eq!(
        std::fs::read(&out).unwrap(),
        old,
        "partial-write preservation detector"
    );
    assert_eq!(
        cli::state(&home.join("out")),
        before,
        "failed publish leaves no owned scratch"
    );
    let recovered = run(&home, &root(), &argv, None);
    json(&recovered, 0);
    assert_eq!(
        healthy.stdout, recovered.stdout,
        "full unmasked healthy envelope"
    );
    assert_eq!(healthy.stderr, recovered.stderr);
    assert_eq!(std::fs::read(&out).unwrap(), expected);
    std::fs::remove_file(&out).unwrap();
    let error = io(&run(
        &home,
        &root(),
        &argv,
        Some("trap '' XFSZ; ulimit -f 1 || exit 97; exec \"$@\""),
    ));
    assert!(error["message"].as_str().unwrap().contains("os error 27"));
    assert!(!out.exists(), "failed new output is not partially visible");
    assert!(names(&home.join("out")).is_empty(), "owned scratch cleaned");
}

#[cfg(unix)]
#[test]
fn regular_modes_and_child_umask_are_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let home = scratch("modes");
    let out = home.join("out/artifact");
    let argv = args_with_output(&["document", "generate", PM], &out);
    json(
        &run(&home, &root(), &argv, Some("umask 027; exec \"$@\"")),
        0,
    );
    assert_eq!(
        std::fs::metadata(&out).unwrap().permissions().mode() & 0o777,
        0o640
    );
    // Only the permission bits carry over: replacing a setuid file must not leave
    // new content setuid, which a direct write would have cleared too.
    for (mode, expected) in [
        (0o604, 0o604),
        (0o1640, 0o640),
        (0o2640, 0o640),
        (0o4640, 0o640),
        (0o4755, 0o755),
    ] {
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            std::fs::metadata(&out).unwrap().permissions().mode() & 0o7777,
            mode,
            "filesystem mode precondition"
        );
        json(
            &run(&home, &root(), &argv, Some("umask 077; exec \"$@\"")),
            0,
        );
        assert_eq!(
            std::fs::metadata(&out).unwrap().permissions().mode() & 0o7777,
            expected,
            "from {mode:o}"
        );
    }
    assert_eq!(names(&home.join("out")), vec![out]);
}

#[cfg(unix)]
#[test]
fn readonly_target_and_unwritable_parent_fail_under_nonprivileged_permissions() {
    use std::fs::{OpenOptions, Permissions};
    use std::os::unix::fs::PermissionsExt;
    let home = scratch("permissions");
    let dir = home.join("out");
    let out = dir.join("artifact");
    let argv = args_with_output(&["document", "generate", PM], &out);
    std::fs::write(&out, OLD).unwrap();
    std::fs::set_permissions(&out, Permissions::from_mode(0o444)).unwrap();
    let precondition = OpenOptions::new().write(true).open(&out);
    if precondition.is_ok() {
        std::fs::set_permissions(&out, Permissions::from_mode(0o644)).unwrap();
        panic!("qualification requires nonprivileged process without DAC override; not a skip");
    }
    assert_eq!(
        precondition.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    let before = cli::state(&dir);
    let failure = run(&home, &root(), &argv, None);
    let after = cli::state(&dir);
    std::fs::set_permissions(&out, Permissions::from_mode(0o644)).unwrap();
    io(&failure);
    assert_eq!(after, before);
    std::fs::set_permissions(&dir, Permissions::from_mode(0o555)).unwrap();
    let probe = std::fs::File::create(dir.join("permission-probe"));
    let failure = if probe.is_err() {
        Some(run(&home, &root(), &argv, None))
    } else {
        None
    };
    std::fs::set_permissions(&dir, Permissions::from_mode(0o755)).unwrap();
    if probe.is_ok() {
        std::fs::remove_file(dir.join("permission-probe")).unwrap();
    }
    assert_eq!(
        probe.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied,
        "nonprivileged parent-write precondition"
    );
    io(&failure.unwrap());
    assert_eq!(std::fs::read(&out).unwrap(), OLD);
    assert_eq!(names(&dir), vec![out]);
}

#[cfg(target_os = "linux")]
#[test]
fn fifo_and_directory_are_rejected_without_replacement() {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let home = scratch("nonregular");
    let out = home.join("out/artifact");
    let base_arguments = [
        "db",
        "import",
        "examples/db/minimal_import.sql",
        "--name",
        "ImportedDb",
    ];
    let argv = args_with_output(&base_arguments, &out);
    json(&run(&home, &root(), &argv, None), 0);
    assert!(
        std::fs::read(&out).unwrap().len() < 4096,
        "old-source FIFO write must fit pipe capacity"
    );
    std::fs::remove_file(&out).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&out)
            .status()
            .unwrap()
            .success()
    );
    // Linux O_RDWR FIFO open prevents old-source blocking without a competing reader.
    let _keep_open = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&out)
        .unwrap();
    let before = cli::state(&home.join("out"));
    io(&run(&home, &root(), &argv, None));
    assert!(
        std::fs::symlink_metadata(&out)
            .unwrap()
            .file_type()
            .is_fifo()
    );
    assert_eq!(cli::state(&home.join("out")), before);
    assert_eq!(
        std::fs::symlink_metadata(&out).unwrap().ino(),
        before["artifact"]["inode"].as_u64().unwrap()
    );
    std::fs::remove_file(&out).unwrap();
    std::fs::create_dir(&out).unwrap();
    let before = cli::state(&home.join("out"));
    io(&run(&home, &root(), &argv, None));
    assert_eq!(cli::state(&home.join("out")), before);
}

/// Every non-regular destination is refused, including ones a caller might use
/// to discard output or read it through process substitution.
#[cfg(unix)]
#[test]
fn dev_null_is_refused_as_a_non_regular_destination() {
    use std::os::unix::fs::FileTypeExt;
    let home = scratch("dev-null");
    let argv = args_with_output(&["document", "generate", PM], Path::new("/dev/null"));
    let error = io(&run(&home, &root(), &argv, None));
    assert_eq!(error["message"], "output target is not a regular file");
    assert!(
        std::fs::symlink_metadata("/dev/null")
            .unwrap()
            .file_type()
            .is_char_device()
    );
}

#[cfg(unix)]
#[test]
fn domain_files_publish_in_order_without_rolling_back_completed_files() {
    use std::os::unix::fs::symlink;
    let home = scratch("domain-order");
    let out = home.join("out");
    let base_arguments: Vec<String> = ["domain", "generate", DOMAIN, "--target", "typescript"]
        .map(str::to_owned)
        .to_vec();
    let manifest = json(&run(&home, &root(), &base_arguments, None), 0);
    let files: Vec<_> = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_owned(),
                f["content"].as_str().unwrap().as_bytes().to_vec(),
            )
        })
        .collect();
    assert!(
        files.len() >= 2,
        "fixture must exercise actual multi-file publication"
    );
    let argv = args_with_output(
        &["domain", "generate", DOMAIN, "--target", "typescript"],
        &out,
    );
    json(&run(&home, &root(), &argv, None), 0);
    for (relative, content) in &files {
        assert_eq!(
            std::fs::read(out.join(relative)).unwrap(),
            *content,
            "domain still creates required parent directories"
        );
    }
    for stop in 0..files.len() {
        for (relative, _) in &files {
            let path = out.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            if std::fs::symlink_metadata(&path).is_ok() {
                std::fs::remove_file(&path).unwrap();
            }
            std::fs::write(path, OLD).unwrap();
        }
        let target = home.join("blocked-target");
        std::fs::write(&target, OLD).unwrap();
        let blocked = out.join(&files[stop].0);
        std::fs::remove_file(&blocked).unwrap();
        symlink(&target, &blocked).unwrap();
        let before = cli::state(&out);
        io(&run(&home, &root(), &argv, None));
        assert_eq!(std::fs::read_link(&blocked).unwrap(), target);
        assert_eq!(std::fs::read(&target).unwrap(), OLD);
        for (index, (relative, content)) in files.iter().enumerate() {
            assert_eq!(
                std::fs::read(out.join(relative)).unwrap(),
                if index < stop {
                    content.as_slice()
                } else {
                    OLD
                },
                "order at {relative}, blocked {stop}"
            );
        }
        // Reconstruct expected state from observed unchanged suffix + new prefix;
        // remove only prefix entries, not any fields inside an output envelope.
        let mut after = cli::state(&out);
        let mut before = before;
        for (relative, _) in files.iter().take(stop) {
            remove_file_entry(&mut before, Path::new(relative));
            remove_file_entry(&mut after, Path::new(relative));
        }
        assert_eq!(after, before, "suffix metadata/bytes and no leaked scratch");
    }
    // Final repair must publish every complete file, not return a spurious success.
    let last = out.join(&files.last().unwrap().0);
    std::fs::remove_file(last).unwrap();
    json(&run(&home, &root(), &argv, None), 0);
    for (relative, content) in files {
        assert_eq!(std::fs::read(out.join(relative)).unwrap(), content);
    }
}

#[cfg(unix)]
fn remove_file_entry(state: &mut serde_json::Value, relative: &Path) {
    let components: Vec<_> = relative.iter().map(|p| p.to_str().unwrap()).collect();
    let mut current = state;
    for component in &components[..components.len() - 1] {
        current = &mut current[*component]["directory"];
    }
    assert!(
        current
            .as_object_mut()
            .unwrap()
            .remove(*components.last().unwrap())
            .is_some()
    );
}
