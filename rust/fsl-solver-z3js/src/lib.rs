// SPDX-License-Identifier: Apache-2.0

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::needless_pass_by_value,
    clippy::unused_self
)]

//! Browser implementation of the FSL SMT contract over an initialized
//! `z3-solver` npm bridge installed on the current Web Worker global.

use fsl_solver::{CheckFuture, ModelValue, SatResult, SmtSolver, SolverError, SolverResult, Sort};
use js_sys::{Array, Promise, Uint32Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Sort)]
    fn js_sort(descriptor: &str) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3BoolValue)]
    fn js_bool_value(value: bool) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3IntValue)]
    fn js_int_value(value: i64) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Constant)]
    fn js_constant(name: &str, sort: u32) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Unary)]
    fn js_unary(operation: &str, term: u32) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Binary)]
    fn js_binary(operation: &str, left: u32, right: u32) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Nary)]
    fn js_nary(operation: &str, terms: &Uint32Array) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Ite)]
    fn js_ite(condition: u32, then_term: u32, else_term: u32) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3ConstArray)]
    fn js_const_array(domain: u32, value: u32) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Substitute)]
    fn js_substitute(term: u32, from: &Uint32Array, to: &Uint32Array) -> u32;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Push)]
    fn js_push();
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Pop)]
    fn js_pop(levels: u32);
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Assert)]
    fn js_assert(term: u32);
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3AssertAndTrack)]
    fn js_assert_and_track(term: u32, tracker: u32);
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3Check)]
    fn js_check(assumptions: &Uint32Array) -> Promise;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3UnsatCore)]
    fn js_unsat_core() -> Array;
    #[wasm_bindgen(js_namespace = globalThis, js_name = fslZ3ModelEval)]
    fn js_model_eval(term: u32, boolean: bool) -> JsValue;
}

#[derive(Clone, Debug)]
pub struct Z3JsTerm {
    handle: u32,
    sort: Sort,
}

#[derive(Debug)]
pub struct Z3JsSolver {
    version: String,
    stack_depth: u32,
}

impl Z3JsSolver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: "Z3 4.16.0 (z3-solver npm)".to_owned(),
            stack_depth: 0,
        }
    }

    fn term(handle: u32, sort: Sort) -> Z3JsTerm {
        Z3JsTerm { handle, sort }
    }
}

impl Default for Z3JsSolver {
    fn default() -> Self {
        Self::new()
    }
}

fn descriptor(sort: &Sort) -> String {
    match sort {
        Sort::Bool => "bool".to_owned(),
        Sort::Int => "int".to_owned(),
        Sort::Array { domain, range } => {
            format!("array({},{})", descriptor(domain), descriptor(range))
        }
    }
}

fn handles(terms: &[Z3JsTerm]) -> Uint32Array {
    let values = terms.iter().map(|term| term.handle).collect::<Vec<_>>();
    Uint32Array::from(values.as_slice())
}

fn same_sort(left: &Z3JsTerm, right: &Z3JsTerm) -> SolverResult<Sort> {
    if left.sort == right.sort {
        Ok(left.sort.clone())
    } else {
        Err(SolverError::new("SMT term sort mismatch"))
    }
}

fn expect(term: &Z3JsTerm, sort: Sort) -> SolverResult<()> {
    if term.sort == sort {
        Ok(())
    } else {
        Err(SolverError::new("SMT term sort mismatch"))
    }
}

fn js_error(value: JsValue) -> SolverError {
    SolverError::new(
        value
            .as_string()
            .unwrap_or_else(|| "z3-solver JavaScript bridge failure".to_owned()),
    )
}

impl SmtSolver for Z3JsSolver {
    type Term = Z3JsTerm;

    fn version(&self) -> &str {
        &self.version
    }

    fn sort(&self, term: &Self::Term) -> Sort {
        term.sort.clone()
    }

    fn bool_value(&self, value: bool) -> Self::Term {
        Self::term(js_bool_value(value), Sort::Bool)
    }

    fn int_value(&self, value: i64) -> Self::Term {
        Self::term(js_int_value(value), Sort::Int)
    }

