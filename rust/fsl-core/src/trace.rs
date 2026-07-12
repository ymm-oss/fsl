// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::FslValue;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceAction {
    pub name: String,
    pub params: BTreeMap<String, FslValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceChange {
    pub from: FslValue,
    pub to: FslValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceStep {
    pub step: usize,
    pub state: BTreeMap<String, FslValue>,
    pub action: Option<TraceAction>,
    pub changes: BTreeMap<String, TraceChange>,
}
