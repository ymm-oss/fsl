// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `fsl-bfs` reads an edition header (#1434). Before #1434 it refused
//! `fsl 4` / `fsl 5` as the unknown declaration `fsl` even with the gate on;
//! with the published edition, a headed spec gives the same BFS result as
//! the same spec without a header. An unknown edition is still refused.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SPEC: &str = "spec S {\n  state { n: 0..2 = 0 }\n  action inc() { requires n < 2  n = n + 1 }\n  invariant Bound { n <= 2 }\n}\n";

fn scratch() -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fsl_bfs_edition_header");
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn bfs(directory: &Path, name: &str, source: &str) -> Output {
    let path = directory.join(name);
    std::fs::write(&path, source).expect("write spec");
    Command::new(env!("CARGO_BIN_EXE_fsl-bfs"))
        .arg(&path)
        .arg("3")
        .output()
        .expect("run fsl-bfs")
}

#[test]
fn a_headed_spec_explores_as_the_headerless_one() {
    let directory = scratch();
    let plain = bfs(&directory, "plain.fsl", SPEC);
    assert_eq!(plain.status.code(), Some(0), "{plain:?}");
    assert!(!plain.stdout.is_empty());
    for header in ["fsl 4", "fsl 5"] {
        let headed = bfs(
            &directory,
            &format!("{}.fsl", header.replace(' ', "")),
            &format!("{header}\n{SPEC}"),
        );
        assert_eq!(headed.status.code(), Some(0), "{header}: {headed:?}");
        assert_eq!(headed.stdout, plain.stdout, "{header}");
    }
    let unknown = bfs(&directory, "fsl6.fsl", &format!("fsl 6\n{SPEC}"));
    assert_eq!(unknown.status.code(), Some(2), "{unknown:?}");
    assert!(unknown.stdout.is_empty(), "{unknown:?}");
    let message = String::from_utf8_lossy(&unknown.stderr);
    assert!(message.contains("fsl 6"), "{message}");
}