    fn constant(&self, name: &str, sort: &Sort) -> SolverResult<Self::Term> {
        Ok(Self::term(
            js_constant(name, js_sort(&descriptor(sort))),
            sort.clone(),
        ))
    }

    fn not(&self, term: &Self::Term) -> SolverResult<Self::Term> {
        expect(term, Sort::Bool)?;
        Ok(Self::term(js_unary("not", term.handle), Sort::Bool))
    }

    fn and(&self, terms: &[Self::Term]) -> SolverResult<Self::Term> {
        for term in terms {
            expect(term, Sort::Bool)?;
        }
        Ok(Self::term(js_nary("and", &handles(terms)), Sort::Bool))
    }

    fn or(&self, terms: &[Self::Term]) -> SolverResult<Self::Term> {
        for term in terms {
            expect(term, Sort::Bool)?;
        }
        Ok(Self::term(js_nary("or", &handles(terms)), Sort::Bool))
    }

    fn implies(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        expect(left, Sort::Bool)?;
        expect(right, Sort::Bool)?;
        Ok(Self::term(
            js_binary("implies", left.handle, right.handle),
            Sort::Bool,
        ))
    }

    fn equal(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        same_sort(left, right)?;
        Ok(Self::term(
            js_binary("eq", left.handle, right.handle),
            Sort::Bool,
        ))
    }

    fn ite(
        &self,
        condition: &Self::Term,
        then_term: &Self::Term,
        else_term: &Self::Term,
    ) -> SolverResult<Self::Term> {
        expect(condition, Sort::Bool)?;
        let sort = same_sort(then_term, else_term)?;
        Ok(Self::term(
            js_ite(condition.handle, then_term.handle, else_term.handle),
            sort,
        ))
    }

    fn neg(&self, term: &Self::Term) -> SolverResult<Self::Term> {
        expect(term, Sort::Int)?;
        Ok(Self::term(js_unary("neg", term.handle), Sort::Int))
    }

    fn add(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.int_binary("add", left, right)
    }
    fn sub(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.int_binary("sub", left, right)
    }
    fn mul(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.int_binary("mul", left, right)
    }
    fn div(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.int_binary("div", left, right)
    }
    fn modulo(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.int_binary("mod", left, right)
    }
    fn lt(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.compare("lt", left, right)
    }
    fn le(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.compare("le", left, right)
    }
    fn gt(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.compare("gt", left, right)
    }
    fn ge(&self, left: &Self::Term, right: &Self::Term) -> SolverResult<Self::Term> {
        self.compare("ge", left, right)
    }

    fn const_array(&self, domain: &Sort, value: &Self::Term) -> SolverResult<Self::Term> {
        let sort = Sort::array(domain.clone(), value.sort.clone());
        Ok(Self::term(
            js_const_array(js_sort(&descriptor(domain)), value.handle),
            sort,
        ))
    }

    fn select(&self, array: &Self::Term, index: &Self::Term) -> SolverResult<Self::Term> {
        let Sort::Array { domain, range } = &array.sort else {
            return Err(SolverError::new("expected array SMT term"));
        };
        if domain.as_ref() != &index.sort {
            return Err(SolverError::new("array index sort mismatch"));
        }
        Ok(Self::term(
            js_binary("select", array.handle, index.handle),
            range.as_ref().clone(),
        ))
    }

    fn store(
        &self,
        array: &Self::Term,
        index: &Self::Term,
        value: &Self::Term,
    ) -> SolverResult<Self::Term> {
        let Sort::Array { domain, range } = &array.sort else {
            return Err(SolverError::new("expected array SMT term"));
        };
        if domain.as_ref() != &index.sort || range.as_ref() != &value.sort {
            return Err(SolverError::new("array store sort mismatch"));
        }
        let arguments = Uint32Array::from([array.handle, index.handle, value.handle].as_slice());
        Ok(Self::term(js_nary("store", &arguments), array.sort.clone()))
    }

