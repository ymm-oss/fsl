// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! T2 of `docs/design/DESIGN-obligation-catalog.md` (#1202): the minimum
//! obligation catalog of each rule family, pinned as an exact multiset per
//! fixture. The fixtures are the reproducers of #1189, #1192, #1196, #1217,
//! #1221 and #1258, and the false-vacuity reproducers of the first P1-a
//! review.
//! `candidates_reach_the_row_from_every_operand_position` puts one live
//! candidate in each operand position the vacuity walk recurses through.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::obligation::{Obligation, ObligationId, ObligationKind, Site, SiteRef, catalog};
use fsl_core::{
    FsResolver, KernelExpr, KernelModel, Span, build_model, parse_kernel_source,
    parse_kernel_source_with_bounds,
};

fn model(source: &str) -> KernelModel {
    let kernel = parse_kernel_source(source, &FsResolver::new(".")).expect("fixture lowers");
    build_model(kernel).expect("fixture builds")
}

fn site_text(site: &Site) -> String {
    match site {
        Site::TypeBound { var, position } => format!("TypeBound({var}#{position})"),
        Site::Invariant(at) => format!("Invariant({})", at.name),
        Site::Trans(at) => format!("Trans({})", at.name),
        Site::Reachable(at) => format!("Reachable({})", at.name),
        Site::LeadsTo(at) => format!("LeadsTo({})", at.name),
        Site::Trigger(at) => format!("Trigger({})", at.name),
        Site::Goal(at) => format!("Goal({})", at.name),
        Site::Measure(at) => format!("Measure({})", at.name),
        Site::RankStep { leads_to, action } => {
            format!("RankStep({},{})", leads_to.name, action.name)
        }
        Site::Terminal => "Terminal".to_owned(),
        Site::Guard(at) => format!("Guard({})", at.name),
        Site::Body(at) => format!("Body({})", at.name),
        Site::Ensures { action, index } => format!("Ensures({}#{index})", action.name),
        Site::Init => "Init".to_owned(),
        Site::Model => "Model".to_owned(),
    }
}

/// `Kind@Site`, with ` [v]` for a statically vacuous row.
fn row_text(row: &Obligation) -> String {
    format!(
        "{:?}@{}{}",
        row.id.kind,
        site_text(&row.id.site),
        if row.statically_vacuous { " [v]" } else { "" }
    )
}

/// Assert `model`'s catalog is exactly `expected` (as a multiset) and that no
/// two rows share an id.
fn assert_catalog(model: &KernelModel, expected: &[&str]) -> Vec<Obligation> {
    let rows = catalog(model).obligations;
    let ids = rows.iter().map(|row| &row.id).collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), rows.len(), "duplicate obligation ids: {rows:#?}");
    let mut actual = rows.iter().map(row_text).collect::<Vec<_>>();
    actual.sort();
    let mut expected = expected
        .iter()
        .map(|row| (*row).to_owned())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(actual, expected);
    rows
}

fn site_ref(name: &str, span: Span) -> SiteRef {
    SiteRef {
        name: name.to_owned(),
        start: span.start.offset,
        end: span.end.offset,
    }
}

/// #1196: a guard's `x / d` is an action-context partial operation.
const PARTIAL_GUARD: &str = r"
spec PartialGuard2 {
  state { x: Int, d: Int }
  init { x = 5  d = 1 }
  action dec() {
    requires x > 0 and x / d < 100
    x = x - 1
  }
  action zap() {
    requires x == 3
    d = 0
  }
  invariant NonNeg { x >= 0 }
}
";

#[test]
fn issue_1196_guard_division_is_a_definedness_obligation() {
    assert_catalog(
        &model(PARTIAL_GUARD),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init [v]",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(x#0) [v]",
            "Holds@TypeBound(d#1) [v]",
            "Holds@Invariant(NonNeg)",
            "PartialDefined@Invariant(NonNeg) [v]",
            "NoOverflow@Invariant(NonNeg) [v]",
            "KeyInDomain@Invariant(NonNeg) [v]",
            "PartialDefined@Guard(dec)",
            "NoOverflow@Guard(dec)",
            "KeyInDomain@Guard(dec) [v]",
            "PartialDefined@Body(dec) [v]",
            "NoOverflow@Body(dec)",
            "KeyInDomain@Body(dec) [v]",
            "PartialDefined@Guard(zap) [v]",
            "NoOverflow@Guard(zap) [v]",
            "KeyInDomain@Guard(zap) [v]",
            "PartialDefined@Body(zap) [v]",
            "NoOverflow@Body(zap) [v]",
            "KeyInDomain@Body(zap) [v]",
            "NoDeadlock@Model",
        ],
    );
}

/// #1217: an `ensures` is a truth obligation of its own.
const ENSURES_HOLE: &str = r"
spec EnsuresHole {
  state { x: 0..10 }
  init { x = 0 }
  action inc() { requires x < 10  x = x + 1  ensures x != 6 }
  invariant Range { 0 <= x and x <= 10 }
}
";

#[test]
fn issue_1217_ensures_owes_a_truth_obligation() {
    assert_catalog(
        &model(ENSURES_HOLE),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init [v]",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(x#0)",
            "Holds@Invariant(Range)",
            "PartialDefined@Invariant(Range) [v]",
            "NoOverflow@Invariant(Range) [v]",
            "KeyInDomain@Invariant(Range) [v]",
            "PartialDefined@Guard(inc) [v]",
            "NoOverflow@Guard(inc) [v]",
            "KeyInDomain@Guard(inc) [v]",
            "PartialDefined@Body(inc) [v]",
            "NoOverflow@Body(inc)",
            "KeyInDomain@Body(inc) [v]",
            "Holds@Ensures(inc#0)",
            "PartialDefined@Ensures(inc#0) [v]",
            "NoOverflow@Ensures(inc#0) [v]",
            "KeyInDomain@Ensures(inc#0) [v]",
            "NoDeadlock@Model",
        ],
    );
}

/// #1189: a ranked `leadsTo` without `helpful` owes a no-deadlock
/// obligation; `dec` is disabled at the pending `x = 2`.
const DEADLOCK_RANK: &str = r"
spec DeadlockRank {
  type X = 0..5
  state { x: X }
  init { x = 5 }
  action dec() { requires x > 2  x = x - 1 }
  leadsTo Drain { x > 0 ~> x == 0 decreases x }
}
";

#[test]
fn issue_1189_ranked_leadsto_owes_no_deadlock_without_helpful() {
    assert_catalog(
        &model(DEADLOCK_RANK),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init [v]",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(x#0)",
            "Responds@LeadsTo(Drain)",
            "PartialDefined@Trigger(Drain) [v]",
            "NoOverflow@Trigger(Drain) [v]",
            "KeyInDomain@Trigger(Drain) [v]",
            "PartialDefined@Goal(Drain) [v]",
            "NoOverflow@Goal(Drain) [v]",
            "KeyInDomain@Goal(Drain) [v]",
            "PartialDefined@Measure(Drain) [v]",
            "NoOverflow@Measure(Drain) [v]",
            "KeyInDomain@Measure(Drain) [v]",
            "RankLowerBound@LeadsTo(Drain)",
            "RankNoDeadlock@LeadsTo(Drain)",
            "RankStep@RankStep(Drain,dec)",
            "PartialDefined@Guard(dec) [v]",
            "NoOverflow@Guard(dec) [v]",
            "KeyInDomain@Guard(dec) [v]",
            "PartialDefined@Body(dec) [v]",
            "NoOverflow@Body(dec)",
            "KeyInDomain@Body(dec) [v]",
            "NoDeadlock@Model",
        ],
    );
}

/// #1192: the header and properties of the regression's duplicate `leadsTo`.
/// `check` rejects the reuse, so the second name is rewritten after building.
const DUP: &str = r"
spec Dup {
  state { x: 0..1, y: 0..5 }
  init { x = 0  y = 0 }
  action flip() { requires y == 5 x = 1 - x }
  action inc() { requires y < 5 y = y + 1 }
  leadsTo L { y < 5 ~> y == 5 decreases 5 - y }
  leadsTo M { x == 0 ~> x == 2 }
}
";

fn dup() -> KernelModel {
    let mut dup = model(DUP);
    "L".clone_into(&mut dup.leadstos[1].name);
    dup
}

