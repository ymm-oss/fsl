// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use fsl_lsp::DocumentIndex;

fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(path).expect("read corpus directory") {
        let path = entry.expect("read corpus entry").path();
        if path.is_dir() {
            if !path.to_string_lossy().contains("gallery/errors") {
                collect(&path, files);
            }
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("fsl") {
            files.push(path);
        }
    }
}

#[test]
fn valid_corpus_is_parsed_and_every_identifier_is_indexed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let mut files = Vec::new();
    collect(&root.join("specs"), &mut files);
    collect(&root.join("examples"), &mut files);
    files.sort();
    assert!(!files.is_empty());

    let mut failures = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(&path).expect("read corpus source");
        match DocumentIndex::build(&source, path.to_str()) {
            Ok(index) => {
                let missing = index.unindexed_identifiers();
                if !missing.is_empty() {
                    failures.push(format!(
                        "{}: unindexed {}",
                        path.display(),
                        missing.join(", ")
                    ));
                }
            }
            Err(error) => failures.push(format!("{}: {error}", path.display())),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
