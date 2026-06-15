# Invalid Spec Catalog

This directory holds minimal examples for each diagnostic kind. Below are excerpts
of JSON output actually confirmed with `fslc`.

```json
// parse_missing_expression.fsl
{"result":"error","kind":"parse","expected":"one of: ABS, COUNT, EXISTS, ..."}
```

```json
// type_option_some_equality.fsl
{"result":"error","kind":"type","message":"Option == and != are only defined against none","hint":"use `x is some(v)` to compare the contained value"}
```

```json
// type_undeclared_type.fsl
{"result":"error","kind":"type","message":"unknown type 'UserId'"}
```

```json
// type_struct_set_field.fsl
{"result":"error","kind":"type","message":"struct field 'Bag.members' has non-scalar type"}
```

```json
// semantics_duplicate_assignment.fsl
{"result":"error","kind":"semantics","message":"double assignment to 'x' on the same execution path"}
```

```json
// vacuous_contradictory_init.fsl
{"result":"error","kind":"vacuous","message":"init constraints are unsatisfiable — the spec has no initial state"}
```

```json
// violated_invariant_counter.fsl
{"result":"violated","violation_kind":"invariant","invariant":"StaysZero"}
```

```json
// violated_type_bound_missing_guard.fsl
{"result":"violated","violation_kind":"type_bound","invariant":"_bounds_stock"}
```

```json
// violated_ensures_wrong_postcondition.fsl
{"result":"violated","violation_kind":"ensures","invariant":"inc"}
```

```json
// violated_partial_op_unchecked_pop.fsl
{"result":"violated","violation_kind":"partial_op","invariant":"_partial_pop_empty","hint":"guard the action with requires q.size() > 0 (or bound the index)"}
```

```json
// violated_leads_to_starvation.fsl
{"result":"violated","violation_kind":"leadsTo","invariant":"EventuallyFinished","hint":"P held at step 2 but the loop from step 2 can repeat forever without Q; if progress relies on some action being taken eventually, annotate it with `fair action ...`"}
```

```json
// violated_deadlock_terminal.fsl
{"result":"violated","violation_kind":"deadlock","invariant":"deadlock"}
```

```json
// refinement_failed_map.fsl
{"result":"refines","checked_to_depth":3,"impl":"GalleryRefinementImpl","abs":"GalleryRefinementAbs","action_map":{"approve_i":"approve","quick_pay_i":"pay"}}
```

`refinement_failed_map.fsl` is correctly expected to give
`refinement_failed/abs_requires_failed`. The diff from the actual output is
`BUG-001` in `docs/DOGFOOD-6.md`.

```json
// error_acceptance_false_expect.fsl
{"result":"error","kind":"acceptance","id":"AC-1","failed_step":1,"state":{"st":"Done"}}
```
