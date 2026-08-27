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
    links: relation Id -> Id
  }
}",
    )
    .expect("nested Option state shapes are accepted");
    assert!(accepted.state_type("depth_three").is_some());

    // This table gives each `bounded_scalar` false branch a concrete
    // bounded-only position. Keep it aligned with
    // DESIGN-nested-option-support.md §3.2: `Int` is scalar but is never a
    // bounded key, element, or relation endpoint; nor are resolved structs or
    // any collection/Option shape.
    for ty in [
        "Option<Record>",
        "Option<Set<Bit>>",
        "Option<Map<Id, Bit>>",
        "Option<Seq<Bit, 1>>",
        "Option<relation Id -> Id>",
        "Map<Id, Option<Set<Bit>>>",
        "Map<Id, Set<Bit>>",
        "Map<Id, Option<Record>>",
        "Map<Record, Bit>",
        "Map<Int, Bit>",
        "Map<Option<Id>, Bit>",
        "Set<Int>",
        "Set<Record>",
        "Set<Option<Id>>",
        "Set<Map<Id, Bit>>",
        "Set<relation Id -> Id>",
        "Set<Set<Id>>",
        "Set<Seq<Bit, 1>>",
        "Seq<Option<Bit>, 1>",
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
        assert_eq!(
            error
                .span
                .expect("struct field rejection is located")
                .start
                .line,
            5,
            "{ty}"
        );
    }
}
