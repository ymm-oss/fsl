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
    let expanded = fsl_tools::domain_kernel_source(&domain).expect("render domain kernel");
    assert!(expanded.contains("enum Status { Status_New, Status_Done }"));
    assert!(expanded.contains("action item_complete()"));
    assert!(expanded.contains("item_status = Status_Done"));
    fsl_core::parse_kernel_source(&expanded, &fsl_core::FsResolver::new("."))
        .expect("expanded source remains valid FSL");
}

#[test]
fn domain_check_rejects_programmatic_outcome_role_conflict() {
    let source = r"
domain ProgrammaticConflict {
  type Id = 0..0
  aggregate Item {
    event Requested { id: Id }
    event Finished { id: Id }
  }
  effect Work {
    async
    correlation_id Requested.id
    handles Requested
    success_event Finished
  }
}
";
    let fsl_syntax::SurfaceDocument::Domain(mut domain) =
        fsl_syntax::parse_surface_document(source).expect("parse valid domain")
    else {
        panic!("expected domain document");
    };
    domain.effects[0].failure_event = Some("Finished".to_owned());

    let error = fsl_tools::check_domain(&domain, &serde_json::json!({}))
        .expect_err("domain check must reject conflicting roles");
    assert!(
        error
            .message
            .contains("effect outcome event 'Finished' has multiple explicit roles")
    );
}

#[test]
fn domain_outputs_normalize_programmatic_explicit_role() {
    let source = r"
domain ProgrammaticRole {
  type Id = 0..0
  aggregate Item {
    event Requested { id: Id }
    event Finished {}
  }
  effect Work {
    async
    correlation_id Requested.id
    handles Requested
  }
}
";
    let fsl_syntax::SurfaceDocument::Domain(mut domain) =
        fsl_syntax::parse_surface_document(source).expect("parse valid domain")
    else {
        panic!("expected domain document");
    };
    domain.effects[0].success_event = Some("Finished".to_owned());

    let checked = fsl_tools::check_domain(&domain, &serde_json::json!({}))
        .expect("check programmatic explicit role");
    assert!(
        checked["generated_actions"]
            .as_array()
            .expect("generated actions")
            .contains(&serde_json::json!("work_complete_finished"))
    );
    assert_eq!(
        fsl_tools::analyze_domain(&domain)["effects"][0]["outcomes"],
        serde_json::json!(["Finished"])
    );
    assert_eq!(
        fsl_tools::domain_scaffold_metadata(&domain)["effects"][0]["outcomes"],
        serde_json::json!(["Finished"])
    );
}
