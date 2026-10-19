// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! R0 (fsl 5 plan §3 S0): the unstable gate changes nothing for documents
//! written in the published language. For every `.fsl` file under `specs/`
//! and `examples/` (the corpus walk shared with `corpus_check_sweep.rs`),
//! `fslc kernel` (positions included) and `fslc check` print the same bytes
//! and exit with the same status with the gate off and on. Every stage behind
//! the gate must keep this green; it is the merge condition that keeps 4.x
//! releases from `main` byte-identical while the next edition is built.

use std::path::Path;
use std::process::Command;

#[path = "support/mod.rs"]
mod support;
use support::{corpus_files, repo_relative, root};

/// `(stdout, stderr, exit)` of `fslc <arguments>` at the repository root,
/// with the gate unset or set to `unstable`.
fn run(arguments: &[&str], path: &Path, unstable: Option<&str>) -> (Vec<u8>, Vec<u8>, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args(arguments)
        .arg(path)
        .current_dir(root())
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    if let Some(features) = unstable {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, features);
    }
    let output = command.output().expect("run fslc");
    let exit = output
        .status
        .code()
        .unwrap_or_else(|| panic!("fslc {arguments:?} {} died by signal", path.display()));
    (output.stdout, output.stderr, exit)
}

#[test]
fn gate_on_and_off_agree_on_kernel_and_check_for_the_whole_corpus() {
    let root = root();
    let files = corpus_files(&root);
    assert!(
        files.len() > 150,
        "corpus scan floor: found only {} .fsl files under specs/+examples/",
        files.len()
    );

    let mut failures = Vec::new();
    let mut kernels = 0;
    for path in &files {
        let rel = repo_relative(&root, path);
        for arguments in [&["kernel"][..], &["check"][..]] {
            let off = run(arguments, path, None);
            let on = run(arguments, path, Some("fsl5"));
            if off != on {
                failures.push(format!(
                    "{rel}: `fslc {}` differs with the gate on (exit {} -> {})",
                    arguments.join(" "),
                    off.2,
                    on.2
                ));
            }
            if arguments == ["kernel"] && off.2 == 0 {
                kernels += 1;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Not vacuous: most of the corpus must actually produce a kernel, so the
    // positions and lowering are compared, not only matching error texts.
    assert!(
        kernels * 2 > files.len(),
        "only {kernels} of {} corpus files produced a kernel",
        files.len()
    );
}
