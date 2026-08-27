// SPDX-License-Identifier: Apache-2.0

use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn build(source: &str) -> Result<fsl_core::KernelModel, fsl_core::ModelError> {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel");
    build_model(kernel)
}

#[test]
#[allow(clippy::too_many_lines)]
fn nested_option_state_shape() {
    let accepted = build(
        "spec Accepted {
  type Bit = 0..1
  type Id = 0..1
  enum Flag { Off, On }
  struct Record { nested: Option<Option<Bit>> }
  state {
    root: Option<Option<Bit>>,
    depth_three: Option<Option<Option<Bit>>>,
    record: Record,
    optional_values: Map<Id, Option<Option<Bit>>>,
    records: Map<Id, Record>,
    members: Set<Id>,
    bool_members: Set<Bool>,
    range_values: Map<0..1, Bit>,
    enum_members: Set<Flag>,
    queue: Seq<Bit, 1>,
    int_queue: Seq<Int, 1>,
    bool_queue: Seq<Bool, 1>,
    range_queue: Seq<0..1, 1>,
    enum_queue: Seq<Flag, 1>,
    links: relation Id -> Id
  }
}",
    )
    .expect("nested Option state shapes are accepted");
    assert!(accepted.state_type("depth_three").is_some());

    // These placements make every reachable false arm of all three shape
    // predicates observable: Map keys reach `bounded_scalar`, Map values
    // reach `struct_value` after `optional_scalar` rejects them, and Seq
    // elements reach `scalar`. `Int` remains scalar but is not bounded.
    for ty in [
        "Option<Record>",
        "Option<Set<Bit>>",
        "Option<Map<Id, Bit>>",
        "Option<Seq<Bit, 1>>",
        "Option<relation Id -> Id>",
        "Map<Id, Option<Set<Bit>>>",
        "Map<Id, Set<Bit>>",
        "Map<Id, Map<Id, Bool>>",
        "Map<Id, Seq<Bit, 1>>",
        "Map<Id, relation Id -> Id>",
        "Map<Id, Option<Record>>",
        "Map<Record, Bit>",
        "Map<Int, Bit>",
        "Map<Option<Id>, Bit>",
        "Map<Map<Id, Bit>, Bit>",
        "Map<relation Id -> Id, Bit>",
        "Map<Set<Id>, Bit>",
        "Map<Seq<Bit, 1>, Bit>",
        "Set<Int>",
        "Set<Record>",
        "Set<Option<Id>>",
        "Set<Map<Id, Bit>>",
        "Set<relation Id -> Id>",
        "Set<Set<Id>>",
        "Set<Seq<Bit, 1>>",
        "Seq<Option<Bit>, 1>",
        "Seq<Record, 1>",
        "Seq<Map<Id, Bit>, 1>",
        "Seq<relation Id -> Id, 1>",
        "Seq<Set<Id>, 1>",
        "Seq<Seq<Bit, 1>, 1>",
        "relation Int -> Id",
        "relation Id -> Option<Id>",
    ] {
        let source = format!(
            "spec Rejected {{
  type Bit = 0..1
  type Id = 0..1
  struct Record {{ value: Bit }}
  state {{
    x: {ty}
  }}
}}"
        );
        let error = build(&source).expect_err("unsupported Option payload must reject");
        let expected_message = if ty == "Map<Int, Bit>" {
            "Map<Int, ...> on 'x' is rejected; use a bounded domain type as key; declare `type K = 0..<max>` and use `Map<K, ...>`"
        } else {
            "state variable 'x' has unsupported state type"
        };
        assert_eq!(error.message, expected_message, "{ty}");
        let span = error.span.expect("state-shape rejection is located");
        assert_eq!((span.start.line, span.start.column), (6, 5), "{ty}");
    }

    for ty in [
        "Inner",
        "Set<Bit>",
        "Map<Id, Bit>",
        "Seq<Bit, 1>",
        "relation Id -> Id",
        "Option<Inner>",
        "Option<Set<Bit>>",
        "Option<Map<Id, Bit>>",
        "Option<Seq<Bit, 1>>",
        "Option<relation Id -> Id>",
    ] {
        let error = build(&format!(
            "spec InvalidStructField {{
  type Bit = 0..1
  type Id = 0..1
  struct Inner {{ value: Bit }}
  struct Record {{ nested: {ty} }}
  state {{ x: Record }}
}}"
        ))
        .expect_err("struct fields must stay optional scalars");
        assert_eq!(
            error.message, "struct field 'Record.nested' has non-scalar type",
            "{ty}"
        );
        let span = error.span.expect("struct field rejection is located");
        assert_eq!((span.start.line, span.start.column), (5, 3), "{ty}");
    }
}
