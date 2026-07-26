// SPDX-License-Identifier: Apache-2.0

#[test]
fn causal_family_has_one_module_owner() {
    let main = include_str!("../src/main.rs");
    let causal = include_str!("../src/causal.rs");

    assert!(main.contains("mod causal;"));
    assert!(main.contains("use causal::{causal_command, run_causal_check};"));
    assert!(!main.contains("fn causal_command("));
    assert!(!main.contains("fn run_causal_analyze("));
    assert!(!main.contains("fn run_causal_diff("));
    assert!(causal.contains("pub(super) fn causal_command("));
    assert!(causal.contains("pub(super) fn run_causal_check("));
}

#[test]
fn causal_family_keeps_an_explicit_nongeneric_boundary() {
    let causal = include_str!("../src/causal.rs");

    assert!(!causal.contains("use super::*"));
    assert!(causal.contains("required_option_value"));
    assert!(causal.contains("error_output"));
    assert!(causal.contains("load_snapshot_value_object"));
    assert!(!causal.contains("struct CommandContext"));
    assert!(!causal.contains("enum CommandOutcome"));
}
