// SPDX-License-Identifier: Apache-2.0

//! T2 of `docs/design/DESIGN-obligation-catalog.md` (#1202): the minimum
//! obligation catalog of each rule family, pinned as an exact multiset per
//! fixture. Dropping any generator -- a family, a row kind, a site, or a
//! vacuity predicate -- changes a fixture's rows and fails here. The fixtures
//! are the reproducers of #1189, #1192, #1196, #1217 and #1221.

use std::collections::BTreeSet;

use fsl_core::obligation::{Obligation, ObligationId, ObligationKind, Site, SiteRef, catalog};
use fsl_core::{FsResolver, KernelModel, Span, build_model, parse_kernel_source};

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
            "PartialDefined@Guard(dec)",
            "PartialDefined@Body(dec) [v]",
            "PartialDefined@Guard(zap) [v]",
            "PartialDefined@Body(zap) [v]",
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
            "PartialDefined@Guard(inc) [v]",
            "PartialDefined@Body(inc) [v]",
            "Holds@Ensures(inc#0)",
            "PartialDefined@Ensures(inc#0) [v]",
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

#[test]
fn issue_1192_same_named_properties_are_distinct_rows() {
    let mut dup = model(DUP);
    "L".clone_into(&mut dup.leadstos[1].name);
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
            "PartialDefined@Goal(L) [v]",
            "PartialDefined@Goal(L) [v]",
            "PartialDefined@Guard(flip) [v]",
            "PartialDefined@Body(flip) [v]",
            "PartialDefined@Guard(inc) [v]",
            "PartialDefined@Body(inc) [v]",
            "NoDeadlock@Model",
        ],
    );
    let responds = rows
        .iter()
        .filter(|row| row.id.kind == ObligationKind::Responds)
        .map(|row| row.id.site.clone())
        .collect::<BTreeSet<_>>();
    let declared = dup
        .leadstos
        .iter()
        .map(|property| Site::LeadsTo(site_ref(&property.name, property.span)))
        .collect::<BTreeSet<_>>();
    assert_eq!(declared.len(), 2, "the fixture's spans differ");
    assert_eq!(responds, declared);
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
            "Holds@Invariant(Peek)",
            "PartialDefined@Invariant(Peek)",
            "Holds@Trans(Grow)",
            "PartialDefined@Trans(Grow) [v]",
            "Holds@Trans(Stay)",
            "PartialDefined@Trans(Stay) [v]",
            "Holds@Trans(Hold_until_safety)",
            "PartialDefined@Trans(Hold_until_safety) [v]",
            "Responds@LeadsTo(Hold)",
            "PartialDefined@Trigger(Hold) [v]",
            "PartialDefined@Goal(Hold) [v]",
            "Responds@LeadsTo(Drain)",
            "PartialDefined@Trigger(Drain)",
            "PartialDefined@Goal(Drain) [v]",
            "Witnessed@Reachable(Full)",
            "PartialDefined@Reachable(Full)",
            "PartialDefined@Terminal",
            "PartialDefined@Guard(add) [v]",
            "PartialDefined@Body(add) [v]",
            "PartialDefined@Guard(drop)",
            "PartialDefined@Body(drop)",
            "Holds@Ensures(drop#0)",
            "PartialDefined@Ensures(drop#0) [v]",
            "Holds@Ensures(drop#1)",
            "PartialDefined@Ensures(drop#1)",
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

/// Every kind and every site variant is exercised by some fixture above, so
/// none of them is pinned by an empty expectation.
#[test]
fn fixtures_cover_every_kind_and_site() {
    let mut dup = model(DUP);
    "L".clone_into(&mut dup.leadstos[1].name);
    let rows = [
        model(PARTIAL_GUARD),
        model(ENSURES_HOLE),
        dup,
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
