// SPDX-License-Identifier: Apache-2.0

//! Source contract for #1251: `fslc mutate` tells an oracle failure from a
//! finding only through `VerifyError::is_solver_failure`, so every place the
//! verifier turns a solver `unknown` into an error must build it with
//! `VerifyError::solver`. An unmarked `VerifyError::new("solver returned
//! unknown …")` would silently become a `build_spec` kill again.

use std::path::Path;

fn sources() -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut pending = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).expect("read source");
                files.push((path.display().to_string(), text));
            }
        }
    }
    files
}

fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn no_unknown_solver_result_is_built_as_an_unmarked_error() {
    for (path, text) in sources() {
        let code = squeeze(&text);
        for unmarked in [
            "VerifyError::new(\"solverreturnedunknown",
            "VerifyError::new(format!(\"solverreturnedunknown",
        ] {
            assert!(
                !code.contains(unmarked),
                "{path}: a solver `unknown` must be built with VerifyError::solver"
            );
        }
        let literals = code.matches("\"solverreturnedunknown").count();
        let marked = code
            .matches("VerifyError::solver(\"solverreturnedunknown")
            .count()
            + code
                .matches("VerifyError::solver(format!(\"solverreturnedunknown")
                .count();
        assert_eq!(
            literals, marked,
            "{path}: every \"solver returned unknown\" message must be a VerifyError::solver"
        );
    }
}

#[test]
fn every_unknown_arm_builds_a_solver_failure() {
    let mut arms = 0;
    for (path, text) in sources() {
        let lines = text.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || !trimmed.contains("SatResult::Unknown") {
                continue;
            }
            arms += 1;
            let window = lines[index..lines.len().min(index + 4)].join("\n");
            assert!(
                window.contains("VerifyError::solver("),
                "{path}:{}: a `SatResult::Unknown` arm must build VerifyError::solver",
                index + 1
            );
        }
    }
    assert!(arms > 0, "the contract must find the unknown arms it pins");
}
