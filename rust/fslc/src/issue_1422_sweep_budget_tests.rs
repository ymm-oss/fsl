// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

type SweepCountRanges = std::collections::BTreeMap<String, (i64, i64)>;

#[test]
fn issue_1422_sweep_empty_axes_and_inclusive_depth_boundaries() {
    assert_eq!(MAX_SWEEP_CELLS, 10_000);
    let empty = SweepCountRanges::new();
    for (lo, hi, expected) in [
        (0, 0, Some(1)),
        (7, 7, Some(1)),
        (0, 9_998, Some(9_999)),
        (0, 9_999, Some(10_000)),
        (0, 10_000, None),
        (3, 2, None),
        (usize::MAX, usize::MAX, Some(1)),
        (usize::MAX - 9_998, usize::MAX, Some(9_999)),
        (usize::MAX - 9_999, usize::MAX, Some(10_000)),
        (usize::MAX - 10_000, usize::MAX, None),
        (0, usize::MAX, None),
    ] {
        assert_eq!(checked_sweep_cell_count(lo, hi, &empty, &empty), expected);
    }
}

#[test]
fn issue_1422_sweep_each_axis_width_uses_signed_widened_inclusive_arithmetic() {
    let empty = SweepCountRanges::new();
    for (lo, hi, expected) in [
        (1, 9_999, Some(9_999)),
        (1, 10_000, Some(10_000)),
        (1, 10_001, None),
        (-5_000, 4_998, Some(9_999)),
        (-5_000, 4_999, Some(10_000)),
        (-5_000, 5_000, None),
        (i64::MIN, i64::MIN, Some(1)),
        (i64::MAX, i64::MAX, Some(1)),
        (i64::MIN, i64::MIN + 9_998, Some(9_999)),
        (i64::MAX - 9_999, i64::MAX, Some(10_000)),
        (i64::MIN, i64::MAX, None),
    ] {
        let ranges = SweepCountRanges::from([("Axis".to_owned(), (lo, hi))]);
        if lo >= 1 {
            assert_eq!(checked_sweep_cell_count(0, 0, &ranges, &empty), expected);
        }
        assert_eq!(checked_sweep_cell_count(0, 0, &empty, &ranges), expected);
    }
}

#[test]
fn issue_1422_sweep_counts_normalized_names_and_both_map_products_with_depth() {
    let instances = SweepCountRanges::from([("Z".to_owned(), (1, 20)), ("A".to_owned(), (1, 25))]);
    let mut values = SweepCountRanges::from([("X".to_owned(), (-5, 4))]);
    assert_eq!(
        checked_sweep_cell_count(7, 7, &instances, &values),
        Some(5_000)
    );
    assert_eq!(
        checked_sweep_cell_count(7, 8, &instances, &values),
        Some(10_000)
    );
    assert_eq!(checked_sweep_cell_count(7, 9, &instances, &values), None);
    values.insert("X".to_owned(), (-5, 5));
    assert_eq!(checked_sweep_cell_count(7, 8, &instances, &values), None);
    let normalized = SweepCountRanges::from([
        ("Same".to_owned(), (1, 10_001)),
        ("Same".to_owned(), (7, 7)),
    ]);
    let empty = SweepCountRanges::new();
    assert_eq!(checked_sweep_cell_count(0, 0, &normalized, &empty), Some(1));
    assert_eq!(checked_sweep_cell_count(0, 0, &empty, &normalized), Some(1));
    let instances = SweepCountRanges::from([("Same".to_owned(), (1, 2))]);
    let values = SweepCountRanges::from([("Same".to_owned(), (10, 12))]);
    assert_eq!(
        checked_sweep_cell_count(0, 1, &instances, &values),
        Some(12)
    );
}

#[test]
fn issue_1422_sweep_rejects_products_that_cannot_fit_usize_without_enumeration() {
    let empty = SweepCountRanges::new();
    let two = SweepCountRanges::from([("Two".to_owned(), (1, 2))]);
    assert_eq!(checked_sweep_cell_count(1, usize::MAX, &two, &empty), None);
    let huge = SweepCountRanges::from([("Huge".to_owned(), (0, i64::MAX))]);
    let three = SweepCountRanges::from([("Three".to_owned(), (1, 3))]);
    assert_eq!(checked_sweep_cell_count(0, 0, &huge, &three), None);
    let binary_axes = (0..usize::BITS)
        .map(|axis| (format!("Axis{axis}"), (0, 1)))
        .collect::<SweepCountRanges>();
    assert_eq!(checked_sweep_cell_count(0, 0, &binary_axes, &empty), None);
    assert_eq!(checked_sweep_cell_count(0, 0, &empty, &binary_axes), None);
}
