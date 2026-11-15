// SPDX-License-Identifier: Apache-2.0

//! The one owner of a scoped satisfiability query (#1873): push, assert,
//! check, pop, with the scope popped on every path. The answer comes back as
//! the solver gave it; each caller folds `unknown` itself through
//! [`OnUnknown`].

use fsl_solver::{SatResult, SmtSolver};

use crate::VerifyError;

/// Which error a scoped query returns when its closing `pop` fails too. The
/// scope is popped on every path; only the reported error differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PopFailure {
    /// A failed `pop` is reported over a failed assert, but not over a failed
    /// check.
    OverAssert,
    /// A failed `pop` is reported only when nothing else failed.
    Last,
    /// A failed `pop` is reported over every other failure.
    First,
}

/// What a solver `unknown` means to the caller of a scoped query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OnUnknown {
    /// The caller needs a decided answer: fail with this solver-failure
    /// message.
    Fail(&'static str),
    /// The caller asked for a proof of unsatisfiability, and `unknown` is not
    /// one: it reads the same as `sat`.
    NotProved,
}

impl OnUnknown {
    /// Whether `result` proves the query unsatisfiable.
    pub(crate) fn proves_unsat(self, result: SatResult) -> Result<bool, VerifyError> {
        match result {
            SatResult::Unsat => Ok(true),
            SatResult::Sat => Ok(false),
            SatResult::Unknown => match self {
                Self::Fail(message) => Err(VerifyError::solver(message)),
                Self::NotProved => Ok(false),
            },
        }
    }
}

/// Check `terms` in a fresh scope and return the solver's answer unchanged.
pub(crate) async fn scoped_check<'t, S>(
    solver: &mut S,
    terms: impl IntoIterator<Item = &'t S::Term>,
    pop_failure: PopFailure,
) -> Result<SatResult, VerifyError>
where
    S: SmtSolver,
    S::Term: 't,
{
    scoped_query(solver, terms, pop_failure, |_, result| Ok(result)).await
}

/// Check `terms` in a fresh scope and read the answer with `inside` while the
/// scope, and so the model of a `sat` answer, is still open.
pub(crate) async fn scoped_query<'t, S, T>(
    solver: &mut S,
    terms: impl IntoIterator<Item = &'t S::Term>,
    pop_failure: PopFailure,
    inside: impl FnOnce(&S, SatResult) -> Result<T, VerifyError>,
) -> Result<T, VerifyError>
where
    S: SmtSolver,
    S::Term: 't,
{
    solver.push();
    for term in terms {
        if let Err(error) = solver.assert(term) {
            let popped = solver.pop(1);
            if pop_failure != PopFailure::Last {
                popped?;
            }
            return Err(error.into());
        }
    }
    let answered = match solver.check().await {
        Ok(result) => inside(solver, result),
        Err(error) => Err(error.into()),
    };
    let popped = solver.pop(1);
    if pop_failure == PopFailure::First {
        popped?;
        return answered;
    }
    let answered = answered?;
    popped?;
    Ok(answered)
}
