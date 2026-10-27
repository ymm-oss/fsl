// SPDX-License-Identifier: Apache-2.0

use crate::{KernelModel, KernelSpec, PublicKernelVersion};

#[test]
fn source_pattern_context_is_owned_check_only_data() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<KernelSpec>();
    send_sync::<KernelModel>();
    let mut kernel = crate::parse_direct_kernel_spec(
        "spec P { state { done: Bool } init { done = false } action go() {} }",
    )
    .unwrap();
    let ast = serde_json::to_vec(&kernel.kernel_ast_v1()).unwrap();
    let model = crate::build_model(kernel.clone()).unwrap();
    let versions = [PublicKernelVersion::V1, PublicKernelVersion::V2];
    let before = versions.map(|version| {
        crate::public_kernel_contract_for_version(&kernel, &model, "p.fsl", "kernel", version)
            .unwrap()
    });
    kernel.names.pattern_scopes.insert(
        "unused_context".into(),
        Ok(crate::typecheck::PatternBinding::Fresh),
    );
    let mut copied = kernel.clone();
    copied.names.pattern_scopes.clear();
    assert_eq!(
        kernel.names.pattern_scopes.len(),
        1,
        "clone must own its data"
    );
    assert_eq!(serde_json::to_vec(&kernel.kernel_ast_v1()).unwrap(), ast);
    let checked = crate::build_model(kernel.clone()).unwrap();
    assert!(checked.names().pattern_scopes.is_empty());
    for (version, expected) in versions.into_iter().zip(before) {
        let actual = crate::public_kernel_contract_for_version(
            &kernel, &checked, "p.fsl", "kernel", version,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&actual).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
    }
}
