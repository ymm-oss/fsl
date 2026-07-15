// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::parse_surface_spec;
use fsl_tools::enumerate_builtin_mutants;

#[test]
fn mutates_option_equality_operator_and_payload() {
    let spec = parse_surface_spec(
        r"
spec OptionMutation {
  enum Status { Pending, Done }
  state { current: Option<Status> }
  init { current = none }
  action select() {
    requires current == some(Pending)
    current = some(Pending)
  }
  invariant Expected { current == some(Pending) }
}
",
    )
    .expect("parse Option spec");

    let mutants = enumerate_builtin_mutants(&spec);
    assert!(
        mutants
            .iter()
            .any(|mutant| mutant.op == "equality_operator_flip")
    );
    assert!(
        mutants
            .iter()
            .any(|mutant| mutant.op == "enum_constant_swap")
    );
}
