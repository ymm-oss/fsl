// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Issue 1167: a core diagnostic carries its classification as a
//! [`CoreErrorKind`], and delivery surfaces derive the public `kind` and hint
//! from that type instead of from the message text. These cases pin that each
//! frontend producer the CLI used to recognise by its wording now tags the
//! error itself, through the public entry points the CLI calls. A producer
//! that lost its tag would silently fall back to `semantics` with no message
//! change to notice.

use std::collections::BTreeMap;

use fsl_core::{
    CoreError, CoreErrorKind, FileResolver, parse_kernel_source, parse_kernel_source_with_bounds,
};

struct MemoryResolver(BTreeMap<String, String>);

impl FileResolver for MemoryResolver {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| CoreError::unlocated(format!("missing {path}")))
    }
}

const LEAF: &str = "spec Leaf { state { flag: Bool } init { flag = false } action noop() {} }";

/// A compose whose invariant binds a type its component does not declare.
const UNKNOWN_ALIAS_TYPE: &str = r#"compose Outer {
  use Leaf as leaf from "leaf.fsl"
  state { x: Int }
  invariant Bad { forall u: leaf.Missing { true } }
}"#;

fn resolver(files: &[(&str, &str)]) -> MemoryResolver {
    MemoryResolver(
        files
            .iter()
            .map(|(path, source)| ((*path).to_owned(), (*source).to_owned()))
            .collect(),
    )
}

fn lowering_error(source: &str, files: &[(&str, &str)]) -> CoreError {
    parse_kernel_source(source, &resolver(files)).expect_err("lowering must fail")
}

#[test]
fn an_unknown_component_type_is_typed_unknown_type() {
    let error = lowering_error(UNKNOWN_ALIAS_TYPE, &[("leaf.fsl", LEAF)]);
    assert_eq!(error.kind, CoreErrorKind::UnknownType, "{error}");
}

#[test]
fn a_component_failure_is_typed_component_load_and_keeps_the_cause() {
    let error = lowering_error(
        r#"compose Top {
  use Dup as dup from "dup.fsl"
  state { y: Bool }
  init { y = false }
}"#,
        &[(
            "dup.fsl",
            "spec Dup { state { flag: Bool } init { flag = false } \
             def p() = flag def p() = flag action noop() {} }",
        )],
    );
    // The compose-level diagnostic keeps its own `semantics` class at the
    // parent's `use`; the component's classification survives as the cause.
    assert_eq!(
        error.kind,
        CoreErrorKind::ComponentLoad(Box::new(CoreErrorKind::NameResolution)),
        "{error}"
    );
}

#[test]
fn an_unregistered_ai_hard_rule_is_typed() {
    let error = lowering_error(
        r"ai_component UnknownRule {
  model m1;
  prompt p1;
  input InV1;
  output OutV1;
  tool SearchOrder {
    schema SearchOrderV1;
    precondition order_exists;
  }
  authority {
    may_execute SearchOrder;
  }
  check hard {
    rule totally_bogus_rule_name;
  }
}",
        &[],
    );
    assert_eq!(
        error.kind,
        CoreErrorKind::UnknownAiHardContractRule,
        "{error}"
    );
}

#[test]
fn an_unknown_domain_symbol_is_typed() {
    let error = lowering_error(
        r"domain InvalidUnusedDefault {
  value_object Unused { x: Int = missing; }
  aggregate Item {
    state { ready: Bool = false; }
    command Touch {}
    event Touched {}
    decide Touch { emits Touched }
    evolve Touched { ready = true }
  }
}",
        &[],
    );
    assert_eq!(error.kind, CoreErrorKind::UnknownDomainSymbol, "{error}");
}

#[test]
fn a_document_the_kernel_does_not_lower_is_typed() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/ai/recursive_support_agent.fsl"),
    )
    .expect("read agent example");
    let error = lowering_error(&source, &[]);
    assert_eq!(error.kind, CoreErrorKind::NotKernelLowerable, "{error}");
}

#[test]
fn a_scope_override_for_a_spec_without_bounds_is_typed() {
    let error = parse_kernel_source_with_bounds(
        LEAF,
        &BTreeMap::from([("User".to_owned(), 2)]),
        &BTreeMap::new(),
    )
    .expect_err("an override with nothing to bound must fail");
    assert_eq!(
        error.kind,
        CoreErrorKind::ScopeOverrideWithoutBounds,
        "{error}"
    );
}