#[test]
fn issue_1192_same_named_properties_are_distinct_rows() {
    let dup = dup();
    let rows = assert_catalog(
        &dup,
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init [v]",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(x#0)",
            "Holds@TypeBound(y#1)",
            "Responds@LeadsTo(L)",
            "Responds@LeadsTo(L)",
            "PartialDefined@Trigger(L) [v]",
            "PartialDefined@Trigger(L) [v]",
            "NoOverflow@Trigger(L) [v]",
            "NoOverflow@Trigger(L) [v]",
            "KeyInDomain@Trigger(L) [v]",
            "KeyInDomain@Trigger(L) [v]",
            "PartialDefined@Goal(L) [v]",
            "PartialDefined@Goal(L) [v]",
            "NoOverflow@Goal(L) [v]",
            "NoOverflow@Goal(L) [v]",
            "KeyInDomain@Goal(L) [v]",
            "KeyInDomain@Goal(L) [v]",
            "PartialDefined@Measure(L) [v]",
            "NoOverflow@Measure(L)",
            "KeyInDomain@Measure(L) [v]",
            "RankLowerBound@LeadsTo(L)",
            "RankNoDeadlock@LeadsTo(L)",
            "RankStep@RankStep(L,flip)",
            "RankStep@RankStep(L,inc)",
            "PartialDefined@Guard(flip) [v]",
            "NoOverflow@Guard(flip) [v]",
            "KeyInDomain@Guard(flip) [v]",
            "PartialDefined@Body(flip) [v]",
            "NoOverflow@Body(flip)",
            "KeyInDomain@Body(flip) [v]",
            "PartialDefined@Guard(inc) [v]",
            "NoOverflow@Guard(inc) [v]",
            "KeyInDomain@Guard(inc) [v]",
            "PartialDefined@Body(inc) [v]",
            "NoOverflow@Body(inc)",
            "KeyInDomain@Body(inc) [v]",
            "NoDeadlock@Model",
        ],
    );
    let ranked = site_ref("L", dup.leadstos[0].span);
    let unranked = site_ref("L", dup.leadstos[1].span);
    assert_ne!(ranked, unranked, "the fixture's spans differ");
    let sites_of = |kind: ObligationKind| {
        rows.iter()
            .filter(|row| row.id.kind == kind)
            .map(|row| row.id.site.clone())
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(
        sites_of(ObligationKind::Responds),
        BTreeSet::from([
            Site::LeadsTo(ranked.clone()),
            Site::LeadsTo(unranked.clone())
        ])
    );
    assert_eq!(
        sites_of(ObligationKind::RankNoDeadlock),
        BTreeSet::from([Site::LeadsTo(ranked.clone())])
    );
    assert_eq!(
        sites_of(ObligationKind::NoOverflow)
            .into_iter()
            .filter(|site| matches!(site, Site::Measure(_)))
            .collect::<Vec<_>>(),
        vec![Site::Measure(ranked)]
    );
}

/// `issue_473_helpful_leadsto.fsl`: `helpful` adds the fairness and
/// stickiness obligations; every `Map` read is keyed by a `Case` binder or
/// parameter, so no key row is live.
const MIN_HELPFUL: &str = r"
spec MinHelpful {
  type Case = 0..1
  type Level = 0..2
  state { level: Map<Case, Level> }
  init { forall c: Case { level[c] = 2 } }
  fair action step(c: Case) {
    requires level[c] > 0
    level[c] = level[c] - 1
  }
  invariant NonNeg { forall c: Case { level[c] >= 0 } }
  leadsTo Responds {
    forall c: Case { level[c] > 0 ~> level[c] == 0 }
    helpful step(c)
    decreases level[c]
  }
}
";

#[test]
fn helpful_adds_fairness_and_stickiness_rows() {
    assert_catalog(
        &model(MIN_HELPFUL),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(level#0)",
            "Holds@Invariant(NonNeg)",
            "PartialDefined@Invariant(NonNeg)",
            "NoOverflow@Invariant(NonNeg) [v]",
            "KeyInDomain@Invariant(NonNeg) [v]",
            "Responds@LeadsTo(Responds)",
            "PartialDefined@Trigger(Responds)",
            "NoOverflow@Trigger(Responds) [v]",
            "KeyInDomain@Trigger(Responds) [v]",
            "PartialDefined@Goal(Responds)",
            "NoOverflow@Goal(Responds) [v]",
            "KeyInDomain@Goal(Responds) [v]",
            "PartialDefined@Measure(Responds)",
            "NoOverflow@Measure(Responds) [v]",
            "KeyInDomain@Measure(Responds) [v]",
            "RankLowerBound@LeadsTo(Responds)",
            "RankNoDeadlock@LeadsTo(Responds)",
            "RankStep@RankStep(Responds,step)",
            "RankHelpfulFair@LeadsTo(Responds)",
            "RankHelpfulSticky@LeadsTo(Responds)",
            "PartialDefined@Guard(step)",
            "NoOverflow@Guard(step) [v]",
            "KeyInDomain@Guard(step) [v]",
            "PartialDefined@Body(step)",
            "NoOverflow@Body(step)",
            "KeyInDomain@Body(step) [v]",
            "NoDeadlock@Model",
        ],
    );
}

/// #1221: a `Map<K, _>` read with an `Int` index can miss the key domain.
const MAP_OUT_OF_DOMAIN: &str = r"
spec MapOutOfDomain {
  type K = 0..3
  type V = 0..9
  state { m: Map<K, V>, i: Int, y: V }
  init { forall k: K { m[k] = 0 }  i = 0  y = 0 }
  action step() { requires i < 10  i = i + 1 }
  action read() { y = m[i] }
}
";

#[test]
fn issue_1221_out_of_domain_map_read_is_a_key_obligation() {
    assert_catalog(
        &model(MAP_OUT_OF_DOMAIN),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(m#0)",
            "Holds@TypeBound(i#1) [v]",
            "Holds@TypeBound(y#2)",
            "PartialDefined@Guard(step) [v]",
            "NoOverflow@Guard(step) [v]",
            "KeyInDomain@Guard(step) [v]",
            "PartialDefined@Body(step) [v]",
            "NoOverflow@Body(step)",
            "KeyInDomain@Body(step) [v]",
            "PartialDefined@Guard(read) [v]",
            "NoOverflow@Guard(read) [v]",
            "KeyInDomain@Guard(read) [v]",
            "PartialDefined@Body(read)",
            "NoOverflow@Body(read) [v]",
            "KeyInDomain@Body(read)",
            "NoDeadlock@Model",
        ],
    );
}

/// The key-domain predicate: a read or write is in domain only when every
/// value of the index lies inside the key type. Each site isolates one case;
/// `Two`/`Writes` hold two misses, `SeqRead` a `Seq` read. `opt` and `Pattern`
/// bind the state variable `high`'s name to a `K` payload: outside the binding
/// `high` may be either, so its type is their join `0..5`.
const KEY_DOMAINS: &str = r"
spec KeyDomains {
  type K = 0..2
  type V = 0..9
  type Low = -1..2
  enum Color { Red, Blue }
  state {
    m: Map<K, V>, w: Map<K, V>, paint: Map<Color, V>, flag: Map<Bool, V>,
    q: Seq<V, 2>, o: Option<K>,
    exact: 0..2, inner: 1..1, high: 0..5, low: Low, color: Color, b: Bool
  }
  init {
    forall k: K { m[k] = 0 }
    forall k: K { w[k] = 0 }
    forall c: Color { paint[c] = 0 }
    q = Seq {}
    o = none
    exact = 0
    inner = 1
    high = 0
    low = 0
    color = Red
    b = false
  }
  action write(j: K) { m[j] = 1 }
  action writeHigh() { m[high] = 1 }
  action writes() { m[high] = 1  w[low] = 1 }
  action lets() { let k = exact  requires m[k] >= 0  w[k] = 2 }
  action opt() { requires o is some(high)  m[high] = 3 }
  action all() { forall k: K { m[k] = 0 } }
  action branch() { if b { m[high] = 1 } else { w[exact] = 1 } }
  invariant Exact { m[exact] >= 0 }
  invariant Inner { m[inner] >= 0 }
  invariant High { m[high] >= 0 }
  invariant Below { m[low] >= 0 }
  invariant LitIn { m[2] >= 0 }
  invariant LitOut { m[7] >= 0 }
  invariant Paint { paint[color] >= 0 }
  invariant Flag { flag[b] >= 0 }
  invariant Bound { forall k: K { m[k] >= 0 } }
  invariant Shadow { forall high: K { m[high] >= 0 } }
  invariant Pattern { o is some(high) => m[high] >= 0 }
  invariant Filtered { count(k: K where m[high] > 0) >= 0 }
  invariant Two { m[high] >= 0 and w[low] >= 0 }
  invariant SeqRead { q.size() == 0 or q[0] >= 0 }
}
";

