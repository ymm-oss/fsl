// SPDX-License-Identifier: Apache-2.0

//! Issue #1302: the corresponded abstract step violates something other than
//! a body partial operation -- a post-state type bound, an abstract
//! invariant, a false `ensures`, or a partial operation in `ensures`. The
//! walk compared only the rolled-back α(s) that `Monitor::step` returns with
//! α(s'), so every consumer of `check_refinement` reported a false `refines`
//! when the impl left the mapped state unchanged. It is now
//! `refinement_failed` / `kind:"abs_step_violation"` (exit 1) through
//! `RefinementCheck::verdict()`: `fslc refine`, `check` / `verify` inline
//! `implements`, `diff`, governance preservations, and the `fslc mutate`
//! implements oracle (a kill by `refinement`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT_SCRATCH: AtomicUsize = AtomicUsize::new(0);

const IMPL: &str = "spec I {\n  type N = 0..4\n  state { z: N }\n  init { z = 0 }\n  \
     action go() { z = 0 }\n}\n";
const MAP: &str = "refinement M {\n  impl I\n  abs A\n  map y = z\n  action go() -> go()\n}\n";
const MAP_W: &str =
    "refinement M {\n  impl I\n  abs A\n  map y = z\n  map w = 0\n  action go() -> go()\n}\n";
const ABS_TYPE: &str = "spec A {\n  type N = 0..4\n  state { y: N }\n  init { y = 0 }\n  \
     action go() { y = y + 5 }\n}\n";
const ABS_ENSURES: &str = "spec A {\n  type N = 0..4\n  state { y: N }\n  init { y = 0 }\n  \
     action go() {\n    y = y\n    ensures y == 3\n  }\n}\n";
const ABS_INVARIANT: &str = "spec A {\n  type N = 0..4\n  state { y: N, w: N }\n  \
     init { y = 0  w = 0 }\n  action go() { w = 1 }\n  invariant W { w == 0 }\n}\n";
const ABS_ENSURES_HOLDS: &str = "spec A {\n  type N = 0..4\n  state { y: N }\n  \
     init { y = 0 }\n  action go() {\n    y = y\n    ensures y == 0\n  }\n}\n";
const GOVERNANCE: &str = "governance G {\n  control CTRL-1 \"go is preserved\"\n\n  \
     preservation P {\n    before A from \"abs_ensures.fsl\"\n    after I from \"impl.fsl\"\n    \
     preserve CTRL-1\n    checked_by refinement \"map.fsl\"\n  }\n}\n";
// `diff` maps by identity, so both sides share the spec name and shape.
const DIFF_BAD: &str = "spec S {\n  type N = 0..4\n  state { y: N }\n  init { y = 0 }\n  \
     action go() {\n    y = y\n    ensures y == 3\n  }\n}\n";
const DIFF_OK: &str = "spec S {\n  type N = 0..4\n  state { y: N }\n  init { y = 0 }\n  \
     action go() { y = y }\n}\n";

fn req(abs_file: &str, extra_map: &str) -> String {
    format!(
        "requirements R {{\n  implements A from \"{abs_file}\" {{\n    map y = z\n{extra_map}    \
         action go() -> go()\n  }}\n  type N = 0..4\n  state {{ z: N }}\n  init {{ z = 0 }}\n  \
         action go() {{ z = 0 }}\n}}\n"
    )
}

