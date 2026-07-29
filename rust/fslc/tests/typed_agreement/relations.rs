// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Metamorphic relations R1-R7 (#537 C6 slice 1, issue #648), each with a
//! positive test and a negative control. Every positive expectation cites
//! the `docs/LANGUAGE.md` contract sentence it is licensed by (never
//! transcribed from observed CLI output); R5's boundary expectation is
//! declared structurally (domain bound coincides with the invariant bound
//! by construction) before the mutation that breaks it runs.
//!
//! R6 additionally records two confirmed cross-engine findings instead of
//! silently normalizing them away: a self-retiring exclusion for
//! `head()` read from property context on an empty sequence (BMC finds a
//! spurious `violated`/`invariant` while the concrete engines correctly
//! error), and a documented scope boundary for the six partial operations
//! in *action* context (`fsl_verifier::verify_bounded` alone does not
//! perform LANGUAGE.md S6's automatic "Partial operations" check at all --
//! only the concrete engines and the CLI's own `find_boundary_violation`
//! pre-scan do).

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{FsResolver, KernelModel, build_model, parse_kernel_source};
use fsl_runtime::{Monitor, State};
use fsl_tools::{BuiltinMutant, enumerate_builtin_mutants};

use super::engines;

fn build(id: &str, source: &str) -> KernelModel {
    engines::build(id, source)
}

