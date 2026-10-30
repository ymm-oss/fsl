// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{FslValue, KernelModel, TypeDef, TypeRef};
use fsl_solver::{ModelValue, SatResult, SmtSolver, Sort};

use crate::VerifyError;

pub type SymbolicState<T> = BTreeMap<String, SymbolicValue<T>>;
pub type Bindings<T> = BTreeMap<String, SymbolicValue<T>>;

#[derive(Clone, Debug)]
pub(crate) struct TypedBinding<T>(pub(crate) SymbolicValue<T>, pub(crate) Option<TypeRef>);

impl<T> From<SymbolicValue<T>> for TypedBinding<T> {
    fn from(value: SymbolicValue<T>) -> Self {
        let ty = value.ty().cloned();
        Self(value, ty)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TypedBindings<T>(BTreeMap<String, TypedBinding<T>>);

impl<T> TypedBindings<T> {
    pub(crate) fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub(crate) fn insert(&mut self, name: String, value: impl Into<TypedBinding<T>>) {
        self.0.insert(name, value.into());
    }

    pub(crate) fn get(&self, name: &str) -> Option<&SymbolicValue<T>> {
        self.0.get(name).map(|binding| &binding.0)
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &SymbolicValue<T>)> {
        self.0.iter().map(|(name, binding)| (name, &binding.0))
    }

    pub(crate) fn types(&self) -> impl Iterator<Item = (&String, &TypeRef)> {
        self.0
            .iter()
            .filter_map(|(name, binding)| binding.1.as_ref().map(|ty| (name, ty)))
    }
}

impl<T> FromIterator<(String, SymbolicValue<T>)> for TypedBindings<T> {
    fn from_iter<I: IntoIterator<Item = (String, SymbolicValue<T>)>>(iter: I) -> Self {
        Self(
            iter.into_iter()
                .map(|(name, value)| (name, value.into()))
                .collect(),
        )
    }
}
type MapEntries<T> = Vec<(FslValue, SymbolicValue<T>)>;
pub(crate) type CollectionEntries<T> = Vec<(Vec<T>, T)>;

#[derive(Clone, Debug)]
pub enum SymbolicValue<T> {
    Scalar {
        ty: TypeRef,
        term: T,
    },
    None,
    Option {
        ty: TypeRef,
        present: T,
        value: Box<Self>,
    },
    Struct {
        ty: TypeRef,
        fields: BTreeMap<String, Self>,
    },
    Map {
        ty: TypeRef,
        entries: Vec<(FslValue, Self)>,
    },
    Set {
        ty: TypeRef,
        entries: Vec<(FslValue, T)>,
    },
    Seq {
        ty: TypeRef,
        slots: Vec<Self>,
        len: T,
    },
    Relation {
        ty: TypeRef,
        entries: Vec<((FslValue, FslValue), T)>,
    },
    /// Actual scalar members/pairs, including values outside a declared carrier.
    /// Live entries are distinct; absent slots may contain arbitrary keys.
    /// Relations retain their declared pair grid as the entry prefix.
    Collection {
        ty: TypeRef,
        entries: CollectionEntries<T>,
    },
    SetLiteral(Vec<Self>),
    SeqLiteral(Vec<Self>),
}

impl<T> SymbolicValue<T> {
    pub fn ty(&self) -> Option<&TypeRef> {
        match self {
            Self::Scalar { ty, .. }
            | Self::Option { ty, .. }
            | Self::Struct { ty, .. }
            | Self::Map { ty, .. }
            | Self::Set { ty, .. }
            | Self::Seq { ty, .. }
            | Self::Collection { ty, .. }
            | Self::Relation { ty, .. } => Some(ty),
            Self::None | Self::SetLiteral(_) | Self::SeqLiteral(_) => None,
        }
    }

    fn inferred_collection_element<'a>(&self, target: &'a TypeRef) -> Option<&'a TypeRef> {
        match (self, target) {
            (
                Self::Collection {
                    ty: TypeRef::Set(source),
                    ..
                },
                TypeRef::Set(element),
            )
            | (
                Self::Seq {
                    ty: TypeRef::Seq(source, _),
                    ..
                },
                TypeRef::Seq(element, _),
            ) if **source == TypeRef::Int => Some(element),
            _ => None,
        }
    }
}

pub(crate) fn solver_sort(model: &KernelModel, ty: &TypeRef) -> Result<Sort, VerifyError> {
    match ty {
        TypeRef::Bool => Ok(Sort::Bool),
        TypeRef::Int | TypeRef::Range(_, _) => Ok(Sort::Int),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => Ok(Sort::Int),
            Some(TypeDef::Struct { .. }) => Err(VerifyError::new(format!(
                "struct type '{name}' has no scalar solver sort"
            ))),
            None => Err(VerifyError::new(format!("unknown type '{name}'"))),
        },
        _ => Err(VerifyError::new(
            "collection type has no scalar solver sort",
        )),
    }
}

pub(crate) fn symbolic_state<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    step: usize,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    symbolic_state_with_suffix(solver, model, &step.to_string())
}

pub(crate) fn symbolic_state_with_suffix<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    suffix: &str,
) -> Result<SymbolicState<S::Term>, VerifyError> {
    model
        .state
        .iter()
        .map(|(name, ty)| {
            Ok((
                name.clone(),
                symbolic_value(solver, model, ty, &format!("{name}@{suffix}"))?,
            ))
        })
        .collect()
}

