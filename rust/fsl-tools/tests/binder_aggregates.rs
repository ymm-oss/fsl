// SPDX-License-Identifier: Apache-2.0

use fsl_core::{FsResolver, parse_kernel_source};
use fsl_tools::enumerate_builtin_mutants;

#[test]
fn mutation_visits_shared_binder_filters_and_sum_values() {
    let source = r"
spec AggregateMutation {
  type Item = 0..2
  state { queue: Seq<Item, 3> }
  init { queue = Seq { 1 } }
  action stay() {
    requires sum(item in queue of item + 1 where item > 0) >= 0
    queue = queue
  }
}
";
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("lower source");
    let mutants = enumerate_builtin_mutants(kernel.syntax());
    assert!(
        mutants
            .iter()
            .filter(|mutant| mutant.target.contains("stay requires"))
            .filter(|mutant| mutant.op.starts_with("integer_literal_"))
            .count()
            >= 6
    );
}
