// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn directory(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = super::cli::repository_root()
        .join("rust/target/ledger-r3")
        .join(format!(
            "{label}-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    assert!(!path.exists());
    std::fs::create_dir_all(&path).expect("create worktree-local test directory");
    path
}

pub fn annotated_fixture(fixture: &str, declaration: &str) -> String {
    let source = std::fs::read_to_string(super::cli::repository_root().join(fixture)).unwrap();
    assert_eq!(source.matches(declaration).count(), 1);
    source.replacen(
        declaration,
        &format!("@requirement(\"REQ-R3\", \"review obligation\")\n  {declaration}"),
        1,
    )
}