fn symbolic_value<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    ty: &TypeRef,
    name: &str,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match ty {
        TypeRef::Int | TypeRef::Bool | TypeRef::Range(_, _) => Ok(SymbolicValue::Scalar {
            ty: ty.clone(),
            term: solver.constant(name, &solver_sort(model, ty)?)?,
        }),
        TypeRef::Named(type_name) => match model.types.get(type_name) {
            Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => Ok(SymbolicValue::Scalar {
                ty: ty.clone(),
                term: solver.constant(name, &Sort::Int)?,
            }),
            Some(TypeDef::Struct { fields }) => Ok(SymbolicValue::Struct {
                ty: ty.clone(),
                fields: fields
                    .iter()
                    .map(|(field, field_ty)| {
                        Ok((
                            field.clone(),
                            symbolic_value(solver, model, field_ty, &format!("{name}__{field}"))?,
                        ))
                    })
                    .collect::<Result<_, VerifyError>>()?,
            }),
            None => Err(VerifyError::new(format!("unknown type '{type_name}'"))),
        },
        TypeRef::Option(inner) => Ok(SymbolicValue::Option {
            ty: ty.clone(),
            present: solver.constant(&format!("{name}__present"), &Sort::Bool)?,
            value: Box::new(symbolic_value(
                solver,
                model,
                inner,
                &format!("{name}__value"),
            )?),
        }),
        TypeRef::Map(key_ty, value_ty) => Ok(SymbolicValue::Map {
            ty: ty.clone(),
            entries: model
                .map_key_values(key_ty)?
                .into_iter()
                .map(|key| {
                    let suffix = concrete_name(&key);
                    Ok((
                        key,
                        symbolic_value(solver, model, value_ty, &format!("{name}__key_{suffix}"))?,
                    ))
                })
                .collect::<Result<_, VerifyError>>()?,
        }),
        TypeRef::Set(element_ty) => Ok(SymbolicValue::Set {
            ty: ty.clone(),
            entries: model
                .domain_values(element_ty)?
                .into_iter()
                .map(|element| {
                    let suffix = concrete_name(&element);
                    Ok((
                        element,
                        solver.constant(&format!("{name}__elem_{suffix}"), &Sort::Bool)?,
                    ))
                })
                .collect::<Result<_, VerifyError>>()?,
        }),
        TypeRef::Seq(element_ty, capacity) => Ok(SymbolicValue::Seq {
            ty: ty.clone(),
            slots: (0..*capacity)
                .map(|index| {
                    symbolic_value(solver, model, element_ty, &format!("{name}__slot_{index}"))
                })
                .collect::<Result<_, VerifyError>>()?,
            len: solver.constant(&format!("{name}__len"), &Sort::Int)?,
        }),
        TypeRef::Relation(source_ty, target_ty) => Ok(SymbolicValue::Relation {
            ty: ty.clone(),
            entries: model
                .domain_values(source_ty)?
                .into_iter()
                .flat_map(|source| {
                    model
                        .domain_values(target_ty)
                        .unwrap_or_default()
                        .into_iter()
                        .map(move |target| (source.clone(), target))
                })
                .map(|(source, target)| {
                    let suffix = format!("{}_{}", concrete_name(&source), concrete_name(&target));
                    Ok((
                        (source, target),
                        solver.constant(&format!("{name}__edge_{suffix}"), &Sort::Bool)?,
                    ))
                })
                .collect::<Result<_, VerifyError>>()?,
        }),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn concrete_value<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    ty: &TypeRef,
    value: &FslValue,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match (ty, value) {
        (TypeRef::Bool, FslValue::Bool(value)) => Ok(SymbolicValue::Scalar {
            ty: ty.clone(),
            term: solver.bool_value(*value),
        }),
        (TypeRef::Int | TypeRef::Range(_, _) | TypeRef::Named(_), FslValue::Int(value)) => {
            Ok(SymbolicValue::Scalar {
                ty: ty.clone(),
                term: solver.int_value(*value),
            })
        }
        (TypeRef::Named(name), FslValue::Enum { type_name, member }) if name == type_name => {
            let Some(TypeDef::Enum { members, .. }) = model.types.get(name) else {
                return Err(VerifyError::new(format!("'{name}' is not an enum")));
            };
            let index = members
                .iter()
                .position(|candidate| candidate == member)
                .ok_or_else(|| VerifyError::new(format!("unknown enum member '{member}'")))?;
            Ok(SymbolicValue::Scalar {
                ty: ty.clone(),
                term: solver.int_value(
                    i64::try_from(index)
                        .map_err(|_| VerifyError::new("enum member index exceeds i64"))?,
                ),
            })
        }
        (TypeRef::Option(inner), FslValue::None) => Ok(SymbolicValue::Option {
            ty: ty.clone(),
            present: solver.bool_value(false),
            value: Box::new(concrete_value(
                solver,
                model,
                inner,
                &model.default_value(inner)?,
            )?),
        }),
        (TypeRef::Option(inner), FslValue::Some(value)) => Ok(SymbolicValue::Option {
            ty: ty.clone(),
            present: solver.bool_value(true),
            value: Box::new(concrete_value(solver, model, inner, value)?),
        }),
        (TypeRef::Named(name), FslValue::Struct { type_name, fields }) if name == type_name => {
            let Some(TypeDef::Struct { fields: expected }) = model.types.get(name) else {
                return Err(VerifyError::new(format!("'{name}' is not a struct")));
            };
            Ok(SymbolicValue::Struct {
                ty: ty.clone(),
                fields: expected
                    .iter()
                    .map(|(field, field_ty)| {
                        Ok((
                            field.clone(),
                            concrete_value(
                                solver,
                                model,
                                field_ty,
                                fields.get(field).ok_or_else(|| {
                                    VerifyError::new(format!("missing struct field '{field}'"))
                                })?,
                            )?,
                        ))
                    })
                    .collect::<Result<_, VerifyError>>()?,
            })
        }
        (TypeRef::Map(key_ty, value_ty), FslValue::Map(values)) => Ok(SymbolicValue::Map {
            ty: ty.clone(),
            entries: model
                .map_key_values(key_ty)?
                .into_iter()
                .map(|key| {
                    Ok((
                        key.clone(),
                        concrete_value(
                            solver,
                            model,
                            value_ty,
                            values.get(&key).ok_or_else(|| {
                                VerifyError::new("concrete map is missing a finite-domain key")
                            })?,
                        )?,
                    ))
                })
                .collect::<Result<_, VerifyError>>()?,
        }),
        (TypeRef::Set(element_ty), FslValue::Set(values)) => Ok(SymbolicValue::Set {
            ty: ty.clone(),
            entries: model
                .domain_values(element_ty)?
                .into_iter()
                .chain(values.iter().cloned())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|element| {
                    let present = values.contains(&element);
                    Ok((element, solver.bool_value(present)))
                })
                .collect::<Result<_, VerifyError>>()?,
        }),
        (TypeRef::Seq(element_ty, capacity), FslValue::Seq(values)) => {
            let default = model.default_value(element_ty)?;
            Ok(SymbolicValue::Seq {
                ty: ty.clone(),
                slots: (0..(*capacity).max(values.len()))
                    .map(|index| {
                        concrete_value(
                            solver,
                            model,
                            element_ty,
                            values.get(index).unwrap_or(&default),
                        )
                    })
                    .collect::<Result<_, VerifyError>>()?,
                len: solver.int_value(
                    i64::try_from(values.len())
                        .map_err(|_| VerifyError::new("sequence length exceeds i64"))?,
                ),
            })
        }
        (TypeRef::Relation(source_ty, target_ty), FslValue::Relation(values)) => {
            let mut pairs: Vec<_> = model
                .domain_values(source_ty)?
                .into_iter()
                .flat_map(|source| {
                    model
                        .domain_values(target_ty)
                        .unwrap_or_default()
                        .into_iter()
                        .map(move |target| (source.clone(), target))
                })
                .collect();
            let extra: Vec<_> = values
                .iter()
                .filter(|pair| !pairs.contains(pair))
                .cloned()
                .collect();
            pairs.extend(extra);
            Ok(SymbolicValue::Relation {
                ty: ty.clone(),
                entries: pairs
                    .into_iter()
                    .map(|pair| {
                        let present = values.contains(&pair);
                        Ok((pair, solver.bool_value(present)))
                    })
                    .collect::<Result<_, VerifyError>>()?,
            })
        }
        _ => Err(VerifyError::new(format!(
            "concrete value {value:?} does not conform to {ty:?}"
        ))),
    }
}

