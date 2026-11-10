// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use serde_json::Value;

// Validate the complete source-specific findings before comparing other warnings.
// Different fixture paths/offsets cannot be equal; no diagnostic field is ignored.
pub fn take_migration_warnings(
    envelope: &mut Value,
    source_path: &Path,
    display_path: &str,
    codes: &[&str],
) {
    let source = std::fs::read_to_string(source_path).expect("fixture");
    let plan = fslc_rust::migration::plan_migration(
        &source,
        display_path,
        fsl_syntax::Edition::Fsl4,
        &fsl_syntax::ParseOptions::default(),
    )
    .expect("migration plan");
    assert_eq!(
        plan.diagnostics.iter().map(|f| f.code).collect::<Vec<_>>(),
        codes
    );
    assert!(!codes.is_empty(), "source-specific comparison must be live");
    let expected = plan
        .diagnostics
        .iter()
        .map(|finding| finding.json(display_path, fsl_syntax::Edition::Fsl4))
        .collect::<Vec<_>>();
    let warnings = envelope["warnings"].as_array_mut().expect("warnings");
    assert!(warnings.len() >= expected.len());
    let split = warnings.len() - expected.len();
    assert_eq!(warnings[split..], expected);
    warnings.truncate(split);
}
