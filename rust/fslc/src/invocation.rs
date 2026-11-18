// SPDX-License-Identifier: Apache-2.0

//! The state of one command invocation: the dependency findings of its open
//! [`DependencyScope`], the files it read as inputs, and the requirements
//! questions it has asked. A thread works in an invocation only when one is
//! handed to it ([`Handoff`]); otherwise the first scope it enters starts one.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use fsl_core::KernelModel;
use fslc_rust::migration::{DependencyRecords, finish_dependency_reports};
use fslc_rust::requirements_solver::Rejection;
use serde_json::Value;

use crate::output_publication::FileIdentity;

thread_local! {
    static CURRENT: RefCell<Option<Current>> = const { RefCell::new(None) };
}

/// What this thread works in: an invocation and the dependency scope open in it.
#[derive(Clone)]
struct Current {
    invocation: Arc<Invocation>,
    dependencies: Option<DependencyRecords>,
}

#[derive(Default)]
struct Invocation {
    // Recorded only for the process's command (`Handoff::command`): in-process
    // unit tests that run several commands do not refuse each other's inputs.
    inputs: Option<Mutex<BTreeSet<FileIdentity>>>,
    asked: Mutex<Vec<Question>>,
}

/// A requirements question -- a source and the model it is asked of -- and
/// its answer, set once by the first thread that asks it.
type Question = (String, KernelModel, Arc<OnceLock<Option<Rejection>>>);

fn current() -> Option<Current> {
    CURRENT.with(|slot| slot.borrow().clone())
}

/// The records of the dependency scope open on this thread, for a resolver
/// that reports into it.
pub(crate) fn dependencies() -> Option<DependencyRecords> {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|current| current.dependencies.clone())
    })
}

/// Apply `f` to the invocation's recorded inputs; `None` when it records none.
pub(crate) fn with_inputs<T>(f: impl FnOnce(&mut BTreeSet<FileIdentity>) -> T) -> Option<T> {
    let current = current()?;
    let inputs = current.invocation.inputs.as_ref()?;
    Some(f(&mut inputs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)))
}

/// The answer to a requirements question, from `solve` the first time this
/// invocation asks it. Only threads asking the same question wait for the
/// solver; without an invocation nothing is kept.
pub(crate) fn ask(
    source: &str,
    model: &KernelModel,
    solve: impl FnOnce() -> Option<Rejection>,
) -> Option<Rejection> {
    let Some(current) = current() else {
        return solve();
    };
    let answer = {
        let mut asked = current
            .invocation
            .asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((_, _, answer)) = asked
            .iter()
            .find(|(asked_source, asked_model, _)| asked_source == source && asked_model == model)
        {
            Arc::clone(answer)
        } else {
            let answer = Arc::default();
            asked.push((source.to_owned(), model.clone(), Arc::clone(&answer)));
            answer
        }
    };
    answer.get_or_init(solve).clone()
}

/// Native owners borrow a nested invocation's log; only its outer owner delivers warnings.
pub(crate) struct DependencyScope {
    log: DependencyRecords,
    outer: bool,
    // The scope started this thread's invocation, so its end ends that too.
    started: bool,
}

impl DependencyScope {
    #[must_use]
    pub(crate) fn enter() -> Self {
        CURRENT.with(|slot| {
            let mut slot = slot.borrow_mut();
            let started = slot.is_none();
            let current = slot.get_or_insert_with(|| Current {
                invocation: Arc::default(),
                dependencies: None,
            });
            let outer = current.dependencies.is_none();
            let log = current
                .dependencies
                .get_or_insert_with(DependencyRecords::default)
                .clone();
            Self {
                log,
                outer,
                started,
            }
        })
    }

    #[must_use]
    pub(crate) fn finish(&self, result: (Value, i32), stderr: bool) -> (Value, i32) {
        if let Some(refusal) = &self.log.lock().refusal {
            return refusal.clone();
        }
        if !self.outer {
            return result;
        }
        finish_dependency_reports(&self.log, result, stderr)
    }

