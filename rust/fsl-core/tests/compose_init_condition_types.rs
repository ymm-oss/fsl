// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue #832 shape B: checked-model construction must type every `init if`
//! condition, not only assignments nested below it.

use std::collections::BTreeMap;

use fsl_core::{CoreError, FileResolver, build_model, parse_kernel_source};

struct MemoryResolver(BTreeMap<String, String>);

impl FileResolver for MemoryResolver {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        self.0.get(path).cloned().ok_or_else(|| CoreError {
            message: format!("missing {path}"),
            line: 1,
            column: 1,
            origin: None,
            name_resolution: false,
        })
    }
}

const CORE_COMPONENT: &str = "spec Core { \
    type RealType = 0..1 \
    state { flag: Bool } \
    init { flag = true } \
    action noop() {} \
}";

fn resolver() -> MemoryResolver {
    MemoryResolver(BTreeMap::from([(
        "core.fsl".to_owned(),
        CORE_COMPONENT.to_owned(),
    )]))
}

/// Rejecting detector: the undeclared root of a field-shaped condition is
/// rejected at the `if`, matching assignment-RHS type checking.
#[test]
fn undeclared_alias_shaped_init_condition_is_rejected() {
    let source = r#"compose Broken {
  use Core as core from "core.fsl"
  state { x: Int }
  init {
    if nonexistent.flag {
      x = 0
    }
  }
}"#;
    let kernel = parse_kernel_source(source, &resolver()).expect("compose lowers");
    let error = build_model(kernel).expect_err("the unknown condition root must fail check");

    assert_eq!(
        error.message,
        "invalid init statement: public Kernel cannot type identifier 'nonexistent'"
    );
    let span = error
        .origin
        .as_deref()
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.span)
        .expect("the condition error is located");
    assert_eq!(span.start.line, 5);
    assert_eq!(span.start.column, 5);
}

/// Accepting control: a declared component alias and real Bool member remain
/// valid in the same `init if` condition position.
#[test]
fn declared_alias_init_condition_still_builds() {
    let source = r#"compose Works {
  use Core as core from "core.fsl"
  state { x: Int }
  init {
    if core.flag {
      x = 0
    }
  }
}"#;
    let kernel = parse_kernel_source(source, &resolver()).expect("compose lowers");
    build_model(kernel).expect("a declared alias Bool condition must remain valid");
}
