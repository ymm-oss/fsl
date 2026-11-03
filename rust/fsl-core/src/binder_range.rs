// SPDX-License-Identifier: Apache-2.0

//! Pure compact integer-range plans, not execution, membership or admission rules.

use std::collections::BTreeSet;

use crate::{KernelExpr as Expr, KernelModel, TypeRef};

/// A validated, nonempty closed i64 interval, without enumerating its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InclusiveI64 {
    lo: i64,
    hi: i64,
}

impl InclusiveI64 {
    /// Construct an interval, or return `None` for reversed endpoints.
    #[must_use]
    pub const fn new(lo: i64, hi: i64) -> Option<Self> {
        if lo <= hi {
            Some(Self { lo, hi })
        } else {
            None
        }
    }

    /// The inclusive lower endpoint.
    #[must_use]
    pub const fn lo(&self) -> i64 {
        self.lo
    }

    /// The inclusive upper endpoint.
    #[must_use]
    pub const fn hi(&self) -> i64 {
        self.hi
    }

    /// Exact envelope cardinality, always `Some` for a validated interval.
    ///
    /// The full i64 interval contains 2^64 values. This is not an execution cap
    /// or the number of members selected by dynamic endpoint expressions.
    #[must_use]
    pub fn checked_cardinality(&self) -> Option<u128> {
        u128::try_from(i128::from(self.hi) - i128::from(self.lo))
            .ok()?
            .checked_add(1)
    }
}

/// A compact lexical-value envelope; its captured validity belongs to the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegerEnvelopeFact {
    envelope: InclusiveI64,
}

impl IntegerEnvelopeFact {
    /// Wrap a validated interval without changing a value's semantic type.
    #[must_use]
    pub const fn new(envelope: InclusiveI64) -> Self {
        Self { envelope }
    }

    /// The supplied envelope, without intersecting the semantic carrier.
    #[must_use]
    pub const fn envelope(&self) -> InclusiveI64 {
        self.envelope
    }
}

/// An enclosing lexical binding. The last matching input entry wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeBinding<'a> {
    /// Presence shadows state and constants, even when `fact` is absent.
    pub name: &'a str,
    /// Existing semantic type information, not an inferred interval.
    pub semantic_ty: &'a TypeRef,
    /// A separate captured value fact, conditional on its owner's validity.
    pub fact: Option<&'a IntegerEnvelopeFact>,
}

/// Eligibility that a later caller must establish, never candidate membership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RangePrerequisite {
    /// This call's read-frame state must satisfy its declared numeric carrier.
    StateCarrier {
        /// The actual state name, owned independently of the model.
        name: String,
        /// The declared finite carrier used to derive this envelope.
        envelope: InclusiveI64,
    },
    /// Validity belongs to the captured lexical input, not a namesake state.
    LexicalFact {
        /// Original index in the supplied slice, including shadowed entries.
        input_index: usize,
    },
}

/// First-occurrence lo-then-hi dependency order, deduplicated by owner identity.
pub type RangePrerequisites = Vec<RangePrerequisite>;

/// A planning limitation, not a runtime partial operation or a check rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeUnknown {
    /// Invalid/noninteger raw expression or unsupported dynamic constructor.
    UnsupportedExpression,
    /// A typed integer binding has no finite value fact.
    MissingFiniteFact,
    /// Checked conservative interval arithmetic cannot represent the envelope.
    EnvelopeUnrepresentable,
    /// The existing checked integer constant owner returned an error.
    ConstantFoldFailed,
}

/// A pure range description. No variant evaluates endpoints or establishes premises.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RangePlan {
    /// Dependency-free folded endpoints; reversed endpoints remain reversed.
    Static {
        /// Folded lower endpoint.
        lo: i64,
        /// Folded upper endpoint.
        hi: i64,
    },
    /// A conservative compact candidate interval with retained eligibility.
    Finite {
        /// Contains every actual candidate when prerequisites hold.
        candidates: InclusiveI64,
        /// Caller eligibility, never a conjunction added to membership.
        prerequisites: RangePrerequisites,
    },
    /// Empty only under these premises; actual endpoint status is still required.
    Empty {
        /// Preserved even if no candidate would be emitted.
        prerequisites: RangePrerequisites,
    },
    /// Explicit planning limitation with no successful partial plan.
    Unknown(RangeUnknown),
}

