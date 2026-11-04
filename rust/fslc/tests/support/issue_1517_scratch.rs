// SPDX-License-Identifier: Apache-2.0

//! Scratch input directories for the #1517 tests.

use std::{
    fs,
    path::{Path, PathBuf},
};

pub struct Scratch {
    path: PathBuf,
    removes_on_drop: bool,
}

impl Scratch {
    /// A directory named by `retain_var` belongs to the caller and is kept for
    /// its receipts. Otherwise this test owns a fresh temporary directory and
    /// removes it on every exit, including a failed assertion.
    pub fn new(retain_var: &str, retained: &str, label: &str) -> Self {
        let scratch = std::env::var_os(retain_var).map_or_else(
            || Self {
                path: std::env::temp_dir()
                    .join(format!("issue-1517-{label}-{}", std::process::id())),
                removes_on_drop: true,
            },
            |root| Self {
                path: PathBuf::from(root).join(retained),
                removes_on_drop: false,
            },
        );
        fs::create_dir_all(&scratch.path).expect("scratch");
        scratch
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if self.removes_on_drop {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
