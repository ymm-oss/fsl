// SPDX-License-Identifier: Apache-2.0

use fsl_core::{
    FsResolver, PublicKernelVersion, build_model, parse_kernel_source,
    parse_kernel_source_with_file, public_kernel_contract_for_version,
};
use serde_json::Value;

const DOMAIN: &str = r"
domain Orders {
  enum Status { Pending, Approved }
  aggregate Order {
    state { status: Status = Pending; }
    command Approve {}
    event Approved {}
    decide Approve {
      requires status == Pending
      emits Approved
    }
    evolve Approved { status = Approved }
    invariant enabled { can(Approve) }
    invariant listed { status in [Pending, Approved] }
  }
}
";

fn contract() -> Value {
    let kernel =
        parse_kernel_source_with_file(DOMAIN, &FsResolver::new("fixtures"), "fixtures/orders.fsl")
            .expect("lower domain");
    let model = build_model(kernel.clone()).expect("build domain");
    public_kernel_contract_for_version(
        &kernel,
        &model,
        "fixtures/orders.fsl",
        "domain",
        PublicKernelVersion::V2,
    )
    .expect("export v2")
}

#[test]
fn v2_publishes_queryable_cardinality_and_explicit_assurance() {
    let first = contract();
    let second = contract();
    assert_eq!(first, second);
    assert_eq!(first["schema_version"], "2.0.0");
    assert_eq!(
        first["$schema"],
        "https://fsl.dev/schemas/fslc/kernel/kernel.v2.schema.json"
    );
    assert_eq!(
        first["actions"][0]["origin"]["target"],
        "action:order_approve"
    );
    assert!(first["properties"]["invariants"][0]["requirement"].is_object());
    assert!(first["actions"][0]["origin"].get("declaration").is_none());

    let origins = first["provenance"]["origins"]
        .as_array()
        .expect("origin records");
    let expanded = origins
        .iter()
        .find(|origin| {
            origin["lowering_steps"]
                .as_array()
                .is_some_and(|steps| steps.iter().any(|step| step["kind"] == "expand_can"))
        })
        .expect("can origin");
    let reverse = first["provenance"]["reverse_index"]
        .as_array()
        .expect("reverse index")
        .iter()
        .find(|entry| entry["source_node_id"] == expanded["source_node_id"])
        .expect("reverse entry");
    assert!(reverse["targets"].as_array().expect("targets").len() >= 3);

    assert!(
        origins.iter().any(|origin| {
            origin["assurance"] == "generated_only" && origin["primary"].is_null()
        })
    );
    assert!(origins.iter().any(|origin| {
        origin["assurance"] == "generated_from_source"
            && !origin["secondary"]
                .as_array()
                .expect("secondary")
                .is_empty()
    }));
}

#[test]
fn v2_coordinates_and_source_identity_are_portable() {
    let contract = contract();
    let source_site = contract["provenance"]["origins"]
        .as_array()
        .expect("origins")
        .iter()
        .find_map(|origin| origin["primary"].as_object())
        .expect("primary source");
    assert_eq!(source_site["source"]["kind"], "repository_path");
    assert_eq!(source_site["source"]["value"], "fixtures/orders.fsl");
    assert!(source_site["span"]["byte_start"].as_u64().is_some());
    assert!(source_site["span"]["byte_end"].as_u64() > source_site["span"]["byte_start"].as_u64());
    assert!(
        source_site["span"]["line"]
            .as_u64()
            .is_some_and(|line| line >= 2)
    );
}

#[test]
fn v2_fails_closed_on_absolute_developer_paths_and_unsupported_majors() {
    let kernel =
        parse_kernel_source_with_file(DOMAIN, &FsResolver::new("fixtures"), "fixtures/orders.fsl")
            .expect("lower domain");
    let model = build_model(kernel.clone()).expect("build domain");
    let error = public_kernel_contract_for_version(
        &kernel,
        &model,
        "/absolute/orders.fsl",
        "domain",
        PublicKernelVersion::V2,
    )
    .expect_err("absolute path must fail");
    assert!(error.message.contains("repository-relative"));
    assert!(PublicKernelVersion::parse("3").is_err());
    assert!(PublicKernelVersion::parse("2.0.0").is_err());
}

#[test]
fn v2_reports_unknown_completeness_instead_of_inventing_direct_kernel_origins() {
    let source = r"
spec Direct {
  state { ready: Bool }
  init { ready = false }
  action enable() { ready = true }
  invariant Valid { ready == true or ready == false }
}
";
    let kernel =
        parse_kernel_source_with_file(source, &FsResolver::new("fixtures"), "fixtures/direct.fsl")
            .expect("parse direct Kernel");
    let model = build_model(kernel.clone()).expect("build direct Kernel");
    let contract = public_kernel_contract_for_version(
        &kernel,
        &model,
        "fixtures/direct.fsl",
        "kernel",
        PublicKernelVersion::V2,
    )
    .expect("export direct Kernel v2");
    assert_eq!(contract["provenance"]["completeness"], "unknown");
    assert!(
        contract["provenance"]["origins"]
            .as_array()
            .is_some_and(|origins| origins
                .iter()
                .all(|origin| origin["assurance"] == "unknown"))
    );
}

#[test]
fn v2_normalizes_partial_source_sites_to_schema_valid_unknown_records() {
    let kernel = parse_kernel_source(DOMAIN, &FsResolver::new("fixtures"))
        .expect("lower domain without source identity");
    let model = build_model(kernel.clone()).expect("build domain");
    let contract = public_kernel_contract_for_version(
        &kernel,
        &model,
        "fixtures/orders.fsl",
        "domain",
        PublicKernelVersion::V2,
    )
    .expect("export v2 with partial internal origins");
    let unknown = contract["provenance"]["origins"]
        .as_array()
        .expect("origins")
        .iter()
        .filter(|origin| origin["assurance"] == "unknown")
        .collect::<Vec<_>>();
    assert!(!unknown.is_empty());
    assert!(unknown.iter().all(|origin| origin["primary"].is_null()));
}