fn concrete_name(value: &FslValue) -> String {
    match value {
        FslValue::Int(value) => format!("i{value}"),
        FslValue::Bool(value) => format!("b{value}"),
        FslValue::Enum { type_name, member } => format!("{type_name}_{member}"),
        _ => "value".to_owned(),
    }
}

pub(crate) fn bool_term<T>(value: &SymbolicValue<T>) -> Result<&T, VerifyError> {
    match value {
        SymbolicValue::Scalar {
            ty: TypeRef::Bool,
            term,
        } => Ok(term),
        _ => Err(VerifyError::new("expected Boolean symbolic value")),
    }
}

pub(crate) fn int_term<T>(value: &SymbolicValue<T>) -> Result<&T, VerifyError> {
    match value {
        SymbolicValue::Scalar {
            ty: TypeRef::Int | TypeRef::Range(_, _) | TypeRef::Named(_),
            term,
        } => Ok(term),
        _ => Err(VerifyError::new("expected integer symbolic value")),
    }
}

#[allow(clippy::only_used_in_recursion, clippy::too_many_lines)]
pub(crate) fn logical_equal<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    left: &SymbolicValue<S::Term>,
    right: &SymbolicValue<S::Term>,
) -> Result<S::Term, VerifyError> {
    if is_collection(left)
        && is_collection(right)
        && (matches!(left, SymbolicValue::Collection { .. })
            || matches!(right, SymbolicValue::Collection { .. }))
    {
        let (_, left) = collection_entries(solver, model, left)?;
        let (_, right) = collection_entries(solver, model, right)?;
        let mut parts = Vec::new();
        for (source, target) in [(&left, &right), (&right, &left)] {
            for (key, present) in source {
                parts.push(solver.implies(present, &collection_contains(solver, target, key)?)?);
            }
        }
        return Ok(solver.and(&parts)?);
    }
    match (left, right) {
        (SymbolicValue::None, SymbolicValue::None) => Ok(solver.bool_value(true)),
        (SymbolicValue::Option { present, .. }, SymbolicValue::None)
        | (SymbolicValue::None, SymbolicValue::Option { present, .. }) => Ok(solver.not(present)?),
        (
            SymbolicValue::Scalar {
                term: left_term, ..
            },
            SymbolicValue::Scalar {
                term: right_term, ..
            },
        ) => Ok(solver.equal(left_term, right_term)?),
        (
            SymbolicValue::Option {
                present: left_present,
                value: left_value,
                ..
            },
            SymbolicValue::Option {
                present: right_present,
                value: right_value,
                ..
            },
        ) => {
            let same_presence = solver.equal(left_present, right_present)?;
            let same_value = logical_equal(solver, model, left_value, right_value)?;
            let visible_same = solver.implies(left_present, &same_value)?;
            Ok(solver.and(&[same_presence, visible_same])?)
        }
        (
            SymbolicValue::Struct {
                fields: left_fields,
                ..
            },
            SymbolicValue::Struct {
                fields: right_fields,
                ..
            },
        ) => {
            if left_fields.keys().ne(right_fields.keys()) {
                return Err(VerifyError::new("struct shape mismatch"));
            }
            let parts = left_fields
                .iter()
                .map(|(name, value)| logical_equal(solver, model, value, &right_fields[name]))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(solver.and(&parts)?)
        }
        (
            SymbolicValue::Map {
                entries: left_entries,
                ..
            },
            SymbolicValue::Map {
                entries: right_entries,
                ..
            },
        ) => {
            if left_entries.len() != right_entries.len() {
                return Err(VerifyError::new("map shape mismatch"));
            }
            let parts = left_entries
                .iter()
                .zip(right_entries)
                .map(|((left_key, left_value), (right_key, right_value))| {
                    if left_key != right_key {
                        return Err(VerifyError::new("map key domain mismatch"));
                    }
                    logical_equal(solver, model, left_value, right_value)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(solver.and(&parts)?)
        }
        (
            SymbolicValue::Set {
                entries: left_entries,
                ..
            },
            SymbolicValue::Set {
                entries: right_entries,
                ..
            },
        ) => equal_term_entries(solver, left_entries, right_entries),
        (
            SymbolicValue::Relation {
                entries: left_entries,
                ..
            },
            SymbolicValue::Relation {
                entries: right_entries,
                ..
            },
        ) => equal_term_entries(solver, left_entries, right_entries),
        (
            SymbolicValue::Seq {
                slots: left_slots,
                len: left_len,
                ..
            },
            SymbolicValue::Seq {
                slots: right_slots,
                len: right_len,
                ..
            },
        ) => {
            // Equal lengths make the common prefix cover every live element of
            // represented temporaries. State-frame equalities also retain an
            // overflow length so the type-bound check can report the failure.
            let mut parts = vec![solver.equal(left_len, right_len)?];
            for (index, (left_value, right_value)) in left_slots.iter().zip(right_slots).enumerate()
            {
                let visible = solver.lt(&solver.int_value(i64_index(index)?), left_len)?;
                let same = logical_equal(solver, model, left_value, right_value)?;
                parts.push(solver.implies(&visible, &same)?);
            }
            Ok(solver.and(&parts)?)
        }
        _ => Err(VerifyError::new("symbolic value shape mismatch")),
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn ite_value<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    condition: &S::Term,
    then_value: &SymbolicValue<S::Term>,
    else_value: &SymbolicValue<S::Term>,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    if is_collection(then_value)
        && is_collection(else_value)
        && (matches!(then_value, SymbolicValue::Collection { .. })
            || matches!(else_value, SymbolicValue::Collection { .. }))
    {
        let (ty, then_entries) = collection_entries(solver, model, then_value)?;
        let (_, else_entries) = collection_entries(solver, model, else_value)?;
        let mut entries = Vec::new();
        let known: Option<Vec<_>> = then_entries
            .iter()
            .chain(&else_entries)
            .map(|(key, _)| literal_key(solver, key))
            .collect();
        if let Some(keys) = known {
            let mut seen = Vec::new();
            for ((key, _), known) in then_entries.iter().chain(&else_entries).zip(keys) {
                if seen.contains(&known) {
                    continue;
                }
                seen.push(known);
                entries.push((
                    key.clone(),
                    solver.ite(
                        condition,
                        &collection_contains(solver, &then_entries, key)?,
                        &collection_contains(solver, &else_entries, key)?,
                    )?,
                ));
            }
            return Ok(SymbolicValue::Collection { ty, entries });
        }
        // Align slots, not copies of whole candidate lists. Each arm has distinct
        // live members, so selection preserves distinctness and growth is linear.
        for index in 0..then_entries.len().max(else_entries.len()) {
            let then_entry = then_entries.get(index);
            let else_entry = else_entries.get(index);
            let key = match (then_entry, else_entry) {
                (Some((left, _)), Some((right, _))) => left
                    .iter()
                    .zip(right)
                    .map(|(left, right)| solver.ite(condition, left, right))
                    .collect::<Result<_, _>>()?,
                (Some((key, _)), None) | (None, Some((key, _))) => key.clone(),
                (None, None) => unreachable!(),
            };
            let absent = solver.bool_value(false);
            entries.push((
                key,
                solver.ite(
                    condition,
                    then_entry.map_or(&absent, |(_, present)| present),
                    else_entry.map_or(&absent, |(_, present)| present),
                )?,
            ));
        }
        return Ok(SymbolicValue::Collection { ty, entries });
    }
    if matches!(then_value, SymbolicValue::None) {
        let ty = else_value
            .ty()
            .ok_or_else(|| VerifyError::new("if arms have no inferable type"))?;
        return ite_value(
            solver,
            model,
            condition,
            &coerce(solver, model, then_value.clone(), ty)?,
            else_value,
        );
    }
    if matches!(else_value, SymbolicValue::None) {
        let ty = then_value
            .ty()
            .ok_or_else(|| VerifyError::new("if arms have no inferable type"))?;
        return ite_value(
            solver,
            model,
            condition,
            then_value,
            &coerce(solver, model, else_value.clone(), ty)?,
        );
    }
    match (then_value, else_value) {
        (
            SymbolicValue::Scalar { ty, term: then },
            SymbolicValue::Scalar {
                term: otherwise, ..
            },
        ) => Ok(SymbolicValue::Scalar {
            ty: ty.clone(),
            term: solver.ite(condition, then, otherwise)?,
        }),
        (
            SymbolicValue::Option {
                ty,
                present: then_present,
                value: then_inner,
            },
            SymbolicValue::Option {
                present: else_present,
                value: else_inner,
                ..
            },
        ) => Ok(SymbolicValue::Option {
            ty: ty.clone(),
            present: solver.ite(condition, then_present, else_present)?,
            value: Box::new(ite_value(solver, model, condition, then_inner, else_inner)?),
        }),
        (
            SymbolicValue::Struct {
                ty,
                fields: then_fields,
            },
            SymbolicValue::Struct {
                fields: else_fields,
                ..
            },
        ) => Ok(SymbolicValue::Struct {
            ty: ty.clone(),
            fields: then_fields
                .iter()
                .map(|(name, value)| {
                    Ok((
                        name.clone(),
                        ite_value(solver, model, condition, value, &else_fields[name])?,
                    ))
                })
                .collect::<Result<_, VerifyError>>()?,
        }),
        (
            SymbolicValue::Map {
                ty,
                entries: then_entries,
            },
            SymbolicValue::Map {
                entries: else_entries,
                ..
            },
        ) => Ok(SymbolicValue::Map {
            ty: ty.clone(),
            entries: then_entries
                .iter()
                .zip(else_entries)
                .map(|((key, value), (else_key, else_value))| {
                    if key != else_key {
                        return Err(VerifyError::new("map key domain mismatch"));
                    }
                    Ok((
                        key.clone(),
                        ite_value(solver, model, condition, value, else_value)?,
                    ))
                })
                .collect::<Result<_, VerifyError>>()?,
        }),
        (
            SymbolicValue::Set {
                ty,
                entries: then_entries,
            },
            SymbolicValue::Set {
                entries: else_entries,
                ..
            },
        ) => Ok(SymbolicValue::Set {
            ty: ty.clone(),
            entries: ite_term_entries(solver, condition, then_entries, else_entries)?,
        }),
        (
            SymbolicValue::Relation {
                ty,
                entries: then_entries,
            },
            SymbolicValue::Relation {
                entries: else_entries,
                ..
            },
        ) => Ok(SymbolicValue::Relation {
            ty: ty.clone(),
            entries: ite_term_entries(solver, condition, then_entries, else_entries)?,
        }),
        (
            SymbolicValue::Seq {
                ty,
                slots: then_slots,
                len: then_len,
            },
            SymbolicValue::Seq {
                slots: else_slots,
                len: else_len,
                ..
            },
        ) => {
            let TypeRef::Seq(element_ty, _) = ty else {
                unreachable!();
            };
            let default =
                concrete_value(solver, model, element_ty, &model.default_value(element_ty)?)?;
            let slots = (0..then_slots.len().max(else_slots.len()))
                .map(|index| {
                    ite_value(
                        solver,
                        model,
                        condition,
                        then_slots.get(index).unwrap_or(&default),
                        else_slots.get(index).unwrap_or(&default),
                    )
                })
                .collect::<Result<_, VerifyError>>()?;
            Ok(SymbolicValue::Seq {
                ty: ty.clone(),
                slots,
                len: solver.ite(condition, then_len, else_len)?,
            })
        }
        _ => Err(VerifyError::new("if arms have different symbolic shapes")),
    }
}

fn equal_term_entries<S, K>(
    solver: &S,
    left: &[(K, S::Term)],
    right: &[(K, S::Term)],
) -> Result<S::Term, VerifyError>
where
    S: SmtSolver,
    K: PartialEq,
{
    if left.len() != right.len() {
        return Err(VerifyError::new("finite collection shape mismatch"));
    }
    let parts = left
        .iter()
        .map(|(key, term)| Ok(solver.equal(term, term_for_key(right, key)?)?))
        .collect::<Result<Vec<_>, VerifyError>>()?;
    Ok(solver.and(&parts)?)
}

fn ite_term_entries<S, K>(
    solver: &S,
    condition: &S::Term,
    then_entries: &[(K, S::Term)],
    else_entries: &[(K, S::Term)],
) -> Result<Vec<(K, S::Term)>, VerifyError>
where
    S: SmtSolver,
    K: Clone + PartialEq,
{
    if then_entries.len() != else_entries.len() {
        return Err(VerifyError::new("finite collection shape mismatch"));
    }
    then_entries
        .iter()
        .map(|(key, term)| {
            Ok((
                key.clone(),
                solver.ite(condition, term, term_for_key(else_entries, key)?)?,
            ))
        })
        .collect()
}

fn term_for_key<'a, K: PartialEq, T>(entries: &'a [(K, T)], key: &K) -> Result<&'a T, VerifyError> {
    entries
        .iter()
        .find(|(candidate, _)| candidate == key)
        .map(|(_, term)| term)
        .ok_or_else(|| VerifyError::new("finite collection domain mismatch"))
}

fn can_retag<T>(
    value: &SymbolicValue<T>,
    ty: &TypeRef,
    model: &KernelModel,
) -> Result<bool, VerifyError> {
    Ok(value.ty() == Some(ty)
        || matches!(
            ty,
            TypeRef::Set(_) | TypeRef::Seq(_, _) | TypeRef::Map(_, _) | TypeRef::Relation(_, _)
        ) && value
            .ty()
            .map(|actual| model.same_carrier(actual, ty))
            .transpose()?
            == Some(true))
}

pub(crate) fn coerce<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: SymbolicValue<S::Term>,
    target_ty: &TypeRef,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match (value, target_ty) {
        (SymbolicValue::None, TypeRef::Option(inner)) => Ok(SymbolicValue::Option {
            ty: target_ty.clone(),
            present: solver.bool_value(false),
            value: Box::new(concrete_value(
                solver,
                model,
                inner,
                &model.default_value(inner)?,
            )?),
        }),
        (SymbolicValue::Option { present, value, .. }, TypeRef::Option(inner)) => {
            Ok(SymbolicValue::Option {
                ty: target_ty.clone(),
                present,
                value: Box::new(coerce(solver, model, *value, inner)?),
            })
        }
        (SymbolicValue::SetLiteral(items), TypeRef::Set(element_ty)) => {
            if items.is_empty() && !matches!(element_ty.as_ref(), TypeRef::Int) {
                return concrete_value(solver, model, target_ty, &model.default_value(target_ty)?);
            }
            let mut entries = Vec::new();
            for item in items {
                let item = coerce(solver, model, item, element_ty)?;
                let SymbolicValue::Scalar { term, .. } = item else {
                    return Err(VerifyError::new("Set elements must be scalar"));
                };
                let key = vec![term];
                let present = solver.not(&collection_contains(solver, &entries, &key)?)?;
                entries.push((key, present));
            }
            Ok(SymbolicValue::Collection {
                ty: target_ty.clone(),
                entries,
            })
        }
        // `Set {}` is the only relation literal form (public Kernel typing
        // already rejects a non-empty one); coerce it to the all-absent
        // finite pair grid, mirroring `TypeRef::Relation`'s default value.
        (SymbolicValue::SetLiteral(items), TypeRef::Relation(source_ty, target_element_ty)) => {
            if !items.is_empty() {
                return Err(VerifyError::new(
                    "relation literals only support the empty initializer Set {}",
                ));
            }
            let entries = model
                .domain_values(source_ty)?
                .into_iter()
                .flat_map(|source| {
                    model
                        .domain_values(target_element_ty)
                        .unwrap_or_default()
                        .into_iter()
                        .map(move |target| (source.clone(), target))
                })
                .map(|pair| Ok((pair, solver.bool_value(false))))
                .collect::<Result<_, VerifyError>>()?;
            Ok(SymbolicValue::Relation {
                ty: target_ty.clone(),
                entries,
            })
        }
        (SymbolicValue::SeqLiteral(items), TypeRef::Seq(element_ty, capacity)) => {
            let default =
                concrete_value(solver, model, element_ty, &model.default_value(element_ty)?)?;
            let slot_count = (*capacity).max(items.len());
            let mut slots = Vec::with_capacity(slot_count);
            for index in 0..slot_count {
                let item = items.get(index).cloned().unwrap_or_else(|| default.clone());
                slots.push(coerce(solver, model, item, element_ty)?);
            }
            Ok(SymbolicValue::Seq {
                ty: target_ty.clone(),
                slots,
                len: solver.int_value(i64_index(items.len())?),
            })
        }
        (value, ty)
            if value
                .inferred_collection_element(ty)
                .is_some_and(|element| solver_sort(model, element).is_ok()) =>
        {
            coerce_inferred_collection(solver, model, value, ty)
        }
        (SymbolicValue::Scalar { term, .. }, ty) if solver_sort(model, ty).is_ok() => {
            Ok(SymbolicValue::Scalar {
                ty: ty.clone(),
                term,
            })
        }
        (mut value, ty) if can_retag(&value, ty, model)? => {
            set_value_type(&mut value, ty.clone());
            Ok(value)
        }
        (value, ty) => Err(VerifyError::new(format!(
            "cannot coerce symbolic value {:?} to {ty:?}",
            value.ty()
        ))),
    }
}