const KEY_DOMAINS_ROWS: &[&str] = &[
    "InitSatisfiable@Init",
    "PartialDefined@Init",
    "NoOverflow@Init [v]",
    "KeyInDomain@Init [v]",
    "Holds@TypeBound(m#0)",
    "Holds@TypeBound(w#1)",
    "Holds@TypeBound(paint#2)",
    "Holds@TypeBound(flag#3)",
    "Holds@TypeBound(q#4)",
    "Holds@TypeBound(o#5)",
    "Holds@TypeBound(exact#6)",
    "Holds@TypeBound(inner#7)",
    "Holds@TypeBound(high#8)",
    "Holds@TypeBound(low#9)",
    "Holds@TypeBound(color#10)",
    "Holds@TypeBound(b#11) [v]",
    "Holds@Invariant(Exact)",
    "PartialDefined@Invariant(Exact)",
    "NoOverflow@Invariant(Exact) [v]",
    "KeyInDomain@Invariant(Exact) [v]",
    "Holds@Invariant(Inner)",
    "PartialDefined@Invariant(Inner)",
    "NoOverflow@Invariant(Inner) [v]",
    "KeyInDomain@Invariant(Inner) [v]",
    "Holds@Invariant(High)",
    "PartialDefined@Invariant(High)",
    "NoOverflow@Invariant(High) [v]",
    "KeyInDomain@Invariant(High)",
    "Holds@Invariant(Below)",
    "PartialDefined@Invariant(Below)",
    "NoOverflow@Invariant(Below) [v]",
    "KeyInDomain@Invariant(Below)",
    "Holds@Invariant(LitIn)",
    "PartialDefined@Invariant(LitIn)",
    "NoOverflow@Invariant(LitIn) [v]",
    "KeyInDomain@Invariant(LitIn) [v]",
    "Holds@Invariant(LitOut)",
    "PartialDefined@Invariant(LitOut)",
    "NoOverflow@Invariant(LitOut) [v]",
    "KeyInDomain@Invariant(LitOut)",
    "Holds@Invariant(Paint)",
    "PartialDefined@Invariant(Paint)",
    "NoOverflow@Invariant(Paint) [v]",
    "KeyInDomain@Invariant(Paint) [v]",
    "Holds@Invariant(Flag)",
    "PartialDefined@Invariant(Flag)",
    "NoOverflow@Invariant(Flag) [v]",
    "KeyInDomain@Invariant(Flag) [v]",
    "Holds@Invariant(Bound)",
    "PartialDefined@Invariant(Bound)",
    "NoOverflow@Invariant(Bound) [v]",
    "KeyInDomain@Invariant(Bound) [v]",
    "Holds@Invariant(Shadow)",
    "PartialDefined@Invariant(Shadow)",
    "NoOverflow@Invariant(Shadow) [v]",
    "KeyInDomain@Invariant(Shadow) [v]",
    "Holds@Invariant(Pattern)",
    "PartialDefined@Invariant(Pattern)",
    "NoOverflow@Invariant(Pattern) [v]",
    "KeyInDomain@Invariant(Pattern)",
    "Holds@Invariant(Filtered)",
    "PartialDefined@Invariant(Filtered)",
    "NoOverflow@Invariant(Filtered) [v]",
    "KeyInDomain@Invariant(Filtered)",
    "Holds@Invariant(Two)",
    "PartialDefined@Invariant(Two)",
    "NoOverflow@Invariant(Two) [v]",
    "KeyInDomain@Invariant(Two)",
    "Holds@Invariant(SeqRead)",
    "PartialDefined@Invariant(SeqRead)",
    "NoOverflow@Invariant(SeqRead) [v]",
    "KeyInDomain@Invariant(SeqRead) [v]",
    "PartialDefined@Guard(write) [v]",
    "NoOverflow@Guard(write) [v]",
    "KeyInDomain@Guard(write) [v]",
    "PartialDefined@Body(write)",
    "NoOverflow@Body(write) [v]",
    "KeyInDomain@Body(write) [v]",
    "PartialDefined@Guard(writeHigh) [v]",
    "NoOverflow@Guard(writeHigh) [v]",
    "KeyInDomain@Guard(writeHigh) [v]",
    "PartialDefined@Body(writeHigh)",
    "NoOverflow@Body(writeHigh) [v]",
    "KeyInDomain@Body(writeHigh)",
    "PartialDefined@Guard(writes) [v]",
    "NoOverflow@Guard(writes) [v]",
    "KeyInDomain@Guard(writes) [v]",
    "PartialDefined@Body(writes)",
    "NoOverflow@Body(writes) [v]",
    "KeyInDomain@Body(writes)",
    "PartialDefined@Guard(lets)",
    "NoOverflow@Guard(lets) [v]",
    "KeyInDomain@Guard(lets) [v]",
    "PartialDefined@Body(lets)",
    "NoOverflow@Body(lets) [v]",
    "KeyInDomain@Body(lets) [v]",
    "PartialDefined@Guard(opt) [v]",
    "NoOverflow@Guard(opt) [v]",
    "KeyInDomain@Guard(opt) [v]",
    "PartialDefined@Body(opt)",
    "NoOverflow@Body(opt) [v]",
    "KeyInDomain@Body(opt)",
    "PartialDefined@Guard(all) [v]",
    "NoOverflow@Guard(all) [v]",
    "KeyInDomain@Guard(all) [v]",
    "PartialDefined@Body(all)",
    "NoOverflow@Body(all) [v]",
    "KeyInDomain@Body(all) [v]",
    "PartialDefined@Guard(branch) [v]",
    "NoOverflow@Guard(branch) [v]",
    "KeyInDomain@Guard(branch) [v]",
    "PartialDefined@Body(branch)",
    "NoOverflow@Body(branch) [v]",
    "KeyInDomain@Body(branch)",
    "NoDeadlock@Model",
];

#[test]
fn key_rows_follow_the_index_type() {
    assert_catalog(&model(KEY_DOMAINS), KEY_DOMAINS_ROWS);
}

/// False-vacuity reproducers of the first P1-a review: each `KeyInDomain`
/// row below but `CondJoin`, `FreshPattern` and `paramKeeps` failed in
/// `fslc verify --engine explicit` (and `bmc`) while the catalog marked it
/// vacuous, because the index was typed by what `check` infers -- the `then`
/// branch of a conditional, the receiver of `push`/`add`, a pattern binding
/// that `check` lets shadow a state variable, binder or parameter -- or by a
/// pattern the evaluators had not bound. The three exceptions are in-domain
/// controls: a conditional whose branches both lie in `K`, a pattern name
/// with no other meaning, and a parameter a pattern cannot rebind. The type
/// bounds of a relation and of `Map<K, Int>` can fail; `Option<Int>` cannot.
const FALSE_VACUITY: &str = r"
spec FalseVacuity {
  type K = 0..2
  type V = 0..9
  type W = 0..5
  type Small = 0..0
  state {
    m: Map<K, V>, n: Map<Small, V>, s: Set<K>, q: Seq<K, 2>,
    o: Option<W>, p: Option<K>, exact: K, high: W, b: Bool,
    r: relation K -> K, mi: Map<K, Int>, oi: Option<Int>
  }
  init {
    forall k: K { m[k] = 0 }
    forall k: Small { n[k] = 0 }
    forall k: K { mi[k] = 0 }
    s = Set {}  q = Seq {}  o = some(5)  p = some(0)  exact = 0  high = 5  b = false
    r = Set {}  oi = none
  }
  invariant CondIndex { m[if b then exact else high] >= 0 }
  invariant CondMap { (if b then m else n)[exact] >= 0 }
  invariant PushHead { m[q.push(high).head()] >= 0 }
  invariant AddBinder { forall x in s.add(high) { m[x] >= 0 } }
  invariant CondPattern { (if o is some(exact) then m[exact] else 0) >= 0 }
  invariant WherePattern { forall k: K where o is some(exact) { m[exact] >= 0 } }
  invariant OrPattern { p is some(high) or m[high] >= 0 }
  invariant TwicePattern { o is some(exact) and p is some(exact) and m[exact] >= 0 }
  invariant BinderPattern { forall exact: W { p is some(exact) => m[exact] >= 0 } }
  invariant CondJoin { m[if b then exact else 0] >= 0 }
  invariant FreshPattern { p is some(v) => m[v] >= 0 }
  action condLet() { let k = if b then exact else high  requires m[k] >= 0  b = not b }
  action ifPattern() { if o is some(exact) { m[exact] = 1 } }
  action paramPattern(exact: W) { requires p is some(exact)  m[exact] = 1 }
  action paramKeeps(exact: K) { requires o is some(exact)  m[exact] = 1 }
}
";

