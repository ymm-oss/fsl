// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The checks and lowering of next-edition decision tables (fsl 5 plan §3
//! S7, design B.3–B.5, decisions/048 J3 and J10).
//!
//! fsl-syntax folds each `decide` into its events' guards and keeps the table
//! as a [`DecisionTable`]. Here, at check time and over the types' domains
//! (not the reachable states), the rows must cover every value of the fields
//! they read (`decide_not_exhaustive`, with the uncovered values), no two rows
//! may hold together (`decide_overlap`, with the shared values), and a row
//! that holds for no value is a warning (`decide_row_emptied`). `assuming Q`
//! narrows the domain to the values where `Q` holds and adds the invariant
//! `stage S ⇒ Q`, which `verify` proves.
//!
//! The domains are enumerated here, so the language server and the browser
//! Worker report the same diagnostics as the CLI. A table whose fields'
//! domain product exceeds [`crate::DOMAIN_BOUND`], or whose condition reads
//! something other than the entity's fields, constants and enum members, is
//! not enumerated: [`crate::solver_obligations`] hands it to the native
//! `check`, which asks the solver.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fsl_syntax::{
    Annotation, Annotations, Binder, DecisionRow, DecisionTable, Expr, ProcessField, QualifiedName,
    RequirementsItem, Span, SpecItem, SurfaceRequirements,
};
use serde_json::{Value, json};

use crate::{
    CoreError, CoreErrorKind, FslValue, KernelModel, LoweringStep, OriginChain, OriginId,
    OriginSite,
};

/// No row of a decision table holds for some value of the fields.
pub const DECIDE_NOT_EXHAUSTIVE: &str = "decide_not_exhaustive";
/// Two rows of a decision table hold for the same value.
pub const DECIDE_OVERLAP: &str = "decide_overlap";
/// Warning: a row of a decision table holds for no value.
pub const DECIDE_ROW_EMPTIED: &str = "decide_row_emptied";

use crate::finite_domain::{Assignment, display, domains, enumerate, evaluate, names};

/// One proof the solver discharges for a table [`crate::DOMAIN_BOUND`] keeps from
/// enumeration: `property` is an invariant over the whole state that holds
/// exactly when the table passes that check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionObligation {
    /// [`DECIDE_NOT_EXHAUSTIVE`] or [`DECIDE_OVERLAP`].
    pub code: &'static str,
    /// The diagnostic's message when the solver finds a counterexample.
    pub message: String,
    /// Why the table was not enumerated.
    pub reason: String,
    pub property: Expr,
    /// The states the counterexample is reported with.
    pub states: Vec<String>,
    pub span: Span,
}

/// The decision tables of one requirements document, with each entity's
/// fields.
pub(crate) struct Tables {
    tables: Vec<DecisionTable>,
    fields: BTreeMap<String, Vec<ProcessField>>,
}

/// The result of enumerating one table.
enum Verdict {
    Checked {
        /// The first value no row covers.
        gap: Option<Assignment>,
        /// The first value two rows (by index) share.
        overlap: Option<(usize, usize, Assignment)>,
        /// The rows that hold for no value.
        empty: Vec<usize>,
        /// The constants the conditions read.
        constants: BTreeMap<String, FslValue>,
    },
    Deferred(String),
}

