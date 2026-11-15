// SPDX-License-Identifier: Apache-2.0
//! #1873: every scoped-query caller keeps its `unknown` fold, its solver call
//! sequence, and the error it reports when the closing `pop` fails too.

use fsl_core::{FsResolver, build_model, parse_kernel_source};
use fsl_solver::{
    CheckFuture, ModelValue, SolverError, SolverResult, Sort, VerificationStatistics,
};

use super::*;
use crate::induction::{partial_witness, rank_measure_path_is_reachable};
use crate::vacuity::proven_unsat;

const ASSERT_FAILED: &str = "stub assert failed";
const CHECK_FAILED: &str = "stub check failed";
const POP_FAILED: &str = "stub pop failed";

// An outcome-routing stub, not an SMT evaluator: terms carry only their sort,
// and every check gives the one scripted answer.
struct Stub {
    answer: SolverResult<SatResult>,
    fail_assert: bool,
    fail_pop: bool,
    calls: Vec<&'static str>,
}

macro_rules! binary_terms {
    ($($name:ident => $sort:ident),* $(,)?) => {
        $(fn $name(&self, _: &Sort, _: &Sort) -> SolverResult<Sort> {
            Ok(Sort::$sort)
        })*
    };
}

impl SmtSolver for Stub {
    type Term = Sort;

    fn version(&self) -> &'static str {
        "scoped-query-stub"
    }
    fn set_query_context(&mut self, _: &str, _: &str) {}
    fn statistics(&self) -> VerificationStatistics {
        VerificationStatistics::default()
    }
    fn sort(&self, term: &Sort) -> Sort {
        term.clone()
    }
    fn literal_value(&self, _: &Sort) -> Option<ModelValue> {
        None
    }
    fn bool_value(&self, _: bool) -> Sort {
        Sort::Bool
    }
    fn int_value(&self, _: i64) -> Sort {
        Sort::Int
    }
    fn constant(&self, _: &str, sort: &Sort) -> SolverResult<Sort> {
        Ok(sort.clone())
    }
    fn not(&self, _: &Sort) -> SolverResult<Sort> {
        Ok(Sort::Bool)
    }
    fn and(&self, _: &[Sort]) -> SolverResult<Sort> {
        Ok(Sort::Bool)
    }
    fn or(&self, _: &[Sort]) -> SolverResult<Sort> {
        Ok(Sort::Bool)
    }
    binary_terms! {
        implies => Bool, equal => Bool,
        add => Int, sub => Int, mul => Int, div => Int, modulo => Int,
        lt => Bool, le => Bool, gt => Bool, ge => Bool,
    }
    fn ite(&self, _: &Sort, then_term: &Sort, _: &Sort) -> SolverResult<Sort> {
        Ok(then_term.clone())
    }
    fn neg(&self, _: &Sort) -> SolverResult<Sort> {
        Ok(Sort::Int)
    }
    fn const_array(&self, domain: &Sort, value: &Sort) -> SolverResult<Sort> {
        Ok(Sort::array(domain.clone(), value.clone()))
    }
    fn select(&self, _: &Sort, _: &Sort) -> SolverResult<Sort> {
        panic!("unexpected select");
    }
    fn store(&self, array: &Sort, _: &Sort, _: &Sort) -> SolverResult<Sort> {
        Ok(array.clone())
    }
    fn substitute(&self, term: &Sort, _: &[(Sort, Sort)]) -> SolverResult<Sort> {
        Ok(term.clone())
    }
    fn push(&mut self) {
        self.calls.push("push");
    }
    fn pop(&mut self, levels: u32) -> SolverResult<()> {
        assert_eq!(levels, 1, "a scoped query pops its own scope only");
        self.calls.push("pop");
        if self.fail_pop {
            return Err(SolverError::new(POP_FAILED));
        }
        Ok(())
    }
    fn reset(&mut self) -> SolverResult<()> {
        panic!("unexpected reset");
    }
    fn assert(&mut self, _: &Sort) -> SolverResult<()> {
        self.calls.push("assert");
        if self.fail_assert {
            return Err(SolverError::new(ASSERT_FAILED));
        }
        Ok(())
    }
    fn assert_and_track(&mut self, _: &Sort, _: &Sort) -> SolverResult<()> {
        panic!("unexpected tracked assertion");
    }
    fn check(&mut self) -> CheckFuture<'_> {
        self.calls.push("check");
        let answer = self.answer.clone();
        Box::pin(async move { answer })
    }
    fn check_assuming(&mut self, _: &[Sort]) -> CheckFuture<'_> {
        panic!("unexpected check_assuming");
    }
    fn unsat_core(&self) -> SolverResult<Vec<Sort>> {
        panic!("unexpected unsat_core");
    }
    fn model_eval(&self, _: &Sort) -> SolverResult<Option<ModelValue>> {
        panic!("an unknown answer has no model to read");
    }
}

#[derive(Clone, Copy, Debug)]
enum Caller {
    Probe,
    ProbeNot,
    Entailed,
    TermsAreUnsat,
    ProvenUnsat,
    PartialWitness,
    BuildWitness,
    RankMeasurePath,
}