#[test]
fn false_vacuity_reproducers_are_live() {
    assert_catalog(
        &model(FALSE_VACUITY),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(m#0)",
            "Holds@TypeBound(n#1)",
            "Holds@TypeBound(s#2)",
            "Holds@TypeBound(q#3)",
            "Holds@TypeBound(o#4)",
            "Holds@TypeBound(p#5)",
            "Holds@TypeBound(exact#6)",
            "Holds@TypeBound(high#7)",
            "Holds@TypeBound(b#8) [v]",
            "Holds@TypeBound(r#9)",
            "Holds@TypeBound(mi#10)",
            "Holds@TypeBound(oi#11) [v]",
            "Holds@Invariant(CondIndex)",
            "PartialDefined@Invariant(CondIndex)",
            "NoOverflow@Invariant(CondIndex) [v]",
            "KeyInDomain@Invariant(CondIndex)",
            "Holds@Invariant(CondMap)",
            "PartialDefined@Invariant(CondMap)",
            "NoOverflow@Invariant(CondMap) [v]",
            "KeyInDomain@Invariant(CondMap)",
            "Holds@Invariant(PushHead)",
            "PartialDefined@Invariant(PushHead)",
            "NoOverflow@Invariant(PushHead) [v]",
            "KeyInDomain@Invariant(PushHead)",
            "Holds@Invariant(AddBinder)",
            "PartialDefined@Invariant(AddBinder)",
            "NoOverflow@Invariant(AddBinder) [v]",
            "KeyInDomain@Invariant(AddBinder)",
            "Holds@Invariant(CondPattern)",
            "PartialDefined@Invariant(CondPattern)",
            "NoOverflow@Invariant(CondPattern) [v]",
            "KeyInDomain@Invariant(CondPattern)",
            "Holds@Invariant(WherePattern)",
            "PartialDefined@Invariant(WherePattern)",
            "NoOverflow@Invariant(WherePattern) [v]",
            "KeyInDomain@Invariant(WherePattern)",
            "Holds@Invariant(OrPattern)",
            "PartialDefined@Invariant(OrPattern)",
            "NoOverflow@Invariant(OrPattern) [v]",
            "KeyInDomain@Invariant(OrPattern)",
            "Holds@Invariant(TwicePattern)",
            "PartialDefined@Invariant(TwicePattern)",
            "NoOverflow@Invariant(TwicePattern) [v]",
            "KeyInDomain@Invariant(TwicePattern)",
            "Holds@Invariant(BinderPattern)",
            "PartialDefined@Invariant(BinderPattern)",
            "NoOverflow@Invariant(BinderPattern) [v]",
            "KeyInDomain@Invariant(BinderPattern)",
            "Holds@Invariant(CondJoin)",
            "PartialDefined@Invariant(CondJoin)",
            "NoOverflow@Invariant(CondJoin) [v]",
            "KeyInDomain@Invariant(CondJoin) [v]",
            "Holds@Invariant(FreshPattern)",
            "PartialDefined@Invariant(FreshPattern)",
            "NoOverflow@Invariant(FreshPattern) [v]",
            "KeyInDomain@Invariant(FreshPattern) [v]",
            "PartialDefined@Guard(condLet)",
            "NoOverflow@Guard(condLet) [v]",
            "KeyInDomain@Guard(condLet)",
            "PartialDefined@Body(condLet) [v]",
            "NoOverflow@Body(condLet) [v]",
            "KeyInDomain@Body(condLet) [v]",
            "PartialDefined@Guard(ifPattern) [v]",
            "NoOverflow@Guard(ifPattern) [v]",
            "KeyInDomain@Guard(ifPattern) [v]",
            "PartialDefined@Body(ifPattern)",
            "NoOverflow@Body(ifPattern) [v]",
            "KeyInDomain@Body(ifPattern)",
            "PartialDefined@Guard(paramPattern) [v]",
            "NoOverflow@Guard(paramPattern) [v]",
            "KeyInDomain@Guard(paramPattern) [v]",
            "PartialDefined@Body(paramPattern)",
            "NoOverflow@Body(paramPattern) [v]",
            "KeyInDomain@Body(paramPattern)",
            "PartialDefined@Guard(paramKeeps) [v]",
            "NoOverflow@Guard(paramKeeps) [v]",
            "KeyInDomain@Guard(paramKeeps) [v]",
            "PartialDefined@Body(paramKeeps)",
            "NoOverflow@Body(paramKeeps) [v]",
            "KeyInDomain@Body(paramKeeps) [v]",
            "NoDeadlock@Model",
        ],
    );
}

/// One index per form whose value the catalog bounds by a type: a literal,
/// an enum member, a field, a map value, `head`/`at`, `old`, a conditional of
/// two members, a set binder and a range binder are in domain. `size`, `abs`
/// and a conditional with a branch below `K` are left live even where the
/// value happens to fit: the catalog bounds no value it does not type.
/// `Member` is lowered from `paint[Red]` and rebuilt with `Color.Red` as an
/// `EnumMember`, which no surface text produces.
const BOUNDED_INDEXES: &str = r"
spec BoundedIndexes {
  type K = 0..2
  type V = 0..9
  type Low = -1..2
  enum Color { Red, Blue }
  struct Rec { k: K }
  state {
    m: Map<K, V>, mk: Map<K, K>, paint: Map<Color, V>, flag: Map<Bool, V>,
    qk: Seq<K, 2>, sk: Set<K>, rec: Rec, exact: K, low: Low, color: Color, b: Bool
  }
  init {
    forall k: K { m[k] = 0 }
    forall k: K { mk[k] = 0 }
    forall c: Color { paint[c] = 0 }
    qk = Seq {}  sk = Set {}  rec = Rec { k: 0 }  exact = 0  low = 0  color = Red  b = false
  }
  action flip() { b = not b }
  invariant BoolLiteral { flag[true] >= 0 }
  invariant Member { paint[Red] >= 0 }
  invariant FieldIndex { m[rec.k] >= 0 }
  invariant MapValue { m[mk[exact]] >= 0 }
  invariant Head { qk.size() == 0 or m[qk.head()] >= 0 }
  invariant At { qk.size() == 0 or m[qk.at(0)] >= 0 }
  invariant Size { m[qk.size()] >= 0 }
  trans OldIndex { m[old(exact)] >= 0 }
  invariant AbsIndex { m[abs(exact)] >= 0 }
  invariant CondLow { m[if b then exact else low] >= 0 }
  invariant CondEnum { paint[if b then color else Red] >= 0 }
  invariant SetBinder { forall x in sk { m[x] >= 0 } }
  invariant RangeBinder { forall j in 0..2 { m[j] >= 0 } }
}
";

#[test]
fn key_rows_bound_only_typed_index_forms() {
    let mut model = model(BOUNDED_INDEXES);
    let member = model
        .invariants
        .iter_mut()
        .find(|invariant| invariant.name == "Member")
        .expect("Member invariant");
    let KernelExpr::Binary { left, .. } = &mut member.expr else {
        panic!("Member is not a comparison: {:?}", member.expr);
    };
    let KernelExpr::Index(_, index) = left.as_mut() else {
        panic!("Member does not index: {left:?}");
    };
    assert_eq!(**index, KernelExpr::Var("Red".to_owned()));
    **index = KernelExpr::EnumMember {
        type_name: "Color".to_owned(),
        member: "Red".to_owned(),
    };
    let property = |name: &str, key_vacuous: bool, overflow_vacuous: bool| {
        let v = |vacuous: bool| if vacuous { " [v]" } else { "" };
        [
            format!("Holds@{name}"),
            format!("PartialDefined@{name}"),
            format!("NoOverflow@{name}{}", v(overflow_vacuous)),
            format!("KeyInDomain@{name}{}", v(key_vacuous)),
        ]
    };
    let mut expected = [
        "InitSatisfiable@Init",
        "PartialDefined@Init",
        "NoOverflow@Init [v]",
        "KeyInDomain@Init [v]",
        "Holds@TypeBound(m#0)",
        "Holds@TypeBound(mk#1)",
        "Holds@TypeBound(paint#2)",
        "Holds@TypeBound(flag#3)",
        "Holds@TypeBound(qk#4)",
        "Holds@TypeBound(sk#5)",
        "Holds@TypeBound(rec#6)",
        "Holds@TypeBound(exact#7)",
        "Holds@TypeBound(low#8)",
        "Holds@TypeBound(color#9)",
        "Holds@TypeBound(b#10) [v]",
        "PartialDefined@Guard(flip) [v]",
        "NoOverflow@Guard(flip) [v]",
        "KeyInDomain@Guard(flip) [v]",
        "PartialDefined@Body(flip) [v]",
        "NoOverflow@Body(flip) [v]",
        "KeyInDomain@Body(flip) [v]",
        "NoDeadlock@Model",
    ]
    .map(str::to_owned)
    .to_vec();
    for (name, key_vacuous, overflow_vacuous) in [
        ("Invariant(BoolLiteral)", true, true),
        ("Invariant(Member)", true, true),
        ("Invariant(FieldIndex)", true, true),
        ("Invariant(MapValue)", true, true),
        ("Invariant(Head)", true, true),
        ("Invariant(At)", true, true),
        ("Invariant(Size)", false, true),
        ("Trans(OldIndex)", true, true),
        ("Invariant(AbsIndex)", false, false),
        ("Invariant(CondLow)", false, true),
        ("Invariant(CondEnum)", true, true),
        ("Invariant(SetBinder)", true, true),
        ("Invariant(RangeBinder)", true, true),
    ] {
        expected.extend(property(name, key_vacuous, overflow_vacuous));
    }
    assert_catalog(
        &model,
        &expected.iter().map(String::as_str).collect::<Vec<_>>(),
    );
}

/// The negative controls of `BOUNDED_INDEXES`: each bounded form typed by a
/// declaration wider than `K` (`W = 0..5`), so typing it narrower than its
/// declaration would make a live row vacuous. A range parameter, a `Map`
/// element, a range binder with a literal, a state and a conditional upper
/// bound, `head`/`at` of a `Seq<W>`, `old` of a `W` and the second field of
/// a struct each reach `5`; `fslc verify --engine explicit` and `--engine
/// bmc` report a key-domain miss for each as a spec of its own. `PairKey`,
/// the struct's first field, is the in-domain control.
const WIDE_INDEXES: &str = r"
spec WideIndexes {
  type K = 0..2
  type V = 0..9
  type W = 0..5
  struct Pair { k: K, w: W }
  state { m: Map<K, V>, n: Map<K, W>, qw: Seq<W, 2>, pair: Pair, exact: K, high: W, b: Bool }
  init {
    forall k: K { m[k] = 0 }
    forall k: K { n[k] = 5 }
    qw = Seq {}  pair = Pair { k: 0, w: 5 }  exact = 0  high = 5  b = false
  }
  action put(i in 0..5) { m[i] = 1 }
  action pushw() { requires qw.size() < 2  qw = qw.push(5) }
  invariant ReadThrough { m[n[exact]] >= 0 }
  invariant RangeLiteral { forall j in 0..5 { m[j] >= 0 } }
  invariant RangeHigh { forall j in 0..high { m[j] >= 0 } }
  invariant RangeCond { forall j in 0..(if b then exact else high) { m[j] >= 0 } }
  invariant WideHead { qw.size() == 0 or m[qw.head()] >= 0 }
  invariant WideAt { qw.size() == 0 or m[qw.at(0)] >= 0 }
  trans OldHigh { m[old(high)] >= 0 }
  invariant PairKey { m[pair.k] >= 0 }
  invariant PairWide { m[pair.w] >= 0 }
}
";

