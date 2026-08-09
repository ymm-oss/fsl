// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for #727: `fslc mutate` rejected every `domain`
//! document with `kind:"semantics"`/`"mutate expects a spec-like FSL file"`,
//! so a domain spec had no kill-rate self-check evidence channel at all.
//!
//! `mutate` now accepts `domain` documents by rendering them through
//! `fsl_tools::domain_kernel_source` (the same textual kernel path
//! `fslc domain expand` uses) and mutating the re-parsed kernel spec.
//! `fsl-core::domain.rs`'s direct lowering (the path `check`/`verify` use)
//! propagates spans at only one site, so its generated nodes carry
//! effectively null spans; rendering to text and re-parsing gives every
//! mutant a `loc` inside the emitted `kernel_source`, which the output
//! envelope now carries so a witness is resolvable from the envelope alone.
//!
//! Both fixtures below are the real corpus files the design decision named,
//! not synthetic stand-ins: `order_async_effect.fsl` is the positive oracle
//! (a real property kill must survive through the rendered path), and
//! `order_fulfillment_saga.fsl` is the negative control (its compensation
//! actions are structurally dead at baseline, so every mutant targeting them
//! must survive with the existing "action dead at baseline" note -- proving
//! this negative control still fires is the actual evidence-channel claim of
//! #727, not merely that *some* mutant runs against a domain file).
//!
//! Both runs enumerate the full (uncapped) 200/198-mutant builtin set at
//! `--depth 4`: the specific kills this test pins (`CapturePayment_
//! SuccessSticky`, the 14 dead saga-compensation mutants) sit late in
//! enumeration order and require the full run to appear, so `--max-mutants`
//! cannot cheapen this test without losing the assertions it exists to make.
//! A debug-profile run of the first test alone measured ~135s; expect this
//! binary to be one of the slower ones in the suite.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

