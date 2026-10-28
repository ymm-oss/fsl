// SPDX-License-Identifier: Apache-2.0

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::path::PathBuf;

pub fn unique(path: PathBuf) -> PathBuf {
    let mut name = path.into_os_string();
    name.push(format!("-{}", RandomState::new().hash_one(())));
    name.into()
}