#[test]
fn key_rows_stay_live_for_indexes_wider_than_the_key() {
    let mut expected = [
        "InitSatisfiable@Init",
        "PartialDefined@Init",
        "NoOverflow@Init [v]",
        "KeyInDomain@Init [v]",
        "Holds@TypeBound(m#0)",
        "Holds@TypeBound(n#1)",
        "Holds@TypeBound(qw#2)",
        "Holds@TypeBound(pair#3)",
        "Holds@TypeBound(exact#4)",
        "Holds@TypeBound(high#5)",
        "Holds@TypeBound(b#6) [v]",
        "PartialDefined@Guard(put) [v]",
        "NoOverflow@Guard(put) [v]",
        "KeyInDomain@Guard(put) [v]",
        "PartialDefined@Body(put)",
        "NoOverflow@Body(put) [v]",
        "KeyInDomain@Body(put)",
        "PartialDefined@Guard(pushw) [v]",
        "NoOverflow@Guard(pushw) [v]",
        "KeyInDomain@Guard(pushw) [v]",
        "PartialDefined@Body(pushw) [v]",
        "NoOverflow@Body(pushw) [v]",
        "KeyInDomain@Body(pushw) [v]",
        "NoDeadlock@Model",
    ]
    .map(str::to_owned)
    .to_vec();
    for (name, key_vacuous) in [
        ("Invariant(ReadThrough)", false),
        ("Invariant(RangeLiteral)", false),
        ("Invariant(RangeHigh)", false),
        ("Invariant(RangeCond)", false),
        ("Invariant(WideHead)", false),
        ("Invariant(WideAt)", false),
        ("Trans(OldHigh)", false),
        ("Invariant(PairKey)", true),
        ("Invariant(PairWide)", false),
    ] {
        expected.extend([
            format!("Holds@{name}"),
            format!("PartialDefined@{name}"),
            format!("NoOverflow@{name} [v]"),
            format!(
                "KeyInDomain@{name}{}",
                if key_vacuous { " [v]" } else { "" }
            ),
        ]);
    }
    assert_catalog(
        &model(WIDE_INDEXES),
        &expected.iter().map(String::as_str).collect::<Vec<_>>(),
    );
}

/// Pattern bindings that only the join over a whole context types soundly;
/// `fslc verify --engine explicit` and `bmc` report a key-domain miss for
/// both. `Nested`: `pp is some(o2)` rebinds the state variable `o2` to a
/// `W` option, so `v` is a `W` -- only if `o2` is untyped while the payloads
/// are typed. `across`: the guard binds `exact` to a `W` and the body's
/// `p is some(exact)` keeps it, so the body's `K` payload alone is too narrow.
const PATTERN_JOINS: &str = r"
spec PatternJoins {
  type K = 0..2
  type V = 0..9
  type W = 0..5
  state {
    m: Map<K, V>, pp: Option<Option<W>>, o2: Option<K>, o: Option<W>, p: Option<K>, exact: K
  }
  init { forall k: K { m[k] = 0 }  pp = some(some(5))  o2 = none  o = some(5)  p = some(0)  exact = 0 }
  action across() { requires o is some(exact)  if p is some(exact) { m[exact] = 1 } }
  invariant Nested { pp is some(o2) and o2 is some(v) and m[v] >= 0 }
}
";

#[test]
fn pattern_types_join_across_the_context() {
    assert_catalog(
        &model(PATTERN_JOINS),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(m#0)",
            "Holds@TypeBound(pp#1)",
            "Holds@TypeBound(o2#2)",
            "Holds@TypeBound(o#3)",
            "Holds@TypeBound(p#4)",
            "Holds@TypeBound(exact#5)",
            "Holds@Invariant(Nested)",
            "PartialDefined@Invariant(Nested)",
            "NoOverflow@Invariant(Nested) [v]",
            "KeyInDomain@Invariant(Nested)",
            "PartialDefined@Guard(across) [v]",
            "NoOverflow@Guard(across) [v]",
            "KeyInDomain@Guard(across) [v]",
            "PartialDefined@Body(across)",
            "NoOverflow@Body(across) [v]",
            "KeyInDomain@Body(across)",
            "NoDeadlock@Model",
        ],
    );
}

/// A predicate call and a stage access are expanded and lowered before a
/// model is built, but a hand-built model can hold them; the walk still
/// reaches their operands. `Called`/`Staged` wrap `m[high]`, the controls
/// wrap `high`.
const UNLOWERED: &str = r"
spec Unlowered {
  type K = 0..2
  type V = 0..9
  state { m: Map<K, V>, high: 0..5 }
  init { forall k: K { m[k] = 0 }  high = 0 }
  action idle() { high = 0 }
  invariant Called { m[high] >= 0 }
  invariant Staged { m[high] >= 0 }
  invariant CalledControl { high >= 0 }
  invariant StagedControl { high >= 0 }
}
";

#[test]
fn unlowered_forms_reach_their_operands() {
    let mut model = model(UNLOWERED);
    for invariant in &mut model.invariants {
        let span = invariant.span;
        let KernelExpr::Binary { left, .. } = &mut invariant.expr else {
            panic!("{} is not a comparison", invariant.name);
        };
        let operand = Box::new(std::mem::replace(left.as_mut(), KernelExpr::Num(0)));
        **left = if invariant.name.starts_with("Called") {
            KernelExpr::Call {
                name: "p".to_owned(),
                args: vec![*operand],
                span,
            }
        } else {
            KernelExpr::Stage {
                process: None,
                entity: operand,
                entity_span: span,
                span,
            }
        };
    }
    assert_catalog(
        &model,
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(m#0)",
            "Holds@TypeBound(high#1)",
            "Holds@Invariant(Called)",
            "PartialDefined@Invariant(Called)",
            "NoOverflow@Invariant(Called) [v]",
            "KeyInDomain@Invariant(Called)",
            "Holds@Invariant(Staged)",
            "PartialDefined@Invariant(Staged)",
            "NoOverflow@Invariant(Staged) [v]",
            "KeyInDomain@Invariant(Staged)",
            "Holds@Invariant(CalledControl)",
            "PartialDefined@Invariant(CalledControl) [v]",
            "NoOverflow@Invariant(CalledControl) [v]",
            "KeyInDomain@Invariant(CalledControl) [v]",
            "Holds@Invariant(StagedControl)",
            "PartialDefined@Invariant(StagedControl) [v]",
            "NoOverflow@Invariant(StagedControl) [v]",
            "KeyInDomain@Invariant(StagedControl) [v]",
            "PartialDefined@Guard(idle) [v]",
            "NoOverflow@Guard(idle) [v]",
            "KeyInDomain@Guard(idle) [v]",
            "PartialDefined@Body(idle) [v]",
            "NoOverflow@Body(idle) [v]",
            "KeyInDomain@Body(idle) [v]",
            "NoDeadlock@Model",
        ],
    );
}

/// Overflow candidates (`abs`, unary `-`, `sum`, two chained `+`/`-`) against
/// their non-overflowing neighbours (`count`, `min`/`max`, `old`), plus a
/// `within` deadline.
const ARITH: &str = r"
spec Arith {
  type K = 0..2
  state { x: 0..9 }
  init { x = 0 }
  action up() { requires x < 9  x = x + 1 }
  invariant Abs { abs(x) >= 0 }
  invariant Negated { -x <= 0 }
  invariant Sum { sum(k: K of k) >= 0 }
  invariant Count { count(k: K where k > 0) >= 0 }
  invariant Chain { x + 1 - 1 >= 0 }
  invariant Bounds { min(x, 3) <= max(x, 0) }
  trans Old { old(x) <= x }
  leadsTo Soon { x == 0 ~> within 3 x == 3 }
}
";

