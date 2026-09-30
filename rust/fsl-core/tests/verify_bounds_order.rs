// SPDX-License-Identifier: Apache-2.0

//! Regression for #1165: with several `verify { }` bounds for undeclared
//! `entity`/`number` names, the reported name and location must be the first
//! offending bound in source order, not whichever one a `HashMap` yields.

use fsl_core::{FsResolver, parse_kernel_source};

const SOURCE: &str = r"spec UndeclaredBounds {
  entity Item
  number Amount
  state { n: Int }
  init { n = 0 }
  action tick() {
    requires n < 1
    n = n + 1
  }
}
verify {
  instances Item = 1
  values Amount = 0..1
  instances Zeta = 1
  instances Alpha = 1
  values Omega = 0..1
  instances Mu = 1
  values Beta = 0..1
}
";

fn lowering_error() -> fsl_core::CoreError {
    parse_kernel_source(SOURCE, &FsResolver::new("."))
        .expect_err("undeclared verify bounds must be rejected")
}

#[test]
fn undeclared_verify_bounds_report_the_first_in_source_order() {
    let error = lowering_error();
    assert_eq!(
        error.message,
        "verify instances for undeclared entity 'Zeta'"
    );
    assert_eq!((error.line, error.column), (14, 3));
}

#[test]
fn undeclared_values_bound_before_instances_is_reported_first() {
    let source = SOURCE.replace(
        "  instances Zeta = 1\n",
        "  values Gamma = 0..1\n  instances Zeta = 1\n",
    );
    let error = parse_kernel_source(&source, &FsResolver::new("."))
        .expect_err("undeclared verify bounds must be rejected");
    assert_eq!(error.message, "verify values for undeclared number 'Gamma'");
    assert_eq!((error.line, error.column), (14, 3));
}

#[test]
fn undeclared_verify_bound_report_is_stable_across_runs() {
    let first = lowering_error();
    for _ in 0..32 {
        let again = lowering_error();
        assert_eq!(
            (again.message.as_str(), again.line, again.column),
            (first.message.as_str(), first.line, first.column)
        );
    }
}
