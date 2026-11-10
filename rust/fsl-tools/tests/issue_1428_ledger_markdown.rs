// SPDX-License-Identifier: Apache-2.0

#[test]
fn ledger_cr_html_detector() {
    let source = "spec Demo { state { x: Bool } init { x = false } action act() { x = true } }";
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new(root)).unwrap();
    let model = fsl_core::build_model(kernel).unwrap();
    let report = fsl_tools::render_ledger(
        "spec.fsl",
        &model,
        &serde_json::json!({"result":"verified", "checked_to_depth":2, "warnings":[{"kind":"untagged", "requirement":{"id":"REQ-1", "text":"日本語😀 <!-- hidden --> `x`|\r\n# forged"}}]}),
        &serde_json::json!({}),
        None,
        &[],
    );
    assert!(
        report.contains("| REQ-1 | 日本語😀 &lt;!-- hidden --&gt; \\`x\\`\\|&#13;&#10;# forged |")
    );
    assert!(!report.contains('\r'));
}
