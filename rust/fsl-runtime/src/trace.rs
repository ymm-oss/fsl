// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Shared parent-link trace reconstruction, used by any BFS that must avoid
//! cloning a full trace at every visited node (issue #697).
//!
//! A per-node `Vec<TraceStep>` clone makes a BFS frontier's memory grow with
//! both branching factor *and* path length, on top of whatever it costs to
//! clone the node's own state. Recording only a `(parent State, TraceAction)`
//! link per newly discovered state and walking that chain backward once --
//! only when a trace is actually needed -- keeps per-node memory bounded by
//! the state and one action, not the whole path so far. `explicit.rs`
//! originated this pattern for `verify_explicit_selected`; `find_boundary_violation`
//! reuses it rather than duplicating it.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::rc::Rc;

use fsl_core::{FslValue as Value, TraceAction, TraceChange, TraceStep};

use super::State;

/// A step-ordered BFS frontier holding only `(State, usize)` per queued
/// node -- never a `Monitor` (a whole `KernelModel` clone) or a
/// `Vec<TraceStep>` (a per-node trace clone). Adding either back to a
/// lane's frontier is exactly the #730/#697/#783 regression this type
/// exists to make harder: a lane that starts needing more than a state and
/// a depth should reach for `ParentLink`/[`reconstruct_trace`] plus a
/// re-pointed scratch `Monitor`, not a wider queue element here.
///
/// This is a type-level guardrail for the *consuming* lane's own queue
/// declaration, not a compile-time proof that no lane anywhere clones a
/// `Monitor` per node: a lane that declares its own `VecDeque<(Monitor,
/// ..)>` instead of using this type is not stopped by it. Pair any change
/// to a lane using `LeanFrontier` with its ceiling regression test
/// (`tests/issue_730_bfs_memory_ceiling.rs`'s pattern) in review.
///
/// `Node` is the `State` itself, or a [`StateStore`] id for a lane that keeps
/// its states packed (the refinement walk, issue #1247): either way a node is
/// a state and a depth, nothing wider.
#[derive(Debug, Default)]
pub(crate) struct LeanFrontier<Node = State> {
    queue: VecDeque<(Node, usize)>,
}

impl<Node> LeanFrontier<Node> {
    pub(crate) fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub(crate) fn push(&mut self, node: Node, step: usize) {
        self.queue.push_back((node, step));
    }

    pub(crate) fn pop(&mut self) -> Option<(Node, usize)> {
        self.queue.pop_front()
    }

    /// The step of the node at the front of the queue, or `None` when the
    /// queue is empty.
    pub(crate) fn front_step(&self) -> Option<usize> {
        self.queue.front().map(|(_, step)| *step)
    }
}

/// A step-ordered BFS frontier for a lane whose per-node `Vec<TraceStep>`
/// is semantically required -- a path-dependent search with no `visited`
/// dedup, where two routes reaching the same state must stay distinct
/// path-trees rather than merge behind a shared [`ParentLink`]
/// (`leadsto_response_traces`'s `pending`/response history is exactly this:
/// it is a property of the *route*, not the state) -- but which must still
/// never carry a whole `Monitor` per node. Holds only `(State,
/// Vec<TraceStep>, usize)`; adding a `Monitor` back is exactly the
/// #730/#783 regression this type exists to make harder.
///
/// Same caveat as [`LeanFrontier`]: this is a type-level guardrail for the
/// *consuming* lane's own queue declaration, not a compile-time proof that
/// no lane anywhere clones a `Monitor` per node.
#[derive(Debug, Default)]
pub(crate) struct PathFrontier {
    queue: VecDeque<(State, Vec<TraceStep>, usize)>,
}

impl PathFrontier {
    pub(crate) fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub(crate) fn push(&mut self, state: State, trace: Vec<TraceStep>, step: usize) {
        self.queue.push_back((state, trace, step));
    }

    pub(crate) fn pop(&mut self) -> Option<(State, Vec<TraceStep>, usize)> {
        self.queue.pop_front()
    }
}