fn coerce_inferred_collection<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: SymbolicValue<S::Term>,
    target: &TypeRef,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    match value {
        SymbolicValue::Collection { entries, .. } => Ok(SymbolicValue::Collection {
            ty: target.clone(),
            entries,
        }),
        SymbolicValue::Seq { slots, len, .. } => {
            let TypeRef::Seq(element, _) = target else {
                unreachable!()
            };
            Ok(SymbolicValue::Seq {
                ty: target.clone(),
                slots: slots
                    .into_iter()
                    .map(|slot| coerce(solver, model, slot, element))
                    .collect::<Result<_, _>>()?,
                len,
            })
        }
        _ => unreachable!(),
    }
}

pub(crate) fn is_collection<T>(value: &SymbolicValue<T>) -> bool {
    matches!(
        value,
        SymbolicValue::Set { .. }
            | SymbolicValue::Relation { .. }
            | SymbolicValue::Collection { .. }
    )
}

pub(crate) fn map_collections<T>(
    target: &mut SymbolicValue<T>,
    source: &SymbolicValue<T>,
    map: &mut impl FnMut(&mut SymbolicValue<T>, &SymbolicValue<T>) -> Result<(), VerifyError>,
) -> Result<(), VerifyError> {
    if is_collection(target) && is_collection(source) {
        return map(target, source);
    }
    Ok(())
}

