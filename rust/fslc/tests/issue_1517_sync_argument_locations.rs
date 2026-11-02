// SPDX-License-Identifier: Apache-2.0

#[path = "support/issue_1517_sync_argument_cases.rs"]
mod forms;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn hash(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).expect("hash input")))
}

#[allow(clippy::too_many_lines)] // Keep complete packet assertions beside raw receipts.
fn run(id: &str) {
    let case = forms::cases()
        .into_iter()
        .find(|c| c.id == id)
        .expect("case");
    let root = std::env::var_os("FSLC_1517_RECEIPTS")
        .map_or_else(
            || std::env::temp_dir().join(format!("issue-1517-{}", std::process::id())),
            std::path::PathBuf::from,
        )
        .join(id);
    fs::create_dir_all(&root).expect("scratch");
    for (file, source) in [
        ("root.fsl", &case.source),
        ("piece.fsl", &case.piece),
        ("good.fsl", &forms::GOOD.to_owned()),
    ] {
        fs::write(root.join(file), source).expect("fixture");
    }
    let versions = fsl_core::version_metadata(
        "fslc-rust",
        env!("CARGO_PKG_VERSION"),
        "native-z3",
        fsl_solver_z3::version(),
    );
    let expected = if let Some((line, column, message)) = &case.error {
        json!({"fsl":"1.0", "result":"error", "kind":"semantics", "message":message,
            "loc":{"line":line,"column":column}, "versions":versions})
    } else {
        json!({"fsl":"1.0", "result":"ok", "spec":case.spec,
            "warnings":[{"kind":"no_user_invariants","message":"spec declares no user invariants (only implicit type bounds are checked)"}], "versions":versions})
    };
    let routes: &[&str] = if case.error.is_some() {
        &["check", "bmc", "explicit"]
    } else {
        &["check"]
    };
    let mut failures = Vec::new();
    for route in routes {
        let args = if *route == "check" {
            vec!["check", "root.fsl"]
        } else {
            vec![
                "verify",
                "root.fsl",
                "--engine",
                route,
                "--depth",
                "3",
                "--no-cache",
            ]
        };
        let binary = Path::new(env!("CARGO_BIN_EXE_fslc"));
        let output = Command::new(binary)
            .args(&args)
            .current_dir(&root)
            .env("FSLC_CACHE", "off")
            .env_remove("FSLC_UNSTABLE")
            .output()
            .expect("CLI");
        fs::write(root.join(format!("{route}.stdout")), &output.stdout).expect("stdout");
        fs::write(root.join(format!("{route}.stderr")), &output.stderr).expect("stderr");
        let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
        let code = if case.error.is_some() { 2 } else { 0 };
        let pass =
            output.status.code() == Some(code) && output.stderr.is_empty() && report == expected;
        let command_line: Vec<_> = std::iter::once(binary.to_str().unwrap())
            .chain(args.iter().copied())
            .collect();
        let receipt = json!({"id":id,"argv":command_line,"cwd":root,
            "binary_sha256":hash(binary),"head":std::env::var("FSLC_1517_HEAD").ok(),"active_test_patch":std::env::var("FSLC_1517_PATCH").ok(),
            "sources_sha256":[hash(&root.join("root.fsl")),hash(&root.join("piece.fsl")),hash(&root.join("good.fsl"))],
            "env":std::env::vars().filter(|(k,_)| k.starts_with("RUST") || k.starts_with("CARGO") || k.starts_with("FSLC") || k == "PATH" || k == "TMPDIR").collect::<std::collections::BTreeMap<_,_>>(),
            "env_overrides":{"FSLC_CACHE":"off","FSLC_UNSTABLE":null},"exit":output.status.code(),
            "expected_exit":code,"expected":expected,"packet":report,"pass":pass});
        fs::write(
            root.join(format!("{route}.json")),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .expect("receipt");
        if !pass {
            failures.push(format!(
                "{id}/{route}: exit={:?}; actual={report}; expected={expected}",
                output.status.code()
            ));
        }
    }
    if case.error.is_none() {
        let kernel = fsl_core::parse_kernel_source(&case.source, &fsl_core::FsResolver::new(&root))
            .expect("kernel");
        fs::write(
            root.join("kernel-ast-v1.json"),
            serde_json::to_vec_pretty(&kernel.kernel_ast_v1()).unwrap(),
        )
        .expect("AST");
        let model = fsl_core::build_model(kernel).expect("model");
        fs::write(root.join("model.debug"), format!("{model:#?}")).expect("whole model");
        if id == "valid_zero" {
            let span = model
                .actions
                .iter()
                .find(|a| a.name == "step")
                .expect("step")
                .span;
            assert_eq!((span.start.line, span.start.column), (4, 3));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

macro_rules! cases {
    ($($name:ident),+ $(,)?) => { $(#[test] fn $name() { run(stringify!($name)); })+ };
}
#[rustfmt::skip] // Keep the finite independent-test inventory compact.
cases!(root_bool, root_index, root_pad_bool, root_pad_index, piece_pad_bool, piece_pad_index,
    second_bool, second_index, second_actual_bool, second_actual_index, component_bool,
    component_index, component_alone, valid_zero, numeric_out_of_range, root_parameter,
    bounded_parameter, qualified_state, forward_constant, forward_root_state, nested_actual,
    unused_valid, forall_capture, parameter_range, let_capture, current_key);
