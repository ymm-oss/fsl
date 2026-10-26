// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::KernelModel;
use serde_json::Value;

/// A presentation-only view; the verifier's raw coverage evidence is unchanged.
#[must_use]
pub fn source_action_coverage<'a>(
    model: &KernelModel,
    coverage: &'a Value,
) -> Option<BTreeMap<String, &'a Value>> {
    coverage.as_object().map(|values| {
        model
            .actions
            .iter()
            .filter(|action| model.action_source_span(action).is_some())
            .filter_map(|action| {
                let name = model.action_display_name(&action.name);
                values.get(&name).map(|value| (name, value))
            })
            .collect()
    })
}
