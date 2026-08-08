// SPDX-License-Identifier: Apache-2.0

//! #779 structural control: the "evolve pairing invariant"
//! (`docs/DESIGN-domain.md`'s saga step section) says an `evolve` is 1:1
//! with the *occurrence* of its event: any generated action whose
//! `updates` sets `event_<E> := true` must, in the SAME action, apply E's
//! declared `evolve` assignments if the domain declares one for E.
//! `event_assignments` alone -- without the paired `evolve_items` call --
//! is exactly the defect class #779 fixes: before the fix, the saga
//! step/timeout/compensation actions in `domain_lowering.rs`
//! (`lower_saga_actions`) and `domain.rs` (`render_saga_actions`) called
//! only `event_assignments`, so a saga step could set its emitted event's
//! flag true without ever writing the state the domain declared that event
//! should evolve.
//!
//! This is a total sweep over every action in every corpus fixture, not a
//! fixture spot-check, because #679's PR-A rewrites the saga
//! step/timeout/resolve/compensate actions wholesale. A spot check on
//! today's action shapes would silently stop covering the rewritten code;
//! a sweep keyed only on "does this action set an event flag true" keeps
//! covering whatever actions PR-A produces without a second copy of the
//! judgment rule to keep in sync.
//!
//! Per #779's design-authority ruling, the check function is
//! `fn check(domain: &DomainSpec, model: &KernelModel)` and stays that
//! shape: "E's declared evolve" is a `DomainSpec`-surface concept with no
//! kernel-model equivalent (the kernel has no notion of `evolve` at all),
//! so a hand-written kernel spec with no backing `DomainSpec` cannot be
//! passed to this check -- that is a scope boundary of the input types,
//! not an oversight. The event-flag and state-variable names this check
//! compares against are computed with the same `fsl_core::event_flag` /
//! `fsl_core::state_name` naming functions `domain_lowering.rs` and
//! `domain.rs` themselves use to generate those names, never guessed from
//! an `event_` prefix on a generated kernel name -- prefix-matching a
//! generated name has no `DomainSpec`-side referent to validate against
//! and is explicitly out of scope for this gate.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use fsl_core::{
    FsResolver, KernelModel, build_model, domain_kernel_source, event_flag, lower_domain,
    parse_kernel_source, state_name,
};
use fsl_syntax::{
    DomainSpec, Expr, LValue, Statement, SurfaceDocument, SyntaxLValue, parse_surface_document,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/ directory")
        .parent()
        .expect("repository root")
        .to_path_buf()
}

/// `.fsl` files directly under `dir` (non-recursive), sorted for
/// deterministic test output.
fn fsl_files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "fsl"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

/// The #779 sweep corpus: every `.fsl` file under `examples/domain/` and
/// `rust/fslc/tests/fixtures/domain_characterization/`, plus
/// `examples/annotations/annotated_domain.fsl`. This is a glob, not a
/// manually maintained registration list like
/// `domain_render_agreement.rs`'s `VALID_DOMAIN_FIXTURES` (that list exists
/// for a different purpose: pinning each fixture's *expected* A/B agreement
/// outcome one by one). Per #779, this sweep must keep covering new domain
/// fixtures automatically as they are added, without a second list to
/// remember to update -- files that fail to parse as a `domain` document,
/// or that fail to lower on a given path (several
/// `domain_characterization/` fixtures are deliberately-invalid negative
/// controls for other gates), are skipped for that path rather than
/// failing this test; this sweep only asserts the pairing invariant over
/// successfully produced kernel models.
fn sweep_corpus() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = fsl_files_in(&root.join("examples/domain"));
    files.extend(fsl_files_in(
        &root.join("rust/fslc/tests/fixtures/domain_characterization"),
    ));
    files.push(root.join("examples/annotations/annotated_domain.fsl"));
    files
}

/// Root identifier of a domain-source lvalue (`item` for `item`,
/// `counts[item]`, and `counter.value` alike). Purely syntactic: it never
/// resolves or evaluates the lvalue, so it cannot drift from
/// `resolve_lvalue`'s value semantics; it only needs the same root name
/// that `state_name` combines with the aggregate name.
fn syntax_lvalue_root(lvalue: &SyntaxLValue) -> &str {
    match lvalue {
        SyntaxLValue::Name(ident) => ident.text.as_str(),
        SyntaxLValue::Index { base, .. } | SyntaxLValue::Field { base, .. } => {
            syntax_lvalue_root(base)
        }
    }
}

/// Root variable name of a generated kernel lvalue (`order_status` for
/// `order_status`, `counts[item]`, and `counter.value` alike).
fn kernel_lvalue_root(lvalue: &LValue) -> &str {
    match lvalue {
        LValue::Var(name) | LValue::Index(name, _) => name.as_str(),
        LValue::Field(base, _) => kernel_lvalue_root(base),
    }
}

