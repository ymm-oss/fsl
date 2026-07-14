// SPDX-License-Identifier: Apache-2.0

//! Mechanical Public Kernel v2 provenance coverage evidence.

use fsl_core::{
    FsResolver, PublicKernelVersion, build_model, parse_kernel_source_with_file,
    public_kernel_contract_for_version,
};
use serde_json::{Value, json};

pub const ORIGIN_COVERAGE_SCHEMA_VERSION: &str = "2.0.0";
pub const ORIGIN_COVERAGE_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/conformance-coverage.v2.schema.json";
const FIXTURE_FILE: &str = "rust/fslc/tests/fixtures/domain_characterization/expressions_valid.fsl";
const FIXTURE_SOURCE: &str =
    include_str!("../tests/fixtures/domain_characterization/expressions_valid.fsl");

fn evidence(key: &str, description: &str, detail: &str, count: usize) -> Value {
    json!({
        "key":key,
        "description":description,
        "level":"exercised",
        "evidence":[{"fixture":FIXTURE_FILE,"detail":detail,"count":count}],
    })
}

/// Build a deterministic coverage matrix whose rows are derived from an actual
/// domain v2 projection and corresponding v2 conformance vectors.
///
/// # Errors
///
/// Returns an error if the fixture cannot be lowered or any required provenance
/// feature lacks structural evidence.
#[allow(clippy::too_many_lines)]
pub fn origin_coverage_matrix_v2() -> Result<Value, String> {
    let kernel = parse_kernel_source_with_file(
        FIXTURE_SOURCE,
        &FsResolver::new("rust/fslc/tests/fixtures/domain_characterization"),
        FIXTURE_FILE,
    )
    .map_err(|error| error.to_string())?;
    let model = build_model(kernel.clone()).map_err(|error| error.to_string())?;
    let contract = public_kernel_contract_for_version(
        &kernel,
        &model,
        FIXTURE_FILE,
        "domain",
        PublicKernelVersion::V2,
    )
    .map_err(|error| error.to_string())?;
    let vectors = crate::conformance_vectors_for_version(&model, 0, PublicKernelVersion::V2)?;
    let origins = contract["provenance"]["origins"]
        .as_array()
        .ok_or_else(|| "v2 provenance has no origins".to_owned())?;
    let reverse = contract["provenance"]["reverse_index"]
        .as_array()
        .ok_or_else(|| "v2 provenance has no reverse index".to_owned())?;

    let rows = vec![
        evidence(
            "portable_source_identity",
            "source sites use typed repository-relative identities",
            "origin.primary.source.kind == repository_path",
            origins
                .iter()
                .filter(|origin| origin["primary"]["source"]["kind"] == "repository_path")
                .count(),
        ),
        evidence(
            "utf8_byte_and_unicode_coordinates",
            "source sites publish UTF-8 bytes and Unicode line/column coordinates",
            "primary span has byte and line coordinates",
            origins
                .iter()
                .filter(|origin| {
                    origin["primary"]["span"]["byte_start"].is_number()
                        && origin["primary"]["span"]["line"].is_number()
                })
                .count(),
        ),
        evidence(
            "one_to_many_reverse_lookup",
            "one source node maps to multiple Kernel targets",
            "reverse-index entry has multiple targets",
            reverse
                .iter()
                .filter(|entry| {
                    entry["targets"]
                        .as_array()
                        .is_some_and(|targets| targets.len() > 1)
                })
                .count(),
        ),
        evidence(
            "many_to_one_secondary_origins",
            "one Kernel target retains secondary source sites",
            "origin record has secondary sites",
            origins
                .iter()
                .filter(|origin| {
                    origin["secondary"]
                        .as_array()
                        .is_some_and(|secondary| !secondary.is_empty())
                })
                .count(),
        ),
        evidence(
            "generated_from_source",
            "generated Kernel targets retain source-backed assurance",
            "assurance == generated_from_source",
            origins
                .iter()
                .filter(|origin| origin["assurance"] == "generated_from_source")
                .count(),
        ),
        evidence(
            "generated_only",
            "source-less generated targets remain explicit",
            "assurance == generated_only and primary == null",
            origins
                .iter()
                .filter(|origin| {
                    origin["assurance"] == "generated_only" && origin["primary"].is_null()
                })
                .count(),
        ),
        evidence(
            "lowering_step_can",
            "can() rewrites remain queryable",
            "lowering steps include expand_can",
            origins
                .iter()
                .filter(|origin| {
                    origin["lowering_steps"]
                        .as_array()
                        .is_some_and(|steps| steps.iter().any(|step| step["kind"] == "expand_can"))
                })
                .count(),
        ),
        evidence(
            "lowering_step_membership",
            "finite-membership rewrites remain queryable",
            "lowering steps include expand_membership",
            origins
                .iter()
                .filter(|origin| {
                    origin["lowering_steps"].as_array().is_some_and(|steps| {
                        steps.iter().any(|step| step["kind"] == "expand_membership")
                    })
                })
                .count(),
        ),
        evidence(
            "requirement_provenance_separation",
            "requirement relations and provenance references are distinct fields",
            "property has requirement object and origin target only",
            contract["properties"]["invariants"]
                .as_array()
                .map_or(0, |properties| {
                    properties
                        .iter()
                        .filter(|property| {
                            property["requirement"].is_object()
                                && property["origin"]["target"].is_string()
                                && property["origin"].get("declaration").is_none()
                        })
                        .count()
                }),
        ),
    ];
    let missing = rows
        .iter()
        .filter(|row| row["evidence"][0]["count"] == 0)
        .filter_map(|row| row["key"].as_str())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "Public Kernel v2 provenance coverage is missing: {}",
            missing.join(", ")
        ));
    }
    Ok(json!({
        "$schema":ORIGIN_COVERAGE_SCHEMA_ID,
        "schema_version":ORIGIN_COVERAGE_SCHEMA_VERSION,
        "kernel_schema_version":fsl_core::KERNEL_V2_SCHEMA_VERSION,
        "conformance_schema_version":crate::CONFORMANCE_V2_SCHEMA_VERSION,
        "result":"conformance_coverage",
        "fixtures":[{
            "file":FIXTURE_FILE,
            "depth":0,
            "states":vectors["states"].as_array().map_or(0,Vec::len),
            "vectors":vectors["vectors"].as_array().map_or(0,Vec::len)
        }],
        "features":rows,
    }))
}
