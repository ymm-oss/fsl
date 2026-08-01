// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Reproducible measurements for the #662 design decision. This intentionally
//! does not implement the selected saga-state representation.

use std::path::{Path, PathBuf};
use std::process::Command;

use fsl_core::{TypeDef, TypeRef};

const FIXTURE: &str = "rust/fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl";
const SAGA_PHASES: usize = 6;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn expanded_model() -> fsl_core::KernelModel {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["domain", "expand", FIXTURE])
        .current_dir(root())
        .output()
        .expect("expand domain fixture");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let source = String::from_utf8(output.stdout).expect("expanded Kernel is UTF-8");
    let kernel = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new("."))
        .expect("parse expanded Kernel");
    fsl_core::build_model(kernel).expect("build expanded Kernel")
}

#[test]
fn candidate_state_costs_are_anchored_to_the_current_fixture() {
    let model = expanded_model();
    let event_count = model
        .state
        .iter()
        .filter(|(name, ty)| name.starts_with("event_") && *ty == TypeRef::Bool)
        .count();
    assert_eq!(event_count, 5, "fixture event vocabulary changed");

    let (_, status_ty) = model
        .state
        .iter()
        .find(|(name, _)| name == "capture_payment_status")
        .expect("effect status map");
    let TypeRef::Map(correlation_ty, phase_ty) = status_ty else {
        panic!("effect status must be a map: {status_ty:?}");
    };
    let correlation_count = model
        .domain_values(correlation_ty)
        .expect("finite correlation domain")
        .len();
    assert_eq!(correlation_count, 2, "fixture correlation domain changed");
    let TypeRef::Named(effect_phase_name) = phase_ty.as_ref() else {
        panic!("effect status value must be an enum: {phase_ty:?}");
    };
    let Some(TypeDef::Enum {
        members: effect_phases,
        ..
    }) = model.types.get(effect_phase_name)
    else {
        panic!("effect status enum missing");
    };
    assert_eq!(effect_phases.len(), 7, "effect status vocabulary changed");

    // Candidate 1 reuses the existing correlation-indexed effect status: no
    // additional state projection, but each saga action must gain C instances.
    let correlation_parameter_projection = 1_usize;
    let correlated_action_instances = correlation_count;

    // Candidate 2 makes the E global event flags sticky. The current one-hot
    // projection admits none-or-one (E+1) valuations; sticky flags admit 2^E.
    let one_hot_event_projection = event_count + 1;
    let sticky_event_projection = 1_usize << event_count;

    // Candidate 3 adds Map<Correlation,SagaPhase>. The accepted six phases are
    // NotStarted/Awaiting/Succeeded/Failed/TimedOut/Compensating.
    let dedicated_saga_projection =
        SAGA_PHASES.pow(u32::try_from(correlation_count).expect("correlation count fits exponent"));

    assert_eq!(correlation_parameter_projection, 1);
    assert_eq!(correlated_action_instances, 2);
    assert_eq!((one_hot_event_projection, sticky_event_projection), (6, 32));
    assert_eq!(dedicated_saga_projection, 36);

    // Negative control for candidate 2: two correlations times five possible
    // first events are ten labeled histories, but a global sticky projection
    // has only five single-event states. It necessarily aliases correlations.
    let labeled_single_event_histories = correlation_count * event_count;
    let global_single_event_states = event_count;
    assert!(labeled_single_event_histories > global_single_event_states);

    let saga_action = model
        .actions
        .iter()
        .find(|action| action.name == "saga_payment_flow_capture")
        .expect("lowered saga action");
    assert!(
        saga_action.params.is_empty(),
        "once implementation adds correlation, update the accepted design measurement"
    );
}