/// T1 (positive oracle) + T5 (envelope replayability): a domain document
/// with a real sticky-guard property bug is accepted, mutated through the
/// rendered kernel, and reports `CapturePayment_SuccessSticky` among the
/// killers with a `loc` that resolves inside the embedded `kernel_source`.
#[test]
fn mutate_domain_positive_oracle_kills_capture_payment_success_sticky() {
    let (output, status) = run(&[
        "mutate",
        "examples/domain/order_async_effect.fsl",
        "--depth",
        "4",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "mutated", "{output:#}");
    // 200 is the default `--max-mutants` cap (`mutate cap 200 reached: 52
    // dropped`), not the full builtin catalog (252 for this fixture): this
    // pins the CLI's default-cap behavior on a domain document, and would
    // need updating if `DEFAULT_MAX_MUTANTS` ever changes for an unrelated
    // reason.
    assert_eq!(output["summary"]["total"], 200, "{}", output["summary"]);
    assert_eq!(output["summary"]["killed"], 14, "{}", output["summary"]);
    assert_eq!(
        output["summary"]["kill_rate"], 0.07,
        "{}",
        output["summary"]
    );

    let mutants = output["mutants"].as_array().expect("mutants array");
    let success_sticky = mutants
        .iter()
        .filter(|mutant| mutant["killed_by"] == "CapturePayment_SuccessSticky")
        .collect::<Vec<_>>();
    assert_eq!(
        success_sticky.len(),
        8,
        "expected 8 CapturePayment_SuccessSticky kills, output={output:#}"
    );

    // T5: the envelope carries the rendered kernel text, and a killed
    // mutant's `loc` resolves inside it to a real, non-blank source line --
    // not a null/placeholder location.
    let kernel_source = output["kernel_source"]
        .as_str()
        .expect("kernel_source present in the envelope");
    assert!(
        kernel_source.starts_with("spec OrderAsyncEffect"),
        "{kernel_source}"
    );
    let lines = kernel_source.lines().collect::<Vec<_>>();
    for mutant in &success_sticky {
        let line = usize::try_from(mutant["loc"]["line"].as_u64().unwrap_or_else(|| {
            panic!("CapturePayment_SuccessSticky mutant has no loc.line: {mutant:#}")
        }))
        .expect("loc.line fits in usize");
        assert!(
            line >= 1 && line <= lines.len(),
            "loc.line {line} out of kernel_source range (1..={}): {mutant:#}",
            lines.len()
        );
        assert!(
            !lines[line - 1].trim().is_empty(),
            "loc.line {line} points at a blank kernel_source line: {mutant:#}"
        );
    }
}

/// T2 (negative control, the load-bearing assertion for #727): every
/// mutant targeting a saga compensation action must survive with the
/// existing "action dead at baseline" note, because the saga's
/// compensations are structurally unreachable in the verified baseline.
/// A green run here that instead shows these mutants killed, or missing the
/// note, would mean the rendered-kernel path is not actually surfacing
/// baseline dead-code the way the direct lowering path does -- domain mutate
/// would then be reporting evidence without a working evidence channel.
#[test]
fn mutate_domain_negative_control_saga_compensation_dead_at_baseline() {
    // #779 adds a new mutable evolve assignment to the compensation action
    // itself (`order_inventory_status = InventoryStatus_ReleaseRequested`)
    // in addition to the five other saga step/timeout actions the fix
    // touches, growing the uncapped mutant count from 198 to 226. At the
    // default 200-mutant cap this pushed the entire compensation-targeting
    // mutant set (this test's own load-bearing evidence) past the cap and
    // out of the report, so `--max-mutants` is raised here to a value
    // comfortably above 226 to keep this test's actual subject --
    // compensation mutants -- present to assert on; T1
    // (`mutate_domain_positive_oracle_kills_capture_payment_success_sticky`,
    // a different fixture) still exercises the untouched default cap path.
    let (output, status) = run(&[
        "mutate",
        "examples/domain/order_fulfillment_saga.fsl",
        "--depth",
        "4",
        "--max-mutants",
        "300",
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "mutated", "{output:#}");
    assert_eq!(output["summary"]["total"], 226, "{}", output["summary"]);
    assert_eq!(output["summary"]["killed"], 3, "{}", output["summary"]);
    assert_eq!(
        output["summary"]["kill_rate"], 0.0133,
        "{}",
        output["summary"]
    );

    let mutants = output["mutants"].as_array().expect("mutants array");
    let compensation_mutants = mutants
        .iter()
        .filter(|mutant| {
            mutant["target"]
                .as_str()
                .is_some_and(|target| target.contains("compensate"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        compensation_mutants.len(),
        19,
        "expected 19 compensation-targeting mutants (14 pre-#779, plus 5 new \
         mutants from the compensation action's now-applied evolve \
         assignment), output={output:#}"
    );
    for mutant in &compensation_mutants {
        assert_eq!(
            mutant["status"], "survived",
            "compensation mutant must survive: {mutant:#}"
        );
        assert_eq!(
            mutant["note"], "action dead at baseline \u{2014} survival expected",
            "compensation mutant must carry the dead-note: {mutant:#}"
        );
    }
}

/// T3 (guard): a domain document that the shared lowering guard rejects
/// (an `on_stale` policy has no executable lowering) must fail closed with
/// the guard's own located `semantics` diagnostic before `mutate`'s own
/// dialect match is ever reached -- not with the generic
/// "mutate expects a spec-like FSL file" gate message T4 pins below.
#[test]
fn mutate_domain_guard_rejects_unlowerable_construct_with_location() {
    let (output, status) = run(&[
        "mutate",
        "rust/fslc/tests/fixtures/domain_stale_policy_rejected.fsl",
        "--depth",
        "4",
    ]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "semantics", "{output:#}");
    let message = output["message"].as_str().expect("message");
    assert!(
        message.contains("on_stale") && message.contains("not supported"),
        "{output:#}"
    );
    assert_ne!(
        message, "mutate expects a spec-like FSL file",
        "guard rejection must not collapse into the generic scope-pin gate: {output:#}"
    );
    assert_eq!(output["loc"]["line"], 30, "{output:#}");
    assert_eq!(output["loc"]["column"], 5, "{output:#}");
}

/// T4 (scope pin): a non-spec-like dialect `mutate` never accepted (here,
/// `governance`) must keep failing with the original generic gate message,
/// unaffected by the new `domain` arm.
#[test]
fn mutate_still_rejects_non_spec_like_dialects() {
    let (output, status) = run(&["mutate", "examples/consulting/governance_controls.fsl"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "semantics", "{output:#}");
    assert_eq!(
        output["message"], "mutate expects a spec-like FSL file",
        "{output:#}"
    );
}