pub(crate) fn snapshot_collections<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    target: &mut SymbolicValue<S::Term>,
    source: &SymbolicValue<S::Term>,
) -> Result<(), VerifyError> {
    map_collections(target, source, &mut |target, source| {
        let (ty, entries) = collection_entries(solver, model, source)?;
        *target = SymbolicValue::Collection { ty, entries };
        Ok(())
    })
}

/// Canonicalize only after proving that no live key escapes the frame's support.
/// Selected, already asserted bounds discharge this proof without a query.
/// Only top-level Set/relation roots are reframed; other roots stay unchanged.
pub(crate) async fn reframe_collections<S: SmtSolver>(
    solver: &mut S,
    model: &KernelModel,
    state: &mut SymbolicState<S::Term>,
    suffix: &str,
    checked_bounds: Option<&BTreeSet<String>>,
) -> Result<(), VerifyError> {
    for (name, _) in &model.state {
        let bounded = checked_bounds.is_none_or(|selected| {
            selected.contains(&fsl_core::SyntheticName::Bounds { state: name }.internal())
        });
        let source = state[name].clone();
        let mut carrier = source.clone();
        let mut index = 0;
        map_collections(&mut carrier, &source, &mut |target, value| {
            if let SymbolicValue::Collection { ty, entries } = value {
                *target = symbolic_value(
                    solver,
                    model,
                    ty,
                    &format!("{name}@carrier_{suffix}_{index}"),
                )?;
                if !bounded {
                    let (_, mut support) = collection_entries(solver, model, target)?;
                    for (key, _) in entries {
                        if let Some(known) = literal_key(solver, key)
                            && !support
                                .iter()
                                .any(|(key, _)| literal_key(solver, key).as_ref() == Some(&known))
                        {
                            let present = solver.constant(
                                &format!("{name}@support_{suffix}_{index}_{}", support.len()),
                                &Sort::Bool,
                            )?;
                            support.push((key.clone(), present));
                            *target = SymbolicValue::Collection {
                                ty: ty.clone(),
                                entries: support.clone(),
                            };
                        }
                    }
                }
                index += 1;
            }
            Ok(())
        })?;
        if index > 0 {
            if !bounded {
                let mut supported = source.clone();
                map_collections(&mut supported, &carrier, &mut |value, frame| {
                    let (ty, mut entries) = collection_entries(solver, model, value)?;
                    let (_, mut support) = collection_entries(solver, model, frame)?;
                    for (_, present) in &mut support {
                        *present = solver.bool_value(true);
                    }
                    for (key, present) in &mut entries {
                        *present = solver
                            .and(&[present.clone(), collection_contains(solver, &support, key)?])?;
                    }
                    *value = SymbolicValue::Collection { ty, entries };
                    Ok(())
                })?;
                let escapes = solver.not(&logical_equal(solver, model, &source, &supported)?)?;
                solver.set_query_context("collection_support", name);
                match solver.check_assuming(&[escapes]).await? {
                    SatResult::Sat => continue,
                    SatResult::Unsat => {}
                    SatResult::Unknown => {
                        return Err(VerifyError::solver(
                            "solver returned unknown in collection support proof",
                        ));
                    }
                }
            }
            solver.assert(&logical_equal(solver, model, &carrier, &source)?)?;
            state.insert(name.clone(), carrier);
        }
    }
    Ok(())
}