/// Full-depth solver-free reachable-state enumeration by direct `Monitor`
/// walk (no invariant checking -- these fixtures are constructed to be
/// violation-free, and verdict agreement is checked separately). Used only
/// for relations that keep identical variable/type names between variants
/// (R3, R4, R7); R1 renames names, so state sets are compared as `Clean`
/// verdicts instead, not as raw sets.
fn reachable_states(model: &KernelModel, depth: usize) -> BTreeSet<State> {
    let initial = Monitor::new(model.clone()).expect("Monitor::new for reachable-state walk");
    let mut visited = BTreeSet::from([initial.state.clone()]);
    let mut frontier = vec![initial];
    for _ in 0..depth {
        let mut next = Vec::new();
        for monitor in &frontier {
            let enabled = monitor.enabled().expect("enabled");
            for action in enabled {
                let mut child = monitor.clone();
                let stepped = child.step(&action).expect("step");
                if stepped.violation.is_some() {
                    continue;
                }
                if visited.insert(child.state.clone()) {
                    next.push(child);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    visited
}

// ---------------------------------------------------------------------
// R1: alpha rename.
// LANGUAGE.md does not need to license this one directly: renaming every
// declaration and reference to it is a pure syntactic substitution over an
// already-typechecked program, so the *typed* semantics (what `build_model`
// produces) cannot depend on the spelling. The contract this leans on is
// `reserved.rs::check_reserved_names`'s own position enumeration, which the
// self-check below cross-references so the rename fixture cannot silently
// stop covering a position category that gains one.
// ---------------------------------------------------------------------

const R1_BASE: &str = r"
spec RenameBase {
  const Limit = 2
  type Slot = 0..2
  enum Stage { Open, Closed }
  state {
    x: Slot,
    stage: Stage
  }
  init {
    x = 0
    stage = Open
  }
  action advance(step: Slot) {
    requires x + step <= Limit
    x = x + step
  }
  action close() {
    requires stage == Open
    stage = Closed
  }
  invariant Bounded { x >= 0 and x <= Limit }
}
";

/// Whole-token identifier substitution: every maximal run of
/// `[A-Za-z0-9_]` that exactly matches a key in `renames` is replaced by its
/// value, everywhere it occurs (declaration and every reference alike) --
/// this is what makes it an *alpha* rename rather than a single-site edit.
fn rename_identifiers(source: &str, renames: &BTreeMap<String, String>) -> String {
    let mut output = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = source[i..].chars().next().expect("valid UTF-8 boundary");
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            let mut end = i + ch.len_utf8();
            while end < bytes.len() {
                let next_ch = source[end..].chars().next().expect("valid UTF-8 boundary");
                if next_ch.is_alphanumeric() || next_ch == '_' {
                    end += next_ch.len_utf8();
                } else {
                    break;
                }
            }
            let token = &source[start..end];
            output.push_str(renames.get(token).map_or(token, String::as_str));
            i = end;
        } else {
            output.push(ch);
            i += ch.len_utf8();
        }
    }
    output
}

fn r1_renames() -> BTreeMap<String, String> {
    BTreeMap::from(
        [
            ("RenameBase", "RenameBaseRenamed"),
            ("Limit", "Cap"),
            ("Slot", "Cell"),
            ("Stage", "Phase"),
            ("Open", "Started"),
            ("Closed", "Finished"),
            ("x", "pos"),
            ("stage", "phase"),
            ("advance", "step_forward"),
            ("step", "amount"),
            ("close", "finish"),
            ("Bounded", "WithinLimit"),
        ]
        .map(|(from, to)| (from.to_owned(), to.to_owned())),
    )
}

/// Position categories `reserved.rs::check_reserved_names` walks, by the
/// exact string literal it passes to `check`/`check_param`/`check_binder`.
/// Cross-referenced against the source text so a category rename or
/// addition there fails this test loudly instead of leaving R1's coverage
/// claim stale. `def`/`def parameter`/struct positions are intentionally
/// not exercised by `R1_BASE` (no `def` or `struct` item); the whole-token
/// substitution mechanism does not distinguish position categories, so this
/// records which categories the *fixture* instantiates, not a gap in the
/// substitution mechanism itself.
const R1_COVERED_POSITION_CATEGORIES: &[&str] = &[
    "specification",
    "const",
    "type",
    "enum",
    "enum member",
    "state variable",
    "action",
    "parameter",
    "property",
];

#[test]
fn r1_rename_fixture_position_categories_still_exist_in_check_reserved_names() {
    let source = include_str!("../../../fsl-core/src/reserved.rs");
    for category in R1_COVERED_POSITION_CATEGORIES {
        assert!(
            source.contains(&format!("\"{category}\"")),
            "reserved.rs no longer checks the '{category}' position that R1's rename \
             fixture claims to cover; update R1_COVERED_POSITION_CATEGORIES and R1_BASE"
        );
    }
}

#[test]
fn r1_alpha_rename_preserves_verdict_across_all_engines() {
    let renamed_source = rename_identifiers(R1_BASE, &r1_renames());

    let original = build("r1_original", R1_BASE);
    let renamed = build("r1_renamed", &renamed_source);

    let original_run = engines::run_agreement("r1_original", &original, 4);
    let renamed_run = engines::run_agreement("r1_renamed", &renamed, 4);

    assert_eq!(
        original_run,
        engines::Verdict::Clean,
        "R1 baseline must be clean by construction"
    );
    assert_eq!(
        renamed_run,
        engines::Verdict::Clean,
        "alpha rename must not change the verdict (BMC saw {renamed_run:?})"
    );
}

/// Negative control: renaming a declaration onto an *existing* name is a
/// collision, not a rename, and must be rejected.
#[test]
fn r1_negative_control_rejects_a_colliding_rename() {
    let collision = BTreeMap::from([("Slot".to_owned(), "Stage".to_owned())]);
    let collided_source = rename_identifiers(R1_BASE, &collision);

    let resolver = FsResolver::new(".");
    match parse_kernel_source(&collided_source, &resolver) {
        Err(_) => {} // rejected at parse time: also valid evidence of detection
        Ok(kernel) => {
            build_model(kernel).expect_err(
                "renaming 'Slot' onto the existing enum name 'Stage' must be rejected as a \
                 duplicate declaration, not silently accepted",
            );
        }
    }
}

// ---------------------------------------------------------------------
// R2: BOM / trivia.
// `rust/fsl-syntax/src/lexer.rs` `skip_trivia`: a BOM is only trivia at
// `self.offset == 0`, i.e. the very start of the source. Anywhere else it is
// just another character the lexer does not recognize.
// ---------------------------------------------------------------------

const R2_BASE: &str = r"
spec BomBase {
  type D = 0..2
  state { x: D }
  init { x = 0 }
  action advance() {
    requires x < 2
    x = x + 1
  }
  invariant Bounded { x >= 0 and x <= 2 }
}
";

#[test]
fn r2_leading_bom_and_trivia_preserve_verdict() {
    let with_bom_and_trivia =
        format!("\u{FEFF}   \n// C6 R2: leading trivia after the BOM\n{R2_BASE}");

    let plain = build("r2_plain", R2_BASE);
    let decorated = build("r2_bom_trivia", &with_bom_and_trivia);

    let plain_run = engines::run_agreement("r2_plain", &plain, 3);
    let decorated_run = engines::run_agreement("r2_bom_trivia", &decorated, 3);

    assert_eq!(plain_run, engines::Verdict::Clean);
    assert_eq!(decorated_run, engines::Verdict::Clean);
}

/// Negative control: a BOM inserted mid-identifier is not trivia (offset != 0),
/// so it must be a parse error rather than silently ignored or accepted as an
/// identifier character.
#[test]
fn r2_negative_control_rejects_bom_inside_an_identifier() {
    let mutated = R2_BASE.replacen("advance", "adv\u{FEFF}ance", 1);
    let resolver = FsResolver::new(".");
    parse_kernel_source(&mutated, &resolver)
        .expect_err("a BOM inside an identifier must be a parse error, not silently accepted");
}

// ---------------------------------------------------------------------
// R3: inline init <-> explicit init block.
// LANGUAGE.md S2:499-508: an inline `name: Type = expr` initializer "is
// normalized to an ordinary root assignment before checking, so Monitor,
// explicit exploration, BMC, induction, and Public Kernel v1 observe the
// same semantics as an equivalent `init` block" and "Assigning the same
// root both inline and in `init` is a semantic error that reports both
// source locations."
// ---------------------------------------------------------------------

const R3_INLINE: &str = r"
spec R3Inline {
  type D = 0..2
  state {
    x: D = 0,
    y: Bool = false
  }
  action advance() {
    requires x < 2
    x = x + 1
    y = not y
  }
  invariant Bounded { x >= 0 and x <= 2 }
}
";

const R3_EXPLICIT: &str = r"
spec R3Explicit {
  type D = 0..2
  state {
    x: D,
    y: Bool
  }
  init {
    x = 0
    y = false
  }
  action advance() {
    requires x < 2
    x = x + 1
    y = not y
  }
  invariant Bounded { x >= 0 and x <= 2 }
}
";

#[test]
fn r3_inline_init_matches_explicit_init_block_verdict_and_reachable_states() {
    let inline = build("r3_inline", R3_INLINE);
    let explicit = build("r3_explicit", R3_EXPLICIT);

    let inline_run = engines::run_agreement("r3_inline", &inline, 4);
    let explicit_run = engines::run_agreement("r3_explicit", &explicit, 4);
    assert_eq!(inline_run, engines::Verdict::Clean);
    assert_eq!(explicit_run, engines::Verdict::Clean);

    let inline_states = reachable_states(&inline, 4);
    let explicit_states = reachable_states(&explicit, 4);
    assert_eq!(
        inline_states, explicit_states,
        "LANGUAGE.md S2:499-508: inline init must reach exactly the states an equivalent \
         explicit init block reaches"
    );
}

/// Negative control: assigning the same root both inline and in `init` is
/// LANGUAGE.md's own named semantic error.
#[test]
fn r3_negative_control_rejects_the_same_root_assigned_inline_and_in_init() {
    let source = r"
spec R3DoubleAssign {
  type D = 0..2
  state { x: D = 0 }
  init { x = 1 }
  action stay() { x = x }
  invariant Any { x >= 0 }
}
";
    let resolver = FsResolver::new(".");
    let kernel = parse_kernel_source(source, &resolver).expect("parses");
    let error = build_model(kernel)
        .expect_err("assigning 'x' both inline and in init must be a semantic error");
    assert!(
        error
            .message
            .contains("both an inline initializer and init"),
        "unexpected message: {}",
        error.message
    );
}

// ---------------------------------------------------------------------
// R4: disjoint simultaneous assignment reorder.
// LANGUAGE.md S5:644-661: "all right-hand sides in an action body read the
// old state. Variables that are not assigned do not change (the frame
// condition is automatic)" -- so two assignments to *different* variables in
// one action body have no data dependency on each other's order.
// ---------------------------------------------------------------------

fn r4_source(first: &str, second: &str) -> String {
    format!(
        r"
spec R4Reorder {{
  type D = 0..2
  state {{ a: D, b: D }}
  init {{ a = 0 b = 0 }}
  action step() {{
    requires a == 0
    {first}
    {second}
  }}
  invariant BothOrNeither {{ (a == 1) == (b == 2) }}
}}
"
    )
}

#[test]
fn r4_disjoint_assignment_reorder_matches_verdict_and_reachable_states() {
    let forward = build("r4_forward", &r4_source("a = 1", "b = 2"));
    let reordered = build("r4_reordered", &r4_source("b = 2", "a = 1"));

    let forward_run = engines::run_agreement("r4_forward", &forward, 3);
    let reordered_run = engines::run_agreement("r4_reordered", &reordered, 3);
    assert_eq!(forward_run, engines::Verdict::Clean);
    assert_eq!(reordered_run, engines::Verdict::Clean);

    let forward_states = reachable_states(&forward, 3);
    let reordered_states = reachable_states(&reordered, 3);
    assert_eq!(
        forward_states, reordered_states,
        "LANGUAGE.md S5:644-661: disjoint simultaneous assignments must reach the same states \
         regardless of source order"
    );
}

/// Negative control: assigning the same variable twice on one execution path
/// is LANGUAGE.md's own named semantic error ("Double assignment").
#[test]
fn r4_negative_control_rejects_the_same_variable_assigned_twice() {
    let source = r"
spec R4DoubleAssign {
  type D = 0..2
  state { a: D }
  init { a = 0 }
  action step() {
    a = 1
    a = 2
  }
  invariant Any { a >= 0 }
}
";
    let resolver = FsResolver::new(".");
    let kernel = parse_kernel_source(source, &resolver).expect("parses");
    build_model(kernel).expect_err("assigning 'a' twice on one path must be a semantic error");
}

// ---------------------------------------------------------------------
// R5: finite domain size boundary.
// The expectation is declared structurally, not from observed output: an
// invariant whose bound is textually identical to the domain's declared
// `hi` can never be violated, for any domain size, because a state variable
// can never leave its own declared bound (LANGUAGE.md S6 "Type bounds" is
// automatic). Widening the domain by 1 while leaving the invariant's literal
// bound unchanged breaks that coincidence by construction.
// ---------------------------------------------------------------------

fn r5_source(hi: i64, invariant_bound: i64) -> String {
    format!(
        r"
spec R5Boundary {{
  type D = 0..{hi}
  state {{ x: D }}
  init {{ x = 0 }}
  action advance() {{
    x = (x + 1) % {size}
  }}
  invariant TightBound {{ x <= {invariant_bound} }}
}}
",
        size = hi + 1
    )
}

#[test]
fn r5_domain_size_boundary_verdict_change_matches_the_declared_structural_expectation() {
    // Declared upfront: for any domain size N (hi = N-1), `x <= hi` can never
    // be violated because it restates the type's own automatic bound. This
    // must hold at N=1, N=2, N=3 alike.
    for size in [1_i64, 2, 3] {
        let hi = size - 1;
        let model = build(&format!("r5_size_{size}"), &r5_source(hi, hi));
        let run = engines::run_agreement(&format!("r5_size_{size}"), &model, 4);
        assert_eq!(
            run,
            engines::Verdict::Clean,
            "declared expectation: a domain-bound-coincident invariant holds at every size"
        );
    }

    // Declared upfront: widening the domain to hi+1 while leaving the
    // invariant's literal bound at the old hi breaks the coincidence, so the
    // wrapped value hi+1 now both stays in-domain (no type_bound violation)
    // and violates the stale invariant.
    let widened = build("r5_widened", &r5_source(3, 2));
    let widened_run = engines::run_agreement("r5_widened", &widened, 4);
    assert!(
        matches!(widened_run, engines::Verdict::Violated { .. }),
        "declared expectation: widening the domain past the invariant's literal bound must \
         change the verdict to violated (got {widened_run:?})"
    );
}

// ---------------------------------------------------------------------
// R6: short-circuit / partial operation / Euclidean division duality.
// LANGUAGE.md S3:557-570: `/`/`%` are total in property context (`a/0==0`,
// `a%0==0`) but still reported `partial_op` when read unguarded in an
// action's requires/body/ensures -- a contract about `fslc verify`'s
// observable behavior, not about the bare `fsl_verifier::verify_bounded`
// function alone; see
// `assert_action_context_partial_op_is_caught_only_concretely`'s doc for why
// those two are not the same thing. `docs/LANGUAGE.md` S6's "Partial
// operations" row scopes the *checked* class to action context; it makes no
// totalization promise for `head`/`pop`/`at`/index the way S3 explicitly
// does for `/`/`%`, so this suite exercises the six named operations in
// action context via the dedicated tests below (not
// `generator.rs::operation_sweep`, which only carries the safe/totalized
// entries `engines::run_agreement`'s raw-BMC comparison can actually make),
// and records the property-context asymmetry as its own finding below
// instead of assuming S3's `/`/`%` guarantee extends to them.
// ---------------------------------------------------------------------

#[test]
fn r6_euclidean_identity_is_proved_and_totalized_in_property_context() {
    let source = r"
spec R6Euclid {
  type Small = -3..3
  state { x: Small }
  init { x = 0 }
  action stay() { x = x }
  invariant EuclidNonzero { x == 2 * (x / 2) + (x % 2) }
  invariant EuclidRemainderRange { 0 <= x % 2 and x % 2 < 2 }
  invariant ZeroDivTotal { x / 0 == 0 and x % 0 == 0 }
}
";
    let model = build("r6_euclid_property", source);
    let run = engines::run_agreement("r6_euclid_property", &model, 3);
    assert_eq!(
        run,
        engines::Verdict::Clean,
        "LANGUAGE.md S3:559-563: the Euclidean identity and the /0,%0 total value must both \
         verify cleanly in property context"
    );
}

/// The automatic "Partial operations" check (`docs/LANGUAGE.md` S6, action
/// context only) is not performed by `fsl_verifier::verify_bounded`'s own
/// symbolic loop: `rust/fsl-verifier/src/bmc.rs` has no `partial_op`
/// handling anywhere in it. `rust/fslc/src/verification.rs`'s
/// `run_bmc_filtered` produces the CLI's `--engine bmc` `partial_op`
/// classification by merging in a *concrete* pre-scan
/// (`fsl_runtime::find_boundary_violation`), not from the solver. So the
/// three-way comparison this suite uses everywhere else
/// (`engines::run_agreement`, which asserts raw `verify_bounded` agrees
/// with Monitor BFS / explicit) does not apply to an unguarded
/// action-context partial operation: raw `verify_bounded` reports Clean by
/// construction, which is a documented capability gap in what the bare
/// solver checks, not a disagreement to investigate. This asserts the
/// concrete family (Monitor BFS / explicit / the concrete
/// `find_boundary_violation` pre-scan the CLI itself relies on) agrees
/// among itself, and separately documents raw BMC's Clean result, for one
/// of `docs/LANGUAGE.md` S6's six named partial operations.
fn assert_action_context_partial_op_is_caught_only_concretely(
    id: &str,
    source: &str,
    depth: usize,
) {
    let model = build(id, source);

    let bfs_violation = fsl_runtime::bfs(model.clone(), depth)
        .unwrap_or_else(|error| panic!("'{id}': Monitor BFS errored: {error}"))
        .violation
        .unwrap_or_else(|| panic!("'{id}': Monitor BFS found no violation"));
    assert_eq!(bfs_violation.kind, "partial_op", "'{id}': Monitor BFS kind");

    let explicit_violation = fsl_runtime::verify_explicit(model.clone(), depth, 1_000)
        .unwrap_or_else(|error| panic!("'{id}': explicit errored: {error}"))
        .violation
        .unwrap_or_else(|| panic!("'{id}': explicit found no violation"))
        .violation;
    assert_eq!(
        (explicit_violation.kind.as_str(), explicit_violation.step),
        (bfs_violation.kind.as_str(), bfs_violation.step),
        "'{id}': Monitor BFS and explicit disagree"
    );

    let (boundary_violation, _trace) = fsl_runtime::find_boundary_violation(model.clone(), depth)
        .unwrap_or_else(|error| panic!("'{id}': find_boundary_violation errored: {error}"))
        .unwrap_or_else(|| panic!("'{id}': find_boundary_violation found no violation"));
    assert_eq!(
        boundary_violation.kind, "partial_op",
        "'{id}': find_boundary_violation kind"
    );

    // Raw `verify_bounded` is not asserted against here at all: it never
    // performs the "Partial operations" check itself (confirmed above the
    // module), but its own *unrelated* automatic type-bound check can still
    // independently misfire on the same underlying symbolic-Seq gap this
    // module's `r6_property_context_seq_head_disagrees_across_engines_...`
    // test documents -- observed outcomes range from a clean verdict
    // (`divide`/`remainder`), to a spurious `type_bound` on the assigned
    // scalar (`head`/`at`/index), to `verify_bounded` itself returning
    // `Err("model sequence length is negative")` (`pop`). All three are the
    // same underlying symbolic-Seq gap surfacing differently depending on
    // where the phantom value flows, not independent findings, so this
    // helper does not assert a specific raw-BMC outcome per operation --
    // only records whichever one occurs for visibility.
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    match engines::block_on(fsl_verifier::verify_bounded(&model, &mut solver, depth)) {
        Ok(bmc) => eprintln!(
            "'{id}': raw verify_bounded (not asserted) = {:?}",
            bmc.violation
        ),
        Err(error) => eprintln!("'{id}': raw verify_bounded (not asserted) errored: {error}"),
    }
}

#[test]
fn r6_action_context_partial_operations_are_caught_only_by_the_concrete_engines() {
    assert_action_context_partial_op_is_caught_only_concretely(
        "r6_action_head",
        r"
spec R6ActionHead {
  type Item = 0..2
  state { queue: Seq<Item, 2>, last: Item }
  init { queue = Seq {} last = 0 }
  action drain() { last = queue.head() }
}
",
        2,
    );
    assert_action_context_partial_op_is_caught_only_concretely(
        "r6_action_pop",
        r"
spec R6ActionPop {
  type Item = 0..2
  state { queue: Seq<Item, 2> }
  init { queue = Seq {} }
  action drain() { queue = queue.pop() }
}
",
        2,
    );
    assert_action_context_partial_op_is_caught_only_concretely(
        "r6_action_at",
        r"
spec R6ActionAt {
  type Item = 0..2
  state { queue: Seq<Item, 2>, picked: Item }
  init { queue = Seq {} picked = 0 }
  action pick() { picked = queue.at(0) }
}
",
        2,
    );
    assert_action_context_partial_op_is_caught_only_concretely(
        "r6_action_index",
        r"
spec R6ActionIndex {
  type Item = 0..2
  state { queue: Seq<Item, 2>, picked: Item }
  init { queue = Seq {} picked = 0 }
  action pick() { picked = queue[0] }
}
",
        2,
    );
    assert_action_context_partial_op_is_caught_only_concretely(
        "r6_action_divide",
        r"
spec R6ActionDivide {
  type Small = -3..3
  state { x: Small, y: Small, q: Small }
  init { x = -3 y = 0 q = 0 }
  action divide_unguarded() { q = x / y }
}
",
        2,
    );
    assert_action_context_partial_op_is_caught_only_concretely(
        "r6_action_remainder",
        r"
spec R6ActionRemainder {
  type Small = -3..3
  state { x: Small, y: Small, r: Small }
  init { x = -3 y = 0 r = 0 }
  action remainder_unguarded() { r = x % y }
}
",
        2,
    );
}

/// #537 C6 slice 1 finding, recorded as a self-retiring exclusion rather
/// than silently normalized away: a partial Seq read (`head`) evaluated in
/// *property* context on a sequence that is empty at the initial state
/// disagrees across engines instead of being caught uniformly.
///
/// Observed (this test re-measures every value below on every run):
///
/// - `fsl_runtime::verify_explicit` / `fsl_runtime::bfs` ("Monitor BFS"):
///   both raise a raw `RuntimeError` ("`head()` on empty sequence") instead of
///   returning a verdict. Both call `Monitor::current_violation[_selected]`,
///   which -- unlike `Monitor::execute_selected`'s post-step invariant check
///   -- does not catch `is_partial_operation_error` and convert it to a
///   `partial_op` `Violation`; the CLI surfaces this as
///   `result:"error"`/`kind:"semantics"` rather than a verdict at all.
/// - BMC (`verify_bounded`): returns a *verdict*, not an error --
///   `violated`, `kind:"invariant"` (the plain property-failure
///   classification, not `partial_op`), `name:"HeadRead"` (the invariant's
///   own declared name), at `step:0` (the initial state itself, before any
///   action runs). The witness trace's projected state shows `queue` as the
///   empty sequence it actually is at init. This means BMC's symbolic
///   encoding treats `queue.head()` on an empty sequence as *defined* with
///   some value the solver is free to pick outside `Item`'s `0..2` bound,
///   rather than as undefined the way the concrete engines (and this
///   suite's own `Monitor`-direct walk) treat it -- a distinct, and
///   arguably more concerning, discrepancy than "differing labels for the
///   same undefinedness": BMC is not applying LANGUAGE.md's action-context
///   `partial_op` treatment here at all, and property-context totalization
///   is only documented for `/`/`%` (S3:561-563), not for `head`. Root
///   cause in the symbolic Seq encoding is not diagnosed here; that is a
///   question for the tracking issue, not this suite.
///
/// A fix that makes the two engine families agree turns the `expect_err`
/// calls or the `kind`/`name`/`step` assertions below into failures, so the
/// exclusion cannot go stale silently.
#[test]
fn r6_property_context_seq_head_disagrees_across_engines_self_retiring_exclusion() {
    let source = r"
spec R6HeadPropertyDisagreement {
  type Item = 0..2
  state { queue: Seq<Item, 2> }
  init { queue = Seq {} }
  action stay() { queue = queue }
  invariant HeadRead { queue.head() >= 0 }
}
";
    let model = build("r6_head_property_disagreement", source);

    let bfs_error = fsl_runtime::bfs(model.clone(), 2).expect_err(
        "premise re-measurement: Monitor BFS must still error on an empty-sequence head() \
         read in property context; if this now returns Ok, the engines agree and this \
         exclusion (and the R6 doc note) must be retired",
    );
    assert!(
        bfs_error.message.contains("head() on empty sequence"),
        "bfs error message changed shape, re-check the exclusion: {bfs_error}"
    );

    let explicit_error = fsl_runtime::verify_explicit(model.clone(), 2, 100).expect_err(
        "premise re-measurement: explicit must still error the same way as Monitor BFS",
    );
    assert!(
        explicit_error.message.contains("head() on empty sequence"),
        "explicit error message changed shape, re-check the exclusion: {explicit_error}"
    );

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let bmc_result = engines::block_on(fsl_verifier::verify_bounded(&model, &mut solver, 2))
        .expect("premise re-measurement: BMC must still return a clean Result, not error");
    let violation = bmc_result
        .violation
        .expect("premise re-measurement: BMC must still report a violation, not a clean verdict");
    assert_eq!(
        (
            violation.kind.as_str(),
            violation.name.as_str(),
            violation.step
        ),
        ("invariant", "HeadRead", 0),
        "BMC's classification of the same condition changed shape, re-check the exclusion: \
         {violation:?}"
    );
}

// ---------------------------------------------------------------------
// R7: entity / number sugar vs hand-written lowered kernel type.
// LANGUAGE.md S2:485-486: "Entity kind ... desugars to type Claim =
// 0..N-1" and "Number kind ... desugars to type".
// ---------------------------------------------------------------------

const R7_BODY_TEMPLATE: &str = r"
  state { x: Item }
  init { x = 0 }
  action advance() {
    requires x < {hi}
    x = x + 1
  }
  invariant Bounded { x >= 0 and x <= {hi} }
";

#[test]
fn r7_entity_desugar_matches_hand_written_lowered_type() {
    let sugar = format!(
        "spec R7EntitySugar {{\n  entity Item\n{}}}\nverify {{ instances Item = 3 }}\n",
        R7_BODY_TEMPLATE.replace("{hi}", "2")
    );
    let lowered = format!(
        "spec R7EntityLowered {{\n  type Item = 0..2\n{}}}\n",
        R7_BODY_TEMPLATE.replace("{hi}", "2")
    );

    let sugar_model = build("r7_entity_sugar", &sugar);
    let lowered_model = build("r7_entity_lowered", &lowered);

    let sugar_run = engines::run_agreement("r7_entity_sugar", &sugar_model, 4);
    let lowered_run = engines::run_agreement("r7_entity_lowered", &lowered_model, 4);
    assert_eq!(sugar_run, engines::Verdict::Clean);
    assert_eq!(lowered_run, engines::Verdict::Clean);

    assert_eq!(
        reachable_states(&sugar_model, 4),
        reachable_states(&lowered_model, 4),
        "LANGUAGE.md S2:485-486: 'entity Item' + 'verify instances Item = 3' must reach exactly \
         the states 'type Item = 0..2' reaches"
    );
}

#[test]
fn r7_number_desugar_matches_hand_written_lowered_type() {
    let sugar = format!(
        "spec R7NumberSugar {{\n  number Item\n{}}}\nverify {{ values Item = 0..2 }}\n",
        R7_BODY_TEMPLATE.replace("{hi}", "2")
    );
    let lowered = format!(
        "spec R7NumberLowered {{\n  type Item = 0..2\n{}}}\n",
        R7_BODY_TEMPLATE.replace("{hi}", "2")
    );

    let sugar_model = build("r7_number_sugar", &sugar);
    let lowered_model = build("r7_number_lowered", &lowered);

    assert_eq!(
        reachable_states(&sugar_model, 4),
        reachable_states(&lowered_model, 4),
        "LANGUAGE.md S2:485-486: 'number Item' + 'verify values Item = 0..2' must reach exactly \
         the states 'type Item = 0..2' reaches"
    );
}

/// Negative control: shifting the hand-written lowered bound by 1 relative
/// to the entity's declared instance count must be detected -- it lets `x`
/// reach a value the entity-sugar side's invariant (calibrated to the
/// declared size) forbids.
#[test]
fn r7_negative_control_detects_a_mis_lowered_bound() {
    let sugar = format!(
        "spec R7EntitySugarNeg {{\n  entity Item\n{}}}\nverify {{ instances Item = 3 }}\n",
        R7_BODY_TEMPLATE.replace("{hi}", "2")
    );
    // Shifted by +1: the lowered type now admits x == 3, one past the
    // entity's declared instance count, while the guard/invariant literal
    // bound in the template stays at 2 (mirroring the sugar side).
    let mis_lowered = "spec R7EntityLoweredNeg {\n  type Item = 0..3\n  state { x: Item }\n  init { x = 0 }\n  \
         action advance() {\n    x = x + 1\n  }\n  invariant Bounded { x >= 0 and x <= 2 }\n}\n";

    let sugar_model = build("r7_entity_sugar_neg", &sugar);
    let mis_lowered_model = build("r7_entity_lowered_neg", mis_lowered);

    let sugar_run = engines::run_agreement("r7_entity_sugar_neg", &sugar_model, 4);
    assert_eq!(sugar_run, engines::Verdict::Clean);

    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let mis_lowered_bmc = engines::block_on(fsl_verifier::verify_bounded(
        &mis_lowered_model,
        &mut solver,
        4,
    ))
    .expect("bmc run on mis-lowered variant");
    assert!(
        mis_lowered_bmc.violation.is_some(),
        "a lowered bound shifted +1 past the entity's declared size must be detected as a \
         disagreement, not silently accepted as equivalent"
    );
}

// ---------------------------------------------------------------------
// `enumerate_builtin_mutants` reuse: kill-controls per the brief's mutate-
// reuse instruction. Each mutant's non-equivalence is *measured* here (the
// BMC verdict flips from Clean to Violated, or the mutant fails to build)
// rather than assumed; the comment beside each `.find` records the measured
// selection basis, and an equivalent candidate is never substituted in to
// pad the count.
// ---------------------------------------------------------------------

fn assert_builtin_mutant_is_a_kill(op: &str, mutant: &BuiltinMutant, depth: usize) {
    let Ok(kernel) = fsl_core::lower_direct_spec(mutant.spec.clone()) else {
        return; // failing to build is itself a valid "kill" per the brief
    };
    let Ok(model) = build_model(kernel) else {
        return;
    };
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("create solver");
    let bmc = engines::block_on(fsl_verifier::verify_bounded(&model, &mut solver, depth))
        .unwrap_or_else(|error| panic!("bmc run on '{op}' mutant errored: {error}"));
    assert!(
        bmc.violation.is_some(),
        "mutant '{op}' is equivalent (still verifies clean) -- reselect a non-equivalent \
         instance and record the new selection basis"
    );
}

/// A dedicated fixture for the `type_bound_hi_plus1` kill-control, distinct
/// from `r5_source`: `r5_source`'s action recomputes its wraparound modulus
/// as `hi + 1` fresh from the *hand-edited* source text (see its negative
/// control above, which calls `r5_source` directly with a new `hi`), but
/// `enumerate_builtin_mutants` only rewrites the `type` item's literal bound
/// -- it does not know the action body's `% size` literal was derived from
/// that same bound, so mutating `r5_source(2, 2)` in place leaves the
/// action wrapping at the *old* modulus and the mutant is equivalent
/// (measured, not assumed: this was the first candidate tried, and
/// `assert_builtin_mutant_is_a_kill` caught it -- see the git history of
/// this file). This fixture's action has no bound-matching literal at all
/// (an unguarded `x = x + 1`), so widening the type's `hi` is the only
/// thing that changes whether `x` can reach a value the stale invariant
/// forbids.
fn r5_mutate_kill_fixture() -> &'static str {
    r"
spec R5MutateKill {
  type D = 0..2
  state { x: D }
  init { x = 0 }
  action advance() {
    x = x + 1
  }
  invariant TightBound { x <= 2 }
}
"
}

#[test]
fn mutate_reuse_kill_controls_are_measured_non_equivalent_and_detected() {
    // R5: type_bound_hi_plus1. Measured basis: widening D's hi from 2 to 3
    // lets the unguarded `advance` reach x == 3 within depth 3, violating
    // the invariant's literal (unmutated) bound of 2.
    let r5_spec =
        fsl_syntax::parse_surface_spec(r5_mutate_kill_fixture()).expect("parse R5 fixture");
    let r5_mutants = enumerate_builtin_mutants(&r5_spec);
    let hi_plus1 = r5_mutants
        .iter()
        .find(|mutant| mutant.op == "type_bound_hi_plus1")
        .expect("type_bound_hi_plus1 mutant must exist for R5's single domain type");
    assert_builtin_mutant_is_a_kill("type_bound_hi_plus1", hi_plus1, 3);

    // R6: equality_operator_flip on a requires guard whose flip newly admits
    // a previously-unreachable action. Measured basis: the fixture has
    // exactly one `==`/`!=` comparison inside a mutated scope (action
    // requires/body/ensures; invariants are not mutated by this tool), so
    // the single `equality_operator_flip` candidate is unambiguous. Flipping
    // `x == 1` to `x != 1` makes `risky` enabled from the initial state
    // (x == 0) instead of `safe`'s unconditional identity update, and
    // `risky` sets x = 2, immediately violating NeverTwo.
    let r6_source = r"
spec R6EqualityKill {
  type D = 0..2
  state { x: D }
  init { x = 0 }
  action safe() {
    x = 0
  }
  action risky() {
    requires x == 1
    x = 2
  }
  invariant NeverTwo { x != 2 }
}
";
    let r6_spec = fsl_syntax::parse_surface_spec(r6_source).expect("parse R6 equality fixture");
    let r6_mutants = enumerate_builtin_mutants(&r6_spec);
    let flip = r6_mutants
        .iter()
        .find(|mutant| mutant.op == "equality_operator_flip")
        .expect("equality_operator_flip mutant must exist for R6's requires guard");
    assert_builtin_mutant_is_a_kill("equality_operator_flip", flip, 2);

    // R4: assignment_remove on one of the disjoint assignments. Measured
    // basis: removing `b = 2` leaves `b` unchanged after the step, breaking
    // BothOrNeither ((a == 1) == (b == 2)).
    let r4_spec =
        fsl_syntax::parse_surface_spec(&r4_source("a = 1", "b = 2")).expect("parse R4 fixture");
    let r4_mutants = enumerate_builtin_mutants(&r4_spec);
    let removed = r4_mutants
        .iter()
        .find(|mutant| mutant.op == "assignment_remove")
        .expect("assignment_remove mutant must exist for R4's disjoint assignments");
    assert_builtin_mutant_is_a_kill("assignment_remove", removed, 3);
}