struct Planner<'model, 'fact> {
    model: &'model KernelModel,
    lexical: &'model [RangeBinding<'fact>],
    types: Vec<(String, TypeRef)>,
    prerequisites: RangePrerequisites,
}

impl<'model, 'fact> Planner<'model, 'fact> {
    fn new(model: &'model KernelModel, lexical: &'model [RangeBinding<'fact>]) -> Self {
        Self {
            model,
            lexical,
            types: lexical
                .iter()
                .map(|binding| (binding.name.to_owned(), binding.semantic_ty.clone()))
                .collect(),
            prerequisites: Vec::new(),
        }
    }

    fn endpoint(&mut self, expr: &Expr) -> Result<InclusiveI64, RangeUnknown> {
        // Result inference alone misses invalid conditions/dead arms. Reuse the
        // existing full validator, once per endpoint before abstract evaluation.
        crate::typecheck::validate_planner_integer_endpoint(expr, &self.types, self.model)
            .map_err(|_| RangeUnknown::UnsupportedExpression)?;
        self.abstract_integer(expr)
    }

    fn lexical_binding(&self, name: &str) -> Option<(usize, RangeBinding<'fact>)> {
        self.lexical
            .iter()
            .copied()
            .enumerate()
            .rfind(|(_, binding)| binding.name == name)
    }

    fn is_constant(&self, expr: &Expr) -> bool {
        let mut dependent = false;
        crate::model::visit_free_names(expr, &BTreeSet::new(), &mut |name| {
            if self.lexical_binding(name).is_some()
                || self.model.state.iter().any(|(state, _)| state == name)
            {
                dependent = true;
            }
        });
        !dependent
    }

    fn numeric_type(&self, ty: &TypeRef) -> Result<TypeRef, RangeUnknown> {
        match crate::typecheck::resolve(self.model, ty)
            .map_err(|_| RangeUnknown::UnsupportedExpression)?
        {
            numeric @ (TypeRef::Int | TypeRef::Range(_, _)) => Ok(numeric),
            _ => Err(RangeUnknown::UnsupportedExpression),
        }
    }

    fn require(&mut self, prerequisite: RangePrerequisite) {
        // A state name/input index has one fixed envelope within this pure call.
        // Structural equality therefore deduplicates owners without reordering.
        if !self.prerequisites.contains(&prerequisite) {
            self.prerequisites.push(prerequisite);
        }
    }

    fn variable(&mut self, name: &str) -> Result<InclusiveI64, RangeUnknown> {
        if let Some((input_index, binding)) = self.lexical_binding(name) {
            self.numeric_type(binding.semantic_ty)?;
            let envelope = binding
                .fact
                .ok_or(RangeUnknown::MissingFiniteFact)?
                .envelope();
            self.require(RangePrerequisite::LexicalFact { input_index });
            return Ok(envelope);
        }
        let (_, ty) = self
            .model
            .state
            .iter()
            .find(|(state, _)| state == name)
            .ok_or(RangeUnknown::UnsupportedExpression)?;
        let TypeRef::Range(lo, hi) = self.numeric_type(ty)? else {
            return Err(RangeUnknown::MissingFiniteFact);
        };
        let envelope = InclusiveI64::new(lo, hi).ok_or(RangeUnknown::MissingFiniteFact)?;
        self.require(RangePrerequisite::StateCarrier {
            name: name.to_owned(),
            envelope,
        });
        Ok(envelope)
    }

    fn abstract_integer(&mut self, expr: &Expr) -> Result<InclusiveI64, RangeUnknown> {
        crate::recursion::guard(|| {
            if self.is_constant(expr) {
                let value = self
                    .model
                    .constant_integer(expr)
                    .map_err(|_| RangeUnknown::ConstantFoldFailed)?;
                return Ok(InclusiveI64 {
                    lo: value,
                    hi: value,
                });
            }
            match expr {
                Expr::Var(name) => self.variable(name),
                Expr::Neg(inner) => {
                    let interval = self.abstract_integer(inner)?;
                    let lo = interval.hi.checked_neg();
                    let hi = interval.lo.checked_neg();
                    lo.zip(hi)
                        .and_then(|(lo, hi)| InclusiveI64::new(lo, hi))
                        .ok_or(RangeUnknown::EnvelopeUnrepresentable)
                }
                Expr::Binary { op, left, right } if matches!(op.as_str(), "+" | "-" | "*") => {
                    let left = self.abstract_integer(left)?;
                    let right = self.abstract_integer(right)?;
                    binary_interval(op, left, right).ok_or(RangeUnknown::EnvelopeUnrepresentable)
                }
                // Unsupported dynamic constructors fail here, before children.
                _ => Err(RangeUnknown::UnsupportedExpression),
            }
        })
    }
}

fn binary_interval(op: &str, left: InclusiveI64, right: InclusiveI64) -> Option<InclusiveI64> {
    let (lo, hi) = match op {
        "+" => (
            left.lo.checked_add(right.lo)?,
            left.hi.checked_add(right.hi)?,
        ),
        "-" => (
            left.lo.checked_sub(right.hi)?,
            left.hi.checked_sub(right.lo)?,
        ),
        "*" => {
            let corners = [
                left.lo.checked_mul(right.lo)?,
                left.lo.checked_mul(right.hi)?,
                left.hi.checked_mul(right.lo)?,
                left.hi.checked_mul(right.hi)?,
            ];
            (*corners.iter().min()?, *corners.iter().max()?)
        }
        _ => return None,
    };
    InclusiveI64::new(lo, hi)
}

pub(crate) fn plan(
    model: &KernelModel,
    lo: &Expr,
    hi: &Expr,
    lexical: &[RangeBinding<'_>],
) -> RangePlan {
    let mut planner = Planner::new(model, lexical);
    let endpoints = (|| {
        let lo = planner.endpoint(lo)?;
        let hi = planner.endpoint(hi)?;
        Ok::<_, RangeUnknown>((lo.lo, hi.hi))
    })();
    match endpoints {
        Err(reason) => RangePlan::Unknown(reason),
        Ok((lo, hi)) if planner.prerequisites.is_empty() => RangePlan::Static { lo, hi },
        Ok((lo, hi)) => match InclusiveI64::new(lo, hi) {
            Some(candidates) => RangePlan::Finite {
                candidates,
                prerequisites: planner.prerequisites,
            },
            None => RangePlan::Empty {
                prerequisites: planner.prerequisites,
            },
        },
    }
}

impl RangePlan {
    /// The one execution-eligibility rule for an integer range binder.
    ///
    /// Checked admission and the symbolic engines both ask this question. They
    /// differ only in how `established` answers: admission from what the scope
    /// statically provides, an engine from the facts of the current evaluation.
    #[must_use]
    pub fn is_executable(&self, established: impl FnMut(&RangePrerequisite) -> bool) -> bool {
        match self {
            Self::Static { .. } => true,
            Self::Finite { prerequisites, .. } | Self::Empty { prerequisites } => {
                prerequisites.iter().all(established)
            }
            Self::Unknown(_) => false,
        }
    }

    /// The envelope holding every candidate, or `None` when none can exist.
    #[must_use]
    pub const fn candidates(&self) -> Option<InclusiveI64> {
        match self {
            Self::Static { lo, hi } => InclusiveI64::new(*lo, *hi),
            Self::Finite { candidates, .. } => Some(*candidates),
            Self::Empty { .. } | Self::Unknown(_) => None,
        }
    }
}

/// Select an init condition from a checked constant fold alone, never from state.
#[must_use]
pub fn constant_condition(model: &KernelModel, condition: &Expr) -> Option<bool> {
    let position = fsl_syntax::SourcePos {
        offset: 0,
        line: 1,
        column: 1,
    };
    let span = fsl_syntax::Span {
        start: position,
        end: position,
    };
    let selector = Expr::Conditional {
        condition: Box::new(condition.clone()),
        then_expr: Box::new(Expr::Num(1)),
        else_expr: Box::new(Expr::Num(0)),
        spans: Box::new(fsl_syntax::ConditionalSpans {
            condition: span,
            then_expr: span,
            else_expr: span,
        }),
    };
    model
        .constant_integer(&selector)
        .ok()
        .map(|value| value == 1)
}

/// Whether a finite target carrier holds both endpoints of an init value envelope.
#[must_use]
pub fn init_envelope_fits(model: &KernelModel, ty: &TypeRef, envelope: InclusiveI64) -> bool {
    matches!(ty, TypeRef::Int)
        || [envelope.lo(), envelope.hi()].into_iter().all(|value| {
            model
                .domain_contains(ty, &crate::FslValue::Int(value))
                .is_ok_and(|contains| contains)
        })
}

/// The envelope of an init value, from unconditional lexical facts and no state carrier.
///
/// `lexical` is `None` when an enclosing binding has no semantic type. `known`
/// selects a conditional arm exactly as the caller's init evaluation would.
#[must_use]
pub fn init_expression_envelope(
    model: &KernelModel,
    expr: &Expr,
    lexical: Option<&[RangeBinding<'_>]>,
    known: &mut dyn FnMut(&Expr) -> Option<bool>,
) -> Option<InclusiveI64> {
    crate::recursion::guard(|| match expr {
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            if let Some(selected) = known(condition) {
                let branch = if selected { then_expr } else { else_expr };
                return init_expression_envelope(model, branch, lexical, known);
            }
            let left = init_expression_envelope(model, then_expr, lexical, known)?;
            let right = init_expression_envelope(model, else_expr, lexical, known)?;
            InclusiveI64::new(left.lo().min(right.lo()), left.hi().max(right.hi()))
        }
        Expr::Neg(inner) => {
            let envelope =
                IntegerEnvelopeFact::new(init_expression_envelope(model, inner, lexical, known)?);
            let expression = Expr::Neg(Box::new(Expr::Var("value".into())));
            let operand = [RangeBinding {
                name: "value",
                semantic_ty: &TypeRef::Int,
                fact: Some(&envelope),
            }];
            match model.plan_integer_range(&expression, &expression, &operand) {
                RangePlan::Finite { candidates, .. } => Some(candidates),
                _ => None,
            }
        }
        Expr::Binary { op, left, right } => {
            let left = init_expression_envelope(model, left, lexical, known)?;
            let right = init_expression_envelope(model, right, lexical, known)?;
            // Exact operands reuse the constant owner, including checked / and %.
            if left.lo() == left.hi() && right.lo() == right.hi() {
                let folded = Expr::Binary {
                    op: op.clone(),
                    left: Box::new(Expr::Num(left.lo())),
                    right: Box::new(Expr::Num(right.lo())),
                };
                let value = model.constant_integer(&folded).ok()?;
                return InclusiveI64::new(value, value);
            }
            // Reuse the Core interval arithmetic for actual RHS operand envelopes.
            // These lexical inputs exist only in this pure planning expression.
            let expression = Expr::Binary {
                op: op.clone(),
                left: Box::new(Expr::Var("left".into())),
                right: Box::new(Expr::Var("right".into())),
            };
            let left = IntegerEnvelopeFact::new(left);
            let right = IntegerEnvelopeFact::new(right);
            let operands = [
                RangeBinding {
                    name: "left",
                    semantic_ty: &TypeRef::Int,
                    fact: Some(&left),
                },
                RangeBinding {
                    name: "right",
                    semantic_ty: &TypeRef::Int,
                    fact: Some(&right),
                },
            ];
            match model.plan_integer_range(&expression, &expression, &operands) {
                RangePlan::Finite { candidates, .. } => Some(candidates),
                _ => None,
            }
        }
        _ => match model.plan_integer_range(expr, expr, lexical?) {
            RangePlan::Static { lo, hi } => InclusiveI64::new(lo, hi),
            RangePlan::Finite {
                candidates,
                prerequisites,
            } if prerequisites
                .iter()
                .all(|p| matches!(p, RangePrerequisite::LexicalFact { .. })) =>
            {
                Some(candidates)
            }
            _ => None,
        },
    })
}
