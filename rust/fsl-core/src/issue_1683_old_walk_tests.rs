// SPDX-License-Identifier: Apache-2.0

//! #1683: `check` walks an `old` operand a second time against what surrounds it. That
//! walk must not double at each nested `old`. It counts validation visits rather than
//! time, so the bound does not depend on the machine.

use std::cell::Cell;

use crate::{FsResolver, build_model, parse_kernel_source};

thread_local! {
    static VISITS: Cell<u64> = const { Cell::new(0) };
}

pub(super) fn count_visit() {
    VISITS.with(|visits| visits.set(visits.get() + 1));
}

fn visits(property: &str) -> u64 {
    let source = format!(
        "spec S {{\n  type A = 0..1\n  state {{ a: Set<A>, sa: Seq<A, 2>, q: Seq<Int, 2>, c: Bool }}\n  init {{ a = Set {{}} sa = Seq {{}} q = Seq {{}} c = false }}\n  action go() {{ c = not c }}\n  trans P {{ {property} }}\n}}\n"
    );
    let kernel = parse_kernel_source(&source, &FsResolver::new(".")).expect("parse");
    VISITS.with(|visits| visits.set(0));
    build_model(kernel).unwrap_or_else(|error| panic!("{property}: {error:?}"));
    VISITS.with(Cell::get)
}

/// `some(some(left)) == old(some(old(some(right))))`, with `depth` of each.
fn nested(depth: usize, left: &str, right: &str) -> String {
    let (mut somes, mut olds) = (left.to_owned(), right.to_owned());
    for _ in 0..depth {
        somes = format!("some({somes})");
        olds = format!("old(some({olds}))");
    }
    format!("{somes} == {olds}")
}

/// The operands agree, so no second walk runs. A rule that ran one at each level would
/// grow by the square of the depth, and one that doubled would not finish at 64.
#[test]
fn nested_old_of_the_expected_carrier_visits_each_level_a_bounded_number_of_times() {
    for depth in [4, 8, 12, 16, 32, 64] {
        let count = visits(&nested(depth, "a", "a"));
        assert!(count <= 40 * depth as u64 + 60, "depth {depth}: {count}");
    }
}

/// `Seq<Int, 2>` against `Seq<A, 2>` passes and differs at the top, so one second walk
/// runs, and it goes down the operand once.
#[test]
fn nested_old_of_another_carrier_runs_one_second_walk() {
    for depth in [4, 8, 12, 16, 32, 64] {
        let count = visits(&nested(depth, "sa", "q"));
        assert!(count <= 40 * depth as u64 + 60, "depth {depth}: {count}");
    }
}
