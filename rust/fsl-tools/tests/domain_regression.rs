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
        fsl_tools::analyze_domain(&domain).expect("analyze programmatic explicit role")["effects"]
            [0]["outcomes"],
        serde_json::json!(["Finished"])
    );
    assert_eq!(
        fsl_tools::domain_scaffold_metadata(&domain)["effects"][0]["outcomes"],
        serde_json::json!(["Finished"])
    );
}

/// #726, N4: `fsl_tools::analyze_domain`'s public contract is
/// `Result<Value, CoreError>`, not `Value` -- an unlowerable domain document
/// must be rejected by the API itself, not only by the `fslc domain
/// analyze` CLI integration test in `rust/fslc/tests/`. Without an
/// `fsl-tools`-level negative control here, a future crate that stops going
/// through the CLI (a second `analyze_domain` caller) would have no test at
/// this layer protecting the type-level fail-closed guarantee the PR body
/// claims.
#[test]
fn analyze_domain_rejects_an_unlowerable_construct() {
    let source = include_str!("../../fslc/tests/fixtures/domain_await_routing_rejected.fsl");
    let fsl_syntax::SurfaceDocument::Domain(domain) =
        fsl_syntax::parse_surface_document(source).expect("parse domain with top-level await")
    else {
        panic!("expected domain document");
    };
    let error = fsl_tools::analyze_domain(&domain)
        .expect_err("analyze_domain must reject a top-level await routing construct");
    assert!(
        error
            .message
            .contains("top-level await 'PaymentResult' has no executable lowering"),
        "{error:?}"
    );
}

/// Parse `source` as a domain document and return the
/// `reliable_effect_without_outbox_boundary` finding, if `fslc domain analyze`
/// reports one.
fn reliable_effect_finding(source: &str) -> Option<serde_json::Value> {
    let fsl_syntax::SurfaceDocument::Domain(domain) =
        fsl_syntax::parse_surface_document(source).expect("parse valid domain")
    else {
        panic!("expected domain document");
    };
    fsl_tools::analyze_domain(&domain)
        .expect("analyze domain for reliable-effect finding")["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .find(|finding| finding["kind"] == "reliable_effect_without_outbox_boundary")
        .cloned()
}

// #723: `reliable_effect_without_outbox_boundary` must honor DESIGN-effect.md's
// "outbox on the effect *or owning saga*" contract, where "owning" means at
// least one saga step emits the effect's request event. The frozen Python
// reference instead treats *any* saga's outbox as satisfying every reliable
// effect (`src/fslc/domain_expand.py`'s `any(saga.outboxes for saga in
// domain.sagas)`), which is looser than the accepted design. These cases are
// deliberately not aligned with that Python behavior.

#[test]
fn c1_reliable_effect_without_outbox_or_saga_fires() {
    let source = r"
domain C1 {
  type Id = 0..0
  aggregate Item {
    event Requested { id: Id }
  }
  effect Ship {
    reliable
    handles Requested
  }
}
";
    let finding = reliable_effect_finding(source).expect("finding must fire");
    assert_eq!(finding["witness"], serde_json::json!({"effect":"Ship"}));
}

#[test]
fn c2_owning_saga_outbox_clears_the_finding() {
    let source = r"
domain C2 {
  type Id = 0..0
  aggregate Item {
    event Requested { id: Id }
  }
  effect Ship {
    reliable
    handles Requested
  }
  saga Fulfillment {
    outbox FulfillmentOutbox
    step Notify {
      emits Requested
    }
  }
}
";
    assert_eq!(reliable_effect_finding(source), None);
}

#[test]
fn c3_unrelated_saga_outbox_does_not_clear_the_finding() {
    // Negative control: an unrelated saga's outbox must not silence the
    // warning for an *owning* saga that has none. A predicate that degrades
    // to "any saga has an outbox" (the frozen Python reference's rule) would
    // wrongly clear this finding.
    let source = r"
domain C3 {
  type Id = 0..0
  aggregate Item {
    event Requested { id: Id }
    event Other { id: Id }
  }
  effect Ship {
    reliable
    handles Requested
  }
  saga Owning {
    step Notify {
      emits Requested
    }
  }
  saga Unrelated {
    outbox UnrelatedOutbox
    step DoOther {
      emits Other
    }
  }
}
";
    let finding = reliable_effect_finding(source).expect("finding must still fire");
    assert_eq!(
        finding["witness"],
        serde_json::json!({"effect":"Ship","uncovered_sagas":["Owning"]})
    );
}

#[test]
fn c4_partially_covered_owning_sagas_fires_with_only_uncovered_names() {
    let source = r"
domain C4 {
  type Id = 0..0
  aggregate Item {
    event Requested { id: Id }
  }
  effect Ship {
    reliable
    handles Requested
  }
  saga First {
    outbox FirstOutbox
    step Notify {
      emits Requested
    }
  }
  saga Second {
    step NotifyToo {
      emits Requested
    }
  }
}
";
    let finding = reliable_effect_finding(source).expect("finding must fire");
    assert_eq!(
        finding["witness"],
        serde_json::json!({"effect":"Ship","uncovered_sagas":["Second"]})
    );
}

#[test]
fn c5_effect_outbox_clears_the_finding() {
    let source = r"
domain C5 {
  type Id = 0..0
  aggregate Item {
    event Requested { id: Id }
  }
  effect Ship {
    reliable
    outbox ShipOutbox
    handles Requested
  }
}
";
    assert_eq!(reliable_effect_finding(source), None);
}