#[test]
fn overflow_rows_and_deadline() {
    assert_catalog(
        &model(ARITH),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init [v]",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(x#0)",
            "PartialDefined@Guard(up) [v]",
            "NoOverflow@Guard(up) [v]",
            "KeyInDomain@Guard(up) [v]",
            "PartialDefined@Body(up) [v]",
            "NoOverflow@Body(up)",
            "KeyInDomain@Body(up) [v]",
            "Holds@Invariant(Abs)",
            "PartialDefined@Invariant(Abs) [v]",
            "NoOverflow@Invariant(Abs)",
            "KeyInDomain@Invariant(Abs) [v]",
            "Holds@Invariant(Negated)",
            "PartialDefined@Invariant(Negated) [v]",
            "NoOverflow@Invariant(Negated)",
            "KeyInDomain@Invariant(Negated) [v]",
            "Holds@Invariant(Sum)",
            "PartialDefined@Invariant(Sum) [v]",
            "NoOverflow@Invariant(Sum)",
            "KeyInDomain@Invariant(Sum) [v]",
            "Holds@Invariant(Count)",
            "PartialDefined@Invariant(Count) [v]",
            "NoOverflow@Invariant(Count) [v]",
            "KeyInDomain@Invariant(Count) [v]",
            "Holds@Invariant(Chain)",
            "PartialDefined@Invariant(Chain) [v]",
            "NoOverflow@Invariant(Chain)",
            "KeyInDomain@Invariant(Chain) [v]",
            "Holds@Invariant(Bounds)",
            "PartialDefined@Invariant(Bounds) [v]",
            "NoOverflow@Invariant(Bounds) [v]",
            "KeyInDomain@Invariant(Bounds) [v]",
            "Holds@Trans(Old)",
            "PartialDefined@Trans(Old) [v]",
            "NoOverflow@Trans(Old) [v]",
            "KeyInDomain@Trans(Old) [v]",
            "Responds@LeadsTo(Soon)",
            "Deadline@LeadsTo(Soon)",
            "PartialDefined@Trigger(Soon) [v]",
            "NoOverflow@Trigger(Soon) [v]",
            "KeyInDomain@Trigger(Soon) [v]",
            "PartialDefined@Goal(Soon) [v]",
            "NoOverflow@Goal(Soon) [v]",
            "KeyInDomain@Goal(Soon) [v]",
            "NoDeadlock@Model",
        ],
    );
}

/// Every non-ranked family: `trans`, `unless`, `until`, `reachable`,
/// `terminal`, `let`, several `ensures`, and property-context partial
/// operations (`/` and `%` are total there, `head`/`at` are not).
const FAMILIES: &str = r"
spec Families {
  type V = 0..3
  state { q: Seq<V, 2>, n: 0..2, d: 1..3, c: Int }
  init { q = Seq {}  n = 0  d = 1  c = 0 }
  action add() { requires n == 0  q = q.push(1)  n = 1 }
  action drop() {
    requires n == 1
    let h = q.head()
    q = q.pop()
    n = 2
    ensures n == 2
    ensures q.size() == 0 or q.at(0) >= 0
  }
  invariant Ratio { n / d <= 2 and n % d >= 0 }
  invariant Peek { q.size() == 0 or q.head() + q.at(0) >= 0 }
  trans Grow { old(n) <= n }
  unless Stay { n == 0 unless n == 1 }
  until Hold { n < 2 until n == 2 }
  leadsTo Drain { q.size() > 0 and q.head() >= 0 ~> n == 2 }
  reachable Full { q.size() == 1 and q.head() >= 0 }
  terminal { n == 2 and (q.size() == 0 or q.head() >= 0) }
}
";

#[test]
fn non_ranked_families_generate_their_rows() {
    assert_catalog(
        &model(FAMILIES),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init [v]",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(q#0)",
            "Holds@TypeBound(n#1)",
            "Holds@TypeBound(d#2)",
            "Holds@TypeBound(c#3) [v]",
            "Holds@Invariant(Ratio)",
            "PartialDefined@Invariant(Ratio) [v]",
            "NoOverflow@Invariant(Ratio)",
            "KeyInDomain@Invariant(Ratio) [v]",
            "Holds@Invariant(Peek)",
            "PartialDefined@Invariant(Peek)",
            "NoOverflow@Invariant(Peek)",
            "KeyInDomain@Invariant(Peek) [v]",
            "Holds@Trans(Grow)",
            "PartialDefined@Trans(Grow) [v]",
            "NoOverflow@Trans(Grow) [v]",
            "KeyInDomain@Trans(Grow) [v]",
            "Holds@Trans(Stay)",
            "PartialDefined@Trans(Stay) [v]",
            "NoOverflow@Trans(Stay) [v]",
            "KeyInDomain@Trans(Stay) [v]",
            "Holds@Trans(Hold_until_safety)",
            "PartialDefined@Trans(Hold_until_safety) [v]",
            "NoOverflow@Trans(Hold_until_safety) [v]",
            "KeyInDomain@Trans(Hold_until_safety) [v]",
            "Responds@LeadsTo(Hold)",
            "PartialDefined@Trigger(Hold) [v]",
            "NoOverflow@Trigger(Hold) [v]",
            "KeyInDomain@Trigger(Hold) [v]",
            "PartialDefined@Goal(Hold) [v]",
            "NoOverflow@Goal(Hold) [v]",
            "KeyInDomain@Goal(Hold) [v]",
            "Responds@LeadsTo(Drain)",
            "PartialDefined@Trigger(Drain)",
            "NoOverflow@Trigger(Drain) [v]",
            "KeyInDomain@Trigger(Drain) [v]",
            "PartialDefined@Goal(Drain) [v]",
            "NoOverflow@Goal(Drain) [v]",
            "KeyInDomain@Goal(Drain) [v]",
            "Witnessed@Reachable(Full)",
            "PartialDefined@Reachable(Full)",
            "NoOverflow@Reachable(Full) [v]",
            "KeyInDomain@Reachable(Full) [v]",
            "PartialDefined@Terminal",
            "NoOverflow@Terminal [v]",
            "KeyInDomain@Terminal [v]",
            "PartialDefined@Guard(add) [v]",
            "NoOverflow@Guard(add) [v]",
            "KeyInDomain@Guard(add) [v]",
            "PartialDefined@Body(add) [v]",
            "NoOverflow@Body(add) [v]",
            "KeyInDomain@Body(add) [v]",
            "PartialDefined@Guard(drop)",
            "NoOverflow@Guard(drop) [v]",
            "KeyInDomain@Guard(drop) [v]",
            "PartialDefined@Body(drop)",
            "NoOverflow@Body(drop) [v]",
            "KeyInDomain@Body(drop) [v]",
            "Holds@Ensures(drop#0)",
            "PartialDefined@Ensures(drop#0) [v]",
            "NoOverflow@Ensures(drop#0) [v]",
            "KeyInDomain@Ensures(drop#0) [v]",
            "Holds@Ensures(drop#1)",
            "PartialDefined@Ensures(drop#1)",
            "NoOverflow@Ensures(drop#1) [v]",
            "KeyInDomain@Ensures(drop#1) [v]",
            "NoDeadlock@Model",
        ],
    );
}

#[test]
fn until_safety_and_progress_share_a_span_but_not_an_id() {
    let families = model(FAMILIES);
    let rows = catalog(&families).obligations;
    let hold = families
        .leadstos
        .iter()
        .find(|property| property.name == "Hold")
        .expect("until lowers to a leadsTo");
    let safety = families
        .transitions
        .iter()
        .find(|property| property.name == "Hold_until_safety")
        .expect("until lowers to a trans");
    assert_eq!(hold.span, safety.span);
    let progress = ObligationId {
        kind: ObligationKind::Responds,
        site: Site::LeadsTo(site_ref("Hold", hold.span)),
    };
    let truth = ObligationId {
        kind: ObligationKind::Holds,
        site: Site::Trans(site_ref("Hold_until_safety", safety.span)),
    };
    assert!(rows.iter().any(|row| row.id == progress), "{rows:#?}");
    assert!(rows.iter().any(|row| row.id == truth), "{rows:#?}");
}

#[test]
fn action_sites_carry_the_action_span() {
    let families = model(FAMILIES);
    let drop = families
        .actions
        .iter()
        .find(|action| action.name == "drop")
        .expect("fixture declares drop");
    let at = site_ref("drop", drop.span);
    let rows = catalog(&families).obligations;
    for site in [
        Site::Guard(at.clone()),
        Site::Body(at.clone()),
        Site::Ensures {
            action: at.clone(),
            index: 1,
        },
    ] {
        assert!(
            rows.iter().any(|row| row.id.site == site),
            "{site:?} missing from {rows:#?}"
        );
    }
    let ranked = model(DEADLOCK_RANK);
    let step = Site::RankStep {
        leads_to: site_ref("Drain", ranked.leadstos[0].span),
        action: site_ref("dec", ranked.actions[0].span),
    };
    assert!(
        catalog(&ranked)
            .obligations
            .iter()
            .any(|row| row.id.site == step)
    );
}

