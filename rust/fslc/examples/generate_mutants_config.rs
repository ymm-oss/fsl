// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    fslc_rust::mutants_config::regenerate(root)
        .unwrap_or_else(|error| panic!("mutants-config: FAIL -- {error}"));
    println!("mutants-config: regenerated rust/.cargo/mutants.toml");
}