fn binary(op: &str, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: op.to_owned(),
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn located(code: &'static str, message: String, span: Span) -> CoreError {
    CoreError {
        message,
        line: span.start.line,
        column: span.start.column,
        origin: None,
        kind: CoreErrorKind::Semantics,
        diagnostic_code: Some(code),
    }
}

impl Tables {
    pub(crate) fn of(requirements: &SurfaceRequirements) -> Self {
        let tables = requirements
            .next
            .as_ref()
            .map(|next| next.decisions.clone())
            .unwrap_or_default();
        let fields = requirements
            .items
            .iter()
            .filter_map(|item| match item {
                RequirementsItem::Process(fsl_syntax::BusinessItem::Process {
                    name,
                    fields,
                    ..
                }) => Some((
                    name.name().to_owned(),
                    fields
                        .as_ref()
                        .map(|fields| fields.fields.clone())
                        .unwrap_or_default(),
                )),
                _ => None,
            })
            .collect();
        Self { tables, fields }
    }

    /// `condition` over the kernel's state, for one instance `binder` of
    /// `entity`.
    fn kernel_condition(&self, entity: &str, condition: Expr) -> Expr {
        let binder = crate::naming::entity_binder(entity);
        let replacements = self
            .fields
            .get(entity)
            .into_iter()
            .flatten()
            .map(|field| {
                (
                    format!("{entity}.{}", field.name),
                    Expr::Index(
                        Box::new(Expr::Var(crate::naming::process_field_state(
                            entity,
                            &field.name,
                        ))),
                        Box::new(Expr::Var(binder.clone())),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();
        crate::substitute_expr(condition, &replacements)
    }

    /// `forall b: Entity { entity_stage[b] == Stage [and Q] => body }`.
    fn at_stage(&self, table: &DecisionTable, body: Expr) -> Expr {
        let binder = crate::naming::entity_binder(&table.entity);
        let mut antecedent = binary(
            "==",
            Expr::Index(
                Box::new(Expr::Var(crate::naming::process_stage_state(&table.entity))),
                Box::new(Expr::Var(binder.clone())),
            ),
            Expr::Var(table.stage.clone()),
        );
        if let Some(assuming) = &table.assuming {
            antecedent = binary(
                "and",
                antecedent,
                self.kernel_condition(&table.entity, assuming.clone()),
            );
        }
        Expr::Quantified {
            quantifier: "forall".to_owned(),
            binder: Binder::Typed {
                name: binder,
                type_name: QualifiedName {
                    namespace: None,
                    name: table.entity.clone(),
                },
                where_expr: None,
            },
            body: Box::new(binary("=>", antecedent, body)),
        }
    }

    /// The invariant each `assuming Q` adds: `stage S ⇒ Q`, with its
    /// requirement annotation and origin.
    pub(crate) fn assumption_invariants(&self) -> Vec<(SpecItem, Annotation, OriginChain)> {
        self.tables
            .iter()
            .filter_map(|table| {
                let assuming = table.assuming.as_ref()?;
                let name = crate::naming::decide_assumption_invariant(&table.id);
                let mut origin = table_origin(table, &format!("decide:{name}"));
                origin.lowering_steps.push(LoweringStep {
                    kind: "assuming".to_owned(),
                    detail: Some(format!("{} {} {}", table.id, table.stage, table.entity)),
                });
                let expression = self.at_stage(
                    &DecisionTable {
                        assuming: None,
                        ..table.clone()
                    },
                    self.kernel_condition(&table.entity, assuming.clone()),
                );
                Some((
                    SpecItem::Invariant {
                        name,
                        expr: Box::new(expression),
                        span: table.span,
                        meta: None,
                        annotations: Annotations::default(),
                    },
                    Annotation::Requirement {
                        id: table.id.clone(),
                        text: Some(table.text.clone()),
                        span: table.span,
                    },
                    origin,
                ))
            })
            .collect()
    }

    /// The `decide` lowering steps of the action lowered from `event` of
    /// `entity`: one per row that names it.
    pub(crate) fn lowering_steps(&self, entity: &str, event: &str) -> Vec<LoweringStep> {
        self.tables
            .iter()
            .filter(|table| table.entity == entity)
            .flat_map(|table| {
                table
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| row.events.iter().any(|named| named == event))
                    .map(|(index, row)| LoweringStep {
                        kind: "decide".to_owned(),
                        detail: Some(format!(
                            "{} {} {} row {}: {}",
                            table.id,
                            table.stage,
                            table.entity,
                            index + 1,
                            row.source
                        )),
                    })
            })
            .collect()
    }

    /// The exhaustiveness, overlap and empty-row checks of every table that
    /// can be enumerated, returning the warnings.
    ///
    /// # Errors
    ///
    /// Returns [`DECIDE_NOT_EXHAUSTIVE`] or [`DECIDE_OVERLAP`] for the first
    /// table that fails.
    pub(crate) fn check(&self, model: &KernelModel) -> Result<Vec<Value>, CoreError> {
        let mut warnings = Vec::new();
        for table in &self.tables {
            let Verdict::Checked {
                gap,
                overlap,
                empty,
                constants,
            } = self.verdict(table, model)
            else {
                continue;
            };
            let at = |assignment: &Assignment| {
                assignment
                    .iter()
                    .map(|(name, value)| (name.as_str(), value))
                    .map(|(name, value)| {
                        let name = name
                            .strip_prefix(&format!("{}.", table.entity))
                            .unwrap_or(name);
                        format!("{name} = {}", display(value))
                    })
                    .chain(
                        constants
                            .iter()
                            .map(|(name, value)| format!("{name} = {}", display(value))),
                    )
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            if let Some(assignment) = gap {
                return Err(located(
                    DECIDE_NOT_EXHAUSTIVE,
                    format!(
                        "{}: no row holds at {}; add a row, close the table with `otherwise`, \
                         or state what holds at {} with `assuming`",
                        table_label(table),
                        at(&assignment),
                        table.stage
                    ),
                    table.span,
                ));
            }
            if let Some((first, second, assignment)) = overlap {
                return Err(located(
                    DECIDE_OVERLAP,
                    format!(
                        "{}: {} and {} both hold at {}; rows must not overlap (name both \
                         outcomes in one row with `or` if either may handle it)",
                        table_label(table),
                        row_label(&table.rows[first], first),
                        row_label(&table.rows[second], second),
                        at(&assignment)
                    ),
                    table.rows[second].span,
                ));
            }
            for index in empty {
                let row = &table.rows[index];
                warnings.push(json!({
                    "kind": DECIDE_ROW_EMPTIED,
                    "code": DECIDE_ROW_EMPTIED,
                    "severity": "warning",
                    "message": format!(
                        "{}: {} holds for no value{}, so its events never take this row",
                        table_label(table),
                        row_label(row, index),
                        if table.assuming.is_some() { " that `assuming` admits" } else { "" }
                    ),
                    "loc": row.span.python_loc(),
                }));
            }
        }
        Ok(warnings)
    }

    /// Whether the document has no table.
    pub(crate) fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// The tables [`Self::check`] did not enumerate, as solver obligations.
    pub(crate) fn obligations(&self, model: &KernelModel) -> Vec<DecisionObligation> {
        let mut obligations = Vec::new();
        for table in &self.tables {
            let Verdict::Deferred(reason) = self.verdict(table, model) else {
                continue;
            };
            let mut states = vec![crate::naming::process_stage_state(&table.entity)];
            states.extend(
                self.fields
                    .get(&table.entity)
                    .into_iter()
                    .flatten()
                    .map(|field| crate::naming::process_field_state(&table.entity, &field.name)),
            );
            let rows = table
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| !row.otherwise)
                .collect::<Vec<_>>();
            if rows.len() == table.rows.len() {
                let cover = rows
                    .iter()
                    .map(|(_, row)| self.kernel_condition(&table.entity, row.condition.clone()))
                    .reduce(|left, right| binary("or", left, right))
                    .unwrap_or(Expr::Bool(false));
                obligations.push(DecisionObligation {
                    code: DECIDE_NOT_EXHAUSTIVE,
                    message: format!(
                        "{}: no row holds in a state the solver found; add a row, close the \
                         table with `otherwise`, or state what holds at {} with `assuming`",
                        table_label(table),
                        table.stage
                    ),
                    reason: reason.clone(),
                    property: self.at_stage(table, cover),
                    states: states.clone(),
                    span: table.span,
                });
            }
            for (position, (first, first_row)) in rows.iter().enumerate() {
                for (second, second_row) in &rows[position + 1..] {
                    let both = binary(
                        "and",
                        self.kernel_condition(&table.entity, first_row.condition.clone()),
                        self.kernel_condition(&table.entity, second_row.condition.clone()),
                    );
                    obligations.push(DecisionObligation {
                        code: DECIDE_OVERLAP,
                        message: format!(
                            "{}: {} and {} both hold in a state the solver found; rows must \
                             not overlap (name both outcomes in one row with `or` if either \
                             may handle it)",
                            table_label(table),
                            row_label(first_row, *first),
                            row_label(second_row, *second)
                        ),
                        reason: reason.clone(),
                        property: self.at_stage(table, Expr::Not(Box::new(both))),
                        states: states.clone(),
                        span: second_row.span,
                    });
                }
            }
        }
        obligations
    }

    /// Enumerate `table` over the domains of the fields it reads.
    fn verdict(&self, table: &DecisionTable, model: &KernelModel) -> Verdict {
        let prefix = format!("{}.", table.entity);
        let is_field = |name: &str| name.starts_with(&prefix);
        let mut read = BTreeSet::new();
        let mut constants = BTreeMap::new();
        for expression in table
            .rows
            .iter()
            .map(|row| &row.condition)
            .chain(&table.assuming)
        {
            if let Err(reason) = names(expression, &is_field, model, &mut read, &mut constants) {
                return Verdict::Deferred(reason);
            }
        }
        let wanted = self
            .fields
            .get(&table.entity)
            .into_iter()
            .flatten()
            .filter_map(|field| {
                let name = format!("{prefix}{}", field.name);
                read.contains(&name)
                    .then(|| (name, &field.type_name, format!("field '{}'", field.name)))
            })
            .collect::<Vec<_>>();
        let domains = match domains(model, &wanted, "fields") {
            Ok(domains) => domains,
            Err(reason) => return Verdict::Deferred(reason),
        };
        let rows = &table.rows;
        let otherwise = rows.iter().any(|row| row.otherwise);
        let mut gap = None;
        let mut overlap = None;
        let mut taken = vec![false; rows.len()];
        let enumerated = enumerate(&domains, &Assignment::new(), |assignment| {
            let holds = |expression: &Expr| evaluate(expression, assignment, model);
            let admitted = match table.assuming.as_ref().map(holds) {
                None | Some(Ok(FslValue::Bool(true))) => true,
                Some(Ok(FslValue::Bool(false))) => false,
                Some(Ok(_)) => return Err("`assuming` is not Boolean".to_owned()),
                Some(Err(reason)) => return Err(reason),
            };
            if admitted {
                let mut holding = Vec::new();
                for (index, row) in rows.iter().enumerate() {
                    match holds(&row.condition)? {
                        FslValue::Bool(true) => {
                            taken[index] = true;
                            if !row.otherwise {
                                holding.push(index);
                            }
                        }
                        FslValue::Bool(false) => {}
                        _ => {
                            return Err(format!(
                                "the condition of row {} is not Boolean",
                                index + 1
                            ));
                        }
                    }
                }
                if holding.is_empty() && !otherwise && gap.is_none() {
                    gap = Some(assignment.clone());
                }
                if let [first, second, ..] = holding[..]
                    && overlap.is_none()
                {
                    overlap = Some((first, second, assignment.clone()));
                }
            }
            Ok(true)
        });
        if let Err(reason) = enumerated {
            return Verdict::Deferred(reason);
        }
        let empty = taken
            .iter()
            .enumerate()
            .filter(|(_, taken)| !**taken)
            .map(|(index, _)| index)
            .collect();
        Verdict::Checked {
            gap,
            overlap,
            empty,
            constants,
        }
    }
}

/// What the check-time enumeration found for one decision table, for a
/// reader of the document (`fslc document`), which shows each table with its
/// result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionTableResult {
    /// The rows were evaluated at every value of the fields they read.
    Enumerated {
        /// Some row holds at every value (or the table has `otherwise`).
        exhaustive: bool,
        /// The first two rows (by index) that hold at the same value.
        overlap: Option<(usize, usize)>,
        /// The rows that hold for no value.
        empty_rows: Vec<usize>,
    },
    /// The table was not enumerated, for this reason; native `check`
    /// discharges it with the solver.
    Deferred(String),
}

/// The result of enumerating each decision table of `requirements` over
/// `model`, in source order: `(table id, result)`.
#[must_use]
pub fn decision_table_results(
    requirements: &SurfaceRequirements,
    model: &KernelModel,
) -> Vec<(String, DecisionTableResult)> {
    let tables = Tables::of(requirements);
    tables
        .tables
        .iter()
        .map(|table| {
            let result = match tables.verdict(table, model) {
                Verdict::Checked {
                    gap,
                    overlap,
                    empty,
                    ..
                } => DecisionTableResult::Enumerated {
                    exhaustive: gap.is_none(),
                    overlap: overlap.map(|(first, second, _)| (first, second)),
                    empty_rows: empty,
                },
                Verdict::Deferred(reason) => DecisionTableResult::Deferred(reason),
            };
            (table.id.clone(), result)
        })
        .collect()
}

fn table_label(table: &DecisionTable) -> String {
    format!(
        "decision table '{}' for {} {}",
        table.id, table.stage, table.entity
    )
}

fn row_label(row: &DecisionRow, index: usize) -> String {
    format!("row {} `{}`", index + 1, row.source)
}

fn table_origin(table: &DecisionTable, id: &str) -> OriginChain {
    OriginChain {
        id: OriginId(format!("requirements:{id}:{}", table.span.start.offset)),
        dialect: "requirements".to_owned(),
        primary: Some(OriginSite {
            source_file: None,
            span: Some(table.span),
            dialect: "requirements".to_owned(),
            declaration_path: vec![table.id.clone()],
        }),
        secondary: Vec::new(),
        lowering_steps: Vec::new(),
        generated: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finite_domain::DOMAIN_BOUND;

    fn span() -> Span {
        let position = fsl_syntax::SourcePos {
            offset: 0,
            line: 1,
            column: 1,
        };
        Span {
            start: position,
            end: position,
        }
    }

    fn table(rows: Vec<(&str, Expr)>, otherwise: bool) -> DecisionTable {
        let mut rows = rows
            .into_iter()
            .map(|(source, condition)| DecisionRow {
                source: source.to_owned(),
                condition,
                events: vec!["e".to_owned()],
                otherwise: false,
                span: span(),
            })
            .collect::<Vec<_>>();
        if otherwise {
            rows.push(DecisionRow {
                source: "otherwise => A v".to_owned(),
                condition: Expr::Not(Box::new(rows[0].condition.clone())),
                events: vec!["e".to_owned()],
                otherwise: true,
                span: span(),
            });
        }
        DecisionTable {
            id: "T".to_owned(),
            text: "t".to_owned(),
            entity: "C".to_owned(),
            stage: "S".to_owned(),
            rows,
            assuming: None,
            span: span(),
        }
    }

    fn model(range: (i64, i64)) -> KernelModel {
        let source = format!(
            "spec M {{\n  type C = 0..0\n  type A = {}..{}\n  const L = 1\n  enum CStage {{ S }}\n  \
             state {{ c_stage: Map<C, CStage>, c_a: Map<C, A> }}\n  init {{ forall c: C {{ \
             c_stage[c] = S\n c_a[c] = {} }} }}\n}}\n",
            range.0, range.1, range.0
        );
        crate::build_model(
            crate::parse_kernel_source(&source, &crate::FsResolver::new(".")).expect("kernel"),
        )
        .expect("model")
    }

    fn tables(table: DecisionTable) -> Tables {
        Tables {
            tables: vec![table],
            fields: BTreeMap::from([(
                "C".to_owned(),
                vec![ProcessField {
                    name: "a".to_owned(),
                    type_name: QualifiedName {
                        namespace: None,
                        name: "A".to_owned(),
                    },
                    initial: None,
                    span: span(),
                    type_span: span(),
                }],
            )]),
        }
    }

    fn field() -> Expr {
        Expr::Var("C.a".to_owned())
    }

    fn limit() -> Expr {
        Expr::Var("L".to_owned())
    }

    #[test]
    fn the_first_uncovered_and_shared_values_are_reported() {
        let gap = tables(table(
            vec![
                ("Small", binary("<", field(), limit())),
                ("Large", binary(">", field(), limit())),
            ],
            false,
        ));
        let Verdict::Checked { gap, overlap, .. } = gap.verdict(&gap.tables[0], &model((0, 3)))
        else {
            panic!("enumerated");
        };
        assert_eq!(
            gap,
            Some(BTreeMap::from([("C.a".to_owned(), FslValue::Int(1))]))
        );
        assert_eq!(overlap, None);

        let shared = tables(table(
            vec![
                ("Small", binary("<=", field(), limit())),
                ("Large", binary(">=", field(), limit())),
            ],
            false,
        ));
        let Verdict::Checked { gap, overlap, .. } =
            shared.verdict(&shared.tables[0], &model((0, 3)))
        else {
            panic!("enumerated");
        };
        assert_eq!(gap, None);
        assert_eq!(
            overlap,
            Some((0, 1, BTreeMap::from([("C.a".to_owned(), FslValue::Int(1))])))
        );
    }

    #[test]
    fn otherwise_closes_the_gap_and_an_impossible_row_is_empty() {
        let closed = tables(table(vec![("Small", binary("<", field(), limit()))], true));
        let Verdict::Checked { gap, empty, .. } = closed.verdict(&closed.tables[0], &model((0, 3)))
        else {
            panic!("enumerated");
        };
        assert_eq!((gap, empty), (None, Vec::new()));

        let impossible = tables(table(
            vec![
                ("Any", Expr::Bool(true)),
                ("Never", binary(">", field(), Expr::Num(9))),
            ],
            false,
        ));
        let Verdict::Checked { empty, .. } =
            impossible.verdict(&impossible.tables[0], &model((0, 3)))
        else {
            panic!("enumerated");
        };
        assert_eq!(empty, vec![1]);
    }

    /// decisions/048 J3: past the bound the table goes to the solver.
    #[test]
    fn a_domain_past_the_bound_is_deferred_and_one_at_it_is_not() {
        let rows = vec![("Any", binary(">=", field(), Expr::Num(0)))];
        let at = tables(table(rows.clone(), false));
        let bound = i64::try_from(DOMAIN_BOUND).expect("bound fits");
        assert!(matches!(
            at.verdict(&at.tables[0], &model((1, bound))),
            Verdict::Checked { .. }
        ));
        let past = tables(table(rows, false));
        let Verdict::Deferred(reason) = past.verdict(&past.tables[0], &model((0, bound))) else {
            panic!("deferred");
        };
        assert!(reason.contains("1000001 values"), "{reason}");
    }
}