pub(crate) fn collection_types(ty: &TypeRef) -> Result<Vec<&TypeRef>, VerifyError> {
    match ty {
        TypeRef::Set(element) => Ok(vec![element]),
        TypeRef::Relation(source, target) => Ok(vec![source, target]),
        _ => Err(VerifyError::new("expected Set or relation")),
    }
}

pub(crate) fn collection_entries<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: &SymbolicValue<S::Term>,
) -> Result<(TypeRef, CollectionEntries<S::Term>), VerifyError> {
    let ty = value
        .ty()
        .ok_or_else(|| VerifyError::new("untyped collection"))?;
    let types = collection_types(ty)?;
    let scalar = |value, ty| match concrete_value(solver, model, ty, value)? {
        SymbolicValue::Scalar { term, .. } => Ok(term),
        _ => Err(VerifyError::new("collection members must be scalar")),
    };
    let entries = match value {
        SymbolicValue::Collection { entries, .. } => entries.clone(),
        SymbolicValue::Set { entries, .. } => entries
            .iter()
            .map(|(key, present)| Ok((vec![scalar(key, types[0])?], present.clone())))
            .collect::<Result<_, VerifyError>>()?,
        SymbolicValue::Relation { entries, .. } => entries
            .iter()
            .map(|((source, target), present)| {
                Ok((
                    vec![scalar(source, types[0])?, scalar(target, types[1])?],
                    present.clone(),
                ))
            })
            .collect::<Result<_, VerifyError>>()?,
        _ => return Err(VerifyError::new("expected Set or relation")),
    };
    Ok((ty.clone(), entries))
}

pub(crate) fn collection_key_equal<S: SmtSolver>(
    solver: &S,
    left: &[S::Term],
    right: &[S::Term],
) -> Result<S::Term, VerifyError> {
    if left.len() != right.len() {
        return Err(VerifyError::new("collection key arity mismatch"));
    }
    Ok(solver.and(
        &left
            .iter()
            .zip(right)
            .map(|(left, right)| solver.equal(left, right))
            .collect::<Result<Vec<_>, _>>()?,
    )?)
}

