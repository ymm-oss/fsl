// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #818: a compose document with an undeclared
//! component alias inside a top-level `init` block's `forall` binder used to
//! panic the process (`rewrite_compose_statements`'s
//! `.expect("compose alias validation occurs during lowering")` in
//! `rust/fsl-core/src/compose.rs`) instead of surfacing the `CoreError` its
//! own signature promised. The panic unwound past the public API boundary,
//! so a caller of `fsl_core::parse_kernel_source` got no `Result` to handle.
//!
//! Both controls go through `parse_kernel_source` directly -- the public
//! entry point named in the issue -- not an internal helper, because the
//! defect was specifically that the panic escaped that boundary.

use std::collections::BTreeMap;

use fsl_core::{CoreError, FileResolver, parse_kernel_source};

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
    type UserId = 0..1 \
    state { flag: Map<UserId, Bool> } \
    init { forall u: UserId { flag[u] = false } } \
    action noop() {} \
}";

fn resolver() -> MemoryResolver {
    MemoryResolver(BTreeMap::from([(
        "core.fsl".to_owned(),
        CORE_COMPONENT.to_owned(),
    )]))
}

/// Rejecting control: an undeclared alias referenced in a top-level `init`
/// block's `forall` binder must return `Err(CoreError)` through
/// `parse_kernel_source`, not panic.
#[test]
fn undeclared_alias_in_forall_binder_returns_core_error_not_panic() {
    let source = "compose Broken { \
        state { x: Int } \
        init { forall u: nonexistent.UserId { x = 0 } } \
    }";

    let error = parse_kernel_source(source, &resolver())
        .expect_err("an undeclared alias must fail, not panic");

    assert_eq!(error.message, "unknown alias 'nonexistent'");
    // The statement span now gives the qualified-name resolver the real
    // source position instead of the former (1, 1) placeholder.
    assert_eq!(error.line, 1);
    assert_eq!(error.column, 42);
}

/// Rejecting detector for issue #832 shape A: a declared alias does not make
/// an absent member a valid binder type. The diagnostic keeps author spelling
/// and the `forall` statement's real location.
#[test]
fn declared_alias_with_unknown_forall_type_returns_located_core_error() {
    let source = r#"compose Broken {
  use Core as core from "core.fsl"
  state { x: Int }
  init {
    forall u: core.NoSuchType {
      x = 0
    }
  }
}"#;

    let error = parse_kernel_source(source, &resolver())
        .expect_err("an unknown member of a declared alias must fail during lowering");

    assert_eq!(error.message, "unknown type 'core.NoSuchType'");
    assert_eq!(error.line, 5);
    assert_eq!(error.column, 5);
    assert!(
        error
            .origin
            .as_deref()
            .is_some_and(|origin| origin.id.0.starts_with("compose:error:"))
    );
}

/// Rejecting detector for the expression-binder path: a qualified type in an
/// invariant must retain the authored property's location while resolving the
/// alias member.
#[test]
fn declared_alias_with_unknown_invariant_binder_returns_located_core_error() {
    let source = r#"compose Broken {
  use Core as core from "core.fsl"
  state { x: Int }
    invariant Bad { forall u: core.NoSuchType { true } }
}"#;

    let error = parse_kernel_source(source, &resolver())
        .expect_err("an unknown invariant binder member must fail during lowering");

    assert_eq!(error.message, "unknown type 'core.NoSuchType'");
    assert_eq!(error.line, 4);
    assert_eq!(error.column, 5);
    let origin = error.origin.expect("authored compose error origin");
    assert_eq!(
        origin
            .primary
            .and_then(|site| site.span)
            .map(|span| (span.start.line, span.start.column)),
        Some((4, 5))
    );
}

/// Accepting control: a correctly declared alias used the same way (in a
/// top-level `init` block's `forall` binder) must still lower successfully,
/// so the fix does not reject valid compose documents.
#[test]
fn declared_alias_in_forall_binder_still_lowers() {
    let source = "compose Works { \
        use Core as core from \"core.fsl\" \
        state { x: Int } \
        init { forall u: core.UserId { x = 0 } } \
    }";

    parse_kernel_source(source, &resolver())
        .expect("a declared alias in a forall binder must lower successfully");
}
