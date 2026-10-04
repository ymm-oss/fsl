// SPDX-License-Identifier: Apache-2.0

//! T2 of `docs/design/DESIGN-obligation-catalog.md` (#1202): the minimum
//! obligation catalog of each rule family, pinned as an exact multiset per
//! fixture. Dropping any generator -- a family, a row kind, a site, or a
//! vacuity predicate -- changes a fixture's rows and fails here. The fixtures
//! are the reproducers of #1189, #1192, #1196, #1217 and #1221.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::obligation::{Obligation, ObligationId, ObligationKind, Site, SiteRef, catalog};
use fsl_core::{
    FsResolver, KernelModel, Span, build_model, parse_kernel_source,
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

/// The key-domain predicate: a read or write is in domain only when the
/// index's static type (or literal value) lies inside the key type. Each
/// site isolates one case; `Two`/`Writes` hold two misses, `Seq` a `Seq` read.
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

#[test]
fn key_rows_follow_the_index_type() {
    let rows = catalog(&model(KEY_DOMAINS)).obligations;
    let live = rows
        .iter()
        .filter(|row| row.id.kind == ObligationKind::KeyInDomain && !row.statically_vacuous)
        .map(|row| site_text(&row.id.site))
        .collect::<BTreeSet<_>>();
    let expected = [
        "Body(writeHigh)",
        "Body(writes)",
        "Body(branch)",
        "Invariant(High)",
        "Invariant(Below)",
        "Invariant(LitOut)",
        "Invariant(Filtered)",
        "Invariant(Two)",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    assert_eq!(live, expected);
    let key_sites = rows
        .iter()
        .filter(|row| row.id.kind == ObligationKind::KeyInDomain)
        .count();
    assert_eq!(
        key_sites,
        7 * 2 + 14,
        "every action guard/body and invariant"
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

/// Property selection removes sites from the model the engines see; the rows
/// of the sites it keeps are unchanged and it adds none.
#[test]
fn a_selected_model_owes_a_subset_with_identical_kept_rows() {
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

/// `entity`/`number` sizes are a verification scope, not a declaration: a
/// scope override leaves every row as it is.
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
    assert_eq!(catalog(&declared), catalog(&overridden));
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