/// For every event with a non-empty declared `evolve`, the set of kernel
/// state-variable ROOT names its assignments must write, keyed by the
/// event's kernel flag name (`event_flag`). Only the root variable is
/// checked, not the resolved value or the full lvalue path -- this mirrors
/// the granularity the lowering-side pairing invariant actually operates
/// at: whether the paired `evolve_items` call happened in the same action,
/// not what expression value it computed (recomputing that would require
/// re-deriving the resolver's expression semantics, the "second copy of
/// the judgment rule" #779 explicitly says not to build). An event with a
/// declared-but-empty evolve (no assignments, e.g. `evolve Opened {}`) has
/// no required targets and is intentionally not registered here, so it is
/// vacuously satisfied wherever its flag is set.
fn required_targets(domain: &DomainSpec) -> BTreeMap<String, BTreeSet<String>> {
    let mut required = BTreeMap::new();
    for aggregate in &domain.aggregates {
        for evolve in &aggregate.evolves {
            if evolve.assignments.is_empty() {
                continue;
            }
            let targets = evolve
                .assignments
                .iter()
                .map(|assignment| state_name(aggregate, syntax_lvalue_root(&assignment.target)))
                .collect::<BTreeSet<_>>();
            required.insert(event_flag(&evolve.event), targets);
        }
    }
    required
}

/// Collects, from `statements` and any nested `If`/`ForAll` branches: every
/// event flag assigned literal `true` (`occurring`), and the root name of
/// every kernel variable any assignment writes (`written`).
fn collect_writes(
    statements: &[Statement],
    occurring: &mut BTreeSet<String>,
    written: &mut BTreeSet<String>,
) {
    for statement in statements {
        match statement {
            Statement::Assign { target, value, .. } => {
                written.insert(kernel_lvalue_root(target).to_owned());
                if matches!(value, Expr::Bool(true))
                    && let LValue::Var(name) = target
                {
                    occurring.insert(name.clone());
                }
            }
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_writes(then_statements, occurring, written);
                collect_writes(else_statements, occurring, written);
            }
            Statement::ForAll { statements, .. } => {
                collect_writes(statements, occurring, written);
            }
        }
    }
}

/// The #779 pairing-invariant check: `fn check(domain: &DomainSpec, model:
/// &KernelModel)`, fixed at this signature per the design-authority ruling.
/// Returns one human-readable finding per violation -- an action that sets
/// some `event_<E> := true` without writing every kernel variable E's
/// declared evolve requires in that same action; an empty result means the
/// invariant held for every action in `model`.
fn check_evolve_pairing(domain: &DomainSpec, model: &KernelModel) -> Vec<String> {
    let required = required_targets(domain);
    let mut findings = Vec::new();
    for action in &model.actions {
        let mut occurring = BTreeSet::new();
        let mut written = BTreeSet::new();
        collect_writes(&action.statements, &mut occurring, &mut written);
        for flag in &occurring {
            let Some(targets) = required.get(flag) else {
                continue;
            };
            for target in targets {
                if !written.contains(target) {
                    findings.push(format!(
                        "action '{}' sets '{flag} := true' but never writes '{target}' \
                         (its declared evolve is not applied in this action)",
                        action.name
                    ));
                }
            }
        }
    }
    findings
}

/// Runs [`check_evolve_pairing`] against both independent lowering paths
/// (`lower_domain`, and `domain_kernel_source` reparsed through
/// `parse_kernel_source`) for every fixture [`sweep_corpus`] discovers.
#[test]
fn evolve_pairing_holds_for_every_action_in_the_domain_corpus() {
    let mut checked_any_model = false;
    let mut findings = Vec::new();
    for path in sweep_corpus() {
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(SurfaceDocument::Domain(domain)) = parse_surface_document(&source) else {
            continue;
        };

        if let Ok(kernel) = lower_domain(&domain)
            && let Ok(model) = build_model(kernel)
        {
            checked_any_model = true;
            findings.extend(
                check_evolve_pairing(&domain, &model)
                    .into_iter()
                    .map(|finding| format!("{} (path A, lower_domain): {finding}", path.display())),
            );
        }

        if let Ok(text) = domain_kernel_source(&domain)
            && let Ok(kernel) = parse_kernel_source(&text, &FsResolver::new("."))
            && let Ok(model) = build_model(kernel)
        {
            checked_any_model = true;
            findings.extend(
                check_evolve_pairing(&domain, &model)
                    .into_iter()
                    .map(|finding| {
                        format!(
                            "{} (path B, domain_kernel_source): {finding}",
                            path.display()
                        )
                    }),
            );
        }
    }

    assert!(
        checked_any_model,
        "the #779 sweep corpus glob (examples/domain/, \
         rust/fslc/tests/fixtures/domain_characterization/, \
         examples/annotations/annotated_domain.fsl) produced zero lowerable \
         domain kernel models; the glob paths themselves are probably wrong"
    );
    assert!(
        findings.is_empty(),
        "evolve pairing invariant violated in the domain corpus:\n{}",
        findings.join("\n")
    );
}