fn scratch(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1302-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    for (file, content) in [
        ("impl.fsl", IMPL.to_owned()),
        ("map.fsl", MAP.to_owned()),
        ("map_w.fsl", MAP_W.to_owned()),
        ("abs_type.fsl", ABS_TYPE.to_owned()),
        ("abs_ensures.fsl", ABS_ENSURES.to_owned()),
        ("abs_invariant.fsl", ABS_INVARIANT.to_owned()),
        ("abs_ensures_holds.fsl", ABS_ENSURES_HOLDS.to_owned()),
        ("req_ensures.fsl", req("abs_ensures.fsl", "")),
        (
            "req_invariant.fsl",
            req("abs_invariant.fsl", "    map w = 0\n"),
        ),
        ("governance.fsl", GOVERNANCE.to_owned()),
        ("bad.fsl", DIFF_BAD.to_owned()),
        ("ok.fsl", DIFF_OK.to_owned()),
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

/// Each row's `fslc refine` envelope: `abs_step_violation`, the abstract
/// violation named, no `abs_after_expected` (the rolled-back α(s) is not an
/// abstract successor), and the rejected successor as `abs_after_attempted`.
#[test]
fn refine_reports_abstract_step_violations_as_abs_step_violation() {
    let dir = scratch("refine");
    for (abs, map, abs_violation, attempted) in [
        (
            "abs_type.fsl",
            "map.fsl",
            json!({"kind": "type_bound", "name": "_bounds_y"}),
            json!({"y": 5}),
        ),
        (
            "abs_ensures.fsl",
            "map.fsl",
            json!({"kind": "ensures", "name": "go"}),
            json!({"y": 0}),
        ),
        (
            "abs_invariant.fsl",
            "map_w.fsl",
            json!({"kind": "invariant", "name": "W"}),
            json!({"y": 0, "w": 1}),
        ),
    ] {
        let (output, code) = fslc(&dir, &["refine", "impl.fsl", abs, map, "--depth", "3"]);
        assert_eq!(output["result"], "refinement_failed", "{abs}: {output}");
        assert_eq!(output["kind"], "abs_step_violation", "{abs}: {output}");
        assert_eq!(output["at"], "step", "{abs}: {output}");
        assert_eq!(output["violated_at_step"], 1, "{abs}: {output}");
        assert_eq!(output["impl_action"]["name"], "go", "{abs}: {output}");
        assert_eq!(output["abs_violation"], abs_violation, "{abs}: {output}");
        assert!(
            output.get("abs_after_expected").is_none(),
            "{abs}: {output}"
        );
        assert_eq!(output["abs_after_attempted"], attempted, "{abs}: {output}");
        assert_eq!(code, 1, "{abs}: {output}");
    }
}

/// Negative control: an abstract `ensures` that holds still `refines`, and
/// the note says what refinement does check of it now.
#[test]
fn refine_still_refines_when_the_abstract_ensures_holds() {
    let dir = scratch("control");
    let (output, code) = fslc(
        &dir,
        &[
            "refine",
            "impl.fsl",
            "abs_ensures_holds.fsl",
            "map.fsl",
            "--depth",
            "3",
        ],
    );
    assert_eq!(output["result"], "refines", "{output}");
    assert_eq!(
        output["note"],
        "abs ensures are checked only on the abstract steps the impl's transitions correspond \
         to within checked_to_depth; verify/prove the abstract spec separately",
        "{output}"
    );
    assert_eq!(code, 0, "{output}");
}

#[test]
fn inline_implements_reports_abs_step_violation_in_check_and_verify() {
    let dir = scratch("inline");
    for file in ["req_ensures.fsl", "req_invariant.fsl"] {
        for args in [&["check", file][..], &["verify", file, "--depth", "3"][..]] {
            let (output, code) = fslc(&dir, args);
            assert_eq!(output["result"], "refinement_failed", "{args:?}: {output}");
            assert_eq!(
                output["implements"]["violation"]["kind"], "abs_step_violation",
                "{args:?}: {output}"
            );
            assert_eq!(code, 1, "{args:?}: {output}");
        }
    }
}

#[test]
fn diff_reports_abs_step_violation_for_the_direction_whose_abstraction_rejects_the_step() {
    let dir = scratch("diff");
    let (output, code) = fslc(&dir, &["diff", "ok.fsl", "bad.fsl", "--depth", "3"]);
    // `old_to_new` checks `ok.fsl` against `bad.fsl`, whose `ensures` is
    // false; `new_to_old` is `bad.fsl`'s own `ensures` violation.
    let old_to_new = &output["directions"]["old_to_new"];
    assert_eq!(old_to_new["result"], "refinement_failed", "{output}");
    assert_eq!(old_to_new["kind"], "abs_step_violation", "{output}");
    assert_eq!(
        old_to_new["abs_violation"],
        json!({"kind": "ensures", "name": "go"}),
        "{output}"
    );
    assert_eq!(
        output["directions"]["new_to_old"]["result"], "impl_violated",
        "{output}"
    );
    assert_eq!(code, 1, "{output}");
}

#[test]
fn governance_reports_refinement_failed_for_an_abstract_step_violation() {
    let dir = scratch("governance");
    let (output, _code) = fslc(&dir, &["check", "governance.fsl"]);
    assert_eq!(
        output["governance"]["preservations"][0]["result"], "refinement_failed",
        "{output}"
    );
}

/// The abstract `ensures d > 0` is false only for `go(0)`; the base impl's
/// parameter domain `1..4` never invokes it, so the base refines. The first
/// builtin mutant, `type_bound_lo_minus1`, widens the impl domain to `0..4`,
/// and the impl's `go(0)` (which leaves the state unchanged, as the abstract
/// update does) now corresponds to a step the abstraction rejects. The
/// implements oracle must kill it: a refinement verdict, not an oracle error.
#[test]
fn mutate_kills_a_mutant_that_reaches_an_abstract_step_violation() {
    let dir = scratch("mutate");
    fs::write(
        dir.join("abs.fsl"),
        "spec MA {\n  type AD = 0..4\n  state { y: Bool }\n  init { y = true }\n  \
         action go(d: AD) {\n    y = y\n    ensures d > 0\n  }\n}\n",
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