/// Property selection removes sites from the model the engines see, and the
/// catalog of that model drops their rows. #1201 needs every row of a
/// selected run -- the unselected ones `NotRun` -- so the ledger (P1-c) must
/// call `catalog` on the full model, never on the selected one; this pins
/// that the selected model's catalog is only a subset, with the kept sites'
/// rows unchanged.
#[test]
fn a_selected_model_drops_rows_so_the_ledger_needs_the_full_model() {
    let full = model(FAMILIES);
    let mut selected = full.clone();
    selected
        .invariants
        .retain(|property| property.name == "Peek");
    selected.transitions.clear();
    selected.leadstos.clear();
    selected.reachables.clear();
    let full_rows = catalog(&full).obligations;
    let selected_rows = catalog(&selected).obligations;
    assert!(selected_rows.len() < full_rows.len());
    for row in &selected_rows {
        assert!(full_rows.contains(row), "{row:?} not in the full catalog");
    }
    let peek = |rows: &[Obligation]| {
        rows.iter()
            .filter(|row| matches!(&row.id.site, Site::Invariant(at) if at.name == "Peek"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(peek(&selected_rows), peek(&full_rows));
}

/// `entity`/`number` sizes are a verification scope. Where every index has
/// the key's own type, as here, an override leaves every row as it is.
const SCOPED: &str = r"
spec Scoped {
  entity Claim
  number Amount
  state { amount: Map<Claim, Amount> }
  init { forall c: Claim { amount[c] = 0 } }
  action set(c: Claim, a: Amount) { amount[c] = a }
  invariant NonNeg { forall c: Claim { amount[c] >= 0 } }
}
verify { instances Claim = 2  values Amount = 0..2 }
";

const SCOPED_ROWS: &[&str] = &[
    "InitSatisfiable@Init",
    "PartialDefined@Init",
    "NoOverflow@Init [v]",
    "KeyInDomain@Init [v]",
    "Holds@TypeBound(amount#0)",
    "Holds@Invariant(NonNeg)",
    "PartialDefined@Invariant(NonNeg)",
    "NoOverflow@Invariant(NonNeg) [v]",
    "KeyInDomain@Invariant(NonNeg) [v]",
    "PartialDefined@Guard(set) [v]",
    "NoOverflow@Guard(set) [v]",
    "KeyInDomain@Guard(set) [v]",
    "PartialDefined@Body(set)",
    "NoOverflow@Body(set) [v]",
    "KeyInDomain@Body(set) [v]",
    "NoDeadlock@Model",
];

#[test]
fn a_scope_override_keeps_every_row() {
    let declared = model(SCOPED);
    let kernel = parse_kernel_source_with_bounds(
        SCOPED,
        &BTreeMap::from([("Claim".to_owned(), 4)]),
        &BTreeMap::from([("Amount".to_owned(), (0, 7))]),
    )
    .expect("override lowers");
    let overridden = build_model(kernel).expect("override builds");
    assert_ne!(declared.types, overridden.types, "the override took effect");
    assert_catalog(&declared, SCOPED_ROWS);
    assert_catalog(&overridden, SCOPED_ROWS);
}

/// An index of another type is in domain only for the sizes that make it
/// so: declared, `Amount = 0..1` lies in `Claim`'s `0..1` and `fslc verify
/// --engine explicit` proves the spec; with `--values Amount=0..3` it reports
/// `map assignment index outside key domain`, and the row is live.
const SCOPE_FLIP: &str = r"
spec ScopeFlip {
  entity Claim
  number Amount
  type V = 0..9
  state { seen: Map<Claim, V>, a: Amount }
  init { forall c: Claim { seen[c] = 0 }  a = 0 }
  action bump() { requires a < 1  a = a + 1 }
  action pick(x: Amount) { seen[x] = 1 }
}
verify { instances Claim = 2  values Amount = 0..1 }
";

#[test]
fn a_scope_override_can_make_a_key_row_live() {
    assert_catalog(
        &model(SCOPE_FLIP),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(seen#0)",
            "Holds@TypeBound(a#1)",
            "PartialDefined@Guard(bump) [v]",
            "NoOverflow@Guard(bump) [v]",
            "KeyInDomain@Guard(bump) [v]",
            "PartialDefined@Body(bump) [v]",
            "NoOverflow@Body(bump)",
            "KeyInDomain@Body(bump) [v]",
            "PartialDefined@Guard(pick) [v]",
            "NoOverflow@Guard(pick) [v]",
            "KeyInDomain@Guard(pick) [v]",
            "PartialDefined@Body(pick)",
            "NoOverflow@Body(pick) [v]",
            "KeyInDomain@Body(pick) [v]",
            "NoDeadlock@Model",
        ],
    );
    let kernel = parse_kernel_source_with_bounds(
        SCOPE_FLIP,
        &BTreeMap::new(),
        &BTreeMap::from([("Amount".to_owned(), (0, 3))]),
    )
    .expect("override lowers");
    assert_catalog(
        &build_model(kernel).expect("override builds"),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init [v]",
            "Holds@TypeBound(seen#0)",
            "Holds@TypeBound(a#1)",
            "PartialDefined@Guard(bump) [v]",
            "NoOverflow@Guard(bump) [v]",
            "KeyInDomain@Guard(bump) [v]",
            "PartialDefined@Body(bump) [v]",
            "NoOverflow@Body(bump)",
            "KeyInDomain@Body(bump) [v]",
            "PartialDefined@Guard(pick) [v]",
            "NoOverflow@Guard(pick) [v]",
            "KeyInDomain@Guard(pick) [v]",
            "PartialDefined@Body(pick)",
            "NoOverflow@Body(pick) [v]",
            "KeyInDomain@Body(pick)",
            "NoDeadlock@Model",
        ],
    );
}

/// One site per operand position of the vacuity walk, each holding `HOLE`
/// and nothing else that can fail: `m[i]` (`i: Int`) is a live key candidate,
/// `x + 1` a live overflow candidate and `0` neither. Every variant passes
/// `fslc check`. `Expr::Call` and `Expr::Stage` are lowered away before a
/// `KernelModel` exists, and a `leadsTo` binder reads no state.
const POSITIONS: &str = r"
spec Positions {
  type K = 0..2
  type V = 0..9
  struct Rec { f: V }
  state {
    m: Map<K, V>, w: Map<K, V>, recs: Map<K, Rec>, q: Seq<V, 2>, s: Set<K>,
    o: Option<V>, r: relation K -> K, i: Int, x: Int, y: Int, b: Bool
  }
  init {
    forall k: K { m[k] = 0 }
    forall k: K { w[k] = 0 }
    forall k: K { recs[k] = Rec { f: 0 } }
    q = Seq {}  s = Set {}  o = none  r = Set {}  i = 0  x = 0  y = 0  b = false
  }
  invariant Neg { -(HOLE) <= 0 }
  invariant Abs { abs(HOLE) >= 0 }
  trans Old { old(HOLE) >= 0 }
  invariant SomeOf { some(HOLE) != none }
  invariant NotOf { not (HOLE > 0) }
  invariant FieldOf { recs[if HOLE > 0 then 0 else 1].f >= 0 }
  invariant IsOf { (if HOLE > 0 then o else o) is none }
  invariant SetItem { Set { HOLE }.contains(0) }
  invariant SeqItem { Seq { HOLE }.size() > 0 }
  invariant StructField { Rec { f: HOLE } == Rec { f: 0 } }
  invariant IndexBase { (if HOLE > 0 then m else w)[0] >= 0 }
  invariant IndexIndex { q.size() == 0 or q[HOLE] >= 0 }
  invariant MinLeft { min(HOLE, 0) >= 0 }
  invariant MaxRight { max(0, HOLE) >= 0 }
  invariant BinaryLeft { HOLE >= 0 }
  invariant BinaryRight { 0 <= HOLE }
  invariant MethodReceiver { (if HOLE > 0 then q else q).size() >= 0 }
  invariant MethodArg { q.contains(HOLE) or true }
  invariant CondCondition { (if HOLE > 0 then 0 else 1) >= 0 }
  invariant CondThen { (if b then HOLE else 0) >= 0 }
  invariant CondElse { (if b then 0 else HOLE) >= 0 }
  invariant QuantBody { forall k: K { HOLE >= 0 } }
  invariant QuantWhere { forall k: K where HOLE > 0 { true } }
  invariant QuantLo { forall k in HOLE..2 { true } }
  invariant QuantHi { forall k in 0..HOLE { true } }
  invariant QuantCollection { forall v in (if HOLE > 0 then s else s) { true } }
  invariant SumValue { sum(k: K of HOLE) >= 0 }
  invariant CountWhere { count(k: K where HOLE > 0) >= 0 }
  invariant ReachFirst { reachable(if HOLE > 0 then r else r, 0, 0) or true }
  invariant ReachSecond { reachable(r, if HOLE > 0 then 0 else 1, 0) or true }
  invariant ReachThird { reachable(r, 0, if HOLE > 0 then 0 else 1) or true }
  reachable Witness { HOLE > 0 }
  leadsTo Trig { HOLE > 0 ~> b }
  leadsTo Goal { b ~> HOLE > 0 }
  leadsTo Meas { b ~> not b decreases HOLE }
  terminal { HOLE > 0 }
  action assignValue() { y = HOLE }
  action assignTarget() { w[if HOLE > 0 then 0 else 1] = 0 }
  action assignField() { recs[if HOLE > 0 then 0 else 1].f = 0 }
  action ifCondition() { if HOLE > 0 { y = 0 } }
  action ifThen() { if b { y = HOLE } }
  action ifElse() { if b { y = 0 } else { y = HOLE } }
  action forallBody() { forall k: K { w[k] = HOLE } }
  action forallWhere() { forall k: K where HOLE > 0 { w[k] = 0 } }
  action guardRequires() { requires HOLE > 0  y = 0 }
  action guardLet() { let z = HOLE  requires z > 0  y = 0 }
  action ensuresOf() { y = 0  ensures HOLE >= 0 }
}
";

/// `(site, whether the site overflows without the hole)`, with the walk
/// steps between the site and its hole.
const OPERAND_POSITIONS: &[(&str, bool)] = &[
    ("Invariant(Neg)", true),              // Neg
    ("Invariant(Abs)", true),              // UnaryNamed abs
    ("Trans(Old)", false),                 // UnaryNamed old
    ("Invariant(SomeOf)", false),          // Some
    ("Invariant(NotOf)", false),           // Not
    ("Invariant(FieldOf)", false),         // Field, Index index, Conditional condition
    ("Invariant(IsOf)", false),            // Is
    ("Invariant(SetItem)", false),         // Set items, Method receiver
    ("Invariant(SeqItem)", false),         // Seq items
    ("Invariant(StructField)", false),     // Struct fields
    ("Invariant(IndexBase)", false),       // Index collection
    ("Invariant(IndexIndex)", false),      // Index index
    ("Invariant(MinLeft)", false),         // BinaryNamed left
    ("Invariant(MaxRight)", false),        // BinaryNamed right
    ("Invariant(BinaryLeft)", false),      // Binary left
    ("Invariant(BinaryRight)", false),     // Binary right
    ("Invariant(MethodReceiver)", false),  // Method receiver
    ("Invariant(MethodArg)", false),       // Method args
    ("Invariant(CondCondition)", false),   // Conditional condition
    ("Invariant(CondThen)", false),        // Conditional then
    ("Invariant(CondElse)", false),        // Conditional else
    ("Invariant(QuantBody)", false),       // Quantified body
    ("Invariant(QuantWhere)", false),      // binder where
    ("Invariant(QuantLo)", false),         // binder range lo
    ("Invariant(QuantHi)", false),         // binder range hi
    ("Invariant(QuantCollection)", false), // binder collection
    ("Invariant(SumValue)", true),         // Aggregate value
    ("Invariant(CountWhere)", false),      // Aggregate binder
    ("Invariant(ReachFirst)", false),      // TernaryNamed first
    ("Invariant(ReachSecond)", false),     // TernaryNamed second
    ("Invariant(ReachThird)", false),      // TernaryNamed third
    ("Reachable(Witness)", false),         // reachable
    ("Trigger(Trig)", false),              // leadsTo trigger
    ("Goal(Goal)", false),                 // leadsTo goal
    ("Measure(Meas)", false),              // decreases
    ("Terminal", false),                   // terminal
    ("Body(assignValue)", false),          // Assign value
    ("Body(assignTarget)", false),         // LValue index
    ("Body(assignField)", false),          // LValue field
    ("Body(ifCondition)", false),          // If condition
    ("Body(ifThen)", false),               // If then
    ("Body(ifElse)", false),               // If else
    ("Body(forallBody)", false),           // ForAll statements
    ("Body(forallWhere)", false),          // ForAll binder
    ("Guard(guardRequires)", false),       // requires
    ("Guard(guardLet)", false),            // let
    ("Ensures(ensuresOf#0)", false),       // ensures
];

#[test]
fn candidates_reach_the_row_from_every_operand_position() {
    let live = |hole: &str, kind: ObligationKind| {
        catalog(&model(&POSITIONS.replace("HOLE", hole)))
            .obligations
            .iter()
            .filter(|row| row.id.kind == kind && !row.statically_vacuous)
            .map(|row| site_text(&row.id.site))
            .collect::<BTreeSet<_>>()
    };
    let sites = |filter: fn(bool) -> bool| {
        OPERAND_POSITIONS
            .iter()
            .filter(|(_, own)| filter(*own))
            .map(|(site, _)| (*site).to_owned())
            .collect::<BTreeSet<_>>()
    };
    let every = sites(|_| true);
    assert_eq!(
        every.len(),
        OPERAND_POSITIONS.len(),
        "a site is listed twice"
    );
    assert_eq!(live("m[i]", ObligationKind::KeyInDomain), every);
    assert_eq!(live("x + 1", ObligationKind::NoOverflow), every);
    assert_eq!(live("0", ObligationKind::KeyInDomain), BTreeSet::new());
    assert_eq!(live("0", ObligationKind::NoOverflow), sites(|own| own));
}

/// #1258: `InitDiv` and `InitMapRead`, where `fslc verify --engine explicit`
/// fails the `init` with `division by zero` and `map index outside finite key
/// domain`, and bmc and induction report the same failure through their
/// `init`/`definedness` question. `init` is evaluated in action
/// context, so its `/` is a partial operation and an overflow candidate, and
/// `m[i]` (`i: 0..5`, `K = 0..3`) can miss the key domain. `InitOvf` is the
/// overflow-only control: a `*` without a partial operation or an index.
const INIT_DIV: &str = r"
spec InitDiv {
  type Small = 0..3
  state { d: Small, x: Int }
  init { d = 0  x = 6 / d }
  action tick() { requires d < 3  d = d + 1 }
  invariant Ok { x >= 0 }
}
";

const INIT_MAP_READ: &str = r"
spec InitMapRead {
  type K = 0..3
  type I = 0..5
  state { m: Map<K, Int>, i: I, x: Int }
  init { forall k: K { m[k] = 1 }  i = 5  x = m[i] }
  action tick() { requires i > 0  i = i - 1 }
  invariant Ok { x >= 1 }
}
";

const INIT_OVF: &str = r"
spec InitOvf {
  type Small = 0..3
  state { d: Small, x: Int }
  init { d = 0  x = 4611686018427387904 * (d + 2) }
  action tick() { requires d < 3  d = d + 1 }
  invariant Ok { x >= 0 }
}
";

/// The rows `InitDiv` and `InitOvf` share outside `init`.
const INIT_SMALL_ROWS: &[&str] = &[
    "InitSatisfiable@Init",
    "Holds@TypeBound(d#0)",
    "Holds@TypeBound(x#1) [v]",
    "Holds@Invariant(Ok)",
    "PartialDefined@Invariant(Ok) [v]",
    "NoOverflow@Invariant(Ok) [v]",
    "KeyInDomain@Invariant(Ok) [v]",
    "PartialDefined@Guard(tick) [v]",
    "NoOverflow@Guard(tick) [v]",
    "KeyInDomain@Guard(tick) [v]",
    "PartialDefined@Body(tick) [v]",
    "NoOverflow@Body(tick)",
    "KeyInDomain@Body(tick) [v]",
    "NoDeadlock@Model",
];

#[test]
fn issue_1258_init_owes_definedness_rows() {
    let with = |init: &[&'static str]| {
        INIT_SMALL_ROWS
            .iter()
            .chain(init)
            .copied()
            .collect::<Vec<_>>()
    };
    assert_catalog(
        &model(INIT_DIV),
        &with(&[
            "PartialDefined@Init",
            "NoOverflow@Init",
            "KeyInDomain@Init [v]",
        ]),
    );
    assert_catalog(
        &model(INIT_OVF),
        &with(&[
            "PartialDefined@Init [v]",
            "NoOverflow@Init",
            "KeyInDomain@Init [v]",
        ]),
    );
    assert_catalog(
        &model(INIT_MAP_READ),
        &[
            "InitSatisfiable@Init",
            "PartialDefined@Init",
            "NoOverflow@Init [v]",
            "KeyInDomain@Init",
            "Holds@TypeBound(m#0)",
            "Holds@TypeBound(i#1)",
            "Holds@TypeBound(x#2) [v]",
            "Holds@Invariant(Ok)",
            "PartialDefined@Invariant(Ok) [v]",
            "NoOverflow@Invariant(Ok) [v]",
            "KeyInDomain@Invariant(Ok) [v]",
            "PartialDefined@Guard(tick) [v]",
            "NoOverflow@Guard(tick) [v]",
            "KeyInDomain@Guard(tick) [v]",
            "PartialDefined@Body(tick) [v]",
            "NoOverflow@Body(tick)",
            "KeyInDomain@Body(tick) [v]",
            "NoDeadlock@Model",
        ],
    );
}

/// Every kind and every site variant is exercised by some fixture above, so
/// none of them is pinned by an empty expectation.
#[test]
fn fixtures_cover_every_kind_and_site() {
    let rows = [
        model(PARTIAL_GUARD),
        model(ENSURES_HOLE),
        model(DEADLOCK_RANK),
        dup(),
        model(MIN_HELPFUL),
        model(MAP_OUT_OF_DOMAIN),
        model(KEY_DOMAINS),
        model(ARITH),
        model(FAMILIES),
    ]
    .iter()
    .flat_map(|model| catalog(model).obligations)
    .collect::<Vec<_>>();
    let kinds = rows.iter().map(|row| row.id.kind).collect::<BTreeSet<_>>();
    assert_eq!(kinds, ObligationKind::ALL.into_iter().collect());
    assert_eq!(
        ObligationKind::ALL.len(),
        ObligationKind::ALL
            .into_iter()
            .collect::<BTreeSet<_>>()
            .len()
    );
    let sites = rows
        .iter()
        .map(|row| site_text(&row.id.site).split('(').next().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let expected = [
        "TypeBound",
        "Invariant",
        "Trans",
        "Reachable",
        "LeadsTo",
        "Trigger",
        "Goal",
        "Measure",
        "RankStep",
        "Terminal",
        "Guard",
        "Body",
        "Ensures",
        "Init",
        "Model",
    ]
    .into_iter()
    .map(|name| Some(name.to_owned()))
    .collect::<BTreeSet<_>>();
    assert_eq!(sites, expected);
}