    fn substitute(
        &self,
        term: &Self::Term,
        replacements: &[(Self::Term, Self::Term)],
    ) -> SolverResult<Self::Term> {
        for (from, to) in replacements {
            same_sort(from, to)?;
        }
        let from = replacements
            .iter()
            .map(|pair| pair.0.handle)
            .collect::<Vec<_>>();
        let to = replacements
            .iter()
            .map(|pair| pair.1.handle)
            .collect::<Vec<_>>();
        Ok(Self::term(
            js_substitute(
                term.handle,
                &Uint32Array::from(from.as_slice()),
                &Uint32Array::from(to.as_slice()),
            ),
            term.sort.clone(),
        ))
    }

    fn push(&mut self) {
        js_push();
        self.stack_depth += 1;
    }

    fn pop(&mut self, levels: u32) -> SolverResult<()> {
        if levels > self.stack_depth {
            return Err(SolverError::new("solver pop exceeds stack depth"));
        }
        js_pop(levels);
        self.stack_depth -= levels;
        Ok(())
    }

    fn assert(&mut self, term: &Self::Term) -> SolverResult<()> {
        expect(term, Sort::Bool)?;
        js_assert(term.handle);
        Ok(())
    }

    fn assert_and_track(&mut self, term: &Self::Term, tracker: &Self::Term) -> SolverResult<()> {
        expect(term, Sort::Bool)?;
        expect(tracker, Sort::Bool)?;
        js_assert_and_track(term.handle, tracker.handle);
        Ok(())
    }

    fn check(&mut self) -> CheckFuture<'_> {
        Box::pin(async move { map_check(js_check(&Uint32Array::new_with_length(0))).await })
    }

    fn check_assuming(&mut self, assumptions: &[Self::Term]) -> CheckFuture<'_> {
        let assumptions = handles(assumptions);
        Box::pin(async move { map_check(js_check(&assumptions)).await })
    }

    fn unsat_core(&self) -> SolverResult<Vec<Self::Term>> {
        Ok(js_unsat_core()
            .iter()
            .filter_map(|value| value.as_f64())
            .map(|handle| Self::term(handle as u32, Sort::Bool))
            .collect())
    }

    fn model_eval(&self, term: &Self::Term) -> SolverResult<Option<ModelValue>> {
        let value = js_model_eval(term.handle, term.sort == Sort::Bool);
        match term.sort {
            Sort::Bool => value
                .as_bool()
                .map(ModelValue::Bool)
                .map(Some)
                .ok_or_else(|| SolverError::new("model returned non-Boolean value")),
            Sort::Int => value
                .as_f64()
                .map(|value| ModelValue::Int(value as i64))
                .map(Some)
                .ok_or_else(|| SolverError::new("model returned non-integer value")),
            Sort::Array { .. } => Err(SolverError::new(
                "project array elements with select before model evaluation",
            )),
        }
    }
}

impl Z3JsSolver {
    fn int_binary(
        &self,
        operation: &str,
        left: &Z3JsTerm,
        right: &Z3JsTerm,
    ) -> SolverResult<Z3JsTerm> {
        expect(left, Sort::Int)?;
        expect(right, Sort::Int)?;
        Ok(Self::term(
            js_binary(operation, left.handle, right.handle),
            Sort::Int,
        ))
    }

    fn compare(
        &self,
        operation: &str,
        left: &Z3JsTerm,
        right: &Z3JsTerm,
    ) -> SolverResult<Z3JsTerm> {
        expect(left, Sort::Int)?;
        expect(right, Sort::Int)?;
        Ok(Self::term(
            js_binary(operation, left.handle, right.handle),
            Sort::Bool,
        ))
    }
}

async fn map_check(promise: Promise) -> SolverResult<SatResult> {
    match JsFuture::from(promise)
        .await
        .map_err(js_error)?
        .as_string()
        .as_deref()
    {
        Some("sat") => Ok(SatResult::Sat),
        Some("unsat") => Ok(SatResult::Unsat),
        Some("unknown") => Ok(SatResult::Unknown),
        _ => Err(SolverError::new("invalid z3-solver check result")),
    }
}