/// One step back from a state to its BFS parent and the action that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParentLink {
    pub(crate) parent: State,
    pub(crate) action: TraceAction,
}

/// Walk `parents` back from `final_state` to its root -- the parentless
/// state the chain terminates at -- and render the full forward
/// [`TraceStep`] sequence, recomputing each step's field-level changes from
/// the two states it connects.
///
/// The root is discovered by the walk itself rather than taken as a
/// parameter, so this works unchanged for a multi-root search (issue #493):
/// as long as every root is left out of `parents` (only non-root states get
/// a `ParentLink` entry), the walk from any state -- reached from any root
/// -- stops at the actual root it descends from, not a caller-assumed
/// single initial state.
pub(crate) fn reconstruct_trace(
    final_state: &State,
    parents: &BTreeMap<State, ParentLink>,
) -> Vec<TraceStep> {
    let mut cursor = final_state.clone();
    let mut reversed = Vec::<(State, TraceAction)>::new();
    while let Some(link) = parents.get(&cursor) {
        reversed.push((cursor, link.action.clone()));
        cursor = link.parent.clone();
    }
    reversed.reverse();
    let root_state = cursor;
    let mut trace = vec![TraceStep {
        step: 0,
        state: root_state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    let mut before = root_state;
    for (index, (state, action)) in reversed.into_iter().enumerate() {
        trace.push(TraceStep {
            step: index + 1,
            changes: state_changes(&before, &state),
            state: state.clone(),
            action: Some(action),
        });
        before = state;
    }
    trace
}

/// The field-level changes `after` made relative to `before`, keyed by state
/// variable name.
pub(crate) fn state_changes(before: &State, after: &State) -> BTreeMap<String, TraceChange> {
    after
        .iter()
        .filter_map(|(name, value)| {
            let old = &before[name];
            (old != value).then(|| {
                (
                    name.clone(),
                    TraceChange {
                        from: old.clone(),
                        to: value.clone(),
                    },
                )
            })
        })
        .collect()
}

/// A search's visited set and parent links over packed states (issue #1247).
///
/// `BTreeSet<State>` plus `BTreeMap<State, ParentLink>` held each reached
/// state three times as a full `State` (the visited entry, the parents key,
/// and the child's `ParentLink.parent`), about 6.7 KB apiece on the e2e repro
/// (a `BTreeMap` leaf allocates eleven slots for two or three entries, and
/// every enum and struct value carries its type and member names as
/// `String`s). Here each state is stored once, as its canonical byte
/// encoding, and a parent link is two indices: the parent's id and the
/// interned action instance that produced the child.
///
/// The encoding is injective, so membership is exactly `State` equality. The
/// store is only ever asked for membership and for one state's parent chain;
/// its iteration order is never observed, which is what lets the index be a
/// `HashMap`.
#[derive(Debug, Default)]
pub(crate) struct StateStore {
    ids: HashMap<Rc<[u8]>, usize>,
    nodes: Vec<StoredState>,
    action_ids: HashMap<Rc<[u8]>, usize>,
    actions: Vec<TraceAction>,
    scratch: Vec<u8>,
}

#[derive(Debug)]
struct StoredState {
    bytes: Rc<[u8]>,
    /// `(parent id, action id)`; `None` for a root, as a root has no
    /// `ParentLink` in [`reconstruct_trace`]'s multi-root contract.
    parent: Option<(usize, usize)>,
}

impl StateStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The number of distinct states stored (a `visited.len()`).
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Store `state` as a root unless it is already stored. Returns its id
    /// when it was new.
    pub(crate) fn insert_root(&mut self, state: &State) -> Option<usize> {
        self.insert(state, None)
    }

    /// Store `state`, reached from the stored state `parent` by `action`,
    /// unless it is already stored (its first link then stays). Returns its
    /// id when it was new.
    pub(crate) fn insert_child(
        &mut self,
        state: &State,
        parent: usize,
        action: &str,
        params: &BTreeMap<String, Value>,
    ) -> Option<usize> {
        encode_state(state, &mut self.scratch);
        if self.ids.contains_key(self.scratch.as_slice()) {
            return None;
        }
        let action = self.action_id(action, params);
        Some(self.insert_encoded(Some((parent, action))))
    }

    /// The stored state `id`.
    pub(crate) fn state(&self, id: usize) -> State {
        decode_state(&self.nodes[id].bytes)
    }

    /// [`reconstruct_trace`] for the stored state `id`: the same steps, from
    /// the root its parent chain ends at.
    pub(crate) fn trace(&self, id: usize) -> Vec<TraceStep> {
        let mut chain = vec![id];
        let mut cursor = id;
        while let Some((parent, _)) = self.nodes[cursor].parent {
            chain.push(parent);
            cursor = parent;
        }
        chain.reverse();
        let mut before = self.state(chain[0]);
        let mut trace = vec![TraceStep {
            step: 0,
            state: before.clone(),
            action: None,
            changes: BTreeMap::new(),
        }];
        for (index, &node) in chain.iter().enumerate().skip(1) {
            let state = self.state(node);
            let action = self.nodes[node].parent.map(|(_, action)| action);
            trace.push(TraceStep {
                step: index,
                changes: state_changes(&before, &state),
                state: state.clone(),
                action: action.map(|action| self.actions[action].clone()),
            });
            before = state;
        }
        trace
    }

    fn insert(&mut self, state: &State, parent: Option<(usize, usize)>) -> Option<usize> {
        encode_state(state, &mut self.scratch);
        if self.ids.contains_key(self.scratch.as_slice()) {
            return None;
        }
        Some(self.insert_encoded(parent))
    }

    /// Store the state encoded in `scratch`, known not to be stored yet.
    fn insert_encoded(&mut self, parent: Option<(usize, usize)>) -> usize {
        let id = self.nodes.len();
        let bytes: Rc<[u8]> = Rc::from(self.scratch.as_slice());
        self.ids.insert(Rc::clone(&bytes), id);
        self.nodes.push(StoredState { bytes, parent });
        id
    }

    fn action_id(&mut self, name: &str, params: &BTreeMap<String, Value>) -> usize {
        let mut key = Vec::new();
        put_str(name, &mut key);
        put_len(params.len(), &mut key);
        for (param, value) in params {
            put_str(param, &mut key);
            encode_value(value, &mut key);
        }
        if let Some(&id) = self.action_ids.get(key.as_slice()) {
            return id;
        }
        let id = self.actions.len();
        self.action_ids.insert(Rc::from(key), id);
        self.actions.push(TraceAction {
            name: name.to_owned(),
            params: params.clone(),
        });
        id
    }
}

/// The canonical byte encoding of `state` into `out` (cleared first): a
/// prefix-free tagged encoding of every value in `BTreeMap`/`BTreeSet`
/// order, so two states encode equal exactly when they are equal.
fn encode_state(state: &State, out: &mut Vec<u8>) {
    out.clear();
    put_len(state.len(), out);
    for (name, value) in state {
        put_str(name, out);
        encode_value(value, out);
    }
}

const TAG_INT: u8 = 0;
const TAG_BOOL: u8 = 1;
const TAG_ENUM: u8 = 2;
const TAG_NONE: u8 = 3;
const TAG_SOME: u8 = 4;
const TAG_STRUCT: u8 = 5;
const TAG_MAP: u8 = 6;
const TAG_SET: u8 = 7;
const TAG_SEQ: u8 = 8;
const TAG_RELATION: u8 = 9;

fn encode_value(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Int(value) => {
            out.push(TAG_INT);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::Bool(value) => {
            out.push(TAG_BOOL);
            out.push(u8::from(*value));
        }
        Value::Enum { type_name, member } => {
            out.push(TAG_ENUM);
            put_str(type_name, out);
            put_str(member, out);
        }
        Value::None => out.push(TAG_NONE),
        Value::Some(value) => {
            out.push(TAG_SOME);
            encode_value(value, out);
        }
        Value::Struct { type_name, fields } => {
            out.push(TAG_STRUCT);
            put_str(type_name, out);
            put_len(fields.len(), out);
            for (field, value) in fields {
                put_str(field, out);
                encode_value(value, out);
            }
        }
        Value::Map(entries) => {
            out.push(TAG_MAP);
            put_len(entries.len(), out);
            for (key, value) in entries {
                encode_value(key, out);
                encode_value(value, out);
            }
        }
        Value::Set(items) => {
            out.push(TAG_SET);
            put_len(items.len(), out);
            for item in items {
                encode_value(item, out);
            }
        }
        Value::Seq(items) => {
            out.push(TAG_SEQ);
            put_len(items.len(), out);
            for item in items {
                encode_value(item, out);
            }
        }
        Value::Relation(pairs) => {
            out.push(TAG_RELATION);
            put_len(pairs.len(), out);
            for (source, target) in pairs {
                encode_value(source, out);
                encode_value(target, out);
            }
        }
    }
}

/// LEB128 length prefix.
fn put_len(mut len: usize, out: &mut Vec<u8>) {
    loop {
        let byte = u8::try_from(len & 0x7f).expect("seven bits fit a byte");
        len >>= 7;
        if len == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn put_str(value: &str, out: &mut Vec<u8>) {
    put_len(value.len(), out);
    out.extend_from_slice(value.as_bytes());
}

/// Reads back what [`encode_state`] wrote. The bytes only ever come from
/// `encode_state` in this process, so malformed input is a bug: it panics.
struct Decoder<'bytes> {
    bytes: &'bytes [u8],
}

impl Decoder<'_> {
    fn byte(&mut self) -> u8 {
        let (&first, rest) = self.bytes.split_first().expect("truncated packed state");
        self.bytes = rest;
        first
    }

    fn take(&mut self, len: usize) -> &[u8] {
        let (taken, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        taken
    }

    fn len(&mut self) -> usize {
        let mut len = 0_usize;
        let mut shift = 0;
        loop {
            let byte = self.byte();
            len |= usize::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return len;
            }
            shift += 7;
        }
    }

    fn string(&mut self) -> String {
        let len = self.len();
        String::from_utf8(self.take(len).to_vec()).expect("packed string is UTF-8")
    }

    fn value(&mut self) -> Value {
        match self.byte() {
            TAG_INT => {
                let bytes = self.take(8).try_into().expect("eight bytes");
                Value::Int(i64::from_le_bytes(bytes))
            }
            TAG_BOOL => Value::Bool(self.byte() != 0),
            TAG_ENUM => Value::Enum {
                type_name: self.string(),
                member: self.string(),
            },
            TAG_NONE => Value::None,
            TAG_SOME => Value::Some(Box::new(self.value())),
            TAG_STRUCT => {
                let type_name = self.string();
                let len = self.len();
                let fields = (0..len).map(|_| (self.string(), self.value())).collect();
                Value::Struct { type_name, fields }
            }
            TAG_MAP => {
                let len = self.len();
                Value::Map((0..len).map(|_| (self.value(), self.value())).collect())
            }
            TAG_SET => {
                let len = self.len();
                Value::Set((0..len).map(|_| self.value()).collect::<BTreeSet<_>>())
            }
            TAG_SEQ => {
                let len = self.len();
                Value::Seq((0..len).map(|_| self.value()).collect())
            }
            TAG_RELATION => {
                let len = self.len();
                Value::Relation((0..len).map(|_| (self.value(), self.value())).collect())
            }
            tag => panic!("unknown packed value tag {tag}"),
        }
    }
}

fn decode_state(bytes: &[u8]) -> State {
    let mut decoder = Decoder { bytes };
    let len = decoder.len();
    let state = (0..len)
        .map(|_| (decoder.string(), decoder.value()))
        .collect();
    assert!(decoder.bytes.is_empty(), "trailing bytes in packed state");
    state
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, BTreeSet, State, StateStore, Value, decode_state, encode_state};

    fn sample() -> State {
        let claim = |st: &str, amount| Value::Struct {
            type_name: "DesignClaim".to_owned(),
            fields: BTreeMap::from([
                ("amount".to_owned(), Value::Int(amount)),
                (
                    "st".to_owned(),
                    Value::Enum {
                        type_name: "DesignSt".to_owned(),
                        member: st.to_owned(),
                    },
                ),
            ]),
        };
        State::from([
            (
                "design".to_owned(),
                Value::Map(BTreeMap::from([
                    (Value::Int(0), claim("DesignDraft", 0)),
                    (Value::Int(1), claim("DesignPaid", -300)),
                ])),
            ),
            ("flag".to_owned(), Value::Bool(true)),
            ("maybe".to_owned(), Value::Some(Box::new(Value::None))),
            ("none".to_owned(), Value::None),
            (
                "outbox".to_owned(),
                Value::Seq(vec![Value::Int(1), Value::Int(1), Value::Int(i64::MIN)]),
            ),
            (
                "set".to_owned(),
                Value::Set(BTreeSet::from([Value::Int(2), Value::Int(i64::MAX)])),
            ),
            (
                "rel".to_owned(),
                Value::Relation(BTreeSet::from([(Value::Int(0), Value::Int(1))])),
            ),
            ("long".to_owned(), Value::Seq(vec![Value::Bool(false); 300])),
        ])
    }

    /// Every `Value` shape round-trips, so a decoded trace state is the
    /// state that was stored.
    #[test]
    fn packed_states_round_trip() {
        let state = sample();
        let mut bytes = Vec::new();
        encode_state(&state, &mut bytes);
        assert_eq!(decode_state(&bytes), state);
    }

    /// Membership is `State` equality: values that differ only in where a
    /// boundary between two names falls, or in one nested element, are
    /// distinct, and an equal state built separately is found.
    #[test]
    fn packed_membership_is_state_equality() {
        let mut store = StateStore::new();
        let state = sample();
        let root = store.insert_root(&state).expect("new root");
        assert_eq!(store.insert_root(&sample()), None);
        let split = |left: &str, right: &str| {
            State::from([(
                left.to_owned(),
                Value::Enum {
                    type_name: right.to_owned(),
                    member: String::new(),
                },
            )])
        };
        assert!(store.insert_root(&split("ab", "c")).is_some());
        assert!(store.insert_root(&split("a", "bc")).is_some());
        let mut changed = sample();
        changed.insert(
            "outbox".to_owned(),
            Value::Seq(vec![Value::Int(1), Value::Int(1)]),
        );
        let child = store
            .insert_child(&changed, root, "send", &BTreeMap::new())
            .expect("new child");
        assert_eq!(
            store.insert_child(&changed, child, "other", &BTreeMap::new()),
            None
        );
        assert_eq!(store.len(), 4);
        assert_eq!(store.state(child), changed);
    }

    /// The parent chain renders the same steps [`super::reconstruct_trace`]
    /// renders from `ParentLink`s.
    #[test]
    fn packed_trace_matches_parent_link_reconstruction() {
        let root = State::from([("n".to_owned(), Value::Int(0))]);
        let one = State::from([("n".to_owned(), Value::Int(1))]);
        let two = State::from([("n".to_owned(), Value::Int(2))]);
        let params = BTreeMap::from([("k".to_owned(), Value::Int(7))]);
        let mut store = StateStore::new();
        let root_id = store.insert_root(&root).expect("root");
        let one_id = store
            .insert_child(&one, root_id, "inc", &params)
            .expect("one");
        let two_id = store
            .insert_child(&two, one_id, "inc", &BTreeMap::new())
            .expect("two");
        let link = |parent: &State, params: &BTreeMap<String, Value>| super::ParentLink {
            parent: parent.clone(),
            action: fsl_core::TraceAction {
                name: "inc".to_owned(),
                params: params.clone(),
            },
        };
        let parents = BTreeMap::from([
            (one.clone(), link(&root, &params)),
            (two.clone(), link(&one, &BTreeMap::new())),
        ]);
        assert_eq!(
            store.trace(two_id),
            super::reconstruct_trace(&two, &parents)
        );
        assert_eq!(
            store.trace(root_id),
            super::reconstruct_trace(&root, &parents)
        );
    }
}