pub(crate) fn collection_contains<S: SmtSolver>(
    solver: &S,
    entries: &[(Vec<S::Term>, S::Term)],
    key: &[S::Term],
) -> Result<S::Term, VerifyError> {
    let terms = entries
        .iter()
        .map(|(candidate, present)| {
            Ok(solver.and(&[
                collection_key_equal(solver, candidate, key)?,
                present.clone(),
            ])?)
        })
        .collect::<Result<Vec<_>, VerifyError>>()?;
    Ok(solver.or(&terms)?)
}

pub(crate) fn literal_key<S: SmtSolver>(solver: &S, key: &[S::Term]) -> Option<Vec<ModelValue>> {
    key.iter().map(|term| solver.literal_value(term)).collect()
}

pub(crate) fn distinct_entries<S: SmtSolver>(
    solver: &S,
    entries: CollectionEntries<S::Term>,
) -> Result<CollectionEntries<S::Term>, VerifyError> {
    let mut distinct = Vec::new();
    for (key, present) in entries {
        let duplicate = collection_contains(solver, &distinct, &key)?;
        distinct.push((key, solver.and(&[present, solver.not(&duplicate)?])?));
    }
    Ok(distinct)
}

fn set_value_type<T>(value: &mut SymbolicValue<T>, ty: TypeRef) {
    match value {
        SymbolicValue::Scalar { ty: current, .. }
        | SymbolicValue::Option { ty: current, .. }
        | SymbolicValue::Struct { ty: current, .. }
        | SymbolicValue::Map { ty: current, .. }
        | SymbolicValue::Set { ty: current, .. }
        | SymbolicValue::Collection { ty: current, .. }
        | SymbolicValue::Seq { ty: current, .. }
        | SymbolicValue::Relation { ty: current, .. } => *current = ty,
        SymbolicValue::None | SymbolicValue::SetLiteral(_) | SymbolicValue::SeqLiteral(_) => {}
    }
}

pub(crate) fn select_finite<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    entries: &[(FslValue, SymbolicValue<S::Term>)],
    index: &SymbolicValue<S::Term>,
    key_ty: &TypeRef,
) -> Result<SymbolicValue<S::Term>, VerifyError> {
    let mut iter = entries.iter().rev();
    let (_, first) = iter
        .next()
        .ok_or_else(|| VerifyError::new("cannot select from an empty finite domain"))?;
    let mut result = first.clone();
    for (key, value) in iter {
        let key = concrete_value(solver, model, key_ty, key)?;
        let matches = logical_equal(solver, model, index, &key)?;
        result = ite_value(solver, model, &matches, value, &result)?;
    }
    Ok(result)
}

pub(crate) fn store_finite<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    entries: &[(FslValue, SymbolicValue<S::Term>)],
    index: &SymbolicValue<S::Term>,
    key_ty: &TypeRef,
    value: &SymbolicValue<S::Term>,
) -> Result<MapEntries<S::Term>, VerifyError> {
    entries
        .iter()
        .map(|(key, old)| {
            let symbolic_key = concrete_value(solver, model, key_ty, key)?;
            let matches = logical_equal(solver, model, index, &symbolic_key)?;
            Ok((key.clone(), ite_value(solver, model, &matches, value, old)?))
        })
        .collect()
}

pub(crate) fn bounds<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: &SymbolicValue<S::Term>,
) -> Result<S::Term, VerifyError> {
    match value {
        SymbolicValue::Scalar { ty, term } => scalar_bounds(solver, model, ty, term),
        SymbolicValue::Option { present, value, .. } => {
            let inner = bounds(solver, model, value)?;
            Ok(solver.implies(present, &inner)?)
        }
        SymbolicValue::Struct { fields, .. } => {
            let parts = fields
                .values()
                .map(|value| bounds(solver, model, value))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(solver.and(&parts)?)
        }
        SymbolicValue::Map { entries, .. } => {
            let parts = entries
                .iter()
                .map(|(_, value)| bounds(solver, model, value))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(solver.and(&parts)?)
        }
        SymbolicValue::Set { .. } | SymbolicValue::Relation { .. } => {
            let (ty, entries) = collection_entries(solver, model, value)?;
            bounds(solver, model, &SymbolicValue::Collection { ty, entries })
        }
        SymbolicValue::Collection { ty, entries } => {
            let types = collection_types(ty)?;
            let mut parts = Vec::new();
            for (key, present) in entries {
                let valid = key
                    .iter()
                    .zip(&types)
                    .map(|(term, ty)| scalar_bounds(solver, model, ty, term))
                    .collect::<Result<Vec<_>, _>>()?;
                parts.push(solver.implies(present, &solver.and(&valid)?)?);
            }
            Ok(solver.and(&parts)?)
        }
        SymbolicValue::Seq { ty, slots, len } => {
            let TypeRef::Seq(_, capacity) = ty else {
                unreachable!();
            };
            let zero = solver.int_value(0);
            let capacity = solver.int_value(i64_index(*capacity)?);
            let mut parts = vec![solver.ge(len, &zero)?, solver.le(len, &capacity)?];
            for (index, value) in slots.iter().enumerate() {
                let active = solver.lt(&solver.int_value(i64_index(index)?), len)?;
                let valid = bounds(solver, model, value)?;
                parts.push(solver.implies(&active, &valid)?);
            }
            Ok(solver.and(&parts)?)
        }
        SymbolicValue::None | SymbolicValue::SetLiteral(_) | SymbolicValue::SeqLiteral(_) => Err(
            VerifyError::new("untyped literal cannot appear in symbolic state"),
        ),
    }
}

fn scalar_bounds<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    ty: &TypeRef,
    term: &S::Term,
) -> Result<S::Term, VerifyError> {
    let range = match ty {
        TypeRef::Int | TypeRef::Bool => None,
        TypeRef::Range(lo, hi) => Some((*lo, *hi)),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => Some((*lo, *hi)),
            Some(TypeDef::Enum { members, .. }) => Some((
                0,
                i64_index(members.len())?
                    .checked_sub(1)
                    .ok_or_else(|| VerifyError::new("enum has no members"))?,
            )),
            Some(TypeDef::Struct { .. }) => {
                return Err(VerifyError::new("struct encoded as scalar"));
            }
            None => return Err(VerifyError::new(format!("unknown type '{name}'"))),
        },
        _ => return Err(VerifyError::new("collection encoded as scalar")),
    };
    if let Some((lo, hi)) = range {
        let lower = solver.ge(term, &solver.int_value(lo))?;
        let upper = solver.le(term, &solver.int_value(hi))?;
        Ok(solver.and(&[lower, upper])?)
    } else {
        Ok(solver.bool_value(true))
    }
}

