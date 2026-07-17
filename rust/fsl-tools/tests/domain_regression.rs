// SPDX-License-Identifier: Apache-2.0

#[test]
fn domain_expansion_preserves_effect_state_and_actions() {
    let source = r"
domain Example {
  type Id = 0..1
  type Status = New | Done
  aggregate Item {
    id Id
    state { status: Status = New; }
    command Complete {}
    event Completed { id: Id }
    error AlreadyDone
    decide Complete {
      rejects AlreadyDone when status == Done
      emits Completed
    }
    evolve Completed { status = Done }
  }
}
";
    let document = fsl_syntax::parse_surface_document(source).expect("parse domain");
    let fsl_syntax::SurfaceDocument::Domain(domain) = document else {
        panic!("expected domain document");
    };
    let expanded = fsl_tools::domain_kernel_source(&domain);
    assert!(expanded.contains("enum Status { Status_New, Status_Done }"));
    assert!(expanded.contains("action item_complete()"));
    assert!(expanded.contains("item_status = Status_Done"));
    fsl_core::parse_kernel_source(&expanded, &fsl_core::FsResolver::new("."))
        .expect("expanded source remains valid FSL");
}
