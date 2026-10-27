// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The `acceptance` / `forbidden` replay that `check` runs before proof must
//! not grow with the `verify { values }` scope of an action parameter.
//!
//! Each trace step was matched by listing every enabled instance of every
//! action (`Monitor::enabled`), so `submit(c, a)` over `values Amount =
//! 0..2000000` held two million instances per step and `check` was killed
//! for memory. A requirements document without traces checks in a few MB on
//! the same scope; the replay now collects only the instance a step names.
//!
//! The replay still evaluates every instance's guards, so its verdicts and
//! its errors are those of the full listing: the second test holds each
//! trace outcome fixed between a small and a large scope.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// A scratch directory under the test target, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("requirement_trace_large_domain")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        Self(directory)
    }

    fn write(&self, source: &str) -> PathBuf {
        let path = self.0.join("claims.fsl");
        std::fs::write(&path, source).expect("write scratch source");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A requirements document whose `submit` takes an amount from
/// `values Amount = 0..{hi}`, with `traces` as its `acceptance` / `forbidden`
/// members.
fn claims(hi: u32, traces: &str) -> String {
    format!(
        "requirements Claims {{\n  number Amount\n\n  \
         process Claim with amount: Amount = 0 {{\n    \
         stages Draft, Submitted, Approved, Rejected\n    initial Draft\n\n    \
         transition submit  Draft     -> Submitted by Employee\n      \
         with a: Amount\n      set amount = a\n    \
         transition approve Submitted -> Approved  by Manager\n      when amount > 1\n    \
         transition reject  Submitted -> Rejected  by Manager\n  }}\n\n\
         {traces}\n}}\nverify {{\n  instances Claim = 1\n  values Amount = 0..{hi}\n}}\n"
    )
}

/// The same shape in the next-edition requirements surface.
#[cfg(target_os = "linux")]
fn claims_next_edition(hi: u32) -> String {
    format!(
        "fsl 5\nrequirements R {{\n  number Amount\n  actor Employee, Manager\n  \
         entity Claim {{\n    amount: Amount = 0\n    \
         lifecycle Draft -> Submitted -> Approved\n    lifecycle Submitted -> Rejected\n  }}\n  \
         event submit = Employee submits Claim with amount Draft -> Submitted\n  \
         event approve = Manager approves Claim Submitted -> Approved\n  \
         event reject = Manager rejects Claim Submitted -> Rejected\n  \
         forbidden FB-1 \"a draft is not approved\" {{\n    Manager approves claim1\n    \
         then rejected\n  }}\n}}\nverify {{\n  instances Claim = 1\n  values Amount = 0..{hi}\n}}\n"
    )
}

/// One forbidden trace whose setup step submits the scope's top value
/// (`TOP`), and whose final step (a second submit) the lifecycle rejects.
const TOP_OF_SCOPE: &str = "  forbidden FB-1 \"a claim is submitted once\" { submit(0, TOP) submit(0, 1) expect rejected }";

/// `traces` with `TOP` read as `hi`, the top of `values Amount = 0..hi`.
fn traces_at(traces: &str, hi: u32) -> String {
    traces.replace("TOP", &hi.to_string())
}

fn fslc_check(path: &Path) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("check")
        .arg(path)
        .output()
        .expect("run fslc check");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit code"))
}

/// On a two-million-value scope the replay listed two million instances per
/// step and aborted past 4 GB of address space; it now stays near the
/// traceless document's few MB, so 1 GiB is a wide margin that the listing
/// cannot meet. `ulimit -v` is enforced on Linux only.
#[cfg(target_os = "linux")]
#[test]
fn a_trace_over_a_large_values_scope_checks_under_an_address_space_ceiling() {
    const CEILING_KB: u64 = 1024 * 1024;
    const HI: u32 = 2_000_000;
    for (label, source, next_edition) in [
        ("published", claims(HI, &traces_at(TOP_OF_SCOPE, HI)), false),
        ("next-edition", claims_next_edition(HI), true),
    ] {
        let scratch = Scratch::new(&format!("ceiling-{label}"));
        let path = scratch.write(&source);
        let mut command = Command::new("sh");
        command.arg("-c").arg(format!(
            "ulimit -v {CEILING_KB} && exec {:?} check {:?}",
            env!("CARGO_BIN_EXE_fslc"),
            path,
        ));
        if next_edition {
            command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
        }
        let output = command.output().expect("run capped fslc check");
        assert_eq!(
            output.status.code(),
            Some(0),
            "{label}: check under {CEILING_KB} KiB: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).expect("check JSON");
        assert_eq!(value["result"], "ok", "{label}: {value:#}");
    }
}

/// Every trace outcome the replay distinguishes — a guard rejection, an
/// accepted forbidden step, a broken setup, an argument outside the verify
/// scope, a step at the scope's top value, a passing and a failing
/// acceptance — is the same on `0..3` and on `0..200000`.
#[test]
fn trace_verdicts_do_not_depend_on_the_values_scope() {
    const SMALL: u32 = 3;
    const LARGE: u32 = 200_000;
    // (label, traces, result, kind)
    let cases = [
        (
            "guard-rejects",
            "  forbidden FB-1 \"a small claim is not approved\" { submit(0, 1) approve(0) expect rejected }",
            "ok",
            None,
        ),
        (
            "accepted",
            "  forbidden FB-1 \"a large claim is not approved\" { submit(0, 2) approve(0) expect rejected }",
            "error",
            Some("forbidden"),
        ),
        (
            "broken-setup",
            "  forbidden FB-1 \"a draft is not rejected\" { approve(0) reject(0) expect rejected }",
            "error",
            Some("forbidden_setup"),
        ),
        (
            "outside-scope",
            "  forbidden FB-1 \"a negative amount is not submitted\" { submit(0, -1) expect rejected }",
            "error",
            Some("forbidden"),
        ),
        ("top-of-scope", TOP_OF_SCOPE, "ok", None),
        (
            "acceptance-passes",
            "  acceptance AC-1 \"a large claim is approved\" { submit(0, 2) approve(0) expect Claim 0 in Approved }",
            "ok",
            None,
        ),
        (
            "acceptance-fails",
            "  acceptance AC-1 \"a small claim is approved\" { submit(0, 1) approve(0) expect Claim 0 in Approved }",
            "error",
            Some("acceptance"),
        ),
    ];
    for (label, traces, result, kind) in cases {
        let mut outcomes = Vec::new();
        for hi in [SMALL, LARGE] {
            let scratch = Scratch::new(&format!("{label}-{hi}"));
            let (output, exit) = fslc_check(&scratch.write(&claims(hi, &traces_at(traces, hi))));
            assert_eq!(
                (output["result"].as_str(), output["kind"].as_str()),
                (Some(result), kind),
                "{label} on 0..{hi}: {output:#}"
            );
            outcomes.push((
                exit,
                output["id"].clone(),
                output["failed_step"].clone(),
                output["accepted_step"].clone(),
                output["step"].clone(),
                output["out_of_scope_argument"]["value"].clone(),
            ));
        }
        assert_eq!(
            outcomes[0], outcomes[1],
            "{label}: 0..{SMALL} vs 0..{LARGE}"
        );
    }
}
