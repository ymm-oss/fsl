// SPDX-License-Identifier: Apache-2.0

//! Issue #1296: an abstract action whose own body hits a partial operation
//! (here `y = 4 / y` from `y = 0`) at the mapped state of a corresponded impl
//! step was accepted, so every consumer of `check_refinement` reported a
//! false `refines`. It is now `refinement_failed` / `kind:"abs_partial_op"`
//! (exit 1) through `RefinementCheck::verdict()`: `fslc refine`, `check` /
//! `verify` inline `implements`, `diff`, governance preservations, and the
//! `fslc mutate` implements oracle (a kill by `refinement`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT_SCRATCH: AtomicUsize = AtomicUsize::new(0);

const ABS: &str = "spec A {\n  type N = 0..4\n  state { y: N }\n  init { y = 0 }\n  \
     action go() { y = 4 / y }\n}\n";
const IMPL: &str = "spec I {\n  type N = 0..4\n  state { z: N }\n  init { z = 0 }\n  \
     action go() { z = 0 }\n}\n";
const MAP: &str = "refinement M {\n  impl I\n  abs A\n  map y = z\n  action go() -> go()\n}\n";
const REQ: &str = "requirements R {\n  implements A from \"abs.fsl\" {\n    map y = z\n    \
     action go() -> go()\n  }\n  type N = 0..4\n  state { z: N }\n  init { z = 0 }\n  \
     action go() { z = 0 }\n}\n";
const GOVERNANCE: &str = "governance G {\n  control CTRL-1 \"go is preserved\"\n\n  \
     preservation P {\n    before A from \"abs.fsl\"\n    after I from \"impl.fsl\"\n    \
     preserve CTRL-1\n    checked_by refinement \"map.fsl\"\n  }\n}\n";
// `diff` maps by identity, so both sides share the spec name and shape;
// only the body of `go` differs.
const DIFF_PARTIAL: &str = "spec S {\n  type N = 0..4\n  state { y: N }\n  init { y = 0 }\n  \
     action go() { y = 4 / y }\n}\n";
const DIFF_TOTAL: &str = "spec S {\n  type N = 0..4\n  state { y: N }\n  init { y = 0 }\n  \
     action go() { y = 0 }\n}\n";

fn scratch(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1296-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    for (file, content) in [
        ("abs.fsl", ABS),
        ("impl.fsl", IMPL),
        ("map.fsl", MAP),
        ("req.fsl", REQ),
        ("governance.fsl", GOVERNANCE),
        ("partial.fsl", DIFF_PARTIAL),
        ("total.fsl", DIFF_TOTAL),
    ] {
        fs::write(dir.join(file), content).expect("write fixture");
    }
    dir
}

fn fslc(dir: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let json = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!(
            "fslc {args:?} printed non-JSON ({error}): {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (json, output.status.code().expect("exit code"))
}

#[test]
fn refine_reports_an_abstract_body_partial_op_as_abs_partial_op() {
    let dir = scratch("refine");
    let (output, code) = fslc(
        &dir,
        &["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "3"],
    );
    assert_eq!(output["result"], "refinement_failed", "{output}");
    assert_eq!(output["kind"], "abs_partial_op", "{output}");
    assert_eq!(output["at"], "step", "{output}");
    assert_eq!(output["violated_at_step"], 1, "{output}");
    assert_eq!(output["impl_action"]["name"], "go", "{output}");
    assert_eq!(output["abs_before"], json!({"y": 0}), "{output}");
    assert!(output.get("abs_after_expected").is_none(), "{output}");
    assert_eq!(code, 1, "{output}");
}

#[test]
fn inline_implements_reports_abs_partial_op_in_check_and_verify() {
    let dir = scratch("inline");
    for args in [
        &["check", "req.fsl"][..],
        &["verify", "req.fsl", "--depth", "3"][..],
    ] {
        let (output, code) = fslc(&dir, args);
        assert_eq!(output["result"], "refinement_failed", "{args:?}: {output}");
        assert_eq!(
            output["implements"]["result"], "refinement_failed",
            "{args:?}: {output}"
        );
        assert_eq!(
            output["implements"]["violation"]["kind"], "abs_partial_op",
            "{args:?}: {output}"
        );
        assert_eq!(code, 1, "{args:?}: {output}");
    }
}

#[test]
fn diff_reports_abs_partial_op_for_the_direction_whose_abstraction_is_partial() {
    let dir = scratch("diff");
    let (output, code) = fslc(&dir, &["diff", "total.fsl", "partial.fsl", "--depth", "3"]);
    // `old_to_new` checks the total side against the partial one: the
    // abstraction (`partial.fsl`) divides by zero. `new_to_old` is the
    // partial side's own `partial_op` (impl self-violation), unchanged.
    assert_eq!(
        output["directions"]["old_to_new"]["result"], "refinement_failed",
        "{output}"
    );
    assert_eq!(
        output["directions"]["old_to_new"]["kind"], "abs_partial_op",
        "{output}"
    );
    assert_eq!(
        output["directions"]["new_to_old"]["result"], "impl_violated",
        "{output}"
    );
    assert_eq!(code, 1, "{output}");
}

#[test]
fn governance_reports_refinement_failed_for_an_abstract_body_partial_op() {
    let dir = scratch("governance");
    let (output, _code) = fslc(&dir, &["check", "governance.fsl"]);
    assert_eq!(
        output["governance"]["preservations"][0]["result"], "refinement_failed",
        "{output}"
    );
}

/// The abstraction is partial only at `go(0)`; the base impl's parameter
/// domain `1..4` never invokes it, so the base refines. The first builtin
/// mutant, `type_bound_lo_minus1`, widens the impl domain to `0..4`, and the
/// impl's `go(0)` (which leaves the state unchanged, exactly what the total
/// `4 / 0 == 0` update would give) now invokes the partial abstract body. The
/// implements oracle must kill it: a deterministic refinement verdict, not
/// an oracle error.
#[test]
fn mutate_kills_a_mutant_that_reaches_an_abstract_partial_op() {
    let dir = scratch("mutate");
    fs::write(
        dir.join("abs.fsl"),
        "spec MA {\n  type AD = 0..4\n  state { y: Bool }\n  init { y = true }\n  \
         action go(d: AD) { y = y and 4 / d > 0 }\n}\n",
    )
    .expect("write abs");
    fs::write(
        dir.join("req.fsl"),
        "requirements MR {\n  implements MA from \"abs.fsl\" {\n    map y = z\n    \
         action go(d) -> go(d)\n  }\n  type D = 1..4\n  state { z: Bool }\n  \
         init { z = true }\n  action go(d: D) { z = z }\n}\n",
    )
    .expect("write req");
    let (base, base_code) = fslc(&dir, &["verify", "req.fsl", "--depth", "3"]);
    assert_eq!(base["implements"]["result"], "refines", "{base}");
    assert_eq!(base_code, 0, "{base}");

    let (output, code) = fslc(
        &dir,
        &[
            "mutate",
            "req.fsl",
            "--depth",
            "3",
            "--max-mutants",
            "1",
            "--oracle-attribution",
        ],
    );
    assert_eq!(code, 0, "{output}");
    let mutant = &output["mutants"][0];
    assert_eq!(mutant["op"], "type_bound_lo_minus1", "{output}");
    assert_eq!(mutant["status"], "killed", "{mutant}");
    assert_eq!(mutant["killed_by"], "refinement", "{mutant}");
    assert_eq!(mutant["killers"], json!(["refinement"]), "{mutant}");
}