    /// A distinct artifact or baseline needs its own diagnostics even when nested.
    #[must_use]
    pub(crate) fn finish_distinct(&self, result: (Value, i32)) -> (Value, i32) {
        self.log.lock().finish(result)
    }
}

impl Drop for DependencyScope {
    fn drop(&mut self) {
        if self.outer {
            CURRENT.with(|slot| {
                let mut slot = slot.borrow_mut();
                if self.started {
                    *slot = None;
                } else if let Some(current) = slot.as_mut() {
                    current.dependencies = None;
                }
            });
        }
    }
}

/// This thread's invocation, for another thread to work in.
pub(crate) struct Handoff(Option<Current>);

impl Handoff {
    /// The process's command: an invocation that records the files it reads
    /// as inputs, so no output replaces one.
    pub(crate) fn command() -> Self {
        Self(Some(Current {
            invocation: Arc::new(Invocation {
                inputs: Some(Mutex::default()),
                ..Invocation::default()
            }),
            dependencies: None,
        }))
    }

    /// This thread's invocation with its open dependency scope: a scope
    /// entered on the receiving thread is nested and leaves delivery to the
    /// scope open here.
    pub(crate) fn current() -> Self {
        Self(current())
    }

    /// The invocation without its dependency scope: a scope entered on the
    /// receiving thread is outer and delivers its own findings, and the
    /// thread's resolvers record nothing outside one, as on a thread handed
    /// nothing. Inputs and requirements answers stay the invocation's.
    #[must_use]
    pub(crate) fn outside_scope(self) -> Self {
        Self(self.0.map(|current| Current {
            dependencies: None,
            ..current
        }))
    }

    /// Work in the handed invocation on this thread until the guard drops.
    #[must_use]
    pub(crate) fn enter(self) -> Entered {
        Entered(CURRENT.with(|slot| slot.replace(self.0)))
    }
}

/// Restores what the thread worked in before [`Handoff::enter`].
pub(crate) struct Entered(Option<Current>);

impl Drop for Entered {
    fn drop(&mut self) {
        let previous = self.0.take();
        CURRENT.with(|slot| *slot.borrow_mut() = previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WARNING: &str = "dependency warning";

    fn record_warning(scope: &DependencyScope) {
        scope.log.lock().warnings.push(json!(WARNING));
    }

    /// What a scope entered on a new thread delivers for one recorded warning
    /// after `work`.
    fn delivered_on_thread(handoff: Handoff, work: impl FnOnce() + Send) -> (Value, i32) {
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _invocation = handoff.enter();
                    let dependencies = DependencyScope::enter();
                    record_warning(&dependencies);
                    work();
                    dependencies.finish((json!({"result": "ok"}), 0), false)
                })
                .join()
                .unwrap()
        })
    }

    #[test]
    fn a_scope_handed_to_a_thread_stays_nested_there() {
        let root = DependencyScope::enter();
        assert!(root.outer);
        let on_thread = delivered_on_thread(Handoff::current(), || {});
        assert_eq!(on_thread, (json!({"result": "ok"}), 0));
        assert_eq!(
            root.finish((json!({"result": "ok"}), 0), false),
            (json!({"result": "ok", "warnings": [WARNING]}), 0)
        );
    }

    #[test]
    fn an_outer_handoff_delivers_on_the_thread_and_shares_the_invocation() {
        const SOURCE: &str = "spec S { state { x: Bool } init { x = false } }";
        let kernel = fsl_core::parse_kernel_source(SOURCE, &fsl_core::FsResolver::new("."))
            .expect("parse unit fixture");
        let model = fsl_core::build_model(kernel).expect("build unit fixture");
        let root = DependencyScope::enter();
        let on_thread = delivered_on_thread(Handoff::current().outside_scope(), || {
            assert_eq!(ask(SOURCE, &model, || None), None);
        });
        assert_eq!(
            on_thread,
            (json!({"result": "ok", "warnings": [WARNING]}), 0)
        );
        let asked_again = ask(SOURCE, &model, || unreachable!("answered on the thread"));
        assert_eq!(asked_again, None);
        assert_eq!(
            root.finish((json!({"result": "ok"}), 0), false),
            (json!({"result": "ok"}), 0)
        );
    }
}
