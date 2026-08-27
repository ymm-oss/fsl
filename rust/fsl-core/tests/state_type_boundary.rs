// SPDX-License-Identifier: Apache-2.0

use fsl_core::{FsResolver, build_model, parse_kernel_source};

fn build(source: &str) -> Result<fsl_core::KernelModel, fsl_core::ModelError> {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel");
    build_model(kernel)
}

#[test]
fn nested_option_state_shape() {
    let accepted = build(
        "spec Accepted {
  type Bit = 0..1
  type Id = 0..1
  struct Record { nested: Option<Option<Bit>> }
  state {
    root: Option<Option<Bit>>,
    depth_three: Option<Option<Option<Bit>>>,
    record: Record,
    optional_values: Map<Id, Option<Option<Bit>>>,
    records: Map<Id, Record>,
    members: Set<Id>,
    queue: Seq<Bit, 1>,
    links: relation Id -> Id
  }
}",
    )
    .expect("nested Option state shapes are accepted");
    assert!(accepted.state_type("depth_three").is_some());

    for ty in [
        "Option<Record>",
        "Option<Set<Bit>>",
        "Option<Map<Id, Bit>>",
        "Option<Seq<Bit, 1>>",
        "Option<relation Id -> Id>",
        "Map<Id, Option<Set<Bit>>>",
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
        assert_eq!(
            error.message, "state variable 'x' has unsupported state type",
            "{ty}"
        );
        let span = error.span.expect("state-shape rejection is located");
        assert_eq!((span.start.line, span.start.column), (6, 5), "{ty}");
    }

    let error = build(
        "spec InvalidStructField {
  type Bit = 0..1
  struct Record { nested: Option<Set<Bit>> }
  state { x: Record }
}",
    )
    .expect_err("struct fields cannot wrap collections in Option");
    assert_eq!(
        error.message,
        "struct field 'Record.nested' has non-scalar type"
    );
    assert_eq!(
        error
            .span
            .expect("struct field rejection is located")
            .start
            .line,
        3
    );
}
