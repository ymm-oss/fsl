// SPDX-License-Identifier: Apache-2.0

#[path = "support/issue_1517_sync_argument_cases.rs"]
mod forms;

use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

fn fixture(id: &str) -> (forms::Case, Option<&'static str>) {
    let base = match id {
        "unused_bool" | "phase_alias" | "phase_arity" => "root_bool",
        "unused_index" => "root_index",
        "unused_valid" => "unused_valid",
        "numeric_carrier" => "bounded_parameter",
        "numeric_literal" => "numeric_out_of_range",
        _ => panic!("unknown finite case {id}"),
    };
    let mut case = forms::cases().into_iter().find(|c| c.id == base).unwrap();
    if id.starts_with("unused_") {
        case.piece = case.piece.replace("last = a", "last = 0");
    }
    let phase = match id {
        "phase_alias" => Some((
            "part.put(true) || missing.put(0)",
            "unknown alias 'missing' at 4:3",
        )),
        "phase_arity" => {
            case.source = case.source.replace(
                "compose Root {\n",
                "compose Root {\n  use Good as good from \"good.fsl\"\n",
            );
            Some((
                "part.put(true) || good.idle(0)",
                "sync reference 'good.idle' expects 0 argument(s), got 1 at 5:3",
            ))
        }
        _ => None,
    };
    if let Some((refs, _)) = phase {
        case.source = case.source.replace("part.put(true)", refs);
    }
    (case, phase.map(|(_, message)| message))
}

fn run(id: &str) {
    let (case, phase) = fixture(id);
    let inputs = std::env::var_os("FSLC_1517_D_INPUTS")
        .map_or_else(
            || std::env::temp_dir().join(format!("issue-1517-precheck-{}", std::process::id())),
            PathBuf::from,
        )
        .join("native")
        .join(id);
    fs::create_dir_all(&inputs).unwrap();
    for (name, text) in [
        ("root.fsl", case.source.as_str()),
        ("piece.fsl", &case.piece),
        ("good.fsl", forms::GOOD),
    ] {
        fs::write(inputs.join(name), text).unwrap();
    }
    let versions = fsl_core::version_metadata(
        "fslc-rust",
        env!("CARGO_PKG_VERSION"),
        "native-z3",
        fsl_solver_z3::version(),
    );
    let expected = if let Some(message) = phase {
        let line = if id == "phase_alias" { 4 } else { 5 };
        json!({"fsl":"1.0","result":"error","kind":"semantics","message":message,
            "loc":{"line":line,"column":3},"versions":versions})
    } else if let Some((line, column, message)) = &case.error {
        json!({"fsl":"1.0","result":"error","kind":"semantics","message":message,
            "loc":{"line":line,"column":column},"versions":versions})
    } else {
        json!({"fsl":"1.0","result":"ok","spec":case.spec,"warnings":[{
            "kind":"no_user_invariants","message":"spec declares no user invariants (only implicit type bounds are checked)"}],"versions":versions})
    };
    let rejecting = phase.is_some() || case.error.is_some();
    let routes: &[&str] = if rejecting {
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
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(&args)
            .current_dir(&inputs)
            .env("FSLC_CACHE", "off")
            .env_remove("FSLC_UNSTABLE")
            .output()
            .expect("CLI");
        let actual: Value = serde_json::from_slice(&output.stdout).expect("full CLI packet");
        let code = if rejecting { 2 } else { 0 };
        let pass =
            actual == expected && output.status.code() == Some(code) && output.stderr.is_empty();
        if let Some(receipts) = std::env::var_os("FSLC_1517_D_RECEIPTS") {
            let dest = PathBuf::from(receipts).join("native").join(id);
            fs::create_dir_all(&dest).unwrap();
            fs::write(dest.join(format!("{route}.stdout")), &output.stdout).unwrap();
            fs::write(dest.join(format!("{route}.stderr")), &output.stderr).unwrap();
            fs::write(dest.join(format!("{route}.json")), serde_json::to_vec_pretty(&json!({
                "id":id,"argv":args,"cwd":inputs,"source":case.source,"piece":case.piece,
                "actual":actual,"expected":expected,"exit":output.status.code(),"expected_exit":code,"pass":pass})).unwrap()).unwrap();
        }
        if !pass {
            failures.push(format!(
                "{id}/{route}: exit={:?}; actual={actual}; expected={expected}",
                output.status.code()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

macro_rules! cases {
    ($($name:ident),+ $(,)?) => { $(#[test] fn $name() { run(stringify!($name)); })+ };
}
cases!(
    unused_bool,
    unused_index,
    unused_valid,
    numeric_carrier,
    numeric_literal,
    phase_alias,
    phase_arity
);