const CALLERS: [Caller; 8] = [
    Caller::Probe,
    Caller::ProbeNot,
    Caller::Entailed,
    Caller::TermsAreUnsat,
    Caller::ProvenUnsat,
    Caller::PartialWitness,
    Caller::BuildWitness,
    Caller::RankMeasurePath,
];

#[derive(Clone, Copy, Debug)]
enum Fault {
    None,
    Pop,
    AssertAndPop,
    CheckAndPop,
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("stub solver yielded Pending"),
    }
}

fn model() -> KernelModel {
    let source = "spec Scoped { state { x: Int } init { x = 0 } action tick() { x = x + 1 } }";
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse"))
        .expect("build model")
}

/// The caller's verdict as a Boolean: `partial_witness` answers whether a
/// witness was projected, `build_witness` that one was.
fn run(caller: Caller, solver: &mut Stub) -> Result<bool, VerifyError> {
    let term = Sort::Bool;
    match caller {
        Caller::Probe => block_on(probe(solver, &term)),
        Caller::ProbeNot => block_on(probe_not(solver, &term)),
        Caller::Entailed => block_on(entailed(solver, &[term.clone(), term])),
        Caller::TermsAreUnsat => {
            let constraints = ["first", "second"].map(|name| StaticReachableConstraint {
                blocker: ReachableBlocker {
                    kind: "invariant".to_owned(),
                    name: name.to_owned(),
                },
                term: Sort::Bool,
            });
            block_on(terms_are_unsat(solver, &constraints, &[1, 0], &term))
        }
        Caller::ProvenUnsat => block_on(proven_unsat(solver, &[term.clone(), term])),
        Caller::PartialWitness => {
            let model = model();
            block_on(partial_witness(solver, &model, &term, &[], &[], &[], 0))
                .map(|witness| witness.is_some())
        }
        Caller::BuildWitness => {
            let model = model();
            block_on(build_witness(solver, &model, &term, &[], &[], &[], 0)).map(|_| true)
        }
        Caller::RankMeasurePath => block_on(rank_measure_path_is_reachable(solver, &term)),
    }
}

fn asserted_terms(caller: Caller) -> usize {
    match caller {
        Caller::Probe
        | Caller::ProbeNot
        | Caller::Entailed
        | Caller::PartialWitness
        | Caller::BuildWitness
        | Caller::RankMeasurePath => 1,
        Caller::ProvenUnsat => 2,
        Caller::TermsAreUnsat => 3,
    }
}

fn calls(caller: Caller, fault: Fault) -> Vec<&'static str> {
    let mut calls = vec!["push"];
    if matches!(fault, Fault::AssertAndPop) {
        calls.extend(["assert", "pop"]);
    } else {
        calls.extend(std::iter::repeat_n("assert", asserted_terms(caller)));
        calls.extend(["check", "pop"]);
    }
    calls
}

/// The stub's scripted answer, and the result the caller returned for it
/// before #1873, read from the caller's own body.
fn scripted(caller: Caller, fault: Fault) -> (SolverResult<SatResult>, Result<bool, VerifyError>) {
    let answer = match fault {
        Fault::CheckAndPop => Err(SolverError::new(CHECK_FAILED)),
        _ => Ok(SatResult::Unknown),
    };
    let unknown = match caller {
        Caller::Probe | Caller::ProbeNot => Err(VerifyError::solver("solver returned unknown")),
        Caller::PartialWitness => Err(VerifyError::solver("solver returned unknown in induction")),
        Caller::BuildWitness => Err(VerifyError::solver("solver returned unknown for witness")),
        Caller::RankMeasurePath => Err(VerifyError::solver(
            "solver returned unknown in ranking proof",
        )),
        Caller::Entailed | Caller::TermsAreUnsat | Caller::ProvenUnsat => Ok(false),
    };
    let pop_hides_assert = !matches!(caller, Caller::TermsAreUnsat);
    let pop_hides_check = matches!(
        caller,
        Caller::PartialWitness | Caller::BuildWitness | Caller::RankMeasurePath
    );
    let pop = Err(VerifyError::solver(POP_FAILED));
    let result = match fault {
        Fault::None => unknown,
        Fault::Pop => pop,
        Fault::AssertAndPop if pop_hides_assert => pop,
        Fault::AssertAndPop => Err(VerifyError::solver(ASSERT_FAILED)),
        Fault::CheckAndPop if pop_hides_check => pop,
        Fault::CheckAndPop => Err(VerifyError::solver(CHECK_FAILED)),
    };
    (answer, result)
}

#[test]
fn every_scoped_query_caller_keeps_its_unknown_fold_and_pop_order() {
    for caller in CALLERS {
        for fault in [
            Fault::None,
            Fault::Pop,
            Fault::AssertAndPop,
            Fault::CheckAndPop,
        ] {
            let (answer, expected) = scripted(caller, fault);
            let mut solver = Stub {
                answer,
                fail_assert: matches!(fault, Fault::AssertAndPop),
                fail_pop: !matches!(fault, Fault::None),
                calls: Vec::new(),
            };
            let produced = run(caller, &mut solver);
            assert_eq!(produced, expected, "{caller:?} under {fault:?}");
            assert_eq!(
                solver.calls,
                calls(caller, fault),
                "{caller:?} under {fault:?}"
            );
        }
    }
}
