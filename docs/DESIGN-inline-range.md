# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Ryoichi Izumita

# Design: Inline Anonymous Range Types (`x: lo..hi`)

## Problem

Writing `state { x: 0..3 }` required a two-step dance: declare a named domain type
(`type R = 0..3`), then reference it (`state { x: R }`). The anonymous form was not
accepted by the grammar.

## Solution

Pure syntactic sugar. The `?type` grammar rule gains one alternative:

```
| expr ".." expr -> t_range
```

The AST transformer produces `("range", lo_expr, hi_expr)`. The two `resolve_type` /
`resolve_type_ref` functions in `model.py` handle the `"range"` tag by evaluating both
bounds with `eval_const` (same path as `collect_types` uses for named domain types) and
returning `("domain", lo_i, hi_i)`.

No new kernel concept is introduced. Everything downstream — `expand_phys_var`, Z3 sort
construction, bounds invariants — already handles `("domain", lo, hi)`.

## Scope

State variable declarations only. Action parameter ranges (`action a(x in 0..3)`) already
had their own syntax via `param_range` and are unchanged.