pub(crate) fn i64_index(value: usize) -> Result<i64, VerifyError> {
    i64::try_from(value).map_err(|_| VerifyError::new("finite index exceeds i64"))
}

pub(crate) fn project_state<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
) -> Result<BTreeMap<String, FslValue>, VerifyError> {
    model
        .state
        .iter()
        .map(|(name, _)| {
            Ok((
                name.clone(),
                project_value(
                    solver,
                    model,
                    state
                        .get(name)
                        .ok_or_else(|| VerifyError::new(format!("missing state '{name}'")))?,
                )?,
            ))
        })
        .collect()
}

pub(crate) fn project_value<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    value: &SymbolicValue<S::Term>,
) -> Result<FslValue, VerifyError> {
    match value {
        SymbolicValue::Scalar { ty, term } => project_scalar(solver, model, ty, term),
        SymbolicValue::Option { present, value, .. } => {
            if project_bool(solver, present)? {
                Ok(FslValue::Some(Box::new(project_value(
                    solver, model, value,
                )?)))
            } else {
                Ok(FslValue::None)
            }
        }
        SymbolicValue::Struct { ty, fields } => {
            let TypeRef::Named(type_name) = ty else {
                return Err(VerifyError::new("struct has no named type"));
            };
            Ok(FslValue::Struct {
                type_name: type_name.clone(),
                fields: fields
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), project_value(solver, model, value)?)))
                    .collect::<Result<_, VerifyError>>()?,
            })
        }
        SymbolicValue::Map { entries, .. } => Ok(FslValue::Map(
            entries
                .iter()
                .map(|(key, value)| Ok((key.clone(), project_value(solver, model, value)?)))
                .collect::<Result<_, VerifyError>>()?,
        )),
        SymbolicValue::Set { entries, .. } => Ok(FslValue::Set(
            entries
                .iter()
                .filter_map(|(element, present)| {
                    project_bool(solver, present)
                        .map(|present| present.then(|| element.clone()))
                        .transpose()
                })
                .collect::<Result<_, VerifyError>>()?,
        )),
        SymbolicValue::Seq { slots, len, .. } => {
            let len = project_int(solver, len)?;
            // Not a solver failure: an unconstrained `Seq` length (for example
            // its init assignment removed) can legitimately be negative or past
            // the capacity in a type-bound counterexample, the same shape as
            // the enum ordinal case (#1283). Until that projection keeps the raw
            // length (a follow-up), it stays a semantic error, so `fslc mutate`
            // counts it as the `build_spec` kill rather than an oracle error.
            let len = usize::try_from(len)
                .map_err(|_| VerifyError::new("model sequence length is negative"))?;
            if len > slots.len() {
                return Err(VerifyError::new("model sequence length exceeds capacity"));
            }
            Ok(FslValue::Seq(
                slots[..len]
                    .iter()
                    .map(|value| project_value(solver, model, value))
                    .collect::<Result<_, VerifyError>>()?,
            ))
        }
        SymbolicValue::Relation { entries, .. } => Ok(FslValue::Relation(
            entries
                .iter()
                .filter_map(|(pair, present)| {
                    project_bool(solver, present)
                        .map(|present| present.then(|| pair.clone()))
                        .transpose()
                })
                .collect::<Result<_, VerifyError>>()?,
        )),
        SymbolicValue::Collection { ty, entries } => {
            let types = collection_types(ty)?;
            let mut values = Vec::new();
            for (key, present) in entries {
                if project_bool(solver, present)? {
                    values.push(
                        key.iter()
                            .zip(&types)
                            .map(|(term, ty)| project_scalar(solver, model, ty, term))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                }
            }
            Ok(match ty {
                TypeRef::Set(_) => {
                    FslValue::Set(values.into_iter().map(|key| key[0].clone()).collect())
                }
                TypeRef::Relation(_, _) => FslValue::Relation(
                    values
                        .into_iter()
                        .map(|key| (key[0].clone(), key[1].clone()))
                        .collect(),
                ),
                _ => unreachable!(),
            })
        }
        SymbolicValue::None | SymbolicValue::SetLiteral(_) | SymbolicValue::SeqLiteral(_) => Err(
            VerifyError::new("untyped literal cannot be projected from a solver model"),
        ),
    }
}

fn project_scalar<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    ty: &TypeRef,
    term: &S::Term,
) -> Result<FslValue, VerifyError> {
    if matches!(ty, TypeRef::Bool) {
        return Ok(FslValue::Bool(project_bool(solver, term)?));
    }
    let value = project_int(solver, term)?;
    if let TypeRef::Named(name) = ty
        && let Some(TypeDef::Enum { members, .. }) = model.types.get(name)
    {
        // A type-bound counterexample deliberately assigns an invalid
        // ordinal. Preserve that raw value so the verifier can emit the
        // witness instead of turning a valid finding into a projection
        // error. The out-of-range side is two-sided: an unconstrained enum
        // state (e.g. its init assignment removed) lets the solver pick any
        // integer, negative ones included, and `_bounds_<state>` is exactly
        // the property that reports it. Negative ordinals are therefore kept
        // as raw values like ordinals past the last member, rather than
        // constraining the solver to non-negative ordinals, which would hide
        // part of the type-bound violation space (#1283).
        let Some(member) = usize::try_from(value)
            .ok()
            .and_then(|index| members.get(index))
        else {
            return Ok(FslValue::Int(value));
        };
        return Ok(FslValue::Enum {
            type_name: name.clone(),
            member: member.clone(),
        });
    }
    Ok(FslValue::Int(value))
}

fn project_bool<S: SmtSolver>(solver: &S, term: &S::Term) -> Result<bool, VerifyError> {
    match solver.model_eval(term)? {
        Some(ModelValue::Bool(value)) => Ok(value),
        Some(ModelValue::Int(_)) => Err(VerifyError::solver(
            "solver projected integer for Boolean term",
        )),
        None => Err(VerifyError::solver("Boolean model value is unavailable")),
    }
}

fn project_int<S: SmtSolver>(solver: &S, term: &S::Term) -> Result<i64, VerifyError> {
    match solver.model_eval(term)? {
        Some(ModelValue::Int(value)) => Ok(value),
        Some(ModelValue::Bool(_)) => Err(VerifyError::solver(
            "solver projected Boolean for integer term",
        )),
        None => Err(VerifyError::solver("integer model value is unavailable")),
    }
}
